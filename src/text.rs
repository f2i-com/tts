//! English text normalization: numbers, ordinals, years, decimals, currency,
//! symbols, common abbreviations and initialisms are rewritten as plain words.
//! Punctuation that marks a pause (. , ; : ! ?) is preserved for the prosody stage.

use crate::lexicon::{Lexicon, LETTER_WORDS};

const ONES: [&str; 20] = [
    "zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve", "thirteen", "fourteen", "fifteen", "sixteen", "seventeen", "eighteen", "nineteen",
];
const TENS: [&str; 10] = [
    "", "", "twenty", "thirty", "forty", "fifty", "sixty", "seventy", "eighty", "ninety",
];

fn below_1000(n: u64) -> String {
    debug_assert!(n > 0 && n < 1000);
    let mut parts: Vec<&str> = Vec::new();
    let h = (n / 100) as usize;
    let r = (n % 100) as usize;
    if h > 0 {
        parts.push(ONES[h]);
        parts.push("hundred");
    }
    if r > 0 {
        if r < 20 {
            parts.push(ONES[r]);
        } else {
            parts.push(TENS[r / 10]);
            if !r.is_multiple_of(10) {
                parts.push(ONES[r % 10]);
            }
        }
    }
    parts.join(" ")
}

/// Cardinal number in words (American style, no "and"). `n` must be below 10^15.
pub fn cardinal(n: u64) -> String {
    if n == 0 {
        return "zero".to_string();
    }
    let scales: [(u64, &str); 4] = [
        (1_000_000_000_000, "trillion"),
        (1_000_000_000, "billion"),
        (1_000_000, "million"),
        (1_000, "thousand"),
    ];
    let mut parts: Vec<String> = Vec::new();
    let mut rem = n;
    for (scale, name) in scales {
        if rem >= scale {
            parts.push(format!("{} {}", below_1000((rem / scale) % 1000), name));
            rem %= scale;
        }
    }
    if rem > 0 {
        parts.push(below_1000(rem));
    }
    parts.join(" ")
}

/// Ordinal number in words: 1 -> first, 21 -> twenty first, 40 -> fortieth.
pub fn ordinal(n: u64) -> String {
    let words = cardinal(n);
    let (head, last) = match words.rfind(' ') {
        Some(i) => (&words[..=i], &words[i + 1..]),
        None => ("", words.as_str()),
    };
    let ord = match last {
        "one" => "first".to_string(),
        "two" => "second".to_string(),
        "three" => "third".to_string(),
        "five" => "fifth".to_string(),
        "eight" => "eighth".to_string(),
        "nine" => "ninth".to_string(),
        "twelve" => "twelfth".to_string(),
        w if w.ends_with('y') => format!("{}ieth", &w[..w.len() - 1]),
        w => format!("{w}th"),
    };
    format!("{head}{ord}")
}

fn digits_spoken(digits: &str) -> String {
    digits
        .bytes()
        .filter(|b| b.is_ascii_digit())
        .map(|b| ONES[(b - b'0') as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whole number as it is normally read aloud: years in pairs, long or zero-padded strings digit by digit.
fn integer_spoken(digits: &str, allow_year: bool) -> String {
    if digits.len() > 15 || (digits.len() > 1 && digits.starts_with('0')) {
        return digits_spoken(digits);
    }
    let v: u64 = digits.parse().unwrap_or(0);
    if allow_year && digits.len() == 4 && ((1900..=1999).contains(&v) || (2010..=2099).contains(&v)) {
        let hi = v / 100;
        let lo = v % 100;
        return if lo == 0 {
            format!("{} hundred", cardinal(hi))
        } else if lo < 10 {
            format!("{} oh {}", cardinal(hi), cardinal(lo))
        } else {
            format!("{} {}", cardinal(hi), cardinal(lo))
        };
    }
    cardinal(v)
}

fn pluralize_last(words: &str) -> String {
    if let Some(stem) = words.strip_suffix('y') {
        format!("{stem}ies")
    } else if words.ends_with('x') {
        format!("{words}es")
    } else {
        format!("{words}s")
    }
}

struct Number {
    int_digits: String,
    grouped: bool,
    frac: Vec<String>,
    end: usize,
}

/// Scans `1,234.56`-style numbers starting at `chars[start]` (an ASCII digit).
/// Returns `None` if the digits are glued to letters ("3D", "2nd", "100Hz").
fn scan_number(chars: &[char], start: usize) -> Option<Number> {
    let n = chars.len();
    let mut i = start;
    while i < n && chars[i].is_ascii_digit() {
        i += 1;
    }
    if i < n && chars[i].is_ascii_alphabetic() {
        return None;
    }
    let mut int_digits: String = chars[start..i].iter().collect();
    let mut grouped = false;
    if int_digits.len() <= 3 {
        // thousands separators: 1,234,567
        while i + 3 < n
            && chars[i] == ','
            && chars[i + 1..i + 4].iter().all(|c| c.is_ascii_digit())
            && !(i + 4 < n && chars[i + 4].is_ascii_alphanumeric())
        {
            int_digits.extend(chars[i + 1..i + 4].iter());
            i += 4;
            grouped = true;
        }
    }
    let mut frac = Vec::new();
    while i + 1 < n && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
        let fs = i + 1;
        let mut j = fs;
        while j < n && chars[j].is_ascii_digit() {
            j += 1;
        }
        frac.push(chars[fs..j].iter().collect());
        i = j;
    }
    Some(Number {
        int_digits,
        grouped,
        frac,
        end: i,
    })
}

fn number_spoken(num: &Number) -> String {
    let mut s = integer_spoken(&num.int_digits, !num.grouped && num.frac.is_empty());
    for f in &num.frac {
        s.push_str(" point ");
        s.push_str(&digits_spoken(f));
    }
    s
}

fn has_vowel_letter(lower: &str) -> bool {
    lower
        .bytes()
        .any(|b| matches!(b, b'a' | b'e' | b'i' | b'o' | b'u' | b'y'))
}

fn spell_letters(lower: &str) -> String {
    lower
        .bytes()
        .filter(|b| b.is_ascii_lowercase())
        .map(|b| LETTER_WORDS[(b - b'a') as usize])
        .collect::<Vec<_>>()
        .join(" ")
}

/// "am", "pm", "a.m" or "p.m" at `k`, not running on into a longer word: whether it is
/// pm, and its length.
fn meridiem_at(chars: &[char], k: usize) -> Option<(bool, usize)> {
    [("a.m", 3), ("p.m", 3), ("am", 2), ("pm", 2)].into_iter().find_map(|(pat, len)| {
        let ends = chars.get(k + len).is_none_or(|c| !c.is_ascii_alphanumeric());
        (starts_with_ci(chars, k, pat) && ends).then_some((pat.starts_with('p'), len))
    })
}

/// After "a.m" / "p.m" / "U.S": a stop that belongs to the abbreviation is skipped, unless
/// a new sentence follows it.
fn skip_abbreviation_stop(chars: &[char], i: usize) -> usize {
    if chars.get(i) != Some(&'.') {
        return i;
    }
    let mut k = i + 1;
    while chars.get(k) == Some(&' ') {
        k += 1;
    }
    match chars.get(k) {
        Some(c) if !c.is_ascii_uppercase() => i + 1,
        _ => i,
    }
}

/// Rewrites one alphabetic token.
fn emit_word(tok: &str, mixed_case_text: bool, lex: &Lexicon, out: &mut String) {
    let lower = tok.to_ascii_lowercase();
    // A pronunciation the caller taught always wins over the built-in rewrites below.
    if lex.is_user_word(&lower) {
        out.push_str(tok);
        return;
    }
    match lower.as_str() {
        "softn" => {
            out.push_str("soft en");
            return;
        }
        "hz" => {
            out.push_str("hertz");
            return;
        }
        "khz" => {
            out.push_str("kilohertz");
            return;
        }
        "etc" => {
            out.push_str("et cetera");
            return;
        }
        _ => {}
    }
    match tok {
        "AI" => {
            out.push_str("ay eye");
            return;
        }
        "IPA" => {
            out.push_str("eye pee ay");
            return;
        }
        "TTS" => {
            out.push_str("text to speech");
            return;
        }
        "dB" => {
            out.push_str("decibels");
            return;
        }
        _ => {}
    }
    // Initialisms: short all-caps tokens the lexicon does not know ("FBI", "CPU", "UK").
    let all_caps = tok.len() >= 2 && tok.len() <= 4 && tok.bytes().all(|b| b.is_ascii_uppercase());
    if all_caps
        && mixed_case_text
        && !lex.contains(&lower)
        && (tok.len() <= 3 || !has_vowel_letter(&lower))
    {
        out.push_str(&spell_letters(&lower));
        return;
    }
    out.push_str(tok);
}

/// Rewrites a token that mixes digits and letters ("3D", "2nd", "1990s", "100Hz", "mp3").
fn emit_mixed(tok: &str, mixed_case_text: bool, lex: &Lexicon, out: &mut String) {
    let lower = tok.to_ascii_lowercase();
    if lower == "3d" {
        out.push_str("three dee");
        return;
    }
    if lower == "2d" {
        out.push_str("two dee");
        return;
    }
    let split = lower.bytes().position(|b| !b.is_ascii_digit()).unwrap_or(0);
    let (digits, suffix) = lower.split_at(split);
    if !digits.is_empty() && suffix.bytes().all(|b| b.is_ascii_lowercase()) {
        if matches!(suffix, "st" | "nd" | "rd" | "th") && digits.len() <= 15 {
            out.push_str(&ordinal(digits.parse().unwrap_or(0)));
            return;
        }
        if suffix == "s" {
            out.push_str(&pluralize_last(&integer_spoken(digits, true)));
            return;
        }
        if let Some((one, many)) = unit_words(&tok[split..], true) {
            out.push_str(&integer_spoken(digits, false));
            out.push(' ');
            out.push_str(if digits == "1" { one } else { many });
            return;
        }
    }
    // "4x4", "1920x1080"
    if let Some((a, b)) = lower.split_once('x') {
        if !a.is_empty() && !b.is_empty() && a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) {
            out.push_str(&integer_spoken(a, false));
            out.push_str(" by ");
            out.push_str(&integer_spoken(b, false));
            return;
        }
    }
    // Generic case: alternate runs of digits and letters become separate words.
    let b = tok.as_bytes();
    let mut i = 0;
    let mut first = true;
    while i < b.len() {
        let is_digit = b[i].is_ascii_digit();
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() == is_digit {
            i += 1;
        }
        if !first {
            out.push(' ');
        }
        first = false;
        let run = &tok[start..i];
        if is_digit {
            out.push_str(&integer_spoken(run, false));
        } else {
            emit_word(run, mixed_case_text, lex, out);
        }
    }
}

fn starts_with_ci(chars: &[char], at: usize, pat: &str) -> bool {
    let pat: Vec<char> = pat.chars().collect();
    at + pat.len() <= chars.len()
        && chars[at..at + pat.len()]
            .iter()
            .zip(pat.iter())
            .all(|(a, b)| a.to_ascii_lowercase() == *b)
}

/// Strips diacritics from common Latin letters so "café" and "naïve" hit the lexicon.
fn fold_accent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
        'ç' => 'c',
        'Ç' => 'C',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'È' | 'É' | 'Ê' | 'Ë' => 'E',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
        'ñ' => 'n',
        'Ñ' => 'N',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' => 'O',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
        'ý' | 'ÿ' => 'y',
        'Ý' => 'Y',
        // full-width forms of the ASCII characters
        '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
        // no-break, thin, zero-width and ideographic spaces
        '\u{00a0}' | '\u{2009}' | '\u{200a}' | '\u{200b}' | '\u{202f}' | '\u{3000}' => ' ',
        '\u{201c}' | '\u{201d}' => '"',
        _ => c,
    }
}

/// Units written after a number ("5 km", "25min"): singular and plural in words.
/// `attached` (no space before the unit) allows the short, more ambiguous ones.
fn unit_words(unit: &str, attached: bool) -> Option<(&'static str, &'static str)> {
    let lower = unit.to_ascii_lowercase();
    // separate tokens must be written the usual way: "5 MB" but not "5 Mb" or "5 IN"
    let usual = unit == lower || matches!(unit, "Hz" | "kHz" | "MHz" | "GHz" | "KB" | "MB" | "GB" | "TB" | "kW" | "kWh" | "dB");
    if !attached && !usual {
        return None;
    }
    Some(match lower.as_str() {
        "km" => ("kilometer", "kilometers"),
        "cm" => ("centimeter", "centimeters"),
        "mm" => ("millimeter", "millimeters"),
        "kg" => ("kilogram", "kilograms"),
        "mg" => ("milligram", "milligrams"),
        "ml" => ("milliliter", "milliliters"),
        "lb" | "lbs" => ("pound", "pounds"),
        "oz" => ("ounce", "ounces"),
        "mph" => ("mile per hour", "miles per hour"),
        "kph" | "kmh" => ("kilometer per hour", "kilometers per hour"),
        "ft" => ("foot", "feet"),
        "min" | "mins" => ("minute", "minutes"),
        "sec" | "secs" => ("second", "seconds"),
        "ms" => ("millisecond", "milliseconds"),
        "hr" | "hrs" => ("hour", "hours"),
        "hz" => ("hertz", "hertz"),
        "khz" => ("kilohertz", "kilohertz"),
        "mhz" => ("megahertz", "megahertz"),
        "ghz" => ("gigahertz", "gigahertz"),
        "kb" => ("kilobyte", "kilobytes"),
        "mb" => ("megabyte", "megabytes"),
        "gb" => ("gigabyte", "gigabytes"),
        "tb" => ("terabyte", "terabytes"),
        "fps" => ("frame per second", "frames per second"),
        "px" => ("pixel", "pixels"),
        "kw" => ("kilowatt", "kilowatts"),
        "kwh" => ("kilowatt hour", "kilowatt hours"),
        "db" => ("decibel", "decibels"),
        "k" if attached => ("thousand", "thousand"),
        "x" if attached => ("times", "times"),
        _ => return None,
    })
}

/// Value of a Roman numeral written with I, V and X ("VIII" -> 8), if it is a proper one.
fn roman_value(tok: &str) -> Option<u64> {
    if tok.len() < 2 || !tok.bytes().all(|b| matches!(b, b'I' | b'V' | b'X')) {
        return None;
    }
    let digit = |b: u8| match b {
        b'I' => 1u64,
        b'V' => 5,
        _ => 10,
    };
    let b = tok.as_bytes();
    let mut total = 0;
    for k in 0..b.len() {
        let v = digit(b[k]);
        if k + 1 < b.len() && v < digit(b[k + 1]) {
            total -= v as i64;
        } else {
            total += v as i64;
        }
    }
    // only the canonical spelling counts: "IIII" or "VX" are not numerals
    let canonical = |mut v: u64| {
        let mut out = String::new();
        for (value, text) in [(10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")] {
            while v >= value {
                out.push_str(text);
                v -= value;
            }
        }
        out
    };
    ((2..=39).contains(&total) && canonical(total as u64) == tok).then_some(total as u64)
}

/// Names after which a Roman numeral is read "the Eighth" rather than "eight".
const REGNAL: [&str; 34] = [
    "henry", "elizabeth", "louis", "george", "edward", "charles", "richard", "william", "james", "john", "paul",
    "pius", "benedict", "leo", "gregory", "nicholas", "alexander", "peter", "catherine", "frederick", "philip",
    "napoleon", "victoria", "mary", "anne", "pope", "king", "queen", "tsar", "emperor", "ramses", "ptolemy",
    "wilhelm", "ivan",
];

/// Titles and other abbreviations written with a full stop. (word, spoken, comes before a name)
const ABBREVIATIONS: [(&str, &str, bool); 30] = [
    ("mr", "mister", true),
    ("mrs", "missus", true),
    ("ms", "miss", true),
    ("dr", "doctor", true),
    ("prof", "professor", true),
    ("gen", "general", true),
    ("gov", "governor", true),
    ("sen", "senator", true),
    ("rep", "representative", true),
    ("lt", "lieutenant", true),
    ("sgt", "sergeant", true),
    ("capt", "captain", true),
    ("col", "colonel", true),
    ("mt", "mount", true),
    ("vs", "versus", true),
    ("jr", "junior", false),
    ("sr", "senior", false),
    ("inc", "incorporated", false),
    ("ltd", "limited", false),
    ("corp", "corporation", false),
    ("ave", "avenue", false),
    ("blvd", "boulevard", false),
    ("approx", "approximately", true),
    ("jan", "january", false),
    ("feb", "february", false),
    ("aug", "august", false),
    ("sept", "september", false),
    ("oct", "october", false),
    ("nov", "november", false),
    ("dec", "december", false),
];

const MONTHS: [&str; 12] = [
    "january", "february", "march", "april", "may", "june", "july", "august", "september", "october", "november",
    "december",
];

/// Turns raw text into speakable words while keeping pause punctuation.
pub fn normalize(raw: &str, lex: &Lexicon) -> String {
    let chars: Vec<char> = raw.trim().chars().map(fold_accent).collect();
    let n = chars.len();
    if n == 0 {
        return String::new();
    }
    let mixed_case_text = chars.iter().any(|c| c.is_ascii_lowercase());
    let mut out = String::with_capacity(n + 32);
    let mut i = 0usize;
    // What was written last, to read what follows in its light: a number ("5 km") or a
    // capitalised word ("Henry VIII"). Valid while `out` has not grown past the mark.
    let mut number_mark: Option<(usize, bool)> = None; // (length of out, the number was 1)
    let mut word_mark: Option<(usize, String, bool)> = None; // (length, lower-case word, capitalised)
    let fresh = |out: &String, mark: usize| out.trim_end().len() == mark;
    let digit_at = |k: usize| k < n && chars[k].is_ascii_digit();
    let digits_from = |k: usize| {
        let mut j = k;
        while j < n && chars[j].is_ascii_digit() {
            j += 1;
        }
        j
    };

    while i < n {
        let c = chars[i];
        let before_ok = i == 0 || !(chars[i - 1].is_ascii_alphanumeric() || matches!(chars[i - 1], '-' | '/' | ':' | '.'));

        if c.is_ascii_digit() && before_ok {
            let e1 = digits_from(i);
            let text = |a: usize, b: usize| chars[a..b].iter().collect::<String>();
            // (the notation must end here: no letter, and no further "-5", "/5" or ":5")
            let after_ok = |k: usize| !(k < n && (chars[k].is_ascii_alphanumeric() || (matches!(chars[k], '-' | '/' | ':') && digit_at(k + 1))));
            // 2026-11-16
            if e1 - i == 4 && e1 < n && chars[e1] == '-' {
                let e2 = digits_from(e1 + 1);
                if e2 - e1 == 3 && e2 < n && chars[e2] == '-' {
                    let e3 = digits_from(e2 + 1);
                    let (month, day): (usize, u64) = (text(e1 + 1, e2).parse().unwrap_or(0), text(e2 + 1, e3).parse().unwrap_or(0));
                    if e3 - e2 == 3 && after_ok(e3) && (1..=12).contains(&month) && (1..=31).contains(&day) {
                        out.push_str(MONTHS[month - 1]);
                        out.push(' ');
                        out.push_str(&ordinal(day));
                        out.push_str(", ");
                        out.push_str(&integer_spoken(&text(i, e1), true));
                        i = e3;
                        continue;
                    }
                }
            }
            // 555-1234, 555-123-4567: telephone numbers are read digit by digit
            if e1 - i == 3 && e1 < n && chars[e1] == '-' {
                let e2 = digits_from(e1 + 1);
                let mut groups = vec![(i, e1), (e1 + 1, e2)];
                let mut end = e2;
                if e2 - e1 == 4 && e2 < n && chars[e2] == '-' {
                    let e3 = digits_from(e2 + 1);
                    if e3 - e2 == 5 {
                        groups.push((e2 + 1, e3));
                        end = e3;
                    }
                }
                let last = groups[groups.len() - 1];
                if last.1 - last.0 == 4 && (groups.len() == 2 || groups[1].1 - groups[1].0 == 3) && after_ok(end) {
                    let spoken: Vec<String> = groups.iter().map(|&(a, b)| digits_spoken(&text(a, b))).collect();
                    out.push_str(&spoken.join(", "));
                    i = end;
                    continue;
                }
            }
            // 10:30, 9:05, 7:00
            if e1 - i <= 2 && e1 < n && chars[e1] == ':' {
                let e2 = digits_from(e1 + 1);
                let (hour, minute): (u64, u64) = (text(i, e1).parse().unwrap_or(99), text(e1 + 1, e2).parse().unwrap_or(99));
                if e2 - e1 == 3 && hour < 24 && minute < 60 && after_ok(e2) {
                    out.push_str(&cardinal(hour));
                    let mut k = e2;
                    while k < n && chars[k] == ' ' {
                        k += 1;
                    }
                    let meridiem = meridiem_at(&chars, k);
                    if minute == 0 {
                        if meridiem.is_none() {
                            out.push_str(if hour == 0 || hour > 12 { " hundred" } else { " o'clock" });
                        }
                    } else if minute < 10 {
                        out.push_str(" oh ");
                        out.push_str(&cardinal(minute));
                    } else {
                        out.push(' ');
                        out.push_str(&cardinal(minute));
                    }
                    i = e2;
                    // "10:30 AM": the letters, not the verb
                    if let Some((pm, len)) = meridiem {
                        out.push_str(if pm { " pee em" } else { " ay em" });
                        i = skip_abbreviation_stop(&chars, k + len);
                    }
                    continue;
                }
            }
            // 1/2, 3/4, 5/8
            if e1 - i <= 2 && e1 < n && chars[e1] == '/' {
                let e2 = digits_from(e1 + 1);
                let (num, den): (u64, u64) = (text(i, e1).parse().unwrap_or(0), text(e1 + 1, e2).parse().unwrap_or(0));
                if e2 > e1 + 1 && after_ok(e2) && !(e2 < n && chars[e2] == '.' && digit_at(e2 + 1)) && num >= 1 && num < den && den <= 16 {
                    // "2 1/2" is two and a half
                    let mixed = i > 0 && chars[i - 1] == ' ' && number_mark.is_some_and(|(mark, _)| fresh(&out, mark));
                    if mixed {
                        out.push_str("and ");
                    }
                    out.push_str(&if mixed && num == 1 { "a".to_string() } else { cardinal(num) });
                    out.push(' ');
                    let part = match den {
                        2 => "half".to_string(),
                        4 => "quarter".to_string(),
                        _ => ordinal(den),
                    };
                    out.push_str(&part);
                    if num > 1 {
                        out.truncate(out.len() - part.len());
                        out.push_str(&if den == 2 { "halves".to_string() } else { format!("{part}s") });
                    }
                    i = e2;
                    continue;
                }
                // 24/7
                if (num, den) == (24, 7) && after_ok(e2) {
                    out.push_str("twenty four seven");
                    i = e2;
                    continue;
                }
            }
        }

        if c.is_ascii_digit() {
            if let Some(num) = scan_number(&chars, i) {
                out.push_str(&number_spoken(&num));
                i = num.end;
                number_mark = Some((out.trim_end().len(), num.int_digits == "1" && num.frac.is_empty()));
                // 3°C, 72 °F, 45°
                let mut k = i;
                while k < n && chars[k] == ' ' {
                    k += 1;
                }
                if k < n && chars[k] == '\u{00b0}' {
                    let one = number_mark.is_some_and(|m| m.1);
                    out.push_str(if one { " degree" } else { " degrees" });
                    i = k + 1;
                    let scale = if i < n && !(i + 1 < n && chars[i + 1].is_ascii_alphanumeric()) { chars[i] } else { ' ' };
                    match scale {
                        'C' => out.push_str(" celsius"),
                        'F' => out.push_str(" fahrenheit"),
                        _ => {}
                    }
                    if matches!(scale, 'C' | 'F') {
                        i += 1;
                    }
                }
                continue;
            }
        }

        if c.is_ascii_alphanumeric() {
            let start = i;
            while i < n && chars[i].is_ascii_alphanumeric() {
                i += 1;
            }
            let tok: String = chars[start..i].iter().collect();
            if tok.bytes().any(|b| b.is_ascii_digit()) {
                if lex.is_user_word(&tok.to_ascii_lowercase()) {
                    out.push_str(&tok);
                } else {
                    emit_mixed(&tok, mixed_case_text, lex, &mut out);
                }
                continue;
            }
            let lower = tok.to_ascii_lowercase();
            // "e.g." / "i.e."
            if tok.len() == 1 {
                if tok.eq_ignore_ascii_case("e") && starts_with_ci(&chars, i, ".g.") {
                    out.push_str("for example,");
                    i += 3;
                    continue;
                }
                if tok.eq_ignore_ascii_case("i") && starts_with_ci(&chars, i, ".e.") {
                    out.push_str("that is,");
                    i += 3;
                    continue;
                }
                // a.m. / p.m.
                if (lower == "a" || lower == "p") && starts_with_ci(&chars, i, ".m") && !(i + 2 < n && chars[i + 2].is_ascii_alphanumeric()) {
                    out.push_str(if lower == "a" { "ay em" } else { "pee em" });
                    i = skip_abbreviation_stop(&chars, i + 2);
                    continue;
                }
                // U.S., U.K., D.C., J.K.: letters whose stops are not the end of a sentence
                if chars[start].is_ascii_uppercase() && i < n && chars[i] == '.' {
                    let mut letters = String::from(chars[start].to_ascii_lowercase());
                    let mut j = i + 1;
                    while j < n && chars[j].is_ascii_uppercase() && chars.get(j + 1).is_none_or(|c| !c.is_ascii_alphanumeric()) {
                        letters.push(chars[j].to_ascii_lowercase());
                        j += if chars.get(j + 1) == Some(&'.') { 2 } else { 1 };
                    }
                    if letters.len() >= 2 {
                        out.push_str(&spell_letters(&letters));
                        i = j;
                        // (a stop that also closes the text still closes it)
                        if chars[i - 1] == '.' && chars[i..].iter().all(|c| c.is_whitespace()) {
                            out.push('.');
                        }
                        continue;
                    }
                }
            }
            let next_is_dot = i < n && chars[i] == '.';
            let user_word = lex.is_user_word(&lower);
            // "http://", "https://": the address is enough
            if !user_word && matches!(lower.as_str(), "http" | "https") && starts_with_ci(&chars, i, "://") {
                i += 3;
                continue;
            }
            // 5 km, 3 GB: a unit after a number
            if let Some((mark, one)) = number_mark {
                if !user_word && fresh(&out, mark) {
                    // "9 am": the letters, not the verb
                    if lower == "am" || lower == "pm" {
                        out.push_str(if lower == "am" { "ay em" } else { "pee em" });
                        continue;
                    }
                    if let Some((single, many)) = unit_words(&tok, false) {
                        out.push_str(if one { single } else { many });
                        if next_is_dot && tok.len() <= 4 && lower != "min" && i + 1 < n && chars[i + 1] == ' ' && i + 2 < n && chars[i + 2].is_ascii_lowercase() {
                            i += 1; // "5 ft. long"
                        }
                        continue;
                    }
                }
            }
            // Henry VIII, Chapter IV, World War II: a Roman numeral after a capitalised word
            if !user_word && mixed_case_text {
                if let (Some(value), Some((mark, prev, capitalised))) = (roman_value(&tok), word_mark.as_ref()) {
                    if *capitalised && fresh(&out, *mark) {
                        if REGNAL.contains(&prev.as_str()) {
                            out.push_str("the ");
                            out.push_str(&ordinal(value));
                        } else {
                            out.push_str(&cardinal(value));
                        }
                        continue;
                    }
                }
            }
            // Titles and other abbreviations with a full stop
            if !user_word && next_is_dot {
                let after = {
                    let mut k = i + 1;
                    while k < n && chars[k] == ' ' {
                        k += 1;
                    }
                    if k < n { chars[k] } else { '\n' }
                };
                // "St. Mary" is a saint, "Elm St." a street
                if lower == "st" && tok != "st" {
                    let spaced = i + 1 < n && chars[i + 1] == ' ';
                    let after_name = word_mark.as_ref().is_some_and(|(mark, _, cap)| *cap && fresh(&out, *mark));
                    if spaced && after.is_ascii_uppercase() && !after_name {
                        out.push_str("saint");
                        i += 1;
                    } else {
                        out.push_str("street");
                        if !(after.is_ascii_uppercase() || after == '\n') {
                            i += 1;
                        }
                    }
                    continue;
                }
                if lower == "no" && tok == "No" && after.is_ascii_digit() {
                    out.push_str("number");
                    i += 1;
                    continue;
                }
                if let Some(&(_, spoken, leading)) = ABBREVIATIONS.iter().find(|a| a.0 == lower) {
                    // (written with a capital, or all lower case for "vs." and "approx.")
                    let titled = tok.chars().next().is_some_and(|f| f.is_ascii_uppercase()) || matches!(lower.as_str(), "vs" | "approx");
                    let month = MONTHS.contains(&spoken);
                    if titled && (!month || after.is_ascii_digit()) {
                        out.push_str(spoken);
                        // a trailing abbreviation may also end the sentence: keep that stop
                        if leading || month || !(after.is_ascii_uppercase() || after == '\n') {
                            i += 1;
                        }
                        word_mark = Some((out.trim_end().len(), spoken.to_string(), true));
                        continue;
                    }
                }
            }
            // APIs, CDs, URLs: the plural of an initialism
            if !user_word && mixed_case_text && tok.len() >= 3 && tok.ends_with('s') {
                let head = &tok[..tok.len() - 1];
                let head_lower = head.to_ascii_lowercase();
                if head.len() >= 2 && head.len() <= 4 && head.bytes().all(|b| b.is_ascii_uppercase()) {
                    if !lex.contains(&head_lower) && (head.len() <= 3 || !has_vowel_letter(&head_lower)) {
                        out.push_str(&spell_letters(&head_lower));
                    } else {
                        out.push_str(&head_lower); // PINs, CEOs
                    }
                    out.push_str("'s");
                    continue;
                }
            }
            // re-enter, re-use: the prefix, not the note
            if !user_word && lower == "re" && i + 1 < n && chars[i] == '-' && chars[i + 1].is_ascii_alphabetic() {
                out.push_str("ree");
                continue;
            }
            emit_word(&tok, mixed_case_text, lex, &mut out);
            word_mark = Some((out.trim_end().len(), lower, tok.chars().next().is_some_and(|f| f.is_ascii_uppercase())));
            // example.com, index.html: a dot inside a name is spoken
            if i + 2 < n
                && chars[i] == '.'
                && tok.len() >= 2
                && chars[i + 1].is_ascii_lowercase()
                && chars[i + 2].is_ascii_alphanumeric()
                && chars[start..i].iter().any(|ch| ch.is_ascii_lowercase())
            {
                out.push_str(" dot ");
                i += 1;
            }
            continue;
        }

        let next_is_digit = i + 1 < n && chars[i + 1].is_ascii_digit();
        match c {
            '%' => out.push_str(" percent "),
            '&' => out.push_str(" and "),
            '+' => out.push_str(" plus "),
            '=' => out.push_str(" equals "),
            '@' => out.push_str(" at "),
            '#' if next_is_digit => out.push_str(" number "),
            '\u{00b0}' => out.push_str(" degrees "),
            '\u{00bd}' | '\u{00bc}' | '\u{00be}' => {
                let part = match c {
                    '\u{00bd}' => "a half",
                    '\u{00bc}' => "a quarter",
                    _ => "three quarters",
                };
                // "1½" is one and a half
                let after_number = number_mark.is_some_and(|(mark, _)| fresh(&out, mark)) && i > 0 && chars[i - 1].is_ascii_digit();
                out.push_str(if after_number { " and " } else { " " });
                out.push_str(part);
                out.push(' ');
            }
            '$' | '£' | '€' if next_is_digit => {
                if let Some(num) = scan_number(&chars, i + 1) {
                    let (one, many, small_one, small_many) = match c {
                        '$' => ("dollar", "dollars", " cent", " cents"),
                        '£' => ("pound", "pounds", " penny", " pence"),
                        _ => ("euro", "euros", " cent", " cents"),
                    };
                    let whole: u64 = num.int_digits.parse().unwrap_or(2);
                    let cents = if num.frac.len() == 1 && num.frac[0].len() == 2 {
                        num.frac[0].parse::<u64>().ok()
                    } else {
                        None
                    };
                    match cents {
                        Some(cv) => {
                            out.push_str(&integer_spoken(&num.int_digits, false));
                            out.push(' ');
                            out.push_str(if whole == 1 { one } else { many });
                            if cv > 0 {
                                out.push_str(" and ");
                                out.push_str(&cardinal(cv));
                                out.push_str(if cv == 1 { small_one } else { small_many });
                            }
                        }
                        None => {
                            out.push_str(&number_spoken(&num));
                            out.push(' ');
                            out.push_str(if whole == 1 && num.frac.is_empty() { one } else { many });
                        }
                    }
                    i = num.end;
                    continue;
                }
                out.push(' ');
            }
            // A leading minus sign ("-5", "(-3)") — but not a hyphen between words or a range.
            '-' if next_is_digit
                && out
                    .chars()
                    .last()
                    .is_none_or(|p| p.is_whitespace() || p == '(') =>
            {
                out.push_str("minus ");
            }
            // Dashes, ellipses and brackets read as a short pause.
            '\u{2014}' | '\u{2013}' | '\u{2026}' | '(' | ')' | '[' | ']' => out.push_str(", "),
            // A line break ends the sentence unless punctuation already did.
            '\n' => {
                let last = out.trim_end().chars().last();
                if matches!(last, Some('.' | '!' | '?' | ',' | ';' | ':') | None) {
                    out.push(' ');
                } else {
                    out.push_str(". ");
                }
            }
            '\u{2018}' | '\u{2019}' => out.push('\''),
            '_' => out.push(' '),
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn everyday_notations() {
        let lex = Lexicon::new();
        let say = |text: &str| normalize(text, &lex).split_whitespace().collect::<Vec<_>>().join(" ");
        // fractions, times, dates, telephone numbers
        assert_eq!(say("1/2 a cup and 3/4 full"), "one half a cup and three quarters full");
        assert_eq!(say("2/3 of 5/8"), "two thirds of five eighths");
        assert_eq!(say("1½ hours"), "one and a half hours");
        assert_eq!(say("2 1/2 hours, 3 3/4 cups"), "two and a half hours, three and three quarters cups");
        assert_eq!(say("on 2026-11-16: done"), "on november sixteenth, twenty twenty six: done");
        assert_eq!(say("on 16/11/2026"), "on sixteen/eleven/twenty twenty six");
        assert_eq!(say("at 10:30 PM, 9:05 or 7:00"), "at ten thirty pee em, nine oh five or seven o'clock");
        assert_eq!(say("at 10:30 AM or 9 am, 7:00 a.m. sharp"), "at ten thirty ay em or nine ay em, seven ay em sharp");
        assert_eq!(say("I am here at 7:00 amazing"), "I am here at seven o'clock amazing");
        // initials: letters, and their stops do not end the sentence
        assert_eq!(say("the U.S. economy and the U.S.A. grew"), "the you ess economy and the you ess ay grew");
        assert_eq!(say("J.K. Rowling moved to the U.K."), "jay kay Rowling moved to the you kay.");
        assert_eq!(say("€5.50, £1.01 and $3.20"), "five euros and fifty cents, one pound and one penny and three dollars and twenty cents");
        assert_eq!(say("on 2026-11-16"), "on november sixteenth, twenty twenty six");
        assert_eq!(say("call 555-1234"), "call five five five, one two three four");
        assert_eq!(say("at 5 p.m. sharp. Then"), "at five pee em sharp. Then");
        // units and degrees
        assert_eq!(say("5km in 25min at 12 kph"), "five kilometers in twenty five minutes at twelve kilometers per hour");
        assert_eq!(say("1 kg, 16 GB, 4x4, 10x"), "one kilogram, sixteen gigabytes, four by four, ten times");
        assert_eq!(say("3°C or 72 °F at 45°"), "three degrees celsius or seventy two degrees fahrenheit at forty five degrees");
        assert_eq!(say("5 in a row"), "five in a row");
        // Roman numerals only after a capitalised word
        assert_eq!(say("Henry VIII and World War II"), "Henry the eighth and World War two");
        assert_eq!(say("an IV drip"), "an eye vee drip");
        // abbreviations
        assert_eq!(say("St. Mary's on Elm St. in May"), "saint Mary's on Elm street in May");
        assert_eq!(say("He lives on Oak St. Then he left."), "He lives on Oak street. Then he left.");
        assert_eq!(say("Prof. Lee, Jr. spoke on Jan. 5"), "professor Lee, junior spoke on january five");
        assert_eq!(say("Acme Inc. The CEO"), "Acme incorporated. The CEO");
        assert_eq!(say("the URLs and QXZs"), "the url's and cue ex zee's");
        assert_eq!(say("we re-entered No. 5"), "we ree-entered number five");
        // addresses
        assert_eq!(say("visit https://example.com/faq"), "visit example dot com/faq");
        assert_eq!(say("mail bob@site.org."), "mail bob at site dot org.");
        assert_eq!(say("It ended.Then it began."), "It ended.Then it began.");
        // full-width letters
        assert_eq!(say("ｈｅｌｌｏ　ｗｏｒｌｄ"), "hello world");
    }

    fn norm(s: &str) -> String {
        normalize(s, &Lexicon::new())
    }

    #[test]
    fn numbers() {
        assert_eq!(cardinal(0), "zero");
        assert_eq!(cardinal(21), "twenty one");
        assert_eq!(cardinal(1234), "one thousand two hundred thirty four");
        assert_eq!(cardinal(2_000_005), "two million five");
        assert_eq!(ordinal(1), "first");
        assert_eq!(ordinal(21), "twenty first");
        assert_eq!(ordinal(40), "fortieth");
        assert_eq!(ordinal(112), "one hundred twelfth");
    }

    #[test]
    fn numbers_in_text() {
        assert_eq!(norm("I have 3 cats"), "I have three cats");
        assert_eq!(norm("In 2026 we ship"), "In twenty twenty six we ship");
        assert_eq!(norm("born in 1905."), "born in nineteen oh five.");
        assert_eq!(norm("pi is 3.14"), "pi is three point one four");
        assert_eq!(norm("1,250 people"), "one thousand two hundred fifty people");
        assert_eq!(norm("the 2nd and 23rd"), "the second and twenty third");
        assert_eq!(norm("the 1990s"), "the nineteen nineties");
        assert_eq!(norm("costs $5.99"), "costs five dollars and ninety nine cents");
        assert_eq!(norm("it is -4 outside"), "it is minus four outside");
        assert_eq!(norm("call 007"), "call zero zero seven");
    }

    #[test]
    fn symbols_and_abbreviations() {
        assert_eq!(norm("50% off"), "fifty percent  off");
        assert_eq!(norm("Dr. Smith"), "doctor Smith");
        assert_eq!(norm("440 Hz"), "four hundred forty hertz");
        assert_eq!(norm("100Hz"), "one hundred hertz");
        assert_eq!(norm("SoftN in 3D"), "soft en in three dee");
        assert_eq!(normalize("my CPU", &Lexicon::core_only()), "my see pee you");
        // an all-caps sentence is shouting, not a string of initialisms
        assert_eq!(norm("RUN NOW"), "RUN NOW");
        assert_eq!(norm("a naïve café"), "a naive cafe");
        assert_eq!(norm("wait — what (really)"), "wait ,  what , really, ");
        assert_eq!(norm("Title\nBody text."), "Title. Body text.");
        assert_eq!(norm("version 1.2.3"), "version one point two point three");
    }
}
