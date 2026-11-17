//! Musical lyric singing & polyphonic chorus synthesis for the physical vocal tract.
//!
//! Unlike speech, where pitch drifts along a conversational phrase contour and vowels compress
//! or expand together with their consonants, a singer:
//! 1. keeps consonant onsets and codas crisp at natural speech-like durations (`~35–65 ms`),
//! 2. sustains the primary vowel nucleus across the full musical note (holding the open nucleus
//!    of a diphthong until the last ~18% of the note),
//! 3. glides smoothly in log-pitch between legato notes (portamento) with a subtle human onset
//!    scoop after rests and progressive vibrato that blooms on sustained notes, and
//! 4. opens the jaw on high soprano/alto notes so `F1` tracks just above `F0` (Sundberg's rule),
//!    keeping high notes resonant rather than thin.

use std::collections::HashMap;

use crate::config::{Gender, VoiceConfig};
use crate::frames::{Frame, FRAME_MS};
use crate::lexicon::Lexicon;
use crate::phone::{Ph, Phone};
use crate::plan;
use crate::render::{normalization_gain, Renderer, SAMPLES_PER_FRAME, SAMPLE_RATE};
use crate::script::{apply_directive_tokens_with_voices, SegmentSpan};
use crate::tables::{self, Place, Spec};
use crate::timeline::clamp;
use crate::{apply_volume, soft_limit, Speech, WordTiming, FRAME_RATE};

const FRAME_SEC: f64 = FRAME_MS / 1000.0;
const PI: f64 = std::f64::consts::PI;
const TAU: f64 = std::f64::consts::TAU;

/// One musical note (or rest) in a vocal line.
#[derive(Clone, Debug)]
pub struct SingNote {
    /// Target pitch in Hz, or `None` for a musical rest.
    pub pitch_hz: Option<f64>,
    /// Note duration in beats (at `bpm`), or `0.0` when `duration_ms` is used directly.
    pub beats: f64,
    /// Optional explicit duration in milliseconds (overrides `beats` when `Some`).
    pub duration_ms: Option<f64>,
    /// Lyric syllable or word sung on this note (`"~"` / `"-"` / `"_"` = legato melisma slur
    /// continuing the previous vowel; `"breath"` on a rest = audible singer's inhale).
    pub lyric: String,
    /// True when this note slurs/ties smoothly from the previous vowel without re-articulation.
    pub slur: bool,
    /// Optional per-note dynamic multiplier (`1.0` = mezzo-forte default).
    pub velocity: f64,
    /// Optional per-note vibrato multiplier (`1.0` = voice default).
    pub vibrato: Option<f64>,
}

impl SingNote {
    pub fn rest(beats: f64) -> Self {
        SingNote {
            pitch_hz: None,
            beats,
            duration_ms: None,
            lyric: String::new(),
            slur: false,
            velocity: 1.0,
            vibrato: None,
        }
    }

    pub fn duration_ms(&self, bpm: f64) -> f64 {
        if let Some(ms) = self.duration_ms {
            ms.max(25.0)
        } else {
            let beat_ms = 60_000.0 / clamp(bpm, 30.0, 320.0);
            (self.beats.max(0.05) * beat_ms).max(25.0)
        }
    }
}

/// A monophonic vocal score.
#[derive(Clone, Debug)]
pub struct SingScore {
    /// Tempo in beats per minute (quarter note = 1 beat).
    pub bpm: f64,
    /// Optional voice override directives parsed from score header (e.g. `[voice=bright]`).
    pub voice: Option<VoiceConfig>,
    /// Semitone transposition applied to every pitched note (`0.0` = as written).
    pub transpose_semitones: f64,
    /// Legato portamento speed (`1.0` = natural ~42 ms glide, `0.0` = crisp step).
    pub legato: f64,
    /// Sequence of notes and rests.
    pub notes: Vec<SingNote>,
}

impl Default for SingScore {
    fn default() -> Self {
        SingScore {
            bpm: 108.0,
            voice: None,
            transpose_semitones: 0.0,
            legato: 1.0,
            notes: Vec::new(),
        }
    }
}

/// One voice part in a polyphonic [`ChorusScore`].
#[derive(Clone, Debug)]
pub struct ChorusPart {
    pub name: String,
    pub config: VoiceConfig,
    pub score: SingScore,
    /// Relative mix gain of this singer (`1.0` = unity).
    pub gain: f64,
    /// Optional timing offset in ms (humanizes ensemble entries).
    pub offset_ms: f64,
    /// Optional detune in cents (`100` cents = 1 semitone).
    pub detune_cents: f64,
}

/// A multi-voice sung chorus rendered and mixed in sample-accurate sync.
#[derive(Clone, Debug)]
pub struct ChorusScore {
    pub bpm: f64,
    pub parts: Vec<ChorusPart>,
}

/// Converts a musical note string (`"C4"`, `"F#4"`, `"Bb3"`, `"Eb5"`, `"440Hz"`, `"midi:60"`,
/// or `"R"` / `"rest"`) into Hz. Returns `Ok(None)` for a rest.
pub fn parse_note_hz(token: &str) -> Result<Option<f64>, String> {
    let s = token.trim();
    if s.is_empty() {
        return Err("empty note".into());
    }
    let lower = s.to_ascii_lowercase();
    if matches!(lower.as_str(), "r" | "rest" | "_" | "-" | "sil" | "pause" | "breath") {
        return Ok(None);
    }
    if let Some(hz_str) = lower.strip_suffix("hz") {
        let hz: f64 = hz_str
            .trim()
            .parse()
            .map_err(|_| format!("invalid frequency {token:?}"))?;
        if !(45.0..=1400.0).contains(&hz) {
            return Err(format!("frequency {hz} Hz out of vocal range (45..1400 Hz)"));
        }
        return Ok(Some(hz));
    }
    if let Some(midi_str) = lower.strip_prefix("midi:") {
        let midi: f64 = midi_str
            .trim()
            .parse()
            .map_err(|_| format!("invalid MIDI note {token:?}"))?;
        return Ok(Some(440.0 * 2f64.powf((midi - 69.0) / 12.0)));
    }
    // Pure integer MIDI number (e.g. "60" = C4)
    if s.bytes().all(|b| b.is_ascii_digit()) {
        if let Ok(midi) = s.parse::<u32>() {
            if (28..=96).contains(&midi) {
                return Ok(Some(440.0 * 2f64.powf((midi as f64 - 69.0) / 12.0)));
            }
        }
    }

    let bytes = s.as_bytes();
    let base_semi: i32 = match bytes[0].to_ascii_uppercase() {
        b'C' => 0,
        b'D' => 2,
        b'E' => 4,
        b'F' => 5,
        b'G' => 7,
        b'A' => 9,
        b'B' => 11,
        _ => return Err(format!("unrecognized note {token:?} (expected e.g. C4, F#4, Bb3, or R)")),
    };

    let mut rest = &s[1..];
    let mut accidental: i32 = 0;
    loop {
        if let Some(r) = rest.strip_prefix('#').or_else(|| rest.strip_prefix('♯')) {
            accidental += 1;
            rest = r;
        } else if let Some(r) = rest.strip_prefix('♭') {
            accidental -= 1;
            rest = r;
        } else if rest.starts_with('b') && rest.len() > 1 {
            accidental -= 1;
            rest = &rest[1..];
        } else if rest.starts_with('s') && rest.len() > 1 && rest.as_bytes()[1].is_ascii_digit() {
            accidental += 1;
            rest = &rest[1..];
        } else {
            break;
        }
    }

    let octave: i32 = if rest.is_empty() {
        4
    } else {
        rest.parse::<i32>()
            .map_err(|_| format!("invalid octave in note {token:?}"))?
    };
    if !(1..=7).contains(&octave) {
        return Err(format!("octave {octave} in {token:?} out of vocal range (1..7)"));
    }
    let midi = (octave + 1) * 12 + base_semi + accidental;
    let hz = 440.0 * 2f64.powf((midi as f64 - 69.0) / 12.0);
    Ok(Some(hz))
}

fn parse_beat_or_ms(raw: &str) -> Result<(f64, Option<f64>), String> {
    let s = raw.trim().to_ascii_lowercase();
    if s.is_empty() {
        return Ok((1.0, None));
    }
    if let Some(ms) = s.strip_suffix("ms") {
        let v: f64 = ms.trim().parse().map_err(|_| format!("bad duration {raw:?}"))?;
        return Ok((0.0, Some(v.max(25.0))));
    }
    if let Some(sec) = s.strip_suffix('s') {
        if let Ok(v) = sec.trim().parse::<f64>() {
            return Ok((0.0, Some((v * 1000.0).max(25.0))));
        }
    }
    // Musical note length shorthands: w=4, h=2, q=1, e=0.5, s=0.25, plus dotted "."
    let (core, dotted) = match s.strip_suffix('.') {
        Some(c) if c.chars().all(|ch| ch.is_ascii_alphabetic()) => (c, 1.5),
        _ => (s.as_str(), 1.0),
    };
    match core {
        "w" | "whole" => return Ok((4.0 * dotted, None)),
        "h" | "half" => return Ok((2.0 * dotted, None)),
        "q" | "quarter" => return Ok((1.0 * dotted, None)),
        "e" | "eighth" => return Ok((0.5 * dotted, None)),
        "sixteenth" => return Ok((0.25 * dotted, None)),
        _ => {}
    }
    if let Some((num, den)) = s.split_once('/') {
        let a: f64 = num.trim().parse().map_err(|_| format!("bad fraction {raw:?}"))?;
        let b: f64 = den.trim().parse().map_err(|_| format!("bad fraction {raw:?}"))?;
        if b > 0.0 {
            return Ok(((a / b).max(0.05), None));
        }
    }
    let beats: f64 = s.parse().map_err(|_| format!("bad beat count {raw:?}"))?;
    Ok((beats.max(0.05), None))
}

fn is_note_head(tok: &str) -> bool {
    let core = tok.split([':', '=', '/']).next().unwrap_or(tok);
    parse_note_hz(core).is_ok()
}

fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_quotes = false;
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quotes = !in_quotes;
        } else if !in_quotes {
            if i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'/' {
                return &line[..i];
            }
            if i + 1 < bytes.len()
                && bytes[i] == b'#'
                && bytes[i + 1] == b' '
                && (i == 0 || bytes[i - 1].is_ascii_whitespace())
            {
                return &line[..i];
            }
            if i == 0 && bytes[i] == b'#' {
                return "";
            }
        }
        i += 1;
    }
    line
}

fn normalize_score_line(raw_line: &str) -> String {
    let line = strip_inline_comment(raw_line).trim();
    if line.is_empty() || line.starts_with('[') {
        return line.to_string();
    }
    if let Some((k, v)) = line.split_once(':') {
        let kl = k.trim().to_ascii_lowercase();
        let vt = v.trim().trim_matches('"').trim();
        match kl.as_str() {
            "lyrics" | "lyric" | "words" => {
                return format!("[lyrics=\"{}\"]", vt.replace('"', ""));
            }
            "bpm" | "tempo" if !vt.contains(char::is_whitespace) => {
                return format!("[bpm={vt}]");
            }
            "transpose" | "shift" if !vt.contains(char::is_whitespace) => {
                return format!("[transpose={vt}]");
            }
            "voice" | "singer" if !vt.contains(char::is_whitespace) => {
                return format!("[voice={vt}]");
            }
            _ => {}
        }
    }
    line.to_string()
}

/// Parses a compact human/AI singing score string into a [`SingScore`].
///
/// Supported forms (freely mixable, `|` barlines and `//` comments ignored):
/// - Inline: `"[bpm=108 voice=bright] C4:1=Twin- C4:1=kle G4:1=twin- G4:1=kle | A4:1=lit- A4:1=tle G4:2=star"`
/// - Space-paired: `"C4:1 Twin- | C4:1 kle | G4:1 twin- | G4:1 kle"`
/// - Shared lyrics directive: `"[bpm=108 lyrics=\"Twin-kle twin-kle\"] C4:1 C4:1 G4:1 G4:1"`
/// - Melisma / slur: `"E4:1=glo- F4:0.5=~ G4:0.5=~ E4:2=ria"`
/// - Rests & breaths: `"R:1"`, `"rest:0.5"`, `"breath:0.5"`
pub fn parse_sing_score(input: &str, base_cfg: &VoiceConfig) -> Result<SingScore, String> {
    parse_sing_score_with_voices(input, base_cfg, None, None)
}

pub fn parse_sing_score_with_lex(
    input: &str,
    base_cfg: &VoiceConfig,
    lex: Option<&Lexicon>,
) -> Result<SingScore, String> {
    parse_sing_score_with_voices(input, base_cfg, lex, None)
}

pub fn parse_sing_score_with_voices(
    input: &str,
    base_cfg: &VoiceConfig,
    lex: Option<&Lexicon>,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<SingScore, String> {
    let mut score = SingScore::default();
    let mut cfg = base_cfg.clone();
    let mut cfg_touched = false;
    let mut directive_lyrics: Option<String> = None;

    // Strip `//` and `# ` comments anywhere on a line while keeping `#` sharps inside notes like `F#4`!
    let mut cleaned = String::with_capacity(input.len());
    for line in input.lines() {
        let uncommented = normalize_score_line(line);
        if uncommented.is_empty() {
            continue;
        }
        cleaned.push_str(&uncommented);
        cleaned.push(' ');
    }

    // Tokenize, keeping `[...]` directive blocks intact.
    let mut raw_tokens: Vec<String> = Vec::new();
    let bytes = cleaned.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        while idx < bytes.len() && (bytes[idx].is_ascii_whitespace() || bytes[idx] == b'|' || bytes[idx] == b',') {
            idx += 1;
        }
        if idx >= bytes.len() {
            break;
        }
        if bytes[idx] == b'[' {
            if let Some(rel_end) = cleaned[idx + 1..].find(']') {
                let end = idx + 1 + rel_end;
                raw_tokens.push(cleaned[idx..=end].to_string());
                idx = end + 1;
                continue;
            }
        }
        let start = idx;
        let mut in_quotes = false;
        while idx < bytes.len() {
            let b = bytes[idx];
            if b == b'"' {
                in_quotes = !in_quotes;
            } else if !in_quotes && (b.is_ascii_whitespace() || b == b'|' || b == b',' || b == b'[') {
                break;
            }
            idx += 1;
        }
        if idx > start {
            raw_tokens.push(cleaned[start..idx].to_string());
        }
    }

    let mut explicit_lyrics = Vec::new();
    let mut ti = 0;
    while ti < raw_tokens.len() {
        let tok = &raw_tokens[ti];
        if tok.starts_with('[') && tok.ends_with(']') {
            let inner = &tok[1..tok.len() - 1];
            parse_sing_directives(
                inner,
                &mut score,
                &mut cfg,
                &mut cfg_touched,
                &mut directive_lyrics,
                custom_voices,
            );
            ti += 1;
            continue;
        }

        // Case 1: `NOTE:DUR=LYRIC` or `NOTE=LYRIC`
        if let Some((left, right)) = tok.split_once('=') {
            // Check if it's a top-level `bpm=120` or `lyrics="..."` written without brackets
            let key_low = left.to_ascii_lowercase();
            if matches!(
                key_low.as_str(),
                "bpm" | "tempo" | "transpose" | "key" | "legato" | "voice" | "lyrics"
            ) {
                parse_sing_directives(
                    tok,
                    &mut score,
                    &mut cfg,
                    &mut cfg_touched,
                    &mut directive_lyrics,
                    custom_voices,
                );
                ti += 1;
                continue;
            }
            let (note_str, dur_str) = left.split_once(':').unwrap_or((left, "1"));
            let mut pitch_hz = parse_note_hz(note_str)?;
            if let Some(hz) = &mut pitch_hz {
                *hz *= 2f64.powf(score.transpose_semitones / 12.0);
            }
            let (beats, duration_ms) = parse_beat_or_ms(dur_str)?;
            let lyric_clean = right.trim_matches('"').trim().to_string();
            let slur = matches!(lyric_clean.as_str(), "~" | "-" | "_");
            let lyric = if pitch_hz.is_none() && note_str.eq_ignore_ascii_case("breath") {
                "breath".to_string()
            } else {
                lyric_clean
            };
            score.notes.push(SingNote {
                pitch_hz,
                beats,
                duration_ms,
                lyric,
                slur,
                velocity: 1.0,
                vibrato: None,
            });
            explicit_lyrics.push(true);
            ti += 1;
            continue;
        }

        // Case 2: `NOTE:DUR` optionally followed by a standalone lyric token (e.g. `C4:1 Twin-`)
        let (note_str, dur_opt) = match tok.split_once(':') {
            Some((n, d)) => (n, Some(d)),
            None => (tok.as_str(), None),
        };
        let mut pitch_hz = parse_note_hz(note_str)?;
        if let Some(hz) = &mut pitch_hz {
            *hz *= 2f64.powf(score.transpose_semitones / 12.0);
        }
        let (beats, duration_ms) = match dur_opt {
            Some(d) => parse_beat_or_ms(d)?,
            None => (1.0, None),
        };

        let mut lyric = String::new();
        let had_explicit;
        if pitch_hz.is_none() {
            if note_str.eq_ignore_ascii_case("breath") {
                lyric = "breath".to_string();
            }
            had_explicit = true;
            ti += 1;
        } else if ti + 1 < raw_tokens.len()
            && !raw_tokens[ti + 1].starts_with('[')
            && !raw_tokens[ti + 1].contains('=')
            && !is_note_head(&raw_tokens[ti + 1])
        {
            lyric = raw_tokens[ti + 1].trim_matches('"').to_string();
            had_explicit = true;
            ti += 2;
        } else {
            lyric = "la".to_string();
            had_explicit = false;
            ti += 1;
        }
        let slur = matches!(lyric.as_str(), "~" | "-" | "_");
        score.notes.push(SingNote {
            pitch_hz,
            beats,
            duration_ms,
            lyric,
            slur,
            velocity: 1.0,
            vibrato: None,
        });
        explicit_lyrics.push(had_explicit);
    }

    if let Some(lyr_str) = directive_lyrics {
        apply_lyrics_to_unlabeled_notes(&mut score, &explicit_lyrics, &lyr_str, lex);
    }

    if cfg_touched {
        score.voice = Some(cfg);
    }
    Ok(score)
}

fn apply_lyrics_to_unlabeled_notes(
    score: &mut SingScore,
    explicit_lyrics: &[bool],
    lyrics_str: &str,
    lex: Option<&Lexicon>,
) {
    let target_count = score
        .notes
        .iter()
        .zip(explicit_lyrics.iter())
        .filter(|(n, &ex)| n.pitch_hz.is_some() && !ex)
        .count();
    if target_count == 0 {
        return;
    }
    let raw_chunks = split_lyric_tokens(lyrics_str);
    if raw_chunks.is_empty() {
        return;
    }
    let expanded = if let Some(l) = lex {
        expand_syllables_to_fit(&raw_chunks, target_count, l)
    } else {
        raw_chunks
    };
    let mut li = 0usize;
    for (note, &ex) in score.notes.iter_mut().zip(explicit_lyrics.iter()) {
        if note.pitch_hz.is_none() || ex {
            continue;
        }
        if li < expanded.len() {
            note.lyric = expanded[li].clone();
            note.slur = matches!(note.lyric.as_str(), "~" | "-" | "_");
            li += 1;
        } else {
            note.lyric = "~".to_string();
            note.slur = true;
        }
    }
}

fn parse_sing_directives(
    inner: &str,
    score: &mut SingScore,
    cfg: &mut VoiceConfig,
    cfg_touched: &mut bool,
    directive_lyrics: &mut Option<String>,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) {
    let mut leftover = Vec::new();
    for part in split_directive_kv(inner) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        if let Some((k, v)) = p.split_once('=') {
            match k.trim().to_ascii_lowercase().as_str() {
                "bpm" | "tempo" => {
                    if let Ok(b) = v.trim().parse::<f64>() {
                        score.bpm = clamp(b, 30.0, 320.0);
                    }
                    continue;
                }
                "transpose" | "shift" => {
                    let vs = v.trim().trim_end_matches("st");
                    if let Ok(st) = vs.parse::<f64>() {
                        score.transpose_semitones = clamp(st, -24.0, 24.0);
                    }
                    continue;
                }
                "legato" => {
                    let vl = v.trim().to_ascii_lowercase();
                    score.legato = match vl.as_str() {
                        "true" | "on" => 1.0,
                        "false" | "off" | "staccato" => 0.15,
                        _ => vl.parse::<f64>().unwrap_or(1.0).clamp(0.0, 2.0),
                    };
                    continue;
                }
                "lyrics" => {
                    let lv = v.trim().trim_matches('"').trim();
                    if !lv.is_empty() {
                        *directive_lyrics = Some(lv.to_string());
                    }
                    continue;
                }
                _ => {}
            }
        }
        leftover.push(p.to_string());
    }
    if !leftover.is_empty() {
        let joined = leftover.join(" ");
        let base = cfg.clone();
        if apply_directive_tokens_with_voices(&joined, cfg, &base, custom_voices).is_some() {
            *cfg_touched = true;
        }
    }
}

/// Pairs a sequence of note tokens (`"C4:1 C4:1 G4:1 G4:1 A4:1 A4:1 G4:2"`) with a plain or
/// hyphenated lyric string (`"Twinkle twinkle little star"` or `"Twin-kle twin-kle lit-tle star"`),
/// automatically splitting multi-syllable English words across consecutive notes when needed!
pub fn pair_notes_and_lyrics(
    notes_str: &str,
    lyrics_str: &str,
    bpm: f64,
    lex: &Lexicon,
) -> Result<SingScore, String> {
    let mut score = parse_sing_score(notes_str, &VoiceConfig::female())?;
    score.bpm = clamp(bpm, 30.0, 320.0);
    let pitched_count = score.notes.iter().filter(|n| n.pitch_hz.is_some()).count();
    if pitched_count == 0 {
        return Ok(score);
    }

    let raw_chunks = split_lyric_tokens(lyrics_str);
    if raw_chunks.is_empty() {
        return Ok(score);
    }

    // Expand unhyphenated multi-syllable words only until we match `pitched_count` notes.
    let expanded = expand_syllables_to_fit(&raw_chunks, pitched_count, lex);
    let mut li = 0usize;
    for note in &mut score.notes {
        if note.pitch_hz.is_none() {
            continue;
        }
        if li < expanded.len() {
            note.lyric = expanded[li].clone();
            note.slur = matches!(note.lyric.as_str(), "~" | "-" | "_");
            li += 1;
        } else {
            note.lyric = "~".to_string();
            note.slur = true;
        }
    }
    Ok(score)
}

fn split_lyric_tokens(lyrics: &str) -> Vec<String> {
    let mut out = Vec::new();
    for word in lyrics.split_whitespace() {
        if matches!(word, "~" | "-" | "_" | "|") {
            if word != "|" {
                out.push(word.to_string());
            }
            continue;
        }
        // Split hyphenated singing syllables: "Twin-kle" -> ["Twin-", "kle"]
        let mut start = 0;
        let bytes = word.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'-' && i > start && i + 1 < bytes.len() {
                out.push(word[start..=i].to_string());
                start = i + 1;
            }
        }
        if start < word.len() {
            out.push(word[start..].to_string());
        }
    }
    out
}

/// Valid English 2-consonant syllable onsets (Maximum Onset Principle).
fn valid_onset_pair(c1: Ph, c2: Ph) -> bool {
    matches!(
        (c1, c2),
        (Ph::P | Ph::B | Ph::K | Ph::G | Ph::F, Ph::L | Ph::R)
            | (Ph::T | Ph::D | Ph::TH, Ph::R | Ph::W)
            | (Ph::S, Ph::P | Ph::T | Ph::K | Ph::L | Ph::W | Ph::M | Ph::N)
            | (Ph::SH, Ph::R)
            | (Ph::P | Ph::B | Ph::K | Ph::F | Ph::M | Ph::V | Ph::HH, Ph::Y)
            | (Ph::K | Ph::G, Ph::W)
    )
}

/// Splits a phoneme slice into phonological `(onset, nucleus, coda)` syllables.
fn split_phones_into_syllables(phones: &[Phone]) -> Vec<Vec<Phone>> {
    let vowel_positions: Vec<usize> = phones
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.ph.is_vowel().then_some(i))
        .collect();
    if vowel_positions.len() <= 1 {
        return vec![phones.to_vec()];
    }
    let mut boundaries = Vec::with_capacity(vowel_positions.len() + 1);
    boundaries.push(0usize);
    for w in vowel_positions.windows(2) {
        let (v1, v2) = (w[0], w[1]);
        let cons = &phones[v1 + 1..v2];
        let split_at = match cons.len() {
            0 => v2,
            1 => {
                if cons[0].ph == Ph::NG {
                    v2
                } else {
                    v1 + 1
                }
            }
            2 => {
                if valid_onset_pair(cons[0].ph, cons[1].ph) {
                    v1 + 1
                } else {
                    v1 + 2
                }
            }
            n => {
                if valid_onset_pair(cons[n - 2].ph, cons[n - 1].ph) {
                    v2 - 2
                } else {
                    v2 - 1
                }
            }
        };
        boundaries.push(split_at);
    }
    boundaries.push(phones.len());
    boundaries
        .windows(2)
        .map(|b| phones[b[0]..b[1]].to_vec())
        .collect()
}

fn phones_to_brace_lyric(phones: &[Phone], display: &str) -> String {
    let arpa = phones
        .iter()
        .map(|p| p.arpabet())
        .collect::<Vec<_>>()
        .join(" ");
    format!("{display}{{{arpa}}}")
}

fn expand_syllables_to_fit(chunks: &[String], target_notes: usize, lex: &Lexicon) -> Vec<String> {
    if chunks.len() >= target_notes {
        return chunks.to_vec();
    }
    let mut extra_needed = target_notes - chunks.len();
    let mut out = Vec::with_capacity(target_notes);
    for chunk in chunks {
        if extra_needed == 0 || chunk.contains('-') || matches!(chunk.as_str(), "~" | "_" | "-") {
            out.push(chunk.clone());
            continue;
        }
        let plan = plan::build_plan(chunk, lex);
        let phones: Vec<Phone> = plan
            .clauses
            .iter()
            .flat_map(|c| c.words.iter())
            .flat_map(|w| w.phones.iter().copied())
            .collect();
        let sylls = split_phones_into_syllables(&phones);
        if sylls.len() > 1 {
            let take_splits = (sylls.len() - 1).min(extra_needed);
            let n_parts = take_splits + 1;
            for (k, sy) in sylls.iter().take(n_parts - 1).enumerate() {
                let label = if k == 0 {
                    format!("{chunk}-")
                } else {
                    "-".to_string()
                };
                out.push(phones_to_brace_lyric(sy, &label));
            }
            let rest_phones: Vec<Phone> = sylls[n_parts - 1..].iter().flatten().copied().collect();
            out.push(phones_to_brace_lyric(&rest_phones, "·"));
            extra_needed -= take_splits;
        } else {
            out.push(chunk.clone());
        }
    }
    out
}

/// Parses a multi-part chorus string into a [`ChorusScore`].
///
/// Format:
/// ```text
/// [bpm=100]
/// [part name="Soprano" voice=bright]
/// E5:1=Sing D5:1=with C5:2=joy
/// [part name="Alto" voice=female]
/// C5:1=Sing B4:1=with G4:2=joy
/// [part name="Tenor" voice=male]
/// G4:1=Sing F4:1=with E4:2=joy
/// [part name="Bass" voice=deep]
/// C3:1=Sing G3:1=with C3:2=joy
/// ```
pub fn parse_chorus_score(input: &str, base_cfg: &VoiceConfig) -> Result<ChorusScore, String> {
    parse_chorus_score_with_voices(input, base_cfg, None, None)
}

pub fn parse_chorus_score_with_lex(
    input: &str,
    base_cfg: &VoiceConfig,
    lex: Option<&Lexicon>,
) -> Result<ChorusScore, String> {
    parse_chorus_score_with_voices(input, base_cfg, lex, None)
}

pub fn parse_chorus_score_with_voices(
    input: &str,
    base_cfg: &VoiceConfig,
    lex: Option<&Lexicon>,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<ChorusScore, String> {
    struct RawPartBlock {
        name: String,
        cfg: VoiceConfig,
        gain: f64,
        offset_ms: f64,
        detune_cents: f64,
        lyrics: Option<String>,
        body: String,
    }

    let mut bpm = 108.0;
    let mut global_lyrics: Option<String> = None;
    let mut parts: Vec<RawPartBlock> = Vec::new();
    let mut cur_name = String::from("Lead");
    let mut cur_cfg = base_cfg.clone();
    let mut cur_gain = 1.0f64;
    let mut cur_offset = 0.0f64;
    let mut cur_detune = 0.0f64;
    let mut cur_lyrics: Option<String> = None;
    let mut cur_body = String::new();
    let mut saw_part_tag = false;

    // Strip `//` and `# ` comments line-by-line before scanning `[part ...]` blocks
    let mut stripped_input = String::with_capacity(input.len());
    for line in input.lines() {
        stripped_input.push_str(&normalize_score_line(line));
        stripped_input.push('\n');
    }
    let input = stripped_input.as_str();

    let mut idx = 0;
    let bytes = input.as_bytes();
    while idx < bytes.len() {
        if bytes[idx] == b'[' {
            if let Some(rel_end) = input[idx + 1..].find(']') {
                let end = idx + 1 + rel_end;
                let inner = input[idx + 1..end].trim();
                let low = inner.to_ascii_lowercase();
                if low.starts_with("part ") || low == "part" || low.starts_with("part:") || low.starts_with("track ") {
                    if saw_part_tag && !cur_body.trim().is_empty() {
                        parts.push(RawPartBlock {
                            name: cur_name.clone(),
                            cfg: cur_cfg.clone(),
                            gain: cur_gain,
                            offset_ms: cur_offset,
                            detune_cents: cur_detune,
                            lyrics: cur_lyrics.take(),
                            body: std::mem::take(&mut cur_body),
                        });
                    } else if !saw_part_tag {
                        cur_body.clear();
                    }
                    saw_part_tag = true;
                    cur_name = format!("Part {}", parts.len() + 1);
                    cur_cfg = base_cfg.clone();
                    cur_gain = 1.0;
                    cur_offset = 0.0;
                    cur_detune = 0.0;
                    cur_lyrics = None;

                    let rest = inner
                        .trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == ':')
                        .trim();
                    let mut voice_directives = Vec::new();
                    for tok in split_directive_kv(rest) {
                        if let Some((k, v)) = tok.split_once('=') {
                            let kl = k.trim().to_ascii_lowercase();
                            let vc = v.trim().trim_matches('"');
                            match kl.as_str() {
                                "name" | "label" | "id" => cur_name = vc.to_string(),
                                "gain" | "level" | "mix" => {
                                    if let Ok(g) = vc.parse::<f64>() {
                                        cur_gain = clamp(g, 0.0, 2.5);
                                    }
                                }
                                "offset" | "offsetms" => {
                                    let vs = vc.trim_end_matches("ms");
                                    if let Ok(ms) = vs.parse::<f64>() {
                                        cur_offset = clamp(ms, -120.0, 120.0);
                                    }
                                }
                                "detune" | "cents" => {
                                    if let Ok(c) = vc.parse::<f64>() {
                                        cur_detune = clamp(c, -50.0, 50.0);
                                    }
                                }
                                "bpm" | "tempo" => {
                                    if let Ok(b) = vc.parse::<f64>() {
                                        bpm = clamp(b, 30.0, 320.0);
                                    }
                                }
                                "lyrics" => {
                                    if !vc.trim().is_empty() {
                                        cur_lyrics = Some(vc.trim().to_string());
                                    }
                                }
                                _ => voice_directives.push(tok),
                            }
                        } else {
                            voice_directives.push(tok);
                        }
                    }
                    if !voice_directives.is_empty() {
                        let joined = voice_directives.join(" ");
                        let _ = apply_directive_tokens_with_voices(
                            &joined,
                            &mut cur_cfg,
                            base_cfg,
                            custom_voices,
                        );
                    }
                    idx = end + 1;
                    continue;
                } else if !saw_part_tag {
                    // Global header directive before first [part ...]
                    for tok in split_directive_kv(inner) {
                        if let Some((k, v)) = tok.split_once('=') {
                            let kl = k.trim().to_ascii_lowercase();
                            let vc = v.trim().trim_matches('"');
                            if matches!(kl.as_str(), "bpm" | "tempo") {
                                if let Ok(b) = vc.parse::<f64>() {
                                    bpm = clamp(b, 30.0, 320.0);
                                }
                            } else if kl == "lyrics" && !vc.trim().is_empty() {
                                global_lyrics = Some(vc.trim().to_string());
                            }
                        }
                    }
                }
            }
        }
        let ch = input[idx..].chars().next().unwrap_or(' ');
        cur_body.push(ch);
        idx += ch.len_utf8();
    }

    if !cur_body.trim().is_empty() {
        parts.push(RawPartBlock {
            name: cur_name,
            cfg: cur_cfg,
            gain: cur_gain,
            offset_ms: cur_offset,
            detune_cents: cur_detune,
            lyrics: cur_lyrics,
            body: cur_body,
        });
    }

    let mut out_parts = Vec::with_capacity(parts.len());
    for RawPartBlock {
        name,
        cfg: mut p_cfg,
        gain,
        offset_ms,
        detune_cents,
        lyrics: part_lyrics,
        mut body,
    } in parts
    {
        if let Some(lyr) = part_lyrics.or_else(|| global_lyrics.clone()) {
            let escaped = lyr.replace('"', "");
            body = format!("[lyrics=\"{escaped}\"] {body}");
        }
        let mut sc = parse_sing_score_with_voices(&body, &p_cfg, lex, custom_voices)?;
        if !saw_part_tag {
            bpm = sc.bpm;
        } else if (sc.bpm - 108.0).abs() < 1e-6 {
            sc.bpm = bpm;
        } else {
            bpm = sc.bpm;
        }
        if let Some(ov) = sc.voice.take() {
            p_cfg = ov;
        }
        out_parts.push(ChorusPart {
            name,
            config: p_cfg,
            score: sc,
            gain,
            offset_ms,
            detune_cents,
        });
    }

    Ok(ChorusScore {
        bpm,
        parts: out_parts,
    })
}

fn split_directive_kv(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        let mut quoted = false;
        while i < bytes.len() {
            if bytes[i] == b'"' {
                quoted = !quoted;
            } else if !quoted && (bytes[i].is_ascii_whitespace() || bytes[i] == b',') {
                break;
            }
            i += 1;
        }
        if i > start {
            out.push(s[start..i].to_string());
        }
    }
    out
}

#[derive(Clone, Debug)]
struct PitchNode {
    start_sec: f64,
    end_sec: f64,
    vowel_start_sec: f64,
    hz: Option<f64>,
    slur: bool,
    vibrato_mul: f64,
}

/// Extracts `(display_word, Vec<Phone>)` for a lyric syllable.
fn lyric_to_phones(lyric: &str, lex: &Lexicon) -> (String, Vec<Phone>) {
    let trimmed = lyric.trim();
    // Explicit `{ARPABET}` override, e.g. `"Twin-{T W IH1 NG}"` or `"{AA1}"`
    if let (Some(open), Some(close)) = (trimmed.find('{'), trimmed.rfind('}')) {
        if open < close {
            let disp = trimmed[..open].trim();
            let inside = &trimmed[open + 1..close];
            let mut phs = Vec::new();
            for sym in inside.split_whitespace() {
                if let Some(p) = Phone::parse(sym) {
                    let st = if p.ph.is_vowel() && p.stress == 0 { 1 } else { p.stress };
                    phs.push(Phone::new(p.ph, st));
                }
            }
            if !phs.is_empty() {
                let label = if disp.is_empty() {
                    phs.iter().map(|p| p.ph.ipa()).collect::<String>()
                } else {
                    disp.to_string()
                };
                return (label, phs);
            }
        }
    }

    let clean_word = trimmed.trim_matches('-').trim();
    if clean_word.is_empty() {
        return ("ah".to_string(), vec![Phone::new(Ph::AA, 1)]);
    }

    // Common sung vocables handled crisply so singers don't G2P-guess them oddly
    let vocable: Option<&[Ph]> = match clean_word.to_ascii_lowercase().as_str() {
        "la" | "lah" => Some(&[Ph::L, Ph::AA]),
        "da" | "dah" => Some(&[Ph::D, Ph::AA]),
        "na" | "nah" => Some(&[Ph::N, Ph::AA]),
        "ba" | "bah" => Some(&[Ph::B, Ph::AA]),
        "pa" | "pah" => Some(&[Ph::P, Ph::AA]),
        "ma" | "mah" => Some(&[Ph::M, Ph::AA]),
        "ah" | "aa" | "aah" => Some(&[Ph::AA]),
        "oh" | "ooh" => None,
        "oo" => Some(&[Ph::UW]),
        "ee" => Some(&[Ph::IY]),
        "mm" | "mmm" | "hum" => Some(&[Ph::M]),
        "kle" => Some(&[Ph::K, Ph::AX, Ph::L]),
        "tle" => Some(&[Ph::T, Ph::AX, Ph::L]),
        "ble" => Some(&[Ph::B, Ph::AX, Ph::L]),
        "ple" => Some(&[Ph::P, Ph::AX, Ph::L]),
        "dle" => Some(&[Ph::D, Ph::AX, Ph::L]),
        "gle" => Some(&[Ph::G, Ph::AX, Ph::L]),
        "zel" | "zle" => Some(&[Ph::Z, Ph::AX, Ph::L]),
        "ing" => Some(&[Ph::IH, Ph::NG]),
        "er" => Some(&[Ph::ER]),
        "ly" => Some(&[Ph::L, Ph::IY]),
        "ry" => Some(&[Ph::R, Ph::IY]),
        "ty" => Some(&[Ph::T, Ph::IY]),
        "ny" => Some(&[Ph::N, Ph::IY]),
        "zy" => Some(&[Ph::Z, Ph::IY]),
        _ => None,
    };
    if let Some(v) = vocable {
        let phs = v
            .iter()
            .map(|&b| Phone::new(b, if b.is_vowel() { 1 } else { 0 }))
            .collect();
        return (trimmed.to_string(), phs);
    }

    let plan = plan::build_plan(clean_word, lex);
    let mut phones: Vec<Phone> = plan
        .clauses
        .iter()
        .flat_map(|c| c.words.iter())
        .flat_map(|w| w.phones.iter().copied())
        .collect();
    if phones.is_empty() {
        phones.push(Phone::new(Ph::AA, 1));
    }
    // In singing, even function words like "the", "to", "and", "of" get full sung vowels
    // when sustained on a melody note rather than disappearing into a 30 ms speech schwa.
    for p in &mut phones {
        if p.ph.is_vowel() && p.stress == 0 {
            *p = Phone::new(p.ph, 1);
        }
    }
    (trimmed.to_string(), phones)
}

fn natural_cons_ms(ph: Ph, is_onset: bool) -> f64 {
    match ph {
        Ph::P | Ph::T | Ph::K => if is_onset { 62.0 } else { 48.0 },
        Ph::B | Ph::D | Ph::G => if is_onset { 46.0 } else { 40.0 },
        Ph::CH | Ph::JH => if is_onset { 68.0 } else { 58.0 },
        Ph::S | Ph::SH | Ph::F | Ph::TH => if is_onset { 66.0 } else { 56.0 },
        Ph::Z | Ph::ZH | Ph::V | Ph::DH => if is_onset { 52.0 } else { 46.0 },
        Ph::HH => 48.0,
        Ph::M | Ph::N | Ph::NG => if is_onset { 52.0 } else { 62.0 },
        Ph::L | Ph::R | Ph::W | Ph::Y => if is_onset { 48.0 } else { 52.0 },
        _ => 45.0,
    }
}

fn vowel_spec_for_voice(ph: Ph, cfg: &VoiceConfig) -> (Spec, Option<Spec>) {
    if let Some(dp) = tables::diph(ph) {
        let mut sa = *tables::spec(dp.a).unwrap_or(&tables::SPEC_AA);
        let mut sb = *tables::spec(dp.b).unwrap_or(&tables::SPEC_IH);
        sa.f1 = dp.f1a;
        sa.f2 = dp.f2a;
        sb.f1 = dp.f1b;
        sb.f2 = dp.f2b;
        if let Some(vi) = crate::config::vowel_index(ph) {
            let m = cfg.vowel_shape(vi);
            sa.f1 *= m[0];
            sa.f2 *= m[1];
            sa.f3 *= m[2];
            sb.f1 *= m[0];
            sb.f2 *= m[1];
            sb.f3 *= m[2];
        }
        (sa, Some(sb))
    } else {
        let mut s = *tables::spec(ph).unwrap_or(&tables::SPEC_AA);
        if let Some(vi) = crate::config::vowel_index(ph) {
            let m = cfg.vowel_shape(vi);
            s.f1 *= m[0];
            s.f2 *= m[1];
            s.f3 *= m[2];
        }
        (s, None)
    }
}

fn append_consonant_segs(
    segments: &mut Vec<SingSeg>,
    ph: Ph,
    dur_ms: f64,
    v_ctx: &Spec,
    cfg: &VoiceConfig,
    word_idx: i32,
    is_onset: bool,
) {
    if let Some(st) = tables::stop(ph) {
        let mut close_ms = dur_ms * if st.voiced { 0.62 } else { 0.50 };
        let mut burst_ms = dur_ms * if ph == Ph::CH || ph == Ph::JH { 0.36 } else { 0.20 };
        let mut vot_ms = (dur_ms - close_ms - burst_ms).max(0.0);
        if !is_onset {
            close_ms = dur_ms * 0.68;
            burst_ms = dur_ms * 0.32;
            vot_ms = 0.0;
        }
        let mut sc = *tables::spec(Ph::SIL).unwrap();
        sc.dur = close_ms.max(12.0);
        sc.f1 = st.f1;
        sc.f2 = st.f2 * 0.6 + v_ctx.f2 * 0.4;
        sc.f3 = st.f3 * 0.7 + v_ctx.f3 * 0.3;
        sc.avb = if st.voiced && !cfg.whisper {
            if cfg.gender == Gender::Female { 0.42 } else { 0.54 }
        } else {
            0.0
        };
        sc.jaw = st.jaw;
        sc.t_h = st.t_h;
        sc.t_f = st.t_f;
        sc.t_tip = st.t_tip;
        sc.lip_r = st.lip_r;
        sc.lip_c = st.lip_c;
        segments.push(SingSeg {
            s: sc,
            glide: None,
            phone: ph,
            word: word_idx,
            is_closure: true,
            is_burst: false,
            is_vowel: false,
            dip_hold: 0.80,
        });

        let mut sb = *v_ctx;
        sb.dur = burst_ms.max(6.0);
        sb.av = if st.voiced && is_onset && !cfg.whisper { 0.65 } else { 0.0 };
        sb.af = st.burst_af * cfg.crispness * if is_onset { 0.92 } else { 0.65 };
        sb.ah = if st.voiced { 0.02 } else { 0.09 };
        sb.fric_f1 = st.b_f1;
        sb.fric_b1 = st.b_b1;
        sb.fric_f2 = st.b_f2;
        sb.fric_b2 = st.b_b2;
        let (g1, g2, hp) = match st.place {
            Place::Labial => (0.85, 0.05, 0.0),
            Place::Alveolar => (0.55, 0.55, 2600.0),
            Place::Velar => (0.92, 0.03, 900.0),
            Place::Other => (0.68, 0.50, 1800.0),
        };
        sb.fric_g1 = g1;
        sb.fric_g2 = g2;
        sb.fric_hp = hp;
        sb.fric_lp = crate::timeline::burst_ceiling(st.place, st.b_f1);
        sb.lip_c = st.lip_c * 0.2;
        segments.push(SingSeg {
            s: sb,
            glide: None,
            phone: ph,
            word: word_idx,
            is_closure: false,
            is_burst: true,
            is_vowel: false,
            dip_hold: 0.80,
        });

        if vot_ms >= 6.0 {
            let mut sv = *v_ctx;
            sv.dur = vot_ms;
            sv.av = if st.voiced && !cfg.whisper { 0.85 } else { 0.0 };
            sv.ah = st.vot_ah * 0.82 * cfg.crispness;
            sv.af = 0.0;
            sv.b1 += if st.voiced { 12.0 } else { 165.0 };
            sv.lip_c = 0.0;
            segments.push(SingSeg {
                s: sv,
                glide: None,
                phone: ph,
                word: word_idx,
                is_closure: false,
                is_burst: false,
                is_vowel: false,
                dip_hold: 0.80,
            });
        }
        return;
    }

    let mut s = *tables::spec(ph).unwrap_or(&tables::SPEC_AX);
    s.dur = dur_ms.max(15.0);
    if matches!(ph, Ph::M | Ph::N | Ph::NG) && !cfg.folds {
        s.av = (s.av * 1.28).min(1.0);
    }
    if s.stiff < 0.75 {
        let bl = 1.0 - s.stiff;
        s.f1 = s.f1 * s.stiff + v_ctx.f1 * bl;
        s.f2 = s.f2 * s.stiff + v_ctx.f2 * bl;
        s.f3 = s.f3 * s.stiff + v_ctx.f3 * bl;
    }
    if ph == Ph::DH {
        s.af *= 1.25;
        s.fric_f1 = 3400.0;
        s.fric_b1 = 3600.0;
        s.fric_f2 = 6800.0;
        s.fric_b2 = 3400.0;
        s.fric_g1 = 0.36;
        s.fric_g2 = 0.44;
    }
    segments.push(SingSeg {
        s,
        glide: None,
        phone: ph,
        word: word_idx,
        is_closure: false,
        is_burst: false,
        is_vowel: false,
        dip_hold: 0.80,
    });
}

#[derive(Clone, Debug)]
struct SingSeg {
    s: Spec,
    glide: Option<Spec>,
    phone: Ph,
    word: i32,
    is_closure: bool,
    is_burst: bool,
    is_vowel: bool,
    dip_hold: f64,
}

/// Renders a [`SingScore`] with a single singer's vocal tract into a full [`Speech`] utterance.
pub fn synthesize_singing(score: &SingScore, base_cfg: &VoiceConfig, lex: &Lexicon) -> Speech {
    let (mut speech, _) = synthesize_singing_raw(score, base_cfg, lex, 0.0, 0.0);
    let mut peak_abs = 0.001f64;
    for &v in &speech.samples {
        let a = (v as f64).abs();
        if a > peak_abs {
            peak_abs = a;
        }
    }
    let gain = normalization_gain(peak_abs);
    for s in &mut speech.samples {
        *s = clamp(*s as f64 * gain, -0.96, 0.96) as f32;
    }
    let cfg = score.voice.as_ref().unwrap_or(base_cfg);
    apply_volume(&mut speech.samples, cfg.volume);
    speech
}

/// Internal un-normalized singing renderer (used directly by `synthesize_chorus` so multi-part
/// dynamics balance naturally before master normalization).
pub(crate) fn synthesize_singing_raw(
    score: &SingScore,
    base_cfg: &VoiceConfig,
    lex: &Lexicon,
    lead_offset_ms: f64,
    detune_cents: f64,
) -> (Speech, VoiceConfig) {
    let mut cfg = score.voice.clone().unwrap_or_else(|| base_cfg.clone());
    // Singing voice defaults: richer warmth, gentle natural vibrato if user left speech default,
    // and pleasant studio room tail.
    if cfg.vibrato < 0.22 {
        cfg.vibrato = (cfg.vibrato + 0.20).min(0.42);
    }
    cfg.warmth = (cfg.warmth + 0.08).min(0.88);
    cfg.room = cfg.room.max(0.32);
    cfg.sanitize();

    if score.notes.is_empty() {
        return (
            Speech {
                samples: Vec::new(),
                sample_rate: SAMPLE_RATE,
                text: String::new(),
                ipa: String::new(),
                words: Vec::new(),
                frames: Vec::new(),
            },
            cfg,
        );
    }

    let detune_ratio = 2f64.powf(detune_cents / 1200.0);
    let mut segs: Vec<SingSeg> = Vec::new();
    let mut nodes: Vec<PitchNode> = Vec::new();
    let mut words_text: Vec<String> = Vec::new();
    let mut ipa_parts: Vec<String> = Vec::new();

    // Leading silence (45 ms + any ensemble humanization offset)
    let lead_ms = (45.0 + lead_offset_ms).max(15.0);
    let mut lead_sil = *tables::spec(Ph::SIL).unwrap();
    lead_sil.dur = lead_ms;
    segs.push(SingSeg {
        s: lead_sil,
        glide: None,
        phone: Ph::SIL,
        word: -1,
        is_closure: true,
        is_burst: false,
        is_vowel: false,
        dip_hold: 0.80,
    });
    let mut cursor_sec = (lead_ms / FRAME_MS).round() * FRAME_SEC;

    let mut last_vowel_ph = Ph::AA;
    let mut last_vowel_spec = vowel_spec_for_voice(Ph::AA, &cfg).0;

    for (ni, note) in score.notes.iter().enumerate() {
        let note_ms = note.duration_ms(score.bpm);
        let Some(raw_hz) = note.pitch_hz else {
            // Musical rest (or audible singer's breath intake before next phrase)
            let is_breath = note.lyric.eq_ignore_ascii_case("breath")
                || (note_ms >= 320.0 && cfg.breaths > 0.05 && ni + 1 < score.notes.len());
            if is_breath && !cfg.whisper {
                let b_ms = (note_ms * 0.42).clamp(80.0, 210.0);
                let pre_ms = (note_ms - b_ms - 45.0).max(15.0);
                let post_ms = (note_ms - pre_ms - b_ms).max(20.0);

                let mut s1 = *tables::spec(Ph::SIL).unwrap();
                s1.dur = pre_ms;
                segs.push(SingSeg { s: s1, glide: None, phone: Ph::SIL, word: -1, is_closure: true, is_burst: false, is_vowel: false, dip_hold: 0.8 });

                let mut sb = *tables::spec(Ph::SIL).unwrap();
                sb.dur = b_ms;
                sb.f1 = 510.0;
                sb.f2 = 1490.0;
                sb.f3 = 2480.0;
                sb.b1 = 250.0;
                sb.b2 = 220.0;
                sb.ah = 0.065 * (cfg.breaths.max(0.25) / 0.25).powf(0.85);
                sb.jaw = 0.22;
                segs.push(SingSeg { s: sb, glide: None, phone: Ph::SIL, word: -1, is_closure: false, is_burst: false, is_vowel: false, dip_hold: 0.8 });

                let mut s2 = *tables::spec(Ph::SIL).unwrap();
                s2.dur = post_ms;
                segs.push(SingSeg { s: s2, glide: None, phone: Ph::SIL, word: -1, is_closure: true, is_burst: false, is_vowel: false, dip_hold: 0.8 });
            } else {
                let mut sr = *tables::spec(Ph::SIL).unwrap();
                sr.dur = note_ms;
                segs.push(SingSeg { s: sr, glide: None, phone: Ph::SIL, word: -1, is_closure: true, is_burst: false, is_vowel: false, dip_hold: 0.8 });
            }
            let frames_added = (note_ms / FRAME_MS).round().max(1.0) * FRAME_SEC;
            nodes.push(PitchNode {
                start_sec: cursor_sec,
                end_sec: cursor_sec + frames_added,
                vowel_start_sec: cursor_sec,
                hz: None,
                slur: false,
                vibrato_mul: 0.0,
            });
            cursor_sec += frames_added;
            continue;
        };

        let target_hz = clamp(raw_hz * detune_ratio, 52.0, 960.0);
        let next_is_slur = score
            .notes
            .get(ni + 1)
            .is_some_and(|nx| nx.pitch_hz.is_some() && nx.slur);

        if note.slur {
            // Melisma / tied note: sustain the previous vowel seamlessly onto `target_hz`!
            let word_idx = (words_text.len() as i32) - 1;
            let mut sv = last_vowel_spec;
            sv.dur = note_ms.max(25.0);
            sv.av = (sv.av * note.velocity.clamp(0.25, 1.5)).min(1.15);
            let n_frames = ((sv.dur / FRAME_MS).round() as usize).max(1);
            let dur_sec = n_frames as f64 * FRAME_SEC;
            segs.push(SingSeg {
                s: sv,
                glide: None,
                phone: last_vowel_ph,
                word: word_idx,
                is_closure: false,
                is_burst: false,
                is_vowel: true,
                dip_hold: 0.85,
            });
            nodes.push(PitchNode {
                start_sec: cursor_sec,
                end_sec: cursor_sec + dur_sec,
                vowel_start_sec: cursor_sec,
                hz: Some(target_hz),
                slur: true,
                vibrato_mul: note.vibrato.unwrap_or(1.0),
            });
            cursor_sec += dur_sec;
            continue;
        }

        let (disp_word, phones) = lyric_to_phones(&note.lyric, lex);
        let word_idx = words_text.len() as i32;
        words_text.push(disp_word);
        ipa_parts.push(phones.iter().map(|p| p.ph.ipa()).collect::<String>());

        // Partition syllable phones into `[onset..., primary_vowel, coda...]`
        let first_vowel_pos = phones.iter().position(|p| p.ph.is_vowel());
        let (onset_phs, vowel_ph, mut coda_phs): (&[Phone], Ph, Vec<Phone>) = match first_vowel_pos {
            Some(vp) => {
                let mut v_ph = phones[vp].ph;
                // On sustained sung notes >= 280 ms, open reduced schwa `AX` slightly toward `AH`
                // so held syllables ("the", "a", "-er") sound warm and sung rather than mumbled.
                if v_ph == Ph::AX && note_ms >= 280.0 {
                    v_ph = Ph::AH;
                }
                (&phones[..vp], v_ph, phones[vp + 1..].to_vec())
            }
            None => {
                // Syllabic hum ("mmm"): sustain the sonorant itself!
                let hum = phones.first().map(|p| p.ph).unwrap_or(Ph::M);
                (&[], hum, Vec::new())
            }
        };

        // If the next note is a melisma slur `~`, defer coda consonants until after the melisma!
        if next_is_slur {
            coda_phs.clear();
        }

        let (mut v_spec, mut v_glide) = if vowel_ph.is_vowel() {
            vowel_spec_for_voice(vowel_ph, &cfg)
        } else {
            (*tables::spec(vowel_ph).unwrap_or(&tables::SPEC_M), None)
        };
        if matches!(vowel_ph, Ph::M | Ph::N | Ph::NG) && !cfg.folds {
            v_spec.av = (v_spec.av * 1.28).min(1.05);
        }

        let raw_onset_ms: f64 = onset_phs.iter().map(|p| natural_cons_ms(p.ph, true)).sum();
        let raw_coda_ms: f64 = coda_phs.iter().map(|p| natural_cons_ms(p.ph, false)).sum();
        let max_cons_budget = note_ms * 0.38;
        let raw_cons_sum = raw_onset_ms + raw_coda_ms;
        let cons_scale = if raw_cons_sum > max_cons_budget && raw_cons_sum > 0.0 {
            max_cons_budget / raw_cons_sum
        } else {
            1.0
        };

        let start_note_sec = cursor_sec;
        for p in onset_phs {
            let d = (natural_cons_ms(p.ph, true) * cons_scale).max(16.0);
            let before = segs.len();
            append_consonant_segs(&mut segs, p.ph, d, &v_spec, &cfg, word_idx, true);
            for s in &segs[before..] {
                cursor_sec += ((s.s.dur / FRAME_MS).round().max(1.0)) * FRAME_SEC;
            }
        }
        let vowel_start_sec = cursor_sec;

        let coda_actual_ms: f64 = coda_phs
            .iter()
            .map(|p| (natural_cons_ms(p.ph, false) * cons_scale).max(16.0))
            .sum();
        let elapsed_onset_ms = (vowel_start_sec - start_note_sec) * 1000.0;
        let vowel_ms = (note_ms - elapsed_onset_ms - coda_actual_ms).max(35.0);

        v_spec.dur = vowel_ms;
        v_spec.av = (v_spec.av * note.velocity.clamp(0.25, 1.5)).min(1.15);
        if next_is_slur {
            // Hold the primary diphthong nucleus until the melisma finishes
            v_glide = None;
        }
        last_vowel_ph = vowel_ph;
        last_vowel_spec = v_glide.unwrap_or(v_spec);

        let v_frames = ((vowel_ms / FRAME_MS).round() as usize).max(1);
        cursor_sec += v_frames as f64 * FRAME_SEC;
        segs.push(SingSeg {
            s: v_spec,
            glide: v_glide,
            phone: vowel_ph,
            word: word_idx,
            is_closure: false,
            is_burst: false,
            is_vowel: vowel_ph.is_vowel(),
            dip_hold: if vowel_ms > 280.0 { 0.80 } else { 0.62 },
        });

        let end_ctx = v_glide.unwrap_or(v_spec);
        for p in &coda_phs {
            let d = (natural_cons_ms(p.ph, false) * cons_scale).max(16.0);
            let before = segs.len();
            append_consonant_segs(&mut segs, p.ph, d, &end_ctx, &cfg, word_idx, false);
            for s in &segs[before..] {
                cursor_sec += ((s.s.dur / FRAME_MS).round().max(1.0)) * FRAME_SEC;
            }
        }

        nodes.push(PitchNode {
            start_sec: start_note_sec,
            end_sec: cursor_sec,
            vowel_start_sec,
            hz: Some(target_hz),
            slur: false,
            vibrato_mul: note.vibrato.unwrap_or(1.0),
        });
    }

    // Trailing release silence so studio room & final vowel ring out naturally
    let mut tail_sil = *tables::spec(Ph::SIL).unwrap();
    tail_sil.dur = 120.0;
    tail_sil.f1 = last_vowel_spec.f1;
    tail_sil.f2 = last_vowel_spec.f2;
    tail_sil.f3 = last_vowel_spec.f3;
    segs.push(SingSeg {
        s: tail_sil,
        glide: None,
        phone: Ph::SIL,
        word: -1,
        is_closure: true,
        is_burst: false,
        is_vowel: false,
        dip_hold: 0.80,
    });

    let frames = rasterize_singing_frames(&segs, &nodes, &cfg, score.legato);
    let words = word_timings_from_list(&words_text, &frames);
    let mut renderer = Renderer::new(frames, &cfg);
    renderer.set_bright_source();
    let n_frames = renderer.frames().len();
    let mut samples = vec![0.0f32; n_frames * SAMPLES_PER_FRAME];
    if !samples.is_empty() {
        renderer.render(n_frames, &mut samples);
    }
    let mut rendered_frames = renderer.into_frames();
    let max_rms = rendered_frames.iter().map(|f| f.rms).fold(0.001f64, f64::max);
    for f in &mut rendered_frames {
        f.rms = clamp(f.rms / max_rms, 0.0, 1.0);
    }

    let speech = Speech {
        samples,
        sample_rate: SAMPLE_RATE,
        text: words_text.join(" "),
        ipa: ipa_parts.join(" "),
        words,
        frames: rendered_frames,
    };
    (speech, cfg)
}

fn word_timings_from_list(words_text: &[String], frames: &[Frame]) -> Vec<WordTiming> {
    let mut spans = vec![(f64::INFINITY, 0.0f64); words_text.len()];
    for f in frames {
        if f.word >= 0 {
            if let Some(sp) = spans.get_mut(f.word as usize) {
                sp.0 = sp.0.min(f.t_sec);
                sp.1 = f.t_sec + FRAME_SEC;
            }
        }
    }
    words_text
        .iter()
        .zip(spans)
        .filter(|(_, sp)| sp.0.is_finite())
        .map(|(w, sp)| WordTiming {
            word: w.clone(),
            start: sp.0,
            end: sp.1,
        })
        .collect()
}

fn smoothstep(t: f64) -> f64 {
    let t = clamp(t, 0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn rasterize_singing_frames(
    segs: &[SingSeg],
    nodes: &[PitchNode],
    cfg: &VoiceConfig,
    legato: f64,
) -> Vec<Frame> {
    let female = cfg.gender == Gender::Female;
    let tract_cm = clamp(cfg.tract_length_cm, 12.2, 21.5);
    let formant_scale = 17.5 / tract_cm;
    let f1_scale = (1.0 + (formant_scale - 1.0) * if female { 0.88 } else { 0.75 }) * cfg.formants[0];
    let f2_scale = formant_scale * cfg.formants[1];
    let f3_scale = (1.0 + (formant_scale - 1.0) * 0.90) * cfg.formants[2];
    let breathiness = clamp(cfg.breathiness, 0.0, 0.85);
    let base_nasality = clamp(cfg.nasality, 0.0, 0.80);
    let whisper = cfg.whisper;

    let female_glottal_damp = if female { 34.0 } else { 0.0 };
    let female_b2_damp = if female { 16.0 } else { 0.0 };
    let female_b3_damp = if female { 10.0 } else { 0.0 };
    let sharp = crate::render::formant_sharpness(female);
    let b1_wall = if female { 0.0 } else { 14.0 };

    let mut frames = Vec::new();
    let mut seg_cursor = 0.0f64;

    for (si, seg) in segs.iter().enumerate() {
        let s = &seg.s;
        let n_f = ((s.dur / FRAME_MS).round() as usize).max(1);
        let next_is_vowel_slur = segs.get(si + 1).is_some_and(|nx| nx.is_vowel && seg.is_vowel);
        let prev_was_vowel_slur = si > 0 && segs[si - 1].is_vowel && seg.is_vowel;

        for fi in 0..n_f {
            let u = if n_f > 1 { fi as f64 / (n_f - 1) as f64 } else { 0.5 };
            let g = match seg.glide {
                Some(_) => smoothstep((u - seg.dip_hold) / (1.0 - seg.dip_hold).max(0.05)) * 0.92,
                None => 0.0,
            };
            let mix = |a: f64, pick: fn(&Spec) -> f64| match &seg.glide {
                Some(end) => a + (pick(end) - a) * g,
                None => a,
            };

            let f1 = clamp(mix(s.f1, |e| e.f1) * f1_scale, 200.0, 1180.0);
            let f2 = clamp(mix(s.f2, |e| e.f2) * f2_scale, 540.0, 3250.0);
            let f3 = clamp(mix(s.f3, |e| e.f3) * f3_scale, 1380.0, 4150.0);

            let open_damp = ((f1 - 460.0) * 0.06).max(0.0);
            let prox_damp = ((460.0 - (f2 - f1)) * 0.11).max(0.0);
            let wall_damp = if b1_wall > 0.0 { b1_wall * (450.0 / f1).powi(2) } else { 0.0 };

            let mut seg_av = s.av;
            let mut seg_ah = s.ah;
            let mut seg_af = s.af;
            let mut lax_extra = 0.0;

            if seg.is_vowel {
                // Expressive musical breath-support swell across sustained notes
                let swell = if s.dur >= 320.0 {
                    0.94 + 0.12 * (PI * clamp(u * 0.90 + 0.05, 0.0, 1.0)).sin()
                } else {
                    0.96 + 0.06 * (PI * u).sin()
                };
                seg_av *= swell;
                if !prev_was_vowel_slur && fi < 3 {
                    let atk = (fi as f64 + 1.0) / 3.0;
                    seg_av *= 0.65 + 0.35 * atk;
                    lax_extra = (1.0 - atk) * 0.28;
                }
                if !next_is_vowel_slur && n_f > 6 && fi + 3 >= n_f {
                    let rel = (n_f - fi) as f64 / 3.0;
                    seg_av *= 0.72 + 0.28 * rel;
                    lax_extra = lax_extra.max((1.0 - rel) * 0.32);
                }
            } else if seg.phone == Ph::SIL && s.ah > 0.01 {
                seg_ah = s.ah * (PI * u).sin();
            } else if s.af > 0.02 && !seg.is_burst {
                seg_af *= 0.52 + 0.58 * (PI * u).sin();
            }

            let mut seg_nasal = s.nasal;
            let mut seg_velum = s.velum;
            if seg_av > 0.4 && seg_nasal < base_nasality {
                seg_nasal = base_nasality;
                seg_velum = seg_velum.max(base_nasality * 0.85);
            }

            frames.push(Frame {
                t_sec: seg_cursor,
                clause_idx: 0,
                micro_f0: 0.0,
                f0: cfg.pitch_hz,
                f1,
                f2,
                f3,
                b1: mix(s.b1, |e| e.b1) * (1.0 + breathiness * 0.18)
                    + wall_damp
                    + open_damp
                    + prox_damp
                    + female_glottal_damp,
                // (the renderer narrows these two for speech; a sung vowel keeps its own)
                b2: (mix(s.b2, |e| e.b2) * (1.0 + breathiness * 0.14) + prox_damp * 0.75 + female_b2_damp) / sharp.0,
                b3: (mix(s.b3, |e| e.b3) * (1.0 + breathiness * 0.10) + female_b3_damp) / sharp.1,
                av: if whisper { 0.0 } else { seg_av },
                ah: if whisper {
                    seg_ah.max(seg_av * 0.65)
                } else {
                    seg_ah + seg_av * breathiness * 0.14
                },
                af: seg_af,
                avb: if whisper { 0.0 } else { s.avb },
                nasal: if whisper { 0.0 } else { seg_nasal },
                oral_seal: s.oral_seal,
                fnz: s.fnz * f1_scale,
                lax: lax_extra,
                creak: 0.0,
                fric_f1: clamp(s.fric_f1 * (1.0 + (formant_scale - 1.0) * 0.65), 600.0, 9500.0),
                fric_b1: s.fric_b1 * formant_scale,
                fric_f2: clamp(s.fric_f2 * (1.0 + (formant_scale - 1.0) * 0.60), 1200.0, 10200.0),
                fric_b2: s.fric_b2 * formant_scale,
                fric_g1: s.fric_g1,
                fric_g2: s.fric_g2,
                fric_hp: s.fric_hp * (1.0 + (formant_scale - 1.0) * 0.60),
                fric_lp: s.fric_lp * (1.0 + (formant_scale - 1.0) * 0.60),
                tone_hz: 1500.0,
                tone: 0.0,
                is_closure: seg.is_closure,
                is_burst: seg.is_burst,
                stress: 1,
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
                rms: clamp(seg_av * 0.88 + seg_af * 0.55 + seg_ah * 0.40, 0.0, 1.0),
            });
            seg_cursor += FRAME_SEC;
        }
    }

    if frames.is_empty() {
        return frames;
    }

    // Pass 2: Singing Pitch Contour (legato portamento + subtle onset scoop + bloom vibrato)
    // and Sundberg high-pitch F1/jaw tracking!
    let vib_rate_hz = 5.45 + ((cfg.seed % 11) as f64 - 5.0) * 0.05;
    let mut vib_phase = ((cfg.seed % 97) as f64) / 97.0;
    let base_vib_st = clamp(cfg.vibrato, 0.0, 1.0) * 0.34;
    let first_voiced_hz = nodes.iter().find_map(|n| n.hz).unwrap_or(cfg.pitch_hz);
    let mut cur_log_f0 = first_voiced_hz.ln();
    let mut prev_was_rest = true;
    let mut ni = 0usize;

    for fr in &mut frames {
        while ni + 1 < nodes.len() && fr.t_sec >= nodes[ni].end_sec - 1e-6 {
            if nodes[ni].hz.is_none() {
                prev_was_rest = true;
            }
            ni += 1;
        }
        let node = &nodes[ni];
        if let Some(target_hz) = node.hz {
            let target_log = target_hz.ln();
            let in_note_sec = (fr.t_sec - node.start_sec).max(0.0);
            let in_vowel_sec = (fr.t_sec - node.vowel_start_sec).max(0.0);
            let note_rem_sec = (node.end_sec - fr.t_sec).max(0.0);

            if prev_was_rest && in_note_sec < 0.006 {
                // Fresh phrase entry: start slightly below target (-0.28 st) and scoop cleanly in
                cur_log_f0 = target_log - 0.28 * (std::f64::consts::LN_2 / 12.0);
                prev_was_rest = false;
            }

            let tau = if node.slur {
                (0.022 * legato.max(0.25)).clamp(0.006, 0.055)
            } else {
                (0.014 * legato.max(0.20)).clamp(0.004, 0.038)
            };
            let alpha = 1.0 - (-FRAME_SEC / tau).exp();
            cur_log_f0 += (target_log - cur_log_f0) * alpha;

            // Progressive vibrato: blooms after 120 ms into the vowel, tapers before note change
            let bloom_in = clamp((in_vowel_sec - 0.11) / 0.18, 0.0, 1.0);
            let taper_out = clamp(note_rem_sec / 0.06, 0.25, 1.0);
            let vib_st = base_vib_st * node.vibrato_mul * bloom_in * taper_out;
            vib_phase = (vib_phase + vib_rate_hz * FRAME_SEC).fract();
            let vib_log = vib_st * (TAU * vib_phase).sin() * (std::f64::consts::LN_2 / 12.0);

            let f0 = clamp((cur_log_f0 + vib_log).exp(), 50.0, 960.0);
            fr.f0 = f0;

            // Sundberg high-pitch formant tracking: singers open the jaw so F1 stays >= 1.05 * F0
            // on high notes, keeping soprano/tenor top notes warm and resonant.
            if fr.av > 0.35 && fr.oral_seal < 0.3 && f0 * 1.05 > fr.f1 {
                let lifted_f1 = clamp(f0 * 1.05, fr.f1, 1040.0);
                let extra_open = clamp((lifted_f1 - fr.f1) / 480.0, 0.0, 0.45);
                fr.f1 = lifted_f1;
                fr.jaw = clamp(fr.jaw + extra_open, 0.0, 0.96);
                if fr.f2 < fr.f1 + 190.0 {
                    fr.f2 = fr.f1 + 190.0;
                }
            }
        } else {
            prev_was_rest = true;
            fr.f0 = cur_log_f0.exp();
        }
    }

    // Pass 3: Smooth formant & articulator transitions across frames so legato vowels and
    // consonant boundaries flow organically without clicks.
    let n = frames.len();
    for i in 1..n {
        let (head, tail) = frames.split_at_mut(i);
        let p = &head[i - 1];
        let c = &mut tail[0];
        let a_fmt = if c.is_closure || p.is_closure { 0.48 } else { 0.34 };
        c.f1 = p.f1 + (c.f1 - p.f1) * a_fmt;
        c.f2 = p.f2 + (c.f2 - p.f2) * a_fmt;
        c.f3 = p.f3 + (c.f3 - p.f3) * a_fmt;
        c.fnz = p.fnz + (c.fnz - p.fnz) * 0.35;
        c.jaw = p.jaw + (c.jaw - p.jaw) * 0.24;
        c.t_h = p.t_h + (c.t_h - p.t_h) * 0.24;
        c.t_f = p.t_f + (c.t_f - p.t_f) * 0.24;
        c.t_tip = p.t_tip + (c.t_tip - p.t_tip) * 0.32;
        c.lip_r = p.lip_r + (c.lip_r - p.lip_r) * 0.24;
        c.lip_s = p.lip_s + (c.lip_s - p.lip_s) * 0.24;
        c.lip_c = p.lip_c + (c.lip_c - p.lip_c) * 0.38;
        c.velum = p.velum + (c.velum - p.velum) * 0.26;
        c.av = p.av + (c.av - p.av) * if c.av > p.av { 0.58 } else { 0.54 };
        c.ah = p.ah + (c.ah - p.ah) * 0.50;
        c.lax = p.lax + (c.lax - p.lax) * 0.35;
        c.af = if c.is_burst {
            c.af
        } else {
            p.af + (c.af - p.af) * 0.65
        };
        c.avb = p.avb + (c.avb - p.avb) * 0.55;
        c.nasal = p.nasal + (c.nasal - p.nasal) * 0.72;
        c.oral_seal = p.oral_seal + (c.oral_seal - p.oral_seal) * 0.72;
    }

    frames
}

/// Synthesizes and mixes a multi-voice [`ChorusScore`] into a single cohesive performance,
/// returning both the rendered [`Speech`] (with lead articulation & combined chorus energy)
/// and per-part metadata spans.
pub fn synthesize_chorus(
    chorus: &ChorusScore,
    base_cfg: &VoiceConfig,
    lex: &Lexicon,
) -> (Speech, Vec<SegmentSpan>) {
    if chorus.parts.is_empty() {
        return (
            Speech {
                samples: Vec::new(),
                sample_rate: SAMPLE_RATE,
                text: String::new(),
                ipa: String::new(),
                words: Vec::new(),
                frames: Vec::new(),
            },
            Vec::new(),
        );
    }

    let n_parts = chorus.parts.len();
    let mut rendered: Vec<(Speech, VoiceConfig, f64, String)> = Vec::with_capacity(n_parts);

    // Subtle ensemble humanization when >= 2 voices sing together so unison/harmony parts
    // sound like distinct human singers in a choir rather than phase-locked clones.
    const ENSEMBLE_OFFSETS_MS: [f64; 6] = [0.0, 6.5, -5.0, 9.0, -7.5, 4.0];
    const ENSEMBLE_CENTS: [f64; 6] = [0.0, 2.6, -2.8, 3.8, -3.4, 1.8];

    for (pi, part) in chorus.parts.iter().enumerate() {
        let mut p_cfg = part.config.clone();
        if n_parts > 1 {
            p_cfg.seed = p_cfg.seed.wrapping_add((pi as u32) * 7919 + 17);
        }
        let mut sc = part.score.clone();
        if sc.bpm <= 0.0 {
            sc.bpm = chorus.bpm;
        }
        let auto_off = if n_parts > 1 && part.offset_ms == 0.0 {
            ENSEMBLE_OFFSETS_MS[pi % ENSEMBLE_OFFSETS_MS.len()]
        } else {
            part.offset_ms
        };
        let auto_det = if n_parts > 1 && part.detune_cents == 0.0 {
            ENSEMBLE_CENTS[pi % ENSEMBLE_CENTS.len()]
        } else {
            part.detune_cents
        };
        let (sp, used_cfg) = synthesize_singing_raw(&sc, &p_cfg, lex, auto_off, auto_det);
        rendered.push((sp, used_cfg, part.gain, part.name.clone()));
    }

    let max_samples = rendered.iter().map(|(s, _, _, _)| s.samples.len()).max().unwrap_or(0);
    if max_samples == 0 {
        return (rendered.remove(0).0, Vec::new());
    }

    let mut mixed = vec![0.0f64; max_samples];
    for (sp, _, gain, _) in &rendered {
        let g = *gain;
        for (dst, &src) in mixed.iter_mut().zip(sp.samples.iter()) {
            *dst += src as f64 * g;
        }
    }

    let peak_abs = mixed.iter().fold(0.001f64, |m, &v| m.max(v.abs()));
    let norm = clamp(0.88 / peak_abs, 0.20, 3.2);
    let out_samples: Vec<f32> = mixed
        .into_iter()
        .map(|v| soft_limit(v * norm * base_cfg.volume) as f32)
        .collect();

    // Lead part provides lip-sync visemes, lyric words, and F0 track, while frame `rms`
    // blends lead articulation with the full choir's acoustic envelope!
    let (mut lead, lead_cfg, _, lead_name) = rendered.into_iter().next().unwrap();
    let total_frames = max_samples / SAMPLES_PER_FRAME;
    if lead.frames.len() < total_frames {
        if let Some(last) = lead.frames.last().cloned() {
            while lead.frames.len() < total_frames {
                let mut f = last.clone();
                f.t_sec = lead.frames.len() as f64 / FRAME_RATE as f64;
                f.av = 0.0;
                f.rms = 0.0;
                lead.frames.push(f);
            }
        }
    }
    let mut max_frame_rms = 0.001f64;
    let mut choir_rms = Vec::with_capacity(lead.frames.len());
    for fi in 0..lead.frames.len() {
        let s0 = fi * SAMPLES_PER_FRAME;
        let s1 = ((fi + 1) * SAMPLES_PER_FRAME).min(out_samples.len());
        let rms = if s1 > s0 {
            let sq: f64 = out_samples[s0..s1].iter().map(|&x| (x as f64) * (x as f64)).sum();
            (sq / (s1 - s0) as f64).sqrt()
        } else {
            0.0
        };
        max_frame_rms = max_frame_rms.max(rms);
        choir_rms.push(rms);
    }
    for (f, c_rms) in lead.frames.iter_mut().zip(choir_rms) {
        let norm_c = clamp(c_rms / max_frame_rms, 0.0, 1.0);
        f.rms = clamp(f.rms * 0.65 + norm_c * 0.35, 0.0, 1.0);
    }
    lead.samples = out_samples;

    let dur = lead.duration();
    let span = SegmentSpan {
        start: 0.0,
        end: dur,
        voice: if n_parts > 1 {
            format!("chorus ({n_parts} voices, lead: {lead_name})")
        } else {
            lead_name
        },
        gender: lead_cfg.gender,
        pitch_hz: lead_cfg.pitch_hz,
        tract_length_cm: lead_cfg.tract_length_cm,
        singing: true,
        sound: None,
    };

    (lead, vec![span])
}
