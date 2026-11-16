//! The vocal-tract synthesizer: turns control frames into audio.
//!
//! Signal path per sample:
//!  1. Glottal source — solved Liljencrants-Fant (LF) flow-derivative wavetables
//!     (modal / warm / lax closures) blended by an open-quotient control, with flutter,
//!     vibrato, jitter, shimmer and optional sub-harmonic creak.
//!  2. Turbulence — warm aspiration (/h/, voice-onset time) and pitch-synchronous
//!     glottal-chink air (harmonic + noise model).
//!  3. Nasal coupling — a nasal pole-zero pair ahead of the oral formants (murmurs of
//!     /m n ng/ and nasalized vowels).
//!  4. Oral tract — F1..F5 cascade with open-phase F1 damping, a piriform-fossa notch,
//!     lip radiation and a parallel F2/F3 presence tap, then a soft tissue roll-off.
//!  5. Chest body resonator, voice bar, and frication / burst noise shaped at the lips.

use crate::config::{Gender, VoiceConfig, TONE_BANDS, TONE_BANDS_HZ};
use crate::folds::{Drive, Folds, Larynx, Voicing};
use crate::frames::Frame;
use crate::phone::Ph;
use crate::timeline::clamp;
use std::f64::consts::{PI, TAU};
use std::sync::{Arc, Mutex, OnceLock};

pub const SAMPLE_RATE: u32 = 24_000;
/// Samples per 5 ms control frame.
pub const SAMPLES_PER_FRAME: usize = 120;
/// Filter coefficients are refreshed 4 times per frame (every 1.25 ms).
const SUBSTEP_SAMPLES: usize = 30;

/// Q of the plain second-order high-passes: 1/sqrt(2) as it was first typed in, kept to
/// the digit because every coefficient - and so the sound - hangs on it.
#[allow(clippy::approx_constant)]
const HIGH_PASS_Q: f64 = 0.7071;
const UG_SIZE: usize = 4096;
/// Level of the voice bar (voicing heard through a closed mouth), about 17 dB below vowels.
const VOICE_BAR_MIX: f64 = 0.06;
const UG_MASK: usize = UG_SIZE - 1;

/// Harmonic counts of the band-limited copies of each glottal pulse. At render time the
/// richest copy whose top harmonic stays below Nyquist is used, so the source never aliases.
/// The pitch stays within 55-520 Hz, which needs between 200 and 21 harmonics: richer or
/// poorer copies would never be played.
const MIP_HARMONICS: [usize; 10] = [161, 128, 102, 81, 64, 51, 40, 32, 25, 20];
const MIP_LEVELS: usize = MIP_HARMONICS.len();

struct GlottalTables {
    /// `MIP_LEVELS` consecutive tables of `UG_SIZE` samples each.
    eg_warm: Vec<f32>,
    eg_lax: Vec<f32>,
    ug_body: Vec<f32>,
}

/// The main ("modal") glottal pulse of a voice: its flow derivative, band-limited for every
/// pitch range, and how far the glottis is open through the cycle.
struct ModalTable {
    /// `MIP_LEVELS` consecutive tables of `UG_SIZE` samples each.
    eg_modal: Vec<f32>,
    ug_env: Vec<f32>,
}

/// Open quotients of the pulse bank: 0.50, 0.51 ... 0.80.
const OQ_MIN: f64 = 0.50;
const OQ_STEPS: usize = 31;

/// Steps of the `closure` setting (0, 0.05 ... 1).
const CLOSURE_STEPS: usize = 21;

/// Return phase of the modal pulse for a `closure` setting: how long the folds take to shut,
/// as a share of the cycle. The shorter, the more abruptly the airflow stops and the further
/// up the harmonics reach (the spectrum bends down above about f0 / (2 pi return)).
fn closure_return(step: usize) -> f64 {
    0.030 - 0.024 * step as f64 / (CLOSURE_STEPS - 1) as f64
}

/// How many closing sharpnesses are kept ready at once (a table is 180 KB, a full row
/// 5.6 MB).
const ROWS_KEPT: usize = 3;

/// The modal pulses for one closing sharpness: one per open quotient, built on first use. A
/// voice does not have one fixed pulse: like a larynx, the renderer moves through
/// neighbouring pulses from cycle to cycle as pitch and effort change.
struct PulseRow {
    closure: usize,
    tables: [OnceLock<ModalTable>; OQ_STEPS],
}

impl PulseRow {
    fn table(&self, index: usize) -> &ModalTable {
        let index = index.min(OQ_STEPS - 1);
        self.tables[index].get_or_init(|| {
            let mut eg_modal = vec![0.0f32; UG_SIZE];
            let mut ug_env = vec![0.0f32; UG_SIZE];
            build_solved_lf_tables(&mut eg_modal, Some(&mut ug_env), 0.43, OQ_MIN + index as f64 * 0.01, closure_return(self.closure));
            ModalTable { eg_modal: mip_chain(&eg_modal), ug_env }
        })
    }
}

/// The pulses for a `closure` setting. Voices with the same setting share them; only the
/// rows asked for most recently are kept, so that trying every value of the setting does not
/// leave every row in memory (an older row goes with the last voice that uses it).
fn pulse_row(closure: usize) -> Arc<PulseRow> {
    static RECENT: Mutex<Vec<Arc<PulseRow>>> = Mutex::new(Vec::new());
    let closure = closure.min(CLOSURE_STEPS - 1);
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = recent.iter().position(|r| r.closure == closure) {
        let row = recent.remove(i);
        recent.push(row.clone());
        return row;
    }
    if recent.len() >= ROWS_KEPT {
        recent.remove(0);
    }
    let row = Arc::new(PulseRow { closure, tables: [const { OnceLock::new() }; OQ_STEPS] });
    recent.push(row.clone());
    row
}

/// Bank index of an open quotient.
fn oq_index(open_quotient: f64) -> usize {
    (((open_quotient - OQ_MIN) * 100.0).round().max(0.0) as usize).min(OQ_STEPS - 1)
}

/// Resonators above the articulated formants (the `air` setting).
const UPPER_MAX: usize = 6;

/// In-place radix-2 FFT (`inverse` leaves the result unscaled).
fn fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = if inverse { TAU } else { -TAU } / len as f64;
        let (w_re, w_im) = (ang.cos(), ang.sin());
        let mut start = 0;
        while start < n {
            let (mut c_re, mut c_im) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let t_re = re[b] * c_re - im[b] * c_im;
                let t_im = re[b] * c_im + im[b] * c_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let n_re = c_re * w_re - c_im * w_im;
                c_im = c_re * w_im + c_im * w_re;
                c_re = n_re;
            }
            start += len;
        }
        len <<= 1;
    }
}

/// Returns the band-limited copies of a table (see `MIP_HARMONICS`), one after another.
fn mip_chain(table: &[f32]) -> Vec<f32> {
    let n = table.len();
    let mut spec_re: Vec<f64> = table.iter().map(|&v| v as f64).collect();
    let mut spec_im = vec![0.0f64; n];
    fft(&mut spec_re, &mut spec_im, false);

    let mut out = Vec::with_capacity(n * MIP_LEVELS);
    for &harmonics in MIP_HARMONICS.iter() {
        let mut re = vec![0.0f64; n];
        let mut im = vec![0.0f64; n];
        // Raised-cosine taper over the top 15% of the kept harmonics avoids Gibbs ringing.
        let taper_start = harmonics as f64 * 0.85;
        for k in 1..=harmonics.min(n / 2 - 1) {
            let g = if (k as f64) <= taper_start {
                1.0
            } else {
                0.5 * (1.0 + (PI * (k as f64 - taper_start) / (harmonics as f64 - taper_start)).cos())
            };
            re[k] = spec_re[k] * g;
            im[k] = spec_im[k] * g;
            re[n - k] = spec_re[n - k] * g;
            im[n - k] = spec_im[n - k] * g;
        }
        fft(&mut re, &mut im, true);
        out.extend(re.iter().map(|&v| (v / n as f64) as f32));
    }
    out
}

/// One period of the equation pulse (the rate of change of the flow) for an open quotient
/// and a `closure` setting, as the default voices mix it with the warm pulse: for
/// comparing the two kinds of source (development tooling).
#[doc(hidden)]
pub fn equation_pulse(open_quotient: f64, closure: f64, warm: f64) -> Vec<f32> {
    let mut modal = vec![0.0f32; UG_SIZE];
    let step = (clamp(closure, 0.0, 1.0) * (CLOSURE_STEPS - 1) as f64).round() as usize;
    build_solved_lf_tables(&mut modal, None, 0.43, open_quotient, closure_return(step));
    let mut soft = vec![0.0f32; UG_SIZE];
    build_solved_lf_tables(&mut soft, None, 0.46, 0.64, 0.044);
    let rms = |x: &[f32]| (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len() as f64).sqrt().max(1e-9);
    let (rm, rs) = (rms(&modal), rms(&soft));
    modal.iter().zip(&soft).map(|(&m, &w)| ((1.0 - warm) * m as f64 / rm + warm * w as f64 / rs) as f32).collect()
}

/// Fills `deriv` with one period of the LF glottal flow derivative E(u) and, optionally,
/// `flow` with the normalized glottal flow envelope.
///
/// The growth parameter is solved by bisection so the period integrates to zero (no DC
/// drift), and the closure corner is smoothed the way the vocal-fold mucosa closes
/// progressively rather than snapping shut — which removes the harsh "buzz" of a naive pulse.
fn build_solved_lf_tables(deriv: &mut [f32], mut flow: Option<&mut [f32]>, tp: f64, te: f64, ta: f64) {
    let eps = 1.0 / ta.max(0.006);
    let span_ret = (1.0 - te).max(0.01);
    let ret_exp = (-eps * span_ret).exp();
    let ret_denom = (1.0 - ret_exp).max(1e-6);
    let mut ret_area = (1.0 / eps) * (1.0 - ret_exp) - span_ret * ret_exp;
    ret_area /= ret_denom;

    let omega_g = PI / tp;
    let sin_te = (omega_g * te).sin();
    let cos_te = (omega_g * te).cos();
    let mut lo = -12.0f64;
    let mut hi = 28.0f64;
    for _ in 0..22 {
        let mid = 0.5 * (lo + hi);
        let e0 = -1.0 / ((mid * te).exp() * sin_te);
        let open_area = e0
            * (((mid * te).exp() * (mid * sin_te - omega_g * cos_te) + omega_g)
                / (mid * mid + omega_g * omega_g));
        if open_area > ret_area {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let alpha = 0.5 * (lo + hi);
    let e0_final = -1.0 / ((alpha * te).exp() * sin_te);

    let mut mean_e = 0.0f64;
    let mut flow_acc = 0.0f64;
    let mut max_flow = 1e-6f64;
    let du = 1.0 / UG_SIZE as f64;

    for i in 0..UG_SIZE {
        let u = (i as f64 + 0.5) * du;
        let e_val = if u < te {
            e0_final * (alpha * u).exp() * (omega_g * u).sin()
        } else {
            -((-eps * (u - te)).exp() - ret_exp) / ret_denom
        };
        deriv[i] = e_val as f32;
        mean_e += e_val;
        if let Some(f) = flow.as_deref_mut() {
            flow_acc = (flow_acc + e_val * du).max(0.0);
            f[i] = flow_acc as f32;
            if flow_acc > max_flow {
                max_flow = flow_acc;
            }
        }
    }

    mean_e /= UG_SIZE as f64;
    for d in deriv.iter_mut() {
        *d = (*d as f64 - mean_e) as f32;
    }

    // Raised-cosine smoothing over ±16 table points (~0.4% of the period).
    let k_rad: i32 = 16;
    let mut smooth = vec![0.0f32; UG_SIZE];
    for (i, out) in smooth.iter_mut().enumerate() {
        let mut acc = 0.0f64;
        let mut w_sum = 0.0f64;
        for d in -k_rad..=k_rad {
            let w = 0.5 * (1.0 + ((PI * d as f64) / (k_rad + 1) as f64).cos());
            let idx = ((i as i32 + d) & UG_MASK as i32) as usize;
            acc += deriv[idx] as f64 * w;
            w_sum += w;
        }
        *out = (acc / w_sum) as f32;
    }

    let mut sq_sum = 0.0f64;
    for i in 0..UG_SIZE {
        let d = smooth[i] as f64;
        deriv[i] = smooth[i];
        sq_sum += d * d;
    }
    let inv_rms = 1.0 / (sq_sum / UG_SIZE as f64).sqrt().max(1e-6);
    for d in deriv.iter_mut() {
        *d = (*d as f64 * inv_rms) as f32;
    }

    if let Some(f) = flow {
        let inv_max = 1.0 / max_flow;
        for v in f.iter_mut() {
            *v = (*v as f64 * inv_max) as f32;
        }
    }
}

fn glottal_tables() -> &'static GlottalTables {
    static TABLES: OnceLock<GlottalTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut eg_warm = vec![0.0f32; UG_SIZE];
        let mut eg_lax = vec![0.0f32; UG_SIZE];
        let mut warm_flow = vec![0.0f32; UG_SIZE];

        // Warm conversational closure / soft breathy closure (the clear modal pulse depends
        // on the voice: see `modal_table`).
        build_solved_lf_tables(&mut eg_warm, Some(&mut warm_flow), 0.46, 0.64, crate::tune("warm_return", 0.044));
        build_solved_lf_tables(&mut eg_lax, None, 0.49, 0.71, 0.075);

        // Demeaned, RMS-normalized flow pulse that drives the chest resonator.
        let mean_body = warm_flow.iter().map(|&v| v as f64).sum::<f64>() / UG_SIZE as f64;
        let mut ug_body = vec![0.0f32; UG_SIZE];
        let mut body_sq = 0.0f64;
        for i in 0..UG_SIZE {
            let b = warm_flow[i] as f64 - mean_body;
            ug_body[i] = b as f32;
            body_sq += b * b;
        }
        let inv_body_rms = 1.0 / (body_sq / UG_SIZE as f64).sqrt().max(1e-6);
        for v in ug_body.iter_mut() {
            *v = (*v as f64 * inv_body_rms) as f32;
        }

        GlottalTables {
            eg_warm: mip_chain(&eg_warm),
            eg_lax: mip_chain(&eg_lax),
            ug_body,
        }
    })
}

/// Length of the room's memory in half-rate samples (0.34 s at 12 kHz).
const ROOM_LEN: usize = 4096;

/// The room's echo pattern: sparse, randomly signed reflections under an exponential decay
/// ("velvet noise"), as (delay in half-rate samples, gain). The same for every voice.
fn room_taps() -> &'static [(u16, f32)] {
    static TAPS: OnceLock<Vec<(u16, f32)>> = OnceLock::new();
    TAPS.get_or_init(|| {
        let rate = SAMPLE_RATE as f64 / 2.0;
        let rt60 = crate::tune("room_rt60", 0.25);
        let spacing = crate::tune("room_spacing", 20.0); // one reflection per 20 half-rate samples: 600 a second
        let mut rng = 0x2545_f491u32;
        let mut taps = Vec::new();
        let mut energy = 0.0f64;
        let mut k = 0.004 * rate; // nothing in the first 4 ms
        while k + spacing < (0.30 * rate).min((ROOM_LEN - 2) as f64) {
            rng = xorshift(rng);
            let delay = k + (rng & 0xffff) as f64 / 65536.0 * spacing;
            rng = xorshift(rng);
            let sign = if rng & 0x8000 == 0 { 1.0 } else { -1.0 };
            let gain = sign * (-6.91 * delay / rate / rt60).exp();
            energy += gain * gain;
            taps.push((delay as u16, gain as f32));
            k += spacing;
        }
        let norm = 1.0 / energy.sqrt().max(1e-9);
        for t in taps.iter_mut() {
            t.1 = (t.1 as f64 * norm) as f32;
        }
        taps
    })
}

/// A small room. Every real recording carries a faint, diffuse tail of what was just said;
/// without it synthetic speech is bone-dry and its pauses are digital silence. Measured on
/// sentences never used for tuning, a quarter-second tail well below the voice raises the
/// predicted naturalness and leaves the phoneme error rate where it was. (A network of
/// feedback delay lines was tried first and measured worse than no room at all: the tail has
/// to be as featureless as noise.) The tail is computed at half the sample rate - walls absorb
/// the top end anyway - which keeps it cheap.
struct Room {
    buf: Box<[f32; ROOM_LEN]>,
    taps: &'static [(u16, f32)],
    pos: usize,
    wet: f64,
    first: f64,
    second_half: bool,
    prev: f64,
    /// Running level of the tail (to know when it has died away).
    level: f64,
}

impl Room {
    /// `amount`: 0 = dry, 0.25 = the default (tail 24 dB below the voice), 0.5 = 18 dB
    /// below, 1 = 12 dB below.
    fn new(amount: f64) -> Room {
        Room {
            buf: Box::new([0.0; ROOM_LEN]),
            taps: room_taps(),
            pos: 0,
            wet: amount * 2.0 * 10f64.powf(-18.0 / 20.0),
            first: 0.0,
            second_half: false,
            prev: 0.0,
            level: 0.0,
        }
    }

    /// One sample in, the room's tail out (the direct sound is not included).
    #[inline]
    fn process(&mut self, x: f64) -> f64 {
        if !self.second_half {
            self.first = x;
            self.second_half = true;
            return self.prev * self.wet;
        }
        self.second_half = false;
        self.pos = (self.pos + 1) & (ROOM_LEN - 1);
        self.buf[self.pos] = ((self.first + x) * 0.5) as f32;
        let mut tail = 0.0f32;
        // (the masked index of a fixed-size buffer needs no bounds check: this loop is half
        // of the renderer's time)
        for &(delay, gain) in self.taps {
            tail += self.buf[(self.pos + ROOM_LEN - delay as usize) & (ROOM_LEN - 1)] * gain;
        }
        let tail = tail as f64;
        let out = (self.prev + tail) * 0.5 * self.wet;
        self.prev = tail;
        self.level = self.level * 0.998 + tail.abs() * 0.002;
        out
    }
}

/// Running filter / oscillator state, carried across calls so audio can be streamed gaplessly.
#[derive(Clone, Copy)]
struct State {
    glot_phase: f64,
    sub_harm_toggle: f64,
    /// This glottal cycle's own deviation in length and in strength (drawn anew every cycle).
    cyc_jit: f64,
    cyc_shim: f64,
    /// Memory of the closing-edge shelf on the voiced source (x1, x2, y1, y2).
    edge: [f64; 4],
    /// One-pole spectral tilt state on the voiced glottal source.
    tilt_y: f64,
    /// The simulated folds (the `folds` setting) and the running mean of their flow.
    fold: Voicing,
    fold_flow_mean: f64,
    flt_phase1: f64,
    flt_phase2: f64,
    flt_phase3: f64,
    vib_phase: f64,
    jitter_walk: f64,
    shimmer_walk: f64,
    rng: u32,
    nm1_1: f64,
    nm1_2: f64,
    nz_x1: f64,
    nz_x2: f64,
    y1_1: f64,
    y1_2: f64,
    y2_1: f64,
    y2_2: f64,
    y3_1: f64,
    y3_2: f64,
    y4_1: f64,
    y4_2: f64,
    y5_1: f64,
    y5_2: f64,
    y6_1: f64,
    /// The resonators above the articulated formants.
    up_y1: [f64; UPPER_MAX],
    up_y2: [f64; UPPER_MAX],
    /// High-pass (four biquads) that keeps only the top of what the upper resonators add.
    up_hp: [f64; 16],
    /// High-pass state of the breath noise in the top octave.
    top_x1: f64,
    top_x2: f64,
    top_y1: f64,
    top_y2: f64,
    y6_2: f64,
    p2_1: f64,
    p2_2: f64,
    p3_1: f64,
    p3_2: f64,
    pir_x1: f64,
    pir_x2: f64,
    pir_y1: f64,
    pir_y2: f64,
    muc1: f64,
    muc2: f64,
    vb1: f64,
    vb2: f64,
    ch1: f64,
    ch2: f64,
    fr1_x1: f64,
    fr1_x2: f64,
    fr1_y1: f64,
    fr1_y2: f64,
    fr2_x1: f64,
    fr2_x2: f64,
    fr2_y1: f64,
    fr2_y2: f64,
    asp_lp: f64,
    asp_lp2: f64,
    hp_x: [f64; 4],
    hp_y: [f64; 4],
    hnm_prev: f64,
    hnm_lp: f64,
    prev_par_in: f64,
    prev_oral: f64,
    room_idx: usize,
    eq_z1: [f64; TONE_BANDS],
    eq_z2: [f64; TONE_BANDS],
    /// Whether the previous frame was a stop burst (to find the instant of release).
    prev_burst: bool,
    /// Which pulse of the open-quotient bank the current glottal cycle uses.
    oq_idx: usize,
    /// Subglottal (windpipe) pole-zero pair.
    tr_x1: f64,
    tr_x2: f64,
    tr_y1: f64,
    tr_y2: f64,
    dc_x1: f64,
    dc_y1: f64,
}

impl State {
    fn new() -> State {
        State {
            glot_phase: 0.0,
            sub_harm_toggle: 1.0,
            cyc_jit: 0.0,
            cyc_shim: 0.0,
            edge: [0.0; 4],
            tilt_y: 0.0,
            fold: Voicing::default(),
            fold_flow_mean: 0.0,
            flt_phase1: 0.0,
            flt_phase2: 0.27,
            flt_phase3: 0.61,
            vib_phase: 0.0,
            jitter_walk: 0.0,
            shimmer_walk: 0.0,
            rng: 0x564F_4943,
            nm1_1: 0.0,
            nm1_2: 0.0,
            nz_x1: 0.0,
            nz_x2: 0.0,
            y1_1: 0.0,
            y1_2: 0.0,
            y2_1: 0.0,
            y2_2: 0.0,
            y3_1: 0.0,
            y3_2: 0.0,
            y4_1: 0.0,
            y4_2: 0.0,
            y5_1: 0.0,
            y5_2: 0.0,
            y6_1: 0.0,
            up_y1: [0.0; UPPER_MAX],
            up_y2: [0.0; UPPER_MAX],
            up_hp: [0.0; 16],
            top_x1: 0.0,
            top_x2: 0.0,
            top_y1: 0.0,
            top_y2: 0.0,
            y6_2: 0.0,
            p2_1: 0.0,
            p2_2: 0.0,
            p3_1: 0.0,
            p3_2: 0.0,
            pir_x1: 0.0,
            pir_x2: 0.0,
            pir_y1: 0.0,
            pir_y2: 0.0,
            muc1: 0.0,
            muc2: 0.0,
            vb1: 0.0,
            vb2: 0.0,
            ch1: 0.0,
            ch2: 0.0,
            fr1_x1: 0.0,
            fr1_x2: 0.0,
            fr1_y1: 0.0,
            fr1_y2: 0.0,
            fr2_x1: 0.0,
            fr2_x2: 0.0,
            fr2_y1: 0.0,
            fr2_y2: 0.0,
            asp_lp: 0.0,
            asp_lp2: 0.0,
            hp_x: [0.0; 4],
            hp_y: [0.0; 4],
            hnm_prev: 0.0,
            hnm_lp: 0.0,
            prev_par_in: 0.0,
            prev_oral: 0.0,
            room_idx: 0,
            eq_z1: [0.0; TONE_BANDS],
            eq_z2: [0.0; TONE_BANDS],
            prev_burst: false,
            oq_idx: 16,
            tr_x1: 0.0,
            tr_x2: 0.0,
            tr_y1: 0.0,
            tr_y2: 0.0,
            dc_x1: 0.0,
            dc_y1: 0.0,
        }
    }
}

/// One band of the tone equalizer (RBJ cookbook shelf / peaking filter), as
/// `[b0, b1, b2, a1, a2]` normalised by a0.
fn tone_band(index: usize, gain_db: f64) -> [f64; 5] {
    let fs = SAMPLE_RATE as f64;
    let a = 10f64.powf(gain_db / 40.0);
    let last = TONE_BANDS - 1;
    // shelves at the ends, bells sized to reach their neighbours in between
    let (f, q) = if index == 0 {
        (180.0, 0.707)
    } else if index == last {
        (6800.0, 0.707)
    } else {
        let width = (TONE_BANDS_HZ[index + 1] - TONE_BANDS_HZ[index - 1]) / 2.0;
        (TONE_BANDS_HZ[index], (TONE_BANDS_HZ[index] / width * 0.75).max(0.7))
    };
    let w0 = TAU * f / fs;
    let (sin_w, cos_w) = (w0.sin(), w0.cos());
    let alpha = sin_w / (2.0 * q);
    let (b0, b1, b2, a0, a1, a2);
    if index == 0 || index == last {
        let s = if index == 0 { 1.0 } else { -1.0 }; // low shelf / high shelf
        let k = 2.0 * a.sqrt() * alpha;
        b0 = a * ((a + 1.0) - s * (a - 1.0) * cos_w + k);
        b1 = s * 2.0 * a * ((a - 1.0) - s * (a + 1.0) * cos_w);
        b2 = a * ((a + 1.0) - s * (a - 1.0) * cos_w - k);
        a0 = (a + 1.0) + s * (a - 1.0) * cos_w + k;
        a1 = -s * 2.0 * ((a - 1.0) + s * (a + 1.0) * cos_w);
        a2 = (a + 1.0) + s * (a - 1.0) * cos_w - k;
    } else {
        b0 = 1.0 + alpha * a;
        b1 = -2.0 * cos_w;
        b2 = 1.0 - alpha * a;
        a0 = 1.0 + alpha / a;
        a1 = -2.0 * cos_w;
        a2 = 1.0 - alpha / a;
    }
    [b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0]
}

/// The simulated folds for a voice: its settings turned into what a speaker does with a
/// larynx. The open quotient becomes how far apart the folds are held, `closure` and
/// `warmth` how much they close like a zip instead of all at once, breathiness a gap that
/// stays open. The larynxes asked for most recently are kept (calibrating one takes a
/// couple of milliseconds).
fn larynx_for(cfg: &VoiceConfig) -> Arc<Larynx> {
    /// A calibrated larynx and what it was calibrated for: drive, gender, pitch.
    type Kept = (Drive, Gender, u64, Arc<Larynx>);
    static RECENT: Mutex<Vec<Kept>> = Mutex::new(Vec::new());
    let female = cfg.gender == Gender::Female;
    // The usual drive of each voice: lung pressure (Pa), half the gap at rest, the zip and
    // the peel (m). Starting from the values at which the simulated source has the spectrum
    // of the equation pulse (examples/folds.rs spectrum), a search kept what the naturalness
    // predictor and the phoneme recognizer liked best on 14 sentences. The female folds
    // close all at once at their usual setting - any zip at all measured worse; lowering
    // `closure` or raising `warmth` adds some - and both voices peel apart over nearly half
    // a millimetre: without that the pulse has two sharp edges, and loses 0.2 on the
    // predictor.
    let (pressure, gap, zip, softer, peel, closure, warmth) = if female {
        (800.0, 0.118e-3, 0.0, 0.04e-3, 0.40e-3, crate::config::FEMALE_CLOSURE, 0.62)
    } else {
        (510.0, 0.099e-3, 0.065e-3, 0.12e-3, 0.54e-3, crate::config::MALE_CLOSURE, 0.56)
    };
    let usual = Drive {
        tension: 1.0,
        pressure: crate::tune("fold_ps", pressure),
        // (held closer than this the folds stay shut, further apart they only flutter)
        gap: clamp(crate::tune("fold_gap", gap) + crate::tune("fold_gap_oq", 1.5e-3) * (clamp(cfg.open_quotient, 0.5, 0.74) - 0.66), -0.05e-3, 0.35e-3),
        zip: {
            let sharper = clamp((cfg.closure - closure) / (1.0 - closure), 0.0, 1.0);
            crate::tune("fold_zip", zip) * (1.0 - sharper)
                + softer * (clamp(closure - cfg.closure, 0.0, 1.0) + 0.5 * clamp(cfg.warmth - warmth, 0.0, 1.0))
        },
        peel: crate::tune("fold_peel", peel),
        leak: crate::tune("fold_leak", if female { 6.0e-6 } else { 4.2e-6 }) * clamp(cfg.breathiness, 0.0, 0.85),
    };
    let pitch = clamp(cfg.pitch_hz, 60.0, 350.0);
    let mut recent = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(i) = recent.iter().position(|(d, g, f, _)| *d == usual && *g == cfg.gender && *f == pitch.to_bits()) {
        let entry = recent.remove(i);
        let larynx = entry.3.clone();
        recent.push(entry);
        return larynx;
    }
    if recent.len() >= 4 {
        recent.remove(0);
    }
    let mut folds = if female { Folds::FEMALE } else { Folds::MALE };
    folds.inertance = crate::tune("fold_inertance", folds.inertance);
    let damp = crate::tune("fold_damp", 1.0);
    folds.r1 *= damp;
    folds.r2 *= damp;
    folds.collision = crate::tune("fold_collision", folds.collision);
    folds.kc *= crate::tune("fold_coupling", 1.0);
    folds.d1 *= crate::tune("fold_thick", 1.0);
    let standard = Drive { tension: 1.0, pressure, gap, zip, peel, leak: 1.0e-6 };
    let larynx = Arc::new(Larynx::new(folds, usual, standard, SAMPLE_RATE as f64, pitch));
    recent.push((usual, cfg.gender, pitch.to_bits(), larynx.clone()));
    larynx
}

/// A second-order high-pass at `w0` (radians per sample) with quality `q`, as
/// `y = b0 (x - 2 x1 + x2) - a1 y1 - a2 y2`: returns (b0, a1, a2).
fn high_pass(w0: f64, q: f64) -> (f64, f64, f64) {
    let (sin_w, cos_w) = (w0.sin(), w0.cos());
    let alpha = sin_w / (2.0 * q);
    let a0 = 1.0 + alpha;
    ((1.0 + cos_w) * 0.5 / a0, -2.0 * cos_w / a0, (1.0 - alpha) / a0)
}

/// Gain that brings a signal with the given absolute peak to 0.86 full scale, limited so
/// quiet passages are not blown up (max ×3.2) and loud ones not crushed (min ×0.65).
pub fn normalization_gain(peak_abs: f64) -> f64 {
    if peak_abs > 0.001 {
        clamp(0.86 / peak_abs, 0.65, 3.2)
    } else {
        1.0
    }
}

#[inline(always)]
fn xorshift(mut x: u32) -> u32 {
    x ^= x << 13;
    x ^= x >> 17;
    x ^= x << 5;
    x
}

/// Stateful renderer for one utterance. Owns the control frames; rendering writes the
/// measured loudness of every frame back into `Frame::rms` (useful for lip-sync).
pub struct Renderer {
    frames: Vec<Frame>,
    frame_idx: usize,
    tail_frames_left: u32,
    done: bool,
    is_female: bool,
    base_ref_f0: f64,
    f4_base: f64,
    b4_hz: f64,
    a5c: f64,
    b5c: f64,
    c5c: f64,
    f6: Option<(f64, f64, f64)>,
    /// (a, b, c) of the resonators above the articulated formants, and how many are in use.
    upper: [(f64, f64, f64); UPPER_MAX],
    upper_n: usize,
    /// How much of their output above 5.2 kHz is mixed in, and the high-pass that selects it.
    upper_gain: f64,
    upper_hp: [(f64, f64, f64); 4],
    /// The pulses for this voice's closing sharpness (`closure`).
    pulses: Arc<PulseRow>,
    /// Gain of the parallel F2/F3 tap (see `render`).
    presence_tap: f64,
    /// Cycle-to-cycle variation of the pulses' length and strength (standard deviations).
    cyc_jit_sd: f64,
    cyc_shim_sd: f64,
    /// High shelf on the voiced source alone (b0, b1, b2, a1, a2), if any: see `new`.
    edge: Option<[f64; 5]>,
    /// The simulated vocal folds, if the voice uses them instead of the pulse tables.
    larynx: Option<Arc<Larynx>>,
    /// Breath noise of the top octave: gain and its high-pass.
    top_gain: f64,
    top_hp: (f64, f64, f64),
    pir_b1: f64,
    pir_b2: f64,
    pir_a1: f64,
    pir_a2: f64,
    pir_norm: f64,
    muc_pole: f64,
    muc_gain: f64,
    fnp: f64,
    r_np: f64,
    np_a1: f64,
    np_a2: f64,
    avb_c: f64,
    bvb: f64,
    cvb: f64,
    a_ch: f64,
    b_ch: f64,
    c_ch: f64,
    warmth: f64,
    hnm_air_gain: f64,
    /// The voice's own open quotient; the cycle-by-cycle value moves around it.
    oq_base: f64,
    /// Open quotient gained per semitone above the base pitch.
    pitch_oq: f64,
    /// Coupling of the airway below the glottis (0..1).
    subglottal: f64,
    chest_mix: f64,
    roughness: f64,
    vibrato_depth: f64,
    out_gain: f64,
    /// Scale of the per-frame loudness written back into the frames (see `set_energy_scale`).
    energy_scale: f64,
    /// Tone equalizer for the voiced sound; `None` when the curve is flat.
    tone_eq: Option<[[f64; 5]; TONE_BANDS]>,
    st: State,
    room_buf: [f32; 512],
    room: Room,
    room_on: bool,
    silent_tail: Frame,
}

fn silent_tail_frame(base_ref_f0: f64) -> Frame {
    Frame {
        t_sec: 0.0,
        clause_idx: -1,
        micro_f0: 0.0,
        f0: base_ref_f0 * 0.92,
        f1: 500.0,
        f2: 1480.0,
        f3: 2480.0,
        b1: 110.0,
        b2: 130.0,
        b3: 185.0,
        av: 0.0,
        ah: 0.0,
        af: 0.0,
        avb: 0.0,
        nasal: 0.0,
        oral_seal: 0.0,
        fnz: 500.0,
        lax: 0.5,
        creak: 0.0,
        fric_f1: 2000.0,
        fric_b1: 1000.0,
        fric_f2: 4500.0,
        fric_b2: 1500.0,
        fric_g1: 0.0,
        fric_g2: 0.0,
        fric_hp: 0.0,
        is_closure: false,
        is_burst: false,
        stress: 0,
        jaw: 0.04,
        t_h: 0.45,
        t_f: 0.50,
        t_tip: 0.15,
        lip_r: 0.04,
        lip_s: 0.0,
        lip_c: 0.16,
        velum: 0.0,
        phone: Ph::SIL,
        word: -1,
        rms: 0.0,
    }
}

impl Renderer {
    pub fn new(frames: Vec<Frame>, cfg: &VoiceConfig) -> Renderer {
        let fs = SAMPLE_RATE as f64;
        let inv_fs = 1.0 / fs;
        let female = cfg.gender == Gender::Female;
        let tract_cm = clamp(cfg.tract_length_cm, 12.2, 21.5);
        let formant_scale = 17.5 / tract_cm;
        let f1_scale = 1.0 + (formant_scale - 1.0) * if female { 0.88 } else { 0.75 };

        let breathiness = clamp(cfg.breathiness, 0.0, 0.85);
        let warmth = clamp(cfg.warmth, 0.0, 1.0);
        let chest_resonance = clamp(cfg.chest_resonance, 0.0, 1.0);
        let roughness = clamp(cfg.roughness, 0.0, 0.75);
        let vibrato = clamp(cfg.vibrato, 0.0, 1.0);
        let base_ref_f0 = cfg.pitch_hz;

        // Higher laryngeal formants: F4 (coarticulated per frame) and F5 (fixed).
        // A man's voice has a cluster of three resonances between about 3.3 and 4.5 kHz (it
        // is what gives a real voice its "presence"); measured against a real speaker this
        // cut the mismatch of the voiced spectrum from 8 dB to 3 dB. Women's upper formants
        // are spread wider and the third one falls above the band that matters.
        let f4_base = clamp(3460.0 * (1.0 + (formant_scale - 1.0) * 0.78) * cfg.formants[3], 2600.0, 4950.0);
        let b4_hz = if female { 310.0 } else { 200.0 } * formant_scale;
        let f5_hz = clamp(
            if female { 4420.0 } else { 3950.0 } * (1.0 + (formant_scale - 1.0) * 0.72) * cfg.formants[4],
            3450.0,
            6200.0,
        );
        let b5_hz = if female { 450.0 } else { 260.0 } * formant_scale;
        let f6_hz = if female { 0.0 } else { 4500.0 } * (1.0 + (formant_scale - 1.0) * 0.72) * cfg.formants[4];
        let r6 = (-PI * 380.0 * formant_scale * inv_fs).exp();
        let b6c = 2.0 * r6 * (TAU * f6_hz * inv_fs).cos();
        let c6c = -r6 * r6;
        let a6c = 1.0 - b6c - c6c;
        let has_f6 = f6_hz > 100.0;

        // Air: the resonances above the articulated formants (`air`). A vocal tract resonates
        // about every 1000 Hz (a 17.5 cm tract) all the way up; a cascade that stops at F5/F6
        // has nothing above them, so its vowels are 30 dB short of a real voice's above 5 kHz
        // (tools/vowel_bands.py). Six more resonators continue the series, as a side branch:
        // only what they add above 5.2 kHz is mixed in, so the spectrum below, which
        // everything else is balanced against, stays as it is. Because the branch hangs off
        // the tract, the top band moves with the articulation as a real voice's does.
        let air = clamp(cfg.air, 0.0, 1.0);
        let upper_n = if air > 0.0 { UPPER_MAX } else { 0 };
        let mut upper = [(1.0, 0.0, 0.0); UPPER_MAX];
        {
            let spacing = 1000.0 * (1.0 + (formant_scale - 1.0) * 0.72);
            let first = if has_f6 { f6_hz } else { f5_hz } + spacing;
            for (k, u) in upper.iter_mut().enumerate() {
                let f = first + spacing * k as f64;
                if f > 0.47 * fs {
                    continue; // past the top of the band: leave it out
                }
                let r = (-PI * (700.0 + 0.08 * (f - 5000.0).max(0.0)) * inv_fs).exp();
                let b = 2.0 * r * (TAU * f * inv_fs).cos();
                let c = -r * r;
                *u = (1.0 - b - c, b, c);
            }
        }
        // 1 = the level measured in real speakers; the scale is squared so that the lower
        // half of the control stays subtle.
        let upper_gain = crate::tune("air_gain", if female { 0.3 } else { 0.2 }) * air * air;
        // Above about 7 kHz a voice is breath noise, not harmonics (tools/harmonicity.py):
        // a shelf of noise that follows the voicing, heard past the tract.
        let top_gain = crate::tune("top_gain", if female { 0.02 } else { 0.008 }) * air * air;
        let top_hp = high_pass(TAU * crate::tune("top_cut", if female { 9500.0 } else { 7500.0 }) * inv_fs, HIGH_PASS_Q);
        // (eighth order: what leaks through below the cut-off would add to, or cancel, the
        // formants there)
        let upper_hp: [(f64, f64, f64); 4] = {
            let w0 = TAU * 5200.0 * inv_fs;
            [0.5098, 0.6013, 0.9000, 2.5629].map(|q| high_pass(w0, q))
        };

        let r5 = (-PI * b5_hz * inv_fs).exp();
        let b5c = 2.0 * r5 * (TAU * f5_hz * inv_fs).cos();
        let c5c = -r5 * r5;
        let a5c = 1.0 - b5c - c5c;

        // Piriform-fossa side-branch anti-resonance (~4.4 kHz male / ~4.8 kHz female).
        let f_pir_hz = clamp(
            if female { 4780.0 } else { 4420.0 } * (1.0 + (formant_scale - 1.0) * 0.50),
            3800.0,
            5600.0,
        );
        let cos_pir = (TAU * f_pir_hz * inv_fs).cos();
        let r_pir_z = crate::tune("pir_z", 0.91);
        let r_pir_p = 0.79;
        let pir_b1 = -2.0 * r_pir_z * cos_pir;
        let pir_b2 = r_pir_z * r_pir_z;
        let pir_a1 = -2.0 * r_pir_p * cos_pir;
        let pir_a2 = r_pir_p * r_pir_p;
        let pir_norm = (1.0 + pir_a1 + pir_a2) / (1.0 + pir_b1 + pir_b2).max(0.15);

        // Soft-tissue roll-off (~5 kHz two-pole low-pass on the voiced path).
        // (This and the notch above leave the vowels 6-20 dB short of two 24 kHz reference
        // voices between 3.3 and 6 kHz. Opening them up matched the references and scored
        // lower: the synthesizer's own top end is noise where a voice has harmonics.)
        let muc_cut_hz = crate::tune("muc_cut", if female { 5150.0 } else { 4850.0 });
        let muc_pole = (-TAU * muc_cut_hz * inv_fs).exp();
        let muc_gain = 1.0 - muc_pole;

        // Nasal pole (the low "murmur" resonance of the nose) and its companion zero.
        let fnp = 270.0 * f1_scale;
        let r_np = (-PI * 100.0 * inv_fs).exp();
        let np_a1 = -2.0 * r_np * (TAU * fnp * inv_fs).cos();
        let np_a2 = r_np * r_np;

        // Voice bar (~175 Hz) heard through the cheeks during voiced closures.
        let fvb_hz = 175.0 * f1_scale;
        let rvb = (-PI * 72.0 * inv_fs).exp();
        let bvb = 2.0 * rvb * (TAU * fvb_hz * inv_fs).cos();
        let cvb = -rvb * rvb;
        let avb_c = 1.0 - bvb - cvb;

        // Sub-glottal chest resonator tuned just above the fundamental.
        let f_chest = clamp(base_ref_f0 * 1.04, 86.0, 255.0);
        let b_chest = clamp(base_ref_f0 * 0.62, 62.0, 155.0);
        let r_ch = (-PI * b_chest * inv_fs).exp();
        let b_ch = 2.0 * r_ch * (TAU * f_chest * inv_fs).cos();
        let c_ch = -r_ch * r_ch;
        let a_ch = 1.0 - b_ch - c_ch;

        let done = frames.is_empty();
        Renderer {
            frames,
            frame_idx: 0,
            tail_frames_left: 2,
            done,
            is_female: female,
            base_ref_f0,
            f4_base,
            b4_hz,
            a5c,
            b5c,
            c5c,
            f6: if has_f6 { Some((a6c, b6c, c6c)) } else { None },
            upper,
            upper_n,
            upper_gain,
            upper_hp,
            pulses: pulse_row((clamp(cfg.closure, 0.0, 1.0) * (CLOSURE_STEPS - 1) as f64).round() as usize),
            // The tap was put in to brighten the second and third formants under a soft
            // pulse. The female voice is better without it on both scores, and its balance
            // then matches the reference voice (H1-A3); the male voice keeps it.
            presence_tap: crate::tune("par_tap", if female { 0.0 } else { 1.0 }),
            // No two cycles of a real voice are alike: each is a little longer or shorter
            // and a little stronger or weaker than the last (0.4% and 3% in the male voice).
            // The slow drift that was there before is not the same thing: it leaves every
            // harmonic razor-sharp. The female voice has none: its harmonics already stand
            // no further above the noise between them than a real voice's
            // (tools/harmonicity.py), and there the variation only cost clarity.
            cyc_jit_sd: crate::tune("cyc_jit", if female { 0.0 } else { 0.004 }),
            cyc_shim_sd: crate::tune("cyc_shim", if female { 0.0 } else { 0.03 }),
            larynx: cfg.folds.then(|| larynx_for(cfg)),
            edge: {
                // The closing edge of a real pulse is sharper than its return phase alone
                // gives: harmonics, not only breath, reach the top band. Applied with the
                // `air` setting, which is what lets that band through.
                let db = crate::tune("edge_db", 10.0) * air;
                (db.abs() > 0.01).then(|| {
                    let a = 10f64.powf(db / 40.0);
                    let w0 = TAU * crate::tune("edge_hz", 5500.0) * inv_fs;
                    let (sin_w, cos_w) = (w0.sin(), w0.cos());
                    let alpha = sin_w * 0.5 * 2f64.sqrt();
                    let k = 2.0 * a.sqrt() * alpha;
                    let a0 = (a + 1.0) - (a - 1.0) * cos_w + k;
                    [
                        a * ((a + 1.0) + (a - 1.0) * cos_w + k) / a0,
                        -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w) / a0,
                        a * ((a + 1.0) + (a - 1.0) * cos_w - k) / a0,
                        2.0 * ((a - 1.0) - (a + 1.0) * cos_w) / a0,
                        ((a + 1.0) - (a - 1.0) * cos_w - k) / a0,
                    ]
                })
            },
            top_gain,
            top_hp,
            pir_b1,
            pir_b2,
            pir_a1,
            pir_a2,
            pir_norm,
            muc_pole,
            muc_gain,
            fnp,
            r_np,
            np_a1,
            np_a2,
            avb_c,
            bvb,
            cvb,
            a_ch,
            b_ch,
            c_ch,
            warmth,
            // Breath noise that leaks through the glottis on every cycle; a voice without
            // it sounds like a buzzer.
            hnm_air_gain: if female { 0.11 } else { 0.15 } + breathiness * 0.22,
            oq_base: clamp(cfg.open_quotient, 0.5, 0.74),
            pitch_oq: 0.015 * clamp(cfg.pitch_coupling, 0.0, 1.0),
            subglottal: clamp(cfg.subglottal, 0.0, 1.0),
            // (the male factor was 0.22: that put the fundamental 6 dB above the second
            // harmonic, where two real male speakers have it 2-3 dB below; tools/model_gaps.py)
            // ... and the female one 0.52: 9 dB between the first two harmonics where the
            // reference voice has 4)
            chest_mix: chest_resonance * if female { crate::tune("chest_f", 0.38) } else { crate::tune("chest_m", 0.064) },
            roughness,
            vibrato_depth: vibrato * 0.014,
            // Calibrated so conversational speech peaks around 0.55 without clipping.
            // (the simulated folds are a little quieter than the equation pulse for the same
            // settings; the whole is brought up so that both sources come out equally loud)
            out_gain: if female { 0.20 } else { 0.19 } * if cfg.folds { crate::tune("fold_level", 1.2) } else { 1.0 },
            energy_scale: 1.0,
            tone_eq: {
                let tone = cfg.effective_tone();
                if tone.iter().any(|g| g.abs() > 0.05) {
                    let mut bands = [[1.0, 0.0, 0.0, 0.0, 0.0]; TONE_BANDS];
                    for (i, band) in bands.iter_mut().enumerate() {
                        *band = tone_band(i, tone[i]);
                    }
                    Some(bands)
                } else {
                    None
                }
            },
            st: {
                let mut st = State::new();
                st.oq_idx = oq_index(clamp(cfg.open_quotient, 0.5, 0.74));
                st
            },
            room_buf: [0.0; 512],
            room: Room::new(clamp(cfg.room, 0.0, 1.0)),
            room_on: cfg.room > 0.001,
            silent_tail: silent_tail_frame(base_ref_f0),
        }
    }

    pub fn frames(&self) -> &[Frame] {
        &self.frames
    }

    pub fn into_frames(self) -> Vec<Frame> {
        self.frames
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    /// Number of control frames rendered so far.
    pub fn frame_position(&self) -> usize {
        self.frame_idx
    }

    /// Synthesizes up to `frame_count` frames into `out` (120 samples each) and returns the
    /// number of samples written. After the last real frame two silent frames let the
    /// resonators ring out; then the renderer reports `is_done()`.
    pub fn render(&mut self, frame_count: usize, out: &mut [f32]) -> usize {
        let tables = glottal_tables();
        let oq_base = self.oq_base;
        // How the open quotient follows the pitch (per semitone above the base pitch).
        let pitch_oq = self.pitch_oq;
        let ug_body = &tables.ug_body[..];

        let total_frames = self.frames.len();
        let inv_fs = 1.0 / SAMPLE_RATE as f64;
        let n_substeps = SAMPLES_PER_FRAME / SUBSTEP_SAMPLES;
        let room_on = self.room_on;
        // The airway below the glottis (`subglottal`): while the folds are open it loads the
        // tract, damping the first resonance and raising it a little for part of every cycle.
        let subglottal = self.subglottal;
        let sti_bw = 400.0 * subglottal;
        let sti_f1 = 0.08 * subglottal;
        // A stop release: the noise starts at once (not as a 5 ms ramp) behind a short
        // pressure pulse.
        // Breath noise at the glottis (/h/, aspiration): how dull it is, how loud, and how
        // much of its top end reaches the lips past the tract's own roll-off.
        let pitch_lax = crate::tune("pitch_lax", 0.0);
        // (negative: the folds close harder on a lifted syllable, so the voice brightens as
        // it rises - the upper bands of a real voice gain 1.2-1.4 dB per dB of level)
        let pitch_warm = crate::tune("pitch_warm", -0.03);
        let stress_warm = crate::tune("stress_warm", 1.0);
        let asp_a1 = crate::tune("asp_a1", 0.78);
        let asp_a2 = crate::tune("asp_a2", 0.68);
        let asp_gain = crate::tune("asp_gain", 2.35);
        let asp_hf = crate::tune("asp_hf", 0.0);
        let par_tap = self.presence_tap;
        let pulses = self.pulses.clone();
        let (cyc_jit_sd, cyc_shim_sd) = (self.cyc_jit_sd, self.cyc_shim_sd);
        let edge = self.edge;
        let larynx = self.larynx.clone();
        // How much harder the lungs push per octave of pitch, how far the folds part in lax
        // voice (m), the level of the simulated source against the equation pulse (set so
        // that hiss and bursts stand against the voice as they do in recordings of people:
        // tools/eval.py levels), how long the folds run before the voice is heard (s), and
        // their share in the chest sound.
        let fold_effort = crate::tune("fold_effort", if self.is_female { 0.4 } else { 0.18 });
        let fold_lax_gap = crate::tune("fold_lax_gap", if self.is_female { 0.15e-3 } else { 0.2e-3 });
        let fold_gain = crate::tune("fold_gain", if self.is_female { 0.55 } else { 0.72 });
        let fold_preroll = crate::tune("fold_preroll", 0.02);
        let fold_chest = crate::tune("fold_chest", if self.is_female { 4.4 } else { 6.9 });
        let fold_lax_quiet = crate::tune("fold_lax_quiet", 0.1);
        let creak_am = crate::tune("creak_am", 0.6);
        let creak_fm = crate::tune("creak_fm", 0.2);
        let burst_sharp = crate::tune("burst_sharp", 6.0);
        let transient = crate::tune("transient", 6.0);
        // One-pole glottal spectral tilt on the female pulse source, softening excess 1.5-3.5 kHz
        // cascade energy induced by higher female open-vowel F1 resonances.
        let glot_tilt = if self.is_female {
            crate::tune("glot_tilt", 0.22)
        } else {
            crate::tune("glot_tilt_m", 0.04)
        };
        // The `subglottal` setting: while the glottis is open the windpipe is part of the
        // tube, and its second resonance (about 1.4 kHz in a man, 1.6 kHz in a woman) puts a
        // dip and a small peak into the spectrum.
        let trach = subglottal;
        let (tr_b1, tr_b2, tr_a1, tr_a2, tr_norm) = {
            let fz = if self.is_female { 1620.0 } else { 1400.0 };
            let rz = (-PI * 180.0 * inv_fs).exp();
            let rp = (-PI * 220.0 * inv_fs).exp();
            let b1 = -2.0 * rz * (TAU * fz * inv_fs).cos();
            let b2 = rz * rz;
            let a1 = -2.0 * rp * (TAU * fz * 1.09 * inv_fs).cos();
            let a2 = rp * rp;
            (b1, b2, a1, a2, (1.0 + a1 + a2) / (1.0 + b1 + b2))
        };
        let inv_substeps = 1.0 / n_substeps as f64;
        let sub_sec = SUBSTEP_SAMPLES as f64 * inv_fs;

        let mut s = self.st;
        let (a5c, b5c, c5c) = (self.a5c, self.b5c, self.c5c);
        let f6 = self.f6;
        let upper = self.upper;
        let upper_n = self.upper_n;
        let upper_gain = self.upper_gain;
        let upper_hp = self.upper_hp;
        let top_gain = self.top_gain;
        let (top_b0, top_a1, top_a2) = self.top_hp;
        let (pir_b1, pir_b2, pir_a1, pir_a2, pir_norm) =
            (self.pir_b1, self.pir_b2, self.pir_a1, self.pir_a2, self.pir_norm);
        let (muc_pole, muc_gain) = (self.muc_pole, self.muc_gain);
        let (fnp, r_np, np_a1, np_a2) = (self.fnp, self.r_np, self.np_a1, self.np_a2);
        let (avb_c, bvb, cvb) = (self.avb_c, self.bvb, self.cvb);
        let (a_ch, b_ch, c_ch) = (self.a_ch, self.b_ch, self.c_ch);
        let warmth = self.warmth;
        let hnm_air_gain = self.hnm_air_gain;
        let chest_mix = self.chest_mix;
        let roughness = self.roughness;
        let vibrato_depth = self.vibrato_depth;
        let out_gain = self.out_gain;
        let tone_eq = self.tone_eq;
        let f4_base = self.f4_base;
        let r4 = (-PI * self.b4_hz * inv_fs).exp();
        let c4c = -r4 * r4;
        let base_ref_f0 = self.base_ref_f0;
        let is_female = self.is_female;

        let max_frames = frame_count.min(out.len() / SAMPLES_PER_FRAME);
        let mut write_idx = 0usize;

        for _ in 0..max_frames {
            let fi = self.frame_idx;
            let is_real_frame = fi < total_frames;
            let (f_cur, f_next): (&Frame, &Frame) = if is_real_frame {
                self.frame_idx = fi + 1;
                (
                    &self.frames[fi],
                    if fi + 1 < total_frames {
                        &self.frames[fi + 1]
                    } else {
                        &self.silent_tail
                    },
                )
            } else if self.tail_frames_left > 0 {
                self.tail_frames_left -= 1;
                (&self.silent_tail, &self.silent_tail)
            } else {
                self.done = true;
                break;
            };

            let burst_onset = f_cur.is_burst && !s.prev_burst;
            s.prev_burst = f_cur.is_burst;
            let mut release_click = if burst_onset { transient * f_cur.af.max(f_next.af) } else { 0.0 };
            let into_burst = f_next.is_burst && !f_cur.is_burst;
            let room_quiet = !room_on || self.room.level * self.room.wet < 2e-6;
            let is_silent_frame = room_quiet
                && f_cur.av < 0.0004
                && f_next.av < 0.0004
                && f_cur.ah < 0.0004
                && f_next.ah < 0.0004
                && f_cur.af < 0.0004
                && f_next.af < 0.0004
                && f_cur.avb < 0.0004
                && f_next.avb < 0.0004
                && s.y1_1.abs() < 0.0001
                && s.y1_2.abs() < 0.0001
                && s.y2_1.abs() < 0.0001
                && s.y2_2.abs() < 0.0001
                && s.muc2.abs() < 0.0001
                && s.nm1_1.abs() < 0.0001
                && s.dc_y1.abs() < 0.0001;
            if is_silent_frame {
                out[write_idx..write_idx + SAMPLES_PER_FRAME].fill(0.0);
                write_idx += SAMPLES_PER_FRAME;
                if is_real_frame {
                    self.frames[fi].rms = 0.0;
                }
                continue;
            }

            // F4 follows tongue frontness and pitch slightly.
            let f4_dyn = clamp(
                f4_base
                    * (1.0
                        + 0.035 * ((f_cur.f2 - 1450.0) * 0.001)
                        + 0.020 * ((f_cur.f0 - base_ref_f0) / base_ref_f0)),
                2550.0,
                5050.0,
            );
            let b4c = 2.0 * r4 * (TAU * f4_dyn * inv_fs).cos();
            let a4c = 1.0 - b4c - c4c;

            let cur_stress = f_cur.stress;
            let (cur_fric_g1, cur_fric_g2) = (f_cur.fric_g1, f_cur.fric_g2);
            let mut frame_sq_sum = 0.0f64;

            for sub in 0..n_substeps {
                let alpha = sub as f64 * inv_substeps;
                let lerp = |a: f64, b: f64| a + (b - a) * alpha;
                let f0 = lerp(f_cur.f0, f_next.f0);
                let f1 = lerp(f_cur.f1, f_next.f1);
                let mut f2 = lerp(f_cur.f2, f_next.f2);
                let mut f3 = lerp(f_cur.f3, f_next.f3);
                let mut b1 = lerp(f_cur.b1, f_next.b1);
                let b2 = lerp(f_cur.b2, f_next.b2);
                let b3 = lerp(f_cur.b3, f_next.b3);
                let fnz = lerp(f_cur.fnz, f_next.fnz);
                let av = lerp(f_cur.av, f_next.av);
                let ah = lerp(f_cur.ah, f_next.ah);
                // A stop release is an explosion: its noise starts at once, not as a ramp.
                let af = if into_burst {
                    f_cur.af + (f_next.af - f_cur.af) * alpha.powf(burst_sharp)
                } else {
                    lerp(f_cur.af, f_next.af)
                };
                let avb = lerp(f_cur.avb, f_next.avb);
                let nasal = lerp(f_cur.nasal, f_next.nasal);
                let oral_seal = lerp(f_cur.oral_seal, f_next.oral_seal);
                let lax = lerp(f_cur.lax, f_next.lax);
                let creak = lerp(f_cur.creak, f_next.creak);

                if f2 < f1 + 170.0 {
                    f2 = f1 + 170.0;
                }
                if f3 < f2 + 210.0 {
                    f3 = f2 + 210.0;
                }
                // Widen F1 when the second harmonic of a high voice lands on it (avoids hollow
                // formant whistles).
                if f0 > 135.0 {
                    let h_dist = (f1 - f0 * 2.0).abs();
                    if h_dist < 110.0 {
                        b1 += (110.0 - h_dist) * 1.15;
                    }
                }

                let r1 = (-PI * b1 * inv_fs).exp();
                let b1c = 2.0 * r1 * (TAU * f1 * inv_fs).cos();
                let c1c = -r1 * r1;
                let a1c = 1.0 - b1c - c1c;
                // The same resonance while the glottis is open: the airway below it loads the
                // tract, so F1 is damped (and a little higher) for part of every cycle.
                let sti_w = if sti_bw > 0.0 || sti_f1 > 0.0 { clamp(av / 0.3, 0.0, 1.0) } else { 0.0 };
                let r1o = (-PI * (b1 + sti_bw) * inv_fs).exp();
                let b1o = 2.0 * r1o * (TAU * f1 * (1.0 + sti_f1) * inv_fs).cos();
                let c1o = -r1o * r1o;

                let r2 = (-PI * b2 * inv_fs).exp();
                let b2c = 2.0 * r2 * (TAU * f2 * inv_fs).cos();
                let c2c = -r2 * r2;
                let a2c = 1.0 - b2c - c2c;

                let r3 = (-PI * b3 * inv_fs).exp();
                let b3c = 2.0 * r3 * (TAU * f3 * inv_fs).cos();
                let c3c = -r3 * r3;
                let a3c = 1.0 - b3c - c3c;

                // Nasal coupling: a pole at ~270 Hz (the nasal murmur) paired with a zero. With
                // the velum closed the zero sits on the pole and cancels it; as the velum opens
                // the zero slides up toward F1, hollowing it out.
                let nas = clamp(nasal, 0.0, 1.0);
                let f_zero = fnp + nas * (clamp(fnz, fnp, 1200.0) - fnp);
                let nz_b1 = -2.0 * r_np * (TAU * f_zero * inv_fs).cos();
                let nz_b2 = r_np * r_np;
                let nz_norm = (1.0 + np_a1 + np_a2) / (1.0 + nz_b1 + nz_b2);

                let has_fric = af > 0.0005
                    || s.fr1_y1.abs() > 0.0001
                    || s.fr1_y2.abs() > 0.0001
                    || s.fr2_y1.abs() > 0.0001
                    || s.fr2_y2.abs() > 0.0001;
                let (mut rf1, mut bf1, mut cf1, mut gf1) = (0.0, 0.0, 0.0, 0.0);
                let (mut rf2, mut bf2, mut cf2, mut gf2) = (0.0, 0.0, 0.0, 0.0);
                let (mut hp_on, mut hp_b0, mut hp_a1, mut hp_a2) = (false, 0.0, 0.0, 0.0);
                if has_fric {
                    let fr_f1 = lerp(f_cur.fric_f1, f_next.fric_f1);
                    let fr_b1 = lerp(f_cur.fric_b1, f_next.fric_b1);
                    let fr_f2 = lerp(f_cur.fric_f2, f_next.fric_f2);
                    let fr_b2 = lerp(f_cur.fric_b2, f_next.fric_b2);

                    rf1 = (-PI * fr_b1 * inv_fs).exp();
                    bf1 = 2.0 * rf1 * (TAU * fr_f1 * inv_fs).cos();
                    cf1 = -rf1 * rf1;
                    gf1 = (1.0 - rf1) * cur_fric_g1;

                    rf2 = (-PI * fr_b2 * inv_fs).exp();
                    bf2 = 2.0 * rf2 * (TAU * fr_f2 * inv_fs).cos();
                    cf2 = -rf2 * rf2;
                    gf2 = (1.0 - rf2) * cur_fric_g2;

                    // 4th-order low cut (two Butterworth-style sections) for the noise.
                    let hp_hz = lerp(f_cur.fric_hp, f_next.fric_hp);
                    hp_on = hp_hz > 150.0;
                    if hp_on {
                        (hp_b0, hp_a1, hp_a2) = high_pass(TAU * hp_hz.min(9000.0) * inv_fs, HIGH_PASS_Q);
                    }
                }

                // Laryngeal micro-flutter, vibrato, jitter and shimmer.
                s.flt_phase1 += 4.3 * sub_sec;
                if s.flt_phase1 >= 1.0 {
                    s.flt_phase1 -= 1.0;
                }
                s.flt_phase2 += 7.1 * sub_sec;
                if s.flt_phase2 >= 1.0 {
                    s.flt_phase2 -= 1.0;
                }
                s.flt_phase3 += 12.7 * sub_sec;
                if s.flt_phase3 >= 1.0 {
                    s.flt_phase3 -= 1.0;
                }
                s.vib_phase += 5.2 * sub_sec;
                if s.vib_phase >= 1.0 {
                    s.vib_phase -= 1.0;
                }

                let flutter = 0.0052 * (TAU * s.flt_phase1).sin()
                    + 0.0034 * (TAU * s.flt_phase2).sin()
                    + 0.0020 * (TAU * s.flt_phase3).sin();
                let vib_mod = vibrato_depth * (TAU * s.vib_phase).sin();
                s.rng = xorshift(s.rng);
                let r_norm = (s.rng & 0x7fff_ffff) as f64 * 4.65661287e-10 - 0.5;
                s.jitter_walk = s.jitter_walk * 0.90 + r_norm * (0.0035 + roughness * 0.032);
                s.shimmer_walk = s.shimmer_walk * 0.90 + r_norm * (0.015 + roughness * 0.14);

                let inst_f0 = clamp(f0 * (1.0 + flutter + vib_mod + s.jitter_walk), 50.0, 980.0);
                let phase_inc = inst_f0 * inv_fs;

                // Richest pulse table whose harmonics all stay below Nyquist at this pitch.
                let max_h = SAMPLE_RATE as f64 * 0.46 / inst_f0;
                let level = MIP_HARMONICS
                    .iter()
                    .position(|&h| h as f64 <= max_h)
                    .unwrap_or(MIP_HARMONICS.len() - 1);
                let oq_target = oq_index(oq_base + pitch_oq * clamp(12.0 * (f0 / base_ref_f0).log2(), -8.0, 8.0));
                let mut modal = pulses.table(s.oq_idx);
                let mut eg_modal = &modal.eg_modal[level * UG_SIZE..(level + 1) * UG_SIZE];
                let mut ug_env = &modal.ug_env[..];
                let eg_warm = &tables.eg_warm[level * UG_SIZE..(level + 1) * UG_SIZE];
                let eg_lax = &tables.eg_lax[level * UG_SIZE..(level + 1) * UG_SIZE];

                // Open-quotient blend: modal <-> warm <-> lax, breathing slowly at 4–7 Hz.
                let oq_mod = 0.08 * (TAU * s.flt_phase1).sin() + 0.05 * (TAU * s.flt_phase2).sin();
                // The voice box: raising the pitch stretches the folds thinner, so they stay
                // open longer and close more gently (development hooks, per semitone above
                // the voice's base pitch; see tools/voice_quality.py).
                let pitch_st = clamp(12.0 * (f0 / base_ref_f0).log2(), -8.0, 8.0);
                let lax_weight = clamp(lax + (1.0 - av) * 0.35 + pitch_lax * pitch_st, 0.0, 0.92);
                let rem_weight = 1.0 - lax_weight;
                let warm_frac = clamp(
                    warmth * if is_female { 0.76 } else { 0.64 }
                        + if cur_stress == 1 { -0.08 } else { 0.09 } * stress_warm
                        + pitch_warm * pitch_st
                        + oq_mod,
                    0.05,
                    0.95,
                );
                let w_warm = rem_weight * warm_frac;
                let w_modal = rem_weight * (1.0 - warm_frac);

                let voiced_gain = av * (1.0 + s.shimmer_walk);

                // The simulated folds: what the speaker does with them in this stretch. The
                // lungs push harder for a higher pitch; the folds are held less firmly in
                // lax voice and more firmly, with less air, in creak; and none of it is
                // quite steady.
                // (they vibrate whenever there is voice: also behind the closed lips of a /b/)
                let fold_voiced = av > 0.02 || avb > 0.02;
                let (fold_pressure, fold_gap) = match larynx.as_deref() {
                    Some(l) if fold_voiced => (
                        l.usual.pressure * 2f64.powf(fold_effort * pitch_st / 12.0) * (1.0 - 0.25 * creak),
                        l.usual.gap + fold_lax_gap * (lax + fold_lax_quiet * (1.0 - av)) + 0.25e-3 * oq_mod - 0.15e-3 * creak,
                    ),
                    Some(l) => (0.0, l.usual.gap + fold_lax_gap),
                    None => (0.0, 0.0),
                };

                // The mouth output fades as lips/tongue seal it during /m n ng/.
                // A nasal murmur radiates from the nose only, which makes it quieter than a vowel.
                let oral_open_factor = 1.0 - 0.5 * oral_seal;
                let par2_gain = a2c * 0.085 * oral_open_factor * par_tap;
                let par3_gain = a3c * 0.075 * oral_open_factor * par_tap;
                let open_trach_damp = if is_female { 0.012 } else { 0.008 };

                let need_noise = ah > 0.0003 || av > 0.12 || has_fric;

                for _ in 0..SUBSTEP_SAMPLES {
                    // 1. Glottal source
                    let (glot_excite, flow_env, chest_in);
                    if let Some(larynx) = larynx.as_deref() {
                        // the folds themselves (in creak every other cycle is slacker)
                        if fold_voiced && larynx.at_rest(&s.fold) {
                            larynx.start(&mut s.fold, inst_f0, fold_pressure, fold_gap, fold_preroll);
                        }
                        let (rate, flow, began) = larynx.tick(&mut s.fold, inst_f0, fold_pressure, fold_gap, 1.0 - s.sub_harm_toggle * creak * creak_fm);
                        if began {
                            s.sub_harm_toggle = -s.sub_harm_toggle;
                        }
                        let creak_scale = 1.0 + s.sub_harm_toggle * (roughness * 0.28 + creak * creak_am);
                        glot_excite = rate * fold_gain * creak_scale;
                        flow_env = flow.min(1.0);
                        s.fold_flow_mean += (flow - s.fold_flow_mean) * 0.002;
                        chest_in = (flow - s.fold_flow_mean) * fold_chest * av * creak_scale * 0.50;
                    } else {
                        // pulse tables (in creak every other cycle is late and weak)
                        s.glot_phase += phase_inc * (1.0 - s.sub_harm_toggle * creak * creak_fm) * (1.0 + s.cyc_jit);
                        if s.glot_phase >= 1.0 {
                            s.glot_phase -= 1.0;
                            s.sub_harm_toggle = -s.sub_harm_toggle;
                            if cyc_jit_sd > 0.0 || cyc_shim_sd > 0.0 {
                                // (the sum of three uniform numbers is close enough to a bell curve)
                                let mut draw = || {
                                    let mut g = 0.0;
                                    for _ in 0..3 {
                                        s.rng = xorshift(s.rng);
                                        g += (s.rng & 0xffff) as f64 * 0.000030517578125 - 1.0;
                                    }
                                    g
                                };
                                s.cyc_jit = draw() * cyc_jit_sd;
                                s.cyc_shim = draw() * cyc_shim_sd;
                            }
                            // A new cycle may open a little longer or shorter than the last one.
                            if s.oq_idx != oq_target {
                                s.oq_idx = if s.oq_idx < oq_target { s.oq_idx + 1 } else { s.oq_idx - 1 };
                                modal = pulses.table(s.oq_idx);
                                eg_modal = &modal.eg_modal[level * UG_SIZE..(level + 1) * UG_SIZE];
                                ug_env = &modal.ug_env[..];
                            }
                        }

                        let f_idx = s.glot_phase * UG_SIZE as f64;
                        let idx0 = (f_idx as usize) & UG_MASK;
                        let frac = f_idx - idx0 as f64;
                        let idx1 = (idx0 + 1) & UG_MASK;

                        let e_mod = eg_modal[idx0] as f64 + (eg_modal[idx1] as f64 - eg_modal[idx0] as f64) * frac;
                        let e_wrm = eg_warm[idx0] as f64 + (eg_warm[idx1] as f64 - eg_warm[idx0] as f64) * frac;
                        let e_lx = eg_lax[idx0] as f64 + (eg_lax[idx1] as f64 - eg_lax[idx0] as f64) * frac;
                        flow_env = ug_env[idx0] as f64 + (ug_env[idx1] as f64 - ug_env[idx0] as f64) * frac;

                        let creak_scale = 1.0 + s.sub_harm_toggle * (roughness * 0.28 + creak * creak_am);
                        glot_excite = (e_mod * w_modal + e_wrm * w_warm + e_lx * lax_weight) * creak_scale;
                        chest_in = ug_body[idx0] as f64 * av * creak_scale * 0.50;
                    }
                    let mut voiced_src = glot_excite * voiced_gain * (1.0 + s.cyc_shim);
                    if glot_tilt > 0.0 && larynx.is_none() {
                        s.tilt_y = glot_tilt * s.tilt_y + (1.0 - glot_tilt) * voiced_src;
                        voiced_src = s.tilt_y;
                    }
                    if let Some([b0, b1, b2, a1, a2]) = edge {
                        let y = b0 * voiced_src + b1 * s.edge[0] + b2 * s.edge[1] - a1 * s.edge[2] - a2 * s.edge[3];
                        s.edge = [voiced_src, s.edge[0], y, s.edge[2]];
                        voiced_src = y;
                    }

                    // 2. Turbulence
                    let mut white = 0.0;
                    let mut asp_src = 0.0;
                    let mut asp_air = 0.0;
                    let mut hnm_src = 0.0;
                    if need_noise {
                        s.rng = xorshift(s.rng);
                        let n1 = (s.rng & 0xffff) as f64 * 0.000030517578125 - 1.0;
                        s.rng = xorshift(s.rng);
                        let n2 = (s.rng & 0xffff) as f64 * 0.000030517578125 - 1.0;
                        white = (n1 + n2) * 0.5;

                        // Two-stage low-passed breath: /h/ and aspiration never click like /k t/.
                        if ah > 0.0003 {
                            s.asp_lp = s.asp_lp * asp_a1 + white * (1.0 - asp_a1);
                            s.asp_lp2 = s.asp_lp2 * asp_a2 + s.asp_lp * (1.0 - asp_a2);
                            asp_air = (white - s.asp_lp) * ah * asp_hf;
                            let glot_mod = if av > 0.15 { 0.38 + 0.62 * flow_env } else { 1.0 };
                            asp_src = s.asp_lp2 * ah * glot_mod * asp_gain;

                        }

                        // Band-limited glottal-chink air, gated by the open phase of each cycle.
                        if av > 0.12 {
                            let hi_air = white - s.hnm_prev * 0.84;
                            s.hnm_prev = white;
                            s.hnm_lp = s.hnm_lp * 0.44 + hi_air * 0.56;
                            hnm_src = s.hnm_lp * flow_env * av * hnm_air_gain;
                        }
                    }

                    let mut top_out = 0.0;
                    if top_gain > 0.0 {
                        let y = top_b0 * (white - 2.0 * s.top_x1 + s.top_x2) - top_a1 * s.top_y1 - top_a2 * s.top_y2;
                        s.top_x2 = s.top_x1;
                        s.top_x1 = white;
                        s.top_y2 = s.top_y1;
                        s.top_y1 = y;
                        top_out = y * av * (0.35 + 0.65 * flow_env) * top_gain;
                    }

                    let glot_total = voiced_src + asp_src + hnm_src;

                    // 3. Nasal pole-zero pair
                    let glot_total = {
                        let y = nz_norm * (glot_total + nz_b1 * s.nz_x1 + nz_b2 * s.nz_x2)
                            - np_a1 * s.nm1_1
                            - np_a2 * s.nm1_2;
                        s.nz_x2 = s.nz_x1;
                        s.nz_x1 = glot_total;
                        s.nm1_2 = s.nm1_1;
                        s.nm1_1 = y;
                        y
                    };

                    // 3b. Subglottal coupling, open phase only
                    let glot_total = if trach > 0.0 {
                        let y = tr_norm * (glot_total + tr_b1 * s.tr_x1 + tr_b2 * s.tr_x2) - tr_a1 * s.tr_y1 - tr_a2 * s.tr_y2;
                        s.tr_x2 = s.tr_x1;
                        s.tr_x1 = glot_total;
                        s.tr_y2 = s.tr_y1;
                        s.tr_y1 = y;
                        let open = if av > 0.15 { flow_env } else { 1.0 };
                        glot_total + (y - glot_total) * (trach * open).min(1.0)
                    } else {
                        glot_total
                    };

                    // 4. Oral cascade F1 -> F5
                    let (a1x, b1x, c1x) = if sti_w > 0.0 {
                        let w = flow_env * sti_w;
                        let (b, c) = (b1c + (b1o - b1c) * w, c1c + (c1o - c1c) * w);
                        (1.0 - b - c, b, c)
                    } else {
                        (a1c, b1c, c1c)
                    };
                    let y1 = (a1x * glot_total + b1x * s.y1_1 + c1x * s.y1_2) * (1.0 - flow_env * open_trach_damp);
                    s.y1_2 = s.y1_1;
                    s.y1_1 = y1;

                    let y2 = a2c * y1 + b2c * s.y2_1 + c2c * s.y2_2;
                    s.y2_2 = s.y2_1;
                    s.y2_1 = y2;

                    let y3 = a3c * y2 + b3c * s.y3_1 + c3c * s.y3_2;
                    s.y3_2 = s.y3_1;
                    s.y3_1 = y3;

                    let y4 = a4c * y3 + b4c * s.y4_1 + c4c * s.y4_2;
                    s.y4_2 = s.y4_1;
                    s.y4_1 = y4;

                    let mut y5 = a5c * y4 + b5c * s.y5_1 + c5c * s.y5_2;
                    s.y5_2 = s.y5_1;
                    s.y5_1 = y5;
                    if let Some((a6c, b6c, c6c)) = f6 {
                        let y6 = a6c * y5 + b6c * s.y6_1 + c6c * s.y6_2;
                        s.y6_2 = s.y6_1;
                        s.y6_1 = y6;
                        y5 = y6;
                    }
                    // Air: the resonances above F5/F6, and of those only the top band
                    if upper_n > 0 {
                        let mut y_up = y5;
                        for (k, &(a, b, c)) in upper[..upper_n].iter().enumerate() {
                            let y = a * y_up + b * s.up_y1[k] + c * s.up_y2[k];
                            s.up_y2[k] = s.up_y1[k];
                            s.up_y1[k] = y;
                            y_up = y;
                        }
                        for (k, &(b0, a1, a2)) in upper_hp.iter().enumerate() {
                            let h = &mut s.up_hp[4 * k..4 * k + 4];
                            let y = b0 * (y_up - 2.0 * h[0] + h[1]) - a1 * h[2] - a2 * h[3];
                            h[1] = h[0];
                            h[0] = y_up;
                            h[3] = h[2];
                            h[2] = y;
                            y_up = y;
                        }
                        y5 += y_up * upper_gain;
                    }

                    // Piriform-fossa notch (unity pass-band pole-zero pair)
                    let y_pir = pir_norm * (y5 + pir_b1 * s.pir_x1 + pir_b2 * s.pir_x2)
                        - pir_a1 * s.pir_y1
                        - pir_a2 * s.pir_y2;
                    s.pir_x2 = s.pir_x1;
                    s.pir_x1 = y5;
                    s.pir_y2 = s.pir_y1;
                    s.pir_y1 = y_pir;

                    // +6 dB/oct lip radiation, plus the parallel F2/F3 presence tap
                    let oral_cascade = (y_pir - s.prev_oral) * 1.58 * oral_open_factor;
                    s.prev_oral = y_pir;

                    let par_diff = glot_total - s.prev_par_in;
                    s.prev_par_in = glot_total;
                    let yp2 = par2_gain * par_diff + b2c * s.p2_1 + c2c * s.p2_2;
                    s.p2_2 = s.p2_1;
                    s.p2_1 = yp2;
                    let yp3 = par3_gain * par_diff + b3c * s.p3_1 + c3c * s.p3_2;
                    s.p3_2 = s.p3_1;
                    s.p3_1 = yp3;

                    let oral_raw = oral_cascade + (yp2 - yp3);
                    s.muc1 = muc_pole * s.muc1 + muc_gain * oral_raw;
                    s.muc2 = muc_pole * s.muc2 + muc_gain * s.muc1;

                    // Chest body + voice bar
                    let y_ch = a_ch * chest_in + b_ch * s.ch1 + c_ch * s.ch2;
                    s.ch2 = s.ch1;
                    s.ch1 = y_ch;

                    // Voicing that continues behind a closed mouth (b, d, g, v, z): only its lowest
                    // harmonics leak out through the cheeks and throat.
                    let y_vb = avb_c * (glot_excite * avb) + bvb * s.vb1 + cvb * s.vb2;
                    s.vb2 = s.vb1;
                    s.vb1 = y_vb;

                    // 5. Frication and bursts (generated at the lips/teeth, bypassing the tissue low-pass)
                    let mut fric_out = 0.0;
                    if has_fric {
                        let fric_mod = if av > 0.2 { 0.56 + 0.44 * flow_env } else { 1.0 };
                        // the pressure pulse of a stop release, ahead of its noise
                        let fr_in = white * af * fric_mod + release_click;
                        release_click = 0.0;

                        let y_fr1 = gf1 * (fr_in - rf1 * s.fr1_x2) + bf1 * s.fr1_y1 + cf1 * s.fr1_y2;
                        s.fr1_x2 = s.fr1_x1;
                        s.fr1_x1 = fr_in;
                        s.fr1_y2 = s.fr1_y1;
                        s.fr1_y1 = y_fr1;

                        let y_fr2 = gf2 * (fr_in - rf2 * s.fr2_x2) + bf2 * s.fr2_y1 + cf2 * s.fr2_y2;
                        s.fr2_x2 = s.fr2_x1;
                        s.fr2_x1 = fr_in;
                        s.fr2_y2 = s.fr2_y1;
                        s.fr2_y1 = y_fr2;

                        fric_out = (y_fr1 + y_fr2) * 2.10;
                        if hp_on {
                            for k in 0..2 {
                                let (x1, x2) = (s.hp_x[2 * k], s.hp_x[2 * k + 1]);
                                let (y1, y2) = (s.hp_y[2 * k], s.hp_y[2 * k + 1]);
                                let y = hp_b0 * (fric_out - 2.0 * x1 + x2) - hp_a1 * y1 - hp_a2 * y2;
                                s.hp_x[2 * k + 1] = x1;
                                s.hp_x[2 * k] = fric_out;
                                s.hp_y[2 * k + 1] = y1;
                                s.hp_y[2 * k] = y;
                                fric_out = y;
                            }
                        }
                    }

                    // Mix, a whisper of early room reflection, DC blocker, soft limiter
                    // Everything that came through the throat gets the voice's tone curve;
                    // noise made at the lips and teeth does not.
                    let mut voiced_out = s.muc2 + y_ch * chest_mix + y_vb * VOICE_BAR_MIX;
                    if let Some(bands) = &tone_eq {
                        for (k, c) in bands.iter().enumerate() {
                            let y = c[0] * voiced_out + s.eq_z1[k];
                            s.eq_z1[k] = c[1] * voiced_out - c[3] * y + s.eq_z2[k];
                            s.eq_z2[k] = c[2] * voiced_out - c[4] * y;
                            voiced_out = y;
                        }
                    }
                    let dry = voiced_out + fric_out + asp_air + top_out;
                    let r_tap1 = self.room_buf[s.room_idx.wrapping_sub(203) & 511] as f64;
                    let r_tap2 = self.room_buf[s.room_idx.wrapping_sub(347) & 511] as f64;
                    self.room_buf[s.room_idx] = dry as f32;
                    s.room_idx = (s.room_idx + 1) & 511;
                    let mut studio_sig = dry + r_tap1 * 0.044 - r_tap2 * 0.025;
                    if room_on {
                        studio_sig += self.room.process(studio_sig);
                    }

                    let clean = studio_sig - s.dc_x1 + 0.993 * s.dc_y1;
                    s.dc_x1 = studio_sig;
                    s.dc_y1 = clean;

                    let mut v = clean * out_gain;
                    if v > 0.72 {
                        let ex = v - 0.72;
                        v = 0.72 + 0.24 * (ex / (0.28 + ex));
                    } else if v < -0.72 {
                        let ex = -v - 0.72;
                        v = -0.72 - 0.24 * (ex / (0.28 + ex));
                    }

                    out[write_idx] = v as f32;
                    write_idx += 1;
                    frame_sq_sum += v * v;
                }
            }

            if is_real_frame {
                let raw_rms = (frame_sq_sum / SAMPLES_PER_FRAME as f64).sqrt();
                self.frames[fi].rms = clamp(clamp(raw_rms * 3.1, 0.0, 1.0) * self.energy_scale, 0.0, 1.0);
            }
        }

        if self.frame_idx >= total_frames && self.tail_frames_left == 0 {
            self.done = true;
        }
        self.st = s;
        write_idx
    }

    /// A stream cannot rescale the loudness of its frames to 0..1 afterwards as `render_all`
    /// does: it sets the scale beforehand, from a test render (1 / the loudest frame).
    pub fn set_energy_scale(&mut self, scale: f64) {
        self.energy_scale = scale;
    }

    /// Renders the whole utterance offline: peak-normalized to 0.86 with short fades,
    /// and per-frame loudness rescaled to 0..1. Consumes the renderer's frames once.
    pub fn render_all(&mut self) -> Vec<f32> {
        let n_frames = self.frames.len();
        let total = n_frames * SAMPLES_PER_FRAME;
        let mut pcm = vec![0.0f32; total];
        if total == 0 {
            return pcm;
        }
        self.render(n_frames, &mut pcm);

        let mut peak_abs = 0.001f64;
        for &v in &pcm {
            let a = (v as f64).abs();
            if a > peak_abs {
                peak_abs = a;
            }
        }
        let gain = normalization_gain(peak_abs);
        let fade = if total > 576 {
            144
        } else {
            (total as f64 * 0.25).floor() as usize
        };
        let tail_start = total - fade;
        let inv_fade = if fade > 0 { 1.0 / fade as f64 } else { 1.0 };

        for (i, sample) in pcm.iter_mut().enumerate() {
            let mut g = gain;
            if i < fade {
                g = gain * (i as f64 * inv_fade);
            } else if i > tail_start {
                g = gain * ((total - i) as f64 * inv_fade);
            }
            let v = clamp(*sample as f64 * g, -0.96, 0.96);
            *sample = v as f32;
        }

        let max_rms = self.frames.iter().map(|f| f.rms).fold(0.001f64, f64::max);
        let inv_max_rms = 1.0 / max_rms;
        for f in self.frames.iter_mut() {
            f.rms = clamp(f.rms * inv_max_rms, 0.0, 1.0);
        }
        pcm
    }
}
