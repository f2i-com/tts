//! Scripted multi-voice dialogue, inline prosody/parameter directives, and mixed
//! speech + singing cues.
//!
//! A script lets a writer or an AI agent steer the physical vocal tract mid-utterance —
//! speeding up, slowing down, raising expressiveness, shifting pitch in Hz or semitones,
//! whispering, inserting pauses or breaths, laughing, sighing or coughing, switching between
//! male and female speakers, or breaking into song — all from a single string or a structured
//! list of [`ScriptCue`]s:
//!
//! ```text
//! [female, cheerful, speed=1.08] Hey! Have you heard what this browser vocal tract can do?
//! [pause=220ms]
//! [deep, calm, speed=0.94] Tell me — I'm all ears.
//! [bright, excited] It can switch voices mid-script, [slow, whisper] drop to a secret whisper,
//! [bright, sing bpm=116] C5:0.5=and E5:0.5=burst G5:1.5=in- E5:0.5=to C5:1.5=song! [/sing]
//! [deep] Well. [chuckle] That was something. [sigh]
//! ```
//!
//! The sounds that are not words - `[laugh]`, `[giggle]`, `[sigh]`, `[cough]`, `[hmm]`,
//! `[whistle]` and the rest of [`crate::vocal::VOCAL_SOUNDS`] - are made by the voice that is
//! speaking at that point, and take `intensity=` (0..1) and `length=` (1 = usual) or a
//! `duration=`: `[laugh intensity=0.9 length=1.5]`, `[sigh duration=1.2s]`, or `[laugh=0.9]`
//! and `[laugh=2s]` for short.
//!
//! Laughing *while* speaking is a delivery, not a sound: `[laughing] I can't believe it!`
//! (or `[laughter=0.8]`) changes the voice itself until `[/laughing]` or `[reset]`.

use std::collections::HashMap;

use crate::config::{Gender, VoiceConfig};
use crate::frames::{self, Frame, FRAME_MS};
use crate::lexicon::Lexicon;
use crate::phone::Ph;
use crate::plan;
use crate::render::{normalization_gain, Renderer, SAMPLES_PER_FRAME, SAMPLE_RATE};
use crate::sing::{self, SingScore};
use crate::tables;
use crate::timeline::{self, clamp};
use crate::vocal::{self, VocalSound};
use crate::{soft_limit, Speech, WordTiming, FRAME_RATE};

const FRAME_SEC: f64 = FRAME_MS / 1000.0;

/// Metadata for one contiguous voice/mode span inside a scripted or chorus [`Speech`].
#[derive(Clone, Debug)]
pub struct SegmentSpan {
    pub start: f64,
    pub end: f64,
    pub voice: String,
    pub gender: Gender,
    pub pitch_hz: f64,
    pub tract_length_cm: f64,
    pub singing: bool,
    /// For a sound that is not a word (a laugh, a sigh ...), its id; `None` for speech and song.
    pub sound: Option<&'static str>,
}

/// A rendered scripted performance: standard [`Speech`] plus per-segment speaker spans.
#[derive(Clone, Debug)]
pub struct ScriptedSpeech {
    pub speech: Speech,
    pub segments: Vec<SegmentSpan>,
}

/// One action in a structured multi-voice / multi-style script.
#[derive(Clone, Debug)]
pub enum ScriptCue {
    /// Speak `text` with `config` (and human-readable `voice_label`).
    Speak {
        text: String,
        config: Box<VoiceConfig>,
        voice_label: String,
    },
    /// Insert an exact pause in milliseconds.
    Pause { duration_ms: f64 },
    /// Insert a natural inspiratory breath (`intensity` in `0.1..=1.0`).
    Inhale {
        duration_ms: f64,
        intensity: f64,
        config: Box<VoiceConfig>,
    },
    /// Sing a musical score with `config`.
    Sing {
        score: Box<SingScore>,
        config: Box<VoiceConfig>,
        voice_label: String,
    },
    /// A sound that is not a word - a laugh, a sigh, a cough (see [`crate::vocal`]) - made by
    /// the voice `config`: `intensity` in `0..=1` (0.6 is the usual), `length` 1 = usual; or
    /// as long as `duration_ms` says, as nearly as the sound allows (a hiccup stays a hiccup).
    Vocal {
        sound: &'static VocalSound,
        intensity: f64,
        length: f64,
        duration_ms: Option<f64>,
        config: Box<VoiceConfig>,
        voice_label: String,
    },
}

/// The usual strength of a vocal sound.
pub const VOCAL_INTENSITY: f64 = 0.6;

/// How strongly a sound is asked for: a number in 0..1 (or a percentage), or a word for one.
pub fn parse_intensity(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "faint" | "tiny" | "barely" => Some(0.15),
        "soft" | "gentle" | "quiet" | "small" | "light" => Some(0.35),
        "medium" | "normal" | "mid" => Some(VOCAL_INTENSITY),
        "strong" | "loud" | "big" | "hard" => Some(0.85),
        "max" | "full" | "huge" => Some(1.0),
        _ => {
            let percent = s.ends_with('%');
            let v: f64 = s.trim_end_matches('%').parse().ok()?;
            if !v.is_finite() {
                return None;
            }
            Some(clamp(if percent || v > 1.0 { v / 100.0 } else { v }, 0.0, 1.0))
        }
    }
}

/// How long a sound is asked to last, in milliseconds: a number with its unit (`2s`,
/// `800ms`). A bare number is not a duration (it is a strength or a factor).
pub fn parse_duration_ms(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase();
    let (number, scale) = if let Some(ms) = s.strip_suffix("ms") {
        (ms, 1.0)
    } else if let Some(sec) = s.strip_suffix("sec") {
        (sec, 1000.0)
    } else if let Some(sec) = s.strip_suffix('s') {
        (sec, 1000.0)
    } else {
        return None;
    };
    let v: f64 = number.trim().parse().ok()?;
    (v.is_finite() && v > 0.0).then(|| clamp(v * scale, 50.0, 30_000.0))
}

/// How long a sound is asked to be against its usual length: a factor, or a word for one.
pub fn parse_length(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase();
    match s.as_str() {
        "short" | "brief" | "quick" => Some(0.6),
        "normal" | "medium" => Some(1.0),
        "long" => Some(1.6),
        "verylong" | "endless" => Some(2.4),
        _ => {
            let v: f64 = s.trim_start_matches('x').trim_end_matches('x').parse().ok()?;
            v.is_finite().then(|| clamp(v, 0.25, 4.0))
        }
    }
}

/// Resolves any built-in voice preset, gender, choir archetype, or fitted speaker profile
/// (`woman_a`/`slt`, `woman_b`/`clb`, `man_a`/`bdl`, `man_b`/`rms`) into a [`VoiceConfig`].
pub fn named_voice(name: &str) -> Option<(VoiceConfig, &'static str)> {
    let key = name.trim().to_ascii_lowercase().replace('-', "_");
    match key.as_str() {
        "male" | "m" | "man" => Some((VoiceConfig::male(), "male")),
        "female" | "f" | "woman" => Some((VoiceConfig::female(), "female")),
        "deep" | "deep_bass" | "velvet_narrator" => Some((VoiceConfig::preset("deep")?, "deep")),
        "husky" | "husky_raspy" => Some((VoiceConfig::preset("husky")?, "husky")),
        "bright" | "bright_soprano" | "young_bright" => Some((VoiceConfig::preset("bright")?, "bright")),
        "whisper" | "soft_whisper" => Some((VoiceConfig::preset("whisper")?, "whisper")),
        // Fitted CMU Arctic speaker profiles (pure parameters, zero audio)
        "woman_a" | "us_woman_a" | "slt" | "arctic_slt" => {
            let mut v = VoiceConfig::female();
            v.pitch_hz = 186.0;
            v.tract_length_cm = 14.95;
            v.speed = 1.08;
            v.expressiveness = 0.94;
            v.tone = [2.4, 3.8, 3.4, -0.6, -2.5, -3.0, 1.2, 3.4, 3.2, 2.8];
            v.formants = [1.0, 1.0, 1.0, 1.085, 1.085];
            Some((v, "woman_a"))
        }
        "woman_b" | "us_woman_b" | "clb" | "arctic_clb" => {
            let mut v = VoiceConfig::female();
            v.pitch_hz = 184.5;
            v.tract_length_cm = 15.4;
            v.speed = 0.93;
            v.expressiveness = 0.94;
            v.warmth = 0.66;
            v.open_quotient = 0.69;
            v.tone = [3.8, 3.7, 1.2, -1.4, 0.7, -1.5, -1.5, 2.8, 2.6, 2.2];
            v.formants = [1.0, 1.0, 1.0, 1.03, 1.03];
            Some((v, "woman_b"))
        }
        "man_a" | "us_man_a" | "bdl" | "arctic_bdl" => {
            let mut v = VoiceConfig::male();
            v.pitch_hz = 113.17;
            v.tract_length_cm = 17.11;
            v.speed = 1.09;
            v.expressiveness = 1.02;
            v.open_quotient = 0.70;
            v.tone = [4.5, 5.8, 3.6, 0.7, -0.2, -0.3, 2.6, 4.2, 4.0, 3.5];
            v.formants = [1.0, 1.0, 1.0, 0.94, 0.94];
            Some((v, "man_a"))
        }
        "man_b" | "us_man_b" | "rms" | "arctic_rms" => {
            let mut v = VoiceConfig::male();
            v.pitch_hz = 92.42;
            v.tract_length_cm = 18.95;
            v.speed = 1.02;
            v.expressiveness = 1.02;
            v.open_quotient = 0.70;
            v.warmth = 0.32;
            v.breathiness = 0.18;
            v.tone = [6.5, 6.4, 2.2, -1.5, 0.1, -1.4, -1.0, 3.6, 4.2, 3.4];
            Some((v, "man_b"))
        }
        // Choir & vocal range archetypes
        "soprano" => {
            let mut v = VoiceConfig::female();
            v.pitch_hz = 246.0;
            v.tract_length_cm = 13.55;
            v.warmth = 0.60;
            v.vibrato = 0.36;
            v.expressiveness = 1.10;
            Some((v, "soprano"))
        }
        "alto" | "mezzo" | "clear_alto" | "warm_mezzo" => {
            let mut v = VoiceConfig::female();
            v.pitch_hz = 196.0;
            v.tract_length_cm = 14.25;
            v.warmth = 0.68;
            v.vibrato = 0.32;
            Some((v, "alto"))
        }
        "tenor" | "bright_tenor" => {
            let mut v = VoiceConfig::male();
            v.pitch_hz = 142.0;
            v.tract_length_cm = 16.45;
            v.warmth = 0.58;
            v.vibrato = 0.30;
            Some((v, "tenor"))
        }
        "baritone" | "warm_baritone" => {
            let mut v = VoiceConfig::male();
            v.pitch_hz = 110.0;
            v.tract_length_cm = 17.5;
            v.warmth = 0.62;
            v.vibrato = 0.28;
            Some((v, "baritone"))
        }
        "bass_singer" | "choir_bass" => {
            let mut v = VoiceConfig::preset("deep")?;
            v.vibrato = 0.26;
            Some((v, "bass_singer"))
        }
        _ => VoiceConfig::preset(&key).map(|v| (v, "preset")),
    }
}

/// Resolves a user-registered custom voice first, then falls back to [`named_voice`].
pub fn resolve_voice(
    name: &str,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Option<(VoiceConfig, String)> {
    let key = name.trim().to_ascii_lowercase().replace('-', "_");
    if let Some(map) = custom_voices {
        if let Some(v) = map.get(&key) {
            return Some((v.clone(), key));
        }
    }
    named_voice(name).map(|(v, canon)| {
        let lbl = if canon == "preset" { key } else { canon.to_string() };
        (v, lbl)
    })
}

#[derive(Clone, Debug)]
pub(crate) enum ImmediateAction {
    PauseMs(f64),
    Inhale { duration_ms: f64, intensity: f64 },
    Vocal { sound: &'static VocalSound, intensity: f64, length: f64, duration_ms: Option<f64> },
    StartSing { bpm: Option<f64> },
    EndSing,
}

fn switch_keeping_listener(cfg: &mut VoiceConfig, mut next: VoiceConfig) {
    next.seed = cfg.seed;
    next.variation = cfg.variation;
    next.folds = cfg.folds;
    *cfg = next;
}

/// Applies expressive style shorthands (`excited`, `calm`, `sad`, `whisper`, `fast`, etc.).
pub fn apply_style_preset(cfg: &mut VoiceConfig, style: &str) -> bool {
    match style.trim().to_ascii_lowercase().as_str() {
        "excited" | "enthusiastic" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 1.12, 0.55, 1.75);
            cfg.pitch_hz = clamp(cfg.pitch_hz * 1.07, 60.0, 350.0);
            cfg.expressiveness = 1.45;
            cfg.crispness = 1.10;
            cfg.volume = clamp(cfg.volume * 1.08, 0.1, 4.0);
            true
        }
        "happy" | "cheerful" | "warm_smile" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 1.06, 0.55, 1.75);
            cfg.pitch_hz = clamp(cfg.pitch_hz * 1.04, 60.0, 350.0);
            cfg.expressiveness = 1.28;
            cfg.warmth = 0.64;
            true
        }
        "calm" | "gentle" | "soothing" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 0.90, 0.55, 1.75);
            cfg.expressiveness = 0.84;
            cfg.warmth = 0.76;
            cfg.breathiness = clamp(cfg.breathiness + 0.08, 0.0, 0.85);
            cfg.open_quotient = 0.70;
            true
        }
        "sad" | "somber" | "melancholy" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 0.85, 0.55, 1.75);
            cfg.pitch_hz = clamp(cfg.pitch_hz * 0.93, 60.0, 350.0);
            cfg.expressiveness = 0.72;
            cfg.warmth = 0.74;
            cfg.breathiness = clamp(cfg.breathiness + 0.10, 0.0, 0.85);
            cfg.volume = clamp(cfg.volume * 0.88, 0.1, 4.0);
            true
        }
        "serious" | "firm" | "authoritative" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 0.95, 0.55, 1.75);
            cfg.pitch_hz = clamp(cfg.pitch_hz * 0.96, 60.0, 350.0);
            cfg.expressiveness = 0.90;
            cfg.crispness = 1.12;
            cfg.open_quotient = 0.60;
            true
        }
        "dramatic" | "theatrical" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 0.92, 0.55, 1.75);
            cfg.expressiveness = 1.58;
            cfg.pauses = 1.30;
            true
        }
        "urgent" | "hurried" => {
            cfg.whisper = false;
            cfg.speed = clamp(cfg.speed * 1.24, 0.55, 1.75);
            cfg.expressiveness = 1.32;
            cfg.crispness = 1.14;
            cfg.pauses = 0.65;
            true
        }
        "fast" | "quicker" => {
            cfg.speed = clamp(cfg.speed * 1.22, 0.55, 1.75);
            true
        }
        "slow" | "slower" => {
            cfg.speed = clamp(cfg.speed * 0.80, 0.55, 1.75);
            true
        }
        "louder" | "loud" => {
            cfg.volume = clamp(cfg.volume * 1.25, 0.1, 4.0);
            true
        }
        "softer" | "soft" | "quiet" => {
            cfg.volume = clamp(cfg.volume * 0.72, 0.1, 4.0);
            cfg.warmth = (cfg.warmth + 0.10).min(1.0);
            true
        }
        "voiced" | "modal" => {
            cfg.whisper = false;
            true
        }
        // laughing while speaking: the laugh comes through the words
        "laughing" | "amused" => {
            cfg.whisper = false;
            cfg.laughter = 0.6;
            true
        }
        "chuckling" | "smiling" => {
            cfg.whisper = false;
            cfg.laughter = 0.3;
            true
        }
        "giggling" | "helpless" => {
            cfg.whisper = false;
            cfg.laughter = 0.9;
            true
        }
        _ => false,
    }
}

fn parse_ms_value(raw: &str) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase();
    if let Some(ms) = s.strip_suffix("ms") {
        return ms.trim().parse::<f64>().ok().map(|v| clamp(v, 10.0, 10_000.0));
    }
    if let Some(sec) = s.strip_suffix('s') {
        return sec.trim().parse::<f64>().ok().map(|v| clamp(v * 1000.0, 10.0, 10_000.0));
    }
    s.parse::<f64>().ok().map(|v| {
        // Bare numbers <= 15 are treated as seconds if decimal (e.g. 0.4), otherwise ms (e.g. 350)
        if s.contains('.') && v <= 15.0 {
            clamp(v * 1000.0, 10.0, 10_000.0)
        } else {
            clamp(v, 10.0, 10_000.0)
        }
    })
}

fn apply_numeric_mod(current: f64, raw: &str, allow_semitones: bool) -> Option<f64> {
    let s = raw.trim().to_ascii_lowercase();
    if allow_semitones {
        if let Some(st_str) = s.strip_suffix("st") {
            let st: f64 = st_str.trim().parse().ok()?;
            return Some(current * 2f64.powf(st / 12.0));
        }
    }
    if let Some(pct_str) = s.strip_suffix('%') {
        let pct: f64 = pct_str.trim().parse().ok()?;
        return if s.starts_with('+') || s.starts_with('-') {
            Some(current * (1.0 + pct / 100.0))
        } else {
            Some(current * (pct / 100.0))
        };
    }
    if let Some(mult_str) = s.strip_prefix('*').or_else(|| s.strip_prefix('x')) {
        let m: f64 = mult_str.trim().parse().ok()?;
        return Some(current * m);
    }
    let clean = s.trim_end_matches("hz").trim_end_matches("cm").trim_end_matches("db");
    if (clean.starts_with('+') || clean.starts_with('-')) && allow_semitones && !s.ends_with("hz") {
        // For pitch, `+20` Hz vs `+3st` is already handled above, so `+20` adds 20 Hz
        let delta: f64 = clean.parse().ok()?;
        return Some(current + delta);
    }
    clean.parse::<f64>().ok()
}

/// Parses the contents of a `[...]` bracket tag.
///
/// Tracks both the active cue configuration (`cfg` / `cur_label`) and the active speaker's
/// clean baseline (`speaker_cfg` / `speaker_label`). Thus `[reset]` or `[normal]` (including
/// `[woman_a, reset]`) restores the active speaker's clean modal voice rather than reverting
/// a female character to a male `base_cfg`, while `[/voice]` or `[base_voice]` explicitly
/// returns to `base_cfg`.
fn try_parse_bracket_tag(
    inner: &str,
    cfg: &mut VoiceConfig,
    speaker_cfg: &mut VoiceConfig,
    base_cfg: &VoiceConfig,
    cur_label: &mut String,
    speaker_label: &mut String,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<Vec<ImmediateAction>, ()> {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return Err(());
    }
    if trimmed.eq_ignore_ascii_case("/sing") {
        return Ok(vec![ImmediateAction::EndSing]);
    }
    if trimmed.eq_ignore_ascii_case("/whisper") {
        cfg.whisper = false;
        cfg.breathiness = speaker_cfg.breathiness;
        return Ok(Vec::new());
    }
    if ["/laughing", "/laughter", "/chuckling", "/giggling", "/smiling", "/amused"].iter().any(|t| trimmed.eq_ignore_ascii_case(t)) {
        cfg.laughter = speaker_cfg.laughter;
        return Ok(Vec::new());
    }
    if trimmed.eq_ignore_ascii_case("/voice")
        || trimmed.eq_ignore_ascii_case("base_voice")
        || trimmed.eq_ignore_ascii_case("default_voice")
    {
        *speaker_cfg = base_cfg.clone();
        *speaker_label = base_cfg.gender.as_str().to_string();
        *cfg = speaker_cfg.clone();
        *cur_label = speaker_label.clone();
        return Ok(Vec::new());
    }
    if trimmed.eq_ignore_ascii_case("reset")
        || trimmed.eq_ignore_ascii_case("normal")
        || trimmed.eq_ignore_ascii_case("default")
    {
        *cfg = speaker_cfg.clone();
        *cur_label = speaker_label.clone();
        return Ok(Vec::new());
    }

    let mut test_cfg = cfg.clone();
    let mut test_label = cur_label.clone();
    let mut test_speaker_cfg = speaker_cfg.clone();
    let mut test_speaker_label = speaker_label.clone();
    let mut actions = Vec::new();

    // Support `[pause 300ms]` or `[sing bpm=110]` space-separated forms too
    let raw_parts: Vec<&str> = trimmed
        .split(|c: char| c.is_ascii_whitespace() || c == ',' || c == ';')
        .filter(|s| !s.is_empty())
        .collect();
    if raw_parts.is_empty() {
        return Err(());
    }

    let mut i = 0;
    while i < raw_parts.len() {
        let tok = raw_parts[i];
        let low = tok.to_ascii_lowercase();

        if low == "sing" {
            let mut bpm = None;
            while i + 1 < raw_parts.len() {
                if let Some((k, v)) = raw_parts[i + 1].split_once('=') {
                    if matches!(k.to_ascii_lowercase().as_str(), "bpm" | "tempo") {
                        bpm = v.parse::<f64>().ok();
                        i += 1;
                        continue;
                    }
                }
                break;
            }
            actions.push(ImmediateAction::StartSing { bpm });
            i += 1;
            continue;
        }

        if matches!(low.as_str(), "pause" | "break" | "wait" | "silence") {
            let ms = if i + 1 < raw_parts.len() {
                if let Some(m) = parse_ms_value(raw_parts[i + 1]) {
                    i += 1;
                    m
                } else {
                    300.0
                }
            } else {
                300.0
            };
            actions.push(ImmediateAction::PauseMs(ms));
            i += 1;
            continue;
        }

        // (a soft breath between phrases; `gasp` and the other breath sounds are in `vocal`)
        if matches!(low.as_str(), "inhale" | "breath_in" | "sigh_in") {
            actions.push(ImmediateAction::Inhale {
                duration_ms: 150.0,
                intensity: 0.55,
            });
            i += 1;
            continue;
        }

        // A sound that is not a word: `laugh`, `sigh`, `giggle intensity=0.8 length=1.5`,
        // `laugh long`, `chuckle gentle`. (A voice registered under that name comes first.)
        let is_voice_name = custom_voices.is_some_and(|m| m.contains_key(&low.replace('-', "_")));
        if let Some(sound) = vocal::find(&low).filter(|_| !is_voice_name) {
            let (mut intensity, mut length) = (VOCAL_INTENSITY, 1.0);
            let mut duration_ms = None;
            while i + 1 < raw_parts.len() {
                let next = raw_parts[i + 1];
                let next_low = next.to_ascii_lowercase();
                if let Some((k, v)) = next.split_once('=').or_else(|| next.split_once(':')) {
                    match k.to_ascii_lowercase().as_str() {
                        "intensity" | "strength" | "level" => intensity = parse_intensity(v).ok_or(())?,
                        "length" | "len" | "x" => length = parse_length(v).ok_or(())?,
                        // `duration=2s`, `duration=800ms`; a bare number is seconds
                        "duration" | "dur" | "for" | "time" => {
                            duration_ms = Some(parse_duration_ms(v).or_else(|| parse_duration_ms(&format!("{v}s"))).ok_or(())?);
                        }
                        _ => break,
                    }
                } else if let Some(ms) = parse_duration_ms(&next_low) {
                    // `laugh 2s`
                    duration_ms = Some(ms);
                } else if matches!(next_low.as_str(), "short" | "brief" | "long" | "verylong") {
                    length = parse_length(&next_low).ok_or(())?;
                } else if matches!(next_low.as_str(), "faint" | "gentle" | "light" | "strong" | "big" | "huge") {
                    intensity = parse_intensity(&next_low).ok_or(())?;
                } else {
                    break;
                }
                i += 1;
            }
            actions.push(ImmediateAction::Vocal { sound, intensity, length, duration_ms });
            i += 1;
            continue;
        }

        if matches!(low.as_str(), "base_voice" | "default_voice") {
            test_speaker_cfg = base_cfg.clone();
            test_speaker_label = base_cfg.gender.as_str().to_string();
            test_cfg = test_speaker_cfg.clone();
            test_label = test_speaker_label.clone();
            i += 1;
            continue;
        }

        if matches!(low.as_str(), "reset" | "normal" | "default") {
            test_cfg = test_speaker_cfg.clone();
            test_label = test_speaker_label.clone();
            i += 1;
            continue;
        }

        if low == "whisper" {
            test_cfg.whisper = true;
            if test_cfg.breathiness < 0.45 {
                test_cfg.breathiness = 0.52;
            }
            test_label = test_speaker_label.clone();
            i += 1;
            continue;
        }
        if low == "folds" {
            test_cfg.folds = true;
            test_speaker_cfg.folds = true;
            i += 1;
            continue;
        }
        if low == "pulse" || low == "equation" {
            test_cfg.folds = false;
            test_speaker_cfg.folds = false;
            i += 1;
            continue;
        }

        if let Some((next_v, lbl)) = resolve_voice(&low, custom_voices) {
            switch_keeping_listener(&mut test_cfg, next_v);
            test_label = lbl.clone();
            test_speaker_cfg = test_cfg.clone();
            test_speaker_label = lbl;
            i += 1;
            continue;
        }

        if apply_style_preset(&mut test_cfg, &low) {
            i += 1;
            continue;
        }

        // `key=value` or `key:value`
        let kv = tok.split_once('=').or_else(|| tok.split_once(':'));
        let Some((raw_k, raw_v)) = kv else {
            return Err(());
        };
        let k = raw_k.trim().to_ascii_lowercase().replace(['_', '-'], "");
        let v = raw_v.trim().trim_matches('"');

        match k.as_str() {
            "voice" | "speaker" | "preset" | "character" => {
                if v.eq_ignore_ascii_case("default") || v.eq_ignore_ascii_case("base") || v.eq_ignore_ascii_case("current") {
                    test_speaker_cfg = base_cfg.clone();
                    test_speaker_label = base_cfg.gender.as_str().to_string();
                    test_cfg = test_speaker_cfg.clone();
                    test_label = test_speaker_label.clone();
                } else if v.eq_ignore_ascii_case("reset") || v.eq_ignore_ascii_case("normal") {
                    test_cfg = test_speaker_cfg.clone();
                    test_label = test_speaker_label.clone();
                } else if let Some((next_v, lbl)) = resolve_voice(v, custom_voices) {
                    switch_keeping_listener(&mut test_cfg, next_v);
                    test_label = lbl.clone();
                    test_speaker_cfg = test_cfg.clone();
                    test_speaker_label = lbl;
                } else {
                    return Err(());
                }
            }
            "gender" => {
                let g = Gender::parse(v).ok_or(())?;
                test_cfg.set_gender(g);
                let lbl = g.as_str().to_string();
                test_label = lbl.clone();
                test_speaker_cfg = test_cfg.clone();
                test_speaker_label = lbl;
            }
            "style" | "emotion" | "mood" | "tonestyle" => {
                if v.eq_ignore_ascii_case("normal") || v.eq_ignore_ascii_case("reset") {
                    test_cfg = test_speaker_cfg.clone();
                    test_label = test_speaker_label.clone();
                } else if !apply_style_preset(&mut test_cfg, v) {
                    return Err(());
                }
            }
            "pause" | "break" | "wait" | "pausems" => {
                let ms = parse_ms_value(v).ok_or(())?;
                actions.push(ImmediateAction::PauseMs(ms));
            }
            "inhale" | "breathin" => {
                let val = v.parse::<f64>().unwrap_or(0.55).clamp(0.1, 1.0);
                actions.push(ImmediateAction::Inhale {
                    duration_ms: 110.0 + val * 120.0,
                    intensity: val,
                });
            }
            "speed" | "rate" | "tempo" => {
                test_cfg.speed = apply_numeric_mod(test_cfg.speed, v, false).ok_or(())?;
            }
            "pitch" | "pitchhz" | "f0" => {
                test_cfg.pitch_hz = apply_numeric_mod(test_cfg.pitch_hz, v, true).ok_or(())?;
            }
            "tract" | "tractlength" | "tractlengthcm" | "size" => {
                test_cfg.tract_length_cm = apply_numeric_mod(test_cfg.tract_length_cm, v, false).ok_or(())?;
            }
            "expr" | "expressiveness" | "intonation" => {
                test_cfg.expressiveness = apply_numeric_mod(test_cfg.expressiveness, v, false).ok_or(())?;
            }
            "warmth" | "warm" => {
                test_cfg.warmth = apply_numeric_mod(test_cfg.warmth, v, false).ok_or(())?;
            }
            "chest" | "chestresonance" => {
                test_cfg.chest_resonance = apply_numeric_mod(test_cfg.chest_resonance, v, false).ok_or(())?;
            }
            "breathiness" | "breath" => {
                test_cfg.breathiness = apply_numeric_mod(test_cfg.breathiness, v, false).ok_or(())?;
            }
            "nasality" | "nasal" => {
                test_cfg.nasality = apply_numeric_mod(test_cfg.nasality, v, false).ok_or(())?;
            }
            "roughness" | "rough" | "jitter" => {
                test_cfg.roughness = apply_numeric_mod(test_cfg.roughness, v, false).ok_or(())?;
            }
            "crispness" | "crisp" => {
                test_cfg.crispness = apply_numeric_mod(test_cfg.crispness, v, false).ok_or(())?;
            }
            "vibrato" | "vib" => {
                test_cfg.vibrato = apply_numeric_mod(test_cfg.vibrato, v, false).ok_or(())?;
            }
            "volume" | "gain" | "vol" | "level" => {
                test_cfg.volume = apply_numeric_mod(test_cfg.volume, v, false).ok_or(())?;
            }
            "variation" | "human" => {
                test_cfg.variation = apply_numeric_mod(test_cfg.variation, v, false).ok_or(())?;
            }
            "bass" => {
                test_cfg.bass = apply_numeric_mod(test_cfg.bass, v, false).ok_or(())?;
            }
            "presence" => {
                test_cfg.presence = apply_numeric_mod(test_cfg.presence, v, false).ok_or(())?;
            }
            "brightness" => {
                test_cfg.brightness = apply_numeric_mod(test_cfg.brightness, v, false).ok_or(())?;
            }
            "pauses" => {
                test_cfg.pauses = apply_numeric_mod(test_cfg.pauses, v, false).ok_or(())?;
            }
            "breaths" => {
                test_cfg.breaths = apply_numeric_mod(test_cfg.breaths, v, false).ok_or(())?;
            }
            "oq" | "openquotient" | "open" => {
                test_cfg.open_quotient = apply_numeric_mod(test_cfg.open_quotient, v, false).ok_or(())?;
            }
            "closure" => {
                test_cfg.closure = apply_numeric_mod(test_cfg.closure, v, false).ok_or(())?;
            }
            "subglottal" => {
                test_cfg.subglottal = apply_numeric_mod(test_cfg.subglottal, v, false).ok_or(())?;
            }
            "air" => {
                test_cfg.air = apply_numeric_mod(test_cfg.air, v, false).ok_or(())?;
            }
            "room" => {
                test_cfg.room = apply_numeric_mod(test_cfg.room, v, false).ok_or(())?;
            }
            "pitchcoupling" => {
                test_cfg.pitch_coupling = apply_numeric_mod(test_cfg.pitch_coupling, v, false).ok_or(())?;
            }
            "whisper" => {
                test_cfg.whisper = matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "on" | "yes");
            }
            "folds" => {
                test_cfg.folds = matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "on" | "yes");
            }
            "seed" => {
                if let Ok(sd) = v.parse::<u32>() {
                    test_cfg.seed = sd;
                } else {
                    return Err(());
                }
            }
            "laughter" | "laughing" | "laugh_while_speaking" | "laughwhilespeaking" => {
                test_cfg.laughter = match v.to_ascii_lowercase().as_str() {
                    "off" | "no" | "false" | "none" => 0.0,
                    "on" | "yes" | "true" => 0.6,
                    other => parse_intensity(other).ok_or(())?,
                };
            }
            _ => {
                // `laugh=0.8` (how strongly), `laugh=long`, or `laugh=2s` (for how long)
                let sound = vocal::find(raw_k).ok_or(())?;
                let (intensity, length, duration_ms) = match (parse_duration_ms(v), parse_intensity(v), parse_length(v)) {
                    (Some(ms), _, _) => (VOCAL_INTENSITY, 1.0, Some(ms)),
                    (None, Some(x), _) => (x, 1.0, None),
                    (None, None, Some(x)) => (VOCAL_INTENSITY, x, None),
                    (None, None, None) => return Err(()),
                };
                actions.push(ImmediateAction::Vocal { sound, intensity, length, duration_ms });
            }
        }
        i += 1;
    }

    test_cfg.sanitize();
    test_speaker_cfg.sanitize();
    *cfg = test_cfg;
    *cur_label = test_label;
    *speaker_cfg = test_speaker_cfg;
    *speaker_label = test_speaker_label;
    Ok(actions)
}

/// Shared helper used by [`crate::sing`] to apply voice directives inside singing score headers.
pub(crate) fn apply_directive_tokens_with_voices(
    tokens: &str,
    cfg: &mut VoiceConfig,
    base_cfg: &VoiceConfig,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Option<()> {
    let mut label = cfg.gender.as_str().to_string();
    let mut sp_cfg = cfg.clone();
    let mut sp_label = label.clone();
    try_parse_bracket_tag(
        tokens,
        cfg,
        &mut sp_cfg,
        base_cfg,
        &mut label,
        &mut sp_label,
        custom_voices,
    )
    .ok()?;
    Some(())
}

/// Parses an inline-tagged script string into a sequence of [`ScriptCue`]s.
pub fn parse_script(script: &str, base_cfg: &VoiceConfig) -> Vec<ScriptCue> {
    parse_script_with_voices(script, base_cfg, None)
}

/// Parses an inline-tagged script string with optional registered custom speaker voices.
pub fn parse_script_with_voices(
    script: &str,
    base_cfg: &VoiceConfig,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Vec<ScriptCue> {
    let mut cues: Vec<ScriptCue> = Vec::new();
    let mut cur_cfg = base_cfg.clone();
    cur_cfg.sanitize();
    let mut cur_label = cur_cfg.gender.as_str().to_string();
    let mut speaker_cfg = cur_cfg.clone();
    let mut speaker_label = cur_label.clone();
    let mut text_buf = String::new();
    let mut in_sing_mode: Option<Option<f64>> = None;

    let flush_buf = |buf: &mut String,
                     cues: &mut Vec<ScriptCue>,
                     cfg: &VoiceConfig,
                     label: &str,
                     sing_mode: Option<Option<f64>>| {
        let trimmed = buf.trim();
        if !trimmed.is_empty() {
            if let Some(bpm_opt) = sing_mode {
                if let Ok(mut sc) = sing::parse_sing_score_with_voices(trimmed, cfg, None, custom_voices) {
                    if let Some(b) = bpm_opt {
                        sc.bpm = clamp(b, 30.0, 320.0);
                    }
                    cues.push(ScriptCue::Sing {
                        score: Box::new(sc),
                        config: Box::new(cfg.clone()),
                        voice_label: label.to_string(),
                    });
                }
            } else {
                cues.push(ScriptCue::Speak {
                    text: trimmed.to_string(),
                    config: Box::new(cfg.clone()),
                    voice_label: label.to_string(),
                });
            }
        }
        buf.clear();
    };

    let mut idx = 0;
    let bytes = script.as_bytes();
    while idx < bytes.len() {
        if bytes[idx] == b'[' {
            if let Some(rel_end) = script[idx + 1..].find(']') {
                let end = idx + 1 + rel_end;
                let inner = &script[idx + 1..end];
                let mut next_cfg = cur_cfg.clone();
                let mut next_label = cur_label.clone();
                let mut next_speaker_cfg = speaker_cfg.clone();
                let mut next_speaker_label = speaker_label.clone();
                if let Ok(actions) = try_parse_bracket_tag(
                    inner,
                    &mut next_cfg,
                    &mut next_speaker_cfg,
                    base_cfg,
                    &mut next_label,
                    &mut next_speaker_label,
                    custom_voices,
                ) {
                    flush_buf(&mut text_buf, &mut cues, &cur_cfg, &cur_label, in_sing_mode);
                    cur_cfg = next_cfg;
                    cur_label = next_label;
                    speaker_cfg = next_speaker_cfg;
                    speaker_label = next_speaker_label;
                    for act in actions {
                        match act {
                            ImmediateAction::PauseMs(ms) => {
                                cues.push(ScriptCue::Pause { duration_ms: ms });
                            }
                            ImmediateAction::Inhale {
                                duration_ms,
                                intensity,
                            } => {
                                cues.push(ScriptCue::Inhale {
                                    duration_ms,
                                    intensity,
                                    config: Box::new(cur_cfg.clone()),
                                });
                            }
                            ImmediateAction::Vocal { sound, intensity, length, duration_ms } => {
                                cues.push(ScriptCue::Vocal {
                                    sound,
                                    intensity,
                                    length,
                                    duration_ms,
                                    config: Box::new(cur_cfg.clone()),
                                    voice_label: cur_label.clone(),
                                });
                            }
                            ImmediateAction::StartSing { bpm } => {
                                in_sing_mode = Some(bpm);
                            }
                            ImmediateAction::EndSing => {
                                in_sing_mode = None;
                            }
                        }
                    }
                    idx = end + 1;
                    continue;
                }
            }
        }
        let ch = script[idx..].chars().next().unwrap_or(' ');
        text_buf.push(ch);
        idx += ch.len_utf8();
    }

    flush_buf(&mut text_buf, &mut cues, &cur_cfg, &cur_label, in_sing_mode);
    cues
}

fn render_inhale_cue(duration_ms: f64, intensity: f64, cfg: &VoiceConfig) -> (Vec<f32>, Vec<Frame>) {
    let n_f = ((duration_ms / FRAME_MS).round() as usize).max(4);
    let mut frames = Vec::with_capacity(n_f);
    let female = cfg.gender == Gender::Female;
    let scale = 17.5 / clamp(cfg.tract_length_cm, 12.2, 21.5);
    for i in 0..n_f {
        let u = i as f64 / (n_f - 1).max(1) as f64;
        let env = (std::f64::consts::PI * u).sin();
        frames.push(Frame {
            t_sec: i as f64 * FRAME_SEC,
            clause_idx: -1,
            micro_f0: 0.0,
            f0: cfg.pitch_hz,
            f1: 520.0 * (1.0 + (scale - 1.0) * 0.8),
            f2: 1500.0 * scale,
            f3: 2490.0 * scale,
            b1: 260.0 + if female { 34.0 } else { 0.0 },
            b2: 220.0,
            b3: 240.0,
            av: 0.0,
            ah: 0.14 * clamp(intensity, 0.1, 1.0) * env,
            af: 0.0,
            avb: 0.0,
            nasal: 0.0,
            oral_seal: 0.0,
            fnz: 500.0,
            lax: 0.5,
            creak: 0.0,
            fric_f1: 2000.0,
            fric_b1: 1200.0,
            fric_f2: 4500.0,
            fric_b2: 1600.0,
            fric_g1: 0.0,
            fric_g2: 0.0,
            fric_hp: 0.0,
            fric_lp: 0.0,
            tone_hz: 1500.0,
            tone: 0.0,
            is_closure: false,
            is_burst: false,
            stress: 0,
            jaw: 0.10 + 0.18 * env,
            t_h: 0.45,
            t_f: 0.50,
            t_tip: 0.16,
            lip_r: 0.05,
            lip_s: 0.04,
            lip_c: 0.0,
            velum: 0.15,
            phone: Ph::SIL,
            word: -1,
            rms: 0.25 * env,
        });
    }
    let mut renderer = Renderer::new(frames, cfg);
    let mut pcm = vec![0.0f32; n_f * SAMPLES_PER_FRAME];
    renderer.render(n_f, &mut pcm);
    (pcm, renderer.into_frames())
}

fn silent_frames(duration_ms: f64, ref_cfg: &VoiceConfig) -> (Vec<f32>, Vec<Frame>) {
    let n_f = ((duration_ms / FRAME_MS).round() as usize).max(1);
    let s = &tables::SPEC_SIL;
    let mut frames = Vec::with_capacity(n_f);
    for i in 0..n_f {
        frames.push(Frame {
            t_sec: i as f64 * FRAME_SEC,
            clause_idx: -1,
            micro_f0: 0.0,
            f0: ref_cfg.pitch_hz,
            f1: s.f1,
            f2: s.f2,
            f3: s.f3,
            b1: s.b1,
            b2: s.b2,
            b3: s.b3,
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
            fric_lp: 0.0,
            tone_hz: 1500.0,
            tone: 0.0,
            is_closure: true,
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
        });
    }
    (vec![0.0f32; n_f * SAMPLES_PER_FRAME], frames)
}

fn smooth_edges(pcm: &mut [f32]) {
    let fade = 96.min(pcm.len() / 4);
    if fade == 0 {
        return;
    }
    for i in 0..fade {
        let g = (i as f32) / (fade as f32);
        pcm[i] *= g;
        let rev = pcm.len() - 1 - i;
        pcm[rev] *= g;
    }
}

/// Synthesizes a slice of [`ScriptCue`]s into one unified [`ScriptedSpeech`] utterance.
pub fn synthesize_cues(
    cues: &[ScriptCue],
    base_cfg: &VoiceConfig,
    lex: &Lexicon,
) -> ScriptedSpeech {
    let mut all_samples: Vec<f32> = Vec::new();
    let mut all_frames: Vec<Frame> = Vec::new();
    let mut all_words: Vec<WordTiming> = Vec::new();
    let mut text_parts: Vec<String> = Vec::new();
    let mut ipa_parts: Vec<String> = Vec::new();
    let mut segments: Vec<SegmentSpan> = Vec::new();

    let mut prev_voice_sig: Option<(Gender, i32, String)> = None;
    let mut prev_was_pause = false;
    // how many sounds that are not words came before: no two laughs of a script are the same
    let mut vocal_takes = 0u32;

    for cue in cues {
        let start_sec = all_samples.len() as f64 / SAMPLE_RATE as f64;
        let word_offset = all_words.len() as i32;

        match cue {
            ScriptCue::Pause { duration_ms } => {
                let (pcm, mut frs) = silent_frames(*duration_ms, base_cfg);
                for f in &mut frs {
                    f.t_sec += start_sec;
                }
                all_samples.extend_from_slice(&pcm);
                all_frames.extend(frs);
                prev_was_pause = true;
            }
            ScriptCue::Inhale {
                duration_ms,
                intensity,
                config,
            } => {
                let (mut pcm, mut frs) = render_inhale_cue(*duration_ms, *intensity, config);
                smooth_edges(&mut pcm);
                for f in &mut frs {
                    f.t_sec += start_sec;
                }
                all_samples.extend_from_slice(&pcm);
                all_frames.extend(frs);
                prev_was_pause = true;
            }
            ScriptCue::Speak {
                text,
                config,
                voice_label,
            } => {
                let mut cfg = (**config).clone();
                cfg.sanitize();
                let sig = (
                    cfg.gender,
                    (cfg.tract_length_cm * 20.0).round() as i32,
                    voice_label.clone(),
                );
                // Insert a natural conversational turn-taking beat (135 ms) when switching
                // between different speakers unless an explicit pause preceded this cue.
                if let Some(psig) = &prev_voice_sig {
                    if *psig != sig && !prev_was_pause && !all_samples.is_empty() {
                        let (gap_pcm, mut gap_frs) = silent_frames(135.0, &cfg);
                        let g_start = all_samples.len() as f64 / SAMPLE_RATE as f64;
                        for f in &mut gap_frs {
                            f.t_sec += g_start;
                        }
                        all_samples.extend_from_slice(&gap_pcm);
                        all_frames.extend(gap_frs);
                    }
                }
                prev_voice_sig = Some(sig);
                prev_was_pause = false;

                let plan = plan::build_plan(text, lex);
                if plan.is_empty() {
                    continue;
                }
                let seg_start = all_samples.len() as f64 / SAMPLE_RATE as f64;
                let tl = timeline::build_timeline(&plan, &cfg);
                let frs_in = frames::rasterize(&tl, &cfg);
                let cue_words = crate::word_timings(&plan, &frs_in);

                let mut renderer = Renderer::new(frs_in, &cfg);
                let n_f = renderer.frames().len();
                let mut pcm = vec![0.0f32; n_f * SAMPLES_PER_FRAME];
                if !pcm.is_empty() {
                    renderer.render(n_f, &mut pcm);
                }
                // Apply per-cue relative volume (e.g. softer whisper vs louder exclamation)
                if (cfg.volume - 1.0).abs() > 1e-6 {
                    let v = cfg.volume as f32;
                    for s in &mut pcm {
                        *s *= v;
                    }
                }
                smooth_edges(&mut pcm);
                let mut frs = renderer.into_frames();
                for f in &mut frs {
                    f.t_sec += seg_start;
                    if f.word >= 0 {
                        f.word += word_offset;
                    }
                }
                for mut w in cue_words {
                    w.start += seg_start;
                    w.end += seg_start;
                    all_words.push(w);
                }
                text_parts.push(plan.normalized);
                ipa_parts.push(plan.ipa);
                all_samples.extend_from_slice(&pcm);
                all_frames.extend(frs);

                let seg_end = all_samples.len() as f64 / SAMPLE_RATE as f64;
                segments.push(SegmentSpan {
                    start: seg_start,
                    end: seg_end,
                    voice: voice_label.clone(),
                    gender: cfg.gender,
                    pitch_hz: cfg.pitch_hz,
                    tract_length_cm: cfg.tract_length_cm,
                    singing: false,
                    sound: None,
                });
            }
            ScriptCue::Vocal {
                sound,
                intensity,
                length,
                duration_ms,
                config,
                voice_label,
            } => {
                let mut cfg = (**config).clone();
                cfg.sanitize();
                let sig = (
                    cfg.gender,
                    (cfg.tract_length_cm * 20.0).round() as i32,
                    voice_label.clone(),
                );
                // the same beat between two speakers as before a line of speech
                if let Some(psig) = &prev_voice_sig {
                    if *psig != sig && !prev_was_pause && !all_samples.is_empty() {
                        let (gap_pcm, mut gap_frs) = silent_frames(135.0, &cfg);
                        let g_start = all_samples.len() as f64 / SAMPLE_RATE as f64;
                        for f in &mut gap_frs {
                            f.t_sec += g_start;
                        }
                        all_samples.extend_from_slice(&gap_pcm);
                        all_frames.extend(gap_frs);
                    }
                }
                prev_voice_sig = Some(sig);
                prev_was_pause = false;

                let seg_start = all_samples.len() as f64 / SAMPLE_RATE as f64;
                let (frs_in, voice) = match duration_ms {
                    Some(ms) => vocal::perform_for(sound, &cfg, *intensity, *ms / 1000.0, vocal_takes),
                    None => vocal::perform(sound, &cfg, *intensity, *length, vocal_takes),
                };
                vocal_takes += 1;
                if frs_in.is_empty() {
                    continue;
                }
                let mut renderer = Renderer::new(frs_in, &voice);
                let n_f = renderer.frames().len();
                let mut pcm = vec![0.0f32; n_f * SAMPLES_PER_FRAME];
                renderer.render(n_f, &mut pcm);
                if (cfg.volume - 1.0).abs() > 1e-6 {
                    let v = cfg.volume as f32;
                    for s in &mut pcm {
                        *s *= v;
                    }
                }
                smooth_edges(&mut pcm);
                let mut frs = renderer.into_frames();
                for f in &mut frs {
                    f.t_sec += seg_start;
                    f.word = word_offset;
                }
                all_samples.extend_from_slice(&pcm);
                all_frames.extend(frs);
                let seg_end = all_samples.len() as f64 / SAMPLE_RATE as f64;
                // it takes its place among the words, in brackets
                let tag = format!("[{}]", sound.id);
                all_words.push(WordTiming { word: tag.clone(), start: seg_start, end: seg_end });
                text_parts.push(tag);
                segments.push(SegmentSpan {
                    start: seg_start,
                    end: seg_end,
                    voice: voice_label.clone(),
                    gender: cfg.gender,
                    pitch_hz: cfg.pitch_hz,
                    tract_length_cm: cfg.tract_length_cm,
                    singing: false,
                    sound: Some(sound.id),
                });
            }
            ScriptCue::Sing {
                score,
                config,
                voice_label,
            } => {
                let mut cfg = (**config).clone();
                cfg.sanitize();
                let seg_start = all_samples.len() as f64 / SAMPLE_RATE as f64;
                let (mut sung, used_cfg) = sing::synthesize_singing_raw(score, &cfg, lex, 0.0, 0.0);
                if sung.samples.is_empty() {
                    continue;
                }
                if (used_cfg.volume - 1.0).abs() > 1e-6 {
                    let v = used_cfg.volume as f32;
                    for s in &mut sung.samples {
                        *s *= v;
                    }
                }
                smooth_edges(&mut sung.samples);
                for f in &mut sung.frames {
                    f.t_sec += seg_start;
                    if f.word >= 0 {
                        f.word += word_offset;
                    }
                }
                for mut w in sung.words {
                    w.start += seg_start;
                    w.end += seg_start;
                    all_words.push(w);
                }
                if !sung.text.is_empty() {
                    text_parts.push(sung.text);
                }
                if !sung.ipa.is_empty() {
                    ipa_parts.push(sung.ipa);
                }
                all_samples.extend_from_slice(&sung.samples);
                all_frames.extend(sung.frames);

                let seg_end = all_samples.len() as f64 / SAMPLE_RATE as f64;
                prev_voice_sig = Some((
                    used_cfg.gender,
                    (used_cfg.tract_length_cm * 20.0).round() as i32,
                    voice_label.clone(),
                ));
                prev_was_pause = false;
                segments.push(SegmentSpan {
                    start: seg_start,
                    end: seg_end,
                    voice: voice_label.clone(),
                    gender: used_cfg.gender,
                    pitch_hz: used_cfg.pitch_hz,
                    tract_length_cm: used_cfg.tract_length_cm,
                    singing: true,
                    sound: None,
                });
            }
        }
    }

    // Calibrated whole-performance normalization (preserves quieter whispers alongside louder cues)
    if !all_samples.is_empty() {
        let mut peak_abs = 0.001f64;
        for &v in &all_samples {
            peak_abs = peak_abs.max((v as f64).abs());
        }
        let gain = normalization_gain(peak_abs);
        for s in &mut all_samples {
            *s = soft_limit(*s as f64 * gain) as f32;
        }
        let max_rms = all_frames.iter().map(|f| f.rms).fold(0.001f64, f64::max);
        for f in &mut all_frames {
            f.rms = clamp(f.rms / max_rms, 0.0, 1.0);
        }
    }

    let _ = FRAME_RATE;
    ScriptedSpeech {
        speech: Speech {
            samples: all_samples,
            sample_rate: SAMPLE_RATE,
            text: text_parts.join(" "),
            ipa: ipa_parts.join(" "),
            words: all_words,
            frames: all_frames,
        },
        segments,
    }
}
