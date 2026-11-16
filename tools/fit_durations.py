"""Fits the synthesizer's phone-duration model to real speech (development tooling).

For a few hundred CMU Arctic utterances (two speakers) it lines up the hand-labelled phone
durations with the context features the engine's timeline code sees for the same text
(`VS_FEATURES_FILE` hook), then fits a multiplicative model

    duration = base[phone] x product of context factors

by least squares on log-duration. The fitted numbers are pasted into src/durations.rs;
nothing here runs inside the synthesizer.

  python tools/fit_durations.py collect     # engine features + natural durations -> .cache/durations.tsv
  python tools/fit_durations.py fit         # prints the Rust table and error statistics
  python tools/fit_durations.py compare     # how far the engine's current durations are from natural
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402
from transplant import labels, match  # noqa: E402

DATA = E.CACHE / "durations.tsv"
TRAIN = range(17, 281)  # sentences 1-16 are the evaluation set; 281-320 are held out
TEST = range(281, 321)
COLS = "utt spk token phone stress content w_init w_final cf_word nuclear n_sylls last_syl onset_stress prev next prev_same next_same to_end punc rule_ms nat_ms".split()
VOWELS = {"IY", "IH", "EH", "AE", "AA", "AO", "UH", "UW", "AH", "AX", "ER", "EY", "AY", "OY", "AW", "OW"}
STOPS = {"P", "B", "T", "D", "K", "G", "CH", "JH"}
VOICELESS = {"P", "T", "K", "CH", "F", "TH", "S", "SH", "HH"}
NASALS = {"M", "N", "NG"}
LIQUIDS = {"L", "R", "W", "Y"}


def prompts() -> dict[int, str]:
    import re

    out = {}
    for line in (E.REF / "bdl.txt").read_text().splitlines():
        m = re.match(r'\( *arctic_a(\d+) +"(.*)" *\)', line)
        if m:
            out[int(m.group(1))] = m.group(2)
    return out


def engine_features(text: str) -> list[list[str]]:
    feat = E.CACHE / "features.txt"
    feat.unlink(missing_ok=True)
    env = dict(os.environ, VS_FEATURES_FILE=str(feat))
    subprocess.run([str(E.SAY), "--timing", "--phones", "--dict", str(E.DICT), text], check=True, capture_output=True, env=env)
    return [l.split(" ") for l in feat.read_text(encoding="utf-8").splitlines() if l != "END"]


def collect() -> None:
    texts = prompts()
    rows = []
    for idx in list(TRAIN) + list(TEST):
        feats = engine_features(texts[idx])
        syn = [(int(f[0]), f[1], 0.0, 0.0) for f in feats]
        for spk in ("bdl", "slt"):
            nat = labels(spk, idx)
            for k, j in match(nat, syn):
                if nat[k][0] != {"AX": "AX"}.get(syn[j][1], syn[j][1]) and {nat[k][0], syn[j][1]} != {"AX", "AH"}:
                    continue  # only phones both sides agree on
                rows.append([str(idx), spk] + feats[j] + [f"{(nat[k][2] - nat[k][1]) * 1000:.1f}"])
    DATA.write_text("\t".join(COLS) + "\n" + "\n".join("\t".join(r) for r in rows) + "\n", encoding="utf-8")
    print(f"collected {len(rows)} phone tokens from {len(TRAIN) + len(TEST)} sentences x 2 speakers -> {DATA}")


def load() -> list[dict]:
    lines = DATA.read_text(encoding="utf-8").splitlines()
    out = []
    for line in lines[1:]:
        d = dict(zip(COLS, line.split("\t")))
        for k in ("utt", "token", "stress", "content", "w_init", "w_final", "cf_word", "nuclear", "n_sylls", "last_syl", "onset_stress", "prev_same", "next_same", "to_end"):
            d[k] = int(d[k])
        d["rule_ms"] = float(d["rule_ms"])
        d["nat_ms"] = float(d["nat_ms"])
        out.append(d)
    return out


def features(d: dict) -> list[str]:
    """Names of the context factors that apply to one phone token (mirrors src/durations.rs)."""
    ph = d["phone"]
    f = [f"base:{ph}"]
    nxt, prv = d["next"], d["prev"]
    final_pos = d["to_end"] == 1
    if ph in VOWELS:
        stress = f"{'c' if d['content'] else 'f'}{d['stress']}"
        if stress != "c1":
            f.append(f"v_stress:{stress}")
        if d["nuclear"] and d["stress"] == 1:
            f.append("v_nuclear")
        if d["cf_word"]:
            f.append("v_final_last" if d["last_syl"] else "v_final_early")
        elif d["last_syl"] and d["n_sylls"] > 1:
            f.append("v_word_last")
        if final_pos or nxt == "SIL":
            f.append("v_open_final")
        elif nxt in VOWELS:
            f.append("v_next:vowel")
        elif nxt in LIQUIDS:
            f.append("v_next:liquid")
        if not d["prev_same"]:
            f.append("v_word_initial")
    else:
        prev_cons = d["prev_same"] and prv not in VOWELS
        next_cons = d["next_same"] and nxt not in VOWELS
        if d["w_init"]:
            f.append("c_initial:c" if d["content"] else "c_initial:f")
        elif d["w_final"]:
            f.append("c_final")
        if prev_cons:
            f.append("c_after_cons")
        if next_cons:
            f.append("c_before_cons")
        if d["cf_word"] and d["last_syl"]:
            f.append("c_phrase_final" if final_pos else "c_final_coda")
        if not d["last_syl"] and d["onset_stress"] == 1 and not next_cons:
            f.append("c_stressed_onset")
        if not d["content"]:
            f.append("c_function")
        if ph in ("T", "D") and prv in VOWELS and nxt in VOWELS and d["onset_stress"] != 1 and not d["w_init"]:
            f.append("c_flap")
    return f


def design(rows: list[dict], names: list[str] | None = None):
    feats = [features(d) for d in rows]
    if names is None:
        names = sorted({n for f in feats for n in f})
    index = {n: i for i, n in enumerate(names)}
    x = np.zeros((len(rows), len(names)))
    for r, f in enumerate(feats):
        for n in f:
            if n in index:
                x[r, index[n]] = 1.0
    return x, names


def fit() -> None:
    rows = load()
    train = [d for d in rows if d["utt"] in TRAIN and 15 <= d["nat_ms"] <= 400]
    test = [d for d in rows if d["utt"] in TEST and 15 <= d["nat_ms"] <= 400]
    x, names = design(train)
    y = np.log(np.array([d["nat_ms"] for d in train]))
    lam = 2.0
    w = np.linalg.solve(x.T @ x + lam * np.eye(len(names)), x.T @ y)
    # ridge shrinks toward 0 = a factor of 1; base terms should not be shrunk, so refit them
    base_cols = [i for i, n in enumerate(names) if n.startswith("base:")]
    resid = y - x @ w
    for i in base_cols:
        mask = x[:, i] > 0
        if mask.sum():
            w[i] += resid[mask].mean()

    def report(tag, data):
        xd, _ = design(data, names)
        nat = np.log(np.array([d["nat_ms"] for d in data]))
        rule = np.log(np.array([max(d["rule_ms"], 5.0) for d in data]))
        fitd = xd @ w
        rmse = lambda a: float(np.sqrt(np.mean((a - nat) ** 2)))  # noqa: E731
        corr = lambda a: float(np.corrcoef(a, nat)[0, 1])  # noqa: E731
        print(f"  {tag}: engine now  rmse(log)={rmse(rule):.3f} corr={corr(rule):.3f} | fitted model rmse(log)={rmse(fitd):.3f} corr={corr(fitd):.3f}  (n={len(data)})")
        return nat - fitd

    print("duration error vs real speakers (log units; 0.30 is about 30%)")
    report("train", train)
    res = report("held-out", test)
    xt, _ = design(test, names)
    nat_t = np.log(np.array([d["nat_ms"] for d in test]))
    old_rmse = float(np.sqrt(np.mean((np.log(np.array([max(d["rule_ms"], 5.0) for d in test])) - nat_t) ** 2)))
    new_rmse = float(np.sqrt(np.mean((xt @ w - nat_t) ** 2)))

    # how much real speakers vary around the model: per-word tempo and per-phone residue
    by_word: dict = {}
    for d, r in zip(test, res):
        by_word.setdefault((d["utt"], d["spk"], d["token"] - (0 if d["w_init"] else 1)), []).append(r)
    words: dict = {}
    wid = -1
    last_key = None
    for d, r in zip(test, res):
        key = (d["utt"], d["spk"])
        if key != last_key or d["w_init"]:
            wid += 1
        last_key = key
        words.setdefault(wid, []).append(r)
    means = np.array([np.mean(v) for v in words.values() if len(v) >= 2])
    within = np.concatenate([np.array(v) - np.mean(v) for v in words.values() if len(v) >= 2])
    print(f"  what the model leaves unexplained: per phone sd={np.std(res):.3f}; whole-word tempo sd={np.std(means):.3f}; within-word sd={np.std(within):.3f}")

    coef = {n: float(np.exp(v)) for n, v in zip(names, w)}
    g = lambda n: coef.get(n, 1.0)  # noqa: E731
    bases = "\n".join(f"        Ph::{n[5:]} => {coef[n]:.1f}," for n in names if n.startswith("base:"))
    consts = [
        ("V_UNSTRESSED", "v_stress:c0", "unstressed vowel in a content word"),
        ("V_SECONDARY", "v_stress:c2", "secondary stress"),
        ("V_FUNCTION", "v_stress:f0", "vowel of a (non-final) function word"),
        ("V_NUCLEAR", "v_nuclear", "the phrase's main accent"),
        ("V_PHRASE_FINAL_SYLLABLE", "v_final_last", "last syllable before a pause"),
        ("V_PHRASE_FINAL_WORD", "v_final_early", "earlier syllables of the last word"),
        ("V_WORD_FINAL_SYLLABLE", "v_word_last", "last syllable of a longer word"),
        ("V_PHRASE_FINAL_OPEN", "v_open_final", "nothing after the vowel at all"),
        ("V_BEFORE_VOWEL", "v_next:vowel", "hiatus"),
        ("V_BEFORE_LIQUID", "v_next:liquid", "the liquid/glide takes over part of the vowel"),
        ("V_WORD_INITIAL", "v_word_initial", "word starts with this vowel"),
        ("C_INITIAL_CONTENT", "c_initial:c", "first sound of a content word"),
        ("C_INITIAL_FUNCTION", "c_initial:f", "first sound of a function word"),
        ("C_FINAL", "c_final", "last sound of a word"),
        ("C_AFTER_CONSONANT", "c_after_cons", "second member of a cluster"),
        ("C_BEFORE_CONSONANT", "c_before_cons", "first member of a cluster"),
        ("C_PHRASE_FINAL", "c_phrase_final", "the very last sound before a pause"),
        ("C_PHRASE_FINAL_CODA", "c_final_coda", "other coda consonants of the last syllable"),
        ("C_STRESSED_ONSET", "c_stressed_onset", "starts a stressed syllable"),
        ("C_FUNCTION", "c_function", "inside a function word"),
        ("C_FLAP", "c_flap", "t/d between vowels before an unstressed syllable"),
    ]
    const_lines = "\n".join(f"const {name}: f64 = {g(key):.3f}; // {doc}" for name, key, doc in consts)
    rust = RUST_TEMPLATE.replace("@BASES@", bases).replace("@CONSTS@", const_lines)
    rust = rust.replace("@N@", str(len(train))).replace("@RMSE_OLD@", f"{old_rmse:.2f}").replace("@RMSE_NEW@", f"{new_rmse:.2f}")
    (E.ROOT / "src" / "durations.rs").write_text(rust, encoding="utf-8")
    print("wrote src/durations.rs")
    for name, key, _ in consts:
        print(f"  {name:26s} x{g(key):.3f}")


RUST_TEMPLATE = """//! Phone durations: `base duration of the sound x context factors`.
//!
//! @generated by tools/fit_durations.py — fitted by least squares (on log duration) to
//! @N@ labelled phones from two speakers of the CMU Arctic corpus. On held-out sentences
//! the hand-written rules this replaced were off by a factor of e^@RMSE_OLD@ on average; this model
//! by e^@RMSE_NEW@ (the rest is speakers simply not timing a word the same way twice).

use crate::phone::Ph;

/// What the timeline knows about a phone's surroundings.
pub(crate) struct Context {
    pub stress: u8,
    /// False for function words (the, of, and ...) that are not phrase-final.
    pub is_content: bool,
    pub word_initial: bool,
    pub word_final: bool,
    /// The phone belongs to the last word before a pause.
    pub phrase_final_word: bool,
    /// The phone belongs to the word carrying the phrase's main accent.
    pub nuclear: bool,
    pub n_sylls: usize,
    /// No vowel follows inside the word (last syllable, or its coda).
    pub last_syllable: bool,
    /// Stress of the syllable a consonant leads into (0 if none in this word).
    pub onset_stress: u8,
    /// Neighbouring phones within the phrase (`Ph::SIL` at its edges).
    pub prev: Ph,
    pub next: Ph,
    pub prev_same_word: bool,
    pub next_same_word: bool,
}

/// Duration in ms of a stressed vowel in a one-syllable content word before a voiced
/// consonant, or of a word-medial single consonant.
pub(crate) fn base_ms(ph: Ph) -> f64 {
    match ph {
@BASES@
        Ph::SIL => 80.0,
    }
}

@CONSTS@

fn is_approximant(ph: Ph) -> bool {
    matches!(ph, Ph::L | Ph::R | Ph::W | Ph::Y)
}

/// Natural duration in milliseconds of `ph` in context `c`, at a normal speaking rate.
pub(crate) fn duration_ms(ph: Ph, c: &Context) -> f64 {
    let mut d = base_ms(ph);
    let phrase_final = c.next == Ph::SIL;
    if ph.is_vowel() {
        d *= match (c.is_content, c.stress) {
            (_, 1) => 1.0,
            (_, 2) => V_SECONDARY,
            (true, _) => V_UNSTRESSED,
            (false, _) => V_FUNCTION,
        };
        if c.nuclear && c.stress == 1 {
            d *= V_NUCLEAR;
        }
        if c.phrase_final_word {
            d *= if c.last_syllable { V_PHRASE_FINAL_SYLLABLE } else { V_PHRASE_FINAL_WORD };
        } else if c.last_syllable && c.n_sylls > 1 {
            d *= V_WORD_FINAL_SYLLABLE;
        }
        if phrase_final {
            d *= V_PHRASE_FINAL_OPEN;
        } else if c.next.is_vowel() {
            d *= V_BEFORE_VOWEL;
        } else if is_approximant(c.next) {
            d *= V_BEFORE_LIQUID;
        }
        if !c.prev_same_word {
            d *= V_WORD_INITIAL;
        }
    } else {
        let after_consonant = c.prev_same_word && !c.prev.is_vowel();
        let before_consonant = c.next_same_word && !c.next.is_vowel();
        if c.word_initial {
            d *= if c.is_content { C_INITIAL_CONTENT } else { C_INITIAL_FUNCTION };
        } else if c.word_final {
            d *= C_FINAL;
        }
        if after_consonant {
            d *= C_AFTER_CONSONANT;
        }
        if before_consonant {
            d *= C_BEFORE_CONSONANT;
        }
        if c.phrase_final_word && c.last_syllable {
            d *= if phrase_final { C_PHRASE_FINAL } else { C_PHRASE_FINAL_CODA };
        }
        if !c.last_syllable && c.onset_stress == 1 && !before_consonant {
            d *= C_STRESSED_ONSET;
        }
        if !c.is_content {
            d *= C_FUNCTION;
        }
        if matches!(ph, Ph::T | Ph::D)
            && c.prev.is_vowel()
            && c.next.is_vowel()
            && c.onset_stress != 1
            && !c.word_initial
        {
            d *= C_FLAP;
        }
    }
    d
}
"""


def compare() -> None:
    rows = [d for d in load() if 15 <= d["nat_ms"] <= 400]
    print("phone   n     natural  engine   ratio(engine/natural)")
    groups: dict = {}
    for d in rows:
        groups.setdefault(d["phone"], []).append(d)
    for ph, g in sorted(groups.items(), key=lambda kv: -len(kv[1])):
        nat = np.exp(np.mean(np.log([d["nat_ms"] for d in g])))
        rule = np.exp(np.mean(np.log([max(d["rule_ms"], 5) for d in g])))
        print(f"  {ph:4s} {len(g):5d}  {nat:6.0f}   {rule:6.0f}   {rule / nat:5.2f}")


if __name__ == "__main__":
    {"collect": collect, "fit": fit, "compare": compare}[sys.argv[1] if len(sys.argv) > 1 else "fit"]()
