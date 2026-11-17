//! Segmental timeline: turns a phoneme plan into acoustic segments with durations,
//! stop sub-segments (closure / burst / aspiration), coarticulated consonant loci
//! and the per-syllable accent targets the intonation model needs.

use crate::config::VoiceConfig;
use crate::phone::Ph;
use crate::plan::{Plan, Weight};
use crate::tables::{self, Place, Spec};
use crate::durations;
use crate::tune;

pub(crate) fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Flat,
    Vowel,
    Burst,
    Fric,
}

/// One acoustic segment (a phone, or a piece of one for stops and diphthongs).
#[derive(Clone, Debug)]
pub struct Seg {
    pub s: Spec,
    pub stress: u8,
    pub shape: Shape,
    pub is_phrase_tail: bool,
    pub is_closure: bool,
    pub accent_boost: f64,
    pub micro_f0: f64,
    /// Index of the clause this segment belongs to, or -1 for pauses.
    pub clause_idx: i32,
    pub onset_devoice: bool,
    pub soft_onset: bool,
    pub hiatus_onset: bool,
    pub word_onset: bool,
    /// Part of the sound (from its start) through which the vocal folds keep vibrating;
    /// 1 = all of it. English /z v/ lose their voice toward a pause or a voiceless sound.
    pub voiced_part: f64,
    /// Part of the sound (from its start) that is a silent closure before the voice sets
    /// in; 0 = none. The "th" of "the" after a pause or a consonant is made like that.
    pub late_voice: f64,
    /// Pitch offset of the whole sound (log units): close vowels sit a little higher.
    pub intrinsic_f0: f64,
    /// For /h/: how much voicing carries through it (0 = a voiceless [h], as after a pause;
    /// near 1 = the breathy-voiced [ɦ] of "ahead", "behind").
    pub breathy_h: f64,
    /// Diphthongs: the target the articulators glide toward during the segment.
    pub glide: Option<Spec>,
    /// Nasals: the formant values neighbouring vowels bend toward (set by the place of the
    /// mouth closure), which differ from the resonances of the murmur itself.
    pub locus: Option<[f64; 3]>,
    pub phone: Ph,
    /// Index of the word in the plan (counting across clauses), or -1 for pauses.
    pub word: i32,
    /// Index of the phone in the plan (counting across clauses), or -1 for pauses. A stop's
    /// closure, burst and aspiration share one token.
    pub token: i32,
}

impl Seg {
    fn from_spec(s: &Spec) -> Seg {
        Seg {
            s: *s,
            stress: 0,
            shape: Shape::Flat,
            is_phrase_tail: false,
            is_closure: false,
            accent_boost: 0.0,
            micro_f0: 0.0,
            clause_idx: 0,
            onset_devoice: false,
            soft_onset: false,
            hiatus_onset: false,
            word_onset: false,
            voiced_part: 1.0,
            late_voice: 0.0,
            intrinsic_f0: 0.0,
            breathy_h: 0.0,
            glide: None,
            locus: None,
            phone: Ph::SIL,
            word: -1,
            token: -1,
        }
    }

    fn silence() -> Seg {
        let mut seg = Seg::from_spec(&tables::SPEC_SIL);
        seg.is_closure = true;
        seg.clause_idx = -1;
        seg
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ClauseMeta {
    pub punc: char,
    pub is_question: bool,
    pub is_yes_no_question: bool,
    pub is_exclaim: bool,
    pub is_first: bool,
    pub is_last: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Timeline {
    pub segments: Vec<Seg>,
    pub clauses: Vec<ClauseMeta>,
}

struct Flat {
    base: Ph,
    stress: u8,
    /// Momentary tempo of this phone's word (1.0 = the voice's average pace).
    tempo: f64,
    /// Momentary vocal effort of this phone's word (1.0 = average).
    effort: f64,
    /// Syllables in the word this phone belongs to.
    n_sylls: usize,
    /// True if no vowel follows this phone inside its word (last syllable or its coda).
    last_syllable: bool,
    /// Stress of the next vowel in the same word (the syllable a consonant leads into), 0 if none.
    onset_stress: u8,
    word: i32,
    is_nuclear_word: bool,
    is_content: bool,
    /// The word is said in full but carries no accent of its own (see `plan::Weight::Light`).
    is_light: bool,
    is_word_initial: bool,
    is_word_final: bool,
    is_clause_final_word: bool,
}

/// Nearest vowel target (looking ahead up to 3 phones, then back up to 2).
fn peek_adjacent_vowel_spec(flat: &[Flat], idx: usize) -> &'static Spec {
    for d in 1..=3 {
        if idx + d < flat.len() {
            let nb = flat[idx + d].base;
            if let Some(dp) = tables::diph(nb) {
                if let Some(s) = tables::spec(dp.a) {
                    return s;
                }
            } else if nb.is_vowel() {
                if let Some(s) = tables::spec(nb) {
                    return s;
                }
            }
        }
    }
    for d in 1..=2 {
        if idx >= d {
            let pb = flat[idx - d].base;
            if let Some(dp) = tables::diph(pb) {
                if let Some(s) = tables::spec(dp.b) {
                    return s;
                }
            } else if pb.is_vowel() {
                if let Some(s) = tables::spec(pb) {
                    return s;
                }
            }
        }
    }
    &tables::SPEC_AX
}

/// Small deterministic random source for the natural unevenness of speech.
struct Rng(u32);

impl Rng {
    fn new(seed: u32, text: &str) -> Rng {
        // FNV-1a over the text, mixed with the seed
        let mut h: u32 = 0x811c_9dc5 ^ seed.wrapping_mul(0x9e37_79b9);
        for b in text.bytes() {
            h = (h ^ b as u32).wrapping_mul(0x0100_0193);
        }
        Rng(h | 1)
    }

    fn uniform(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0 as f64 / u32::MAX as f64
    }

    /// Roughly standard-normal (sum of four uniforms), limited to about ±3.
    fn gauss(&mut self) -> f64 {
        (self.uniform() + self.uniform() + self.uniform() + self.uniform() - 2.0) * 1.732
    }

    /// A factor around 1.0 with the given relative spread.
    fn spread(&mut self, sd: f64) -> f64 {
        (sd * self.gauss()).exp()
    }
}

/// Vocalic target at `flat[idx]`, including diphthong onset/offglide, speaker vowel space,
/// and the glides /j/ and /w/ (`forward`: entering `idx` from the left vs leaving on its right).
fn vocalic_spec_at(flat: &[Flat], idx: usize, forward: bool, cfg: &VoiceConfig) -> Option<(Spec, Ph, u8)> {
    let item = flat.get(idx)?;
    let nb = item.base;
    if let Some(dp) = tables::diph(nb) {
        let mut s = *tables::spec(if forward { dp.a } else { dp.b })?;
        if forward {
            s.f1 = dp.f1a;
            s.f2 = dp.f2a;
        } else {
            s.f1 = dp.f1b;
            s.f2 = dp.f2b;
        }
        if let Some(vi) = crate::config::vowel_index(nb) {
            let m = cfg.vowel_shape(vi);
            s.f1 *= m[0];
            s.f2 *= m[1];
            s.f3 *= m[2];
        }
        return Some((s, nb, item.stress));
    }
    if nb.is_vowel() {
        let mut s = *tables::spec(nb)?;
        if let Some(vi) = crate::config::vowel_index(nb) {
            let m = cfg.vowel_shape(vi);
            s.f1 *= m[0];
            s.f2 *= m[1];
            s.f3 *= m[2];
        }
        let next_ph = flat.get(idx + 1).map(|x| x.base).unwrap_or(Ph::SIL);
        apply_vowel_context(nb, next_ph, cfg, &mut s);
        return Some((s, nb, item.stress));
    }
    if matches!(nb, Ph::Y | Ph::W) {
        return Some((*tables::spec(nb)?, nb, item.stress));
    }
    if !forward && matches!(nb, Ph::L | Ph::R) {
        let mut s = *tables::spec(nb)?;
        if nb == Ph::L {
            s.f1 = 425.0;
            s.f2 = 920.0;
            s.f3 = 2800.0;
        } else {
            s.f1 = 465.0;
            s.f2 = 1340.0;
            s.f3 = 1680.0;
        }
        return Some((s, nb, item.stress));
    }
    None
}

/// Context-conditioned vowel allophony shared by segment targets and consonant loci:
/// NORTH/FORCE (/ao r/: "lord", "boards", "shorter") is close-mid rounded rather than open [ɑ];
/// START (/aa r/: "start", "large", "garbage", "tomorrow") is retracted rhotic [ɑɹ];
/// SQUARE (/eh r/: "stared", "careful", "their") stays mid rather than opening into [æ];
/// GOOSE before dark /l/ ("tool", "cool", "fool") stays back rather than fronting to [i].
fn apply_vowel_context(base: Ph, next_base: Ph, cfg: &VoiceConfig, s: &mut Spec) {
    let female = cfg.gender == crate::config::Gender::Female;
    if base == Ph::AO && next_base == Ph::R {
        s.f1 *= tune("aor_f1", if female { 0.68 } else { 0.78 });
        s.f2 *= tune("aor_f2", if female { 0.84 } else { 0.86 });
    } else if base == Ph::AA && next_base == Ph::R {
        s.f1 *= if female { 0.85 } else { 0.90 };
        s.f2 *= 0.91;
        s.f3 *= 0.88;
    } else if base == Ph::EH && next_base == Ph::R {
        s.f1 *= tune("ehr_f1", if female { 0.86 } else { 0.89 });
    } else if base == Ph::IH && next_base == Ph::R {
        // NEAR (/ih r/: "reared", "clear", "near"): close-front nucleus resists collapsing into /r/.
        s.f1 *= tune("ihr_f1", 0.86);
        s.f2 *= tune("ihr_f2", 1.06);
    } else if base == Ph::IH && matches!(next_base, Ph::NG | Ph::G) {
        // Pre-velar raising ("raining", "playing", "pink", "single"): tense high-front [ɪ~i].
        s.f1 *= 0.88;
        s.f2 *= 1.065;
    } else if base == Ph::UW && next_base == Ph::L {
        s.f2 *= tune("uwl_f2", if female { 0.80 } else { 0.85 });
    }
}

fn blend_spec(a: &Spec, b: &Spec, w_b: f64) -> Spec {
    let w_a = 1.0 - w_b;
    let mut out = *b;
    out.f1 = a.f1 * w_a + b.f1 * w_b;
    out.f2 = a.f2 * w_a + b.f2 * w_b;
    out.f3 = a.f3 * w_a + b.f3 * w_b;
    out.jaw = a.jaw * w_a + b.jaw * w_b;
    out.t_h = a.t_h * w_a + b.t_h * w_b;
    out.t_f = a.t_f * w_a + b.t_f * w_b;
    out.t_tip = a.t_tip * w_a + b.t_tip * w_b;
    out.lip_r = a.lip_r * w_a + b.lip_r * w_b;
    out.lip_s = a.lip_s * w_a + b.lip_s * w_b;
    out
}

/// Returns `(closing_context, release_context)` for a consonant at `idx`.
/// Coda consonants ("makes", "Gregson", "singing") close from the preceding vowel rather
/// than jumping across obstruents to a later word; intervocalic consonants before unstressed
/// schwa ("chicken") keep most of the stressed vowel's tongue body.
fn coarticulated_specs(flat: &[Flat], idx: usize, cfg: &VoiceConfig) -> (Spec, Spec) {
    let prev = if idx > 0 { vocalic_spec_at(flat, idx - 1, false, cfg) } else { None };
    let next = vocalic_spec_at(flat, idx + 1, true, cfg).or_else(|| {
        if idx + 2 < flat.len()
            && is_approximant(flat[idx + 1].base)
            && flat[idx + 1].word == flat[idx].word
        {
            vocalic_spec_at(flat, idx + 2, true, cfg)
        } else {
            None
        }
    });
    match (prev, next) {
        (Some((p, p_ph, _)), Some((nx, nx_ph, nx_st))) => {
            if nx_ph == Ph::AX && nx_st == 0 && p_ph != Ph::AX {
                (blend_spec(&p, &nx, 0.20), blend_spec(&p, &nx, 0.45))
            } else {
                (blend_spec(&p, &nx, 0.55), nx)
            }
        }
        (Some((p, _, _)), None) => (p, p),
        (None, Some((nx, _, _))) => (nx, nx),
        (None, None) => {
            let fb = *peek_adjacent_vowel_spec(flat, idx);
            (fb, fb)
        }
    }
}

/// Formant locus (F2, F3) and burst F1 of a closure at `place`, coarticulated with `v_adj`.
fn place_locus_ext(place: Place, v_adj: &Spec, default: (f64, f64), default_burst: f64) -> (f64, f64, f64) {
    match place {
        // The lips close over whatever the tongue is doing, so F2 at /b p m/ follows the
        // vowel half way: the readers' vowels start close to their own F2 after /b/. From a
        // locus a quarter of the way (740 Hz at F2 1150) the voice glided up from far below
        // as a /w/ does, and "but", "big" and "bear" were heard with a /w/ one time in 25.
        Place::Labial => (clamp(900.0 + (v_adj.f2 - 1150.0) * 0.5, 700.0, 1700.0), default.1, default_burst),
        Place::Alveolar => (
            clamp(
                1780.0 + (v_adj.f2 - 1650.0) * 0.18,
                tune("alv_floor", 1580.0),
                2050.0,
            ),
            default.1,
            default_burst,
        ),
        Place::Velar => {
            if v_adj.f2 > 1580.0 {
                let f2 = clamp(1980.0 + (v_adj.f2 - 1600.0) * 0.40, 2020.0, 2260.0);
                let f3 = clamp(f2 + 280.0, 2380.0, 2540.0);
                let bf1 = clamp(f2 + tune("velar_front_off", 340.0), 2260.0, tune("velar_front_burst", 2640.0));
                (f2, f3, bf1)
            } else {
                // Back velars keep F2 above the vowel and pinch F3 down toward it, distinguishing
                // /k g/ from labial /p b/ while staying below alveolar /t d/. Their burst is one
                // narrow peak just above the vowel's F2: both readers put it at 1-1.5 kHz before
                // back vowels and /r/. Set 300-400 Hz higher, "go", "got" and "grass" were heard
                // with a /d/ (one /g/ in seven; now one in twenty, the readers none).
                let f2 = clamp(v_adj.f2 * 0.88 + 290.0, 1200.0, 1660.0);
                let f3 = clamp(f2 + 860.0, 2370.0, 2500.0);
                let bf1 = clamp(v_adj.f2 + 100.0, 1100.0, 1980.0);
                (f2, f3, bf1)
            }
        }
        Place::Other => (default.0, default.1, default_burst),
    }
}

/// Formant locus (F2, F3) of a closure at `place`, coarticulated with the nearby vowel.
fn place_locus(place: Place, v_adj: &Spec, default: (f64, f64)) -> (f64, f64) {
    let (f2, f3, _) = place_locus_ext(place, v_adj, default, 0.0);
    (f2, f3)
}

fn is_approximant(p: Ph) -> bool {
    matches!(p, Ph::L | Ph::R | Ph::W | Ph::Y)
}

/// The frequency above which the noise of a stop's release is cut off (0 = not at all),
/// from where the stop is made and the frequency of its burst. The burst of /k g/ is one
/// compact band: before "w" or "r" a real one has next to nothing above 2 kHz, and with a
/// skirt reaching to the top of the band, as a single resonator leaves it, it was heard as
/// /t d/ - a quarter of the misheard /k g/ went with it (and lowering the ceiling from 1.7 to
/// 1.4 times the burst halved what was left). /t d/ and the affricates hiss all the way up.
pub(crate) fn burst_ceiling(place: Place, burst_hz: f64) -> f64 {
    match place {
        Place::Velar => clamp(tune("velar_lp", 1.4) * burst_hz, 0.0, 6500.0),
        // (a ceiling on /p b/ was tried too: without their click they were heard as /w/)
        Place::Labial => tune("labial_lp", 0.0),
        Place::Alveolar | Place::Other => 0.0,
    }
}

pub fn build_timeline(plan: &Plan, cfg: &VoiceConfig) -> Timeline {
    let speed = clamp(cfg.speed, 0.55, 1.75);
    let rate_scale = 1.0 / speed;
    let expressiveness = clamp(cfg.expressiveness, 0.0, 3.0);
    let crispness = clamp(cfg.crispness, 0.45, 1.6);
    let whisper = cfg.whisper;
    // (The reference voices speak 5.4-5.7 syllables a second; at 1.0 the synthesizer spoke
    // 4.4. Going all the way costs clarity; 8% costs the female voice nothing, and the male
    // voice, whose consonants are less robust, keeps to 4%. A further 2-3% keeps the pace
    // where it was now that the duration model no longer counts pronouns and auxiliaries
    // among the content words, which made the rest come out longer.)
    let dur_scale = tune("dur_scale", if cfg.gender == crate::config::Gender::Female { 0.90 } else { 0.93 });
    // The corpus speakers read briskly; the default voice is a little more deliberate.
    let vowel_scale = tune("vdur", 1.35);
    let cons_scale = tune("cdur", 1.05);
    // Human unevenness: a seeded random source (the same text and seed always give the same
    // reading; another seed gives another "take").
    let variation = clamp(cfg.variation, 0.0, 1.0) * tune("variation_scale", 1.0);
    let mut rng = Rng::new(cfg.seed, &plan.normalized);
    let mut tempo_walk = 0.0f64;

    let mut segments: Vec<Seg> = Vec::new();
    let mut sil_lead = Seg::silence();
    sil_lead.s.dur = 75.0;
    segments.push(sil_lead);

    let n_clauses = plan.clauses.len();
    let mut clause_meta: Vec<ClauseMeta> = Vec::with_capacity(n_clauses);
    let mut word_base: i32 = 0;
    let mut token_base: i32 = 0;
    #[cfg(feature = "dev-hooks")]
    let dump_features = std::env::var("VS_FEATURES_FILE").is_ok();
    #[cfg(feature = "dev-hooks")]
    let mut feature_lines: Vec<String> = Vec::new();

    for (ci, clause) in plan.clauses.iter().enumerate() {
        let ci_i = ci as i32;
        let mut flat: Vec<Flat> = Vec::new();

        // The nuclear (most prominent) accent falls on the last word that carries meaning
        // ("I *saw* him", not "I saw *him*"); a phrase of little words only ("It is.") has it
        // on its last word.
        let nuclear_word_idx = clause
            .words
            .iter()
            .rposition(|w| w.weight == Weight::Content)
            .or(if clause.words.is_empty() {
                None
            } else {
                Some(clause.words.len() - 1)
            });

        // No two phrases are spoken at exactly the same pace.
        let clause_tempo = (tune("var_clause", 0.06) * variation * rng.gauss()).exp();

        for (wi, w_obj) in clause.words.iter().enumerate() {
            let n_sylls = w_obj.phones.iter().filter(|p| p.ph.is_vowel()).count().max(1);
            // Tempo drifts from word to word: each word inherits part of the previous word's
            // pace and adds its own deviation.
            tempo_walk = 0.55 * tempo_walk + 0.835 * rng.gauss();
            let word_tempo = clause_tempo * (tune("var_word", 0.048) * variation * tempo_walk).exp();
            // ... and so does loudness, a little. (It was a decibel or so: of the kinds of
            // unevenness that was the one that cost naturalness instead of adding it -
            // a real voice is louder where the sentence wants it, not at random.)
            let word_effort = (tune("var_effort", 0.025) * variation * rng.gauss()).exp();
            let n_ph = w_obj.phones.len();
            for (pi, p) in w_obj.phones.iter().enumerate() {
                let next_vowel = w_obj.phones[pi + 1..].iter().find(|q| q.ph.is_vowel());
                flat.push(Flat {
                    base: p.ph,
                    stress: p.stress,
                    tempo: word_tempo * (tune("var_phone", 0.022) * variation * rng.gauss()).exp(),
                    effort: word_effort,
                    n_sylls,
                    last_syllable: next_vowel.is_none(),
                    onset_stress: next_vowel.map_or(0, |q| q.stress),
                    word: word_base + wi as i32,
                    is_nuclear_word: Some(wi) == nuclear_word_idx,
                    is_content: w_obj.is_content,
                    is_light: w_obj.weight == Weight::Light && Some(wi) != nuclear_word_idx,
                    is_word_initial: pi == 0,
                    is_word_final: pi == n_ph - 1,
                    is_clause_final_word: w_obj.is_clause_final,
                });
            }
        }
        word_base += clause.words.len() as i32;

        clause_meta.push(ClauseMeta {
            punc: clause.punc,
            is_question: clause.is_question,
            is_yes_no_question: clause.is_yes_no_question,
            is_exclaim: clause.is_exclaim,
            is_first: ci == 0,
            is_last: ci == n_clauses - 1,
        });

        let n_phones = flat.len();
        let mut content_seen = 0.0f64;
        let clause_token_base = token_base;
        token_base += n_phones as i32;

        for i in 0..n_phones {
            let item = &flat[i];
            let token = clause_token_base + i as i32;
            #[cfg(feature = "dev-hooks")]
            if dump_features {
                let name = |k: Option<&Flat>| k.map_or("SIL", |f| f.base.name());
                feature_lines.push(format!(
                    "{token} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
                    item.base.name(),
                    item.stress,
                    item.is_content as u8,
                    item.is_word_initial as u8,
                    item.is_word_final as u8,
                    item.is_clause_final_word as u8,
                    item.is_nuclear_word as u8,
                    item.n_sylls,
                    item.last_syllable as u8,
                    item.onset_stress,
                    name(if i > 0 { flat.get(i - 1) } else { None }),
                    name(flat.get(i + 1)),
                    (i > 0 && flat[i - 1].word == item.word) as u8,
                    (i + 1 < n_phones && flat[i + 1].word == item.word) as u8,
                    n_phones - i,
                    clause.punc,
                    item.is_light as u8,
                ));
            }
            let base = item.base;
            let prev_base = if i > 0 { flat[i - 1].base } else { Ph::SIL };
            let next_base = if i + 1 < n_phones { flat[i + 1].base } else { Ph::SIL };
            let base_is_vowel = base.is_vowel();

            // Local pitch movement (a Gaussian bump or dip centred early in the vowel), in log
            // units (0.058 = one semitone): the phrase's main accent stands out most, other
            // stressed content words less, function words sit a little low, and every speaker
            // wanders from word to word, which is what `variation` reproduces. The overall size
            // (`acc`) is set so that the pitch movement left after taking out each sentence's
            // slope is as large as in the reference voices (2 to 3 semitones of spread; with
            // the earlier, least-squares-fitted size it was 0.85).
            let mut accent_boost = 0.0;
            if base_is_vowel {
                let acc = tune("acc", if cfg.gender == crate::config::Gender::Female { 3.0 } else { 2.6 }) * expressiveness;
                let wander = tune("acc_wander", 0.07) * variation * expressiveness.min(1.0);
                if item.stress == 1 && item.is_nuclear_word {
                    accent_boost = tune("acc_nuc", 0.10) * acc + wander * rng.gauss();
                } else if item.is_light {
                    // said in full, but passed over by the melody
                    accent_boost = -tune("acc_light", 0.0) * acc;
                } else if item.stress == 1 && item.is_content {
                    let alternate = if (content_seen as i32) % 2 == 1 {
                        tune("acc_alt", 0.85)
                    } else {
                        1.0
                    };
                    // (0.06 before the pronouns and auxiliaries lost their accents: the
                    // movement they no longer make belongs to the words that carry meaning)
                    accent_boost = tune("acc_first", 0.075) * alternate * acc + wander * rng.gauss();
                    content_seen += 1.0;
                } else if !item.is_content {
                    accent_boost = -tune("acc_func", 0.02) * acc;
                }
            }

            // Consonantal microprosody: voiceless obstruents raise the onset F0 of the next vowel.
            let prev_voiceless = matches!(
                prev_base,
                Ph::P | Ph::T | Ph::K | Ph::F | Ph::TH | Ph::S | Ph::SH | Ph::CH | Ph::HH
            );
            let micro_f0 = if base_is_vowel && prev_voiceless { 0.030 * tune("micro_scale", 1.35) } else { 0.0 };
            // Intrinsic pitch: with the tongue high the larynx is pulled up, so close vowels sit
            // a little higher than open ones (semitones between the two; five reference
            // voices have +0.1 to +0.9 where the synthesizer had -0.6: tools/model_gaps.py).
            let intrinsic_f0 = tune("if0", 0.8) * (std::f64::consts::LN_2 / 12.0)
                * match base {
                    Ph::IY | Ph::IH | Ph::UW | Ph::UH => 0.5,
                    Ph::AA | Ph::AE | Ph::AO | Ph::AW | Ph::AY => -0.5,
                    _ => 0.0,
                };

            let prev_same_word_cons = i > 0 && flat[i - 1].word == item.word && !prev_base.is_vowel();
            let next_same_word_cons =
                i + 1 < n_phones && flat[i + 1].word == item.word && !next_base.is_vowel();
            let cluster_scale = if !base_is_vowel
                && (prev_same_word_cons || next_same_word_cons)
                && !(matches!(base, Ph::S | Ph::SH) && matches!(prev_base, Ph::B | Ph::D | Ph::G))
            {
                0.80
            } else {
                1.0
            };

            let is_voiceless_obs = |p: Ph| {
                matches!(p, Ph::P | Ph::T | Ph::K | Ph::CH | Ph::F | Ph::TH | Ph::S | Ph::SH)
            };
            let is_voiced_obs = |p: Ph| {
                matches!(p, Ph::B | Ph::D | Ph::G | Ph::JH | Ph::V | Ph::DH | Ph::Z | Ph::ZH)
            };
            // Polysyllabic shortening (Lehiste 1972) on unstressed/secondary syllables so
            // "almost", "forever", "crooked" stay cohesive single words without clipping stressed nuclei.
            let poly_scale = if base_is_vowel && item.n_sylls >= 2 && item.stress != 1 {
                let ps = tune("poly_unstressed", 1.0);
                if item.n_sylls >= 3 { ps * ps } else { ps }
            } else {
                1.0
            };
            let pre_fortis = if base_is_vowel && i + 1 < n_phones && flat[i + 1].word == item.word {
                if is_voiceless_obs(next_base) {
                    tune("pre_fortis", 1.0)
                } else if is_voiced_obs(next_base) && !item.is_clause_final_word {
                    tune("pre_lenis", 1.0)
                } else {
                    1.0
                }
            } else {
                1.0
            };
            let ax_init_scale = if base == Ph::AX && item.stress == 0 && item.is_word_initial {
                if item.is_word_final {
                    1.20
                } else {
                    tune("ax_init", 1.0)
                }
            } else {
                1.0
            };
            let prestress_scale = if base == Ph::T
                && item.is_word_initial
                && next_base == Ph::AX
                && i + 1 < n_phones
                && flat[i + 1].stress == 0
                && !flat[i + 1].last_syllable
                && flat[i + 1].onset_stress == 1
            {
                0.56
            } else if base == Ph::AX
                && item.stress == 0
                && !item.last_syllable
                && item.onset_stress == 1
                && prev_base == Ph::T
                && i >= 1
                && flat[i - 1].is_word_initial
            {
                0.72
            } else if matches!(base, Ph::M | Ph::N | Ph::L | Ph::R)
                && item.onset_stress == 1
                && i >= 2
                && flat[i - 1].word == item.word
                && flat[i - 1].base == Ph::AX
                && flat[i - 1].stress == 0
                && flat[i - 2].word == item.word
                && flat[i - 2].is_word_initial
            {
                0.66
            } else {
                1.0
            };
            let lm_cluster_scale = if !base_is_vowel
                && ((next_same_word_cons && matches!(base, Ph::L | Ph::R) && matches!(next_base, Ph::M | Ph::N))
                    || (prev_same_word_cons && matches!(prev_base, Ph::L) && base == Ph::M))
            {
                0.68
            } else {
                1.0
            };
            let cw_rime_scale = if item.is_word_final
                && !item.is_clause_final_word
                && item.is_content
                && base_is_vowel
                && i + 1 < n_phones
                && flat[i + 1].is_content
            {
                tune("cw_rime", 1.05)
            } else {
                1.0
            };
            let word_juncture = item.is_word_initial
                && prev_base != Ph::SIL
                && (prev_base.is_vowel()
                    || is_approximant(prev_base)
                    || matches!(prev_base, Ph::M | Ph::N | Ph::NG | Ph::Z | Ph::V | Ph::DH | Ph::D | Ph::G | Ph::B))
                && (base_is_vowel || base == Ph::HH || is_approximant(base));

            // How long this sound lasts: the duration model fitted to real speech, scaled by
            // the speaking rate and this word's momentary tempo.
            let model_ms = durations::duration_ms(
                base,
                &durations::Context {
                    stress: item.stress,
                    is_content: item.is_content,
                    light: item.is_light,
                    word_initial: item.is_word_initial,
                    word_final: item.is_word_final,
                    phrase_final_word: item.is_clause_final_word,
                    nuclear: item.is_nuclear_word,
                    n_sylls: item.n_sylls,
                    last_syllable: item.last_syllable,
                    onset_stress: item.onset_stress,
                    prev: prev_base,
                    next: next_base,
                    prev_same_word: i > 0 && flat[i - 1].word == item.word,
                    next_same_word: i + 1 < n_phones && flat[i + 1].word == item.word,
                },
            ) * rate_scale
                * dur_scale
                * if base_is_vowel {
                    if base == Ph::AX && item.stress == 0 {
                        tune("ax_vdur", vowel_scale)
                    } else if tables::diph(base).is_some() && item.stress == 0 && !item.is_content {
                        vowel_scale * tune("weak_diph", 0.84)
                    } else {
                        vowel_scale
                    }
                } else {
                    cons_scale
                }
                * pre_fortis
                * poly_scale
                * ax_init_scale
                * prestress_scale
                * lm_cluster_scale
                * cw_rime_scale
                // (the corpus labels count part of the neighbouring vowel as l/r/w/y)
                * if is_approximant(base) { 0.85 } else { 1.0 }
                * item.tempo;
            let first_seg = segments.len();

            // Vocal effort by stress. Measured on real speech, unstressed and function-word
            // vowels are only 2-3 dB quieter than stressed ones; their shortness and softer
            // voice quality already cost most of that, so they get a small lift here.
            let mut effort_gain = 1.0;
            if base_is_vowel {
                effort_gain = if item.stress == 1 && item.is_nuclear_word {
                    tune("gain_nuclear", 1.08)
                } else if item.stress == 1 {
                    1.0
                } else if item.stress == 2 {
                    tune("gain_secondary", 1.0)
                } else {
                    tune("gain_unstressed", 1.10)
                } * item.effort;
            }
            let is_phrase_tail_vowel = item.is_clause_final_word
                && base_is_vowel
                && (next_base == Ph::SIL || i + 2 >= n_phones);

            // 1. Diphthongs: one segment whose tongue glides from the nucleus toward the offglide
            if let Some(dp) = tables::diph(base) {
                let spec_a = tables::spec(dp.a).unwrap_or(&tables::SPEC_AX);
                let spec_b = tables::spec(dp.b).unwrap_or(&tables::SPEC_AX);
                let mut seg = Seg::from_spec(spec_a);
                seg.s.f1 = dp.f1a;
                seg.s.f2 = dp.f2a;
                let mut end = *spec_b;
                end.f1 = dp.f1b;
                end.f2 = dp.f2b;
                // the speaker's own version of this vowel
                if let Some(vi) = crate::config::vowel_index(base) {
                    let m = cfg.vowel_shape(vi);
                    seg.s.f1 *= m[0];
                    seg.s.f2 *= m[1];
                    seg.s.f3 *= m[2];
                    end.f1 *= m[0];
                    end.f2 *= m[1];
                    end.f3 *= m[2];
                }
                seg.glide = Some(end);
                seg.s.dur = model_ms.max(if item.stress == 0 && !item.is_content { 44.0 } else { 55.0 });
                seg.s.av = (seg.s.av * effort_gain).min(1.2);
                seg.stress = item.stress;
                seg.accent_boost = accent_boost;
                seg.micro_f0 = micro_f0;
                seg.clause_idx = ci_i;
                seg.shape = Shape::Vowel;
                seg.is_phrase_tail = is_phrase_tail_vowel;
                seg.phone = base;
                seg.word = item.word;
                seg.token = token;
                seg.soft_onset = prev_base == Ph::SIL;
                seg.hiatus_onset = item.is_word_initial && item.stress >= 1 && prev_base.is_vowel();
                seg.word_onset = word_juncture;
                if matches!(next_base, Ph::M | Ph::N | Ph::NG) {
                    seg.s.nasal = seg.s.nasal.max(0.20);
                    seg.s.velum = seg.s.velum.max(0.38);
                }
                segments.push(seg);
                continue;
            }

            // 2. Plosives, affricates and the word-internal alveolar tap
            if let Some(st) = tables::stop(base) {
                let (v_close, v_rel) = coarticulated_specs(&flat, i, cfg);
                let v_adj = &v_rel;
                let next_stress = if i + 1 < n_phones { flat[i + 1].stress } else { 0 };
                let next2_stress = if i + 2 < n_phones && is_approximant(next_base) {
                    flat[i + 2].stress
                } else {
                    0
                };
                let stressed_onset =
                    (next_stress == 1 || next2_stress == 1) && (i == 0 || prev_base != Ph::S);
                let prev_stress = if i > 0 { flat[i - 1].stress } else { 0 };
                // American English turns /t d/ into a quick voiced tap inside a word before an
                // unstressed vowel ("letter", "ladder", "water", "city"), except before syllabic /n/ ("button").
                let before_syllabic_n =
                    next_base == Ph::AX && i + 2 < n_phones && flat[i + 2].base == Ph::N && flat[i + 2].word == item.word;
                let word_internal_tap = (base == Ph::T || base == Ph::D)
                    && !item.is_word_initial
                    && !item.is_word_final
                    && i > 0
                    && prev_base.is_vowel()
                    && (prev_stress >= 1 || next_base == Ph::IY)
                    && i + 1 < n_phones
                    && next_base.is_vowel()
                    && next_stress == 0
                    && !before_syllabic_n;
                let cross_word_tap = base == Ph::T
                    && item.is_word_final
                    && !item.is_clause_final_word
                    && i > 0
                    && prev_base.is_vowel()
                    && i + 1 < n_phones
                    && next_base.is_vowel()
                    && (!item.is_content || next_stress == 0)
                    && tune("xword_tap", 0.0) > 0.5;
                let intervocalic_alveolar = word_internal_tap || cross_word_tap;

                // Consonant closure and release loci, coarticulated separately with left and right context.
                let loc_f1 = st.f1;
                let (close_f2, close_f3, _) =
                    place_locus_ext(st.place, &v_close, (st.f2, st.f3), st.b_f1);
                let (mut loc_f2, loc_f3, mut burst_f1) =
                    place_locus_ext(st.place, &v_rel, (st.f2, st.f3), st.b_f1);

                let same_word_next = i + 1 < n_phones && flat[i + 1].word == item.word;
                let next_is_stop = tables::stop(next_base).is_some()
                    && base != Ph::CH
                    && base != Ph::JH
                    && !(!same_word_next && matches!(next_base, Ph::CH | Ph::JH));
                let next_is_sonorant = is_approximant(next_base);
                let next_same_word_sonorant = next_is_sonorant && same_word_next;
                let next_is_obstruent_or_nasal =
                    !next_base.is_vowel() && !next_is_sonorant && next_base != Ph::SIL;

                let mut close_base_ms =
                    st.close_ms * if stressed_onset { 1.0 } else { 0.74 } * cluster_scale;
                if intervocalic_alveolar {
                    close_base_ms = 20.0;
                } else if next_base == base && !same_word_next {
                    close_base_ms = 52.0;
                } else if next_is_stop {
                    close_base_ms = if !same_word_next { 40.0 } else { 28.0 };
                } else if i > 0 && prev_base == Ph::S && same_word_next {
                    close_base_ms = close_base_ms.max(38.0);
                }
                let mut seg_close = Seg::from_spec(&tables::SPEC_SIL);
                seg_close.s.dur =
                    (close_base_ms * rate_scale * if i == 0 { 0.55 } else { 0.88 }).max(18.0);
                seg_close.s.f1 = loc_f1;
                seg_close.s.f2 = close_f2;
                seg_close.s.f3 = close_f3;
                seg_close.s.av = if intervocalic_alveolar { 0.48 } else { 0.0 };
                seg_close.s.avb = if (st.voiced || intervocalic_alveolar) && !whisper {
                    // after a pause English /b d g/ have no voicing in the closure (hook)
                    let vbar = if cfg.gender == crate::config::Gender::Female {
                        tune("vbar_f", 0.28)
                    } else {
                        tune("vbar_m", 0.35)
                    };
                    let reg_devoice = if is_voiceless_obs(next_base) { 0.85 } else { 1.0 };
                    vbar * reg_devoice * if prev_base == Ph::SIL || i == 0 { tune("init_prevoice", 0.22) } else { 1.0 }
                } else {
                    0.0
                };
                seg_close.s.jaw = st.jaw;
                seg_close.s.t_h = st.t_h;
                seg_close.s.t_f = st.t_f;
                seg_close.s.t_tip = st.t_tip;
                seg_close.s.lip_r = st.lip_r;
                seg_close.s.lip_c = st.lip_c;
                seg_close.is_closure = true;
                seg_close.micro_f0 = if st.voiced { -0.018 } else { 0.0 };
                seg_close.clause_idx = ci_i;
                seg_close.phone = base;
                seg_close.word = item.word;
                seg_close.token = token;
                segments.push(seg_close);

                if next_base != base && !next_is_stop {
                    let v_target: &Spec = if next_is_sonorant && !(matches!(base, Ph::CH | Ph::JH) && !same_word_next) {
                        tables::spec(next_base).unwrap_or(v_adj)
                    } else {
                        v_adj
                    };
                    if st.place == Place::Velar && next_base == Ph::W {
                        loc_f2 = 860.0;
                        burst_f1 = 1350.0;
                    } else if st.place == Place::Velar && next_base == Ph::R {
                        // Before /r/ ("Gregson", "grass", "green"), lips round and tongue retracts
                        // regardless of the vowel after /r/, keeping a compact rhotic-velar burst.
                        loc_f2 = clamp(loc_f2, 1240.0, 1520.0);
                        burst_f1 = tune("velar_r_burst", 1500.0);
                    } else if st.place == Place::Velar && next_base == Ph::L {
                        // Before /l/ ("glue", "clear", "clean"), keep velar burst above labial /b/.
                        loc_f2 = clamp(loc_f2, 1550.0, 1820.0);
                        burst_f1 = burst_f1.max(1750.0);
                    } else if st.place == Place::Labial && matches!(next_base, Ph::L | Ph::R | Ph::W) {
                        // Closed-lip release into /l r w/ ("playing", "spring", "blue"): deep labial dip.
                        loc_f2 = loc_f2.min(820.0);
                    }
                    let voiced_like = st.voiced || intervocalic_alveolar;
                    let tr_affric = base == Ph::T
                        && next_base == Ph::R
                        && same_word_next
                        && !(i > 0 && prev_base == Ph::S);

                    let is_affricate = matches!(base, Ph::CH | Ph::JH);
                    let mut seg_burst = Seg::from_spec(v_target);
                    let burst_scale = if intervocalic_alveolar {
                        0.45
                    } else if is_affricate {
                        if stressed_onset { 1.0 } else { 0.88 }
                    } else if tr_affric {
                        1.32
                    } else if next_is_obstruent_or_nasal {
                        0.50
                    } else if st.place == Place::Velar && next_same_word_sonorant && next_base == Ph::L {
                        1.25
                    } else if st.voiced && (prev_base == Ph::SIL || i == 0) {
                        1.22
                    } else if stressed_onset {
                        1.0
                    } else {
                        0.82
                    };
                    seg_burst.s.dur = (st.burst_ms * burst_scale * clamp(rate_scale, 0.80, 1.25)).max(6.0);
                    seg_burst.s.f1 = loc_f1 * 0.65 + v_target.f1 * 0.35;
                    seg_burst.s.f2 = loc_f2 * 0.75 + v_target.f2 * 0.25;
                    seg_burst.s.f3 = loc_f3 * 0.70 + v_target.f3 * 0.30;
                    seg_burst.s.av = if base == Ph::JH {
                        if next_base != Ph::SIL && !whisper {
                            tune("jh_av", 0.36)
                        } else {
                            0.0
                        }
                    } else if voiced_like
                        && next_base != Ph::SIL
                        && !next_is_obstruent_or_nasal
                        && !whisper
                    {
                        tune("vburst_av", 0.45)
                    } else {
                        0.0
                    };
                    // A voiced stop or /jh/ keeps its voice bar going through a final or pre-consonant release.
                    seg_burst.s.avb = if is_voiceless_obs(next_base) {
                        0.14
                    } else if base == Ph::JH && !whisper {
                        0.35
                    } else if voiced_like && !whisper && seg_burst.s.av == 0.0 {
                        0.30
                    } else {
                        0.0
                    };
                    // How strongly this stop is released (development hooks, see
                    // tools/stop_release.py).
                    let release = match base {
                        Ph::P => tune("rel_p", 1.22),
                        Ph::B => tune("rel_b", 1.0),
                        Ph::K => tune("rel_k", 1.0),
                        Ph::G => tune("rel_g", if prev_base == Ph::SIL || i == 0 { 1.20 } else { 1.0 }),
                        _ => 1.0,
                    } * if next_is_obstruent_or_nasal { tune("cons_release", 1.0) } else { 1.0 };
                    seg_burst.s.af = st.burst_af
                        * release
                        * crispness
                        * if intervocalic_alveolar {
                            0.45
                        } else if is_affricate {
                            if next_base == Ph::SIL && st.voiced { 0.85 } else { 0.98 }
                        } else if next_is_obstruent_or_nasal {
                            if st.place == Place::Velar { 0.80 } else { 0.55 }
                        } else if next_base == Ph::SIL {
                            // utterance-final releases are weak, voiced ones barely audible
                            if st.voiced {
                                tune("final_vburst", 0.40)
                            } else {
                                0.72
                            }
                        } else {
                            1.0
                        };
                    seg_burst.s.ah = if voiced_like || next_is_obstruent_or_nasal {
                        0.02
                    } else if stressed_onset {
                        0.14
                    } else {
                        0.08
                    };
                    seg_burst.s.fric_f1 = if tr_affric {
                        3050.0
                    } else if st.place == Place::Velar && cfg.gender == crate::config::Gender::Female {
                        burst_f1.max(1240.0)
                    } else {
                        burst_f1
                    };
                    // (a velar burst is narrower in speech than the table's, which singing keeps)
                    seg_burst.s.fric_b1 = if tr_affric {
                        1150.0
                    } else {
                        st.b_b1 * if st.place == Place::Velar { 0.6 } else { 1.0 }
                    };
                    seg_burst.s.fric_f2 = st.b_f2;
                    seg_burst.s.fric_b2 = st.b_b2;
                    // Burst spectrum by place: labial = diffuse and falling, alveolar = diffuse and
                    // rising (above 4 kHz), velar = one compact peak near the next vowel's F2/F3.
                    let (g1, g2, low_cut) = match st.place {
                        Place::Labial => (0.85, 0.05, 0.0),
                        Place::Alveolar => if tr_affric { (0.68, 0.48, 1950.0) } else { (0.55, 0.55, 2600.0) },
                        Place::Velar => (0.92, tune("velar_g2", 0.05), 600.0),
                        Place::Other => (0.68, 0.50, 1800.0),
                    };
                    seg_burst.s.fric_g1 = g1;
                    seg_burst.s.fric_g2 = g2;
                    seg_burst.s.fric_hp = low_cut;
                    seg_burst.s.fric_lp = if i > 0
                        && prev_base == Ph::S
                        && base == Ph::P
                        && next_base == Ph::R
                        && same_word_next
                    {
                        1750.0
                    } else if st.place == Place::Velar && cfg.gender == crate::config::Gender::Female {
                        clamp(1.72 * burst_f1, 0.0, 6500.0)
                    } else {
                        burst_ceiling(st.place, burst_f1)
                    };
                    // Lips already rounded for the next sound ("two", "tree", "twin") lengthen
                    // the cavity in front of the tongue, so the burst of /t d/ sits lower:
                    // measured 13% (a man) and 19% (a woman) lower than before unrounded sounds.
                    let round_base = tune("round_burst", 0.16);
                    let round_shift = if matches!(next_base, Ph::UW | Ph::UH | Ph::W | Ph::R | Ph::ER) {
                        round_base
                    } else if matches!(next_base, Ph::OW | Ph::OY) {
                        round_base * 0.45
                    } else {
                        0.0
                    };
                    if round_shift > 0.0 && same_word_next && matches!(base, Ph::T | Ph::D) {
                        let k = 1.0 - round_shift;
                        seg_burst.s.fric_f1 *= k;
                        seg_burst.s.fric_f2 *= k;
                        seg_burst.s.fric_hp *= k;
                        // ... and the tract behind the release is already the rounded one
                        seg_burst.s.f2 *= 1.0 - 0.5 * round_shift;
                        seg_burst.s.f3 *= 1.0 - 0.6 * round_shift;
                    }
                    seg_burst.s.jaw = st.jaw * 0.45 + v_target.jaw * 0.55;
                    seg_burst.s.t_tip = st.t_tip * 0.55 + v_target.t_tip * 0.45;
                    seg_burst.s.lip_r = st.lip_r.max(v_target.lip_r * 0.85);
                    seg_burst.s.lip_c = st.lip_c * 0.20;
                    seg_burst.shape = Shape::Burst;
                    seg_burst.clause_idx = ci_i;
                    seg_burst.phone = base;
                    seg_burst.word = item.word;
                    seg_burst.token = token;
                    segments.push(seg_burst);

                    // Voice-onset time: aspiration between the burst and the vowel.
                    let mut vot_dur = st.vot_ms * cluster_scale * clamp(rate_scale, 0.78, 1.22);
                    if !st.voiced && !stressed_onset && base != Ph::CH {
                        vot_dur = 14.0 * cluster_scale * clamp(rate_scale, 0.8, 1.2);
                    }
                    if intervocalic_alveolar || next_same_word_sonorant || next_is_obstruent_or_nasal {
                        vot_dur = 0.0;
                    } else if next_is_sonorant {
                        vot_dur = if st.voiced { 0.0 } else { 11.0 };
                    }
                    if i > 0 && prev_base == Ph::S && !st.voiced && next_base.is_vowel() {
                        vot_dur = 9.0;
                    }
                    if next_base == Ph::SIL {
                        vot_dur = if st.voiced { 6.0 } else { 16.0 };
                    }
                    if vot_dur >= 6.0 {
                        let mut seg_vot = Seg::from_spec(v_target);
                        seg_vot.s.dur = vot_dur;
                        let lab_w = if st.place == Place::Labial { 0.56 } else { 0.40 };
                        seg_vot.s.f1 = loc_f1 * 0.34 + v_target.f1 * 0.66;
                        seg_vot.s.f2 = loc_f2 * lab_w + v_target.f2 * (1.0 - lab_w);
                        seg_vot.s.f3 = loc_f3 * 0.34 + v_target.f3 * 0.66;
                        seg_vot.s.b1 = v_target.b1 + if st.voiced { 12.0 } else { 185.0 };
                        seg_vot.s.av = if st.voiced && next_base != Ph::SIL && !whisper {
                            0.88
                        } else {
                            0.0
                        };
                        seg_vot.s.ah = st.vot_ah
                            * crispness
                            * if stressed_onset { 1.0 } else { 0.55 }
                            * if next_base == Ph::SIL { 0.60 } else { 1.0 };
                        seg_vot.s.af = 0.0;
                        seg_vot.s.lip_r = v_target.lip_r;
                        seg_vot.s.lip_c = 0.0;
                        seg_vot.micro_f0 = if st.voiced { -0.01 } else { 0.025 };
                        seg_vot.clause_idx = ci_i;
                        seg_vot.phone = base;
                        seg_vot.word = item.word;
                        seg_vot.token = token;
                        segments.push(seg_vot);
                    }
                }
                // Fit closure + burst + aspiration into the stop's total duration; a tap
                // between vowels ("letter", "ladder") stays very short, and an unreleased first
                // stop in a same-word cluster ("actually", "kept", "doctor") shares closure with the next.
                let target_ms = if intervocalic_alveolar {
                    model_ms.min(36.0 * rate_scale)
                } else if next_is_stop && same_word_next {
                    model_ms.min(34.0 * rate_scale)
                } else {
                    model_ms
                };
                let built_ms: f64 = segments[first_seg..].iter().map(|s| s.s.dur).sum();
                for s in &mut segments[first_seg..] {
                    s.s.dur *= target_ms / built_ms;
                }
                continue;
            }

            // 3. Vowels, nasals, liquids, glides, fricatives
            let mut seg = Seg::from_spec(tables::spec(base).unwrap_or(&tables::SPEC_AX));
            // /m n ng/ were 4 dB (ng: 8) quieter against the vowels than in recordings of
            // people (tools/eval.py levels); 3 dB of that is made up here. The simulated
            // folds need no such help: their low end carries the murmur at its real level.
            if matches!(base, Ph::M | Ph::N | Ph::NG) && !cfg.folds {
                seg.s.av *= tune("nasal_level", 1.41);
            }
            if base == Ph::HH && next_base == Ph::Y {
                // Palatal [ç] before /j/ ("huge", "human"): high front cavity, voiceless, crisp.
                seg.s.f1 = 310.0;
                seg.s.f2 = 2250.0;
                seg.s.f3 = 2950.0;
                seg.s.stiff = 0.92;
                seg.breathy_h = 0.0;
            } else if base == Ph::HH && (prev_base.is_vowel() || matches!(prev_base, Ph::L | Ph::R | Ph::W | Ph::Y | Ph::M | Ph::N | Ph::NG)) {
                // Between voiced sounds the folds keep vibrating through an /h/: in recordings
                // its low band is within 5 dB of the vowels around it, while word-initial content
                // /h/ ("he", "heat", "head") drops voicing cleanly to mark the onset.
                if item.is_word_initial && item.is_content {
                    seg.breathy_h = 0.0;
                    seg.s.av = 0.14;
                } else {
                    seg.breathy_h = tune("h_voice", if cfg.gender == crate::config::Gender::Female { 0.0 } else { 0.25 });
                }
            }
            if let Some(vi) = crate::config::vowel_index(base) {
                let m = cfg.vowel_shape(vi);
                seg.s.f1 *= m[0];
                seg.s.f2 *= m[1];
                seg.s.f3 *= m[2];
                if base == Ph::AX && cfg.gender == crate::config::Gender::Female {
                    seg.s.f1 *= tune("ax_f1_f", 1.0);
                }
            }
            apply_vowel_context(base, next_base, cfg, &mut seg.s);
            if base == Ph::AA && next_base != Ph::R {
                seg.s.f3 *= 1.055;
            }
            if base == Ph::IH && prev_base == Ph::SIL && item.is_word_initial {
                // Utterance-initial "Is" / "It" / "In": keep close-front [ɪ] distinct from "as".
                seg.s.f1 *= 0.91;
                seg.s.f2 *= 1.045;
            }
            if base == Ph::R && prev_base.is_vowel() && !next_base.is_vowel() {
                // Post-vocalic coda /r/ ("rare", "there", "start", "corn"): vocalic [ɚ] offglide
                // rather than lip-rounded syllable-onset [ɹʷ] (whose F1=405, F2=1180 mimics /v/).
                seg.s.f1 = 465.0;
                seg.s.f2 = 1340.0;
                seg.s.f3 = 1680.0;
                seg.s.av = 0.96;
                seg.s.lip_r = 0.24;
            }
            if base == Ph::AX
                && item.is_word_initial
                && next_base == Ph::N
                && i + 2 < n_phones
                && flat[i + 2].base == Ph::D
                && flat[i + 2].word == item.word
            {
                // Weak-form "and" ([ɐnd]): keep F1 open enough that /n/'s 280 Hz locus doesn't raise it to "in".
                seg.s.f1 *= 1.16;
            }
            if base == Ph::UH && prev_base == Ph::Y && next_base == Ph::R {
                // CURE (/j uh r/: "curious", "curiously"): keep F1 open enough to stay distinct from /uw/.
                seg.s.f1 *= 1.08;
            }
            // "too", "do", "new", "soon": after a coronal consonant the tongue body is still
            // forward when /u/ begins, and glides back (skipped after /y/, which already glides
            // from 2320 Hz, and before dark /l/ as in "tool", "cool", "fool").
            if base == Ph::UW
                && next_base != Ph::L
                && matches!(prev_base, Ph::T | Ph::D | Ph::N | Ph::S | Ph::Z | Ph::SH | Ph::ZH | Ph::CH | Ph::JH | Ph::TH | Ph::DH)
            {
                let start = tune("uw_start", 1.35);
                if start > 1.0 {
                    let end = seg.s;
                    seg.s.f2 *= start;
                    seg.glide = Some(end);
                }
            }
            seg.s.dur = if base == Ph::W
                && matches!(prev_base, Ph::S | Ph::SH | Ph::Z | Ph::CH)
                && prev_same_word_cons
            {
                seg.s.f2 = 550.0;
                model_ms.max(66.0)
            } else if base == Ph::W {
                model_ms.max(if item.is_word_initial { 68.0 } else { 52.0 })
            } else if base == Ph::HH && next_base == Ph::Y {
                (model_ms * 0.72).max(28.0)
            } else if base == Ph::IH && next_base == Ph::R && item.stress >= 1 {
                model_ms.max(76.0)
            } else if base == Ph::EH && next_base == Ph::R && item.stress >= 1 {
                model_ms.max(92.0 * rate_scale)
            } else if base == Ph::R && prev_base.is_vowel() && !next_base.is_vowel() {
                (model_ms * 0.85).max(38.0)
            } else if base == Ph::IH && prev_base == Ph::SIL && item.is_word_initial {
                model_ms.max(66.0 * rate_scale)
            } else {
                model_ms.max(22.0)
            };
            seg.stress = item.stress;
            seg.accent_boost = accent_boost;
            seg.micro_f0 = micro_f0;
            seg.intrinsic_f0 = intrinsic_f0;
            seg.clause_idx = ci_i;
            seg.is_phrase_tail = is_phrase_tail_vowel;
            // Voiced fricatives give up their voicing before a pause and before voiceless
            // sounds (development hooks: the part that stays voiced).
            if matches!(base, Ph::Z | Ph::V | Ph::DH | Ph::ZH) {
                let next_voiceless = matches!(
                    next_base,
                    Ph::P | Ph::T | Ph::K | Ph::F | Ph::TH | Ph::S | Ph::SH | Ph::CH | Ph::HH
                );
                if next_base == Ph::SIL || i + 1 >= n_phones {
                    seg.voiced_part = tune("zv_final", 0.25);
                } else if next_voiceless {
                    seg.voiced_part = tune("zv_voiceless", 0.5);
                } else if matches!(base, Ph::Z | Ph::ZH) && !next_base.is_vowel() {
                    // ... and /z/ before any other consonant: in two readers its low band
                    // is 35 dB down from the middle of the sound on
                    seg.voiced_part = tune("zv_cons", 0.45);
                }
            }
            if base == Ph::DH {
                let after_vowel_glide = prev_base.is_vowel() || matches!(prev_base, Ph::L | Ph::W | Ph::Y);
                let after_nasal_r = matches!(prev_base, Ph::M | Ph::N | Ph::NG | Ph::R);
                if after_nasal_r {
                    seg.late_voice = tune("dh_son_late", 0.30);
                } else if !after_vowel_glide {
                    // After a pause or a consonant the "th" of "the", "that", "this" is not a
                    // buzzing fricative but a soft stop: two readers are silent for the
                    // first 60% of it (40 dB under their vowels), and the voice sets in
                    // only as the tongue leaves the teeth.
                    seg.late_voice = tune("dh_late", 0.6);
                }
            }
            if base == Ph::V {
                let v_def = if cfg.gender == crate::config::Gender::Female { 0.62 } else { 0.50 };
                seg.s.af *= tune("v_af", v_def);
                if next_base.is_vowel() {
                    let v_nx = peek_adjacent_vowel_spec(&flat, i);
                    seg.s.f2 = clamp(980.0 + (v_nx.f2 - 1100.0) * 0.42, 920.0, 1420.0);
                }
            }
            if base == Ph::F {
                seg.s.af *= tune("f_af", 0.75);
                if next_base.is_vowel() {
                    let v_nx = peek_adjacent_vowel_spec(&flat, i);
                    seg.s.f2 = clamp(980.0 + (v_nx.f2 - 1100.0) * 0.36, 920.0, 1380.0);
                }
            }

            // The coda [ɫ] ("male", "will", "bowls") and the /l/ before a vowel ("lazy",
            // "blue"). American English keeps both dark: at the middle of an /l/ before a
            // vowel two readers have F2 at 960 and 1230 Hz (a man and a woman; 920 and 1190
            // in the coda), not the 1400 of a clear European [l].
            if base == Ph::L {
                if !next_base.is_vowel() {
                    let (v_prev_l, _) = coarticulated_specs(&flat, i, cfg);
                    let l_co = tune("l_coart", 0.22);
                    seg.s.f1 = tune("l_coda_f1", 425.0);
                    seg.s.f2 = 880.0 * (1.0 - l_co) + clamp(v_prev_l.f2 * 0.72, 820.0, 1280.0) * l_co;
                    seg.s.f3 = tune("l_coda_f3", 2800.0);
                } else {
                    seg.s.f1 = tune("l_ons_f1", 395.0);
                    seg.s.f2 = tune("l_ons_f2", 1020.0);
                    seg.s.f3 = tune("l_ons_f3", 2860.0);
                }
                let l_default = if cfg.gender == crate::config::Gender::Female { 1.06 } else { 1.0 };
                seg.s.av *= tune("l_av", l_default);
            }
            if base == Ph::DH {
                seg.s.af *= tune("dh_af", 0.85);
            }

            // Sonorants are partly devoiced after voiceless stops in the same syllable onset ("please", "quick").
            if is_approximant(base)
                && matches!(prev_base, Ph::P | Ph::T | Ph::K)
                && prev_same_word_cons
                && !(i >= 2 && flat[i - 2].base == Ph::S)
            {
                seg.onset_devoice = true;
                seg.s.dur = (seg.s.dur * 1.08).max(50.0);
            }

            if base_is_vowel {
                seg.s.av = (seg.s.av * effort_gain).min(1.2);
                seg.shape = Shape::Vowel;
            } else if seg.s.af > 0.02 {
                seg.s.af *= crispness;
                seg.shape = Shape::Fric;
            }

            // Loosely articulated phones take on the colour of the neighbouring vowel.
            let stiff = if base == Ph::AX {
                tune("ax_stiff", 0.70)
            } else if matches!(base, Ph::F | Ph::V) {
                tune("fv_stiff", 0.78)
            } else {
                seg.s.stiff
            };
            if stiff < 0.75 {
                let v_adj2 = peek_adjacent_vowel_spec(&flat, i);
                let blend = 1.0 - stiff;
                seg.s.f1 = seg.s.f1 * stiff + v_adj2.f1 * blend;
                seg.s.f2 = seg.s.f2 * stiff + v_adj2.f2 * blend;
                seg.s.f3 = seg.s.f3 * stiff + v_adj2.f3 * blend;
                seg.s.jaw = seg.s.jaw * stiff + v_adj2.jaw * blend;
                seg.s.t_h = seg.s.t_h * stiff + v_adj2.t_h * blend;
                seg.s.t_f = seg.s.t_f * stiff + v_adj2.t_f * blend;
                seg.s.lip_r = seg.s.lip_r * stiff + v_adj2.lip_r * blend;
                seg.s.lip_s = seg.s.lip_s * stiff + v_adj2.lip_s * blend;
            }

            // Lip rounding of the neighbouring vowel darkens fricative noise ("sue" vs "see")
            // beyond any rounding the fricative already has of its own (/sh/).
            if seg.s.af > 0.02 {
                let v_round = peek_adjacent_vowel_spec(&flat, i).lip_r;
                let lip_darken = 1.0 - 0.16 * (v_round - seg.s.lip_r).max(0.0);
                seg.s.fric_f1 *= lip_darken;
                seg.s.fric_f2 *= lip_darken;
            }

            // Nasal consonants: neighbouring vowels move toward the locus of the mouth closure.
            let nasal_place = match base {
                Ph::M => Some(Place::Labial),
                Ph::N => Some(Place::Alveolar),
                Ph::NG => Some(Place::Velar),
                _ => None,
            };
            if let Some(place) = nasal_place {
                let (v_close_nas, _) = coarticulated_specs(&flat, i, cfg);
                let (mut l2, mut l3) = place_locus(place, &v_close_nas, (seg.s.f2, seg.s.f3));
                match place {
                    Place::Labial => l3 = 2250.0,
                    Place::Alveolar => {
                        l2 = clamp(1820.0 + (v_close_nas.f2 - 1700.0) * 0.28, 1620.0, 2080.0);
                        l3 = 2650.0;
                    }
                    Place::Velar => {
                        // Velar pinch before /ng/ ("sing", "gang", "coming"): F2 rises to meet F3.
                        l2 = clamp(v_close_nas.f2 * 0.90 + 300.0, 1380.0, 2260.0);
                        l3 = clamp(l2 + 160.0, 2220.0, 2440.0);
                    }
                    Place::Other => {}
                }
                seg.locus = Some([280.0, l2, l3]);
            }

            // Anticipatory velum lowering: vowels before /m n ng/ are lightly nasalized
            // (less on open /ae aa/ so the 450 Hz nasal zero does not pull F1 down to /eh/).
            if base_is_vowel && matches!(next_base, Ph::M | Ph::N | Ph::NG) {
                let nas_amt = if matches!(base, Ph::AE | Ph::AA) {
                    tune("open_nasal", 0.10)
                } else {
                    0.24
                };
                seg.s.nasal = seg.s.nasal.max(nas_amt);
                seg.s.oral_seal = 0.0;
                seg.s.velum = seg.s.velum.max(0.38);
            }

            seg.soft_onset = prev_base == Ph::SIL
                && (base_is_vowel || matches!(base, Ph::L | Ph::R | Ph::W | Ph::Y | Ph::M | Ph::N));
            seg.hiatus_onset = item.is_word_initial && base_is_vowel && item.stress >= 1 && prev_base.is_vowel();
            seg.word_onset = word_juncture;
            seg.phone = base;
            seg.word = item.word;
            seg.token = token;
            segments.push(seg);
        }

        // Pause between clauses, plus a soft breath intake before a new sentence.
        let is_sentence_break = matches!(clause.punc, '.' | '!' | '?') && ci + 1 < n_clauses;
        // People run on through many of their commas: of two readers measured, one paused
        // at 6% of them and the other at 55% (for about 100 ms). The comma is still marked
        // by pitch and lengthening. With `variation` off every comma gets its pause.
        let run_on = clause.punc == ','
            && ci + 1 < n_clauses
            && (clause.words.len() >= 5 || clause.words.len() <= 2)
            && variation > 0.0
            && rng.uniform() < tune("comma_run_on", 0.9) * variation.min(0.5);
        let pause_ms = if clause.punc == ',' {
            if run_on { 55.0 } else { 105.0 }
        } else if clause.punc == ';' || clause.punc == ':' {
            140.0
        } else if ci == n_clauses - 1 {
            95.0
        } else {
            150.0
        };

        let mut sil_end = Seg::silence();
        // The pause after the very last word is only a tail of silence; the others are the
        // speaker's own and follow the `pauses` setting.
        let pause_scale = if ci + 1 < n_clauses { cfg.pauses } else { 1.0 };
        let total_pause_ms = if is_sentence_break { tune("sentence_pause_ms", 400.0) } else { pause_ms }
            * pause_scale
            * clamp(rate_scale, 0.82, 1.18)
            * rng.spread(tune("var_pause", 0.25) * variation);
        let breaths = cfg.breaths;
        let has_breath = is_sentence_break && !whisper && breaths > 0.01;
        let pre_phon_ms = if has_breath { (total_pause_ms * 0.14).clamp(35.0, 60.0) } else { 0.0 };
        if is_sentence_break || pause_ms > 1.0 {
            sil_end.s.dur = (total_pause_ms - pre_phon_ms).max(20.0);
            segments.push(sil_end);
        }

        // Breath intake before the next sentence: relax -> soft inhale -> brief glottal closure
        // before phonation begins.
        if has_breath {
            let mut breath = Seg::silence();
            breath.s.dur = 70.0 + (breaths - 0.25).max(0.0) * 435.0;
            breath.s.f1 = 510.0;
            breath.s.f2 = 1490.0;
            breath.s.f3 = 2480.0;
            breath.s.b1 = 260.0;
            breath.s.b2 = 220.0;
            breath.s.ah = 0.055 * (breaths / 0.25).powf(0.9);
            breath.s.jaw = 0.18;
            segments.push(breath);

            let mut sil_adduct = Seg::silence();
            sil_adduct.s.dur = pre_phon_ms;
            segments.push(sil_adduct);
        }
    }

    // Silent gaps hold the formants of the neighbouring speech so the tract never jumps.
    for si in 0..segments.len() {
        if segments[si].phone == Ph::SIL && segments[si].s.ah < 0.01 {
            let reference = if si + 1 < segments.len() && segments[si + 1].phone != Ph::SIL {
                Some(si + 1)
            } else if si > 0 && segments[si - 1].phone != Ph::SIL {
                Some(si - 1)
            } else {
                None
            };
            if let Some(ri) = reference {
                let r = &segments[ri];
                // after a diphthong, hold where its glide ended rather than where it began
                let (f1, f2, f3) = match (&r.glide, ri < si) {
                    (Some(end), true) => (
                        r.s.f1 + (end.f1 - r.s.f1) * 0.94,
                        r.s.f2 + (end.f2 - r.s.f2) * 0.94,
                        r.s.f3 + (end.f3 - r.s.f3) * 0.94,
                    ),
                    _ => (r.s.f1, r.s.f2, r.s.f3),
                };
                segments[si].s.f1 = f1;
                segments[si].s.f2 = f2;
                segments[si].s.f3 = f3;
            }
        }
    }

    #[cfg(feature = "dev-hooks")]
    if let Ok(path) = std::env::var("VS_DUR_FILE") {
        apply_duration_overrides(&mut segments, &path);
    }
    // Development hook: `VS_FEATURES_FILE` appends one line per phone with the context
    // features the duration rules see, plus the duration they produced (tools/fit_durations.py).
    #[cfg(feature = "dev-hooks")]
    if let Ok(path) = std::env::var("VS_FEATURES_FILE") {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            for line in &feature_lines {
                let tok: i32 = line.split(' ').next().and_then(|t| t.parse().ok()).unwrap_or(-1);
                let ms: f64 = segments.iter().filter(|s| s.token == tok).map(|s| s.s.dur).sum();
                let _ = writeln!(f, "{line} {ms:.1}");
            }
            let _ = writeln!(f, "END");
        }
    }

    Timeline {
        segments,
        clauses: clause_meta,
    }
}

/// Development hook: `VS_DUR_FILE` names a text file of `token milliseconds` lines that force
/// the duration of individual phones (used by tools/eval.py to transplant a real speaker's
/// timing onto the synthesizer).
#[cfg(feature = "dev-hooks")]
fn apply_duration_overrides(segments: &mut [Seg], path: &str) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    for line in text.lines() {
        let mut it = line.split_whitespace();
        let (Some(tok), Some(ms)) = (it.next(), it.next()) else {
            continue;
        };
        let (Ok(tok), Ok(ms)) = (tok.parse::<i32>(), ms.parse::<f64>()) else {
            continue;
        };
        let total: f64 = segments.iter().filter(|s| s.token == tok).map(|s| s.s.dur).sum();
        if total > 0.0 && ms > 0.0 {
            for s in segments.iter_mut().filter(|s| s.token == tok) {
                s.s.dur *= ms / total;
            }
        }
    }
}
