//! ARPAbet phone inventory used by the synthesizer.

/// Base phone (stress is carried separately, see [`Phone`]).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(u8)]
#[allow(clippy::upper_case_acronyms)]
pub enum Ph {
    SIL = 0,
    // Monophthong vowels
    IY,
    IH,
    EH,
    AE,
    AA,
    AO,
    UH,
    UW,
    AH,
    AX,
    ER,
    // Diphthongs
    EY,
    AY,
    OY,
    AW,
    OW,
    // Plosives & affricates
    P,
    B,
    T,
    D,
    K,
    G,
    CH,
    JH,
    // Fricatives & aspirate
    F,
    V,
    TH,
    DH,
    S,
    Z,
    SH,
    ZH,
    HH,
    // Nasals
    M,
    N,
    NG,
    // Liquids & glides
    L,
    R,
    W,
    Y,
}

impl Ph {
    /// Parses an upper-case ARPAbet symbol without stress digits.
    pub fn parse_bytes(name: &[u8]) -> Option<Ph> {
        Some(match name {
            b"SIL" => Ph::SIL,
            b"IY" => Ph::IY,
            b"IH" => Ph::IH,
            b"EH" => Ph::EH,
            b"AE" => Ph::AE,
            b"AA" => Ph::AA,
            b"AO" => Ph::AO,
            b"UH" => Ph::UH,
            b"UW" => Ph::UW,
            b"AH" => Ph::AH,
            b"AX" => Ph::AX,
            b"ER" => Ph::ER,
            b"EY" => Ph::EY,
            b"AY" => Ph::AY,
            b"OY" => Ph::OY,
            b"AW" => Ph::AW,
            b"OW" => Ph::OW,
            b"P" => Ph::P,
            b"B" => Ph::B,
            b"T" => Ph::T,
            b"D" => Ph::D,
            b"K" => Ph::K,
            b"G" => Ph::G,
            b"CH" => Ph::CH,
            b"JH" => Ph::JH,
            b"F" => Ph::F,
            b"V" => Ph::V,
            b"TH" => Ph::TH,
            b"DH" => Ph::DH,
            b"S" => Ph::S,
            b"Z" => Ph::Z,
            b"SH" => Ph::SH,
            b"ZH" => Ph::ZH,
            b"HH" | b"H" => Ph::HH,
            b"M" => Ph::M,
            b"N" => Ph::N,
            b"NG" => Ph::NG,
            b"L" => Ph::L,
            b"R" => Ph::R,
            b"W" => Ph::W,
            b"Y" => Ph::Y,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Ph::SIL => "SIL",
            Ph::IY => "IY",
            Ph::IH => "IH",
            Ph::EH => "EH",
            Ph::AE => "AE",
            Ph::AA => "AA",
            Ph::AO => "AO",
            Ph::UH => "UH",
            Ph::UW => "UW",
            Ph::AH => "AH",
            Ph::AX => "AX",
            Ph::ER => "ER",
            Ph::EY => "EY",
            Ph::AY => "AY",
            Ph::OY => "OY",
            Ph::AW => "AW",
            Ph::OW => "OW",
            Ph::P => "P",
            Ph::B => "B",
            Ph::T => "T",
            Ph::D => "D",
            Ph::K => "K",
            Ph::G => "G",
            Ph::CH => "CH",
            Ph::JH => "JH",
            Ph::F => "F",
            Ph::V => "V",
            Ph::TH => "TH",
            Ph::DH => "DH",
            Ph::S => "S",
            Ph::Z => "Z",
            Ph::SH => "SH",
            Ph::ZH => "ZH",
            Ph::HH => "HH",
            Ph::M => "M",
            Ph::N => "N",
            Ph::NG => "NG",
            Ph::L => "L",
            Ph::R => "R",
            Ph::W => "W",
            Ph::Y => "Y",
        }
    }

    pub fn ipa(self) -> &'static str {
        match self {
            Ph::SIL => "·",
            Ph::IY => "iː",
            Ph::IH => "ɪ",
            Ph::EH => "ɛ",
            Ph::AE => "æ",
            Ph::AA => "ɑː",
            Ph::AO => "ɔː",
            Ph::UH => "ʊ",
            Ph::UW => "uː",
            Ph::AH => "ʌ",
            Ph::AX => "ə",
            Ph::ER => "ɝ",
            Ph::EY => "eɪ",
            Ph::AY => "aɪ",
            Ph::OY => "ɔɪ",
            Ph::AW => "aʊ",
            Ph::OW => "oʊ",
            Ph::P => "p",
            Ph::B => "b",
            Ph::T => "t",
            Ph::D => "d",
            Ph::K => "k",
            Ph::G => "ɡ",
            Ph::CH => "tʃ",
            Ph::JH => "dʒ",
            Ph::F => "f",
            Ph::V => "v",
            Ph::TH => "θ",
            Ph::DH => "ð",
            Ph::S => "s",
            Ph::Z => "z",
            Ph::SH => "ʃ",
            Ph::ZH => "ʒ",
            Ph::HH => "h",
            Ph::M => "m",
            Ph::N => "n",
            Ph::NG => "ŋ",
            Ph::L => "l",
            Ph::R => "ɹ",
            Ph::W => "w",
            Ph::Y => "j",
        }
    }

    #[inline]
    pub fn is_vowel(self) -> bool {
        let v = self as u8;
        v >= Ph::IY as u8 && v <= Ph::OW as u8
    }

    #[inline]
    pub fn is_voiced(self) -> bool {
        self.is_vowel()
            || matches!(
                self,
                Ph::B
                    | Ph::D
                    | Ph::G
                    | Ph::JH
                    | Ph::V
                    | Ph::DH
                    | Ph::Z
                    | Ph::ZH
                    | Ph::M
                    | Ph::N
                    | Ph::NG
                    | Ph::L
                    | Ph::R
                    | Ph::W
                    | Ph::Y
            )
    }

    /// Sibilants take the syllabic `-es` / `'s` ending ("voices", "watch's").
    #[inline]
    pub fn is_sibilant(self) -> bool {
        matches!(self, Ph::S | Ph::Z | Ph::SH | Ph::ZH | Ph::CH | Ph::JH)
    }
}

/// A phone with lexical stress: 1 = primary, 2 = secondary, 0 = unstressed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Phone {
    pub ph: Ph,
    pub stress: u8,
}

impl Phone {
    pub const fn new(ph: Ph, stress: u8) -> Phone {
        Phone { ph, stress }
    }

    /// Parses `"AY1"`, `"hh"`, `"AX0"` … (case-insensitive, stress digit optional).
    pub fn parse(tok: &str) -> Option<Phone> {
        let mut buf = [0u8; 3];
        let mut n = 0usize;
        let mut stress = 0u8;
        for b in tok.bytes() {
            match b {
                b'1' => stress = 1,
                b'2' => {
                    if stress != 1 {
                        stress = 2
                    }
                }
                b'0' => {}
                _ => {
                    if n >= 3 {
                        return None;
                    }
                    buf[n] = b.to_ascii_uppercase();
                    n += 1;
                }
            }
        }
        Ph::parse_bytes(&buf[..n]).map(|ph| Phone { ph, stress })
    }

    /// ARPAbet rendering with a stress digit on vowels (`"OW1"`, `"T"`).
    pub fn arpabet(self) -> String {
        if self.ph.is_vowel() {
            format!("{}{}", self.ph.name(), self.stress)
        } else {
            self.ph.name().to_string()
        }
    }
}

/// Parses a whitespace/underscore separated phone string.
pub fn parse_phones(s: &str) -> Option<Vec<Phone>> {
    let mut out = Vec::new();
    for tok in s.split(|c: char| c.is_whitespace() || c == '_') {
        if tok.is_empty() {
            continue;
        }
        out.push(Phone::parse(tok)?);
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
