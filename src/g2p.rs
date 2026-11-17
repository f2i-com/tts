//! Letter-to-sound rules for words the lexicon does not know.
//!
//! The tables in `g2p_rules.txt` are derived offline from the CMU Pronouncing Dictionary by
//! `tools/train_g2p.py`: for every letter, the sound it stands for given up to four letters
//! of context on each side (a context is stored only where it changes the answer), and for
//! every word ending, which of the word's full vowels carries the main stress. Here each
//! letter is looked up, longest context first.

use crate::phone::{Ph, Phone};
use std::collections::HashMap;
use std::sync::OnceLock;

const RULES: &str = include_str!("g2p_rules.txt");

/// Context sizes (letters to the left, letters to the right), shortest first.
const LEVELS: [(usize, usize); 9] = [(0, 0), (0, 1), (1, 1), (1, 2), (2, 2), (2, 3), (3, 3), (3, 4), (4, 4)];
/// The same for "this letter as a full vowel".
const FULL_LEVELS: [(usize, usize); 4] = [(0, 0), (0, 1), (1, 1), (1, 2)];
/// Longest word ending the stress table knows.
const SUFFIX_MAX: usize = 6;
const PAD: usize = 4;

struct Tables {
    /// `left.letter.right` (or `+left.letter.right`) -> sounds
    letters: HashMap<&'static str, &'static str>,
    /// `<full vowels>:<ending>` -> index of the main stress among the full vowels, from the end
    stress: HashMap<&'static str, u8>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut letters = HashMap::with_capacity(52_000);
        let mut stress = HashMap::with_capacity(2_000);
        for line in RULES.lines() {
            // (a comment is "# ..."; a key that starts with "#" is a rule for the start of a
            // word - "#.h.i HH" - and there are thousands of them)
            if line.is_empty() || line == "#" || line.starts_with("# ") {
                continue;
            }
            let Some((key, value)) = line.split_once(' ') else { continue };
            if let Some(key) = key.strip_prefix('=') {
                if let Ok(v) = value.trim().parse::<u8>() {
                    stress.insert(key, v);
                }
            } else {
                letters.insert(key, value.trim());
            }
        }
        Tables { letters, stress }
    })
}

/// A sound from the tables: the phone and, for vowels, whether it is a full vowel.
#[derive(Clone, Copy)]
struct Sound {
    ph: Ph,
    vowel: bool,
    full: bool,
}

fn parse_sounds(text: &str, out: &mut Vec<Sound>) {
    for tok in text.split(' ') {
        if tok == "_" || tok.is_empty() {
            continue;
        }
        let (name, digit) = match tok.as_bytes()[tok.len() - 1] {
            d @ (b'0' | b'1') => (&tok[..tok.len() - 1], Some(d)),
            _ => (tok, None),
        };
        if let Some(ph) = Ph::parse_bytes(name.as_bytes()) {
            out.push(Sound { ph, vowel: digit.is_some(), full: digit == Some(b'1') });
        }
    }
}

/// Pronounces a word by rule. Characters other than a-z are ignored.
pub fn pronounce(word: &str) -> Vec<Phone> {
    let w: Vec<u8> = word.bytes().map(|b| b.to_ascii_lowercase()).filter(|b| b.is_ascii_lowercase()).collect();
    let n = w.len();
    if n == 0 {
        return Vec::new();
    }
    let t = tables();
    let mut padded = vec![b'#'; n + 2 * PAD];
    padded[PAD..PAD + n].copy_from_slice(&w);
    let mut key = String::with_capacity(16);
    let make_key = |i: usize, left: usize, right: usize, mark: &str, key: &mut String| {
        key.clear();
        key.push_str(mark);
        let k = i + PAD;
        for &b in &padded[k - left..k] {
            key.push(b as char);
        }
        key.push('.');
        key.push(padded[k] as char);
        key.push('.');
        for &b in &padded[k + 1..k + 1 + right] {
            key.push(b as char);
        }
    };

    // 1. every letter's sounds, longest known context first
    let mut groups: Vec<Vec<Sound>> = Vec::with_capacity(n);
    for i in 0..n {
        let mut sounds = Vec::new();
        for &(left, right) in LEVELS.iter().rev() {
            make_key(i, left, right, "", &mut key);
            if let Some(text) = t.letters.get(key.as_str()) {
                parse_sounds(text, &mut sounds);
                break;
            }
        }
        groups.push(sounds);
    }

    // 2. a run of vowel letters cannot be wholly silent if it has two or more letters
    // ("feebly"), if a silent w follows it (ow, aw and ew always sound: "prowling"), or if it
    // is the word's first and no vowel has sounded before it ("lemanski") - except a final
    // "ue", and a u that only marks a q or g. Its first letter is then read as a full vowel.
    let is_vowel_letter = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u');
    let mut sounded = false;
    let mut i = 0;
    while i < n {
        if !is_vowel_letter(w[i]) {
            sounded |= groups[i].iter().any(|s| s.vowel);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < n && is_vowel_letter(w[j]) {
            j += 1;
        }
        let silent = !groups[i..j].iter().flatten().any(|s| s.vowel);
        // (the vowel of "er", "ur", "aw" or "ow" is listed under the letter after it)
        let carried = j < n && groups[j].iter().any(|s| s.vowel);
        let before_silent_w = j < n && w[j] == b'w' && groups[j].is_empty();
        let marker_u = j - i == 1 && w[i] == b'u' && i > 0 && matches!(w[i - 1], b'q' | b'g');
        let first = !sounded && j < n && w[j] != b'y' && !marker_u && !carried;
        if silent && (j - i >= 2 || before_silent_w || first) && !(j == n && &w[i..j] == b"ue") {
            for &(left, right) in FULL_LEVELS.iter().rev() {
                make_key(i, left, right, "+", &mut key);
                if let Some(text) = t.letters.get(key.as_str()) {
                    groups[i].clear();
                    parse_sounds(text, &mut groups[i]);
                    break;
                }
            }
        }
        sounded |= groups[i..j].iter().flatten().any(|s| s.vowel);
        i = j;
    }

    // 3. a word needs a full vowel: if the rules left none, the first vowel becomes one
    if !groups.iter().flatten().any(|s| s.full) {
        if let Some(i) = groups.iter().position(|g| g.iter().any(|s| s.vowel)) {
            let mut replaced = false;
            for &(left, right) in FULL_LEVELS.iter().rev() {
                make_key(i, left, right, "+", &mut key);
                if let Some(text) = t.letters.get(key.as_str()) {
                    groups[i].clear();
                    parse_sounds(text, &mut groups[i]);
                    replaced = true;
                    break;
                }
            }
            if !replaced {
                for s in groups[i].iter_mut().filter(|s| s.vowel) {
                    s.full = true;
                    if s.ph == Ph::AX {
                        s.ph = Ph::AH;
                    }
                }
            }
        }
    }

    // 4. the main stress, by the word's ending
    let sounds: Vec<Sound> = groups.into_iter().flatten().collect();
    let full: Vec<usize> = sounds.iter().enumerate().filter(|(_, s)| s.full).map(|(k, _)| k).collect();
    let main = match full.len() {
        0 => usize::MAX,
        1 => full[0],
        count => {
            let mut from_end = count - 1;
            let word_str = std::str::from_utf8(&w).unwrap_or("");
            for k in (0..=SUFFIX_MAX.min(n)).rev() {
                key.clear();
                key.push((b'0' + count.min(5) as u8) as char);
                key.push(':');
                key.push_str(&word_str[n - k..]);
                if let Some(&v) = t.stress.get(key.as_str()) {
                    from_end = (v as usize).min(count - 1);
                    break;
                }
            }
            full[count - 1 - from_end]
        }
    };
    sounds
        .iter()
        .enumerate()
        .map(|(k, s)| Phone::new(s.ph, if !s.full { 0 } else if k == main { 1 } else { 2 }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn say(word: &str) -> String {
        pronounce(word).iter().map(|p| p.arpabet()).collect::<Vec<_>>().join(" ")
    }

    #[test]
    fn rules_place_stress_and_reduce_vowels() {
        // words the embedded lexicon does not need to hold: the rules know them
        assert!(say("audacious").ends_with("D EY1 SH AX0 S"), "{}", say("audacious"));
        assert!(say("amended").ends_with("M EH1 N D IH0 D"), "{}", say("amended"));
        assert_eq!(say("absurdity"), "AX0 B S ER1 D AX0 T IY0");
        assert!(say("glorification").ends_with("K EY1 SH AX0 N"), "{}", say("glorification"));
        // one stress per word, and never an empty answer for letters
        for word in ["zxqv", "a", "strengths", "pneumonoultramicroscopic", "glorptastic"] {
            let phones = pronounce(word);
            assert!(!phones.is_empty(), "{word}");
            assert!(phones.iter().filter(|p| p.stress == 1).count() <= 1, "{word}");
        }
        assert!(pronounce("glorp").iter().any(|p| p.stress == 1));
        assert!(say("feebly").starts_with("F IY1 B"), "{}", say("feebly"));
        // the rules for the start of a word ("#.h.i HH") are read, not skipped as comments
        for word in ["hickling", "hiyo", "heston", "hinterberg"] {
            assert!(say(word).starts_with("HH "), "{word}: {}", say(word));
        }
        assert!(pronounce("").is_empty());
        assert!(pronounce("123").is_empty());
    }
}
