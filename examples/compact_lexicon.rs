//! Development tool: which lexicon entries do the letter-to-sound rules make redundant?
//!
//! Takes a list of words with the pronunciation each must have (`word PH PH1 …` per line, in
//! the lexicon's own notation; a leading `!` marks a word that has to stay listed whatever
//! the rules say) and writes the smallest lexicon with which the synthesizer pronounces every
//! one of them exactly so: a word is listed only if lexicon lookup, inflection stripping and
//! the rules together get it wrong without it. Run through `tools/build_lexicon.py`.
//!
//!     cargo run --release --example compact_lexicon -- <truth file> <output lexicon>

use std::collections::BTreeMap;
use tts::lexicon::Lexicon;
use tts::phone::{parse_phones, Phone};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: compact_lexicon <truth file> <output lexicon>");
        std::process::exit(2);
    }
    let text = std::fs::read_to_string(&args[0]).expect("could not read the truth file");
    let base = Lexicon::base();
    let mut lex = Lexicon::base();
    let mut kept: BTreeMap<String, String> = BTreeMap::new();
    let mut truth: Vec<(String, Vec<Phone>, String)> = Vec::new();
    for line in text.lines() {
        let (forced, line) = match line.strip_prefix('!') {
            Some(rest) => (true, rest),
            None => (false, line),
        };
        let Some((word, phones)) = line.split_once(' ') else { continue };
        if base.contains(word) {
            continue; // the hand-made core lexicon wins anyway
        }
        let Some(parsed) = parse_phones(phones) else { continue };
        if forced {
            kept.insert(word.to_string(), line.to_string());
        }
        truth.push((word.to_string(), parsed, line.to_string()));
    }
    let load = |lex: &mut Lexicon, lines: &[&str]| {
        lex.load_entries(&lines.join("\n"));
    };
    load(&mut lex, &kept.values().map(|s| s.as_str()).collect::<Vec<_>>());
    let forced = kept.len();
    for round in 1.. {
        let wrong: Vec<&(String, Vec<Phone>, String)> =
            truth.iter().filter(|(w, p, _)| !kept.contains_key(w) && lex.pronounce(w) != *p).collect();
        eprintln!("round {round}: {} more words need an entry", wrong.len());
        if let Ok(list) = std::env::var("VS_LEX_DEBUG") {
            let show = |ps: &[Phone]| ps.iter().map(|p| p.arpabet()).collect::<Vec<_>>().join(" ");
            for (w, p, _) in wrong.iter().filter(|t| list.split(',').any(|x| x == t.0)) {
                eprintln!("  {w}: got {} | want {}", show(&lex.pronounce(w)), show(p));
            }
        }
        if wrong.is_empty() {
            break;
        }
        load(&mut lex, &wrong.iter().map(|t| t.2.as_str()).collect::<Vec<_>>());
        for (w, _, line) in wrong {
            kept.insert(w.clone(), line.clone());
        }
    }
    let bad = truth.iter().filter(|(w, p, _)| lex.pronounce(w) != *p).count();
    assert_eq!(bad, 0, "the compacted lexicon must reproduce every pronunciation");
    let mut out = String::new();
    for line in kept.values() {
        out.push_str(line);
        out.push('\n');
    }
    std::fs::write(&args[1], &out).expect("could not write the lexicon");
    println!(
        "{} words pronounced exactly; {} listed ({} because they must stay listed), {} left to the rules; {} KB",
        truth.len(),
        kept.len(),
        forced,
        truth.len() - kept.len(),
        out.len() / 1024
    );
}
