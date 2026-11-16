//! Rasterizes the segmental timeline into 5 ms control frames: voice-source levels,
//! formant targets, the intonation contour and articulator positions, then shapes the
//! formant transitions between neighbouring sounds.

use crate::config::{Gender, VoiceConfig};
use crate::phone::Ph;
use crate::tables;
use crate::timeline::{clamp, ClauseMeta, Seg, Shape, Timeline};
use crate::tune;

pub const FRAME_MS: f64 = 5.0;
const FRAME_SEC: f64 = FRAME_MS / 1000.0;
const PI: f64 = std::f64::consts::PI;

/// One 5 ms control frame.
#[derive(Clone, Debug)]
pub struct Frame {
    pub t_sec: f64,
    pub clause_idx: i32,
    pub micro_f0: f64,
    pub f0: f64,
    pub f1: f64,
    pub f2: f64,
    pub f3: f64,
    pub b1: f64,
    pub b2: f64,
    pub b3: f64,
    /// Voicing amplitude.
    pub av: f64,
    /// Aspiration (breath noise at the glottis).
    pub ah: f64,
    /// Frication (noise at the mouth constriction).
    pub af: f64,
    /// Voice bar during voiced closures.
    pub avb: f64,
    pub nasal: f64,
    pub oral_seal: f64,
    pub fnz: f64,
    pub lax: f64,
    /// Creak: 0 = regular pulses, 1 = every other pulse weak and late (the irregular voice
    /// that real speakers fall into at the end of a phrase).
    pub creak: f64,
    pub fric_f1: f64,
    pub fric_b1: f64,
    pub fric_f2: f64,
    pub fric_b2: f64,
    pub fric_g1: f64,
    pub fric_g2: f64,
    /// Low cut-off of the frication noise, Hz.
    pub fric_hp: f64,
    pub is_closure: bool,
    pub is_burst: bool,
    pub stress: u8,
    // Articulators (0..1), for lip-sync / mouth animation.
    pub jaw: f64,
    pub t_h: f64,
    pub t_f: f64,
    pub t_tip: f64,
    pub lip_r: f64,
    pub lip_s: f64,
    pub lip_c: f64,
    pub velum: f64,
    pub phone: Ph,
    pub word: i32,
    /// Estimated loudness before rendering, measured loudness after.
    pub rms: f64,
}

struct Accent {
    t_peak: f64,
    amp: f64,
    sigma: f64,
}

struct ClauseSpan {
    start_sec: f64,
    end_sec: f64,
    accents: Vec<Accent>,
    meta: ClauseMeta,
}

/// Articulatory class of a segment; decides how its formants move into its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Class {
    Pause,
    /// Silent (or voice-bar) closure of a stop: formants sit at the consonant's locus.
    Closure,
    /// Burst / aspiration after a stop closure.
    Release,
    Vowel,
    Glide,
    Liquid,
    Nasal,
    Fricative,
    Aspirate,
}

fn class_of(seg: &Seg) -> Class {
    let ph = seg.phone;
    if ph == Ph::SIL {
        Class::Pause
    } else if tables::stop(ph).is_some() {
        if seg.is_closure {
            Class::Closure
        } else {
            Class::Release
        }
    } else if ph.is_vowel() {
        Class::Vowel
    } else {
        match ph {
            Ph::W | Ph::Y => Class::Glide,
            Ph::L | Ph::R => Class::Liquid,
            Ph::M | Ph::N | Ph::NG => Class::Nasal,
            Ph::HH => Class::Aspirate,
            _ => Class::Fricative,
        }
    }
}

/// How the formants cross the boundary between segment classes `a` and `b`:
/// (ms of transition inside `a`, ms inside `b`, weight of `b`'s target at the boundary).
///
/// Consonants dominate the boundary value (their constriction sets the resonances), the
/// audible movement happens inside the neighbouring vowel, and glides move slowly.
fn transition_rule(a: Class, b: Class) -> (f64, f64, f64) {
    use Class::*;
    match (a, b) {
        (Pause, _) | (_, Pause) => (0.0, 0.0, 0.5),
        (Vowel, Closure) => (50.0, 10.0, 0.78),
        (Closure, Release) => (0.0, 5.0, 0.0),
        (Release, Vowel) => (5.0, 40.0, 0.35),
        (Closure, Vowel) => (0.0, 45.0, 0.15),
        (Vowel, Vowel) => (35.0, 35.0, 0.5),
        (Vowel, Glide) => (55.0, 30.0, 0.62),
        (Glide, Vowel) => (30.0, 60.0, 0.38),
        (Vowel, Liquid) => (45.0, 25.0, 0.58),
        (Liquid, Vowel) => (25.0, 50.0, 0.42),
        (Vowel, Nasal) => (45.0, 10.0, 0.72),
        (Nasal, Vowel) => (10.0, 45.0, 0.28),
        (Vowel, Fricative) => (40.0, 15.0, 0.66),
        (Fricative, Vowel) => (15.0, 40.0, 0.34),
        (Vowel, Aspirate) | (Aspirate, Vowel) => (15.0, 15.0, 0.5),
        (Release, Liquid) | (Release, Glide) => (5.0, 35.0, 0.35),
        (Closure, _) => (0.0, 20.0, 0.2),
        (_, Closure) => (25.0, 10.0, 0.75),
        (Vowel, Release) | (Release, _) => (10.0, 15.0, 0.5),
        _ => (15.0, 15.0, 0.5),
    }
}

fn smoothstep(t: f64) -> f64 {
    let t = clamp(t, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Replaces the stepwise formant targets with class-specific transitions around every
/// segment boundary.
fn shape_transitions(
    frames: &mut [Frame],
    segs: &[Seg],
    bounds: &[(usize, usize)],
    scale: f64,
    formant_scale: [f64; 3],
    undershoot_ms: f64,
) {
    let raw: Vec<[f64; 3]> = frames.iter().map(|f| [f.f1, f.f2, f.f3]).collect();
    let n_seg = bounds.len();
    let loci: Vec<Option<[f64; 3]>> = segs[..n_seg]
        .iter()
        .map(|s| {
            s.locus
                .map(|l| [l[0] * formant_scale[0], l[1] * formant_scale[1], l[2] * formant_scale[2]])
        })
        .collect();
    let classes: Vec<Class> = segs[..n_seg].iter().map(class_of).collect();
    let mut left: Vec<Option<(usize, [f64; 3])>> = vec![None; n_seg];
    let mut right: Vec<Option<(usize, [f64; 3])>> = vec![None; n_seg];

    for k in 0..n_seg.saturating_sub(1) {
        let (ms_a, ms_b, w) = transition_rule(classes[k], classes[k + 1]);
        if ms_a == 0.0 && ms_b == 0.0 {
            continue;
        }
        // Nasals pull their neighbours toward the closure's locus but keep their own
        // murmur resonances, so the spectrum switches abruptly at the closure, as in speech.
        let ta = loci[k].unwrap_or(raw[bounds[k].1 - 1]);
        let tb = loci[k + 1].unwrap_or(raw[bounds[k + 1].0]);
        let vb = [
            ta[0] + (tb[0] - ta[0]) * w,
            ta[1] + (tb[1] - ta[1]) * w,
            ta[2] + (tb[2] - ta[2]) * w,
        ];
        let to_frames = |ms: f64| ((ms * scale / FRAME_MS) + 0.5).floor() as usize;
        if loci[k].is_none() {
            right[k] = Some((to_frames(ms_a), vb));
        }
        if loci[k + 1].is_none() {
            left[k + 1] = Some((to_frames(ms_b), vb));
        }
    }

    // Undershoot: a vowel that is over quickly never gets all the way to its target - the
    // tongue is still on its way from the previous sound and already leaving for the next.
    let mut raw = raw;
    if undershoot_ms > 0.0 {
        for k in 0..n_seg {
            if classes[k] != Class::Vowel {
                continue;
            }
            let (s, e) = bounds[k];
            let (Some((_, vl)), Some((_, vr))) = (left[k], right[k]) else {
                continue;
            };
            let pull = (-((e - s) as f64 * FRAME_MS) / undershoot_ms).exp();
            for frame in raw.iter_mut().take(e).skip(s) {
                for j in 0..3 {
                    let context = 0.5 * (vl[j] + vr[j]);
                    frame[j] += (context - frame[j]) * pull;
                }
            }
        }
    }

    for k in 0..n_seg {
        let (s, e) = bounds[k];
        let len = e - s;
        let mut n_l = left[k].map_or(0, |x| x.0);
        let mut n_r = right[k].map_or(0, |x| x.0);
        if n_l + n_r > len && n_l + n_r > 0 {
            // Short segment: both transitions shrink and the target is only touched (undershoot).
            let total = (n_l + n_r) as f64;
            n_l = ((n_l as f64 / total) * len as f64).round() as usize;
            n_r = len - n_l;
        }
        for i in s..e {
            let mut v = raw[i];
            if let Some((_, vb)) = left[k] {
                if i - s < n_l {
                    let t = smoothstep(((i - s) as f64 + 0.5) / n_l as f64);
                    for j in 0..3 {
                        v[j] = vb[j] + (raw[i][j] - vb[j]) * t;
                    }
                }
            }
            if let Some((_, vb)) = right[k] {
                let start = e - n_r;
                if i >= start && n_r > 0 {
                    let t = smoothstep(((i - start) as f64 + 0.5) / n_r as f64);
                    for j in 0..3 {
                        v[j] += (vb[j] - v[j]) * t;
                    }
                }
            }
            frames[i].f1 = v[0];
            frames[i].f2 = v[1];
            frames[i].f3 = v[2];
        }
    }

    // Round the corners (zero-phase, ~6 ms).
    let a = 0.55;
    let n = frames.len();
    for i in 1..n {
        let (p1, p2, p3) = (frames[i - 1].f1, frames[i - 1].f2, frames[i - 1].f3);
        let c = &mut frames[i];
        c.f1 = p1 + (c.f1 - p1) * a;
        c.f2 = p2 + (c.f2 - p2) * a;
        c.f3 = p3 + (c.f3 - p3) * a;
    }
    for i in (0..n.saturating_sub(1)).rev() {
        let (n1, n2, n3) = (frames[i + 1].f1, frames[i + 1].f2, frames[i + 1].f3);
        let c = &mut frames[i];
        c.f1 = n1 + (c.f1 - n1) * a;
        c.f2 = n2 + (c.f2 - n2) * a;
        c.f3 = n3 + (c.f3 - n3) * a;
    }
}

pub fn rasterize(timeline: &Timeline, cfg: &VoiceConfig) -> Vec<Frame> {
    let female = cfg.gender == Gender::Female;
    let h_noise = tune("h_noise", 1.0);
    // How breathy the very end of a phrase gets.
    let tail_lax = tune("tail_lax", 0.42);
    // Creak at the end of a phrase (development hooks: how strong, and from which point of
    // the last vowel).
    let tail_creak = tune("tail_creak", if female { 0.52 } else { 0.6 });
    let creak_from = tune("creak_from", 0.6);
    let base_f0 = cfg.pitch_hz;
    let tract_cm = clamp(cfg.tract_length_cm, 12.2, 21.5);
    let formant_scale = 17.5 / tract_cm;
    let f1_scale = (1.0 + (formant_scale - 1.0) * if female { 0.88 } else { 0.75 }) * cfg.formants[0];
    let f2_scale = formant_scale * cfg.formants[1];
    let f3_scale = (1.0 + (formant_scale - 1.0) * 0.90) * cfg.formants[2];

    let breathiness = clamp(cfg.breathiness, 0.0, 0.85);
    let base_nasality = clamp(cfg.nasality, 0.0, 0.80);
    let crispness = clamp(cfg.crispness, 0.45, 1.6);
    let expressiveness = clamp(cfg.expressiveness, 0.0, 3.0);
    let whisper = cfg.whisper;
    let female_glottal_damp = if female { 36.0 } else { 0.0 };
    let female_b2_damp = if female { 18.0 } else { 0.0 };
    let female_b3_damp = if female { 10.8 } else { 0.0 };
    let b1_wall = if female { 0.0 } else { tune("b1_wall_m", 14.0) };

    let acc_peak = tune("acc_peak", 0.30);
    let dip_hold = tune("dip_hold", 0.22);
    let dip_reach = tune("dip_reach", 0.94);

    // Pass 1: frames + per-clause timing and accent centres.
    let mut clause_spans: Vec<ClauseSpan> = timeline
        .clauses
        .iter()
        .map(|m| ClauseSpan {
            start_sec: -1.0,
            end_sec: 0.0,
            accents: Vec::new(),
            meta: *m,
        })
        .collect();

    let mut frames: Vec<Frame> = Vec::new();
    let mut bounds: Vec<(usize, usize)> = Vec::with_capacity(timeline.segments.len());
    let mut seg_cursor = 0.0f64;

    for seg in &timeline.segments {
        let s = &seg.s;
        let n_f = ((s.dur / FRAME_MS + 0.5).floor() as i64).max(1) as usize;
        let seg_dur_sec = n_f as f64 * FRAME_SEC;
        let c_idx = seg.clause_idx;
        let first_frame = frames.len();

        if c_idx >= 0 && (c_idx as usize) < clause_spans.len() {
            let cs = &mut clause_spans[c_idx as usize];
            if cs.start_sec < 0.0 {
                cs.start_sec = seg_cursor;
            }
            cs.end_sec = seg_cursor + seg_dur_sec;
            if seg.accent_boost.abs() > 0.004 {
                // The pitch peak sits early in the vowel, so the voice falls through the rest
                // of the syllable as it does in real accents.
                let peak_at = if seg.glide.is_some() { acc_peak * 0.8 } else { acc_peak };
                cs.accents.push(Accent {
                    t_peak: seg_cursor + seg_dur_sec * peak_at,
                    amp: seg.accent_boost,
                    sigma: (seg_dur_sec * 0.72).clamp(0.068, 0.105),
                });
            }
        }

        for fi in 0..n_f {
            let u = if n_f > 1 {
                fi as f64 / (n_f - 1) as f64
            } else {
                0.5
            };
            // Diphthongs: hold the nucleus briefly, then glide toward (not quite onto) the offglide.
            let g = match seg.glide {
                Some(_) => smoothstep((u - dip_hold) / (1.0 - dip_hold)) * dip_reach,
                None => 0.0,
            };
            let mix = |a: f64, pick: fn(&tables::Spec) -> f64| match &seg.glide {
                Some(end) => a + (pick(end) - a) * g,
                None => a,
            };
            let f1 = clamp(mix(s.f1, |e| e.f1) * f1_scale, 200.0, 1180.0);
            let f2 = clamp(mix(s.f2, |e| e.f2) * f2_scale, 540.0, 3250.0);
            let f3 = clamp(mix(s.f3, |e| e.f3) * f3_scale, 1380.0, 4150.0);

            let open_damp = ((f1 - 460.0) * 0.07).max(0.0);
            let prox_damp = ((460.0 - (f2 - f1)) * 0.12).max(0.0);
            let wall_damp = if b1_wall > 0.0 { b1_wall * (450.0 / f1).powi(2) } else { 0.0 };
            let mut devoice_damp = 0.0;

            let mut seg_av = s.av;
            let mut seg_ah = s.ah;
            let mut seg_af = s.af;
            let mut lax_extra = 0.0;
            let mut creak = 0.0;

            if seg.onset_devoice {
                // Aspirated, devoiced sonorant onset after a voiceless stop.
                let v_ramp = clamp((u - 0.22) / 0.48, 0.0, 1.0);
                seg_av = s.av * (0.08 + 0.92 * v_ramp);
                seg_ah = s.ah.max((1.0 - v_ramp) * 0.26 * crispness);
                lax_extra = (1.0 - v_ramp) * 0.50;
                devoice_damp = (1.0 - v_ramp) * 150.0;
            } else if seg.phone == Ph::HH {
                let h_env = (PI * clamp(u * 0.90 + 0.05, 0.0, 1.0)).sin();
                seg_ah = s.ah * h_env * h_noise;
                // Between voiced sounds an /h/ is a breathy stretch of voice, not a gap in it.
                let voiced_level = s.av + (0.8 - s.av) * seg.breathy_h;
                seg_av = voiced_level * (seg.breathy_h + (1.0 - seg.breathy_h) * u * u);
                lax_extra = 0.42;
            } else if seg.shape == Shape::Vowel {
                let arch = 0.92 + 0.10 * (PI * clamp(u * 0.92 + 0.05, 0.0, 1.0)).sin();
                seg_av *= arch * (1.0 - 0.05 * g);
                // Rising questions and comma continuations keep regular voicing rather than
                // dropping into phrase-final creak.
                let non_falling_tail = c_idx >= 0
                    && (c_idx as usize) < clause_spans.len()
                    && (clause_spans[c_idx as usize].meta.is_yes_no_question
                        || matches!(clause_spans[c_idx as usize].meta.punc, ',' | ';' | ':'));
                if seg.is_phrase_tail && u > 0.55 {
                    let tail_frac = (u - 0.55) / 0.45;
                    if non_falling_tail {
                        seg_av *= 1.0 - tail_frac * 0.10;
                        lax_extra = tail_frac * tail_lax * 0.35;
                    } else {
                        seg_av *= 1.0 - tail_frac * 0.24;
                        lax_extra = tail_frac * tail_lax;
                    }
                }
                if seg.is_phrase_tail && u > creak_from && !non_falling_tail {
                    creak = tail_creak * clamp((u - creak_from) / 0.15, 0.0, 1.0);
                }
            } else if seg.shape == Shape::Fric {
                let f_env = 0.48 + 0.62 * (PI * u).sin();
                seg_af *= f_env;
                if seg.voiced_part < 1.0 {
                    // the voice dies away over 30% of the sound around the given point
                    seg_av *= clamp((seg.voiced_part + 0.15 - u) / 0.30, 0.0, 1.0);
                }
            } else if seg.phone == Ph::SIL && s.ah > 0.01 {
                seg_ah = s.ah * (PI * u).sin();
            }

            let mut seg_nasal = s.nasal;
            let mut seg_velum = s.velum;
            if seg_av > 0.4 && seg_nasal < base_nasality {
                seg_nasal = base_nasality;
                seg_velum = seg_velum.max(base_nasality * 0.85);
            }

            let micro_decay = seg.micro_f0 * (-u * 2.6).exp();
            let est_energy = clamp(
                seg_av * 0.88 + seg_af * 0.58 + seg_ah * 0.42 + s.avb * 0.35,
                0.0,
                1.0,
            );

            frames.push(Frame {
                t_sec: seg_cursor,
                clause_idx: c_idx,
                micro_f0: micro_decay + seg.intrinsic_f0,
                f0: base_f0,
                f1,
                f2,
                f3,
                b1: mix(s.b1, |e| e.b1) * (1.0 + breathiness * 0.20)
                    + wall_damp
                    + open_damp
                    + prox_damp
                    + female_glottal_damp
                    + devoice_damp,
                b2: mix(s.b2, |e| e.b2) * (1.0 + breathiness * 0.16) + prox_damp * 0.75 + female_b2_damp,
                b3: mix(s.b3, |e| e.b3) * (1.0 + breathiness * 0.12) + female_b3_damp,
                av: if whisper { 0.0 } else { seg_av },
                ah: if whisper {
                    seg_ah.max(seg_av * 0.65)
                } else {
                    seg_ah + seg_av * breathiness * 0.16
                },
                af: seg_af,
                avb: if whisper { 0.0 } else { s.avb },
                nasal: if whisper { 0.0 } else { seg_nasal },
                oral_seal: s.oral_seal,
                fnz: s.fnz * f1_scale,
                lax: lax_extra + if seg.stress == 0 { 0.14 } else { 0.0 },
                creak,
                fric_f1: clamp(s.fric_f1 * (1.0 + (formant_scale - 1.0) * 0.65), 600.0, 9500.0),
                fric_b1: s.fric_b1 * formant_scale,
                fric_f2: clamp(s.fric_f2 * (1.0 + (formant_scale - 1.0) * 0.60), 1200.0, 10200.0),
                fric_b2: s.fric_b2 * formant_scale,
                fric_g1: s.fric_g1,
                fric_g2: s.fric_g2,
                fric_hp: s.fric_hp * (1.0 + (formant_scale - 1.0) * 0.60),
                is_closure: seg.is_closure,
                is_burst: seg.shape == Shape::Burst,
                stress: seg.stress,
                jaw: mix(s.jaw, |e| e.jaw),
                t_h: mix(s.t_h, |e| e.t_h),
                t_f: mix(s.t_f, |e| e.t_f),
                t_tip: mix(s.t_tip, |e| e.t_tip),
                lip_r: mix(s.lip_r, |e| e.lip_r),
                lip_s: mix(s.lip_s, |e| e.lip_s),
                lip_c: s.lip_c,
                velum: seg_velum,
                phone: seg.phone,
                word: seg.word,
                rms: est_energy,
            });
            seg_cursor += FRAME_SEC;
        }
        bounds.push((first_frame, frames.len()));
    }

    let n = frames.len();
    if n == 0 {
        return frames;
    }

    // per clause: the first accent still in reach, the widest accent, whether they are in time order
    let mut accent_walk: Vec<(usize, f64, bool)> = clause_spans
        .iter()
        .map(|cs| {
            let widest = cs.accents.iter().map(|a| a.sigma).fold(0.0, f64::max);
            (0, widest, cs.accents.windows(2).all(|w| w[0].t_peak <= w[1].t_peak))
        })
        .collect();

    // Pass 2: phrase contour + Gaussian syllable accents, mean-centred per clause so the
    // voice stays anchored on the speaker's pitch instead of drifting upward.
    // The shapes below are set from the average pitch contours of two lively reference
    // voices, sentence type by sentence type (tools/fit_style.py --shape): a sentence starts
    // three to four semitones high and comes down within its first third, runs level through
    // its middle and falls at the end; a clause after a comma starts a little high again; a
    // yes/no question jumps on its last word only; exclamations and wh-questions carry their
    // weight early. (Earlier the rules were fitted by least squares to flat read speech,
    // which shrinks every movement whose place cannot be predicted: half the pitch spread
    // of a real voice, and the monotone that goes with it.)
    let t_first = tune("f0_first", 0.058);
    let t_comma = tune("comma_tone", -0.08);
    let t_decl = tune("decl", 0.0);
    let t_fall = tune("fall", if female { 1.0 } else { 1.35 });
    let t_rise = tune("rise", if female { 1.58 } else { 1.8 });
    let t_on_first = tune("on_first", 0.42);
    let t_on_reset = tune("on_reset", 0.14);
    let t_on_tau = tune("on_tau", 0.5);
    let t_fall_from = tune("fall_from", 0.85);
    let t_q_from = tune("q_from", 0.83);
    let t_wh_fall = tune("wh_fall", 0.14);
    let t_excl_lift = tune("excl_lift", 0.22);
    let t_excl_fall = tune("excl_fall", 0.45);
    let t_decl_shape = tune("decl_shape", 0.63);
    let mut voiced_end = vec![0.0f64; clause_spans.len()];
    for fr in frames.iter() {
        if fr.clause_idx >= 0 && (fr.clause_idx as usize) < clause_spans.len() && fr.av > 0.35 {
            voiced_end[fr.clause_idx as usize] = fr.t_sec;
        }
    }
    let mut raw_log_f0 = vec![0.0f32; n];
    let mut phrase_pos = vec![0.0f32; n];
    let mut clause_log_sum = vec![0.0f64; clause_spans.len()];
    let mut clause_voiced_cnt = vec![0u32; clause_spans.len()];

    for i in 0..n {
        let fr = &frames[i];
        let ci = fr.clause_idx;
        if ci < 0 || ci as usize >= clause_spans.len() {
            continue;
        }
        let cs = &clause_spans[ci as usize];
        if cs.end_sec <= cs.start_sec {
            continue;
        }
        let meta = &cs.meta;
        let dur = (cs.end_sec - cs.start_sec).max(0.18);
        let t_rel = clamp(fr.t_sec - cs.start_sec, 0.0, dur);
        let pos_frac = t_rel / dur;
        // position for the boundary tones: 1 where the voice ends
        let voiced_dur = (voiced_end[ci as usize] - cs.start_sec).max(0.18).min(dur);
        let pos_b = (t_rel / voiced_dur).min(1.0);

        // Phrase onset arch + declination (longer phrases drift further down in total)
        let sentence_start = ci == 0 || matches!(clause_spans[ci as usize - 1].meta.punc, '.' | '?' | '!');
        let mut on_amp = if sentence_start { t_on_first } else { t_on_reset };
        if meta.is_yes_no_question {
            on_amp *= 0.25;
        } else if meta.is_exclaim {
            on_amp *= 0.45;
        }
        let onset_rise_tau = if meta.is_exclaim { 0.11 } else { 0.05 };
        let onset_arch = if t_on_tau > 0.0 {
            let tau = clamp(t_on_tau * dur, 0.3, 1.2);
            on_amp * (1.0 - (-t_rel / onset_rise_tau).exp()) * (-(t_rel / tau) * (t_rel / tau)).exp()
        } else {
            on_amp * (1.0 - (-t_rel / 0.090).exp()) * (-t_rel / (dur * 0.75).max(0.45)).exp()
        };
        let first_boost = if meta.is_first && !(meta.is_yes_no_question || meta.is_exclaim) {
            t_first
        } else {
            0.0
        };
        // Declination: gentle at first, steeper toward the end; longer phrases fall further.
        let decline = clamp(0.12 + 0.054 * dur, 0.144, 0.288) * t_decl;
        let drift = (1.0 - t_decl_shape) * pos_frac + t_decl_shape * pos_frac * pos_frac;
        let mut phrase_log = (first_boost + onset_arch - decline * drift) * expressiveness;

        // Boundary tones
        if meta.is_yes_no_question {
            // yes/no question (ToBI L* H-H%): dip into the nuclear syllable, then rise cleanly
            if pos_b > 0.58 && pos_b <= 0.88 {
                let dip = ((pos_b - 0.58) / 0.30 * PI).sin();
                phrase_log -= 0.13 * dip * expressiveness;
            }
            if pos_b > t_q_from {
                let q = (pos_b - t_q_from) / (1.0 - t_q_from);
                phrase_log += 0.34 * t_rise * q.powf(1.25) * expressiveness;
            }
        } else if meta.is_question {
            // wh-question: early peak on the wh-word, then a gentle fall (less deep than a statement)
            if pos_b > 0.72 {
                let qw = (pos_b - 0.72) / 0.28;
                phrase_log -= t_wh_fall * t_fall * qw * qw * expressiveness;
            }
        } else if meta.is_exclaim {
            let lead = (1.0 - (-t_rel / 0.12).exp()) * (1.0 - 0.65 * pos_frac);
            phrase_log += t_excl_lift * lead * expressiveness;
            if pos_b > 0.75 {
                let sf = (pos_b - 0.75) / 0.25;
                phrase_log -= t_excl_fall * t_fall * sf * sf * expressiveness;
            }
        } else if matches!(meta.punc, ',' | ';' | ':') {
            // continuation: slight pre-boundary dip, then a gentle continuation rise at commas
            if meta.punc == ',' {
                if pos_b > 0.55 && pos_b <= 0.84 {
                    let dip = ((pos_b - 0.55) / 0.29 * PI).sin();
                    phrase_log -= 0.06 * dip * expressiveness;
                }
                if pos_b > 0.80 {
                    let cr = (pos_b - 0.80) / 0.20;
                    phrase_log += 0.065 * cr * cr * expressiveness;
                }
            } else if pos_b > 0.72 {
                let cr = (pos_b - 0.72) / 0.28;
                phrase_log += t_comma * cr * cr * expressiveness;
            }
        } else if pos_b > t_fall_from {
            // statement: fall to the bottom of the speaker's range on the last syllable
            let sf = (pos_b - t_fall_from) / (1.0 - t_fall_from);
            let depth = if meta.is_last { 0.24 } else { 0.17 };
            phrase_log -= depth * t_fall * sf.powf(1.5) * expressiveness;
        }

        // Only the accents within reach of this frame are visited (a clause of thousands of
        // words would otherwise cost a second): those wholly behind it are dropped for good,
        // and the walk stops at the first one too far ahead for even the widest.
        let (first, widest, in_order) = &mut accent_walk[ci as usize];
        while *first < cs.accents.len() && (fr.t_sec - cs.accents[*first].t_peak) / cs.accents[*first].sigma >= 2.8 {
            *first += 1;
        }
        let mut acc_log = 0.0;
        for ac in &cs.accents[*first..] {
            let dz = (fr.t_sec - ac.t_peak) / ac.sigma;
            if dz > -2.8 && dz < 2.8 {
                acc_log += ac.amp * (-0.5 * dz * dz).exp();
            } else if *in_order && (fr.t_sec - ac.t_peak) / *widest <= -2.8 {
                break;
            }
        }

        let tot_log = phrase_log + acc_log + fr.micro_f0;
        raw_log_f0[i] = tot_log as f32;
        phrase_pos[i] = pos_frac as f32;
        if fr.av > 0.35 && !(meta.is_yes_no_question && pos_b > 0.83) {
            clause_log_sum[ci as usize] += tot_log;
            clause_voiced_cnt[ci as usize] += 1;
        }
    }

    // Loudness declination: like pitch, vocal effort drifts down through a phrase (real
    // speakers lose about 4 dB from the first vowel to the last).
    let fade_db = tune("fade_db", 2.0);
    for i in 0..n {
        let g = 10f64.powf(-fade_db * phrase_pos[i] as f64 / 20.0);
        frames[i].av *= g;
    }

    // Register: a lively speaker says a question or an exclamation in a higher part of the
    // voice than a statement - the whole sentence, not only its last clause. The reference
    // voices sit 2-4 semitones higher in yes/no questions, 3.5 in wh-questions and 3-5.5 in
    // exclamations (tools/style.py).
    let st = std::f64::consts::LN_2 / 12.0;
    let (reg_yn, reg_wh, reg_excl) = (
        tune("reg_yn", if female { 1.7 } else { 3.0 }) * st,
        tune("reg_wh", 3.5) * st,
        tune("reg_excl", 4.0) * st,
    );
    let register: Vec<f64> = (0..clause_spans.len())
        .map(|c| {
            let end = (c..clause_spans.len())
                .find(|&j| matches!(clause_spans[j].meta.punc, '.' | '?' | '!'))
                .unwrap_or(clause_spans.len() - 1);
            let m = &clause_spans[end].meta;
            let shift = if m.is_yes_no_question {
                reg_yn
            } else if m.is_question {
                reg_wh
            } else if m.is_exclaim {
                reg_excl
            } else {
                0.0
            };
            shift * expressiveness.min(1.5)
        })
        .collect();

    let f0_gain = tune("f0_gain", if female { 0.4 } else { 0.15 });
    // The lowest pitch the voice settles on, in semitones under its own pitch, and how far
    // above it a fall starts to level out. Without it a final fall ran on down to 9
    // semitones under; real speakers end 3-4 under (tools/model_gaps.py).
    let floor_depth = tune("f0_floor_knee", 2.5) * st;
    let floor_knee = floor_depth - tune("f0_floor", if female { 4.5 } else { 5.0 }) * st;
    let mut last_valid_f0 = base_f0;
    for i in 0..n {
        let ci = frames[i].clause_idx;
        if ci >= 0
            && (ci as usize) < clause_spans.len()
            && clause_spans[ci as usize].end_sec > clause_spans[ci as usize].start_sec
        {
            let cu = ci as usize;
            let mean_clause_log = if clause_voiced_cnt[cu] > 0 {
                clause_log_sum[cu] / clause_voiced_cnt[cu] as f64
            } else {
                0.0
            };
            let mut centered_log = raw_log_f0[i] as f64 - mean_clause_log * 0.96 + register[cu];
            // The bottom of the voice: a fall does not go on down, it settles on the lowest
            // pitch the speaker uses.
            if floor_depth > 0.0 && centered_log < floor_knee {
                centered_log = floor_knee - floor_depth * (1.0 - ((centered_log - floor_knee) / floor_depth).exp());
            }
            last_valid_f0 = clamp(base_f0 * centered_log.exp(), 62.0, 520.0);
            // Effort: a syllable that is lifted in pitch is also spoken louder. The same
            // vowel is 0.7 (male) to 0.9 dB (female) louder per semitone in two reference
            // voices (tools/effort_tilt.py); the amounts here bring the synthesizer's own
            // figure, which the harmonics passing the first formant already give in part, to
            // theirs.
            if f0_gain != 0.0 {
                let st_up = clamp(centered_log / st, -6.0, 8.0);
                frames[i].av *= 10f64.powf(f0_gain * st_up / 20.0);
            }
        }
        frames[i].f0 = last_valid_f0;
    }

    // Pass 3: coarticulation. Formants get explicit transitions; the articulator tracks
    // (used for lip-sync) and the source levels are smoothed.
    let crisp_speed = clamp(0.85 + crispness * 0.15, 0.75, 1.18);
    let a_mouth = 0.24;
    let release_attack = tune("release_attack", 0.8);
    // How fast the nasal passage and voicing switch between frames (1 = at once). Real
    // speech changes abruptly where the mouth seals or opens and where voicing stops; with
    // the earlier, gentler values the largest spectral jumps were 1-2 dB smaller than a real
    // speaker's (tools/dynamics.py). These values match them, and cut the phoneme error rate
    // by 1.3 (male) and 1.8 (female) points on 146 sentences.
    let sm_nasal = tune("sm_nasal", 0.8);
    let sm_av_on = tune("sm_av_on", 0.6);
    let sm_av_off = tune("sm_av_off", if female { 0.6 } else { 0.8 });
    let abduct_lax = tune("abduct_lax", if female { 0.45 } else { 0.0 });

    shape_transitions(
        &mut frames,
        &timeline.segments,
        &bounds,
        tune("tr_scale", 0.55) / crisp_speed,
        [f1_scale, f2_scale, f3_scale],
        tune("undershoot", 30.0),
    );

    for i in 1..n {
        let (head, tail) = frames.split_at_mut(i);
        let p = &head[i - 1];
        let c = &mut tail[0];
        c.fnz = p.fnz + (c.fnz - p.fnz) * 0.35;
        c.jaw = p.jaw + (c.jaw - p.jaw) * a_mouth;
        c.t_h = p.t_h + (c.t_h - p.t_h) * a_mouth;
        c.t_f = p.t_f + (c.t_f - p.t_f) * a_mouth;
        c.t_tip = p.t_tip + (c.t_tip - p.t_tip) * 0.34;
        c.lip_r = p.lip_r + (c.lip_r - p.lip_r) * a_mouth;
        c.lip_s = p.lip_s + (c.lip_s - p.lip_s) * a_mouth;
        c.lip_c = p.lip_c + (c.lip_c - p.lip_c) * 0.38;
        c.velum = p.velum + (c.velum - p.velum) * a_mouth;

        // Soft vocal-fold adduction at voice onset and abduction at offset — except after a
        // stop, where voicing starts abruptly at the release.
        let a_av = if c.av > p.av {
            if p.is_closure || p.is_burst || c.is_burst {
                release_attack
            } else {
                sm_av_on
            }
        } else {
            sm_av_off
        };
        if c.av > p.av + 0.08 {
            c.lax = (c.lax + (c.av - p.av) * 0.55).min(0.85);
        } else if p.av > c.av + 0.08 && abduct_lax > 0.0 {
            c.lax = (c.lax + (p.av - c.av) * abduct_lax).min(0.85);
        }
        c.av = p.av + (c.av - p.av) * a_av;
        c.ah = p.ah + (c.ah - p.ah) * 0.52;
        c.lax = p.lax + (c.lax - p.lax) * 0.35;

        if c.af < 0.002 && p.af > 0.015 && c.fric_g1 < 0.01 {
            c.fric_f1 = p.fric_f1;
            c.fric_b1 = p.fric_b1;
            c.fric_f2 = p.fric_f2;
            c.fric_b2 = p.fric_b2;
            c.fric_g1 = p.fric_g1;
            c.fric_g2 = p.fric_g2;
            c.fric_hp = p.fric_hp;
        }
        // Bursts stay crisp at onset and decay smoothly into the following vowel.
        c.af = if c.is_burst {
            c.af
        } else if c.af > p.af {
            p.af + (c.af - p.af) * 0.85
        } else {
            p.af + (c.af - p.af) * 0.55
        };
        c.avb = p.avb + (c.avb - p.avb) * 0.55;
        c.nasal = p.nasal + (c.nasal - p.nasal) * sm_nasal;
        c.oral_seal = p.oral_seal + (c.oral_seal - p.oral_seal) * sm_nasal;
        c.rms = p.rms + (c.rms - p.rms) * 0.36;
    }

    for i in (0..n - 1).rev() {
        let (head, tail) = frames.split_at_mut(i + 1);
        let c = &mut head[i];
        let nx = &tail[0];
        c.jaw += (nx.jaw - c.jaw) * a_mouth;
        c.t_h += (nx.t_h - c.t_h) * a_mouth;
        c.t_f += (nx.t_f - c.t_f) * a_mouth;
        c.t_tip += (nx.t_tip - c.t_tip) * 0.32;
        c.lip_r += (nx.lip_r - c.lip_r) * a_mouth;
        c.lip_s += (nx.lip_s - c.lip_s) * a_mouth;
        c.lip_c += (nx.lip_c - c.lip_c) * a_mouth;
        c.velum += (nx.velum - c.velum) * a_mouth;
    }

    // Light smoothing so microprosody blends into the phrase contour.
    for i in 1..n {
        frames[i].f0 = frames[i - 1].f0 + (frames[i].f0 - frames[i - 1].f0) * 0.35;
    }

    // Development hook: `VS_F0_FILE` names a text file with one pitch value (Hz) per frame
    // that replaces the model's contour (0 keeps the model value).
    #[cfg(feature = "dev-hooks")]
    if let Ok(path) = std::env::var("VS_F0_FILE") {
        if let Ok(text) = std::fs::read_to_string(path) {
            for (i, line) in text.lines().enumerate().take(n) {
                if let Ok(v) = line.trim().parse::<f64>() {
                    if v > 0.0 {
                        frames[i].f0 = v;
                    }
                }
            }
        }
    }

    // Development hook: `VS_FORMANT_FILE` names a text file with one line per frame,
    // `f1 f2 f3 [level]` in Hz, that replaces the model's formant movement with a measured
    // one (copy synthesis; 0 keeps the model value). The measured track is faded in and out
    // over three frames where it starts and stops. `level` scales the voicing amplitude.
    #[cfg(feature = "dev-hooks")]
    if let Ok(path) = std::env::var("VS_FORMANT_FILE") {
        if let Ok(text) = std::fs::read_to_string(path) {
            let rows: Vec<Vec<f64>> = text
                .lines()
                .take(n)
                .map(|l| l.split_whitespace().filter_map(|t| t.parse().ok()).collect())
                .collect();
            let has = |i: usize| rows.get(i).is_some_and(|r| r.len() >= 3 && r[0] > 0.0 && r[1] > 0.0 && r[2] > 0.0);
            // distance (in frames) to the nearest frame without a measurement
            let mut dist = vec![0usize; n];
            let mut run = 0usize;
            for (i, d) in dist.iter_mut().enumerate() {
                run = if has(i) { run + 1 } else { 0 };
                *d = run;
            }
            run = 0;
            for i in (0..n).rev() {
                run = if has(i) { run + 1 } else { 0 };
                dist[i] = dist[i].min(run);
            }
            for i in 0..n {
                if has(i) {
                    let w = (dist[i] as f64 / 3.0).min(1.0);
                    let r = &rows[i];
                    frames[i].f1 += (r[0] - frames[i].f1) * w;
                    frames[i].f2 += (r[1] - frames[i].f2) * w;
                    frames[i].f3 += (r[2] - frames[i].f3) * w;
                }
                if let Some(level) = rows.get(i).and_then(|r| r.get(3)) {
                    if *level > 0.0 {
                        frames[i].av *= *level;
                    }
                }
            }
        }
    }

    // Development hook: `VS_FRAMES_DUMP` writes `f1 f2 f3 av f0` for every frame, so tools can
    // compare the formant movement the rules produce with a measured one.
    #[cfg(feature = "dev-hooks")]
    if let Ok(path) = std::env::var("VS_FRAMES_DUMP") {
        let text: String = frames
            .iter()
            .map(|f| format!("{:.1} {:.1} {:.1} {:.3} {:.1}\n", f.f1, f.f2, f.f3, f.av, f.f0))
            .collect();
        let _ = std::fs::write(path, text);
    }

    frames
}
