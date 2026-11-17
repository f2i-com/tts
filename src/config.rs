//! Voice configuration: gender, pitch, tract length and voice-quality controls.

use crate::phone::Ph;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gender {
    Male,
    Female,
}

impl Gender {
    pub fn parse(s: &str) -> Option<Gender> {
        match s.trim().to_ascii_lowercase().as_str() {
            "male" | "m" | "man" => Some(Gender::Male),
            "female" | "f" | "woman" => Some(Gender::Female),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Gender::Male => "male",
            Gender::Female => "female",
        }
    }
}

/// Every knob of the voice. All fields are plain numbers so a UI can bind sliders to them.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceConfig {
    pub gender: Gender,
    /// Average speaking pitch in Hz (male ≈ 85–140, female ≈ 165–250).
    pub pitch_hz: f64,
    /// Vocal-tract length in cm; shorter tracts raise every formant (male ≈ 17, female ≈ 14.2).
    pub tract_length_cm: f64,
    /// Speaking-rate multiplier (1.0 = natural).
    pub speed: f64,
    /// 0..1 — blends modal vocal-fold closure toward a softer, warmer pulse.
    pub warmth: f64,
    /// 0..1 — sub-glottal chest body under the fundamental.
    pub chest_resonance: f64,
    /// 0..0.85 — aspiration noise mixed into voicing.
    pub breathiness: f64,
    /// 0..0.8 — baseline velum opening on vowels.
    pub nasality: f64,
    /// 0..0.75 — jitter, shimmer and sub-harmonic creak.
    pub roughness: f64,
    /// 0.45..1.6 — consonant burst / frication energy and transition speed.
    pub crispness: f64,
    /// 0..3 — depth of the intonation contour (0 = monotone, 1 = natural).
    pub expressiveness: f64,
    /// 0..1 — slow pitch vibrato depth.
    pub vibrato: f64,
    /// Whisper: noise-excited tract, no vocal-fold vibration.
    pub whisper: bool,
    /// Simulate the vocal folds - two masses on springs per fold, blown apart by the lungs
    /// (`src/folds.rs`) - instead of reading each puff of air from a formula.
    pub folds: bool,
    /// Output gain applied after synthesis (1.0 = calibrated level).
    pub volume: f64,
    /// 0..1 — how unevenly the voice speaks: word-to-word tempo drift, pause lengths and
    /// accent heights vary like a person's do. 0 is perfectly regular.
    pub variation: f64,
    /// Picks the random "take". The same text, voice and seed always sound identical.
    pub seed: u32,
    /// Tone curve of the voice: gain in dB at each of [`TONE_BANDS_HZ`], applied to the
    /// voiced sound. This is the "fingerprint" part of a voice's timbre; a profile fitted
    /// to a real speaker (tools/fit_voice.py) is mostly these numbers.
    pub tone: [f64; TONE_BANDS],
    /// Simple tone controls in dB, added on top of `tone`: low end, the 3-4 kHz region that
    /// makes a voice cut through, and the airy top.
    pub bass: f64,
    pub presence: f64,
    pub brightness: f64,
    /// Multipliers for the five formant frequencies of the whole voice (1 = unchanged):
    /// F1 (how open the vowels sound), F2 (how far forward the tongue is), F3, and the two
    /// upper resonances F4 and F5 that colour a voice without changing the vowel.
    pub formants: [f64; 5],
    /// The speaker's own vowels: multipliers for F1, F2, F3 of each vowel, in the order of
    /// [`VOWEL_KEYS`] (1 = unchanged). This is where an accent and the shape of a particular
    /// mouth live; `tools/fit_voice.py` measures it from recordings.
    pub vowels: [[f64; 3]; VOWEL_COUNT],
    /// Length of the pauses between phrases and sentences (1 = normal).
    pub pauses: f64,
    /// How audibly the speaker breathes in between sentences: 0 = silent, 0.25 = a barely
    /// audible intake, 1 = a clear breath.
    pub breaths: f64,
    /// Share of each cycle the vocal folds are open: the voice's basic sound. Low values
    /// give a pressed, tight voice; high values a relaxed, softer one.
    pub open_quotient: f64,
    /// How strongly the airway below the glottis is felt while the folds are open: it damps
    /// the first resonance for part of each cycle and adds the windpipe's own resonance.
    pub subglottal: f64,
    /// How much of a small room is heard around the voice: the faint tail every real
    /// recording has. 0 = bone-dry, 0.25 = the default, 1 = clearly roomy.
    pub room: f64,
    /// How much the vocal folds open up as the pitch rises, as in a real larynx (stretched
    /// folds are thinner and close for a shorter part of each cycle). 0 = one fixed pulse,
    /// 1 = about what was measured in real speakers.
    pub pitch_coupling: f64,
    /// The top of the voice: how much of the vocal tract's resonances above about 5 kHz is
    /// heard. 0 = none (a formant synthesizer's usual dull vowels), 1 = the level measured in
    /// real speakers. See [`DEFAULT_AIR`].
    pub air: f64,
    /// 0..1 - how much the speaker is laughing while speaking. A little is a smile in the
    /// voice (lips spread, pitch up, breath in it); more, and the laugh breaks into the
    /// stressed vowels, which are drawn out and shake with it ("I ca-ha-han't believe it").
    pub laughter: f64,
    /// 0..1 - vocal fry: how far the voice drops into creak (slow, uneven pulses) as a phrase
    /// runs out. 0 = never, 0.5 = the plain voices' own amount, 1 = strongly and early.
    pub creak: f64,
    /// How abruptly the vocal folds shut at the end of each cycle: 0 = gently (a soft, dark
    /// voice), 1 = sharply (bright, with harmonics far up). This is the tone of the voice at
    /// its source. Defaults: 0.75 for the male voice, 0.25 for the female one - the values
    /// at which the balance of the fundamental against the third formant matches real
    /// speakers of each.
    pub closure: f64,
}

/// Default of the `air` setting. At 1 the vowels' top band is as strong as in real voices
/// (about -40 to -47 dB from 5 to 12 kHz against the band below 2.5 kHz); at 0 they have
/// nothing there, which no real voice does. 0.4 puts it 16 dB under the real level: the
/// spectrum is then far closer to a reference voice's (tools/closeness.py) and no sound is
/// misheard more often, at a cost of 0.02-0.05 on a naturalness predictor that marks any
/// synthetic top band down.
pub const DEFAULT_AIR: f64 = 0.14;
/// Defaults of the `closure` setting.
pub const MALE_CLOSURE: f64 = 0.78;
pub const FEMALE_CLOSURE: f64 = 0.43;
/// Open quotient of the default voices. Measured on 146 sentences against the earlier 0.58.
pub const MALE_OPEN_QUOTIENT: f64 = 0.66;
pub const FEMALE_OPEN_QUOTIENT: f64 = 0.66;

/// How breathy a voice is beyond the plain voices (0 at their own breathiness, 0.2, and less;
/// 1 at the most the setting allows): what slackens the pulse and lets more breath through.
pub fn breathy_excess(breathiness: f64) -> f64 {
    ((breathiness - 0.2) / 0.65).clamp(0.0, 1.0)
}

/// Number of vowels with individual formant multipliers.
pub const VOWEL_COUNT: usize = 16;
/// The vowels of [`VoiceConfig::vowels`], in order.
pub const VOWEL_KEYS: [Ph; VOWEL_COUNT] = [
    Ph::IY, Ph::IH, Ph::EH, Ph::AE, Ph::AA, Ph::AO, Ph::UH, Ph::UW,
    Ph::AH, Ph::AX, Ph::ER, Ph::EY, Ph::AY, Ph::OY, Ph::AW, Ph::OW,
];

/// Position of a vowel in [`VoiceConfig::vowels`].
pub fn vowel_index(ph: Ph) -> Option<usize> {
    VOWEL_KEYS.iter().position(|&v| v == ph)
}

/// Each voice's own vowels: F1, F2 and F3 of every vowel relative to the shared vowel
/// table, in the order of [`VOWEL_KEYS`]. Real vowels are not one table scaled by the length
/// of the tract: the first formant sits higher than the length alone gives (a more open
/// mouth), most of all in a woman's voice, and /u/ and /o/ are further front. Both tables
/// were measured on reference voices (`tools/fit_voice.py --vowels`). On 146 sentences the
/// female voice gains 0.10 predicted naturalness and loses a point of misheard sounds with
/// hers, the male voice gains 0.05 with his. The female third formants were then brought
/// down to those of a woman reading (`tools/phones.py formants`: they stood 5-15% above
/// hers in every vowel, where the man's match his reader's), for another 0.09 and a point
/// fewer misheard sounds on 90 sentences; and the second formants of her front vowels half
/// way down to hers (they stood 7-14% above), for a little more of both. The vowel of
/// "could" and "good" (UH) has a lower F1 and higher F2 and F3 in both voices than the fit
/// gave (her F2 stood 7-11% below her reader's): it was heard as the vowel of "cot" or
/// "coat" one time in six, against one in fifty for the readers. The man's /i/ has an F1 8%
/// lower than the fit gave: the F1 his output really has stood 11% above the value asked for,
/// and "keep" and "each" were heard with the vowel of "cape" (13 times in 400 sentences,
/// now 2; the reader never).
#[rustfmt::skip]
pub const FEMALE_VOWEL_SPACE: [[f64; 3]; VOWEL_COUNT] = [
    [1.037, 0.960, 0.852], // IY
    [1.065, 0.925, 0.955], // IH
    [1.180, 0.929, 0.962], // EH
    [1.225, 0.903, 0.968], // AE
    [1.113, 0.985, 1.015], // AA
    [1.159, 1.082, 0.909], // AO
    [1.024, 1.142, 1.023], // UH
    [0.978, 1.169, 0.975], // UW
    [1.164, 1.099, 1.025], // AH
    [1.180, 1.090, 1.006], // AX
    [1.039, 0.967, 0.960], // ER
    [1.180, 0.970, 0.926], // EY
    [1.131, 1.063, 0.949], // AY
    [1.104, 1.019, 1.020], // OY
    [1.154, 1.072, 1.019], // AW
    [1.077, 1.111, 0.971], // OW
];
#[rustfmt::skip]
pub const MALE_VOWEL_SPACE: [[f64; 3]; VOWEL_COUNT] = [
    [1.017, 0.954, 0.925], // IY
    [1.032, 0.920, 0.975], // IH
    [1.057, 0.955, 0.978], // EH
    [1.145, 0.905, 0.979], // AE
    [1.005, 0.952, 1.002], // AA
    [1.019, 1.070, 0.988], // AO
    [0.972, 1.062, 1.042], // UH
    [1.044, 1.110, 0.997], // UW
    [1.048, 1.024, 0.996], // AH
    [1.056, 1.024, 0.994], // AX
    [0.987, 0.994, 1.000], // ER
    [1.057, 0.931, 0.954], // EY
    [1.037, 1.017, 0.969], // AY
    [1.048, 0.977, 0.978], // OY
    [1.057, 1.022, 0.986], // AW
    [1.044, 1.063, 1.018], // OW
];

/// Number of bands in a tone curve.
pub const TONE_BANDS: usize = 10;
/// Centre frequencies of the tone-curve bands (the first and last are shelves).
pub const TONE_BANDS_HZ: [f64; TONE_BANDS] =
    [125.0, 250.0, 500.0, 1000.0, 1500.0, 2200.0, 3000.0, 4000.0, 5500.0, 8000.0];


/// Into the range, and a number that is not one to its lower end.
fn clamp_setting(v: f64, lo: f64, hi: f64) -> f64 {
    if v.is_nan() {
        return lo;
    }
    v.max(lo).min(hi)
}

impl VoiceConfig {
    /// The plain male voice.
    pub fn male() -> VoiceConfig {
        VoiceConfig {
            gender: Gender::Male,
            pitch_hz: 98.0,
            tract_length_cm: 17.55,
            speed: 1.0,
            warmth: 0.56,
            chest_resonance: 0.54,
            breathiness: 0.11,
            nasality: 0.04,
            roughness: 0.04,
            crispness: 1.0,
            expressiveness: 1.21,
            vibrato: 0.02,
            whisper: false,
            folds: false,
            volume: 1.0,
            variation: 0.5,
            seed: 0,
            tone: [0.0; TONE_BANDS],
            bass: 0.0,
            presence: 0.0,
            brightness: 0.0,
            formants: [1.0; 5],
            vowels: [[1.0; 3]; VOWEL_COUNT],
            pauses: 1.0,
            breaths: 0.25,
            open_quotient: MALE_OPEN_QUOTIENT,
            subglottal: 0.28,
            room: 0.52,
            pitch_coupling: 0.0,
            air: DEFAULT_AIR,
            laughter: 0.0,
            creak: 0.5,
            closure: MALE_CLOSURE,
        }
    }

    /// The plain female voice. Its pitch, tract length and vowels ([`FEMALE_VOWEL_SPACE`])
    /// are those of a reference voice; everything not listed here is as in the male voice.
    pub fn female() -> VoiceConfig {
        VoiceConfig {
            gender: Gender::Female,
            pitch_hz: 190.5,
            tract_length_cm: 14.2,
            warmth: 0.62,
            chest_resonance: 0.48,
            breathiness: 0.2,
            roughness: 0.08,
            crispness: 1.02,
            expressiveness: 1.17,
            vibrato: 0.03,
            open_quotient: FEMALE_OPEN_QUOTIENT,
            subglottal: 0.34,
            closure: FEMALE_CLOSURE,
            ..VoiceConfig::male()
        }
    }

    pub fn for_gender(gender: Gender) -> VoiceConfig {
        match gender {
            Gender::Male => VoiceConfig::male(),
            Gender::Female => VoiceConfig::female(),
        }
    }

    /// A built-in voice by its id (see [`PRESETS`]; earlier ids are still understood).
    pub fn preset(id: &str) -> Option<VoiceConfig> {
        let id = PRESET_ALIASES.iter().find(|(old, _)| *old == id).map_or(id, |(_, now)| *now);
        PRESETS.iter().find(|p| p.id == id).map(|p| p.config())
    }

    /// The numeric settings by the name the JavaScript API gives them.
    pub fn number_mut(&mut self, name: &str) -> Option<&mut f64> {
        Some(match name {
            "pitchHz" => &mut self.pitch_hz,
            "tractLengthCm" => &mut self.tract_length_cm,
            "speed" => &mut self.speed,
            "warmth" => &mut self.warmth,
            "chestResonance" => &mut self.chest_resonance,
            "breathiness" => &mut self.breathiness,
            "nasality" => &mut self.nasality,
            "roughness" => &mut self.roughness,
            "crispness" => &mut self.crispness,
            "expressiveness" => &mut self.expressiveness,
            "vibrato" => &mut self.vibrato,
            "volume" => &mut self.volume,
            "variation" => &mut self.variation,
            "bass" => &mut self.bass,
            "presence" => &mut self.presence,
            "brightness" => &mut self.brightness,
            "pauses" => &mut self.pauses,
            "breaths" => &mut self.breaths,
            "openQuotient" => &mut self.open_quotient,
            "subglottal" => &mut self.subglottal,
            "room" => &mut self.room,
            "pitchCoupling" => &mut self.pitch_coupling,
            "air" => &mut self.air,
            "closure" => &mut self.closure,
            "laughter" => &mut self.laughter,
            "creak" => &mut self.creak,
            _ => return None,
        })
    }

    /// Switches to the default voice of `gender`, keeping speaking rate, volume and the
    /// kind of voice source.
    pub fn set_gender(&mut self, gender: Gender) {
        let (speed, volume, variation, seed, folds) = (self.speed, self.volume, self.variation, self.seed, self.folds);
        *self = VoiceConfig::for_gender(gender);
        self.folds = folds;
        self.speed = speed;
        self.volume = volume;
        self.variation = variation;
        self.seed = seed;
    }

    /// Clamps every field into the range the engine is stable and sensible in.
    pub fn sanitize(&mut self) {
        self.pitch_hz = clamp_setting(self.pitch_hz, 60.0, 350.0);
        self.tract_length_cm = clamp_setting(self.tract_length_cm, 12.2, 21.5);
        self.speed = clamp_setting(self.speed, 0.55, 1.75);
        self.warmth = clamp_setting(self.warmth, 0.0, 1.0);
        self.chest_resonance = clamp_setting(self.chest_resonance, 0.0, 1.0);
        self.breathiness = clamp_setting(self.breathiness, 0.0, 0.85);
        self.nasality = clamp_setting(self.nasality, 0.0, 0.80);
        self.roughness = clamp_setting(self.roughness, 0.0, 0.75);
        self.crispness = clamp_setting(self.crispness, 0.45, 1.6);
        self.expressiveness = clamp_setting(self.expressiveness, 0.0, 3.0);
        self.vibrato = clamp_setting(self.vibrato, 0.0, 1.0);
        self.volume = clamp_setting(self.volume, 0.0, 4.0);
        self.variation = clamp_setting(self.variation, 0.0, 1.0);
        for g in self.tone.iter_mut() {
            *g = clamp_setting(*g, -18.0, 18.0);
        }
        self.bass = clamp_setting(self.bass, -12.0, 12.0);
        self.presence = clamp_setting(self.presence, -12.0, 12.0);
        self.brightness = clamp_setting(self.brightness, -12.0, 12.0);
        self.open_quotient = clamp_setting(self.open_quotient, 0.5, 0.74);
        self.subglottal = clamp_setting(self.subglottal, 0.0, 1.0);
        self.room = clamp_setting(self.room, 0.0, 1.0);
        self.pitch_coupling = clamp_setting(self.pitch_coupling, 0.0, 1.0);
        self.air = clamp_setting(self.air, 0.0, 1.0);
        self.laughter = clamp_setting(self.laughter, 0.0, 1.0);
        self.creak = clamp_setting(self.creak, 0.0, 1.0);
        self.closure = clamp_setting(self.closure, 0.0, 1.0);
        self.pauses = clamp_setting(self.pauses, 0.3, 3.0);
        self.breaths = clamp_setting(self.breaths, 0.0, 1.0);
        for f in self.formants.iter_mut() {
            *f = clamp_setting(*f, 0.75, 1.35);
        }
        for v in self.vowels.iter_mut() {
            for f in v.iter_mut() {
                *f = clamp_setting(*f, 0.6, 1.6);
            }
        }
    }

    /// F1, F2 and F3 of a vowel relative to the shared table: the voice's own vowel space
    /// times the [`vowels`](Self::vowels) setting.
    pub fn vowel_shape(&self, index: usize) -> [f64; 3] {
        let own = self.vowels[index];
        let space = match self.gender {
            Gender::Male => MALE_VOWEL_SPACE[index],
            Gender::Female => FEMALE_VOWEL_SPACE[index],
        };
        [own[0] * space[0], own[1] * space[1], own[2] * space[2]]
    }

    /// Generates a distinct, physically coherent human [`VoiceConfig`] from `seed`.
    ///
    /// A body size sets the tract length and (loosely) the pitch, the resonances and the
    /// vowels get the speaker's own pattern, the voice quality is one of a few kinds (soft and
    /// breathy, pressed and bright, gravelly, modal), and the speaking style its own rate,
    /// liveliness and pauses - all inside natural conversational ranges.
    pub fn randomized(
        seed: u32,
        gender_override: Option<Gender>,
        locked_pitch_hz: Option<f64>,
    ) -> VoiceConfig {
        struct Rng(u64);
        impl Rng {
            fn u01(&mut self) -> f64 {
                self.0 ^= self.0 >> 12;
                self.0 ^= self.0 << 25;
                self.0 ^= self.0 >> 27;
                let z = self.0.wrapping_mul(0x2545_F491_4F6C_DD1D);
                ((z >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
            }
            fn range(&mut self, lo: f64, hi: f64) -> f64 {
                lo + (hi - lo) * self.u01()
            }
            fn tri(&mut self) -> f64 {
                self.u01() + self.u01() - 1.0
            }
        }

        let mut init = (seed as u64)
            .wrapping_mul(0x9E37_79B9_7F4A_7C15)
            .wrapping_add(0xD1B5_4A32_D192_ED03);
        if init == 0 {
            init = 0xCAFE_BABE_1234_5678;
        }
        let mut rng = Rng(init);

        let gender = gender_override.unwrap_or_else(|| {
            if rng.u01() < 0.5 { Gender::Male } else { Gender::Female }
        });
        let mut cfg = VoiceConfig::for_gender(gender);
        cfg.seed = seed;

        let male = gender == Gender::Male;
        // (to a whole number of steps, divided back so that 0.41 prints as 0.41)
        let round = |v: f64, step: f64| {
            let per_unit = (1.0 / step).round();
            (v * per_unit).round() / per_unit
        };

        // Body: one draw moves the length of the tract and the pitch together - a larger
        // speaker has a longer tract and, more loosely, a lower voice - and each also varies on
        // its own. The tract's length is what a listener (and a speaker-verification model)
        // hears most as "who": a centimetre moves a voice further than any other setting.
        let base_pitch = if male { 112.0 } else { 206.0 };
        let mut size = rng.tri();
        let pitch_hz = match locked_pitch_hz {
            Some(hz) => {
                // a voice given its pitch is built around it
                let st = 12.0 * (hz.max(1.0) / base_pitch).log2();
                size = (-st / 5.0).clamp(-1.0, 1.0) * 0.6 + 0.4 * size;
                hz
            }
            None => {
                let st = -2.6 * size + 3.0 * rng.tri();
                (base_pitch * 2f64.powf(st / 12.0)).clamp(if male { 82.0 } else { 160.0 }, if male { 150.0 } else { 242.0 })
            }
        };
        cfg.pitch_hz = round(pitch_hz, 0.1);
        let tract = if male { 17.0 + 1.3 * size + 0.8 * rng.tri() } else { 14.2 + 0.85 * size + 0.6 * rng.tri() };
        cfg.tract_length_cm = round(tract.clamp(if male { 15.3 } else { 13.0 }, if male { 19.0 } else { 15.5 }), 0.01);

        // The rest of the tract's shape: each speaker's resonances stand a little apart from
        // where the length alone puts them, the upper ones (the "speaker's formant") most.
        let upper = 1.0 + 0.07 * rng.tri();
        cfg.formants = [
            round(1.0 + 0.05 * rng.tri(), 0.001),
            round(1.0 + 0.06 * rng.tri(), 0.001),
            round(1.0 + 0.05 * rng.tri(), 0.001),
            round(upper, 0.001),
            round(upper, 0.001),
        ];

        // Idiolect: where this speaker puts the vowels. A few habits shared across the vowels
        // (how open the mouth is, how far forward "goose" and "goat" are, how front the front
        // vowels and how back the back ones, a raised "trap") and a little of each vowel's own.
        let openness = 0.035 * rng.tri();
        let back_fronting = 0.08 * rng.tri();
        let spread = 0.03 * rng.tri();
        let tense_ae = 0.05 * rng.tri();
        for (i, v) in VOWEL_KEYS.iter().enumerate() {
            let mut m = [1.0 + openness + 0.02 * rng.tri(), 1.0 + 0.025 * rng.tri(), 1.0 + 0.012 * rng.tri()];
            match v {
                Ph::UW | Ph::UH | Ph::OW | Ph::AW => m[1] *= 1.0 + back_fronting,
                _ => {}
            }
            match v {
                Ph::IY | Ph::IH | Ph::EH | Ph::AE | Ph::EY => m[1] *= 1.0 + spread,
                Ph::AA | Ph::AO | Ph::UW | Ph::UH | Ph::OW => m[1] *= 1.0 - spread,
                _ => {}
            }
            if *v == Ph::AE {
                m[0] *= 1.0 - tense_ae;
                m[1] *= 1.0 + 0.6 * tense_ae;
            }
            cfg.vowels[i] = [round(m[0], 0.001), round(m[1], 0.001), round(m[2], 0.001)];
        }

        // Voice quality: a kind of voice, not every setting drawn on its own - that gives the
        // middle of every range, and voices that all sound alike.
        let kind = rng.u01();
        let (breathy, warm, closure, open, rough) = if kind < 0.22 {
            // soft and breathy: the folds never quite close
            (rng.range(0.24, 0.42), rng.range(0.6, 0.8), if male { rng.range(0.4, 0.62) } else { rng.range(0.1, 0.26) }, rng.range(0.65, 0.7), rng.range(0.03, 0.1))
        } else if kind < 0.42 {
            // pressed and bright: a firm, ringing voice
            (rng.range(0.02, 0.09), rng.range(0.24, 0.46), if male { rng.range(0.8, 0.98) } else { rng.range(0.42, 0.62) }, rng.range(0.57, 0.62), rng.range(0.04, 0.12))
        } else if kind < 0.56 {
            // gravelly: an uneven, rough voice
            (rng.range(0.1, 0.22), rng.range(0.4, 0.7), if male { rng.range(0.55, 0.85) } else { rng.range(0.2, 0.45) }, rng.range(0.6, 0.67), rng.range(0.2, 0.34))
        } else {
            // modal
            (rng.range(0.06, 0.2), rng.range(0.4, 0.74), if male { rng.range(0.55, 0.88) } else { rng.range(0.16, 0.42) }, rng.range(0.6, 0.68), rng.range(0.04, 0.14))
        };
        cfg.breathiness = round(breathy, 0.01);
        cfg.warmth = round(warm, 0.01);
        cfg.closure = round(closure, 0.01);
        cfg.open_quotient = round(open, 0.01);
        cfg.roughness = round(rough, 0.01);
        cfg.chest_resonance = round(if male { rng.range(0.3, 0.74) } else { rng.range(0.26, 0.6) }, 0.01);
        // most voices are hardly nasal; some are clearly so
        cfg.nasality = round(if rng.u01() < 0.12 { rng.range(0.12, 0.22) } else { rng.range(0.0, 0.08) }, 0.01);
        cfg.vibrato = round(rng.range(0.0, 0.16), 0.01);
        cfg.air = round(rng.range(0.25, 0.6), 0.01);
        cfg.subglottal = round(rng.range(0.0, 0.25), 0.01);
        cfg.pitch_coupling = round(rng.range(0.0, 0.45), 0.01);
        cfg.crispness = round(rng.range(0.86, 1.2), 0.01);
        cfg.creak = round(rng.range(0.1, 0.9), 0.01);

        // Speaking style: how fast, how lively, how long the pauses, how even the pace.
        cfg.speed = round(1.01 + 0.15 * rng.tri(), 0.01);
        cfg.expressiveness = round(1.08 + 0.32 * rng.tri(), 0.01);
        cfg.pauses = round(1.0 + 0.25 * rng.tri(), 0.01);
        cfg.variation = round(0.5 + 0.2 * rng.tri(), 0.01);
        cfg.room = round(rng.range(0.16, 0.36), 0.01);
        cfg.bass = round(rng.tri() * 3.0, 0.5);
        cfg.presence = round(rng.tri() * 3.0, 0.5);
        cfg.brightness = round(rng.tri() * 2.5, 0.5);

        // Smooth correlated 10-band speaker coloration curve
        let mut raw_eq = [0.0f64; TONE_BANDS];
        for slot in &mut raw_eq {
            *slot = rng.tri() * 4.5;
        }
        for i in 0..TONE_BANDS {
            let l = raw_eq[i.saturating_sub(1)];
            let c = raw_eq[i];
            let r = raw_eq[(i + 1).min(TONE_BANDS - 1)];
            cfg.tone[i] = round(0.25 * l + 0.5 * c + 0.25 * r, 0.5);
        }

        cfg.sanitize();
        cfg
    }

    /// The tone curve actually applied: `tone` plus the bass / presence / brightness controls.
    pub fn effective_tone(&self) -> [f64; TONE_BANDS] {
        const BASS: [f64; TONE_BANDS] = [1.0, 0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
        const PRESENCE: [f64; TONE_BANDS] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.4, 1.0, 1.0, 0.4, 0.0];
        const BRIGHTNESS: [f64; TONE_BANDS] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.3, 1.0, 1.0];
        let mut out = self.tone;
        for (i, g) in out.iter_mut().enumerate() {
            *g = clamp_setting(
                *g + self.bass * BASS[i] + self.presence * PRESENCE[i] + self.brightness * BRIGHTNESS[i],
                -24.0,
                24.0,
            );
        }
        out
    }
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig::male()
    }
}

/// Names of the numeric settings, as [`VoiceConfig::number_mut`] takes them.
pub const NUMBER_SETTINGS: [&str; 26] = [
    "pitchHz", "tractLengthCm", "speed", "warmth", "chestResonance", "breathiness", "nasality", "roughness",
    "crispness", "expressiveness", "vibrato", "volume", "variation", "bass", "presence", "brightness",
    "pauses", "breaths", "openQuotient", "subglottal", "room", "pitchCoupling", "air", "closure",
    "laughter", "creak",
];

/// A built-in voice: one of the two plain voices with a few settings changed, so that
/// whatever improves the plain voices improves these too.
#[derive(Clone, Copy)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub gender: Gender,
    change: fn(&mut VoiceConfig),
}

impl Preset {
    pub fn config(&self) -> VoiceConfig {
        let mut cfg = VoiceConfig::for_gender(self.gender);
        (self.change)(&mut cfg);
        cfg
    }
}

impl std::fmt::Debug for Preset {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Preset({:?})", self.id)
    }
}

/// The built-in voices: the two plain ones, and for each a few characters - lower or higher,
/// rougher or softer, firmer or older.
pub static PRESETS: [Preset; 10] = [
    Preset { id: "male", label: "Male", gender: Gender::Male, change: |_| {} },
    Preset {
        id: "deep",
        label: "Deep",
        gender: Gender::Male,
        change: |v| {
            v.pitch_hz = 86.0;
            v.tract_length_cm = 18.8;
            v.speed = 0.96;
            v.warmth = 0.64;
            v.chest_resonance = 0.72;
            v.expressiveness = 0.95;
        },
    },
    Preset {
        id: "husky",
        label: "Husky",
        gender: Gender::Male,
        change: |v| {
            v.pitch_hz = 106.0;
            v.speed = 0.96;
            v.warmth = 0.62;
            v.breathiness = 0.28;
            v.roughness = 0.34;
        },
    },
    Preset {
        id: "crisp",
        label: "Crisp",
        gender: Gender::Male,
        change: |v| {
            // firm and bright, every consonant in place: an announcer
            v.pitch_hz = 118.0;
            v.tract_length_cm = 16.6;
            v.closure = 0.95;
            v.breathiness = 0.06;
            v.warmth = 0.36;
            v.crispness = 1.18;
            v.expressiveness = 1.18;
            v.speed = 1.04;
        },
    },
    Preset {
        id: "elder",
        label: "Elder",
        gender: Gender::Male,
        change: |v| {
            // an older man: the voice a little higher, rougher and breathier, slower, unsteady
            v.pitch_hz = 124.0;
            v.tract_length_cm = 17.4;
            v.roughness = 0.2;
            v.breathiness = 0.28;
            v.vibrato = 0.2;
            v.speed = 0.9;
            v.crispness = 0.9;
            v.expressiveness = 0.92;
            v.pauses = 1.2;
        },
    },
    Preset { id: "female", label: "Female", gender: Gender::Female, change: |_| {} },
    Preset {
        id: "bright",
        label: "Bright",
        gender: Gender::Female,
        change: |v| {
            v.pitch_hz = 222.0;
            v.tract_length_cm = 13.65;
            v.speed = 1.03;
            v.warmth = 0.58;
            v.expressiveness = 1.12;
        },
    },
    Preset {
        id: "soft",
        label: "Soft",
        gender: Gender::Female,
        change: |v| {
            // gentle and breathy, unhurried
            v.pitch_hz = 192.0;
            v.breathiness = 0.34;
            v.warmth = 0.78;
            v.closure = 0.15;
            v.open_quotient = 0.68;
            v.speed = 0.94;
            v.expressiveness = 0.92;
        },
    },
    Preset {
        id: "mellow",
        label: "Mellow",
        gender: Gender::Female,
        change: |v| {
            // a lower, warmer speaking voice
            v.pitch_hz = 178.0;
            v.tract_length_cm = 14.9;
            v.warmth = 0.72;
            v.chest_resonance = 0.62;
            v.speed = 0.96;
            v.expressiveness = 0.98;
        },
    },
    Preset {
        id: "whisper",
        label: "Whisper",
        gender: Gender::Female,
        change: |v| {
            v.whisper = true;
            v.speed = 0.94;
            v.breathiness = 0.58;
            v.expressiveness = 0.9;
        },
    },
];

/// Ids of the earlier, longer preset list: each now gives the nearest of [`PRESETS`].
pub const PRESET_ALIASES: [(&str, &str); 10] = [
    ("warm_baritone", "male"),
    ("bright_tenor", "male"),
    ("deep_bass", "deep"),
    ("velvet_narrator", "deep"),
    ("husky_raspy", "husky"),
    ("clear_alto", "female"),
    ("warm_mezzo", "female"),
    ("bright_soprano", "bright"),
    ("young_bright", "bright"),
    ("soft_whisper", "whisper"),
];
