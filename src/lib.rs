//! # TTS (`tts.computer`)
//!
//! A physical human vocal-tract speech and singing synthesizer: English text or musical
//! notes in, 24 kHz audio out. Zero recordings, zero neural networks, zero external crates —
//! every sample is computed directly from vocal-tract physics:
//!
//! text → normalization → pronunciation (lexicon + rules) → prosody (durations, intonation)
//! → 5 ms articulation frames → glottal source + vocal-tract filter → 24 kHz PCM.
//!
//! Compiles natively and to a single self-contained WebAssembly binary (`tts.wasm`) with
//! zero external imports and its JavaScript `TTS` wrapper embedded right inside the `.wasm`.
//!
//! ```
//! use tts::{TTS, VoiceConfig};
//!
//! let voice = TTS::new(VoiceConfig::female());
//! let speech = voice.speak("Hello there!");
//! assert_eq!(speech.sample_rate, 24_000);
//! assert!(speech.duration() > 0.5);
//! ```

use std::collections::HashMap;

pub mod config;
mod durations;
pub mod folds;
pub mod frames;
mod g2p;
pub mod lexicon;
pub mod phone;
pub mod plan;
pub mod render;
pub mod script;
pub mod sing;
pub mod tables;
pub mod text;
pub mod timeline;
pub mod vocal;
pub mod wasm;
pub mod wav;

pub use config::{Gender, Preset, VoiceConfig, PRESETS};
pub use frames::Frame;
pub use lexicon::Lexicon;
pub use plan::Plan;
pub use render::{SAMPLES_PER_FRAME, SAMPLE_RATE};
pub use script::{ScriptCue, ScriptedSpeech, SegmentSpan};
pub use sing::{ChorusPart, ChorusScore, SingNote, SingScore};
pub use vocal::{VocalSound, VOCAL_SOUNDS};

use render::Renderer;

/// Development-time parameter overrides, read once from the environment variable
/// `VS_TUNE="name=value,name=value"`. Only in builds with the `dev-hooks` feature, which the
/// measuring tools need (`cargo build --release --example say --features dev-hooks`);
/// without it every hook is its default and no environment variable is read.
#[cfg(feature = "dev-hooks")]
pub(crate) fn tune(name: &str, default: f64) -> f64 {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    static MAP: OnceLock<HashMap<String, f64>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        std::env::var("VS_TUNE")
            .unwrap_or_default()
            .split(',')
            .filter_map(|kv| {
                let (k, v) = kv.split_once('=')?;
                Some((k.trim().to_string(), v.trim().parse().ok()?))
            })
            .collect()
    });
    map.get(name).copied().unwrap_or(default)
}

#[cfg(not(feature = "dev-hooks"))]
#[inline(always)]
pub(crate) fn tune(_name: &str, default: f64) -> f64 {
    default
}

/// Whether this build listens to the development hooks (`VS_TUNE` and the `VS_*_FILE`
/// environment variables).
#[doc(hidden)]
pub const DEV_HOOKS: bool = cfg!(feature = "dev-hooks");

/// Control frames per second (one every 5 ms).
pub const FRAME_RATE: u32 = 200;
/// Values per frame in [`visemes`] output.
pub const VISEME_STRIDE: usize = 9;
/// Meaning of each value in a viseme frame.
pub const VISEME_FIELDS: [&str; VISEME_STRIDE] = [
    "jaw",
    "tongueHeight",
    "tongueFront",
    "tongueTip",
    "lipRound",
    "lipSpread",
    "lipClose",
    "velum",
    "energy",
];

/// When a word is spoken, in seconds from the start of the audio.
#[derive(Clone, Debug, PartialEq)]
pub struct WordTiming {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

/// A fully rendered utterance.
#[derive(Clone, Debug)]
pub struct Speech {
    /// Mono PCM in -1..1 at [`SAMPLE_RATE`].
    pub samples: Vec<f32>,
    pub sample_rate: u32,
    /// The text as it was read (numbers and symbols expanded to words).
    pub text: String,
    /// IPA transcription.
    pub ipa: String,
    pub words: Vec<WordTiming>,
    /// Articulation frames (5 ms each) for lip-sync or visualisation.
    pub frames: Vec<Frame>,
}

impl Speech {
    pub fn duration(&self) -> f64 {
        self.samples.len() as f64 / self.sample_rate as f64
    }

    pub fn to_wav(&self) -> Vec<u8> {
        wav::encode_wav(&self.samples, self.sample_rate)
    }

    pub fn visemes(&self) -> Vec<f32> {
        visemes(&self.frames)
    }
}

/// Flattens articulation frames to `[jaw, tongueHeight, tongueFront, tongueTip, lipRound,
/// lipSpread, lipClose, velum, energy]` per frame, all 0..1.
pub fn visemes(frames: &[Frame]) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames.len() * VISEME_STRIDE);
    for f in frames {
        out.extend_from_slice(&[
            f.jaw as f32,
            f.t_h as f32,
            f.t_f as f32,
            f.t_tip as f32,
            f.lip_r as f32,
            f.lip_s as f32,
            f.lip_c as f32,
            f.velum as f32,
            f.rms as f32,
        ]);
    }
    out
}

pub(crate) fn word_timings(plan: &Plan, frames: &[Frame]) -> Vec<WordTiming> {
    let frame_sec = 1.0 / FRAME_RATE as f64;
    let mut spans = vec![(f64::INFINITY, 0.0f64); plan.word_count()];
    for f in frames {
        if f.word >= 0 {
            if let Some(span) = spans.get_mut(f.word as usize) {
                span.0 = span.0.min(f.t_sec);
                span.1 = f.t_sec + frame_sec;
            }
        }
    }
    plan.clauses
        .iter()
        .flat_map(|c| c.words.iter())
        .zip(spans)
        .filter(|(_, span)| span.0.is_finite())
        .map(|(w, span)| WordTiming {
            word: w.text.clone(),
            start: span.0,
            end: span.1,
        })
        .collect()
}

pub(crate) fn apply_volume(samples: &mut [f32], volume: f64) {
    if volume == 1.0 {
        return;
    }
    for s in samples.iter_mut() {
        *s = soft_limit(*s as f64 * volume) as f32;
    }
}

/// Leaves a sample alone up to 0.9 of full scale and bends what is above it over, so that
/// a raised volume, or a passage of a long stream that is louder than its calibrated
/// opening, is rounded off instead of clipped.
pub(crate) fn soft_limit(x: f64) -> f64 {
    const KNEE: f64 = 0.9;
    let over = x.abs() - KNEE;
    if over <= 0.0 {
        x
    } else {
        x.signum() * (KNEE + (1.0 - KNEE) * (over / (1.0 - KNEE)).tanh())
    }
}

/// The synthesizer: a voice configuration, pronunciation lexicon, and optional reusable
/// custom speaker profiles.
pub struct TTS {
    pub config: VoiceConfig,
    pub lexicon: Lexicon,
    pub voices: HashMap<String, VoiceConfig>,
}

/// Alias kept for convenience in Rust callers.
pub type Synth = TTS;

impl Default for TTS {
    fn default() -> Self {
        TTS::new(VoiceConfig::default())
    }
}

impl TTS {
    pub fn new(config: VoiceConfig) -> TTS {
        TTS::with_lexicon(config, Lexicon::new())
    }

    pub fn with_lexicon(mut config: VoiceConfig, lexicon: Lexicon) -> TTS {
        config.sanitize();
        TTS {
            config,
            lexicon,
            voices: HashMap::new(),
        }
    }

    /// Registers a named [`VoiceConfig`] (e.g. a dynamically randomized voice) so it can be
    /// referenced by `[name]` in [`TTS::speak_script`] and [`TTS::sing_chorus`].
    pub fn register_voice(&mut self, name: &str, mut voice: VoiceConfig) {
        voice.sanitize();
        let key = name.trim().to_ascii_lowercase().replace('-', "_");
        if !key.is_empty() {
            self.voices.insert(key, voice);
        }
    }

    /// Randomizes `self.config` using `seed` (and optional `gender`), returning the new config.
    pub fn randomize(&mut self, seed: u32, gender: Option<Gender>) -> VoiceConfig {
        let next = VoiceConfig::randomized(seed, gender, None);
        let (volume, folds) = (self.config.volume, self.config.folds);
        self.config = next;
        self.config.volume = volume;
        self.config.folds = folds;
        self.config.clone()
    }

    /// Text analysis only: normalized text, clauses, words and phones.
    pub fn plan(&self, text: &str) -> Plan {
        plan::build_plan(text, &self.lexicon)
    }

    fn prepare(&self, text: &str) -> (Plan, Vec<Frame>, VoiceConfig) {
        let mut cfg = self.config.clone();
        cfg.sanitize();
        let plan = self.plan(text);
        let frames = if plan.is_empty() {
            Vec::new()
        } else {
            frames::rasterize(&timeline::build_timeline(&plan, &cfg), &cfg)
        };
        (plan, frames, cfg)
    }

    /// Renders the whole utterance at once (peak-normalized).
    pub fn speak(&self, text: &str) -> Speech {
        let (plan, frames, cfg) = self.prepare(text);
        let words = word_timings(&plan, &frames);
        let mut renderer = Renderer::new(frames, &cfg);
        let mut samples = renderer.render_all();
        apply_volume(&mut samples, cfg.volume);
        Speech {
            samples,
            sample_rate: SAMPLE_RATE,
            text: plan.normalized,
            ipa: plan.ipa,
            words,
            frames: renderer.into_frames(),
        }
    }

    /// Renders a multi-voice / inline-controlled script (`[female, excited, speed=1.15] ...
    /// [pause=250ms] [deep] ... [sing bpm=110] C4:1=La [/sing]`) into a single utterance
    /// plus per-segment speaker metadata.
    pub fn speak_script(&self, script_text: &str) -> ScriptedSpeech {
        let cues = script::parse_script_with_voices(script_text, &self.config, Some(&self.voices));
        script::synthesize_cues(&cues, &self.config, &self.lexicon)
    }

    /// A sound that is not a word, in this voice: `"laugh"`, `"giggle"`, `"sigh"`, `"cough"`,
    /// `"hmm"`, `"whistle"` ... (the ids of [`VOCAL_SOUNDS`], or another name for one:
    /// [`vocal::find`]). `intensity` (0..1, usually 0.6) is how strongly, `length` (1 = usual)
    /// how long. `None` if there is no such sound.
    ///
    /// In a script the same sounds are tags: `"That is ridiculous! [laugh]"`.
    pub fn vocalize(&self, name: &str, intensity: f64, length: f64) -> Option<Speech> {
        self.vocal_cue(name, intensity, length, None)
    }

    /// Like [`TTS::vocalize`], lasting `seconds` as nearly as the sound allows: a longer laugh
    /// has more calls in it, a longer sigh is drawn out, a hiccup stays a hiccup.
    pub fn vocalize_for(&self, name: &str, intensity: f64, seconds: f64) -> Option<Speech> {
        self.vocal_cue(name, intensity, 1.0, Some(seconds * 1000.0))
    }

    fn vocal_cue(&self, name: &str, intensity: f64, length: f64, duration_ms: Option<f64>) -> Option<Speech> {
        let sound = vocal::find(name)?;
        let cue = ScriptCue::Vocal {
            sound,
            intensity,
            length,
            duration_ms,
            config: Box::new(self.config.clone()),
            voice_label: self.config.gender.as_str().to_string(),
        };
        Some(script::synthesize_cues(&[cue], &self.config, &self.lexicon).speech)
    }

    /// Renders a structured slice of [`ScriptCue`]s.
    pub fn speak_cues(&self, cues: &[ScriptCue]) -> ScriptedSpeech {
        script::synthesize_cues(cues, &self.config, &self.lexicon)
    }

    /// Sings a musical score string (e.g. `"[bpm=108] C4:1=Twin- C4:1=kle G4:1=twin- G4:1=kle"`).
    pub fn sing(&self, score_str: &str) -> Result<Speech, String> {
        let mut score = sing::parse_sing_score_with_voices(
            score_str,
            &self.config,
            Some(&self.lexicon),
            Some(&self.voices),
        )?;
        let cfg = score.voice.take().unwrap_or_else(|| self.config.clone());
        Ok(sing::synthesize_singing(&score, &cfg, &self.lexicon))
    }

    /// Sings a parsed [`SingScore`].
    pub fn sing_score(&self, score: &SingScore) -> Speech {
        sing::synthesize_singing(score, &self.config, &self.lexicon)
    }

    /// Pairs a note sequence (`"C4:1 C4:1 G4:1 G4:1"`) with plain or hyphenated lyrics
    /// (`"Twinkle twinkle"`) at `bpm`, auto-syllabifying multi-syllable words across notes.
    pub fn sing_lyrics(&self, notes: &str, lyrics: &str, bpm: f64) -> Result<Speech, String> {
        let score = sing::pair_notes_and_lyrics(notes, lyrics, bpm, &self.lexicon)?;
        Ok(sing::synthesize_singing(&score, &self.config, &self.lexicon))
    }

    /// Renders a multi-voice chorus string (`[part name="Soprano" voice=bright] ...`) in sync.
    pub fn sing_chorus(&self, chorus_str: &str) -> Result<ScriptedSpeech, String> {
        let chorus = sing::parse_chorus_score_with_voices(
            chorus_str,
            &self.config,
            Some(&self.lexicon),
            Some(&self.voices),
        )?;
        let (speech, segments) = sing::synthesize_chorus(&chorus, &self.config, &self.lexicon);
        Ok(ScriptedSpeech { speech, segments })
    }

    /// Renders a structured [`ChorusScore`] in sync.
    pub fn sing_chorus_score(&self, chorus: &ChorusScore) -> ScriptedSpeech {
        let (speech, segments) = sing::synthesize_chorus(chorus, &self.config, &self.lexicon);
        ScriptedSpeech { speech, segments }
    }

    /// Prepares an utterance for incremental rendering (low latency, any length).
    pub fn stream(&self, text: &str) -> SpeechStream {
        let (plan, frames, cfg) = self.prepare(text);
        let words = word_timings(&plan, &frames);
        let (peak, energy_scale) = stream_level(&frames, &cfg);
        let mut renderer = Renderer::new(frames, &cfg);
        renderer.set_energy_scale(energy_scale);
        SpeechStream {
            renderer,
            pending: Vec::new(),
            pending_pos: 0,
            delivered: 0,
            volume: cfg.volume,
            gain: render::normalization_gain(peak),
            peak,
            text: plan.normalized,
            ipa: plan.ipa,
            words,
        }
    }
}

/// How much of a stream is test-rendered up front to calibrate its loudness (30 s).
const STREAM_CALIBRATION_FRAMES: usize = 6000;

/// Streams cannot be peak-normalized after the fact, so the opening of the utterance (all of
/// it, for anything up to 30 s) is test-rendered once - about 2 ms per second of speech,
/// before the first sample is handed out - and the same gain rule as [`Synth::speak`] is
/// applied. Streamed and one-shot audio therefore come out at the same loudness, and so do
/// the loudness values of their frames. Returns the peak found and the scale for those
/// values. (A later passage that is louder still takes the gain down as it arrives: see
/// [`SpeechStream::read`].)
fn stream_level(frames: &[Frame], cfg: &VoiceConfig) -> (f64, f64) {
    let n = frames.len().min(STREAM_CALIBRATION_FRAMES);
    if n == 0 {
        return (0.001, 1.0);
    }
    let mut renderer = Renderer::new(frames[..n].to_vec(), cfg);
    let mut buf = vec![0.0f32; 64 * SAMPLES_PER_FRAME];
    let mut peak = 0.001f64;
    let mut left = n;
    while left > 0 {
        let chunk = left.min(64);
        let got = renderer.render(chunk, &mut buf);
        for &v in &buf[..got] {
            peak = peak.max((v as f64).abs());
        }
        left -= chunk;
    }
    let loudest = renderer.frames().iter().map(|f| f.rms).fold(0.001f64, f64::max);
    (peak, 1.0 / loudest)
}

/// Incremental renderer for one utterance. Pull audio with [`SpeechStream::read`].
pub struct SpeechStream {
    renderer: Renderer,
    pending: Vec<f32>,
    pending_pos: usize,
    delivered: usize,
    volume: f64,
    /// The gain that brings the loudest passage rendered so far to the level of
    /// [`Synth::speak`], and that passage's peak.
    gain: f64,
    peak: f64,
    pub text: String,
    pub ipa: String,
    pub words: Vec<WordTiming>,
}

impl SpeechStream {
    /// Fills `out` with the next samples; returns how many were written
    /// (less than `out.len()` only at the end of the utterance).
    pub fn read(&mut self, out: &mut [f32]) -> usize {
        let mut written = 0;
        while written < out.len() {
            if self.pending_pos < self.pending.len() {
                let n = (out.len() - written).min(self.pending.len() - self.pending_pos);
                out[written..written + n]
                    .copy_from_slice(&self.pending[self.pending_pos..self.pending_pos + n]);
                self.pending_pos += n;
                written += n;
                continue;
            }
            if self.renderer.is_done() {
                break;
            }
            let want_frames = (out.len() - written).div_ceil(SAMPLES_PER_FRAME).clamp(1, 64);
            self.pending.clear();
            self.pending.resize(want_frames * SAMPLES_PER_FRAME, 0.0);
            let got = self.renderer.render(want_frames, &mut self.pending);
            self.pending.truncate(got);
            self.pending_pos = 0;
            self.level_pending();
            if got == 0 {
                break;
            }
        }
        self.delivered += written;
        written
    }

    /// Brings freshly rendered audio to the stream's level, frame by frame (so that the
    /// result does not depend on how the caller sizes its reads). A frame louder than
    /// anything before it - only possible past the calibrated opening - takes the gain down
    /// over its 5 ms; what still overshoots is rounded off, not clipped.
    fn level_pending(&mut self) {
        for frame in self.pending.chunks_mut(SAMPLES_PER_FRAME) {
            let from = self.gain;
            let peak = frame.iter().fold(0.0f64, |m, &v| m.max((v as f64).abs()));
            if peak > self.peak {
                self.peak = peak;
                self.gain = from.min(render::normalization_gain(peak));
            }
            if self.gain == from {
                apply_volume(frame, self.volume * from);
            } else {
                let step = (self.gain - from) / frame.len() as f64;
                for (i, s) in frame.iter_mut().enumerate() {
                    *s = soft_limit(*s as f64 * self.volume * (from + step * (i + 1) as f64)) as f32;
                }
            }
        }
    }

    /// True once every sample has been read.
    pub fn is_done(&self) -> bool {
        self.renderer.is_done() && self.pending_pos >= self.pending.len()
    }

    /// Total length in seconds (including the short ring-out tail).
    pub fn duration(&self) -> f64 {
        let frames = self.renderer.frames().len();
        if frames == 0 {
            0.0
        } else {
            (frames + 2) as f64 / FRAME_RATE as f64
        }
    }

    /// Seconds of audio handed out so far.
    pub fn position(&self) -> f64 {
        self.delivered as f64 / SAMPLE_RATE as f64
    }

    pub fn frames(&self) -> &[Frame] {
        self.renderer.frames()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PANGRAM: &str = "The quick brown fox jumps over the lazy dog.";

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>() / x.len().max(1) as f64).sqrt()
    }

    /// Average pitch of the voiced part of a signal via autocorrelation.
    fn estimate_f0(x: &[f32]) -> f64 {
        let sr = SAMPLE_RATE as usize;
        let win = 1200; // 50 ms
        let mut votes: Vec<f64> = Vec::new();
        let mut start = 0;
        while start + win * 2 < x.len() {
            let seg = &x[start..start + win * 2];
            let energy: f64 = seg[..win].iter().map(|&v| (v as f64).powi(2)).sum();
            if energy / win as f64 > 0.004 {
                let (mut best_lag, mut best) = (0usize, 0.0f64);
                for lag in (sr / 400)..(sr / 60) {
                    let c: f64 = (0..win).map(|i| seg[i] as f64 * seg[i + lag] as f64).sum();
                    if c > best {
                        best = c;
                        best_lag = lag;
                    }
                }
                if best_lag > 0 && best > 0.3 * energy {
                    votes.push(sr as f64 / best_lag as f64);
                }
            }
            start += win;
        }
        votes.sort_by(|a, b| a.partial_cmp(b).unwrap());
        votes.get(votes.len() / 2).copied().unwrap_or(0.0)
    }

    #[test]
    fn speaks_clean_audio() {
        for cfg in [VoiceConfig::male(), VoiceConfig::female()] {
            let speech = Synth::new(cfg).speak(PANGRAM);
            assert!(speech.duration() > 1.5 && speech.duration() < 6.0, "{}", speech.duration());
            assert!(speech.samples.iter().all(|v| v.is_finite() && v.abs() <= 0.961));
            let peak = speech.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!(peak > 0.5, "peak {peak}");
            assert!(rms(&speech.samples) > 0.03);
            let mean = speech.samples.iter().map(|&v| v as f64).sum::<f64>() / speech.samples.len() as f64;
            assert!(mean.abs() < 0.01, "dc offset {mean}");
            assert_eq!(speech.samples.len(), speech.frames.len() * SAMPLES_PER_FRAME);
        }
    }

    #[test]
    fn male_and_female_pitch() {
        let text = "We are all here now, you know.";
        let male = estimate_f0(&Synth::new(VoiceConfig::male()).speak(text).samples);
        let female = estimate_f0(&Synth::new(VoiceConfig::female()).speak(text).samples);
        assert!((90.0..135.0).contains(&male), "male f0 {male}");
        assert!((155.0..225.0).contains(&female), "female f0 {female}");
    }

    #[test]
    fn deterministic() {
        let synth = Synth::default();
        assert_eq!(synth.speak(PANGRAM).samples, synth.speak(PANGRAM).samples);
    }

    #[test]
    fn empty_and_unpronounceable_text() {
        let synth = Synth::default();
        for text in ["", "   ", "...", "?!"] {
            let speech = synth.speak(text);
            assert!(speech.samples.is_empty(), "{text:?}");
            assert!(speech.words.is_empty());
            let mut stream = synth.stream(text);
            assert_eq!(stream.read(&mut [0.0; 256]), 0);
            assert!(stream.is_done());
        }
    }

    #[test]
    fn word_timings_are_ordered() {
        let speech = Synth::default().speak("Hello there, how are you today?");
        let words: Vec<&str> = speech.words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(words, ["hello", "there", "how", "are", "you", "today"]);
        for pair in speech.words.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        assert!(speech.words.last().unwrap().end <= speech.duration());
        assert_eq!(speech.visemes().len(), speech.frames.len() * VISEME_STRIDE);
    }

    #[test]
    fn stream_matches_any_chunking() {
        let synth = Synth::new(VoiceConfig::female());
        let mut a = synth.stream(PANGRAM);
        let mut b = synth.stream(PANGRAM);
        let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
        let mut buf = [0.0f32; 4096];
        loop {
            let n = a.read(&mut buf[..4096]);
            out_a.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        loop {
            let n = b.read(&mut buf[..128]);
            out_b.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        assert!(a.is_done() && b.is_done());
        assert_eq!(out_a, out_b);
        assert_eq!(out_a.len(), (a.frames().len() + 2) * SAMPLES_PER_FRAME);
        assert!((a.duration() - out_a.len() as f64 / SAMPLE_RATE as f64).abs() < 1e-9);
        assert!(out_a.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }

    #[test]
    fn stream_is_as_loud_as_one_shot() {
        for cfg in [VoiceConfig::male(), VoiceConfig::female()] {
            let synth = Synth::new(cfg);
            let offline = synth.speak(PANGRAM);
            let mut stream = synth.stream(PANGRAM);
            let mut streamed = vec![0.0f32; offline.samples.len() + 4 * SAMPLES_PER_FRAME];
            let n = stream.read(&mut streamed);
            streamed.truncate(n);
            let peak = |x: &[f32]| x.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            assert!((peak(&streamed) - peak(&offline.samples)).abs() < 0.01);
            assert!((rms(&streamed[..offline.samples.len()]) / rms(&offline.samples) - 1.0).abs() < 0.02);
        }
    }

    #[test]
    fn stream_frames_are_as_loud_as_one_shot_frames() {
        // the per-frame loudness (lip-sync energy) must not depend on which API made it
        let synth = Synth::new(VoiceConfig::female());
        let offline = synth.speak(PANGRAM);
        let mut stream = synth.stream(PANGRAM);
        let mut buf = vec![0.0f32; offline.samples.len() + 4 * SAMPLES_PER_FRAME];
        stream.read(&mut buf);
        let loudest = |frames: &[Frame]| frames.iter().map(|f| f.rms).fold(0.0f64, f64::max);
        assert!((loudest(&offline.frames) - 1.0).abs() < 1e-9);
        assert!((loudest(stream.frames()) - 1.0).abs() < 1e-6, "{}", loudest(stream.frames()));
        for (a, b) in offline.frames.iter().zip(stream.frames()) {
            assert!((a.rms - b.rms).abs() < 1e-6);
        }
    }

    #[test]
    fn a_long_stream_is_rounded_off_not_clipped() {
        // quiet opening, loud ending: the level is set from the first 30 seconds only
        let text = "Shh. Psst. ".repeat(45) + "Hello there, how are you today? We are all here now, you know.";
        let mut stream = Synth::new(VoiceConfig::male()).stream(&text);
        let mut all = Vec::new();
        let mut buf = vec![0.0f32; 4096];
        loop {
            let n = stream.read(&mut buf);
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
        }
        let peak = all.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        // (rounded off by the limiter, well short of full scale)
        assert!(peak > 0.5 && peak < 0.99, "peak {peak}");
        // once the gain has come down, the stream is at the level the one-shot rendering has
        let offline = Synth::new(VoiceConfig::male()).speak(&text);
        let tail_peak = |x: &[f32]| x[x.len() - 3 * SAMPLE_RATE as usize..].iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!((tail_peak(&all) - tail_peak(&offline.samples)).abs() < 0.02, "{} {}", tail_peak(&all), tail_peak(&offline.samples));
        // and how the caller reads does not change it
        let mut again = Synth::new(VoiceConfig::male()).stream(&text);
        let mut other = Vec::new();
        let mut small = vec![0.0f32; 333];
        loop {
            let n = again.read(&mut small);
            if n == 0 {
                break;
            }
            other.extend_from_slice(&small[..n]);
        }
        assert!(all == other);
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.9), -0.9);
        assert!(soft_limit(3.0) <= 1.0 && soft_limit(1.0) > 0.97 && soft_limit(1.0) < 1.0);
    }

    #[test]
    fn odd_input_and_old_names() {
        // a chain of clitics is not a word, and must not exhaust the stack
        let mut synth = Synth::new(VoiceConfig::male());
        let chain = format!("a{}", "'s".repeat(4000));
        assert!(!synth.speak(&chain).samples.is_empty());
        // a taught word may hold digits
        synth.lexicon.add_word("mp3", "EH2 M P IY2 TH R IY1").unwrap();
        assert!(synth.speak("an mp3 file").ipa.contains("ɛmpiːθɹiː"), "{}", synth.speak("an mp3 file").ipa);
        // the earlier, longer preset list still resolves, and every preset is its plain voice changed
        for (old, now) in crate::config::PRESET_ALIASES {
            assert_eq!(VoiceConfig::preset(old), VoiceConfig::preset(now), "{old}");
        }
        assert_eq!(VoiceConfig::preset("male"), Some(VoiceConfig::male()));
        assert_eq!(VoiceConfig::preset("female"), Some(VoiceConfig::female()));
        assert!(VoiceConfig::preset("nobody").is_none());
        // every numeric setting is reachable by its JavaScript name
        let mut cfg = VoiceConfig::male();
        for name in crate::config::NUMBER_SETTINGS {
            *cfg.number_mut(name).unwrap() = 0.5;
        }
        assert!(cfg.number_mut("gender").is_none());
    }

    #[test]
    fn simulated_folds_speak_at_the_pitch_asked_for() {
        use crate::folds::{Drive, Folds, Larynx, Voicing};
        for (folds, pitch, pressure) in [(Folds::MALE, 110.0, 600.0), (Folds::FEMALE, 208.0, 800.0)] {
            let usual = Drive { tension: 1.0, pressure, gap: 0.12e-3, zip: 0.12e-3, peel: 0.0, leak: 1e-6 };
            let larynx = Larynx::new(folds, usual, usual, SAMPLE_RATE as f64, pitch);
            for semitones in [-6.0, 0.0, 6.0] {
                let asked = pitch * 2f64.powf(semitones / 12.0);
                let mut v = Voicing::default();
                larynx.start(&mut v, asked, pressure, usual.gap, 0.02);
                let (mut cycles, mut first, mut last, mut power) = (0, 0, 0, 0.0);
                for i in 0..SAMPLE_RATE as usize {
                    let (rate, flow, began) = larynx.tick(&mut v, asked, pressure, usual.gap, 1.0);
                    assert!(rate.is_finite() && (0.0..=1.5).contains(&flow));
                    power += rate * rate;
                    if began && i > 4000 {
                        if cycles == 0 {
                            first = i;
                        }
                        last = i;
                        cycles += 1;
                    }
                }
                let got = (cycles - 1) as f64 * SAMPLE_RATE as f64 / (last - first) as f64;
                assert!((got / asked - 1.0).abs() < 0.01, "asked {asked:.1} Hz, got {got:.1}");
                assert!(power / SAMPLE_RATE as f64 > 0.05, "the folds must sound at {asked:.1} Hz");
            }
            // no pressure, no voice
            let mut v = Voicing::default();
            let quiet: f64 = (0..4800).map(|_| larynx.tick(&mut v, pitch, 0.0, usual.gap, 1.0).0.abs()).sum();
            assert!(quiet < 1e-6);
        }
    }

    #[test]
    fn simulated_folds_make_speech_and_stream_identically() {
        for mut cfg in [VoiceConfig::male(), VoiceConfig::female()] {
            cfg.folds = true;
            let synth = Synth::new(cfg.clone());
            let speech = synth.speak(PANGRAM);
            assert!(speech.samples.iter().all(|v| v.is_finite()));
            assert!(rms(&speech.samples) > 0.02);
            // the pitch is the one the same voice has with the equation pulse
            cfg.folds = false;
            let plain = Synth::new(cfg).speak(PANGRAM);
            let (got, usual) = (estimate_f0(&speech.samples), estimate_f0(&plain.samples));
            assert!((got / usual - 1.0).abs() < 0.08, "{got} {usual}");
            // chunked rendering gives the same samples as one pass
            let mut stream = synth.stream(PANGRAM);
            let (mut a, mut b) = (Vec::new(), Vec::new());
            let mut buf = vec![0.0f32; 777];
            loop {
                let n = stream.read(&mut buf);
                if n == 0 {
                    break;
                }
                a.extend_from_slice(&buf[..n]);
            }
            let mut stream = synth.stream(PANGRAM);
            let mut buf = vec![0.0f32; 5000];
            loop {
                let n = stream.read(&mut buf);
                if n == 0 {
                    break;
                }
                b.extend_from_slice(&buf[..n]);
            }
            assert!(a == b);
        }
    }

    #[test]
    fn whisper_has_no_pitch() {
        let mut cfg = VoiceConfig::preset("whisper").unwrap();
        cfg.sanitize();
        let speech = Synth::new(cfg).speak("We are all here now, you know.");
        assert!(rms(&speech.samples) > 0.01);
        assert!(speech.frames.iter().all(|f| f.av == 0.0));
    }

    #[test]
    fn every_preset_and_extreme_settings_stay_finite() {
        for preset in PRESETS.iter() {
            let speech = Synth::new(preset.config()).speak("Testing one, two, three.");
            assert!(speech.samples.iter().all(|v| v.is_finite()), "{}", preset.id);
            assert!(rms(&speech.samples) > 0.01, "{}", preset.id);
        }
        let mut wild = VoiceConfig::male();
        wild.pitch_hz = 1e9;
        wild.tract_length_cm = -5.0;
        wild.speed = f64::NAN;
        wild.roughness = 99.0;
        wild.breathiness = 99.0;
        wild.expressiveness = 99.0;
        let speech = Synth::new(wild).speak("Still fine? Yes!");
        assert!(speech.samples.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn speed_changes_duration() {
        let mut fast = VoiceConfig::male();
        fast.speed = 1.5;
        let mut slow = VoiceConfig::male();
        slow.speed = 0.7;
        let d_fast = Synth::new(fast).speak(PANGRAM).duration();
        let d_slow = Synth::new(slow).speak(PANGRAM).duration();
        assert!(d_slow > d_fast * 1.5, "{d_slow} vs {d_fast}");
    }

    // ---- articulation and intonation behaviours that make the voice sound human ----------

    use crate::phone::Ph;

    fn mean(values: impl Iterator<Item = f64>) -> f64 {
        let v: Vec<f64> = values.collect();
        v.iter().sum::<f64>() / v.len().max(1) as f64
    }

    fn semitones(a: f64, b: f64) -> f64 {
        12.0 * (a / b).log2()
    }

    #[test]
    fn diphthongs_glide_smoothly() {
        let speech = Synth::default().speak("I");
        let f2: Vec<f64> = speech.frames.iter().filter(|f| f.phone == Ph::AY).map(|f| f.f2).collect();
        assert!(f2.len() > 15);
        assert!(f2[f2.len() - 1] - f2[0] > 400.0, "F2 should rise through /ai/: {f2:?}");
        // one continuous movement, not a jump between two steady vowels
        let biggest_step = f2.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max);
        assert!(biggest_step < 60.0, "largest 5 ms step {biggest_step} Hz");
    }

    #[test]
    fn vowels_bend_toward_the_next_consonant() {
        // The last part of the vowel in "at" must already be closing toward the /t/.
        let speech = Synth::default().speak("at");
        let f1: Vec<f64> = speech.frames.iter().filter(|f| f.phone == Ph::AE).map(|f| f.f1).collect();
        let mid = f1[f1.len() / 2];
        let last = f1[f1.len() - 1];
        assert!(mid - last > 80.0, "F1 mid {mid} -> end {last}");
    }

    #[test]
    fn voiced_stops_hum_and_voiceless_stops_are_silent() {
        let closure_level = |text: &str, ph: Ph| {
            // without the room, whose tail would fill the closure
            let mut cfg = VoiceConfig::male();
            cfg.room = 0.0;
            let speech = Synth::new(cfg).speak(text);
            let frames: Vec<&Frame> = speech.frames.iter().filter(|f| f.phone == ph && f.is_closure).collect();
            // skip the first frames, where the previous vowel is still dying away
            mean(frames.iter().skip(4).map(|f| f.rms))
        };
        let voiced = closure_level("a bad day", Ph::B);
        let voiceless = closure_level("a pad day", Ph::P);
        assert!(voiced > 0.02, "voice bar level {voiced}");
        assert!(voiceless < voiced * 0.4, "voiceless closure {voiceless} vs voiced {voiced}");
    }

    #[test]
    fn nasals_are_murmurs_with_real_energy() {
        for (text, ph) in [("mama", Ph::M), ("nana", Ph::N), ("singing", Ph::NG)] {
            let speech = Synth::default().speak(text);
            let nasal = mean(speech.frames.iter().filter(|f| f.phone == ph).map(|f| f.rms));
            let vowel = mean(speech.frames.iter().filter(|f| f.phone.is_vowel()).map(|f| f.rms));
            let ratio = nasal / vowel;
            assert!((0.2..0.95).contains(&ratio), "{text}: nasal/vowel level {ratio}");
        }
    }

    #[test]
    fn pitch_moves_like_speech() {
        for cfg in [VoiceConfig::male(), VoiceConfig::female()] {
            let base = cfg.pitch_hz;
            let speech = Synth::new(cfg).speak("The quick brown fox jumps over the lazy dog, and nobody seems to mind.");
            let voiced: Vec<f64> = speech.frames.iter().filter(|f| f.av > 0.4).map(|f| f.f0).collect();
            let st: Vec<f64> = voiced.iter().map(|&f| semitones(f, base)).collect();
            let m = mean(st.iter().copied());
            let sd = mean(st.iter().map(|v| (v - m).powi(2))).sqrt();
            assert!((1.5..3.5).contains(&sd), "pitch standard deviation {sd} semitones");
            // a statement ends near the bottom of the range
            let end = semitones(voiced[voiced.len() - 1], base);
            assert!(end < -3.0, "final pitch {end} st re the speaker's average");
        }
    }

    /// Mean duration (s) of the frames carrying `ph`, per occurrence, in `text`.
    fn phone_seconds(speech: &Speech, ph: Ph) -> f64 {
        speech.frames.iter().filter(|f| f.phone == ph).count() as f64 / FRAME_RATE as f64
    }

    #[test]
    fn timing_follows_natural_speech() {
        let mut cfg = VoiceConfig::male();
        cfg.variation = 0.0;
        let synth = Synth::new(cfg);

        // function words rush by, content words take their time
        let speech = synth.speak("The cat sat on the mat today.");
        let dur = |i: usize| speech.words[i].end - speech.words[i].start;
        assert!(dur(0) < 0.6 * dur(1), "the {} vs cat {}", dur(0), dur(1));
        assert!(dur(4) < 0.6 * dur(5), "the {} vs mat {}", dur(4), dur(5));

        // the last syllable before a pause is stretched
        let mid = synth.speak("A cat sat there.");
        let end = synth.speak("There sat a cat.");
        let ae_mid = phone_seconds(&mid, Ph::AE) / 2.0; // cat + sat
        let ae_end = phone_seconds(&end, Ph::AE) / 2.0;
        assert!(ae_end > ae_mid * 1.15, "phrase-final lengthening {ae_end} vs {ae_mid}");

        // consonants get real time: /s/ is not a click, short vowels are short
        let s = phone_seconds(&synth.speak("a sister"), Ph::S) / 2.0;
        assert!((0.06..0.16).contains(&s), "/s/ lasts {s}");
        let ih = phone_seconds(&synth.speak("a sister"), Ph::IH);
        assert!((0.03..0.10).contains(&ih), "/ih/ lasts {ih}");
    }

    #[test]
    fn the_voice_marks_the_words_that_carry_meaning() {
        let mut cfg = VoiceConfig::male();
        cfg.variation = 0.0;
        let base = cfg.pitch_hz;
        let synth = Synth::new(cfg);
        // highest pitch reached inside each word, in semitones re the voice's own pitch
        let peaks = |text: &str| -> Vec<(String, f64)> {
            let speech = synth.speak(text);
            speech
                .words
                .iter()
                .map(|w| {
                    let top = speech
                        .frames
                        .iter()
                        .filter(|f| f.t_sec >= w.start && f.t_sec < w.end && f.av > 0.4)
                        .map(|f| semitones(f.f0, base))
                        .fold(f64::NEG_INFINITY, f64::max);
                    (w.word.clone(), top)
                })
                .collect()
        };
        // "I saw him": the main accent is on the verb, and the pronoun after it is passed over
        let p = peaks("We really saw him.");
        let (saw, him) = (p[2].1, p[3].1);
        assert!(saw > him + 2.0, "saw {saw} him {him}");
        // a pronoun and an auxiliary are hurried through like "the", not dwelt on like a noun
        let speech = synth.speak("The cat would sit on the mat today.");
        let dur = |i: usize| speech.words[i].end - speech.words[i].start;
        assert!(dur(2) < 0.75 * dur(1), "would {} vs cat {}", dur(2), dur(1));
        let speech = synth.speak("He sat there and Tom sat there.");
        let dur = |i: usize| speech.words[i].end - speech.words[i].start;
        assert!(dur(0) < 0.8 * dur(4), "he {} vs tom {}", dur(0), dur(4));
    }

    #[test]
    fn tone_controls_shape_the_voice_without_touching_timing() {
        // Energy of the sample-to-sample differences relative to the signal: grows with treble.
        fn treble(x: &[f32]) -> f64 {
            let d: f64 = x.windows(2).map(|w| ((w[1] - w[0]) as f64).powi(2)).sum();
            let e: f64 = x.iter().map(|&v| (v as f64) * (v as f64)).sum();
            d / e.max(1e-12)
        }
        let plain = Synth::new(VoiceConfig::male()).speak(PANGRAM);
        let mut cfg = VoiceConfig::male();
        cfg.presence = 9.0;
        let present = Synth::new(cfg.clone()).speak(PANGRAM);
        assert_eq!(present.samples.len(), plain.samples.len());
        assert!(treble(&present.samples) > treble(&plain.samples) * 1.15);
        cfg.presence = 0.0;
        cfg.bass = 9.0;
        let bassy = Synth::new(cfg.clone()).speak(PANGRAM);
        assert!(treble(&bassy.samples) < treble(&plain.samples) * 0.8);
        // A flat curve is exactly the plain voice.
        cfg.bass = 0.0;
        assert!(Synth::new(cfg.clone()).speak(PANGRAM).samples == plain.samples);
        // Extreme curves stay clean.
        cfg.tone = [18.0, -18.0, 18.0, -18.0, 18.0, -18.0, 18.0, -18.0, 18.0, -18.0];
        let wild = Synth::new(cfg).speak(PANGRAM);
        assert!(wild.samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
    }

    #[test]
    fn room_fills_the_pauses_and_streams_identically() {
        let text = "One. Two.";
        let levels = |room: f64| -> Vec<f64> {
            let mut cfg = VoiceConfig::male();
            cfg.room = room;
            let speech = Synth::new(cfg).speak(text);
            // 50 ms at a time
            speech.samples.chunks(SAMPLE_RATE as usize / 20).map(rms).collect()
        };
        let (dry, roomy) = (levels(0.0), levels(0.5));
        // the first 50 ms of the pause: silent when dry, a faint tail with the room
        let first_loud = dry.iter().position(|&v| v > 0.01).expect("speech");
        let pause = first_loud + dry[first_loud..].iter().position(|&v| v < 1e-4).expect("a dry pause is silent");
        assert!(roomy[pause] > 1e-5 && roomy[pause] < 0.02, "the room leaves a faint tail in the pause: {}", roomy[pause]);
        // pitch coupling changes the sound, not the timing
        let mut cfg = VoiceConfig::male();
        cfg.pitch_coupling = 1.0;
        let coupled = Synth::new(cfg).speak(PANGRAM);
        let plain = Synth::new(VoiceConfig::male()).speak(PANGRAM);
        assert_eq!(coupled.samples.len(), plain.samples.len());
        assert!(coupled.samples != plain.samples);
    }

    #[test]
    fn air_adds_a_top_band_to_the_vowels_only() {
        // level above 7 kHz relative to the whole signal, of a sustained vowel
        let top_share = |air: f64, text: &str| {
            let mut cfg = VoiceConfig::male();
            cfg.air = air;
            cfg.room = 0.0;
            let x = Synth::new(cfg).speak(text).samples;
            // fourth-order high-pass at 7 kHz
            let w0 = std::f64::consts::TAU * 7000.0 / SAMPLE_RATE as f64;
            let mut y: Vec<f64> = x.iter().map(|&v| v as f64).collect();
            for q in [0.5412, 1.3066] {
                let alpha = w0.sin() / (2.0 * q);
                let (b0, a1, a2) = ((1.0 + w0.cos()) / 2.0 / (1.0 + alpha), -2.0 * w0.cos() / (1.0 + alpha), (1.0 - alpha) / (1.0 + alpha));
                let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
                for v in y.iter_mut() {
                    let out = b0 * (*v - 2.0 * x1 + x2) - a1 * y1 - a2 * y2;
                    (x2, x1, y2, y1) = (x1, *v, y1, out);
                    *v = out;
                }
            }
            (y.iter().map(|v| v * v).sum::<f64>() / x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>()).sqrt()
        };
        let none = top_share(0.0, "Aah.");
        let half = top_share(0.7, "Aah.");
        let full = top_share(1.0, "Aah.");
        assert!(half > none * 1.3 && full > half * 1.5, "air brightens the vowel: {none} {half} {full}");
        // timing is untouched, and so is streaming
        let mut cfg = VoiceConfig::male();
        cfg.air = 1.0;
        let airy = Synth::new(cfg.clone()).speak(PANGRAM);
        let plain = Synth::new(VoiceConfig::male()).speak(PANGRAM);
        assert_eq!(airy.samples.len(), plain.samples.len());
        let synth = Synth::new(cfg);
        let (mut big, mut small) = (synth.stream(PANGRAM), synth.stream(PANGRAM));
        let (mut out_big, mut out_small) = (Vec::new(), Vec::new());
        let mut buf = [0.0f32; 4096];
        loop {
            let n = big.read(&mut buf[..4096]);
            out_big.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        loop {
            let n = small.read(&mut buf[..100]);
            out_small.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        assert_eq!(out_big, out_small, "the air branch streams identically in any chunking");
    }

    #[test]
    fn closure_sets_the_tone_at_the_source() {
        // sharper closing of the folds = stronger upper harmonics, same timing
        let bright = |closure: f64| {
            let mut cfg = VoiceConfig::male();
            cfg.closure = closure;
            cfg.room = 0.0;
            let x = Synth::new(cfg).speak("Aah.").samples;
            let d: f64 = x.windows(2).map(|w| ((w[1] - w[0]) as f64).powi(2)).sum();
            (d / x.iter().map(|&v| (v as f64) * (v as f64)).sum::<f64>(), x.len())
        };
        let (soft, n_soft) = bright(0.0);
        let (mid, _) = bright(0.5);
        let (sharp, n_sharp) = bright(1.0);
        assert!(soft < mid && mid < sharp, "closure brightens the voice: {soft} {mid} {sharp}");
        assert_eq!(n_soft, n_sharp);
        assert_eq!(VoiceConfig::male().closure, crate::config::MALE_CLOSURE);
        assert_eq!(VoiceConfig::female().closure, crate::config::FEMALE_CLOSURE);
    }

    #[test]
    fn voice_source_and_airway_settings() {
        let plain = Synth::new(VoiceConfig::male()).speak(PANGRAM);
        // A pressed voice: the same words and timing from a different basic sound.
        let mut cfg = VoiceConfig::male();
        cfg.open_quotient = 0.54;
        let pressed = Synth::new(cfg.clone()).speak(PANGRAM);
        assert_eq!(pressed.samples.len(), plain.samples.len());
        assert!(pressed.samples != plain.samples);
        // The airway below the glottis changes the sound too, stays clean, and streams
        // identically whatever the chunk size.
        cfg.open_quotient = VoiceConfig::male().open_quotient;
        cfg.subglottal = 1.0;
        let synth = Synth::new(cfg);
        let loaded = synth.speak(PANGRAM);
        assert!(loaded.samples != plain.samples);
        assert!(loaded.samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        let (mut a, mut b) = (synth.stream(PANGRAM), synth.stream(PANGRAM));
        let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
        let mut buf = [0.0f32; 4096];
        loop {
            let n = a.read(&mut buf[..4096]);
            out_a.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        loop {
            let n = b.read(&mut buf[..100]);
            out_b.extend_from_slice(&buf[..n]);
            if n == 0 {
                break;
            }
        }
        assert_eq!(out_a, out_b);
    }

    #[test]
    fn formant_vowel_pause_and_breath_settings() {
        let mid_f2 = |cfg: VoiceConfig, text: &str, ph: Ph| {
            let speech = Synth::new(cfg).speak(text);
            let f2: Vec<f64> = speech.frames.iter().filter(|f| f.phone == ph).map(|f| f.f2).collect();
            f2[f2.len() / 2]
        };
        let base_uw = mid_f2(VoiceConfig::male(), "who", Ph::UW);
        let base_iy = mid_f2(VoiceConfig::male(), "he", Ph::IY);
        // One vowel moved: only that vowel changes.
        let mut cfg = VoiceConfig::male();
        cfg.vowels[crate::config::vowel_index(Ph::UW).unwrap()] = [1.0, 1.25, 1.0];
        assert!(mid_f2(cfg.clone(), "who", Ph::UW) > base_uw * 1.15);
        assert!((mid_f2(cfg, "he", Ph::IY) - base_iy).abs() < 1.0);
        // A formant of the whole voice moved.
        let mut cfg = VoiceConfig::male();
        cfg.formants[1] = 1.1;
        assert!(mid_f2(cfg, "he", Ph::IY) > base_iy * 1.07);

        let text = "We see the moon. Then we go home.";
        let secs = |cfg: VoiceConfig| Synth::new(cfg).speak(text).duration();
        let normal = secs(VoiceConfig::male());
        let mut cfg = VoiceConfig::male();
        cfg.pauses = 2.5;
        assert!(secs(cfg) > normal + 0.35);
        let mut cfg = VoiceConfig::male();
        cfg.breaths = 1.0;
        assert!(secs(cfg) > normal + 0.25);
        let mut cfg = VoiceConfig::male();
        cfg.breaths = 0.0;
        assert!(secs(cfg) < normal - 0.03);
    }

    #[test]
    fn variation_is_seeded_and_gentle() {
        let text = "The quick brown fox jumps over the lazy dog, and nobody seems to mind.";
        let take = |variation: f64, seed: u32| {
            let mut cfg = VoiceConfig::male();
            cfg.variation = variation;
            cfg.seed = seed;
            Synth::new(cfg).speak(text)
        };
        // same seed: identical; another seed: a different reading of the same words
        assert_eq!(take(0.5, 7).samples, take(0.5, 7).samples);
        let (a, b) = (take(0.5, 1), take(0.5, 2));
        assert_ne!(a.samples, b.samples);
        assert_eq!(a.words.len(), b.words.len());
        // with variation off the seed changes nothing
        assert_eq!(take(0.0, 1).samples, take(0.0, 2).samples);
        // the unevenness stays human-sized: the overall pace moves by a few percent at the
        // default setting, and stays recognisably the same pace even at the maximum
        let regular = take(0.0, 0).duration();
        for seed in 0..12 {
            let d = take(0.5, seed).duration();
            assert!((d / regular - 1.0).abs() < 0.12, "seed {seed}: {d} vs {regular}");
            let d = take(1.0, seed).duration();
            assert!((d / regular - 1.0).abs() < 0.25, "seed {seed} at maximum: {d} vs {regular}");
        }
        // different sentences do not all get the same random pattern
        let other = Synth::default().speak("Completely different words go here today.");
        assert!(other.duration() > 1.0);
    }

    #[test]
    fn questions_rise_and_statements_fall() {
        let last_pitch = |text: &str| {
            let cfg = VoiceConfig::male();
            let base = cfg.pitch_hz;
            let speech = Synth::new(cfg).speak(text);
            let f0 = speech.frames.iter().filter(|f| f.av > 0.4).map(|f| f.f0).next_back().unwrap();
            semitones(f0, base)
        };
        assert!(last_pitch("Is it ready?") > 2.0);
        assert!(last_pitch("Isn't that interesting?") > 2.0);
        assert!(last_pitch("It is ready.") < -2.0);

        // Rising questions keep clean voicing at the end instead of dropping into vocal fry.
        let q = Synth::new(VoiceConfig::female()).speak("Isn't that interesting?");
        assert!(q.frames.iter().all(|f| f.creak == 0.0));
        let s = Synth::new(VoiceConfig::female()).speak("That is interesting.");
        assert!(s.frames.iter().any(|f| f.creak > 0.2));
    }

    #[test]
    fn alveolar_taps_and_coda_velars_coarticulate() {
        let mut cfg = VoiceConfig::male();
        cfg.variation = 0.0;
        let synth = Synth::new(cfg);

        // Word-internal /t/ before unstressed -er ("water") is a short voiced tap, unlike "top".
        let water = synth.speak("water");
        let tap_voiced = mean(water.frames.iter().filter(|f| f.phone == Ph::T).map(|f| f.av));
        let top = synth.speak("a top");
        let stop_voiced = mean(top.frames.iter().filter(|f| f.phone == Ph::T).map(|f| f.av));
        assert!(tap_voiced > 0.3 && stop_voiced < 0.1, "tap {tap_voiced} vs stop {stop_voiced}");

        // Coda /k/ after front /IH/ ("kick the") closes at a front palatal locus rather than jumping
        // across /DH/ to the neutral vowel of "the".
        let kick = synth.speak("kick the");
        let ih_f2: Vec<f64> = kick.frames.iter().filter(|f| f.phone == Ph::IH).map(|f| f.f2).collect();
        assert!(ih_f2[ih_f2.len() - 1] > 1950.0, "coda /k/ keeps front F2: {ih_f2:?}");

        // The /l/ before a vowel ("glue") is a little higher in F2 than the coda [ɫ]
        // ("bowls"), and both are dark, as American speakers have them (not the clear [l]
        // of 1400 Hz).
        let glue = synth.speak("glue");
        let bowls = synth.speak("bowls");
        let ons_l_f2 = mean(glue.frames.iter().filter(|f| f.phone == Ph::L).map(|f| f.f2));
        let coda_l_f2 = mean(bowls.frames.iter().filter(|f| f.phone == Ph::L).map(|f| f.f2));
        assert!(ons_l_f2 > coda_l_f2 + 50.0 && ons_l_f2 < 1250.0, "onset [l] {ons_l_f2} vs coda [ɫ] {coda_l_f2}");
    }

    #[test]
    fn long_text_is_not_truncated() {
        let text = "This is a fairly ordinary sentence about nothing in particular. ".repeat(40);
        let speech = Synth::default().speak(&text);
        assert_eq!(speech.words.len(), 400);
        assert!(speech.duration() > 60.0);
    }

    #[test]
    fn custom_words_and_dictionaries() {
        let mut synth = Synth::default();
        synth.lexicon.add_word("zyxgrb", "Z IH1 K S G R AX B").unwrap();
        assert_eq!(synth.plan("zyxgrb").arpabet(), "Z IH1 K S G R AX0 B");
        // taught words beat the built-in lexicon and the normalizer's own rewrites
        assert_eq!(synth.plan("SoftN").arpabet(), "S AO1 F T | EH1 N");
        synth.lexicon.add_word("SoftN", "S AO1 F T EH1 N").unwrap();
        assert_eq!(synth.plan("SoftN").arpabet(), "S AO1 F T EH1 N");
        synth.lexicon.add_word("hello", "HH AX L OW1").unwrap();
        assert_eq!(synth.plan("hello").arpabet(), "HH AX0 L OW1");
        assert!(synth.lexicon.add_word("bad word", "B").is_err());
        assert!(synth.lexicon.add_word("ok", "QQ").is_err());
        let loaded = synth
            .lexicon
            .load_dictionary(";;; comment\nQWERTZ  K W ER1 T S\nqwertz(2) K W ER1 Z\nbanana B AH0 N AE1 N AH0\n");
        assert_eq!(loaded, 2);
        assert_eq!(synth.plan("qwertz").arpabet(), "K W ER1 T S");
    }

    #[test]
    fn scripted_dialogue_switches_voices_and_controls() {
        let synth = Synth::default();
        let res = synth.speak_script(
            "[male, speed=0.95] Hello there! [pause=200ms] \
             [female, excited, speed=1.18] Hi! Look at item [12] — isn't this fast? \
             [whisper, slow] Keep it secret. \
             [soprano, sing bpm=120] C5:1=La G4:1.5=la [/sing]",
        );
        assert_eq!(res.segments.len(), 4);
        assert_eq!(res.segments[0].gender, Gender::Male);
        assert!(!res.segments[0].singing);
        assert_eq!(res.segments[1].gender, Gender::Female);
        assert!(res.speech.text.contains("twelve"), "literal [12] stays spoken text: {}", res.speech.text);
        assert_eq!(res.segments[3].gender, Gender::Female);
        assert!(res.segments[3].singing);
        assert!(res.speech.duration() > 3.0);
        assert!(res.speech.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        // `[woman_a, ...]` followed by styles, breaths, pauses, whisper, and `[woman_a, reset]`
        // (or `[reset]`) keeps the female speaker throughout instead of reverting to male base_cfg.
        let arc = synth.speak_script(
            "[woman_a, calm, speed=0.86] We can start slowly, warmly, and gently, taking our time on every word. \
             [inhale] [excited, speed=1.22, pitch=+2st] Then suddenly speed right up with bright energy when something thrilling happens! \
             [pause=280ms] [whisper, slow] Or drop to an intimate whisper... \
             [woman_a, reset] before returning smoothly to normal.",
        );
        assert_eq!(arc.segments.len(), 4);
        for (idx, seg) in arc.segments.iter().enumerate() {
            assert_eq!(seg.gender, Gender::Female, "segment {idx} gender");
            assert_eq!(seg.voice, "woman_a", "segment {idx} voice label");
            assert!((seg.tract_length_cm - 14.95).abs() < 1e-6, "segment {idx} tract");
        }
        assert!((arc.segments[3].pitch_hz - 186.0).abs() < 1e-6, "reset restores clean woman_a pitch");
        let seg2_voiced = arc
            .speech
            .frames
            .iter()
            .filter(|f| f.t_sec >= arc.segments[2].start + 0.05 && f.t_sec < arc.segments[2].end - 0.05)
            .any(|f| f.av > 0.1);
        let seg3_voiced = arc
            .speech
            .frames
            .iter()
            .filter(|f| f.t_sec >= arc.segments[3].start + 0.05 && f.t_sec < arc.segments[3].end - 0.05)
            .any(|f| f.av > 0.5);
        assert!(!seg2_voiced, "whisper segment is unvoiced");
        assert!(seg3_voiced, "[woman_a, reset] restores modal voicing");
    }

    #[test]
    fn scripts_laugh_sigh_and_cough() {
        let synth = Synth::default();
        let plain = synth.speak_script("[female] That is ridiculous!");
        let res = synth.speak_script("[female] That is ridiculous! [laugh] [deep] Hmm. [sigh] [cough=strong] Quite.");
        // the sounds take their place among the words and the segments, each by its speaker
        let words: Vec<&str> = res.speech.words.iter().map(|w| w.word.as_str()).collect();
        assert_eq!(words, ["that", "is", "ridiculous", "[laugh]", "hmm", "[sigh]", "[cough]", "quite"]);
        let sounds: Vec<Option<&str>> = res.segments.iter().map(|s| s.sound).collect();
        assert_eq!(sounds, [None, Some("laugh"), None, Some("sigh"), Some("cough"), None]);
        assert_eq!(res.segments[1].gender, Gender::Female);
        assert_eq!(res.segments[1].voice, "female");
        assert_eq!(res.segments[3].voice, "deep");
        assert!(res.speech.duration() > plain.speech.duration() + 1.5);
        assert!(res.speech.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        for pair in res.speech.words.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        // the laugh is the woman's: its calls are far above the deep voice that follows
        let laugh = &res.segments[1];
        let top = res
            .speech
            .frames
            .iter()
            .filter(|f| f.t_sec >= laugh.start && f.t_sec < laugh.end && f.av > 0.3)
            .map(|f| f.f0)
            .fold(0.0, f64::max);
        assert!(top > 330.0, "the laugh reaches {top} Hz");

        // how strongly and how long: `name=`, or `intensity=` / `length=` after it
        let secs = |script: &str| synth.speak_script(script).speech.duration();
        assert!(secs("[laugh length=2]") > secs("[laugh]") * 1.4);
        assert!(secs("[laugh=long]") > secs("[laugh]") * 1.2);
        assert!(secs("[giggle short]") < secs("[giggle]"));
        let level = |script: &str| rms(&synth.speak_script(&format!("We are all here. {script}")).speech.samples);
        assert!(level("[laugh intensity=0.1]") < level("[laugh intensity=1]"));
        // other names for the same sounds; what is no tag at all stays text
        assert_eq!(synth.speak_script("[haha]").segments[0].sound, Some("laugh"));
        assert_eq!(synth.speak_script("[uh-huh]").segments[0].sound, Some("uh-huh"));
        let literal = synth.speak_script("see note [laughable] here");
        assert!(literal.segments.iter().all(|s| s.sound.is_none()));
        // a voice registered under a sound's name is still that voice
        let mut named = Synth::default();
        named.register_voice("giggle", VoiceConfig::female());
        let res = named.speak_script("[giggle] Hello.");
        assert_eq!(res.segments.len(), 1);
        assert_eq!(res.segments[0].sound, None);

        // from code
        let laugh = synth.vocalize("laugh", 0.6, 1.0).unwrap();
        assert!(laugh.duration() > 0.8 && laugh.duration() < 3.0);
        assert_eq!(laugh.words.len(), 1);
        assert_eq!(laugh.text, "[laugh]");
        let peak = laugh.samples.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.5, "peak {peak}");
        assert!(synth.vocalize("no such sound", 0.6, 1.0).is_none());
    }

    #[test]
    fn sounds_last_as_long_as_asked() {
        let synth = Synth::new(VoiceConfig::female());
        let secs = |script: &str| synth.speak_script(script).speech.duration();
        // a longer laugh has more calls in it, a longer sigh is drawn out
        for (script, want) in [("[laugh=3s]", 3.0), ("[laugh duration=0.8s]", 0.8), ("[sigh 2s]", 2.0), ("[giggle duration=4]", 4.0), ("[shh=2500ms]", 2.5), ("[snore for=5s]", 5.0)] {
            let got = secs(script);
            assert!((got - want).abs() < 0.12, "{script}: {got} s");
        }
        let calls = |script: &str| {
            let res = synth.speak_script(script);
            let mut n = 0;
            let mut on = false;
            for f in &res.speech.frames {
                if f.av > 0.25 && !on {
                    n += 1;
                }
                on = f.av > 0.25;
            }
            n
        };
        assert!(calls("[laugh=3s]") >= calls("[laugh]") + 4, "{} vs {}", calls("[laugh=3s]"), calls("[laugh]"));
        // what cannot be longer stays what it is
        assert!(secs("[hiccup=3s]") < 0.8);
        // from code, and next to speech
        let laugh = synth.vocalize_for("laugh", 0.6, 2.0).unwrap();
        assert!((laugh.duration() - 2.0).abs() < 0.12, "{}", laugh.duration());
        let plain = secs("Well then.");
        assert!((secs("Well then. [sigh=1.5s]") - plain - 1.5).abs() < 0.15);
        // a bare number after the name is how strongly, not how long
        assert!((secs("[laugh=0.9]") - secs("[laugh]")).abs() < 0.7);
    }

    #[test]
    fn laughing_while_speaking_changes_the_voice_and_keeps_the_words() {
        let text = "I can't believe you actually did that! That is the funniest thing I have heard all week.";
        let mut cfg = VoiceConfig::female();
        cfg.variation = 0.0;
        let plain = Synth::new(cfg.clone()).speak(text);
        let median_pitch = |s: &Speech| {
            let mut f0: Vec<f64> = s.frames.iter().filter(|f| f.av > 0.4 && f.word >= 0).map(|f| f.f0).collect();
            f0.sort_by(|a, b| a.partial_cmp(b).unwrap());
            f0[f0.len() / 2]
        };
        // breath between two stretches of voice inside one word: the laugh breaking in
        let breaks = |s: &Speech| {
            let mut n = 0;
            for w in 0..s.words.len() as i32 {
                let fr: Vec<&Frame> = s.frames.iter().filter(|f| f.word == w).collect();
                for k in 1..fr.len().saturating_sub(1) {
                    if fr[k].phone.is_vowel() && fr[k].av < 0.12 && fr[k].ah > 0.2 && fr[k - 1].av >= 0.12 {
                        n += 1;
                    }
                }
            }
            n
        };
        assert_eq!(breaks(&plain), 0);

        // a little: a smile in the voice - higher, breathier, the same length and words
        cfg.laughter = 0.12;
        let smiling = Synth::new(cfg.clone()).speak(text);
        assert_eq!(smiling.samples.len(), plain.samples.len());
        assert!(semitones(median_pitch(&smiling), median_pitch(&plain)) > 0.2);
        assert!(smiling.samples != plain.samples);

        // more: the laugh breaks into stressed vowels and comes out after the sentence
        cfg.laughter = 0.7;
        let laughing = Synth::new(cfg.clone()).speak(text);
        assert_eq!(laughing.words.len(), plain.words.len());
        assert_eq!(laughing.text, plain.text);
        assert!(laughing.duration() > plain.duration() + 0.4, "{} vs {}", laughing.duration(), plain.duration());
        assert!(semitones(median_pitch(&laughing), median_pitch(&plain)) > 1.0);
        assert!(breaks(&laughing) >= 2, "{} breaks", breaks(&laughing));
        // (frames after the last word that are voiced: the laugh that follows it)
        let after_words = laughing.words.last().unwrap().end;
        assert!(laughing.frames.iter().any(|f| f.t_sec > after_words + 0.05 && f.av > 0.3), "a laugh after the sentence");
        assert!(laughing.samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        for pair in laughing.words.windows(2) {
            assert!(pair[0].end <= pair[1].start + 1e-9);
        }
        // the same take is the same performance, in one piece or streamed
        assert_eq!(Synth::new(cfg.clone()).speak(text).samples, laughing.samples);
        let mut stream = Synth::new(cfg.clone()).stream(text);
        let mut total = 0;
        let mut buf = vec![0.0f32; 4096];
        loop {
            let got = stream.read(&mut buf);
            if got == 0 {
                break;
            }
            total += got;
        }
        assert!((total as f64 / SAMPLE_RATE as f64 - laughing.duration()).abs() < 0.05);
        // a whisper has no voice to laugh with
        cfg.whisper = true;
        assert!(Synth::new(cfg).speak(text).frames.iter().all(|f| f.av == 0.0));

        // in a script it is a delivery that starts and stops
        let synth = Synth::new(VoiceConfig::female());
        let res = synth.speak_script("[laughing] You should have seen his face! [/laughing] Anyway. [laughter=0.9] Stop it! [reset] Fine.");
        assert_eq!(res.segments.len(), 4);
        let seg_pitch = |k: usize| {
            let sg = &res.segments[k];
            let mut f0: Vec<f64> = res.speech.frames.iter().filter(|f| f.t_sec >= sg.start && f.t_sec < sg.end && f.av > 0.4 && f.word >= 0).map(|f| f.f0).collect();
            f0.sort_by(|a, b| a.partial_cmp(b).unwrap());
            f0[f0.len() / 2]
        };
        assert!(seg_pitch(0) > seg_pitch(1) * 1.04, "{} vs {}", seg_pitch(0), seg_pitch(1));
        assert!(res.segments.iter().all(|sg| sg.sound.is_none()));
        // ... and `[laughing]` is not the sound `[laugh]`
        assert!(synth.speak_script("[laughing] Hello there.").speech.words.iter().all(|w| !w.word.starts_with('[')));
    }

    #[test]
    fn singing_tracks_musical_pitches_syllables_and_chorus() {
        let synth = Synth::new(VoiceConfig::female());
        let sung = synth
            .sing("[bpm=120] C4:1=Twin- C4:1=kle G4:1=twin- G4:1=kle A4:1=lit- A4:1=~ G4:2=star")
            .unwrap();
        assert_eq!(sung.words.len(), 6);
        assert!(sung.duration() > 3.5);
        assert!(sung.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));

        // First sung vowel sits near C4 (~261.6 Hz), fifth near A4 (~440 Hz).
        let c4_w = &sung.words[0];
        let a4_w = &sung.words[4];
        let c4_f0 = mean(
            sung.frames
                .iter()
                .filter(|f| f.t_sec >= c4_w.start + 0.08 && f.t_sec < c4_w.end - 0.05 && f.av > 0.5)
                .map(|f| f.f0),
        );
        let a4_f0 = mean(
            sung.frames
                .iter()
                .filter(|f| f.t_sec >= a4_w.start + 0.08 && f.t_sec < a4_w.end - 0.05 && f.av > 0.5)
                .map(|f| f.f0),
        );
        assert!((c4_f0 - 261.63).abs() < 12.0, "C4 f0={c4_f0}");
        assert!((a4_f0 - 440.0).abs() < 18.0, "A4 f0={a4_f0}");

        // Auto-syllabification splits multi-syllable words across notes ("Twinkle" -> 2 notes).
        let auto = synth
            .sing_lyrics("C4:1 C4:1 G4:1 G4:1", "Twinkle twinkle", 120.0)
            .unwrap();
        assert_eq!(auto.words.len(), 4);

        // Four-part SATB chorus renders cleanly in sync.
        let choir = synth
            .sing_chorus(
                "[bpm=110]
                 [part name=\"Soprano\" voice=soprano] E5:1=Sing D5:1=with C5:2=joy
                 [part name=\"Alto\" voice=alto] C5:1=Sing B4:1=with G4:2=joy
                 [part name=\"Tenor\" voice=tenor] G4:1=Sing F4:1=with E4:2=joy
                 [part name=\"Bass\" voice=deep] C3:1=Sing G3:1=with C3:2=joy",
            )
            .unwrap();
        assert_eq!(choir.speech.words.len(), 3);
        assert!(choir.segments[0].singing);
        assert!(choir.speech.samples.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }

    #[test]
    fn randomized_and_registered_voices_work_in_dialogue_and_singing() {
        let mut tts = TTS::default();
        let alice = VoiceConfig::randomized(101, Some(Gender::Female), None);
        let bob = VoiceConfig::randomized(202, Some(Gender::Male), Some(92.0));
        assert_eq!(alice.gender, Gender::Female);
        assert_eq!(bob.gender, Gender::Male);
        assert!((bob.pitch_hz - 92.0).abs() < 1e-6);

        tts.register_voice("alice", alice.clone());
        tts.register_voice("bob", bob.clone());

        let scene = tts.speak_script("[alice] Hello Bob! [bob] Hi Alice! [alice, reset] Still Alice.");
        assert_eq!(scene.segments.len(), 3);
        assert_eq!(scene.segments[0].voice, "alice");
        assert_eq!(scene.segments[0].gender, Gender::Female);
        assert_eq!(scene.segments[1].voice, "bob");
        assert_eq!(scene.segments[1].gender, Gender::Male);
        assert_eq!(scene.segments[2].voice, "alice");
        assert!((scene.segments[2].pitch_hz - alice.pitch_hz).abs() < 1e-6);
    }
}
