//! Sounds a voice makes that are not words: laughter in its kinds, breaths and sighs, coughs
//! and sneezes, the hums and grunts of a listener ("mm-hm", "uh-oh"), shushing, whistling.
//!
//! None of them is a recording. Each is a short performance for the same vocal tract that
//! speaks: a sequence of poses (where the pitch is, how open the mouth, how much voice,
//! breath and hiss) that the renderer is driven through, so every voice - any pitch, any
//! tract length, a whisper - laughs and sighs as itself.
//!
//! In a script they are tags (`[laugh]`, `[sigh]`, `[giggle intensity=0.8 length=1.5]`); from
//! code, [`crate::TTS::vocalize`].
//!
//! The shapes follow what is measured in people. A laugh is a train of short voiced calls
//! ("ha") about a fifth of a second apart, each followed by a puff of breath, the pitch well
//! above the speaking voice and sinking, the calls growing longer and weaker (Bachorowski,
//! Smoski & Owren 2001; Provine 2000). A giggle is the same thing faster, higher and closer
//! in the mouth; a chuckle lower, shorter and often behind closed lips.

use crate::config::{Gender, VoiceConfig};
use crate::frames::{Frame, FRAME_MS};
use crate::phone::Ph;
use crate::tables::{self, Spec};
use crate::timeline::clamp;

const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;

/// One of the sounds, as the script tag `[id]` and [`crate::TTS::vocalize`] name it.
pub struct VocalSound {
    pub id: &'static str,
    pub label: &'static str,
    /// What kind of sound: one of [`GROUPS`].
    pub group: &'static str,
    /// One line on what it is.
    pub about: &'static str,
    aliases: &'static [&'static str],
    /// What the sound asks of the voice itself (a cackle is rough, a groan is not breathy).
    voice: Option<fn(&mut VoiceConfig)>,
    perform: fn(&mut Take),
}

impl std::fmt::Debug for VocalSound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VocalSound({:?})", self.id)
    }
}

/// The kinds of sound, in the order a list of them reads best.
pub const GROUPS: [&str; 6] = ["laughter", "breath", "reaction", "voice", "mouth", "body"];

/// Finds a sound by its id or another name for it ("haha", "tee-hee", "throat"); case,
/// hyphens, underscores and spaces do not matter.
pub fn find(name: &str) -> Option<&'static VocalSound> {
    let key: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).map(|c| c.to_ascii_lowercase()).collect();
    if key.is_empty() {
        return None;
    }
    let same = |s: &str| s.chars().filter(|c| c.is_ascii_alphanumeric()).eq(key.chars());
    VOCAL_SOUNDS.iter().find(|s| same(s.id) || s.aliases.iter().any(|a| same(a)))
}

/// The control frames of one performance of `sound` by the voice `cfg`, and that voice as the
/// sound colours it (render the frames with the returned configuration).
///
/// `intensity` (0..1, usually 0.6) is how strongly, `length` (1 = usual) how long. `take`
/// tells apart the occurrences within one script, so that two laughs are not twice the same
/// laugh; with the voice's `seed` it decides the small irregularities.
pub fn perform(sound: &VocalSound, cfg: &VoiceConfig, intensity: f64, length: f64, take: u32) -> (Vec<Frame>, VoiceConfig) {
    let length = if length.is_finite() { clamp(length, 0.25, 4.0) } else { 1.0 };
    perform_stretched(sound, cfg, intensity, length, 1.0, take)
}

/// Like [`perform`], lasting `seconds` - as nearly as the sound allows. A laugh of four
/// seconds has more calls in it, a sigh of two is drawn out; a hiccup or a click of the
/// tongue is what it is, and comes out at its own length.
pub fn perform_for(sound: &VocalSound, cfg: &VoiceConfig, intensity: f64, seconds: f64, take: u32) -> (Vec<Frame>, VoiceConfig) {
    if !seconds.is_finite() || seconds <= 0.0 {
        return perform(sound, cfg, intensity, 1.0, take);
    }
    let want = (seconds * 1000.0 / FRAME_MS).round().max(4.0);
    let frames_at = |length: f64| perform_stretched(sound, cfg, intensity, length, 1.0, take).0.len() as f64;
    // more of it (more calls, more coughs, a longer breath): the length that comes closest
    let (mut lo, mut hi) = (0.1f64, 16.0f64);
    let mut best = (1.0, frames_at(1.0));
    for _ in 0..14 {
        let mid = (lo * hi).sqrt();
        let got = frames_at(mid);
        if (got - want).abs() < (best.1 - want).abs() {
            best = (mid, got);
        }
        if got < want {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // ... and the rest by taking it a little slower or faster
    let stretch = clamp(want / best.1.max(1.0), 0.75, 1.35);
    perform_stretched(sound, cfg, intensity, best.0, stretch, take)
}

fn perform_stretched(sound: &VocalSound, cfg: &VoiceConfig, intensity: f64, length: f64, stretch: f64, take: u32) -> (Vec<Frame>, VoiceConfig) {
    let mut voice = cfg.clone();
    voice.sanitize();
    if let Some(colour) = sound.voice {
        colour(&mut voice);
        voice.sanitize();
    }
    let mut seed = 0x9e37_79b9u32 ^ voice.seed.wrapping_mul(0x85eb_ca6b) ^ take.wrapping_mul(0xc2b2_ae35);
    for b in sound.id.bytes() {
        seed = (seed ^ b as u32).wrapping_mul(0x0100_0193);
    }
    let female = voice.gender == Gender::Female;
    let fs = 17.5 / clamp(voice.tract_length_cm, 12.2, 21.5);
    let mut t = Take {
        cfg: &voice,
        frames: Vec::new(),
        now: Pose::rest(),
        rng: seed | 1,
        intensity: if intensity.is_finite() { clamp(intensity, 0.0, 1.0) } else { 0.6 },
        length: if length.is_finite() { clamp(length, 0.05, 16.0) } else { 1.0 },
        stretch,
        // (even a perfectly regular voice does not laugh like a metronome)
        wobble: 0.5 + clamp(voice.variation, 0.0, 1.0),
        female,
        scale: [
            (1.0 + (fs - 1.0) * if female { 0.88 } else { 0.75 }) * voice.formants[0],
            fs * voice.formants[1],
            (1.0 + (fs - 1.0) * 0.90) * voice.formants[2],
            fs,
        ],
    };
    (sound.perform)(&mut t);
    // let the last pose die away instead of stopping dead
    let mut end = t.now;
    end.quiet();
    t.to(25.0, end);
    let frames = t.frames;
    (frames, voice)
}

// ------------------------------------------------------------------------------ poses

/// Where the frication noise of a pose is made.
#[derive(Clone, Copy)]
struct Hiss {
    f1: f64,
    b1: f64,
    f2: f64,
    b2: f64,
    g1: f64,
    g2: f64,
    hp: f64,
    lp: f64,
}

/// What the voice is doing at one moment. Frequencies are those of the reference tract
/// (17.5 cm); the voice's own size is applied when the frames are made.
#[derive(Clone, Copy)]
struct Pose {
    /// Pitch as a multiple of the voice's own.
    pitch: f64,
    f: [f64; 3],
    b: [f64; 3],
    /// Voice, breath (noise at the glottis), hiss (noise at the mouth), voice behind closed lips.
    av: f64,
    ah: f64,
    af: f64,
    avb: f64,
    /// How much of the breath is also heard as a hiss at the lips and teeth. Breath that has
    /// come through the whole throat is dull; a real breath, in or out, is airy to the top.
    air: f64,
    nasal: f64,
    seal: f64,
    fnz: f64,
    lax: f64,
    creak: f64,
    hiss: Hiss,
    /// A whistled tone: its pitch (Hz, not scaled by the tract) and level.
    tone_hz: f64,
    tone: f64,
    /// jaw, tongue height, tongue frontness, tongue tip, lip rounding, lip spreading, lip
    /// closure, velum (for a face to follow).
    mouth: [f64; 8],
    phone: Ph,
    burst: bool,
    closure: bool,
}

impl Pose {
    /// The mouth shape of a table entry, silent.
    fn of(s: &Spec, phone: Ph) -> Pose {
        Pose {
            pitch: 1.0,
            f: [s.f1, s.f2, s.f3],
            b: [s.b1, s.b2, s.b3],
            av: 0.0,
            ah: 0.0,
            af: 0.0,
            avb: 0.0,
            air: 0.0,
            nasal: s.nasal,
            seal: s.oral_seal,
            fnz: s.fnz,
            lax: 0.2,
            creak: 0.0,
            hiss: Hiss { f1: s.fric_f1, b1: s.fric_b1, f2: s.fric_f2, b2: s.fric_b2, g1: s.fric_g1, g2: s.fric_g2, hp: s.fric_hp, lp: s.fric_lp },
            tone_hz: 1500.0,
            tone: 0.0,
            mouth: [s.jaw, s.t_h, s.t_f, s.t_tip, s.lip_r, s.lip_s, s.lip_c, s.velum],
            phone,
            burst: false,
            closure: false,
        }
    }

    fn rest() -> Pose {
        let mut p = Pose::of(&tables::SPEC_SIL, Ph::SIL);
        p.closure = true;
        p
    }

    /// The same mouth, making no sound.
    fn quiet(&mut self) {
        self.av = 0.0;
        self.ah = 0.0;
        self.af = 0.0;
        self.avb = 0.0;
        self.tone = 0.0;
        self.burst = false;
    }

    fn voiced(mut self, level: f64) -> Pose {
        self.av = level;
        self
    }

    fn breath(mut self, level: f64) -> Pose {
        self.ah = level;
        self
    }

    fn hissing(mut self, level: f64) -> Pose {
        self.af = level;
        self
    }

    fn at(mut self, pitch: f64) -> Pose {
        self.pitch = pitch;
        self
    }

    fn lax(mut self, lax: f64) -> Pose {
        self.lax = lax;
        self
    }

    fn creaky(mut self, creak: f64) -> Pose {
        self.creak = creak;
        self
    }

    fn jaw(mut self, jaw: f64) -> Pose {
        self.mouth[0] = jaw;
        self
    }

    fn nasalized(mut self, nasal: f64) -> Pose {
        self.nasal = nasal;
        self.mouth[7] = self.mouth[7].max(nasal * 0.9);
        self
    }

    fn silent(mut self) -> Pose {
        self.quiet();
        self
    }

}

fn mix(a: f64, b: f64, u: f64) -> f64 {
    a + (b - a) * u
}

fn smoothstep(u: f64) -> f64 {
    let u = clamp(u, 0.0, 1.0);
    u * u * (3.0 - 2.0 * u)
}

/// The hiss of air at the lips and teeth that goes with any breath.
const AIR: Hiss = Hiss { f1: 3200.0, b1: 3500.0, f2: 6500.0, b2: 4500.0, g1: 0.5, g2: 0.5, hp: 1700.0, lp: 0.0 };

/// The rush of a breath through an open mouth: a low rumble and the hiss of air together.
/// Recordings of breathing have a fifth of their energy below 500 Hz, as much again in each
/// of the next two octaves, and a few percent right up to 8 kHz; breath noise that has only
/// come through the throat's resonances sits between 500 Hz and 4 kHz, and sounds like sawing.
const RUSH: Hiss = Hiss { f1: 320.0, b1: 560.0, f2: 4800.0, b2: 5200.0, g1: 0.8, g2: 0.16, hp: 0.0, lp: 0.0 };

/// A pitch `st` semitones away, as a factor.
fn st(st: f64) -> f64 {
    2f64.powf(st / 12.0)
}

/// One performance being built.
struct Take<'a> {
    cfg: &'a VoiceConfig,
    frames: Vec<Frame>,
    now: Pose,
    rng: u32,
    /// How strongly (0..1) and how long (1 = usual) the sound was asked for.
    intensity: f64,
    length: f64,
    /// How uneven this voice is (1 = the usual amount).
    wobble: f64,
    /// Everything this much slower (to fit a duration asked for).
    stretch: f64,
    female: bool,
    /// The voice's factors for F1, F2, F3 and for noise frequencies.
    scale: [f64; 4],
}

impl Take<'_> {
    fn uniform(&mut self) -> f64 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        self.rng as f64 / u32::MAX as f64
    }

    /// Roughly standard-normal.
    fn gauss(&mut self) -> f64 {
        (self.uniform() + self.uniform() + self.uniform() + self.uniform() - 2.0) * 1.732
    }

    /// A factor around 1 with the given spread, scaled by how uneven the voice is.
    fn about(&mut self, spread: f64) -> f64 {
        (spread * self.wobble * self.gauss()).exp()
    }

    /// The mouth shape of a vowel as this voice says it.
    fn vowel(&self, ph: Ph) -> Pose {
        let mut p = Pose::of(tables::spec(ph).unwrap_or(&tables::SPEC_AX), ph);
        if let Some(vi) = crate::config::vowel_index(ph) {
            let m = self.cfg.vowel_shape(vi);
            for (f, scale) in p.f.iter_mut().zip(m) {
                *f *= scale;
            }
        }
        p
    }

    /// The two ends of a diphthong.
    fn glide(&self, ph: Ph) -> (Pose, Pose) {
        let Some(d) = tables::diph(ph) else {
            return (self.vowel(ph), self.vowel(ph));
        };
        let (mut a, mut b) = (self.vowel(d.a), self.vowel(d.b));
        let m = crate::config::vowel_index(ph).map_or([1.0; 3], |vi| self.cfg.vowel_shape(vi));
        a.f[0] = d.f1a * m[0];
        a.f[1] = d.f2a * m[1];
        b.f[0] = d.f1b * m[0];
        b.f[1] = d.f2b * m[1];
        a.phone = ph;
        b.phone = ph;
        (a, b)
    }

    /// Breath through a mouth shape: an /h/ with the colour of that vowel.
    fn breathed(&self, mut v: Pose, level: f64) -> Pose {
        v.av = 0.0;
        v.ah = level;
        v.b[0] += 190.0;
        v.b[1] += 70.0;
        v.b[2] += 50.0;
        v.lax = 0.6;
        v.phone = Ph::HH;
        v.hiss = AIR;
        v.air = 0.13;
        v
    }

    /// The open, slack mouth of a breath in or out.
    fn airway(&self, inward: bool) -> Pose {
        let mut p = Pose::rest();
        p.closure = false;
        // breathing in, the larynx drops and the mouth is a little wider: a duller noise
        p.f = if inward { [560.0, 1650.0, 2600.0] } else { [610.0, 1420.0, 2500.0] };
        p.b = [270.0, 220.0, 240.0];
        p.lax = 0.6;
        p.mouth = [0.26, 0.42, 0.5, 0.16, 0.05, 0.04, 0.0, 0.15];
        p.hiss = RUSH;
        // (drawn in over the teeth there is more of it than let out)
        p.air = if inward { 0.75 } else { 0.6 };
        p
    }

    /// Lips closed, sound through the nose: little but the lowest resonance gets out.
    fn hum(&self) -> Pose {
        let mut p = Pose::of(&tables::SPEC_M, Ph::M);
        p.f = [290.0, 1100.0, 2250.0];
        p.b = [80.0, 420.0, 480.0];
        p.lax = 0.6;
        p
    }

    /// Moves from the present pose to `target` over `ms`.
    fn to(&mut self, ms: f64, target: Pose) {
        self.shape_to(ms, target, |_, _| {});
    }

    /// Stays in the present pose.
    fn hold(&mut self, ms: f64) {
        let now = self.now;
        self.to(ms, now);
    }

    /// Silence, the mouth staying where it is.
    fn pause(&mut self, ms: f64) {
        let mut p = self.now;
        p.quiet();
        p.closure = true;
        self.to(ms.min(15.0), p);
        if ms > 15.0 {
            self.hold(ms - 15.0);
        }
    }

    /// Like [`Take::to`], with a last word on every frame (`shape(ms since the start, pose)`):
    /// for trills and tremors.
    fn shape_to(&mut self, ms: f64, target: Pose, shape: impl Fn(f64, &mut Pose)) {
        let n = ((ms * self.stretch / FRAME_MS).round() as usize).max(1);
        let from = self.now;
        for i in 0..n {
            let u = (i + 1) as f64 / n as f64;
            let s = smoothstep(u);
            let mut p = target;
            p.pitch = from.pitch * (target.pitch / from.pitch).powf(u);
            for k in 0..3 {
                p.f[k] = mix(from.f[k], target.f[k], s);
                p.b[k] = mix(from.b[k], target.b[k], s);
            }
            p.av = mix(from.av, target.av, u);
            p.ah = mix(from.ah, target.ah, u);
            p.af = mix(from.af, target.af, u);
            p.avb = mix(from.avb, target.avb, u);
            p.air = mix(from.air, target.air, u);
            p.nasal = mix(from.nasal, target.nasal, s);
            p.seal = mix(from.seal, target.seal, s);
            p.fnz = mix(from.fnz, target.fnz, s);
            p.lax = mix(from.lax, target.lax, u);
            p.creak = mix(from.creak, target.creak, u);
            p.tone = mix(from.tone, target.tone, u);
            p.tone_hz = from.tone_hz * (target.tone_hz / from.tone_hz).powf(u);
            for k in 0..8 {
                p.mouth[k] = mix(from.mouth[k], target.mouth[k], s);
            }
            // noise keeps the colour it had until the new one is audible
            if target.af < 0.002 && from.af > 0.002 {
                p.hiss = from.hiss;
            }
            shape(i as f64 * FRAME_MS, &mut p);
            self.emit(&p);
        }
        self.now = target;
    }

    fn emit(&mut self, p: &Pose) {
        let cfg = self.cfg;
        let breathiness = clamp(cfg.breathiness, 0.0, 0.85);
        let [s1, s2, s3, fs] = self.scale;
        let f1 = clamp(p.f[0] * s1, 200.0, 1180.0);
        let f2 = clamp(p.f[1] * s2, 540.0, 3250.0);
        let f3 = clamp(p.f[2] * s3, 1380.0, 4150.0);
        // the same damping the spoken frames get (see frames::rasterize)
        let open_damp = ((f1 - 460.0) * 0.07).max(0.0);
        let prox_damp = ((460.0 - (f2 - f1)) * 0.12).max(0.0);
        let wall_damp = if self.female { 0.0 } else { 14.0 * (450.0 / f1).powi(2) };
        let noise = 1.0 + (fs - 1.0) * 0.60;
        let (av, ah) = if cfg.whisper { (0.0, p.ah.max(p.av * 0.65)) } else { (p.av, p.ah + p.av * breathiness * 0.16) };
        let af = p.af + p.ah * p.air;
        let i = self.frames.len();
        self.frames.push(Frame {
            t_sec: i as f64 * FRAME_MS / 1000.0,
            clause_idx: -1,
            micro_f0: 0.0,
            f0: clamp(cfg.pitch_hz * p.pitch, 55.0, 500.0),
            f1,
            f2,
            f3,
            b1: p.b[0] * (1.0 + breathiness * 0.20) + wall_damp + open_damp + prox_damp + if self.female { 36.0 } else { 0.0 },
            b2: p.b[1] * (1.0 + breathiness * 0.16) + prox_damp * 0.75 + if self.female { 18.0 } else { 0.0 },
            b3: p.b[2] * (1.0 + breathiness * 0.12) + if self.female { 10.8 } else { 0.0 },
            av,
            ah,
            af,
            avb: if cfg.whisper { 0.0 } else { p.avb },
            nasal: if cfg.whisper { 0.0 } else { clamp(p.nasal, 0.0, 1.0) },
            oral_seal: clamp(p.seal, 0.0, 1.0),
            fnz: p.fnz * s1,
            lax: clamp(p.lax, 0.0, 0.9),
            creak: clamp(p.creak, 0.0, 1.0),
            fric_f1: clamp(p.hiss.f1 * (1.0 + (fs - 1.0) * 0.65), 250.0, 9500.0),
            fric_b1: p.hiss.b1 * fs,
            fric_f2: clamp(p.hiss.f2 * noise, 600.0, 10200.0),
            fric_b2: p.hiss.b2 * fs,
            fric_g1: p.hiss.g1,
            fric_g2: p.hiss.g2,
            fric_hp: p.hiss.hp * noise,
            fric_lp: p.hiss.lp * noise,
            tone_hz: clamp(p.tone_hz, 200.0, 5000.0),
            tone: clamp(p.tone, 0.0, 1.5),
            is_closure: p.closure,
            is_burst: p.burst,
            stress: 0,
            jaw: p.mouth[0],
            t_h: p.mouth[1],
            t_f: p.mouth[2],
            t_tip: p.mouth[3],
            lip_r: p.mouth[4],
            lip_s: p.mouth[5],
            lip_c: p.mouth[6],
            velum: p.mouth[7],
            phone: p.phone,
            word: -1,
            rms: clamp(av * 0.88 + af * 0.58 + ah * 0.42 + p.avb * 0.35 + p.tone * 0.5, 0.0, 1.0),
        });
    }

    /// A pitch for laughing or crying, as a multiple of the speaking pitch. People laugh far
    /// above where they speak, and low voices more so than high ones: men's laughs average
    /// about 280 Hz and women's about 420 (Bachorowski et al. 2001), 2.5 and 2.1 times an
    /// ordinary speaking pitch.
    fn lifted(&self, ratio: f64) -> f64 {
        ratio * (140.0 / clamp(self.cfg.pitch_hz, 60.0, 350.0)).powf(0.3)
    }

    /// How loud for the intensity asked: half as loud when barely, a little over when fully.
    fn level(&self, level: f64) -> f64 {
        level * (0.55 + 0.65 * self.intensity)
    }

    /// A breath in (`inward`) or out: noise rising over `rise` of its length and dying away.
    fn breath(&mut self, ms: f64, level: f64, inward: bool, rise: f64) {
        let a = self.airway(inward);
        let mut start = a;
        start.mouth = self.now.mouth;
        start.f = self.now.f;
        self.now = start.silent();
        self.to(ms * rise, a.breath(level * 0.8));
        self.to(ms * (1.0 - rise), a.breath(level * 0.1));
    }

    /// The release of a closure at the lips, the teeth ridge or the soft palate.
    fn pop(&mut self, stop: Ph, level: f64, ms: f64) {
        let Some(s) = tables::stop(stop) else { return };
        let (g1, g2, hp) = match s.place {
            tables::Place::Labial => (0.85, 0.05, 0.0),
            tables::Place::Alveolar => (0.55, 0.55, 2600.0),
            tables::Place::Velar => (0.92, 0.05, 900.0),
            tables::Place::Other => (0.68, 0.50, 1800.0),
        };
        let mut p = self.now;
        p.quiet();
        p.hiss = Hiss { f1: s.b_f1, b1: s.b_b1, f2: s.b_f2, b2: s.b_b2, g1, g2, hp, lp: 0.0 };
        p.af = level;
        p.burst = true;
        p.closure = false;
        p.phone = stop;
        p.mouth[6] = s.lip_c * 0.2;
        self.now = p;
        self.hold(ms.max(FRAME_MS));
        p.quiet();
        self.now = p;
    }
}

// ---------------------------------------------------------------------------- laughter

/// A kind of laughter: a train of voiced calls with a puff of breath after each.
#[derive(Clone, Copy)]
struct Laugh {
    /// Calls ("ha") at the usual length.
    calls: f64,
    /// Calls per second at the start, and how much longer each is than the one before.
    rate: f64,
    slowing: f64,
    /// Pitch of the first call (a multiple of the speaking pitch), its change from call to
    /// call (semitones), an arch over the whole train (semitones at its middle) and the fall
    /// inside each call (semitones).
    pitch: f64,
    drift: f64,
    arch: f64,
    fall: f64,
    vowel: Ph,
    /// The share of each call that is voiced; the rest is breath.
    voiced: f64,
    breath: f64,
    /// How much of that breath hisses at the teeth as well (see `Pose::air`).
    air: f64,
    /// How much of the sound goes through the nose (a laugh held in).
    nasal: f64,
    level: f64,
    /// Level of each call against the one before.
    fade: f64,
    lax: f64,
    creak: f64,
    /// Behind closed lips ("hm-hm-hm").
    closed: bool,
    /// How irregular (1 = as people are).
    ragged: f64,
    /// How much of a breath in follows (0 = none).
    recover: f64,
}

/// The plain laugh. Its numbers were set against a tagger trained on two million clips of
/// real sound (tools/sounds.py): of what it hears in this laugh, the share that is laughter
/// went from a tenth to seven tenths with a softer onset of each call (25 ms), a steeper fall
/// of the pitch inside it, a full puff of breath after it - and without the hiss of air at
/// the teeth, which makes a gasp of it.
const LAUGH: Laugh = Laugh {
    calls: 5.0,
    rate: 5.5,
    slowing: 0.05,
    pitch: 2.2,
    drift: -1.1,
    arch: 0.0,
    fall: 3.5,
    vowel: Ph::AH,
    voiced: 0.42,
    breath: 0.5,
    air: 0.0,
    nasal: 0.0,
    level: 0.9,
    fade: 0.90,
    lax: 0.32,
    creak: 0.0,
    closed: false,
    ragged: 1.0,
    recover: 0.0,
};

/// One train of calls. Returns the pitch it ended on.
fn laugh_train(t: &mut Take, l: &Laugh, calls: usize, start_pitch: f64) -> f64 {
    // (development hooks: see tools/sounds.py)
    let l = &Laugh {
        breath: l.breath * crate::tune("lv_breath", 1.0),
        voiced: clamp(l.voiced * crate::tune("lv_voiced", 1.0), 0.2, 0.8),
        rate: l.rate * crate::tune("lv_rate", 1.0),
        fall: l.fall * crate::tune("lv_fall", 1.0),
        lax: clamp(l.lax + crate::tune("lv_lax", 0.0), 0.0, 0.9),
        drift: l.drift * crate::tune("lv_drift", 1.0),
        ..*l
    };
    let start_pitch = start_pitch * crate::tune("lv_pitch", 1.0);
    let attack_ms = crate::tune("lv_attack", 25.0);
    let air = l.air + crate::tune("lv_air", 0.0);
    let open = if l.closed { t.hum() } else { t.vowel(l.vowel).nasalized(l.nasal) };
    let level = t.level(l.level);
    // A stronger laugh sits higher. And a low voice starts higher still above its speaking
    // pitch: each call falls and the train sinks, so a man's laugh that only begins at the
    // 2.5 times of `lifted` averages well under it - and is heard as a quack, not a laugh.
    let low_voice = clamp((220.0 - t.cfg.pitch_hz) / 70.0, 0.0, 1.0);
    let top = t.lifted(start_pitch) * (1.0 + crate::tune("lv_low_lift", 0.17) * low_voice) * (1.0 + 0.22 * (t.intensity - 0.6));
    let h = |t: &Take, amount: f64| {
        if l.closed {
            // breath through the nose
            let mut p = open.silent().breath(amount * 0.7);
            p.b[0] += 120.0;
            p
        } else {
            let mut p = t.breathed(open, amount);
            p.air = air;
            p
        }
    };
    // the first puff of breath that starts it
    t.now = h(t, 0.0);
    t.to(55.0, h(t, l.breath * level * 0.8));
    let mut pitch = top;
    for k in 0..calls {
        let period = 1000.0 / l.rate * (1.0 + l.slowing).powi(k as i32) * t.about(0.07 * l.ragged);
        let voiced = period * l.voiced;
        let rest = period - voiced;
        let along = if calls > 1 { k as f64 / (calls - 1) as f64 } else { 0.0 };
        pitch = top * st(l.drift * k as f64 + l.arch * (PI * along).sin()) * t.about(0.025 * l.ragged);
        // the second call is the strongest, then they die away
        let loud = level * if k == 0 { 0.92 } else { l.fade.powi(k as i32 - 1) } * t.about(0.08 * l.ragged);
        let v = open.lax(l.lax).creaky(l.creak);
        let attack = attack_ms.min(voiced * 0.3);
        t.to(attack, v.voiced(loud * 0.9).at(pitch * st(0.5)));
        t.to(voiced * 0.35, v.voiced(loud).at(pitch));
        t.to((voiced * 0.65 - attack).max(FRAME_MS), v.voiced(loud * 0.72).at(pitch * st(-l.fall)).lax(l.lax + 0.12).jaw(open.mouth[0] * 0.8));
        // ... and the breath that follows it
        let puff = l.breath * (0.45 + 0.55 * loud / level.max(1e-6)) * level;
        t.to(20.0, h(t, puff).at(pitch * st(-l.fall)).jaw(open.mouth[0] * 0.7));
        t.to((rest * 0.45).max(FRAME_MS), h(t, puff * 0.8).at(pitch * st(-l.fall)).jaw(open.mouth[0] * 0.62));
        t.to((rest * 0.55 - 20.0).max(FRAME_MS), h(t, puff * 0.4).at(pitch * st(-l.fall + 0.5)).jaw(open.mouth[0] * 0.7));
    }
    // the breath runs out
    let tail = h(t, 0.0).at(pitch * st(-l.fall));
    t.to(130.0, tail);
    pitch
}

fn laugh(t: &mut Take, l: &Laugh) {
    let calls = (l.calls * t.length * (0.8 + 0.4 * t.intensity)).round().clamp(1.0, 40.0) as usize;
    laugh_train(t, l, calls, l.pitch);
    let recover = l.recover * (0.4 + t.intensity) * crate::tune("lv_recover", 1.0);
    if recover > 0.45 && calls >= 3 {
        // out of air: the breath back in
        let wait = 70.0 + 60.0 * t.uniform();
        t.pause(wait);
        t.breath(260.0 + 240.0 * recover.min(1.2), 0.16 + 0.2 * recover.min(1.2), true, 0.4);
    }
}

fn laugh_plain(t: &mut Take) {
    laugh(t, &LAUGH);
}

/// Laughing voices are breathier than speaking ones: the folds never quite close.
fn laughing_voice(v: &mut VoiceConfig) {
    v.breathiness = (v.breathiness + 0.12).min(0.85);
    v.vibrato = 0.0;
    // (recorded laughs have a tenth of their energy above 2 kHz: the folds come together hard)
    v.closure = (v.closure + crate::tune("lv_closure", 0.2)).min(1.0);
}

fn chuckle(t: &mut Take) {
    // barely amused, it stays behind the lips
    let closed = t.intensity < 0.3;
    laugh(
        t,
        &Laugh { calls: 3.5, rate: 6.0, pitch: 1.7, drift: -1.5, fall: 3.1, vowel: Ph::AH, voiced: 0.40, breath: 0.45, level: 0.8, fade: 0.82, lax: 0.45, closed, ..LAUGH },
    );
}

fn giggle(t: &mut Take) {
    laugh(
        t,
        &Laugh { calls: 7.0, rate: 7.8, slowing: 0.03, pitch: 2.75, drift: -0.6, arch: 1.5, fall: 2.8, vowel: Ph::IH, voiced: 0.45, breath: 0.39, level: 0.85, fade: 0.94, lax: 0.3, ..LAUGH },
    );
}

fn guffaw(t: &mut Take) {
    let l = Laugh { calls: 5.0, rate: 4.8, slowing: 0.06, pitch: 2.35, drift: -1.3, fall: 5.6, vowel: Ph::AA, voiced: 0.5, breath: 0.56, level: 0.95, fade: 0.92, lax: 0.2, ..LAUGH };
    let first = (l.calls * t.length * (0.8 + 0.4 * t.intensity)).round().clamp(2.0, 30.0) as usize;
    laugh_train(t, &l, first, l.pitch);
    // a snatch of air, and off again, a little lower and shorter
    t.pause(30.0);
    let mut gulp = t.airway(true);
    gulp.air = 0.0;
    t.now = gulp.silent();
    t.to(60.0, gulp.breath(0.4));
    t.to(100.0, gulp.breath(0.1));
    t.pause(30.0);
    laugh_train(t, &l, (first * 3 / 4).max(2), l.pitch * 0.9);
    // out of air
    t.pause(80.0);
    t.now = gulp.silent();
    t.to(130.0, gulp.breath(0.3));
    t.to(230.0, gulp.breath(0.05));
}

fn cackle(t: &mut Take) {
    // a drawn-out first cry, then the rattle
    let v = t.vowel(Ph::AE);
    t.now = t.breathed(v, 0.0);
    let level = t.level(0.8);
    let top = t.lifted(2.8);
    t.to(40.0, t.breathed(v, 0.2 * level).at(top * 0.7));
    t.to(30.0, v.voiced(level * 0.9).at(top * 0.72).lax(0.05));
    t.to(170.0 * t.length.sqrt(), v.voiced(level).at(top).lax(0.05));
    t.to(40.0, t.breathed(v, 0.2 * level).at(top * 0.98));
    laugh(
        t,
        &Laugh { calls: 7.0, rate: 6.4, slowing: 0.035, pitch: 2.8, drift: -0.45, arch: 1.2, fall: 4.2, vowel: Ph::AE, voiced: 0.5, breath: 0.31, level: 0.8, fade: 0.95, lax: 0.05, ..LAUGH },
    );
}

fn cackle_voice(v: &mut VoiceConfig) {
    v.roughness = (v.roughness + 0.22).min(0.75);
    v.closure = (v.closure + 0.2).min(1.0);
    v.breathiness = (v.breathiness - 0.08).max(0.0);
    v.nasality = (v.nasality + 0.12).min(0.8);
}

fn snicker(t: &mut Take) {
    // a laugh held in: quick and quiet, the mouth barely open and half of it down the nose
    laugh(
        t,
        &Laugh { calls: 4.0, rate: 7.2, slowing: 0.04, pitch: 2.0, drift: -1.0, fall: 2.8, vowel: Ph::AX, voiced: 0.32, breath: 0.55, nasal: 0.45, level: 0.6, fade: 0.88, lax: 0.55, ..LAUGH },
    );
}

fn hoho(t: &mut Take) {
    laugh(
        t,
        &Laugh { calls: 3.0, rate: 3.7, slowing: 0.08, pitch: 1.6, drift: -1.2, fall: 4.5, vowel: Ph::AO, voiced: 0.56, breath: 0.63, level: 1.0, fade: 0.93, lax: 0.2, ..LAUGH },
    );
}

fn hoho_voice(v: &mut VoiceConfig) {
    v.chest_resonance = (v.chest_resonance + 0.2).min(1.0);
    v.warmth = (v.warmth + 0.08).min(1.0);
}

fn hehe(t: &mut Take) {
    laugh(
        t,
        &Laugh { calls: 4.0, rate: 5.5, pitch: 1.95, drift: -0.9, fall: 2.2, vowel: Ph::EH, voiced: 0.42, breath: 0.3, air: 0.13, level: 0.85, fade: 0.88, lax: 0.35, ..LAUGH },
    );
}

fn nervous(t: &mut Take) {
    laugh(
        t,
        &Laugh { calls: 4.0, rate: 7.1, slowing: 0.02, pitch: 2.15, drift: -0.4, fall: 2.1, vowel: Ph::AH, voiced: 0.36, breath: 0.59, level: 0.65, fade: 0.85, lax: 0.55, ragged: 2.6, ..LAUGH },
    );
    // and trails off in a breath out
    let mut out = t.airway(false);
    out.air = 0.0;
    t.now = out.silent();
    t.to(60.0, out.breath(0.22));
    t.to(180.0, out.breath(0.03));
}

fn evil(t: &mut Take) {
    // "mwa-ha-ha-ha": slow, low, and growing instead of dying away
    let level = t.level(1.0);
    let m = t.hum();
    let w = Pose::of(&tables::SPEC_W, Ph::W);
    let a = t.vowel(Ph::AA);
    let low = t.lifted(1.0);
    t.now = m.silent().at(low);
    t.to(40.0, m.voiced(1.1 * level).at(low));
    t.hold(80.0);
    t.to(55.0, w.voiced(0.8 * level).at(low * 1.02));
    t.to(80.0, a.voiced(level * 0.85).at(low * 1.1).lax(0.1));
    t.to(150.0 * t.length.sqrt(), a.voiced(level * 0.9).at(low * 1.02).lax(0.1));
    t.to(30.0, t.breathed(a, 0.25 * level).at(low * 1.02));
    laugh(
        t,
        &Laugh { calls: 5.0, rate: 3.6, slowing: -0.02, pitch: 1.05, drift: 1.4, fall: 2.2, vowel: Ph::AA, voiced: 0.6, breath: 0.39, level: 0.85, fade: 1.07, lax: 0.08, creak: 0.12, ..LAUGH },
    );
}

fn evil_voice(v: &mut VoiceConfig) {
    v.roughness = (v.roughness + 0.14).min(0.75);
    v.chest_resonance = (v.chest_resonance + 0.2).min(1.0);
    v.closure = (v.closure + 0.1).min(1.0);
}

// ------------------------------------------------------------------------------ breath

fn inhale(t: &mut Take) {
    t.breath(360.0 * t.length, 0.14 + 0.36 * t.intensity, true, 0.45);
}

fn exhale(t: &mut Take) {
    t.breath(540.0 * t.length, 0.16 + 0.4 * t.intensity, false, 0.22);
}

fn gasp(t: &mut Take) {
    // air snatched in through a mouth falling open, and held
    let mut a = t.airway(true);
    a.f = [640.0, 1750.0, 2700.0];
    let level = 0.25 + 0.4 * t.intensity;
    t.now = a.silent().jaw(0.3);
    t.to(40.0, a.breath(level).jaw(0.6));
    t.to(170.0 * t.length, a.breath(level * 0.9).jaw(0.75));
    t.to(15.0, a.silent().jaw(0.75));
    t.pause(70.0);
}

fn sigh(t: &mut Take) {
    let level = t.level(1.0);
    if t.length >= 0.8 {
        t.breath(300.0, 0.14 + 0.14 * t.intensity, true, 0.5);
        t.pause(70.0);
    }
    // the breath let go, with a little voice at its start that sinks and fades
    let v = t.vowel(Ph::AH);
    let h = t.breathed(v, 0.0);
    let ms = 660.0 * t.length;
    t.now = h.at(1.22);
    t.to(60.0, t.breathed(v, 0.45 * level).at(1.22));
    t.to(ms * 0.18, v.voiced(0.5 * level).breath(0.45 * level).at(1.16).lax(0.7));
    t.to(ms * 0.42, v.voiced(0.2 * level).breath(0.5 * level).at(0.92).lax(0.8).jaw(v.mouth[0] * 0.8));
    t.to(ms * 0.40, t.breathed(v, 0.1 * level).at(0.82).jaw(v.mouth[0] * 0.5));
}

fn yawn(t: &mut Take) {
    let level = t.level(1.0);
    let a = t.vowel(Ph::AA);
    // the long breath in as the jaw drops
    let mut wide = t.airway(true);
    wide.f = [680.0, 1250.0, 2550.0];
    t.now = t.airway(true).silent();
    t.to(650.0 * t.length, wide.breath(0.16 + 0.2 * t.intensity).jaw(1.0));
    t.to(90.0, wide.breath(0.08).jaw(1.0));
    // the voice swells up and sinks away through it
    t.to(140.0, a.voiced(0.45 * level).breath(0.25).at(0.95).lax(0.55).jaw(1.0));
    t.to(300.0 * t.length, a.voiced(0.75 * level).breath(0.25).at(1.36).lax(0.5).jaw(1.0));
    let ah = t.vowel(Ph::AH);
    t.to(520.0 * t.length, ah.voiced(0.4 * level).breath(0.3).at(0.78).lax(0.6).jaw(0.6));
    t.to(260.0, t.breathed(ah, 0.14).at(0.72).jaw(0.3));
}

fn sniff(t: &mut Take) {
    // air pulled sharply up the nose: a rush that swells and is cut off
    let times = if t.length > 1.4 { 2 } else { 1 };
    let mut p = t.hum().silent();
    p.hiss = Hiss { f1: 1700.0, b1: 1500.0, f2: 3400.0, b2: 2400.0, g1: 0.7, g2: 0.3, hp: 450.0, lp: 5200.0 };
    p.f = [420.0, 1500.0, 2500.0];
    p.b = [260.0, 300.0, 320.0];
    p.phone = Ph::SIL;
    p.closure = false;
    t.now = p;
    for _ in 0..times {
        let level = 0.1 + 0.16 * t.intensity;
        t.to(35.0, p.hissing(level * 0.5).breath(0.2));
        t.to(150.0, p.hissing(level).breath(0.42));
        t.to(18.0, p);
        t.pause(110.0);
    }
}

fn pant(t: &mut Take) {
    // out of breath: "hah" out, and a sharper breath in, about three times a second
    let times = (3.0 * t.length).round().clamp(2.0, 16.0) as usize;
    let level = 0.3 + 0.45 * t.intensity;
    let out = t.vowel(Ph::AH);
    for _ in 0..times {
        let (a, b) = (190.0 * t.about(0.07), 150.0 * t.about(0.07));
        let mut hah = t.breathed(out, level);
        hah.hiss = RUSH;
        hah.air = 0.5;
        t.now = hah.silent().jaw(0.6);
        t.to(a * 0.3, hah.jaw(0.7));
        t.to(a * 0.7, hah.breath(level * 0.15).jaw(0.6));
        t.pause(25.0);
        t.breath(b, level * 0.75, true, 0.4);
        t.pause(25.0);
    }
}

fn phew(t: &mut Take) {
    // lips nearly shut, the breath blown out through them and falling
    let level = t.level(1.0);
    let f = Pose::of(&tables::SPEC_F, Ph::F);
    let u = t.vowel(Ph::UW);
    t.now = f.silent();
    t.to(40.0, f.hissing(0.1 * level));
    t.to(90.0, f.hissing(0.09 * level));
    let y = Pose::of(&tables::SPEC_Y, Ph::Y);
    t.to(60.0, y.voiced(0.35 * level).breath(0.35 * level).at(1.55).lax(0.6));
    t.to(140.0 * t.length, u.voiced(0.45 * level).breath(0.4 * level).at(1.35).lax(0.65));
    t.to(300.0 * t.length, u.voiced(0.15 * level).breath(0.35 * level).at(0.85).lax(0.8));
    t.to(120.0, t.breathed(u, 0.1).at(0.8));
}

// ------------------------------------------------------------------------- body sounds

/// One cough: the glottis shut on the pressure, then blown open. Recorded coughs are over in
/// a fifth of a second: a blast across the whole band that has fallen to a quarter within
/// 40 ms, then air torn through the throat (most of a cough's energy is below 1 kHz) with
/// only a little voice in it - more voice than that and it is a grunt - and often the folds
/// catching once more at the end. `open` is how far the mouth opens, `long` stretches the
/// rush after the blast.
fn hack(t: &mut Take, loud: f64, open: f64, long: f64) {
    let v = t.vowel(Ph::AH);
    // (development hooks: see tools/sounds.py)
    let (k_ah, k_af, k_av) = (crate::tune("cough_ah", 2.0), crate::tune("cough_af", 0.6), crate::tune("cough_voice", 0.5));
    let mut blast = t.breathed(v, 0.0);
    blast.hiss = Hiss { f1: 900.0, b1: 1600.0, f2: 3800.0, b2: 4200.0, g1: 0.9, g2: 0.55, hp: 0.0, lp: 0.0 };
    blast.air = 0.0;
    blast.closure = false;
    blast.phone = Ph::HH;
    blast.b[0] -= 120.0;
    let top = 1.35 * t.about(0.04);
    t.now = blast.silent().jaw(0.75 * open).at(top);
    t.to(12.0, blast.hissing(0.5 * loud * k_af).breath(1.0 * loud * k_ah).voiced(0.9 * loud * k_av).at(top).creaky(0.3).lax(0.15).jaw(0.8 * open));
    t.hold(14.0);
    t.to(38.0, blast.hissing(0.07 * loud * k_af).breath(0.45 * loud * k_ah).voiced(0.5 * loud * k_av).at(top * st(-2.0)).creaky(0.4).lax(0.3).jaw(0.7 * open));
    t.to(85.0 * long, blast.hissing(0.01 * loud * k_af).breath(0.2 * loud * k_ah).voiced(0.16 * loud * k_av).at(top * st(-5.0)).creaky(0.5).lax(0.45).jaw(0.5 * open));
    // the folds meeting again
    t.to(20.0, blast.breath(0.1 * loud * k_ah).voiced(0.32 * loud * k_av).at(top * st(-6.0)).creaky(0.6).lax(0.3).jaw(0.4 * open));
    t.to(35.0, blast.silent().at(top * st(-7.0)).jaw(0.3 * open));
}

fn cough(t: &mut Take) {
    let times = if t.length > 1.3 { 3 } else if t.length < 0.7 { 1 } else { 2 };
    let level = t.level(1.0);
    t.breath(150.0, 0.3, true, 0.5);
    for k in 0..times {
        t.pause(if k == 0 { 70.0 } else { 150.0 });
        hack(t, level * 0.75f64.powi(k), 1.0, 1.0);
    }
}

fn cough_voice(v: &mut VoiceConfig) {
    v.roughness = (v.roughness + 0.2).min(0.75);
}

fn ahem(t: &mut Take) {
    // A throat being cleared is a small cough or two kept behind nearly closed lips: short,
    // then longer. (A clear hum for the "hem" is heard as a grunt; what is left of it here
    // is the mouth closing on the end of the second one.)
    let level = t.level(1.0);
    // (how far the mouth opens: it only shows, the sound is made in the throat)
    let open = 0.45;
    t.pause(30.0);
    hack(t, level * 0.8, open, 0.8 * t.length);
    t.pause(60.0);
    hack(t, level, open, 1.7 * t.length);
    let m = t.hum();
    t.now = m.silent().at(0.8);
    t.to(30.0, m.voiced(0.35 * level).breath(0.15).at(0.8).creaky(0.3));
    t.to(110.0 * t.length, m.voiced(0.2 * level).breath(0.1).at(0.72).creaky(0.3));
    t.to(50.0, m.silent().at(0.7));
}

fn rough_voice(v: &mut VoiceConfig) {
    v.roughness = (v.roughness + 0.25).min(0.75);
    v.breathiness = (v.breathiness - 0.05).max(0.0);
}

fn hiccup(t: &mut Take) {
    // the breath jerked in and cut off by the throat snapping shut: a clipped, rising squeak
    let level = t.level(1.0);
    let v = t.vowel(Ph::IH).jaw(0.15);
    let top = t.lifted(3.4);
    t.pause(40.0);
    let wide = t.airway(true);
    t.now = wide.silent();
    t.to(30.0, wide.breath(0.3));
    t.to(10.0, v.voiced(0.8 * level).at(top * 0.6).lax(0.1));
    t.to(45.0, v.voiced(0.9 * level).at(top).lax(0.05));
    t.to(5.0, v.silent().at(top));
    t.pause(60.0);
    t.breath(110.0, 0.14, false, 0.3);
}

fn sneeze(t: &mut Take) {
    let level = t.level(1.0);
    let a = t.vowel(Ph::AA);
    // "ah ...": the breath snatched in and up, far quieter than what follows
    let mut wide = t.airway(true);
    wide.f = [650.0, 1500.0, 2650.0];
    t.now = wide.silent();
    t.to(70.0, wide.breath(0.12).jaw(0.7));
    t.to(150.0 * t.length, a.voiced(0.1 * level).breath(0.1).at(1.35).lax(0.7).jaw(0.9));
    t.to(60.0, a.voiced(0.06 * level).breath(0.06).at(1.7).lax(0.7).jaw(1.0));
    t.to(30.0, a.silent().at(1.75).jaw(1.0));
    t.pause(110.0);
    // "CHOO": the whole breath thrown out against the teeth with the voice in it. Recorded
    // sneezes last about 0.4 s, are at their loudest a quarter of the way in, and are
    // noise and voice together across the whole band.
    let Some(ch) = tables::stop(Ph::CH) else { return };
    let u = t.vowel(Ph::UH);
    let mut blast = u;
    blast.phone = Ph::CH;
    blast.hiss = Hiss { f1: ch.b_f1, b1: ch.b_b1 * 2.5, f2: 6200.0, b2: 4500.0, g1: 0.7, g2: 0.65, hp: 500.0, lp: 0.0 };
    let top = 1.6;
    t.now = blast.silent().at(top);
    t.to(40.0, blast.hissing(0.45 * level).breath(0.5).voiced(0.35 * level).at(top).lax(0.3));
    t.to(60.0, blast.hissing(0.6 * level).breath(0.7).voiced(level).at(top * st(-1.0)).lax(0.2));
    t.to(85.0, blast.hissing(0.3 * level).breath(0.5).voiced(0.6 * level).at(top * st(-3.5)).lax(0.35));
    t.to(190.0 * t.length, blast.hissing(0.1 * level).breath(0.3).voiced(0.3 * level).at(top * st(-8.0)).lax(0.55));
    t.to(70.0, t.breathed(u, 0.06).at(top * st(-9.0)));
}

fn snore(t: &mut Take) {
    let times = (1.0 * t.length).round().clamp(1.0, 8.0) as usize;
    // (slack folds give little sound for their effort: it is driven hard to be heard)
    let level = t.level(2.6);
    for _ in 0..times {
        // The soft palate flapping in the breath drawn in. In recordings it is not a train
        // of clicks but a slack, rounded rumble at 70 to 110 Hz, nearly all of its energy
        // below 500 Hz, with the noise of the breath riding on it right up the band; it
        // swells over a third of a second and lasts a second or more.
        let mut p = t.vowel(Ph::ER);
        // (the resonance sits on the rumble itself, not where a vowel's would)
        p.f = [180.0, 750.0, 2300.0];
        p.b = [140.0, 400.0, 380.0];
        p.nasal = 0.3;
        p.phone = Ph::SIL;
        p.mouth = [0.35, 0.4, 0.4, 0.15, 0.05, 0.0, 0.0, 0.8];
        p.hiss = RUSH;
        p.air = 0.12;
        // the palate beating: the rumble wavers with each beat
        let rate = 27.0 * t.about(0.06);
        let rattle = move |ms: f64, q: &mut Pose| {
            q.av *= if (ms / 1000.0 * rate).fract() < 0.55 { 1.0 } else { 0.6 };
        };
        let low = (92.0 / t.cfg.pitch_hz).min(0.85);
        let ms = 1450.0 * t.about(0.08);
        t.now = p.silent().at(low);
        t.shape_to(320.0, p.voiced(level).breath(0.3 * level).at(low).creaky(0.0).lax(0.9), rattle);
        t.shape_to(ms - 420.0, p.voiced(0.95 * level).breath(0.3 * level).at(low * 1.08).creaky(0.0).lax(0.9), rattle);
        t.shape_to(100.0, p.voiced(0.0).breath(0.02 * level).at(low).creaky(0.0).lax(0.9), rattle);
        // ... and the breath out, barely heard
        t.pause(200.0);
        t.breath(420.0, 0.1 * level, false, 0.35);
        t.pause(120.0);
    }
}

fn snore_voice(v: &mut VoiceConfig) {
    // slack folds: a round pulse with little in it above the first few harmonics
    v.closure = (v.closure - 0.3).max(0.0);
    v.warmth = (v.warmth + 0.3).min(1.0);
    v.roughness = (v.roughness + 0.2).min(0.75);
}

// ---------------------------------------------------------------------------- reactions

/// A hummed or spoken syllable: a pose held with the pitch running through `pitches`
/// (multiples of the speaking pitch, evenly spaced over its length).
fn tune_on(t: &mut Take, pose: Pose, ms: f64, level: f64, pitches: &[f64]) {
    let attack = 30.0f64.min(ms * 0.2);
    let mut start = pose.silent().at(pitches[0]);
    start.mouth = t.now.mouth;
    if t.now.av < 0.05 {
        t.now = start;
    }
    t.to(attack, pose.voiced(level).at(pitches[0]));
    let steps = pitches.len().max(2) - 1;
    for (i, &p) in pitches.iter().enumerate().skip(1) {
        // a little weaker toward the end
        let fade = 1.0 - 0.25 * i as f64 / steps as f64;
        t.to((ms - attack) / steps as f64, pose.voiced(level * fade).at(p));
    }
}

fn release(t: &mut Take, ms: f64) {
    let p = t.now.silent();
    t.to(ms, p);
}

fn hmm(t: &mut Take) {
    // thinking it over: level, dipping, and up a little at the end
    let level = t.level(1.45);
    let m = t.hum();
    tune_on(t, m, 560.0 * t.length, level, &[1.12, 1.05, 1.02, 1.08, 1.22]);
    release(t, 70.0);
}

fn mm(t: &mut Take) {
    // pleased: a slow rise and fall
    let level = t.level(1.5);
    let m = t.hum();
    tune_on(t, m, 540.0 * t.length, level, &[1.15, 1.42, 1.34, 1.05, 0.92]);
    release(t, 80.0);
}

fn mhm(t: &mut Take) {
    // yes: low, a breath through the nose, high
    let level = t.level(1.45);
    let m = t.hum();
    tune_on(t, m, 150.0, level, &[1.0, 0.98]);
    let mut h = m.silent().breath(0.2);
    h.b[0] += 120.0;
    t.to(35.0, h.voiced(0.15 * level).at(1.05));
    t.to(40.0, h.voiced(0.12 * level).at(1.25));
    tune_on(t, m, 240.0 * t.length, level, &[1.34, 1.4, 1.36]);
    release(t, 70.0);
}

fn uh_huh(t: &mut Take) {
    let level = t.level(0.95);
    let v = t.vowel(Ph::AH).nasalized(0.15);
    tune_on(t, v, 140.0, level, &[0.96, 0.93]);
    t.to(35.0, t.breathed(v, 0.14).at(1.0));
    t.to(45.0, t.breathed(v, 0.12).at(1.2));
    tune_on(t, v, 240.0 * t.length, level, &[1.28, 1.36, 1.32]);
    release(t, 70.0);
}

fn uh_uh(t: &mut Take) {
    // no: high, the throat shut for a moment, low
    let level = t.level(0.95);
    let v = t.vowel(Ph::AH).nasalized(0.12);
    tune_on(t, v, 130.0, level, &[1.28, 1.25]);
    t.to(12.0, v.voiced(0.2 * level).at(1.2).creaky(0.6));
    t.pause(70.0);
    t.now = v.silent().at(0.95).creaky(0.4);
    tune_on(t, v, 230.0 * t.length, level, &[0.95, 0.9, 0.82]);
    release(t, 70.0);
}

fn uh_oh(t: &mut Take) {
    let level = t.level(1.0);
    let v = t.vowel(Ph::AH);
    tune_on(t, v, 150.0, level, &[1.32, 1.3]);
    t.to(12.0, v.voiced(0.2 * level).at(1.25).creaky(0.6));
    t.pause(80.0);
    let (o1, o2) = t.glide(Ph::OW);
    t.now = o1.silent().at(0.98);
    t.to(25.0, o1.voiced(level).at(0.98));
    t.to(130.0, o1.voiced(level).at(0.94));
    t.to(190.0 * t.length, o2.voiced(level * 0.7).at(0.84));
    release(t, 80.0);
}

fn uh(t: &mut Take) {
    // searching for the word: level and low
    let level = t.level(0.85);
    let v = t.vowel(Ph::AH);
    tune_on(t, v, 330.0 * t.length, level, &[0.95, 0.93, 0.91]);
    release(t, 80.0);
}

fn um(t: &mut Take) {
    let level = t.level(0.85);
    let v = t.vowel(Ph::AH);
    tune_on(t, v, 210.0 * t.length, level, &[0.95, 0.93]);
    let m = t.hum();
    t.to(45.0, m.voiced(level * 1.4).at(0.92));
    t.to(220.0 * t.length, m.voiced(level * 1.0).at(0.9));
    release(t, 80.0);
}

fn huh(t: &mut Take) {
    // what? - up it goes
    let level = t.level(1.0);
    let v = t.vowel(Ph::AH).nasalized(0.2);
    t.now = t.breathed(v, 0.0).at(1.0);
    t.to(55.0, t.breathed(v, 0.16).at(1.0));
    tune_on(t, v, 230.0 * t.length, level, &[1.0, 1.15, 1.45, 1.7]);
    release(t, 60.0);
}

fn aha(t: &mut Take) {
    // got it: a short low "a", a breath, and a high one falling
    let level = t.level(0.85);
    let v = t.vowel(Ph::AA);
    tune_on(t, v, 120.0, level * 0.85, &[1.0, 0.98]);
    t.to(30.0, t.breathed(v, 0.35).at(1.1));
    t.to(40.0, t.breathed(v, 0.3).at(1.45));
    tune_on(t, v, 330.0 * t.length, level, &[1.55, 1.6, 1.3, 1.0]);
    release(t, 80.0);
}

fn oh(t: &mut Take) {
    let level = t.level(1.0);
    let (o1, o2) = t.glide(Ph::OW);
    t.now = o1.silent().at(1.3);
    t.to(30.0, o1.voiced(level).at(1.38));
    t.to(150.0 * t.length, o1.voiced(level).at(1.2));
    t.to(220.0 * t.length, o2.voiced(level * 0.7).at(0.86));
    release(t, 90.0);
}

fn ooh(t: &mut Take) {
    let level = t.level(1.0);
    let u = t.vowel(Ph::UW);
    tune_on(t, u, 540.0 * t.length, level, &[1.1, 1.35, 1.52, 1.4, 1.05]);
    release(t, 90.0);
}

fn aww(t: &mut Take) {
    // tender: high and sinking slowly, soft
    let level = t.level(0.9);
    let o = t.vowel(Ph::AO).lax(0.4);
    tune_on(t, o, 680.0 * t.length, level, &[1.5, 1.58, 1.45, 1.22, 1.02]);
    release(t, 110.0);
}

fn ahh(t: &mut Take) {
    // relief: the voice let down with the breath
    let level = t.level(0.9);
    let a = t.vowel(Ph::AA).lax(0.45);
    t.now = t.breathed(a, 0.0).at(1.25);
    t.to(50.0, t.breathed(a, 0.14).at(1.25));
    tune_on(t, a.breath(0.1), 600.0 * t.length, level, &[1.25, 1.2, 1.05, 0.9, 0.8]);
    t.to(150.0, t.breathed(a, 0.06).at(0.78));
}

fn ugh(t: &mut Take) {
    // disgust: low and rasping, far back in the mouth, cut short
    let level = t.level(1.0);
    let mut v = t.vowel(Ph::AH);
    v.f[1] *= 0.9;
    v.hiss = Hiss { f1: 1500.0, b1: 600.0, f2: 3800.0, b2: 2600.0, g1: 0.9, g2: 0.05, hp: 700.0, lp: 2600.0 };
    t.now = v.silent().at(0.9);
    t.to(25.0, v.voiced(level).at(0.9).creaky(0.35).lax(0.05));
    t.to(260.0 * t.length, v.voiced(level * 0.85).hissing(0.035).breath(0.06).at(0.72).creaky(0.5).lax(0.05));
    t.to(40.0, v.silent().hissing(0.05).at(0.7));
    t.to(60.0, v.silent());
}

fn eww(t: &mut Take) {
    let level = t.level(1.0);
    let i = t.vowel(Ph::IY).nasalized(0.25);
    let u = t.vowel(Ph::UW).nasalized(0.2);
    t.now = i.silent().at(1.3);
    t.to(30.0, i.voiced(level).at(1.32));
    t.to(200.0 * t.length, i.voiced(level).at(1.62));
    t.to(330.0 * t.length, u.voiced(level * 0.75).at(1.08));
    release(t, 100.0);
}

// ------------------------------------------------------------------------------- voice

fn scream(t: &mut Take) {
    let level = t.level(0.62);
    let a = t.vowel(Ph::AE).lax(0.0);
    // as high as the voice goes
    let top = (480.0 / t.cfg.pitch_hz).min(4.2);
    t.now = a.silent().at(top * 0.75).jaw(1.0);
    t.to(35.0, a.voiced(level).breath(0.2).at(top * 0.9).jaw(1.0));
    let ms = 850.0 * t.length;
    let shake = |ms: f64, q: &mut Pose| q.pitch *= 1.0 + 0.02 * (TAU * 6.5 * ms / 1000.0).sin();
    t.shape_to(ms * 0.25, a.voiced(level).breath(0.25).at(top).jaw(1.0), shake);
    t.shape_to(ms * 0.55, a.voiced(level * 0.95).breath(0.25).at(top * 0.97).jaw(1.0), shake);
    t.shape_to(ms * 0.20, a.voiced(level * 0.6).breath(0.2).at(top * 0.82).jaw(0.8), shake);
    t.to(80.0, t.breathed(a, 0.2).at(top * 0.75));
}

fn scream_voice(v: &mut VoiceConfig) {
    v.closure = 1.0;
    v.breathiness = (v.breathiness * 0.4).min(0.1);
    v.roughness = (v.roughness + 0.12).min(0.75);
    v.warmth = (v.warmth - 0.2).max(0.0);
}

fn groan(t: &mut Take) {
    let level = t.level(1.0);
    let v = t.vowel(Ph::ER).lax(0.2);
    let ms = 820.0 * t.length;
    t.now = v.silent().at(0.86).jaw(0.25);
    t.to(60.0, v.voiced(level * 0.8).at(0.86).jaw(0.3));
    t.to(ms * 0.45, v.voiced(level).at(0.78).creaky(0.2).jaw(0.35));
    t.to(ms * 0.55, v.voiced(level * 0.6).at(0.62).creaky(0.65).jaw(0.25));
    release(t, 120.0);
}

fn sob(t: &mut Take) {
    let times = (3.0 * t.length).round().clamp(1.0, 12.0) as usize;
    let level = t.level(1.0);
    let v = t.vowel(Ph::EH).lax(0.45).nasalized(0.2);
    for k in 0..times {
        // the breath caught on the way in ...
        let mut wide = t.airway(true);
        wide.f = [600.0, 1800.0, 2700.0];
        let caught = 90.0 * t.about(0.1);
        t.now = wide.silent();
        t.to(40.0, wide.breath(0.3));
        t.to(caught, wide.breath(0.25));
        t.to(15.0, wide.silent());
        t.pause(30.0);
        // ... and the wail on the way out, high, shaking, and sinking
        let loud = level * 0.92f64.powi(k as i32);
        let top = t.lifted(2.5) * st(-(k as f64)) * t.about(0.03);
        let rate = 6.5 * t.about(0.08);
        let shake = move |ms: f64, q: &mut Pose| {
            let w = (TAU * rate * ms / 1000.0).sin();
            q.pitch *= 1.0 + 0.05 * w;
            q.av *= 1.0 + 0.35 * w;
        };
        let wail = 460.0 * t.about(0.1);
        t.now = v.silent().at(top);
        t.to(30.0, v.voiced(loud * 0.9).breath(0.2).at(top));
        t.shape_to(wail, v.voiced(loud * 0.75).breath(0.25).at(top * st(-5.0)), shake);
        t.to(70.0, t.breathed(v, 0.2).at(top * st(-6.0)));
        t.pause(60.0);
    }
    sniff(t);
}

fn whimper(t: &mut Take) {
    // small cries that barely open the mouth, each one falling
    let times = (3.0 * t.length).round().clamp(1.0, 12.0) as usize;
    let level = t.level(1.2);
    let v = t.vowel(Ph::IH).nasalized(0.5).lax(0.5).jaw(0.12);
    for k in 0..times {
        let top = t.lifted(2.9) * st(-0.7 * k as f64) * t.about(0.03);
        let cry = 170.0 * t.about(0.1);
        t.now = v.silent().at(top * st(-1.0));
        t.to(30.0, v.voiced(level).breath(0.12).at(top));
        t.to(cry, v.voiced(level * 0.55).breath(0.15).at(top * st(-4.0)));
        t.to(45.0, v.silent().at(top * st(-4.5)));
        let wait = 100.0 * t.about(0.15);
        t.pause(wait);
    }
}

fn hum(t: &mut Take) {
    // a few notes of nothing in particular, where a voice hums: above where it speaks
    let level = t.level(1.4);
    let m = t.hum();
    let key = t.lifted(1.45);
    // do, mi, sol, mi, do: the fifth held
    let notes: [(f64, f64); 5] = [(0.0, 230.0), (4.0, 230.0), (7.0, 420.0), (4.0, 230.0), (0.0, 480.0)];
    let count = ((notes.len() as f64 * t.length).round() as usize).clamp(2, notes.len());
    t.now = m.silent().at(key);
    for &(semis, ms) in &notes[..count] {
        t.to(28.0, m.voiced(level).at(key * st(semis)));
        t.to(ms - 60.0, m.voiced(level * 0.9).at(key * st(semis)));
        t.to(32.0, m.voiced(level * 0.45).at(key * st(semis)));
    }
    release(t, 90.0);
}

// ------------------------------------------------------------------------------- mouth

fn shh(t: &mut Take) {
    let sh = Pose::of(&tables::SPEC_SH, Ph::SH);
    let level = 0.22 + 0.3 * t.intensity;
    t.now = sh.silent();
    t.to(70.0, sh.hissing(level));
    t.to(300.0 * t.length, sh.hissing(level * 0.85));
    t.to(420.0 * t.length, sh.hissing(level * 0.25));
    t.to(120.0, sh);
}

fn psst(t: &mut Take) {
    let level = 0.6 + 0.6 * t.intensity;
    let s = Pose::of(&tables::SPEC_S, Ph::S);
    t.now = Pose::of(&tables::SPEC_M, Ph::P).silent();
    t.pause(40.0);
    t.pop(Ph::P, 0.1 * level, 10.0);
    t.now = s.silent();
    t.to(35.0, s.hissing(0.28 * level));
    t.to(200.0 * t.length, s.hissing(0.26 * level));
    t.to(30.0, s);
    t.pause(45.0);
    t.pop(Ph::T, 0.16 * level, 12.0);
    t.breath(45.0, 0.05, false, 0.2);
}

fn tsk(t: &mut Take) {
    // the tongue pulled off the teeth ridge: a click with nothing after it
    let times = if t.length < 0.7 { 1 } else if t.length > 1.4 { 3 } else { 2 };
    let level = 0.25 + 0.35 * t.intensity;
    let n = Pose::of(&tables::SPEC_N, Ph::T).silent();
    for _ in 0..times {
        t.now = n;
        t.pause(55.0);
        t.pop(Ph::T, level, 8.0);
        let wait = 150.0 * t.about(0.06);
        t.pause(wait);
    }
}

fn kiss(t: &mut Take) {
    // "mwah": lips pressed, popped apart, and an open vowel after
    let level = t.level(1.0);
    let m = t.hum();
    tune_on(t, m, 130.0, level * 0.8, &[1.15, 1.2]);
    t.to(15.0, m.silent().at(1.2));
    t.pause(30.0);
    t.pop(Ph::P, 0.22, 8.0);
    let w = Pose::of(&tables::SPEC_W, Ph::W);
    let a = t.vowel(Ph::AA);
    t.now = w.silent().at(1.3);
    t.to(25.0, w.voiced(level * 0.8).at(1.3));
    t.to(70.0, a.voiced(level).at(1.3));
    t.to(260.0 * t.length, a.voiced(level * 0.5).breath(0.08).at(0.88).lax(0.5));
    release(t, 90.0);
}

/// Lips or tongue flapping in the airstream: `open` of each beat they are apart.
fn trill(rate: f64, open: f64) -> impl Fn(f64, &mut Pose) {
    move |ms, q| {
        let apart = (ms / 1000.0 * rate).fract() < open;
        if apart {
            q.mouth[6] = 0.0;
        } else {
            // shut: only the hum through the cheeks is left
            q.avb = q.avb.max(q.av * 0.55);
            q.av *= 0.1;
            q.af *= 0.1;
            q.ah *= 0.2;
            q.mouth[6] = 1.0;
        }
    }
}

fn brr(t: &mut Take) {
    // shivering: the lips blown into a flutter with the voice on
    let level = t.level(1.0);
    let mut lips = t.vowel(Ph::UH);
    lips.f = [330.0, 820.0, 2250.0];
    lips.phone = Ph::B;
    let rate = 25.0 * t.about(0.05);
    let ms = 520.0 * t.length;
    t.now = lips.silent().at(1.12);
    t.shape_to(60.0, lips.voiced(level).at(1.12), trill(rate, 0.45));
    t.shape_to(ms, lips.voiced(level * 0.85).at(1.02), trill(rate, 0.45));
    let r = t.vowel(Ph::ER);
    t.to(70.0, r.voiced(level * 0.7).at(0.98));
    t.to(120.0, r.voiced(level * 0.3).at(0.9));
    release(t, 70.0);
}

fn grr(t: &mut Take) {
    let level = t.level(1.5);
    let mut r = t.vowel(Ph::ER);
    r.mouth[0] = 0.2;
    let rate = 29.0 * t.about(0.05);
    // the back of the tongue rattling: the voice dips with each beat instead of stopping
    let rattle = move |ms: f64, q: &mut Pose| {
        q.av *= if (ms / 1000.0 * rate).fract() < 0.55 { 1.0 } else { 0.5 };
    };
    let ms = 700.0 * t.length;
    let low = (82.0 / t.cfg.pitch_hz).min(0.8);
    t.now = r.silent().at(low);
    t.shape_to(70.0, r.voiced(level * 0.85).at(low).creaky(0.3).lax(0.0), rattle);
    t.shape_to(ms * 0.6, r.voiced(level).at(low * 0.95).creaky(0.35).lax(0.0), rattle);
    t.shape_to(ms * 0.4, r.voiced(level * 0.7).at(low * 0.85).creaky(0.45).lax(0.0), rattle);
    release(t, 90.0);
}

fn raspberry(t: &mut Take) {
    // the tongue between the lips, blown: the lips themselves buzz, slow and slack, with
    // air spluttering round them
    let level = 0.5 + 0.5 * t.intensity;
    let mut lips = t.vowel(Ph::UH);
    lips.phone = Ph::F;
    lips.f = [360.0, 850.0, 2250.0];
    lips.b = [160.0, 200.0, 280.0];
    lips.hiss = Hiss { f1: 700.0, b1: 1000.0, f2: 2200.0, b2: 2400.0, g1: 0.9, g2: 0.25, hp: 0.0, lp: 3000.0 };
    // as slow as a buzz goes: the voice source stands in for the flapping lips
    let low = 58.0 / t.cfg.pitch_hz;
    let rate = 27.0 * t.about(0.06);
    let flap = move |ms: f64, q: &mut Pose| {
        let beat = 0.5 - 0.5 * (TAU * ms / 1000.0 * rate).cos();
        q.af *= 0.25 + 0.75 * beat;
        q.mouth[6] = 1.0 - 0.8 * beat;
    };
    t.now = lips.silent().at(low);
    t.shape_to(50.0, lips.voiced(0.9 * level).hissing(0.16 * level).breath(0.5 * level).at(low).creaky(1.0).lax(0.0), flap);
    t.shape_to(450.0 * t.length, lips.voiced(0.8 * level).hissing(0.14 * level).breath(0.45 * level).at(low * 0.95).creaky(1.0).lax(0.0), flap);
    t.shape_to(120.0, lips.voiced(0.2 * level).hissing(0.04 * level).breath(0.1 * level).at(low * 0.9).creaky(1.0), flap);
    t.to(40.0, lips.silent().at(low * 0.9));
}

/// The mouth as it whistles: lips drawn to a small round hole, a tone and a little breath.
fn whistling(t: &Take, hz: f64, level: f64) -> Pose {
    let mut p = t.vowel(Ph::UW);
    p.quiet();
    p.phone = Ph::W;
    p.tone_hz = hz;
    p.tone = level;
    // the breath that goes with it, around the note
    p.hiss = Hiss { f1: hz / t.scale[3].max(0.5), b1: 500.0, f2: hz * 2.0, b2: 1500.0, g1: 0.8, g2: 0.1, hp: 500.0, lp: 0.0 };
    p.af = 0.01 + 0.05 * level;
    p.mouth[4] = 1.0;
    p
}

fn whistle(t: &mut Take) {
    // the two-note whistle: a quick slide up; then up, over and a long way down
    let level = 0.12 + 0.2 * t.intensity;
    let w = |t: &Take, hz: f64, a: f64| whistling(t, hz, a * level);
    t.now = w(t, 900.0, 0.0);
    t.to(40.0, w(t, 1000.0, 0.8));
    t.to(190.0 * t.length, w(t, 2300.0, 1.0));
    t.to(35.0, w(t, 2400.0, 0.0));
    t.pause(90.0);
    t.now = w(t, 1100.0, 0.0);
    t.to(40.0, w(t, 1200.0, 0.85));
    t.to(170.0 * t.length, w(t, 2350.0, 1.0));
    t.to(380.0 * t.length, w(t, 850.0, 0.7));
    t.to(70.0, w(t, 780.0, 0.0));
}

fn whistle_up(t: &mut Take) {
    // "fweet": one rising note, to catch someone's attention
    let level = 0.12 + 0.2 * t.intensity;
    t.now = whistling(t, 1300.0, 0.0);
    t.to(45.0, whistling(t, 1500.0, 0.85 * level));
    t.to(260.0 * t.length, whistling(t, 2700.0, level));
    t.to(60.0, whistling(t, 2900.0, level));
    t.to(40.0, whistling(t, 2900.0, 0.0));
}

// ----------------------------------------------------------------------------- the list

macro_rules! sound {
    ($id:literal, $label:literal, $group:literal, $about:literal, [$($alias:literal),*], $perform:expr) => {
        VocalSound { id: $id, label: $label, group: $group, about: $about, aliases: &[$($alias),*], voice: None, perform: $perform }
    };
    ($id:literal, $label:literal, $group:literal, $about:literal, [$($alias:literal),*], $perform:expr, $voice:expr) => {
        VocalSound { id: $id, label: $label, group: $group, about: $about, aliases: &[$($alias),*], voice: Some($voice), perform: $perform }
    };
}

/// Every sound the voice can make besides words, grouped as in [`GROUPS`].
pub static VOCAL_SOUNDS: &[VocalSound] = &[
    // laughter
    sound!("laugh", "Laugh", "laughter", "An open, easy laugh: ha-ha-ha-ha.", ["haha", "hahaha", "laughs", "lol"], laugh_plain, laughing_voice),
    sound!("chuckle", "Chuckle", "laughter", "A short, low laugh, half to oneself.", ["chuckles", "chortle", "heh"], chuckle, laughing_voice),
    sound!("giggle", "Giggle", "laughter", "Quick, high and light: hee-hee-hee.", ["giggles", "teehee", "titter", "hihi"], giggle, laughing_voice),
    sound!("guffaw", "Belly laugh", "laughter", "Loud and helpless, with a gasp for air in the middle.", ["bellylaugh", "roar", "guffaws", "rofl"], guffaw, laughing_voice),
    sound!("cackle", "Cackle", "laughter", "A sharp, rattling laugh after a long first cry.", ["cackles", "witch"], cackle, cackle_voice),
    sound!("snicker", "Snicker", "laughter", "A laugh held back, let out through the nose.", ["snickers", "snigger", "snort"], snicker, laughing_voice),
    sound!("hoho", "Ho ho ho", "laughter", "Slow, deep and round.", ["hohoho", "santa"], hoho, hoho_voice),
    sound!("hehe", "Heh heh", "laughter", "A dry little laugh.", ["hehehe", "hehheh"], hehe, laughing_voice),
    sound!("nervous", "Nervous laugh", "laughter", "Breathy, uneven, and trailing off.", ["nervouslaugh", "awkward"], nervous, laughing_voice),
    sound!("evil", "Evil laugh", "laughter", "Mwa-ha-ha: low, slow, and growing.", ["evillaugh", "mwahaha", "muahaha", "villain"], evil, evil_voice),
    // breath
    sound!("inhale", "Breath in", "breath", "A breath drawn in.", ["breathin", "breathein"], inhale),
    sound!("exhale", "Breath out", "breath", "A breath let out.", ["breathout", "breatheout", "blow"], exhale),
    sound!("gasp", "Gasp", "breath", "Air snatched in, in surprise.", ["gasps"], gasp),
    sound!("sigh", "Sigh", "breath", "A breath in, and out with the voice sinking through it.", ["sighs"], sigh),
    sound!("yawn", "Yawn", "breath", "The long breath in, and the voice swelling and fading on the way out.", ["yawns"], yawn),
    sound!("sniff", "Sniff", "breath", "Air drawn sharply through the nose.", ["sniffs", "sniffle"], sniff),
    sound!("pant", "Pant", "breath", "Out of breath: quick breaths out and in.", ["pants", "panting"], pant),
    sound!("phew", "Phew", "breath", "Relief, blown out through nearly closed lips.", ["whew"], phew),
    // reactions
    sound!("hmm", "Hmm", "reaction", "Thinking it over.", ["hm", "hmmm"], hmm),
    sound!("mm", "Mmm", "reaction", "Pleased.", ["mmm", "yum"], mm),
    sound!("mhm", "Mm-hm", "reaction", "Yes, with the mouth shut.", ["mmhm", "mmhmm"], mhm),
    sound!("uh-huh", "Uh-huh", "reaction", "Yes.", ["yep"], uh_huh),
    sound!("uh-uh", "Uh-uh", "reaction", "No.", ["nuhuh", "nope"], uh_uh),
    sound!("uh-oh", "Uh-oh", "reaction", "Something has gone wrong.", ["oops"], uh_oh),
    sound!("uh", "Uh", "reaction", "Searching for the word.", ["er", "erm"], uh),
    sound!("um", "Um", "reaction", "Searching for the word, lips closing on it.", ["umm"], um),
    sound!("huh", "Huh?", "reaction", "What? - rising.", ["eh"], huh),
    sound!("aha", "Aha", "reaction", "Got it.", ["ahha"], aha),
    sound!("oh", "Oh", "reaction", "Taking something in: a fall.", ["ohh"], oh),
    sound!("ooh", "Ooh", "reaction", "Impressed: up and over.", ["oooh"], ooh),
    sound!("aww", "Aww", "reaction", "Tender, high and sinking.", ["aw", "awww"], aww),
    sound!("ahh", "Ahh", "reaction", "Relief or contentment.", ["ah", "aah", "aaah"], ahh),
    sound!("ugh", "Ugh", "reaction", "Disgust or weariness, low and rasping.", ["ughh", "bleh"], ugh, rough_voice),
    sound!("eww", "Eww", "reaction", "Revulsion, through the nose.", ["ew", "ewww", "yuck"], eww),
    // voice
    sound!("hum", "Hum", "voice", "A few idle notes behind closed lips.", ["hums", "humming"], hum),
    sound!("groan", "Groan", "voice", "Long, low, and creaking toward its end.", ["groans", "moan"], groan),
    sound!("scream", "Scream", "voice", "As high and as loud as the voice goes.", ["screams", "shriek", "yell"], scream, scream_voice),
    sound!("sob", "Sob", "voice", "Crying: the breath caught, and a shaking wail.", ["sobs", "cry", "crying", "weep"], sob),
    sound!("whimper", "Whimper", "voice", "Small, high, falling cries through the nose.", ["whimpers", "whine"], whimper),
    // mouth
    sound!("shh", "Shh", "mouth", "Hush.", ["shhh", "shush", "hush"], shh),
    sound!("psst", "Psst", "mouth", "A hiss to catch someone's attention quietly.", ["pst"], psst),
    sound!("tsk", "Tsk tsk", "mouth", "The click of disapproval.", ["tsktsk", "tut", "tuttut", "click"], tsk),
    sound!("kiss", "Mwah", "mouth", "A kiss blown.", ["mwah", "smooch"], kiss),
    sound!("brr", "Brr", "mouth", "Shivering: the lips in a flutter.", ["brrr", "shiver"], brr),
    sound!("grr", "Grr", "mouth", "A growl in the throat.", ["grrr", "growl"], grr, rough_voice),
    sound!("raspberry", "Raspberry", "mouth", "The tongue between the lips, blown.", ["pbbt", "pfft"], raspberry),
    sound!("whistle", "Whistle", "mouth", "The two-note whistle: up, and up-and-down.", ["wolfwhistle", "whistles"], whistle),
    sound!("fweet", "Fweet", "mouth", "One sharp rising whistle.", ["whistleup", "tweet"], whistle_up),
    // body
    sound!("cough", "Cough", "body", "A cough or two.", ["coughs", "coughing"], cough, cough_voice),
    sound!("ahem", "Ahem", "body", "Clearing the throat.", ["throat", "clearthroat", "clearsthroat", "throatclear"], ahem, rough_voice),
    sound!("hiccup", "Hiccup", "body", "Hic.", ["hic", "hiccups"], hiccup),
    sound!("sneeze", "Sneeze", "body", "Ah ... ah ... choo.", ["achoo", "atchoo", "sneezes"], sneeze),
    sound!("snore", "Snore", "body", "Asleep: the rattle in, the breath out.", ["snores", "snoring", "zzz"], snore, snore_voice),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{Renderer, SAMPLES_PER_FRAME};

    fn render(sound: &VocalSound, cfg: &VoiceConfig, intensity: f64, length: f64, take: u32) -> (Vec<f32>, Vec<Frame>) {
        let (frames, voice) = perform(sound, cfg, intensity, length, take);
        let n = frames.len();
        let mut renderer = Renderer::new(frames, &voice);
        let mut pcm = vec![0.0f32; n * SAMPLES_PER_FRAME];
        renderer.render(n, &mut pcm);
        (pcm, renderer.into_frames())
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    /// Runs of frames with the voice on: (first frame, length in frames, mean pitch).
    fn voiced_runs(frames: &[Frame]) -> Vec<(usize, usize, f64)> {
        let mut runs = Vec::new();
        let mut i = 0;
        while i < frames.len() {
            if frames[i].av > 0.25 {
                let start = i;
                while i < frames.len() && frames[i].av > 0.25 {
                    i += 1;
                }
                let pitch = frames[start..i].iter().map(|f| f.f0).sum::<f64>() / (i - start) as f64;
                runs.push((start, i - start, pitch));
            } else {
                i += 1;
            }
        }
        runs
    }

    #[test]
    fn every_sound_is_made_by_every_voice() {
        let mut whisper = VoiceConfig::female();
        whisper.whisper = true;
        let mut folds = VoiceConfig::male();
        folds.folds = true;
        for cfg in [VoiceConfig::male(), VoiceConfig::female(), VoiceConfig::preset("deep").unwrap(), whisper, folds] {
            for sound in VOCAL_SOUNDS {
                let (pcm, frames) = render(sound, &cfg, 0.6, 1.0, 0);
                let secs = frames.len() as f64 * FRAME_MS / 1000.0;
                assert!((0.15..6.0).contains(&secs), "{}: {secs} s", sound.id);
                assert!(pcm.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{}", sound.id);
                assert!(rms(&pcm) > 0.0015, "{} is silent ({})", sound.id, rms(&pcm));
                // extremes stay sound too
                for (intensity, length) in [(0.0, 0.25), (1.0, 4.0), (f64::NAN, f64::INFINITY)] {
                    let (pcm, frames) = render(sound, &cfg, intensity, length, 3);
                    assert!(!frames.is_empty() && frames.len() < 6000, "{}: {} frames", sound.id, frames.len());
                    assert!(pcm.iter().all(|v| v.is_finite()), "{}", sound.id);
                }
            }
        }
        // the list has no two sounds answering to one name, and every one is in a known group
        for (i, a) in VOCAL_SOUNDS.iter().enumerate() {
            assert!(GROUPS.contains(&a.group), "{}", a.id);
            assert!(std::ptr::eq(find(a.id).unwrap(), a), "{}", a.id);
            for name in a.aliases {
                assert!(std::ptr::eq(find(name).unwrap(), a), "{name} should be {}", a.id);
            }
            for b in &VOCAL_SOUNDS[i + 1..] {
                assert_ne!(a.id, b.id);
            }
        }
        assert!(std::ptr::eq(find("Uh-Huh").unwrap(), find("uh_huh").unwrap()));
        assert!(std::ptr::eq(find("tee-hee").unwrap(), find("giggle").unwrap()));
        assert!(find("").is_none() && find("the").is_none() && find("female").is_none());
    }

    #[test]
    fn laughter_is_a_train_of_high_calls_that_sink() {
        for cfg in [VoiceConfig::male(), VoiceConfig::female()] {
            let laugh = find("laugh").unwrap();
            let (_, frames) = render(laugh, &cfg, 0.6, 1.0, 0);
            let calls = voiced_runs(&frames);
            assert!((4..=7).contains(&calls.len()), "{} calls", calls.len());
            // each call is short, with breath (and no voice) between it and the next
            for (start, len, _) in &calls {
                assert!((8..=30).contains(len), "a call of {} ms", len * 5);
                let after = &frames[start + len..(start + len + 6).min(frames.len())];
                assert!(after.iter().any(|f| f.av < 0.1 && f.ah > 0.1), "breath after a call");
            }
            // about five a second
            let gap = (calls[1].0 - calls[0].0) as f64 * FRAME_MS;
            assert!((140.0..260.0).contains(&gap), "calls {gap} ms apart");
            // well above the speaking voice (men's laughs average 2.5 times it, women's 2), and sinking
            let lift = calls[0].2 / cfg.pitch_hz;
            assert!((1.7..3.0).contains(&lift), "first call at {lift} times the speaking pitch");
            assert!(calls.last().unwrap().2 < calls[0].2 * 0.92, "the pitch sinks");
            // a giggle is faster and higher than a laugh, a chuckle shorter and lower
            let (_, g) = render(find("giggle").unwrap(), &cfg, 0.6, 1.0, 0);
            let (_, c) = render(find("chuckle").unwrap(), &cfg, 0.6, 1.0, 0);
            let (g, c) = (voiced_runs(&g), voiced_runs(&c));
            assert!(g.len() > calls.len() && c.len() < calls.len(), "{} giggles, {} chuckles", g.len(), c.len());
            assert!(g[0].2 > calls[0].2 && c[0].2 < calls[0].2);
            assert!(g[1].0 - g[0].0 < calls[1].0 - calls[0].0);
        }
    }

    #[test]
    fn strength_length_and_take_do_what_they_say() {
        let cfg = VoiceConfig::female();
        let laugh = find("laugh").unwrap();
        let (soft, soft_frames) = render(laugh, &cfg, 0.2, 1.0, 0);
        let (strong, _) = render(laugh, &cfg, 1.0, 1.0, 0);
        assert!(rms(&strong) > rms(&soft) * 1.3, "{} vs {}", rms(&strong), rms(&soft));
        let (_, long_frames) = render(laugh, &cfg, 0.2, 2.0, 0);
        assert!(long_frames.len() as f64 > soft_frames.len() as f64 * 1.5);
        // the same take is the same laugh; the next one in a script is another
        let (a, _) = render(laugh, &cfg, 0.6, 1.0, 0);
        let (b, _) = render(laugh, &cfg, 0.6, 1.0, 0);
        let (c, _) = render(laugh, &cfg, 0.6, 1.0, 1);
        assert!(a == b && a != c);
        // a whispering voice laughs without voice
        let mut whisper = cfg.clone();
        whisper.whisper = true;
        let (pcm, frames) = render(laugh, &whisper, 0.6, 1.0, 0);
        assert!(frames.iter().all(|f| f.av == 0.0) && rms(&pcm) > 0.002);
        // a whistle is a tone, not a voice
        let (_, w) = render(find("whistle").unwrap(), &cfg, 0.6, 1.0, 0);
        assert!(w.iter().any(|f| f.tone > 0.1) && w.iter().all(|f| f.av == 0.0));
        let top = w.iter().filter(|f| f.tone > 0.05).map(|f| f.tone_hz).fold(0.0, f64::max);
        assert!((1800.0..3200.0).contains(&top), "whistle tops out at {top} Hz");
        // a hum keeps the lips shut
        let (_, h) = render(find("hmm").unwrap(), &cfg, 0.6, 1.0, 0);
        assert!(h.iter().filter(|f| f.av > 0.3).all(|f| f.oral_seal > 0.9 && f.nasal > 0.9));
    }
}
