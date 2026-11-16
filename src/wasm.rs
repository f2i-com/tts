//! Self-contained WebAssembly ABI + zero-dependency JSON config/score marshaling.
//!
//! When compiled to `wasm32-unknown-unknown`, this module exports a pure C-ABI (`tts_*`)
//! with **zero external imports** and embeds the complete JavaScript `TTS` wrapper
//! (`src/glue.js`) right inside the `.wasm` binary (`tts_glue_ptr` / `tts_glue_len`).

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

use std::collections::HashMap;

use crate::config::{
    vowel_index, Gender, VoiceConfig, NUMBER_SETTINGS, PRESETS, TONE_BANDS, TONE_BANDS_HZ,
    VOWEL_KEYS,
};
use crate::phone::Ph;
use crate::script::{self, ScriptCue, SegmentSpan};
use crate::sing::{self, ChorusPart, ChorusScore, SingNote, SingScore};
use crate::{WordTiming, FRAME_RATE, SAMPLE_RATE, VISEME_FIELDS};

/// Complete JavaScript `TTS` wrapper embedded directly inside `tts.wasm`.
pub const EMBEDDED_JS_GLUE: &str = include_str!("glue.js");

const ACOUSTIC_FIELDS: [&str; 6] = ["f0", "f1", "f2", "f3", "voicing", "noise"];

// ============================================================================
// Minimal, complete RFC 8259 JSON parser & serializer (zero external crates)
// ============================================================================

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonVal {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<JsonVal>),
    Obj(Vec<(String, JsonVal)>),
}

impl JsonVal {
    fn get(&self, key: &str) -> Option<&JsonVal> {
        match self {
            JsonVal::Obj(entries) => entries
                .iter()
                .rev()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v)
                .filter(|v| !matches!(v, JsonVal::Null)),
            _ => None,
        }
    }

    fn as_str(&self) -> Option<&str> {
        match self {
            JsonVal::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    fn as_bool(&self) -> Option<bool> {
        match self {
            JsonVal::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.s.get(self.i).copied()
    }

    fn parse_value(&mut self) -> Result<JsonVal, String> {
        let b = self.peek().ok_or_else(|| "unexpected end of JSON".to_string())?;
        match b {
            b'n' => {
                if self.s.get(self.i..self.i + 4) == Some(b"null") {
                    self.i += 4;
                    Ok(JsonVal::Null)
                } else {
                    Err("invalid JSON literal".into())
                }
            }
            b't' => {
                if self.s.get(self.i..self.i + 4) == Some(b"true") {
                    self.i += 4;
                    Ok(JsonVal::Bool(true))
                } else {
                    Err("invalid JSON literal".into())
                }
            }
            b'f' => {
                if self.s.get(self.i..self.i + 5) == Some(b"false") {
                    self.i += 5;
                    Ok(JsonVal::Bool(false))
                } else {
                    Err("invalid JSON literal".into())
                }
            }
            b'"' => self.parse_string().map(JsonVal::Str),
            b'[' => self.parse_array(),
            b'{' => self.parse_object(),
            b'-' | b'0'..=b'9' => self.parse_number(),
            _ => Err(format!("unexpected JSON character '{}'", b as char)),
        }
    }

    fn parse_string(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.s.get(self.i) != Some(&b'"') {
            return Err("expected JSON string".into());
        }
        self.i += 1;
        let mut out = String::new();
        while self.i < self.s.len() {
            let b = self.s[self.i];
            self.i += 1;
            if b == b'"' {
                return Ok(out);
            }
            if b == b'\\' {
                let esc = *self.s.get(self.i).ok_or("unterminated JSON escape")?;
                self.i += 1;
                match esc {
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'n' => out.push('\n'),
                    b'r' => out.push('\r'),
                    b't' => out.push('\t'),
                    b'u' => {
                        let hex = std::str::from_utf8(
                            self.s.get(self.i..self.i + 4).ok_or("short \\u escape")?,
                        )
                        .map_err(|_| "bad \\u escape")?;
                        self.i += 4;
                        let cp = u32::from_str_radix(hex, 16).map_err(|_| "bad \\u hex")?;
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                    _ => out.push(esc as char),
                }
            } else {
                // Rewind 1 byte and decode full UTF-8 scalar
                let start = self.i - 1;
                let rest = std::str::from_utf8(&self.s[start..]).map_err(|_| "invalid UTF-8")?;
                let ch = rest.chars().next().ok_or("empty UTF-8")?;
                out.push(ch);
                self.i = start + ch.len_utf8();
            }
        }
        Err("unterminated JSON string".into())
    }

    fn parse_number(&mut self) -> Result<JsonVal, String> {
        self.skip_ws();
        let start = self.i;
        while self.i < self.s.len()
            && matches!(
                self.s[self.i],
                b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E'
            )
        {
            self.i += 1;
        }
        let num_str = std::str::from_utf8(&self.s[start..self.i]).map_err(|_| "bad number")?;
        let n: f64 = num_str.parse().map_err(|_| format!("bad number {num_str:?}"))?;
        Ok(JsonVal::Num(n))
    }

    fn parse_array(&mut self) -> Result<JsonVal, String> {
        self.i += 1; // skip '['
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(JsonVal::Arr(items));
        }
        loop {
            items.push(self.parse_value()?);
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(JsonVal::Arr(items));
                }
                _ => return Err("expected ',' or ']' in JSON array".into()),
            }
        }
    }

    fn parse_object(&mut self) -> Result<JsonVal, String> {
        self.i += 1; // skip '{'
        let mut entries = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(JsonVal::Obj(entries));
        }
        loop {
            let key = self.parse_string()?;
            if self.peek() != Some(b':') {
                return Err("expected ':' in JSON object".into());
            }
            self.i += 1;
            let val = self.parse_value()?;
            entries.push((key, val));
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(JsonVal::Obj(entries));
                }
                _ => return Err("expected ',' or '}' in JSON object".into()),
            }
        }
    }
}

pub(crate) fn parse_json(raw: &str) -> Result<JsonVal, String> {
    let mut p = JsonParser {
        s: raw.as_bytes(),
        i: 0,
    };
    let v = p.parse_value()?;
    p.skip_ws();
    if p.i != p.s.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(v)
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

fn push_json_num(out: &mut String, v: f64) {
    if v.is_finite() {
        // Avoid "-0"
        let clean = if v == 0.0 { 0.0 } else { v };
        out.push_str(&clean.to_string());
    } else {
        out.push('0');
    }
}

// ============================================================================
// Options validation, randomizer, and JSON builders
// ============================================================================

fn preset_ids() -> String {
    PRESETS.iter().map(|p| p.id).collect::<Vec<_>>().join(", ")
}

fn apply_preset_with_voices(
    cfg: &mut VoiceConfig,
    id: &str,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(), String> {
    if id.eq_ignore_ascii_case("current")
        || id.eq_ignore_ascii_case("default")
        || id.eq_ignore_ascii_case("base")
    {
        return Ok(());
    }
    let key = id.trim().to_ascii_lowercase().replace('-', "_");
    let mut preset = custom_voices
        .and_then(|m| m.get(&key).cloned())
        .or_else(|| VoiceConfig::preset(id))
        .or_else(|| script::named_voice(id).map(|(v, _)| v))
        .ok_or_else(|| format!("unknown voice {id:?}: the built-in voices are {}", preset_ids()))?;
    preset.volume = cfg.volume;
    preset.variation = cfg.variation;
    preset.seed = cfg.seed;
    preset.folds = cfg.folds;
    *cfg = preset;
    Ok(())
}

fn apply_named_voice_with_voices(
    cfg: &mut VoiceConfig,
    name: &str,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(), String> {
    match Gender::parse(name) {
        Some(gender) => {
            cfg.set_gender(gender);
            Ok(())
        }
        None => apply_preset_with_voices(cfg, name, custom_voices),
    }
}

fn finite_val(value: &JsonVal, key: &str) -> Result<f64, String> {
    match value {
        JsonVal::Num(v) if v.is_finite() => Ok(*v),
        _ => Err(format!("{key} must be a finite number")),
    }
}

fn number_list_val(value: &JsonVal, key: &str, into: &mut [f64]) -> Result<(), String> {
    let JsonVal::Arr(list) = value else {
        return Err(format!("{key} must be an array of numbers"));
    };
    for (i, slot) in into.iter_mut().enumerate() {
        if let Some(v) = list.get(i) {
            if !matches!(v, JsonVal::Null) {
                *slot = finite_val(v, key)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn parse_options_payload(raw: &str) -> Result<JsonVal, String> {
    let t = raw.trim();
    if t.is_empty() {
        return Ok(JsonVal::Null);
    }
    if let Ok(v) = parse_json(t) {
        return Ok(v);
    }
    // Allow unquoted voice names like `female` from non-JS C-ABI callers
    if !t.starts_with('{') && !t.starts_with('[') && !t.starts_with('"') {
        return Ok(JsonVal::Str(t.to_string()));
    }
    Err("invalid voice options JSON".into())
}

/// Atomically validates and applies a voice name or options object onto `cfg`.
pub(crate) fn apply_json_options(
    cfg: &mut VoiceConfig,
    options: &JsonVal,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(), String> {
    match options {
        JsonVal::Null => return Ok(()),
        JsonVal::Str(name) => return apply_named_voice_with_voices(cfg, name, custom_voices),
        JsonVal::Obj(_) => {}
        _ => {
            return Err(
                "voice options must be an object, \"male\", \"female\" or a preset id".into(),
            )
        }
    }

    let mut next = cfg.clone();
    if let Some(v) = options.get("preset") {
        let id = v
            .as_str()
            .ok_or_else(|| "preset must be the id of a built-in voice".to_string())?;
        apply_preset_with_voices(&mut next, id, custom_voices)?;
    }
    if let Some(v) = options.get("gender") {
        let gender = v
            .as_str()
            .and_then(Gender::parse)
            .ok_or_else(|| "gender must be \"male\" or \"female\"".to_string())?;
        if gender != next.gender {
            next.set_gender(gender);
        }
    }

    let JsonVal::Obj(entries) = options else {
        unreachable!()
    };
    for (key, value) in entries {
        if matches!(value, JsonVal::Null) {
            continue;
        }
        if let Some(field) = next.number_mut(key) {
            *field = finite_val(value, key)?;
            continue;
        }
        match key.as_str() {
            "preset" | "gender" | "id" | "label" => {}
            "whisper" => {
                next.whisper = value
                    .as_bool()
                    .ok_or_else(|| "whisper must be true or false".to_string())?;
            }
            "folds" => {
                next.folds = value
                    .as_bool()
                    .ok_or_else(|| "folds must be true or false".to_string())?;
            }
            "seed" => {
                next.seed = finite_val(value, "seed")?.abs() as u64 as u32;
            }
            "tone" => number_list_val(value, "tone", &mut next.tone)?,
            "formants" => number_list_val(value, "formants", &mut next.formants)?,
            "vowels" => {
                let JsonVal::Obj(v_entries) = value else {
                    return Err("vowels must be an object such as { UW: [1, 1.2, 1] }".into());
                };
                for (raw_name, v_arr) in v_entries {
                    let name = raw_name.to_ascii_uppercase();
                    let Some(index) = Ph::parse_bytes(name.as_bytes()).and_then(vowel_index) else {
                        return Err(format!("vowels: {name:?} is not a vowel name"));
                    };
                    number_list_val(v_arr, &format!("vowels.{name}"), &mut next.vowels[index])?;
                }
            }
            _ => {
                return Err(format!(
                    "unknown setting {key:?}; the settings are: preset, gender, {}, whisper, folds, seed, tone, formants, vowels",
                    NUMBER_SETTINGS.join(", ")
                ));
            }
        }
    }
    next.sanitize();
    *cfg = next;
    Ok(())
}

/// Builds a randomized [`VoiceConfig`] while respecting any locked parameters in `options`.
/// Returns `(config, optional_register_name)`.
pub(crate) fn build_randomized_voice(
    seed: u32,
    options: &JsonVal,
    keep_volume: f64,
    keep_folds: bool,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(VoiceConfig, Option<String>), String> {
    match options {
        JsonVal::Null => {
            let mut cfg = VoiceConfig::randomized(seed, None, None);
            cfg.volume = keep_volume;
            cfg.folds = keep_folds;
            Ok((cfg, None))
        }
        JsonVal::Str(s) => {
            if let Some(g) = Gender::parse(s) {
                let mut cfg = VoiceConfig::randomized(seed, Some(g), None);
                cfg.volume = keep_volume;
                cfg.folds = keep_folds;
                Ok((cfg, None))
            } else {
                Err(format!(
                    "randomize string option must be \"male\" or \"female\" (got {s:?})"
                ))
            }
        }
        JsonVal::Obj(entries) => {
            let mut reg_name: Option<String> = None;
            let mut gender_opt: Option<Gender> = None;
            let mut locked_pitch: Option<f64> = None;
            let mut filtered_entries = Vec::with_capacity(entries.len());

            for (k, v) in entries {
                if matches!(v, JsonVal::Null) {
                    continue;
                }
                if k == "name" {
                    let n = v
                        .as_str()
                        .ok_or_else(|| "name must be a string".to_string())?
                        .trim();
                    if n.is_empty() {
                        return Err("name must not be empty".into());
                    }
                    reg_name = Some(n.to_string());
                    continue;
                }
                if k == "gender" {
                    let g = v
                        .as_str()
                        .and_then(Gender::parse)
                        .ok_or_else(|| "gender must be \"male\" or \"female\"".to_string())?;
                    gender_opt = Some(g);
                } else if k == "pitchHz" {
                    locked_pitch = Some(finite_val(v, "pitchHz")?);
                }
                filtered_entries.push((k.clone(), v.clone()));
            }

            let mut cfg = VoiceConfig::randomized(seed, gender_opt, locked_pitch);
            cfg.volume = keep_volume;
            cfg.folds = keep_folds;
            let filtered_obj = JsonVal::Obj(filtered_entries);
            apply_json_options(&mut cfg, &filtered_obj, custom_voices)?;
            Ok((cfg, reg_name))
        }
        _ => Err("randomize options must be an object or \"male\" / \"female\"".into()),
    }
}

pub(crate) fn settings_to_json(cfg: &VoiceConfig, extra_id_label: Option<(&str, &str)>) -> String {
    let mut out = String::with_capacity(512);
    out.push('{');
    let mut first = true;
    let sep = |o: &mut String, f: &mut bool| {
        if !*f {
            o.push(',');
        }
        *f = false;
    };

    if let Some((id, label)) = extra_id_label {
        sep(&mut out, &mut first);
        push_json_str(&mut out, "id");
        out.push(':');
        push_json_str(&mut out, id);
        sep(&mut out, &mut first);
        push_json_str(&mut out, "label");
        out.push(':');
        push_json_str(&mut out, label);
    }

    sep(&mut out, &mut first);
    push_json_str(&mut out, "gender");
    out.push(':');
    push_json_str(&mut out, cfg.gender.as_str());

    let mut numbers = cfg.clone();
    for name in NUMBER_SETTINGS {
        if let Some(v) = numbers.number_mut(name) {
            sep(&mut out, &mut first);
            push_json_str(&mut out, name);
            out.push(':');
            push_json_num(&mut out, *v);
        }
    }

    sep(&mut out, &mut first);
    out.push_str("\"whisper\":");
    out.push_str(if cfg.whisper { "true" } else { "false" });

    sep(&mut out, &mut first);
    out.push_str("\"folds\":");
    out.push_str(if cfg.folds { "true" } else { "false" });

    sep(&mut out, &mut first);
    out.push_str("\"seed\":");
    out.push_str(&cfg.seed.to_string());

    sep(&mut out, &mut first);
    out.push_str("\"tone\":[");
    for (i, &g) in cfg.tone.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_num(&mut out, g);
    }
    out.push(']');

    sep(&mut out, &mut first);
    out.push_str("\"formants\":[");
    for (i, &f) in cfg.formants.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_num(&mut out, f);
    }
    out.push(']');

    sep(&mut out, &mut first);
    out.push_str("\"vowels\":{");
    let mut vf = true;
    for (i, v) in cfg.vowels.iter().enumerate() {
        if v.iter().any(|f| (f - 1.0).abs() > 1e-9) {
            if !vf {
                out.push(',');
            }
            vf = false;
            push_json_str(&mut out, VOWEL_KEYS[i].name());
            out.push_str(":[");
            for (j, & x) in v.iter().enumerate() {
                if j > 0 {
                    out.push(',');
                }
                push_json_num(&mut out, x);
            }
            out.push(']');
        }
    }
    out.push_str("}}");
    out
}

fn meta_json_string() -> String {
    let mut out = String::with_capacity(2048);
    out.push_str("{\"sampleRate\":");
    out.push_str(&SAMPLE_RATE.to_string());
    out.push_str(",\"visemeRate\":");
    out.push_str(&FRAME_RATE.to_string());

    out.push_str(",\"visemeFields\":[");
    for (i, f) in VISEME_FIELDS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, f);
    }
    out.push(']');

    out.push_str(",\"acousticFields\":[");
    for (i, f) in ACOUSTIC_FIELDS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, f);
    }
    out.push(']');

    out.push_str(",\"vowelNames\":[");
    for (i, v) in VOWEL_KEYS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, v.name());
    }
    out.push(']');

    let bands: [f64; TONE_BANDS] = TONE_BANDS_HZ;
    out.push_str(",\"toneBandsHz\":[");
    for (i, &hz) in bands.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_num(&mut out, hz);
    }
    out.push(']');

    let voice_ids = [
        "male", "deep", "husky", "female", "bright", "whisper",
        "woman_a", "woman_b", "man_a", "man_b",
        "soprano", "alto", "tenor", "baritone", "bass_singer",
    ];
    out.push_str(",\"voiceIds\":[");
    for (i, id) in voice_ids.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, id);
    }
    out.push(']');

    let styles = [
        "excited", "happy", "calm", "sad", "serious", "dramatic",
        "urgent", "fast", "slow", "louder", "softer", "whisper",
    ];
    out.push_str(",\"styleNames\":[");
    for (i, s) in styles.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_str(&mut out, s);
    }
    out.push(']');

    out.push_str(",\"presets\":[");
    for (i, p) in PRESETS.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&settings_to_json(&p.config(), Some((p.id, p.label))));
    }
    out.push_str("]}");
    out
}

fn push_words_json(out: &mut String, words: &[WordTiming]) {
    out.push('[');
    for (i, w) in words.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str("{\"word\":");
        push_json_str(out, &w.word);
        out.push_str(",\"start\":");
        push_json_num(out, w.start);
        out.push_str(",\"end\":");
        push_json_num(out, w.end);
        out.push('}');
    }
    out.push(']');
}

fn push_phones_json(out: &mut String, frames: &[crate::Frame]) {
    out.push('[');
    let frame_sec = 1.0 / FRAME_RATE as f64;
    let mut first = true;
    let mut i = 0;
    while i < frames.len() {
        let ph = frames[i].phone;
        if ph == Ph::SIL {
            i += 1;
            continue;
        }
        let word = frames[i].word;
        let start = frames[i].t_sec;
        let mut j = i + 1;
        while j < frames.len() && frames[j].phone == ph && frames[j].word == word {
            j += 1;
        }
        let end = frames[j - 1].t_sec + frame_sec;
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str("{\"phone\":");
        push_json_str(out, ph.name());
        out.push_str(",\"ipa\":");
        push_json_str(out, ph.ipa());
        out.push_str(",\"start\":");
        push_json_num(out, start);
        out.push_str(",\"end\":");
        push_json_num(out, end);
        out.push('}');
        i = j;
    }
    out.push(']');
}

fn speech_meta_json(speech: &crate::Speech, segments: Option<&[SegmentSpan]>) -> String {
    let mut out = String::with_capacity(1024 + speech.words.len() * 48);
    out.push_str("{\"sampleRate\":");
    out.push_str(&speech.sample_rate.to_string());
    out.push_str(",\"duration\":");
    push_json_num(&mut out, speech.duration());
    out.push_str(",\"text\":");
    push_json_str(&mut out, &speech.text);
    out.push_str(",\"ipa\":");
    push_json_str(&mut out, &speech.ipa);
    out.push_str(",\"words\":");
    push_words_json(&mut out, &speech.words);
    out.push_str(",\"phones\":");
    push_phones_json(&mut out, &speech.frames);
    if let Some(segs) = segments {
        out.push_str(",\"segments\":[");
        for (i, s) in segs.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str("{\"start\":");
            push_json_num(&mut out, s.start);
            out.push_str(",\"end\":");
            push_json_num(&mut out, s.end);
            out.push_str(",\"voice\":");
            push_json_str(&mut out, &s.voice);
            out.push_str(",\"gender\":");
            push_json_str(&mut out, s.gender.as_str());
            out.push_str(",\"pitchHz\":");
            push_json_num(&mut out, s.pitch_hz);
            out.push_str(",\"tractLengthCm\":");
            push_json_num(&mut out, s.tract_length_cm);
            out.push_str(",\"singing\":");
            out.push_str(if s.singing { "true" } else { "false" });
            out.push('}');
        }
        out.push(']');
    }
    out.push('}');
    out
}

fn acoustics_vec(frames: &[crate::Frame]) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames.len() * ACOUSTIC_FIELDS.len());
    for f in frames {
        let voicing = (f.av + f.avb * 0.7).clamp(0.0, 1.0);
        let noise = (f.af * 2.2 + f.ah * 1.8).clamp(0.0, 1.0);
        out.extend_from_slice(&[
            f.f0 as f32,
            f.f1 as f32,
            f.f2 as f32,
            f.f3 as f32,
            voicing as f32,
            noise as f32,
        ]);
    }
    out
}

// ============================================================================
// Script, Singing, and Chorus input decoders (DSL strings OR JSON objects)
// ============================================================================

fn apply_cue_voice_fields(
    cfg: &mut VoiceConfig,
    obj: &JsonVal,
    label: &mut String,
    ignore_keys: &[&str],
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(), String> {
    if let Some(v) = obj.get("voice").or_else(|| obj.get("preset")) {
        let id = v
            .as_str()
            .ok_or_else(|| "voice must be a string".to_string())?;
        if !id.eq_ignore_ascii_case("current")
            && !id.eq_ignore_ascii_case("default")
            && !id.eq_ignore_ascii_case("base")
        {
            apply_named_voice_with_voices(cfg, id, custom_voices)?;
            *label = id.to_string();
        }
    }
    if let Some(v) = obj.get("gender") {
        let g = v
            .as_str()
            .and_then(Gender::parse)
            .ok_or_else(|| "gender must be \"male\" or \"female\"".to_string())?;
        if g != cfg.gender {
            cfg.set_gender(g);
        }
        *label = g.as_str().to_string();
    }
    if let Some(v) = obj.get("style") {
        let st = v
            .as_str()
            .ok_or_else(|| "style must be a string".to_string())?;
        if !script::apply_style_preset(cfg, st) {
            return Err(format!("unknown style {st:?}"));
        }
    }
    let JsonVal::Obj(entries) = obj else {
        return Ok(());
    };
    let filtered: Vec<(String, JsonVal)> = entries
        .iter()
        .filter(|(k, _)| {
            !matches!(k.as_str(), "voice" | "preset" | "gender" | "style")
                && !ignore_keys.contains(&k.as_str())
        })
        .cloned()
        .collect();
    apply_json_options(cfg, &JsonVal::Obj(filtered), custom_voices)
}

pub(crate) fn parse_script_payload(
    raw: &str,
    base_cfg: &VoiceConfig,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<Vec<ScriptCue>, String> {
    let trimmed = raw.trim();
    // Check if payload is `{ "__cues": [...] }` (from glue.js) or a JSON array `[{ ... }]`
    let maybe_arr = if trimmed.starts_with('{') {
        parse_json(trimmed)
            .ok()
            .and_then(|v| match v.get("__cues") {
                Some(JsonVal::Arr(a)) => Some(a.clone()),
                _ => None,
            })
    } else if trimmed.starts_with("[{") || trimmed.starts_with("[\"") || trimmed == "[]" {
        parse_json(trimmed).ok().and_then(|v| match v {
            JsonVal::Arr(a) => Some(a),
            _ => None,
        })
    } else {
        None
    };

    let Some(arr) = maybe_arr else {
        return Ok(script::parse_script_with_voices(raw, base_cfg, custom_voices));
    };

    let mut cues = Vec::with_capacity(arr.len());
    let mut cur_cfg = base_cfg.clone();
    let mut cur_label = cur_cfg.gender.as_str().to_string();

    for item in &arr {
        if let Some(s) = item.as_str() {
            cues.extend(script::parse_script_with_voices(s, &cur_cfg, custom_voices));
            continue;
        }
        if !matches!(item, JsonVal::Obj(_)) {
            return Err("each script cue must be an object or string".into());
        }
        apply_cue_voice_fields(
            &mut cur_cfg,
            item,
            &mut cur_label,
            &["text", "pauseMs", "inhale", "sing", "bpm"],
            custom_voices,
        )?;
        if let Some(pause_val) = item.get("pauseMs") {
            cues.push(ScriptCue::Pause {
                duration_ms: finite_val(pause_val, "pauseMs")?.clamp(10.0, 10_000.0),
            });
        }
        if let Some(inh_val) = item.get("inhale") {
            let intensity = if let Some(b) = inh_val.as_bool() {
                if b { 0.55 } else { 0.0 }
            } else {
                finite_val(inh_val, "inhale")?.clamp(0.1, 1.0)
            };
            if intensity > 0.0 {
                cues.push(ScriptCue::Inhale {
                    duration_ms: 110.0 + intensity * 120.0,
                    intensity,
                    config: Box::new(cur_cfg.clone()),
                });
            }
        }
        if let Some(sing_val) = item.get("sing") {
            let sing_str = sing_val
                .as_str()
                .ok_or_else(|| "sing must be a score string".to_string())?;
            let mut sc =
                sing::parse_sing_score_with_voices(sing_str, &cur_cfg, None, custom_voices)?;
            if let Some(bpm_val) = item.get("bpm") {
                sc.bpm = finite_val(bpm_val, "bpm")?.clamp(30.0, 320.0);
            }
            cues.push(ScriptCue::Sing {
                score: Box::new(sc),
                config: Box::new(cur_cfg.clone()),
                voice_label: cur_label.clone(),
            });
        }
        if let Some(text_val) = item.get("text") {
            let t = text_val
                .as_str()
                .ok_or_else(|| "text must be a string".to_string())?;
            if !t.trim().is_empty() {
                cues.push(ScriptCue::Speak {
                    text: t.to_string(),
                    config: Box::new(cur_cfg.clone()),
                    voice_label: cur_label.clone(),
                });
            }
        }
    }
    Ok(cues)
}

fn parse_sing_json_obj(
    obj: &JsonVal,
    default_bpm: Option<f64>,
    base_cfg: &VoiceConfig,
    lex: &crate::Lexicon,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(SingScore, VoiceConfig, String), String> {
    let mut cfg = base_cfg.clone();
    let mut label = cfg.gender.as_str().to_string();
    apply_cue_voice_fields(
        &mut cfg,
        obj,
        &mut label,
        &[
            "bpm",
            "transpose",
            "legato",
            "score",
            "notes",
            "lyrics",
            "name",
            "gain",
            "offsetMs",
            "detuneCents",
        ],
        custom_voices,
    )?;

    let bpm = if let Some(v) = obj.get("bpm") {
        finite_val(v, "bpm")?.clamp(30.0, 320.0)
    } else {
        default_bpm.unwrap_or(108.0)
    };

    let transpose = if let Some(v) = obj.get("transpose") {
        finite_val(v, "transpose")?.clamp(-24.0, 24.0)
    } else {
        0.0
    };

    let legato = if let Some(v) = obj.get("legato") {
        if let Some(b) = v.as_bool() {
            if b { 1.0 } else { 0.15 }
        } else {
            finite_val(v, "legato")?.clamp(0.0, 2.0)
        }
    } else {
        1.0
    };

    let lyrics_prop = obj.get("lyrics").and_then(JsonVal::as_str);

    let mut sc = if let Some(score_v) = obj.get("score") {
        let s = score_v
            .as_str()
            .ok_or_else(|| "score must be a string".to_string())?;
        sing::parse_sing_score_with_voices(s, &cfg, Some(lex), custom_voices)?
    } else if let Some(notes_v) = obj.get("notes") {
        match notes_v {
            JsonVal::Str(notes_str) => {
                if let Some(lyr) = lyrics_prop.filter(|l| !l.trim().is_empty()) {
                    sing::pair_notes_and_lyrics(notes_str, lyr, bpm, lex)?
                } else {
                    sing::parse_sing_score_with_voices(notes_str, &cfg, Some(lex), custom_voices)?
                }
            }
            JsonVal::Arr(arr) => {
                let mut parsed_notes = Vec::with_capacity(arr.len());
                for item in arr {
                    if let Some(tok) = item.as_str() {
                        let sub = sing::parse_sing_score_with_voices(
                            tok,
                            &cfg,
                            Some(lex),
                            custom_voices,
                        )?;
                        parsed_notes.extend(sub.notes);
                        continue;
                    }
                    if !matches!(item, JsonVal::Obj(_)) {
                        return Err("each note in `notes` must be an object or string".into());
                    }
                    if let Some(rest_v) = item.get("rest") {
                        parsed_notes.push(SingNote::rest(finite_val(rest_v, "rest")?.max(0.05)));
                        continue;
                    }
                    let pitch_hz = if let Some(hz_v) = item.get("hz") {
                        let h = finite_val(hz_v, "hz")?;
                        if h <= 0.0 { None } else { Some(h) }
                    } else if let Some(ns) = item.get("note").and_then(JsonVal::as_str) {
                        sing::parse_note_hz(ns)?
                    } else {
                        None
                    };
                    let beats = if let Some(bv) = item.get("beats") {
                        finite_val(bv, "beats")?.max(0.05)
                    } else {
                        1.0
                    };
                    let duration_ms = if let Some(dv) = item.get("durationMs") {
                        Some(finite_val(dv, "durationMs")?.max(25.0))
                    } else {
                        None
                    };
                    let lyric = item
                        .get("lyric")
                        .and_then(JsonVal::as_str)
                        .unwrap_or("la")
                        .to_string();
                    let slur = item
                        .get("slur")
                        .and_then(JsonVal::as_bool)
                        .unwrap_or_else(|| matches!(lyric.trim(), "~" | "-" | "_"));
                    let velocity = if let Some(vv) = item.get("velocity") {
                        finite_val(vv, "velocity")?.clamp(0.1, 2.0)
                    } else {
                        1.0
                    };
                    let vibrato = if let Some(vb) = item.get("vibrato") {
                        Some(finite_val(vb, "vibrato")?.clamp(0.0, 3.0))
                    } else {
                        None
                    };
                    parsed_notes.push(SingNote {
                        pitch_hz,
                        beats,
                        duration_ms,
                        lyric,
                        slur,
                        velocity,
                        vibrato,
                    });
                }
                SingScore {
                    bpm,
                    voice: None,
                    transpose_semitones: 0.0,
                    legato,
                    notes: parsed_notes,
                }
            }
            _ => return Err("notes must be a string or array".into()),
        }
    } else {
        return Err("sing options must include `score` or `notes`".into());
    };

    sc.bpm = bpm;
    sc.legato = legato;
    if transpose.abs() > 1e-6 {
        let mul = 2f64.powf(transpose / 12.0);
        for n in &mut sc.notes {
            if let Some(hz) = &mut n.pitch_hz {
                *hz *= mul;
            }
        }
    }
    if let Some(ov) = sc.voice.take() {
        cfg = ov;
    }
    Ok((sc, cfg, label))
}

pub(crate) fn parse_sing_payload(
    raw: &str,
    base_cfg: &VoiceConfig,
    lex: &crate::Lexicon,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<(SingScore, VoiceConfig, String), String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('{') {
        let obj = parse_json(trimmed)?;
        return parse_sing_json_obj(&obj, None, base_cfg, lex, custom_voices);
    }
    let mut sc = sing::parse_sing_score_with_voices(raw, base_cfg, Some(lex), custom_voices)?;
    let cfg = sc.voice.take().unwrap_or_else(|| base_cfg.clone());
    let lbl = cfg.gender.as_str().to_string();
    Ok((sc, cfg, lbl))
}

pub(crate) fn parse_chorus_payload(
    raw: &str,
    base_cfg: &VoiceConfig,
    lex: &crate::Lexicon,
    custom_voices: Option<&HashMap<String, VoiceConfig>>,
) -> Result<ChorusScore, String> {
    let trimmed = raw.trim();
    if !trimmed.starts_with('{') {
        return sing::parse_chorus_score_with_voices(raw, base_cfg, Some(lex), custom_voices);
    }
    let obj = parse_json(trimmed)?;
    let bpm = if let Some(v) = obj.get("bpm") {
        finite_val(v, "bpm")?.clamp(30.0, 320.0)
    } else {
        108.0
    };
    let Some(JsonVal::Arr(parts_arr)) = obj.get("parts") else {
        return Err("singChorus options must contain a `parts` array".into());
    };
    let mut parts = Vec::with_capacity(parts_arr.len());
    for (idx, item) in parts_arr.iter().enumerate() {
        if let Some(s) = item.as_str() {
            let mut sc = sing::parse_sing_score_with_voices(s, base_cfg, Some(lex), custom_voices)?;
            sc.bpm = bpm;
            let p_cfg = sc.voice.take().unwrap_or_else(|| base_cfg.clone());
            parts.push(ChorusPart {
                name: format!("Part {}", idx + 1),
                config: p_cfg,
                score: sc,
                gain: 1.0,
                offset_ms: 0.0,
                detune_cents: 0.0,
            });
            continue;
        }
        let (sc, p_cfg, _) = parse_sing_json_obj(item, Some(bpm), base_cfg, lex, custom_voices)?;
        let name = item
            .get("name")
            .and_then(JsonVal::as_str)
            .map(String::from)
            .unwrap_or_else(|| format!("Part {}", idx + 1));
        let gain = if let Some(v) = item.get("gain") {
            finite_val(v, "gain")?.clamp(0.0, 2.5)
        } else {
            1.0
        };
        let offset_ms = if let Some(v) = item.get("offsetMs") {
            finite_val(v, "offsetMs")?.clamp(-120.0, 120.0)
        } else {
            0.0
        };
        let detune_cents = if let Some(v) = item.get("detuneCents") {
            finite_val(v, "detuneCents")?.clamp(-50.0, 50.0)
        } else {
            0.0
        };
        parts.push(ChorusPart {
            name,
            config: p_cfg,
            score: sc,
            gain,
            offset_ms,
            detune_cents,
        });
    }
    Ok(ChorusScore { bpm, parts })
}

// ============================================================================
// Zero-Import WebAssembly C-ABI Exports (compiled on `wasm32`)
// ============================================================================

#[cfg(target_arch = "wasm32")]
mod ffi {
    use std::cell::RefCell;

    use super::*;
    use crate::TTS;

    #[derive(Default)]
    struct WasmHost {
        instances: Vec<Option<TTS>>,
        streams: Vec<Option<crate::SpeechStream>>,
        scratch_in: Vec<u8>,
        scratch_f32: Vec<f32>,
        out_samples: Vec<f32>,
        out_visemes: Vec<f32>,
        out_acoustics: Vec<f32>,
        out_bytes: Vec<u8>,
        out_json: String,
    }

    impl WasmHost {
        fn set_err(&mut self, msg: impl Into<String>) -> i32 {
            self.out_json = msg.into();
            -1
        }

        fn read_str(&self, ptr: usize, len: usize) -> Result<String, String> {
            if len == 0 {
                return Ok(String::new());
            }
            let slice = unsafe { std::slice::from_raw_parts(ptr as *const u8, len) };
            std::str::from_utf8(slice)
                .map(String::from)
                .map_err(|_| "input is not valid UTF-8".to_string())
        }

        fn get_inst(&self, handle: i32) -> Result<&TTS, String> {
            if handle <= 0 {
                return Err("invalid TTS handle".into());
            }
            self.instances
                .get((handle - 1) as usize)
                .and_then(Option::as_ref)
                .ok_or_else(|| "TTS instance has been freed".into())
        }

        fn get_inst_mut(&mut self, handle: i32) -> Result<&mut TTS, String> {
            if handle <= 0 {
                return Err("invalid TTS handle".into());
            }
            self.instances
                .get_mut((handle - 1) as usize)
                .and_then(Option::as_mut)
                .ok_or_else(|| "TTS instance has been freed".into())
        }

        fn store_speech(&mut self, speech: crate::Speech, segments: Option<&[SegmentSpan]>) {
            self.out_visemes = speech.visemes();
            self.out_acoustics = acoustics_vec(&speech.frames);
            self.out_json = speech_meta_json(&speech, segments);
            self.out_samples = speech.samples;
        }
    }

    thread_local! {
        static HOST: RefCell<WasmHost> = RefCell::new(WasmHost::default());
    }

    #[no_mangle]
    pub extern "C" fn tts_glue_ptr() -> *const u8 {
        EMBEDDED_JS_GLUE.as_ptr()
    }

    #[no_mangle]
    pub extern "C" fn tts_glue_len() -> usize {
        EMBEDDED_JS_GLUE.len()
    }

    #[no_mangle]
    pub extern "C" fn tts_alloc(len: usize) -> *mut u8 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            h.scratch_in.resize(len, 0);
            h.scratch_in.as_mut_ptr()
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_alloc_f32(len: usize) -> *mut f32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            h.scratch_f32.resize(len, 0.0);
            h.scratch_f32.as_mut_ptr()
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_json_ptr() -> *const u8 {
        HOST.with(|h| h.borrow().out_json.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn tts_json_len() -> usize {
        HOST.with(|h| h.borrow().out_json.len())
    }

    #[no_mangle]
    pub extern "C" fn tts_samples_ptr() -> *const f32 {
        HOST.with(|h| h.borrow().out_samples.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn tts_samples_len() -> usize {
        HOST.with(|h| h.borrow().out_samples.len())
    }

    #[no_mangle]
    pub extern "C" fn tts_visemes_ptr() -> *const f32 {
        HOST.with(|h| h.borrow().out_visemes.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn tts_visemes_len() -> usize {
        HOST.with(|h| h.borrow().out_visemes.len())
    }

    #[no_mangle]
    pub extern "C" fn tts_acoustics_ptr() -> *const f32 {
        HOST.with(|h| h.borrow().out_acoustics.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn tts_acoustics_len() -> usize {
        HOST.with(|h| h.borrow().out_acoustics.len())
    }

    #[no_mangle]
    pub extern "C" fn tts_bytes_ptr() -> *const u8 {
        HOST.with(|h| h.borrow().out_bytes.as_ptr())
    }

    #[no_mangle]
    pub extern "C" fn tts_bytes_len() -> usize {
        HOST.with(|h| h.borrow().out_bytes.len())
    }

    #[no_mangle]
    pub extern "C" fn tts_meta_json() -> i32 {
        HOST.with(|h| {
            h.borrow_mut().out_json = meta_json_string();
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_note_to_hz(ptr: usize, len: usize) -> f64 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let s = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => {
                    h.out_json = e;
                    return -1.0;
                }
            };
            match sing::parse_note_hz(&s) {
                Ok(opt) => opt.unwrap_or(0.0),
                Err(e) => {
                    h.out_json = e;
                    -1.0
                }
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_random_voice(seed: u32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let opts = match parse_options_payload(&raw) {
                Ok(v) => v,
                Err(e) => return h.set_err(e),
            };
            match build_randomized_voice(seed, &opts, 1.0, false, None) {
                Ok((cfg, _)) => {
                    h.out_json = settings_to_json(&cfg, None);
                    0
                }
                Err(e) => h.set_err(e),
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_encode_wav(f32_ptr: usize, f32_len: usize, sample_rate: u32) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let pcm: Vec<f32> = if f32_len == 0 {
                Vec::new()
            } else {
                unsafe { std::slice::from_raw_parts(f32_ptr as *const f32, f32_len) }.to_vec()
            };
            h.out_bytes = crate::wav::encode_wav(&pcm, sample_rate);
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_new(ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let mut cfg = VoiceConfig::male();
            if len > 0 {
                let raw = match h.read_str(ptr, len) {
                    Ok(s) => s,
                    Err(e) => return h.set_err(e),
                };
                let opts = match parse_options_payload(&raw) {
                    Ok(v) => v,
                    Err(e) => return h.set_err(e),
                };
                if let Err(e) = apply_json_options(&mut cfg, &opts, None) {
                    return h.set_err(e);
                }
            }
            let json = settings_to_json(&cfg, None);
            let tts = TTS::new(cfg);
            let slot = h.instances.iter().position(Option::is_none);
            let handle = if let Some(idx) = slot {
                h.instances[idx] = Some(tts);
                (idx + 1) as i32
            } else {
                h.instances.push(Some(tts));
                h.instances.len() as i32
            };
            h.out_json = json;
            handle
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_drop(handle: i32) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            if handle > 0 {
                if let Some(slot) = h.instances.get_mut((handle - 1) as usize) {
                    *slot = None;
                }
            }
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_get_settings(handle: i32) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let json = match h.get_inst(handle) {
                Ok(inst) => settings_to_json(&inst.config, None),
                Err(e) => return h.set_err(e),
            };
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_configure(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let opts = match parse_options_payload(&raw) {
                Ok(v) => v,
                Err(e) => return h.set_err(e),
            };
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            if let Err(e) = apply_json_options(&mut inst.config, &opts, Some(&inst.voices)) {
                return h.set_err(e);
            }
            let json = settings_to_json(&inst.config, None);
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_randomize(handle: i32, seed: u32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let opts = match parse_options_payload(&raw) {
                Ok(v) => v,
                Err(e) => return h.set_err(e),
            };
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            let (cfg, reg_name) = match build_randomized_voice(
                seed,
                &opts,
                inst.config.volume,
                inst.config.folds,
                Some(&inst.voices),
            ) {
                Ok(r) => r,
                Err(e) => return h.set_err(e),
            };
            inst.config = cfg.clone();
            if let Some(name) = reg_name {
                inst.register_voice(&name, cfg);
            }
            let json = settings_to_json(&inst.config, None);
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_register_voice(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let obj = match parse_json(&raw) {
                Ok(v) => v,
                Err(e) => return h.set_err(e),
            };
            let Some(name) = obj.get("name").and_then(JsonVal::as_str).map(str::trim) else {
                return h.set_err("registerVoice requires a `name` string");
            };
            if name.is_empty() {
                return h.set_err("voice name must not be empty");
            }
            let name_owned = name.to_string();
            let voice_val = obj.get("voice").cloned().unwrap_or(JsonVal::Null);
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            let mut cfg = VoiceConfig::male();
            if let Err(e) = apply_json_options(&mut cfg, &voice_val, Some(&inst.voices)) {
                return h.set_err(e);
            }
            let json = settings_to_json(&cfg, None);
            inst.register_voice(&name_owned, cfg);
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_list_voices(handle: i32) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let json = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let mut keys: Vec<&String> = inst.voices.keys().collect();
                keys.sort();
                let mut out = String::from("{");
                for (i, k) in keys.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    push_json_str(&mut out, k);
                    out.push(':');
                    out.push_str(&settings_to_json(&inst.voices[*k], None));
                }
                out.push('}');
                out
            };
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_set_gender(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let Some(gender) = Gender::parse(raw.trim()) else {
                return h.set_err("gender must be \"male\" or \"female\"");
            };
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            inst.config.set_gender(gender);
            let json = settings_to_json(&inst.config, None);
            h.out_json = json;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_speak(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let speech = match h.get_inst(handle) {
                Ok(inst) => inst.speak(&text),
                Err(e) => return h.set_err(e),
            };
            h.store_speech(speech, None);
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_speak_wav(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let wav = match h.get_inst(handle) {
                Ok(inst) => inst.speak(&text).to_wav(),
                Err(e) => return h.set_err(e),
            };
            h.out_bytes = wav;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_speak_script(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let res = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let cues = match parse_script_payload(&raw, &inst.config, Some(&inst.voices)) {
                    Ok(c) => c,
                    Err(e) => return h.set_err(e),
                };
                inst.speak_cues(&cues)
            };
            h.store_speech(res.speech, Some(&res.segments));
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_speak_script_wav(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let wav = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let cues = match parse_script_payload(&raw, &inst.config, Some(&inst.voices)) {
                    Ok(c) => c,
                    Err(e) => return h.set_err(e),
                };
                inst.speak_cues(&cues).speech.to_wav()
            };
            h.out_bytes = wav;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_sing(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let (sung, span) = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let (sc, cfg, lbl) =
                    match parse_sing_payload(&raw, &inst.config, &inst.lexicon, Some(&inst.voices))
                    {
                        Ok(r) => r,
                        Err(e) => return h.set_err(e),
                    };
                let sung = sing::synthesize_singing(&sc, &cfg, &inst.lexicon);
                let span = SegmentSpan {
                    start: 0.0,
                    end: sung.duration(),
                    voice: lbl,
                    gender: cfg.gender,
                    pitch_hz: cfg.pitch_hz,
                    tract_length_cm: cfg.tract_length_cm,
                    singing: true,
                };
                (sung, span)
            };
            h.store_speech(sung, Some(&[span]));
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_sing_wav(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let wav = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let (sc, cfg, _) =
                    match parse_sing_payload(&raw, &inst.config, &inst.lexicon, Some(&inst.voices))
                    {
                        Ok(r) => r,
                        Err(e) => return h.set_err(e),
                    };
                sing::synthesize_singing(&sc, &cfg, &inst.lexicon).to_wav()
            };
            h.out_bytes = wav;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_sing_chorus(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let res = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let parsed = match parse_chorus_payload(
                    &raw,
                    &inst.config,
                    &inst.lexicon,
                    Some(&inst.voices),
                ) {
                    Ok(p) => p,
                    Err(e) => return h.set_err(e),
                };
                inst.sing_chorus_score(&parsed)
            };
            h.store_speech(res.speech, Some(&res.segments));
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_sing_chorus_wav(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let wav = {
                let inst = match h.get_inst(handle) {
                    Ok(i) => i,
                    Err(e) => return h.set_err(e),
                };
                let parsed = match parse_chorus_payload(
                    &raw,
                    &inst.config,
                    &inst.lexicon,
                    Some(&inst.voices),
                ) {
                    Ok(p) => p,
                    Err(e) => return h.set_err(e),
                };
                inst.sing_chorus_score(&parsed).speech.to_wav()
            };
            h.out_bytes = wav;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_stream_new(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let stream = match h.get_inst(handle) {
                Ok(inst) => inst.stream(&text),
                Err(e) => return h.set_err(e),
            };
            let vis = crate::visemes(stream.frames());
            let mut meta = String::with_capacity(256 + stream.words.len() * 48);
            meta.push_str("{\"sampleRate\":");
            meta.push_str(&SAMPLE_RATE.to_string());
            meta.push_str(",\"duration\":");
            push_json_num(&mut meta, stream.duration());
            meta.push_str(",\"text\":");
            push_json_str(&mut meta, &stream.text);
            meta.push_str(",\"ipa\":");
            push_json_str(&mut meta, &stream.ipa);
            meta.push_str(",\"words\":");
            push_words_json(&mut meta, &stream.words);
            meta.push('}');

            let slot = h.streams.iter().position(Option::is_none);
            let sh = if let Some(idx) = slot {
                h.streams[idx] = Some(stream);
                (idx + 1) as i32
            } else {
                h.streams.push(Some(stream));
                h.streams.len() as i32
            };
            h.out_visemes = vis;
            h.out_json = meta;
            sh
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_stream_read(stream_handle: i32, max_samples: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let cap = max_samples.min(1 << 22);
            let mut buf = vec![0.0f32; cap];
            let n = {
                let Some(Some(stream)) = h.streams.get_mut((stream_handle - 1) as usize) else {
                    return h.set_err("SpeechStream has been freed");
                };
                stream.read(&mut buf)
            };
            buf.truncate(n);
            h.out_samples = buf;
            n as i32
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_stream_done(stream_handle: i32) -> i32 {
        HOST.with(|h| {
            let h = h.borrow();
            match h.streams.get((stream_handle - 1) as usize).and_then(Option::as_ref) {
                Some(s) => i32::from(s.is_done()),
                None => 1,
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_stream_position(stream_handle: i32) -> f64 {
        HOST.with(|h| {
            let h = h.borrow();
            match h.streams.get((stream_handle - 1) as usize).and_then(Option::as_ref) {
                Some(s) => s.position(),
                None => 0.0,
            }
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_stream_drop(stream_handle: i32) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            if stream_handle > 0 {
                if let Some(slot) = h.streams.get_mut((stream_handle - 1) as usize) {
                    *slot = None;
                }
            }
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_phonemize(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let ipa = match h.get_inst(handle) {
                Ok(inst) => inst.plan(&text).ipa,
                Err(e) => return h.set_err(e),
            };
            h.out_json = ipa;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_arpabet(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let arpa = match h.get_inst(handle) {
                Ok(inst) => inst.plan(&text).arpabet(),
                Err(e) => return h.set_err(e),
            };
            h.out_json = arpa;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_normalize(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let text = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let norm = match h.get_inst(handle) {
                Ok(inst) => inst.plan(&text).normalized,
                Err(e) => return h.set_err(e),
            };
            h.out_json = norm;
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_add_word(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let Some((word, phones)) = raw.split_once('\t') else {
                return h.set_err("expected word<TAB>phones");
            };
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            if let Err(e) = inst.lexicon.add_word(word, phones) {
                return h.set_err(e);
            }
            0
        })
    }

    #[no_mangle]
    pub extern "C" fn tts_load_dictionary(handle: i32, ptr: usize, len: usize) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            let raw = match h.read_str(ptr, len) {
                Ok(s) => s,
                Err(e) => return h.set_err(e),
            };
            let inst = match h.get_inst_mut(handle) {
                Ok(i) => i,
                Err(e) => return h.set_err(e),
            };
            inst.lexicon.load_dictionary(&raw) as i32
        })
    }
}
