//! Utterance planning: normalized text -> clauses -> words -> stressed phones.

use crate::lexicon::Lexicon;
use crate::phone::{Ph, Phone};
use crate::text::normalize;

#[derive(Clone, Debug)]
pub struct PlanWord {
    pub text: String,
    pub phones: Vec<Phone>,
    pub ipa: String,
    /// Content words carry pitch accents; non-final function words are destressed.
    pub is_content: bool,
    pub is_clause_final: bool,
}

#[derive(Clone, Debug)]
pub struct Clause {
    pub words: Vec<PlanWord>,
    pub punc: char,
    pub is_question: bool,
    pub is_yes_no_question: bool,
    pub is_exclaim: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Plan {
    pub normalized: String,
    pub clauses: Vec<Clause>,
    pub ipa: String,
}

impl Plan {
    pub fn word_count(&self) -> usize {
        self.clauses.iter().map(|c| c.words.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.word_count() == 0
    }

    /// ARPAbet transcription, words separated by " | ".
    pub fn arpabet(&self) -> String {
        let mut parts = Vec::new();
        for c in &self.clauses {
            for w in &c.words {
                parts.push(
                    w.phones
                        .iter()
                        .map(|p| p.arpabet())
                        .collect::<Vec<_>>()
                        .join(" "),
                );
            }
        }
        parts.join(" | ")
    }
}

const FUNCTION_WORDS: [&str; 35] = [
    "a", "an", "the", "to", "of", "in", "on", "at", "for", "by", "with", "and", "or", "but", "if",
    "as", "is", "am", "are", "was", "were", "be", "it", "its", "my", "your", "his", "her", "our",
    "their", "that", "from", "than", "them", "can",
];

/// Weak forms: in running speech these little words are not pronounced the way a dictionary
/// lists them - their vowel shrinks to a schwa ("and" -> "'nd", "for" -> "fer", "can" -> "c'n").
/// Saying the full form every time is one of the things that makes a voice sound like it is
/// reading a word list. Only used when the word is not the last one before a pause.
fn weak_form(word: &str, next_starts_vowel: bool) -> Option<Vec<Phone>> {
    let p = |list: &[Ph]| Some(list.iter().map(|&x| ph(x)).collect());
    match word {
        "the" => p(&[Ph::DH, if next_starts_vowel { Ph::IY } else { Ph::AX }]),
        "a" => p(&[Ph::AX]),
        "an" => p(&[Ph::AX, Ph::N]),
        "to" => p(&[Ph::T, if next_starts_vowel { Ph::UW } else { Ph::AX }]),
        "of" => p(&[Ph::AX, Ph::V]),
        "and" => p(&[Ph::AX, Ph::N, Ph::D]),
        "for" => p(&[Ph::F, Ph::ER]),
        "at" => p(&[Ph::AX, Ph::T]),
        "from" => p(&[Ph::F, Ph::R, Ph::AX, Ph::M]),
        "as" => p(&[Ph::AX, Ph::Z]),
        "or" => p(&[Ph::ER]),
        "are" => p(&[Ph::ER]),
        "was" => p(&[Ph::W, Ph::AX, Ph::Z]),
        "were" => p(&[Ph::W, Ph::ER]),
        "can" => p(&[Ph::K, Ph::AX, Ph::N]),
        "your" => p(&[Ph::Y, Ph::ER]),
        "but" => p(&[Ph::B, Ph::AX, Ph::T]),
        "than" => p(&[Ph::DH, Ph::AX, Ph::N]),
        "them" => p(&[Ph::DH, Ph::AX, Ph::M]),
        _ => None,
    }
}

/// Interrogative words that mark a falling wh-question rather than a rising yes/no or echo question.
fn is_wh_word(w: &str) -> bool {
    let base = w.split('\'').next().unwrap_or(w);
    matches!(base, "who" | "whom" | "whose" | "what" | "which" | "where" | "when" | "why" | "how")
}

fn is_punct(c: char) -> bool {
    matches!(c, '.' | '!' | '?' | ',' | ';' | ':')
}

fn is_tok_char(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit() || c == '\''
}

fn ph(p: Ph) -> Phone {
    Phone::new(p, 0)
}

/// Pronounces a piece that may contain apostrophes ("we'd", "dog's", "would've").
/// Known contractions are single lexicon entries; otherwise the clitic is attached
/// to its host so the result stays one fluid phonological word.
fn pronounce_apostrophe(piece: &str, lex: &Lexicon) -> Option<Vec<Phone>> {
    if let Some(p) = lex.get(piece) {
        return Some(p.to_vec());
    }
    let cut = piece.rfind('\'')?;
    let (base, suffix) = (&piece[..cut], &piece[cut + 1..]);
    if base.is_empty() {
        return None;
    }
    // O'Brien, O'Neil: the O belongs to the name
    if base == "o" && suffix.len() >= 3 {
        let mut p = vec![Phone::new(Ph::OW, 0)];
        p.extend(lex.pronounce(suffix));
        return Some(p);
    }
    let host = |b: &str| -> Vec<Phone> {
        // (one level per apostrophe: a long chain of them is not a word)
        if b.contains('\'') && b.matches('\'').count() <= 3 {
            pronounce_apostrophe(b, lex).unwrap_or_else(|| lex.pronounce(&b.replace('\'', "")))
        } else {
            lex.pronounce(b)
        }
    };
    let mut p;
    match suffix {
        "s" => {
            p = host(base);
            let last = p.last()?.ph;
            if last.is_sibilant() {
                p.push(ph(Ph::IH));
                p.push(ph(Ph::Z));
            } else {
                p.push(ph(if last.is_voiced() { Ph::Z } else { Ph::S }));
            }
        }
        "d" => {
            p = host(base);
            let last = p.last()?.ph;
            if last == Ph::T || last == Ph::D {
                p.push(ph(Ph::IH));
            }
            p.push(ph(Ph::D));
        }
        "ll" => {
            p = host(base);
            if !p.last()?.ph.is_vowel() {
                p.push(ph(Ph::AX));
            }
            p.push(ph(Ph::L));
        }
        "ve" => {
            p = host(base);
            if !p.last()?.ph.is_vowel() {
                p.push(ph(Ph::AX));
            }
            p.push(ph(Ph::V));
        }
        "re" => {
            p = host(base);
            p.push(ph(Ph::ER));
        }
        "m" => {
            p = host(base);
            p.push(ph(Ph::M));
        }
        "t" if base.len() > 1 && base.ends_with('n') => {
            p = host(&base[..base.len() - 1]);
            p.extend([ph(Ph::AX), ph(Ph::N), ph(Ph::T)]);
        }
        _ => return None,
    }
    Some(p)
}

/// Words before which the next word is most likely a verb ("to record", "they present").
const VERB_CUES: [&str; 36] = [
    "to", "will", "would", "can", "could", "shall", "should", "may", "might", "must", "i", "you", "we", "they",
    "not", "don't", "doesn't", "didn't", "won't", "wouldn't", "can't", "couldn't", "cannot", "shouldn't", "i'll",
    "you'll", "we'll", "they'll", "he'll", "she'll", "it'll", "let's", "please", "never", "always", "who",
];

/// Words before which the next word is most likely a noun or an adjective ("the record").
const NOUN_CUES: [&str; 46] = [
    "the", "a", "an", "this", "these", "those", "my", "your", "his", "its", "our", "their", "some", "any", "no",
    "each", "every", "of", "in", "on", "for", "with", "at", "by", "from", "about", "new", "old", "first", "last",
    "same", "world", "big", "great", "good", "bad", "own", "main", "whole", "another", "such", "what", "which",
    "whose", "one", "two",
];

/// Noun / verb pairs told apart by where the stress falls: (word, as a verb, as a noun or adjective).
const STRESS_PAIRS: [(&str, &str, &str); 25] = [
    ("record", "R IH K AO1 R D", "R EH1 K ER D"),
    ("records", "R IH K AO1 R D Z", "R EH1 K ER D Z"),
    ("present", "P R IY Z EH1 N T", "P R EH1 Z AX N T"),
    ("presents", "P R IY Z EH1 N T S", "P R EH1 Z AX N T S"),
    ("object", "AX B JH EH1 K T", "AA1 B JH EH K T"),
    ("project", "P R AX JH EH1 K T", "P R AA1 JH EH K T"),
    ("produce", "P R AX D UW1 S", "P R OW1 D UW S"),
    ("conduct", "K AX N D AH1 K T", "K AA1 N D AH K T"),
    ("contract", "K AX N T R AE1 K T", "K AA1 N T R AE K T"),
    ("permit", "P ER M IH1 T", "P ER1 M IH T"),
    ("increase", "IH N K R IY1 S", "IH1 N K R IY S"),
    ("decrease", "D IH K R IY1 S", "D IY1 K R IY S"),
    ("import", "IH M P AO1 R T", "IH1 M P AO R T"),
    ("export", "IH K S P AO1 R T", "EH1 K S P AO R T"),
    ("insult", "IH N S AH1 L T", "IH1 N S AH L T"),
    ("protest", "P R AX T EH1 S T", "P R OW1 T EH S T"),
    ("rebel", "R IH B EH1 L", "R EH1 B AX L"),
    ("subject", "S AX B JH EH1 K T", "S AH1 B JH IH K T"),
    ("suspect", "S AX S P EH1 K T", "S AH1 S P EH K T"),
    ("conflict", "K AX N F L IH1 K T", "K AA1 N F L IH K T"),
    ("contest", "K AX N T EH1 S T", "K AA1 N T EH S T"),
    ("convert", "K AX N V ER1 T", "K AA1 N V ER T"),
    ("progress", "P R AX G R EH1 S", "P R AA1 G R EH S"),
    ("content", "K AX N T EH1 N T", "K AA1 N T EH N T"),
    ("desert", "D IH Z ER1 T", "D EH1 Z ER T"),
];

/// Words in -ate that end in "ate" as a verb and in a weak "it" as a noun or adjective.
const ATE_PAIRS: [&str; 16] = [
    "separate", "estimate", "graduate", "duplicate", "moderate", "alternate", "associate", "advocate",
    "delegate", "deliberate", "elaborate", "approximate", "appropriate", "articulate", "coordinate", "aggregate",
];

/// Words whose pronunciation depends on their role in the sentence, decided from the words
/// next to them. `None` keeps the lexicon's reading (when the context says nothing).
fn heteronym(word: &str, prev: Option<&str>, next: Option<&str>, lex: &Lexicon) -> Option<Vec<Phone>> {
    let prev = prev.unwrap_or("");
    let next = next.unwrap_or("");
    let verb_cue = VERB_CUES.contains(&prev);
    let noun_cue = NOUN_CUES.contains(&prev);
    let say = |text: &str| crate::phone::parse_phones(text);
    if let Some(&(_, verb, noun)) = STRESS_PAIRS.iter().find(|p| p.0 == word) {
        return if verb_cue {
            say(verb)
        } else if noun_cue {
            say(noun)
        } else {
            None
        };
    }
    if ATE_PAIRS.contains(&word) && (verb_cue || noun_cue) {
        let mut phones = lex.get(word)?.to_vec();
        let last = phones.iter().rposition(|p| p.ph.is_vowel())?;
        phones[last] = if verb_cue { Phone::new(Ph::EY, 2) } else { Phone::new(Ph::AX, 0) };
        return Some(phones);
    }
    match word {
        // "have read", "was read", "he read" (he reads otherwise)
        "read" => matches!(
            prev,
            "have" | "has" | "had" | "having" | "haven't" | "hasn't" | "hadn't" | "i've" | "you've" | "we've" | "they've"
                | "was" | "were" | "been" | "being" | "is" | "are" | "be" | "already" | "he" | "she" | "it" | "widely" | "well"
        )
        .then(|| say("R EH1 D"))
        .flatten(),
        // "used to go" - but "is used to cut"
        "used" if next == "to" && !matches!(prev, "is" | "are" | "was" | "were" | "be" | "been" | "being" | "it's") => say("Y UW1 S T"),
        "use" if noun_cue || matches!(prev, "make" | "makes" | "made" | "making") => say("Y UW1 S"),
        "use" if verb_cue => say("Y UW1 Z"),
        "close" if next == "to" || matches!(prev, "very" | "so" | "too" | "as" | "quite" | "a" | "how" | "that's") => say("K L OW1 S"),
        "close" if verb_cue => say("K L OW1 Z"),
        "live" if noun_cue
            || matches!(next, "music" | "show" | "shows" | "performance" | "broadcast" | "stream" | "audience" | "coverage" | "event" | "events" | "concert" | "television" | "wire" | "demo" | "band" | "feed" | "video" | "album") =>
        {
            say("L AY1 V")
        }
        "live" if verb_cue => say("L IH1 V"),
        "lives" if matches!(prev, "their" | "our" | "your" | "my" | "many" | "of" | "two" | "save" | "saved" | "saving" | "the" | "these" | "those" | "whose" | "both" | "all" | "own" | "lost" | "new" | "people's") => say("L AY1 V Z"),
        "wind" if verb_cue || (matches!(next, "up" | "down") && !noun_cue) => say("W AY1 N D"),
        "lead" if matches!(next, "pipe" | "pipes" | "paint" | "poisoning" | "bullet" | "bullets" | "pencil" | "pencils") => say("L EH1 D"),
        _ => None,
    }
}

/// Splits one whitespace-delimited token into pronounceable words.
fn expand_token(tok: &str, lex: &Lexicon, out: &mut Vec<(String, Vec<Phone>)>) {
    if tok.contains('\'') {
        if let Some(p) = lex.get(tok) {
            out.push((tok.to_string(), p.to_vec()));
            return;
        }
    }
    if tok == "cannot" {
        out.push(("can".to_string(), lex.pronounce("can")));
        out.push(("not".to_string(), lex.pronounce("not")));
        return;
    }
    for piece in tok.split(|c: char| !is_tok_char(c)) {
        let piece = piece.trim_matches('\'');
        if piece.is_empty() {
            continue;
        }
        if !piece.contains('\'') {
            out.push((piece.to_string(), lex.pronounce(piece)));
        } else if let Some(p) = pronounce_apostrophe(piece, lex) {
            out.push((piece.to_string(), p));
        } else {
            for part in piece.split('\'') {
                if !part.is_empty() {
                    out.push((part.to_string(), lex.pronounce(part)));
                }
            }
        }
    }
}

/// Tokenizes text into a clause-aware word & phoneme plan.
pub fn build_plan(raw_text: &str, lex: &Lexicon) -> Plan {
    let normalized = normalize(raw_text, lex);
    if normalized.is_empty() {
        return Plan::default();
    }

    // Split into prosodic clauses, keeping the punctuation that closed each one.
    let mut raw_clauses: Vec<(String, char)> = Vec::new();
    {
        let mut body = String::new();
        let mut punc: Option<char> = None;
        for c in normalized.chars() {
            if is_punct(c) {
                if body.is_empty() && punc.is_none() {
                    continue; // leading punctuation
                }
                if punc.is_none() {
                    punc = Some(c);
                }
            } else {
                if let Some(p) = punc.take() {
                    raw_clauses.push((std::mem::take(&mut body), p));
                }
                body.push(c);
            }
        }
        if !body.is_empty() {
            raw_clauses.push((body, punc.unwrap_or('.')));
        }
    }

    let mut clauses = Vec::new();
    let mut ipa_parts: Vec<String> = Vec::new();

    for (body, punc) in raw_clauses {
        let lowered = body.to_lowercase().replace(['\u{2018}', '\u{2019}'], "'");
        let mut expanded: Vec<(String, Vec<Phone>)> = Vec::new();
        for raw_tok in lowered.split_whitespace() {
            let tok = raw_tok.trim_matches(|c: char| !is_tok_char(c));
            if !tok.is_empty() {
                expand_token(tok, lex, &mut expanded);
            }
        }
        if expanded.is_empty() {
            continue;
        }

        // Wh-questions ("Where did you go?", "At what time?") and choice questions ("tea or coffee?")
        // fall at the end; yes/no questions, negative questions ("Isn't that interesting?"), tag
        // questions (", isn't it?") and short echo questions ("Really?") rise.
        let is_yes_no = punc == '?'
            && !expanded.iter().take(3).any(|(w, _)| is_wh_word(w))
            && !expanded
                .iter()
                .enumerate()
                .any(|(i, (w, _))| i > 0 && i + 1 < expanded.len() && w == "or");
        let count = expanded.len();
        let mut words = Vec::with_capacity(count);

        for wi in 0..count {
            let w = expanded[wi].0.as_str();
            let mut phones = expanded[wi].1.clone();
            if phones.is_empty() {
                continue;
            }

            // "to record" / "the record", "have read", "used to"
            if !lex.is_user_word(w) {
                let prev = (wi > 0).then(|| expanded[wi - 1].0.as_str());
                let next = (wi + 1 < count).then(|| expanded[wi + 1].0.as_str());
                if let Some(p) = heteronym(w, prev, next, lex) {
                    phones = p;
                }
            }

            // Connected-speech weak forms for function words that are not clause-final.
            let is_non_final = wi + 1 < count;
            if is_non_final {
                let next_starts_vowel = expanded[wi + 1].1.first().is_some_and(|p| p.ph.is_vowel());
                if !lex.is_user_word(w) {
                    if let Some(weak) = weak_form(w, next_starts_vowel) {
                        phones = weak;
                    }
                }
            }

            let is_func = is_non_final && FUNCTION_WORDS.contains(&w);
            if is_func {
                for p in phones.iter_mut() {
                    if p.stress == 1 {
                        p.stress = 0;
                    }
                }
            }

            let ipa: String = phones.iter().map(|p| p.ph.ipa()).collect();
            ipa_parts.push(ipa.clone());
            words.push(PlanWord {
                text: w.to_string(),
                phones,
                ipa,
                is_content: !is_func,
                is_clause_final: wi == count - 1,
            });
        }

        if !words.is_empty() {
            clauses.push(Clause {
                words,
                punc,
                is_question: punc == '?',
                is_yes_no_question: is_yes_no,
                is_exclaim: punc == '!',
            });
        }
    }

    Plan {
        normalized,
        clauses,
        ipa: format!("/{}/", ipa_parts.join(" ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arpabet(text: &str) -> String {
        build_plan(text, &Lexicon::new()).arpabet()
    }

    #[test]
    fn words_that_depend_on_their_role() {
        assert_eq!(arpabet("to record a record"), "T AX0 | R IH0 K AO1 R D | AX0 | R EH1 K ER0 D");
        assert_eq!(arpabet("they present the present"), "DH EY1 | P R IY0 Z EH1 N T | DH AX0 | P R EH1 Z AX0 N T");
        assert!(arpabet("I have read it").contains("R EH1 D"));
        assert!(arpabet("I will read it").contains("R IY1 D"));
        assert!(arpabet("we used to go").contains("Y UW1 S T"));
        assert!(arpabet("it is used to cut").contains("Y UW1 Z D"));
        assert!(arpabet("the use of it").contains("Y UW1 S"));
        assert!(arpabet("to use it").contains("Y UW1 Z"));
        assert!(arpabet("close to home").contains("K L OW1 S"));
        assert!(arpabet("please close it").contains("K L OW1 Z"));
        assert!(arpabet("a live show").contains("L AY1 V"));
        assert!(arpabet("we live here").contains("L IH1 V"));
        assert!(arpabet("to separate them").ends_with("EY2 T | DH EH1 M"), "{}", arpabet("to separate them"));
        assert!(arpabet("a separate room").contains("AX0 T | R UW1 M"), "{}", arpabet("a separate room"));
    }

    #[test]
    fn clauses_and_questions() {
        let lex = Lexicon::new();
        let plan = build_plan("Hello there, how are you? Fine!", &lex);
        let puncs: Vec<char> = plan.clauses.iter().map(|c| c.punc).collect();
        assert_eq!(puncs, vec![',', '?', '!']);
        assert!(plan.clauses[1].is_question && !plan.clauses[1].is_yes_no_question);
        assert!(build_plan("Can you hear me?", &lex).clauses[0].is_yes_no_question);
        assert!(build_plan("Isn't that interesting?", &lex).clauses[0].is_yes_no_question);
        assert!(build_plan("It is cold, isn't it?", &lex).clauses[1].is_yes_no_question);
        assert!(build_plan("Really?", &lex).clauses[0].is_yes_no_question);
        assert!(!build_plan("At what time does it start?", &lex).clauses[0].is_yes_no_question);
        assert!(!build_plan("Do you want tea or coffee?", &lex).clauses[0].is_yes_no_question);
        assert!(build_plan("...", &lex).is_empty());
    }

    #[test]
    fn weak_forms_and_destressing() {
        assert_eq!(arpabet("the apple"), "DH IY0 | AE1 P AX0 L");
        assert_eq!(arpabet("cats and dogs"), "K AE1 T S | AX0 N D | D AO1 G Z");
        assert_eq!(arpabet("go to bed"), "G OW1 | T AX0 | B EH1 D");
        assert_eq!(arpabet("go to it"), "G OW1 | T UW0 | IH1 T");
        // ... but not before a pause, where the full form is used
        assert_eq!(arpabet("what for"), "W AA1 T | F AO0 R");
        assert_eq!(arpabet("the dog"), "DH AX0 | D AO1 G");
        // clause-final function words keep their stress
        assert_eq!(arpabet("what is it"), "W AA1 T | IH0 Z | IH1 T");
    }

    #[test]
    fn contractions_and_clitics() {
        assert_eq!(arpabet("don't"), "D OW1 N T");
        assert_eq!(arpabet("we'd"), "W IY1 D");
        assert_eq!(arpabet("it'll"), "IH1 T AX0 L");
        assert_eq!(arpabet("the dog's"), "DH AX0 | D AO1 G Z");
        assert_eq!(arpabet("voice's"), "V OY1 S IH0 Z");
    }

    #[test]
    fn spelling_and_morphology() {
        // vowel-less tokens no dictionary knows are read letter by letter
        assert_eq!(arpabet("zq"), "Z IY2 K Y UW1");
        assert_eq!(arpabet("plan b"), "P L AE1 N | B IY1");
        assert_eq!(arpabet("voices"), "V OY1 S IH0 Z");
        assert_eq!(arpabet("jumped"), "JH AH1 M P T");
    }
}
