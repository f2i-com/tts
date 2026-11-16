"""Letter-to-sound rules for words the built-in lexicon does not know (development tooling).

The rules are derived from the CMU Pronouncing Dictionary, offline:

  1. every word's letters are lined up with its phonemes (each letter stands for no sound,
     one sound or two - "x" = K S), by expectation-maximisation;
  2. for every letter, the sound it stands for is tabulated against its neighbours: first the
     letter alone, then with the next letter, then also the previous one, ... up to four
     letters on each side. A context is kept only where it changes the answer;
  3. which of the full vowels carries the main stress is tabulated against the word's ending.

At run time the synthesizer looks each letter up, longest context first (`src/g2p.rs`). The
same tables decide which lexicon entries are redundant, so the embedded lexicon only has to
hold the words the rules get wrong (`examples/compact_lexicon.rs`).

  python tools/train_g2p.py                 # train on everything, write src/g2p_rules.txt
  python tools/train_g2p.py --holdout       # accuracy on a tenth of the dictionary not trained on
"""
from __future__ import annotations

import math
import pickle
import random
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / "tools" / ".cache"
DICT = CACHE / "cmudict.dict"
OUT = ROOT / "src" / "g2p_rules.txt"

VOWELS = {"AA", "AE", "AH", "AO", "AW", "AY", "EH", "ER", "EY", "IH", "IY", "OW", "OY", "UH", "UW", "AX"}
# context chain: (letters to the left, letters to the right); each level adds one letter
LEVELS = [(0, 0), (0, 1), (1, 1), (1, 2), (2, 2), (2, 3), (3, 3), (3, 4), (4, 4)]
MIN_COUNT = [1, 1, 1, 1, 1, 2, 2, 3, 3]
# the same for "what does this letter sound like when it is a full vowel"
FULL_LEVELS = [(0, 0), (0, 1), (1, 1), (1, 2)]
SUFFIX_MAX = 6


def load_cmudict() -> dict[str, list[str]]:
    out = {}
    for line in DICT.read_text(encoding="utf-8").splitlines():
        line = line.split(" #")[0].strip()
        if not line:
            continue
        word, *phones = line.split()
        if "(" in word or not re.fullmatch(r"[a-z]+", word):
            continue
        out[word] = phones
    return out


def base(ph: str) -> str:
    return ph.rstrip("012")


def target_phone(ph: str) -> str:
    """CMUdict phone -> rule symbol: vowels carry 1 (full: stress 1 or 2) or 0 (unstressed)."""
    b = base(ph)
    if b not in VOWELS:
        return b
    if ph[-1] == "0":
        return "AX0" if b == "AH" else b + "0"
    return b + "1"


# ----------------------------------------------------------------------------- alignment
def viterbi(letters: str, phones: list[str], score) -> list[tuple[str, ...]] | None:
    """Best split of `phones` over `letters` (0, 1 or 2 phones per letter)."""
    n, m = len(letters), len(phones)
    if m > 2 * n:
        return None
    NEG = -1e18
    best = [[NEG] * (m + 1) for _ in range(n + 1)]
    back = [[0] * (m + 1) for _ in range(n + 1)]
    best[0][0] = 0.0
    for i in range(n):
        c = letters[i]
        row, nxt = best[i], best[i + 1]
        for j in range(m + 1):
            v = row[j]
            if v <= NEG:
                continue
            for k in (0, 1, 2):
                if j + k > m:
                    break
                s = v + score(c, tuple(phones[j : j + k]), i, j, n, m)
                if s > nxt[j + k]:
                    nxt[j + k] = s
                    back[i + 1][j + k] = k
    if best[n][m] <= NEG:
        return None
    out, j = [], m
    for i in range(n, 0, -1):
        k = back[i][j]
        out.append(tuple(phones[j - k : j]))
        j -= k
    return out[::-1]


def align_all(words: dict[str, list[str]], iterations: int = 5) -> dict[str, list[tuple[str, ...]]]:
    cache = CACHE / "g2p_align.pkl"
    if cache.exists():
        got = pickle.loads(cache.read_bytes())
        if got.get("n") == len(words):
            return got["align"]
    stripped = {w: [base(p) for p in ph] for w, ph in words.items()}

    def prior(c, ph, i, j, n, m):
        # before anything is known: stay near the diagonal, prefer one sound per letter
        k = len(ph)
        pos = abs((i + 0.5) / n - (j + k / 2) / max(m, 1))
        return -4.0 * pos - (0.0 if k == 1 else 1.5 if k == 0 else 3.0)

    score = prior
    align = {}
    for it in range(iterations):
        counts: dict[str, Counter] = defaultdict(Counter)
        align = {}
        for w, ph in stripped.items():
            a = viterbi(w, ph, score)
            if a is None:
                continue
            align[w] = a
            for c, p in zip(w, a):
                counts[c][p] += 1
        logp = {}
        for c, cnt in counts.items():
            total = sum(cnt.values())
            logp[c] = {p: math.log((v + 0.01) / (total + 1.0)) for p, v in cnt.items()}

        def learned(c, ph, i, j, n, m, logp=logp):
            return logp[c].get(ph, -14.0 - 2.0 * len(ph))

        score = learned
        print(f"  alignment pass {it + 1}: {len(align)} of {len(stripped)} words", file=sys.stderr)
    # put the stress digits back
    out = {}
    for w, a in align.items():
        full = iter(words[w])
        out[w] = [tuple(next(full) for _ in group) for group in a]
    cache.write_bytes(pickle.dumps({"n": len(words), "align": out}))
    return out


# ----------------------------------------------------------------------------- rules
def context_key(word: str, i: int, left: int, right: int, mark: str = "") -> str:
    padded = "####" + word + "####"
    k = i + 4
    return mark + padded[k - left : k] + "." + padded[k] + "." + padded[k + 1 : k + 1 + right]


def prune(tables: list, levels: list, min_count, mark: str = "") -> dict[str, str]:
    """Keep a context only where its most frequent answer differs from the answer the next
    shorter stored context gives."""
    rules: dict[str, str] = {}
    for lv in range(len(levels)):
        new = {}
        for key, cnt in tables[lv].items():
            if sum(cnt.values()) < min_count(lv):
                continue
            target, _ = cnt.most_common(1)[0]
            l_ctx, c, r_ctx = key[len(mark) :].split(".")
            inherited = None
            for lower in range(lv - 1, -1, -1):
                pl, pr = levels[lower]
                inherited = rules.get(mark + l_ctx[len(l_ctx) - pl :] + "." + c + "." + r_ctx[:pr])
                if inherited is not None:
                    break
            if inherited != target:
                new[key] = target
        rules.update(new)
    return rules


def train_rules(align: dict[str, list[tuple[str, ...]]]) -> dict[str, str]:
    tables = [defaultdict(Counter) for _ in LEVELS]
    full = [defaultdict(Counter) for _ in FULL_LEVELS]
    for w, a in align.items():
        for i, group in enumerate(a):
            target = " ".join(target_phone(p) for p in group) or "_"
            for lv, (left, right) in enumerate(LEVELS):
                tables[lv][context_key(w, i, left, right)][target] += 1
            if target[-1] == "1":
                for lv, (left, right) in enumerate(FULL_LEVELS):
                    full[lv][context_key(w, i, left, right, "+")][target] += 1
    rules = prune(tables, LEVELS, lambda lv: MIN_COUNT[lv])
    # what a vowel letter sounds like when it has to be a full vowel (used when the rules
    # leave a word without any)
    rules.update(prune(full, FULL_LEVELS, lambda lv: 3, "+"))
    return rules


def apply_rules(rules: dict[str, str], word: str) -> list[str]:
    groups = []
    for i in range(len(word)):
        target = None
        for left, right in reversed(LEVELS):
            target = rules.get(context_key(word, i, left, right))
            if target is not None:
                break
        groups.append(target.split() if target and target != "_" else [])
    # two or more vowel letters in a row cannot all be silent ("feebly"), except a final "ue"
    i = 0
    while i < len(word):
        j = i
        while j < len(word) and word[j] in "aeiou":
            j += 1
        if j - i >= 2 and not any(p[-1] in "01" for g in groups[i:j] for p in g) and not (j == len(word) and word[i:j] == "ue"):
            for left, right in reversed(FULL_LEVELS):
                alt = rules.get(context_key(word, i, left, right, "+"))
                if alt is not None:
                    groups[i] = alt.split()
                    break
        i = max(j, i + 1)
    if not any(p[-1] == "1" for g in groups for p in g):
        # a word needs a full vowel: the first vowel letter that has a sound becomes one
        for i, g in enumerate(groups):
            if any(p[-1] == "0" for p in g):
                for left, right in reversed(FULL_LEVELS):
                    alt = rules.get(context_key(word, i, left, right, "+"))
                    if alt is not None:
                        groups[i] = alt.split()
                        break
                else:
                    groups[i] = [("AH1" if p == "AX0" else p[:-1] + "1") if p[-1] == "0" else p for p in g]
                break
    return [p for g in groups for p in g]


# ----------------------------------------------------------------------------- stress
def stress_key(n_full: int, word: str, k: int) -> str:
    return f"{min(n_full, 5)}:{word[len(word) - k :] if k else ''}"


def train_stress(words: dict[str, list[str]]) -> dict[str, int]:
    """{'<full vowels>:<suffix>': index of the main stress among the full vowels, from the end}."""
    tables = [defaultdict(Counter) for _ in range(SUFFIX_MAX + 1)]
    for w, phones in words.items():
        marks = [p[-1] for p in phones if base(p) in VOWELS and p[-1] in "12"]
        if len(marks) < 2 or "1" not in marks:
            continue
        from_end = len(marks) - 1 - marks.index("1")
        for k in range(min(SUFFIX_MAX, len(w)) + 1):
            tables[k][stress_key(len(marks), w, k)][from_end] += 1
    rules: dict[str, int] = {}
    for k in range(SUFFIX_MAX + 1):
        new = {}
        for key, cnt in tables[k].items():
            if sum(cnt.values()) < (1 if k < 4 else 2):
                continue
            target, _ = cnt.most_common(1)[0]
            n_full, suffix = key.split(":")
            inherited = None
            for shorter in range(k - 1, -1, -1):
                inherited = rules.get(f"{n_full}:{suffix[len(suffix) - shorter :] if shorter else ''}")
                if inherited is not None:
                    break
            if inherited != target:
                new[key] = target
        rules.update(new)
    return rules


def apply_stress(stress: dict[str, int], word: str, phones: list[str]) -> list[str]:
    """Rule symbols -> CMUdict-style phones with 0/1/2 stress."""
    out = list(phones)
    full = [k for k, p in enumerate(out) if p[-1] == "1"]
    if not full:
        return out
    main = full[-1]
    if len(full) > 1:
        from_end = None
        for k in range(min(SUFFIX_MAX, len(word)), -1, -1):
            from_end = stress.get(stress_key(len(full), word, k))
            if from_end is not None:
                break
        from_end = min(from_end if from_end is not None else len(full) - 1, len(full) - 1)
        main = full[len(full) - 1 - from_end]
    for k in full:
        out[k] = out[k][:-1] + ("1" if k == main else "2")
    return out


def predict(rules, stress, word: str) -> list[str]:
    return apply_stress(stress, word, apply_rules(rules, word))


def normal(phones: list[str]) -> list[str]:
    """CMUdict pronunciation in the form the rules produce (AH0 = AX0)."""
    return ["AX0" if p == "AH0" else p for p in phones]


# ----------------------------------------------------------------------------- main
def accuracy(rules, stress, words: dict[str, list[str]]):
    right = right_phones = right_reduced = 0
    for w, phones in words.items():
        got = predict(rules, stress, w)
        want = normal(phones)
        right += got == want
        right_phones += [base(p) for p in got] == [base(p) for p in want]
        flat = lambda ps: [p[:-1] + ("0" if p[-1] == "0" else "1") if p[-1] in "012" else p for p in ps]  # noqa: E731
        right_reduced += flat(got) == flat(want)
    n = max(len(words), 1)
    return right / n, right_reduced / n, right_phones / n


def write_rules(rules: dict[str, str], stress: dict[str, int]) -> None:
    lines = ["# Letter-to-sound rules derived from the CMU Pronouncing Dictionary by tools/train_g2p.py."]
    lines.append("# <left context>.<letter>.<right context> <sounds>   (_ = silent; vowels: 1 = full, 0 = unstressed)")
    lines.append("# +<context> <sounds>: the letter when it has to be a full vowel")
    for key in sorted(rules, key=lambda k: (k.startswith("+"), len(k), k)):
        lines.append(f"{key} {rules[key]}")
    lines.append("# main stress: =<number of full vowels>:<word ending> <which full vowel, counted from the end>")
    for key in sorted(stress, key=lambda k: (len(k), k)):
        lines.append(f"={key} {stress[key]}")
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {OUT.name}: {len(rules)} letter rules, {len(stress)} stress rules, {OUT.stat().st_size / 1024:.0f} KB")


def main() -> None:
    words = load_cmudict()
    align = align_all(words)
    print(f"{len(words)} words, {len(align)} aligned")
    if "--holdout" in sys.argv:
        rng = random.Random(7)
        names = sorted(align)
        rng.shuffle(names)
        test = set(names[: len(names) // 10])
        tr_align = {w: a for w, a in align.items() if w not in test}
        rules = train_rules(tr_align)
        stress = train_stress({w: words[w] for w in tr_align})
        held = {w: words[w] for w in test}
        full, reduced, phones = accuracy(rules, stress, held)
        print(f"held-out words ({len(held)}): exactly right {100 * full:.1f}%, right up to which vowel has the main stress {100 * reduced:.1f}%, phonemes right {100 * phones:.1f}%")
        print(f"rules: {len(rules)} letter contexts, {len(stress)} stress endings")
        return
    rules = train_rules(align)
    stress = train_stress({w: words[w] for w in align})
    full, reduced, phones = accuracy(rules, stress, {w: words[w] for w in align})
    print(f"on the dictionary itself: exactly right {100 * full:.1f}%, up to main stress {100 * reduced:.1f}%, phonemes {100 * phones:.1f}%")
    write_rules(rules, stress)


if __name__ == "__main__":
    main()
