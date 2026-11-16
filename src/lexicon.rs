//! Pronouncing lexicon, morphological fallback and letter-to-sound rules.
//!
//! Lookup order for a word:
//! 1. words added at runtime with [`Lexicon::add_word`] and the hand-tuned core lexicon,
//! 2. the embedded common-word lexicon and any dictionary loaded with
//!    [`Lexicon::load_dictionary`],
//! 3. suffix stripping against (1) and (2) (`-s`, `-ed`, `-ing`, `-ly`, …),
//! 4. spelling out (single letters, vowel-less tokens such as "tv" or "html"),
//! 5. context-sensitive letter-to-sound rules.

use crate::phone::{Ph, Phone};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

/// Hand-tuned core lexicon carried over from the SoftN Voice Simulator.
const CORE: &str = include_str!("lexicon_builtin.txt");
/// Words the normalizer emits, letter names, heteronym defaults.
const EXTRA: &str = include_str!("lexicon_extra.txt");
/// Frequent English words that the letter-to-sound rules (`g2p.rs`) get wrong, plus the
/// short ones; together with the rules this pronounces the ~50,000 most frequent words exactly
/// as CMUdict does (derived from CMUdict, see lexicon_common.LICENSE; `tools/build_lexicon.py`).
const COMMON: &str = include_str!("lexicon_common.txt");

type Map = HashMap<Box<str>, Box<[Phone]>>;

#[derive(Clone)]
pub struct Lexicon {
    /// Words taught at runtime; they win over everything, including the normalizer's rewrites.
    user: Map,
    /// The built-in words: shared by every lexicon, copied only by one that adds to them.
    primary: Arc<Map>,
    secondary: Arc<Map>,
}

impl Default for Lexicon {
    fn default() -> Self {
        Lexicon::new()
    }
}

fn valid_word(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'\'')
}

/// Parses one `word PH PH PH` line (CMUdict style). Alternate pronunciations
/// (`word(2) …`), comments and unparseable lines yield `None`.
fn parse_entry(line: &str, cmu_schwa: bool) -> Option<(String, Vec<Phone>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(";;;") {
        return None;
    }
    let line = match line.find(" #") {
        Some(i) => &line[..i],
        None => line,
    };
    let mut it = line.split_whitespace();
    let word = it.next()?;
    if word.ends_with(')') {
        return None;
    }
    let word = word.to_ascii_lowercase();
    if !valid_word(&word) {
        return None;
    }
    let mut phones = Vec::with_capacity(8);
    for tok in it {
        let mut p = Phone::parse(tok)?;
        // CMUdict writes the reduced vowel (schwa) as AH0; this engine has a dedicated AX.
        if cmu_schwa && p.ph == Ph::AH && p.stress == 0 {
            p.ph = Ph::AX;
        }
        phones.push(p);
    }
    if phones.is_empty() {
        None
    } else {
        Some((word, phones))
    }
}

fn load_lines(map: &mut Map, text: &str, cmu_schwa: bool) -> usize {
    let mut count = 0;
    for line in text.lines() {
        if let Some((word, phones)) = parse_entry(line, cmu_schwa) {
            map.insert(word.into_boxed_str(), phones.into_boxed_slice());
            count += 1;
        }
    }
    count
}

impl Lexicon {
    /// Core + supplement + embedded common-word lexicon.
    pub fn new() -> Lexicon {
        static BUILT_IN: OnceLock<Lexicon> = OnceLock::new();
        BUILT_IN
            .get_or_init(|| {
                let mut lex = Lexicon::base();
                let common = Arc::make_mut(&mut lex.secondary);
                common.reserve(22_000);
                load_lines(common, COMMON, false);
                lex
            })
            .clone()
    }

    /// Core + supplement, without the embedded common-word lexicon (development tooling).
    pub fn base() -> Lexicon {
        let mut lex = Lexicon::core_only();
        load_lines(Arc::make_mut(&mut lex.primary), EXTRA, false);
        lex
    }

    /// Loads entries written in the lexicon's own notation (`AX` for the reduced vowel).
    #[doc(hidden)]
    pub fn load_entries(&mut self, text: &str) -> usize {
        load_lines(Arc::make_mut(&mut self.secondary), text, false)
    }

    /// Only the hand-tuned core lexicon (the SoftN reference behaviour).
    pub fn core_only() -> Lexicon {
        let mut primary = Map::with_capacity(1024);
        load_lines(&mut primary, CORE, false);
        Lexicon {
            user: Map::new(),
            primary: Arc::new(primary),
            secondary: Arc::new(Map::new()),
        }
    }

    pub fn len(&self) -> usize {
        self.user.len() + self.primary.len() + self.secondary.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, word: &str) -> Option<&[Phone]> {
        self.user
            .get(word)
            .or_else(|| self.primary.get(word))
            .or_else(|| self.secondary.get(word))
            .map(|p| &**p)
    }

    /// True for words added with [`Lexicon::add_word`].
    pub fn is_user_word(&self, word: &str) -> bool {
        self.user.contains_key(word)
    }

    pub fn contains(&self, word: &str) -> bool {
        self.get(word).is_some()
    }

    /// Adds or overrides one pronunciation, e.g. `add_word("softn", "S AO1 F T EH1 N")`.
    pub fn add_word(&mut self, word: &str, phones: &str) -> Result<(), String> {
        let word = word.trim().to_ascii_lowercase();
        if !valid_word(&word) {
            return Err(format!(
                "word {word:?} must consist of letters, digits or apostrophes"
            ));
        }
        let parsed = crate::phone::parse_phones(phones)
            .ok_or_else(|| format!("could not parse ARPAbet phones {phones:?}"))?;
        self.user
            .insert(word.into_boxed_str(), parsed.into_boxed_slice());
        Ok(())
    }

    /// Loads a pronunciation dictionary in CMUdict format (`word PH PH …` per line).
    /// Entries fill in behind the core lexicon; returns how many were loaded.
    pub fn load_dictionary(&mut self, text: &str) -> usize {
        load_lines(Arc::make_mut(&mut self.secondary), text, true)
    }

    /// Direct lookup, then inflection stripping against known stems.
    ///
    /// The stripping follows English spelling: "taking" is take + ing (a single consonant
    /// after a single vowel means the base ends in a silent e), "stopping" is stop + ing,
    /// "times" is time + s and never "tim" + es. A derivation is only accepted when its base
    /// is a listed word; everything else is left to the letter-to-sound rules.
    pub fn lookup_with_morphology(&self, word: &str) -> Option<Vec<Phone>> {
        self.derive(word, 2)
    }

    fn derive(&self, word: &str, depth: u8) -> Option<Vec<Phone>> {
        if let Some(p) = self.get(word) {
            return Some(p.to_vec());
        }
        if !word.is_ascii() || depth == 0 {
            return None;
        }
        let n = word.len();
        let get = |w: &str| self.get(w).map(|p| p.to_vec());
        let is_vowel = |c: u8| matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y');
        let ph = |p: Ph| Phone::new(p, 0);
        let push_s = |p: &mut Vec<Phone>| {
            let last = p[p.len() - 1].ph;
            if last.is_sibilant() {
                p.push(ph(Ph::IH));
                p.push(ph(Ph::Z));
            } else {
                p.push(ph(if last.is_voiced() { Ph::Z } else { Ph::S }));
            }
        };
        // The word a stem stands for once -ing / -ed / -er / -est is taken off.
        let base_of = |stem: &str| -> Option<Vec<Phone>> {
            let b = stem.as_bytes();
            let m = b.len();
            if m < 2 || !b.iter().any(|&c| is_vowel(c)) {
                return None;
            }
            if m < 3 {
                return get(&format!("{stem}e")); // us-ed, di-ed
            }
            let last = b[m - 1];
            // carri-ed, happi-er
            if last == b'i' {
                return get(&format!("{}y", &stem[..m - 1]));
            }
            // stopp-ing is stop, but add-ing is add and tell-ing is tell
            if last == b[m - 2] && !is_vowel(last) {
                return get(stem).or_else(|| get(&stem[..m - 1]));
            }
            let groups = b.iter().zip(std::iter::once(&b'#').chain(b.iter())).filter(|(&c, &prev)| is_vowel(c) && !is_vowel(prev)).count();
            let light = !is_vowel(last) && !matches!(last, b'w' | b'x') && is_vowel(b[m - 2]) && !is_vowel(b[m - 3]);
            if light {
                // tak-ing, nam-ed, lat-er: one vowel letter and one consonant before the
                // ending mean a silent e; only a longer stem can also stand alone (visit-ing)
                let with_e = get(&format!("{stem}e"));
                return if groups <= 1 { with_e } else { with_e.or_else(|| get(stem)) };
            }
            get(stem).or_else(|| get(&format!("{stem}e")))
        };

        // Plural / 3rd person: -ies, -s, and -es after a hissing sound or o
        if n > 4 && word.ends_with("ies") {
            if let Some(mut p) = get(&format!("{}y", &word[..n - 3])) {
                p.push(ph(Ph::Z));
                return Some(p);
            }
        }
        if n > 2 && word.ends_with('s') && !word.ends_with("ss") && !word.ends_with("us") && !word.ends_with("is") {
            if let Some(mut p) = self.derive(&word[..n - 1], depth - 1) {
                push_s(&mut p);
                return Some(p);
            }
            if n > 3 && word.ends_with("es") {
                let stem = &word[..n - 2];
                if ["s", "x", "z", "ch", "sh", "o"].iter().any(|e| stem.ends_with(e)) {
                    if let Some(mut p) = get(stem) {
                        push_s(&mut p);
                        return Some(p);
                    }
                }
            }
        }

        // Progressive -ing
        if n > 4 && word.ends_with("ing") {
            if let Some(mut p) = base_of(&word[..n - 3]) {
                p.push(ph(Ph::IH));
                p.push(ph(Ph::NG));
                return Some(p);
            }
        }

        // Past -ed
        if n > 3 && word.ends_with("ed") {
            if let Some(mut p) = base_of(&word[..n - 2]) {
                let last = p[p.len() - 1].ph;
                if last == Ph::T || last == Ph::D {
                    p.push(ph(Ph::IH));
                    p.push(ph(Ph::D));
                } else {
                    p.push(ph(if last.is_voiced() { Ph::D } else { Ph::T }));
                }
                return Some(p);
            }
        }

        // Adverb -ly: real-ly and full-y have one l; happi-ly; basic-ally
        if n > 4 && word.ends_with("ly") {
            let stem = &word[..n - 2];
            if word.ends_with("lly") {
                if let Some(mut p) = get(&word[..n - 1]).or_else(|| get(stem)) {
                    p.push(ph(Ph::IY));
                    return Some(p);
                }
            }
            if n > 6 && word.ends_with("ically") {
                if let Some(mut p) = get(&word[..n - 4]) {
                    p.push(ph(Ph::L));
                    p.push(ph(Ph::IY));
                    return Some(p);
                }
            }
            if let Some(root) = stem.strip_suffix('i') {
                if let Some(mut p) = get(&format!("{root}y")) {
                    if p.last().map(|x| x.ph) == Some(Ph::IY) {
                        p.pop();
                        p.extend([ph(Ph::AX), ph(Ph::L), ph(Ph::IY)]);
                        return Some(p);
                    }
                }
            } else if let Some(mut p) = self.derive(stem, depth - 1) {
                p.push(ph(Ph::L));
                p.push(ph(Ph::IY));
                return Some(p);
            }
        }

        // Comparative -er / superlative -est
        if n > 4 && word.ends_with("er") {
            if let Some(mut p) = base_of(&word[..n - 2]) {
                p.push(ph(Ph::ER));
                return Some(p);
            }
        }
        if n > 5 && word.ends_with("est") {
            if let Some(mut p) = base_of(&word[..n - 3]) {
                p.extend([ph(Ph::AX), ph(Ph::S), ph(Ph::T)]);
                return Some(p);
            }
        }

        // Derivational -ment / -ness / -less / -ful
        let suffixes: [(&str, usize, &[Ph]); 4] = [
            ("ment", 5, &[Ph::M, Ph::AX, Ph::N, Ph::T]),
            ("ness", 5, &[Ph::N, Ph::AX, Ph::S]),
            ("less", 5, &[Ph::L, Ph::AX, Ph::S]),
            ("ful", 5, &[Ph::F, Ph::AX, Ph::L]),
        ];
        for (suffix, min_len, tail) in suffixes {
            if n > min_len && word.ends_with(suffix) {
                let stem = &word[..n - suffix.len()];
                let base = get(stem).or_else(|| {
                    // happi-ness, beauti-ful
                    if let Some(root) = stem.strip_suffix('i') {
                        get(&format!("{root}y"))
                    } else {
                        None
                    }
                });
                if let Some(mut p) = base {
                    p.extend(tail.iter().map(|&t| ph(t)));
                    return Some(p);
                }
            }
        }

        None
    }

    /// Best pronunciation for any token made of `[a-z0-9]`.
    pub fn pronounce(&self, word: &str) -> Vec<Phone> {
        if let Some(p) = self.lookup_with_morphology(word) {
            return p;
        }
        if let Some(p) = spell_out(word) {
            return p;
        }
        crate::g2p::pronounce(word)
    }
}

/// Spoken names of the letters a–z.
const LETTER_PHONES: [&str; 26] = [
    "EY1", "B IY1", "S IY1", "D IY1", "IY1", "EH1 F", "JH IY1", "EY1 CH", "AY1", "JH EY1", "K EY1",
    "EH1 L", "EH1 M", "EH1 N", "OW1", "P IY1", "K Y UW1", "AA1 R", "EH1 S", "T IY1", "Y UW1",
    "V IY1", "D AH1 B AX L Y UW0", "EH1 K S", "W AY1", "Z IY1",
];

/// Letter names as words, for the normalizer ("USA" -> "you ess ay").
pub const LETTER_WORDS: [&str; 26] = [
    "ay", "bee", "see", "dee", "ee", "eff", "gee", "aitch", "eye", "jay", "kay", "ell", "em", "en",
    "oh", "pee", "cue", "ar", "ess", "tee", "you", "vee", "double you", "ex", "why", "zee",
];

fn is_letter_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'e' | b'i' | b'o' | b'u' | b'y')
}

/// Single letters and short vowel-less tokens ("b", "tv", "html") are read letter by letter,
/// with the main stress on the last letter as in spoken initialisms.
fn spell_out(word: &str) -> Option<Vec<Phone>> {
    let b = word.as_bytes();
    if b.is_empty() || b.len() > 5 || !b.iter().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    if b.len() > 1 && b.iter().any(|&c| is_letter_vowel(c)) {
        return None;
    }
    let mut out = Vec::new();
    for (i, &c) in b.iter().enumerate() {
        let last = i + 1 == b.len();
        for mut p in crate::phone::parse_phones(LETTER_PHONES[(c - b'a') as usize])? {
            if !last && p.stress == 1 {
                p.stress = 2;
            }
            out.push(p);
        }
    }
    Some(out)
}
