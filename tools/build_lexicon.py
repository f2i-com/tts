"""Rebuild the embedded lexicon `src/lexicon_common.txt` (development tooling).

The synthesizer must pronounce every word in `tools/lexicon_words.txt` exactly as the CMU
Pronouncing Dictionary does. Most of them it gets right by rule (`src/g2p_rules.txt`, see
`tools/train_g2p.py`), so the embedded lexicon only lists the rest. Run after retraining the
rules:

  python tools/train_g2p.py
  python tools/build_lexicon.py                 # rewrites src/lexicon_common.txt
  python tools/build_lexicon.py --extend 50000  # first add the 50,000 most frequent words of a
                                                # frequency list (tools/.cache/freq/count_1w.txt)
"""
from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / "tools" / ".cache"
WORDS = ROOT / "tools" / "lexicon_words.txt"
OUT = ROOT / "src" / "lexicon_common.txt"


def cmudict() -> dict[str, list[str]]:
    out = {}
    for line in (CACHE / "cmudict.dict").read_text(encoding="utf-8").splitlines():
        line = line.split(" #")[0].strip()
        if not line:
            continue
        word, *phones = line.split()
        if "(" not in word:
            out[word] = phones
    return out


def internal(phones: list[str]) -> str:
    """CMUdict notation -> the lexicon's: the reduced vowel AH0 is AX, a 0 is not written."""
    return " ".join("AX" if p == "AH0" else p[:-1] if p.endswith("0") else p for p in phones)


def main() -> None:
    cmu = cmudict()
    words = WORDS.read_text(encoding="utf-8").split()
    if "--extend" in sys.argv:
        n = int(sys.argv[sys.argv.index("--extend") + 1])
        have = set(words)
        freq = [line.split("\t")[0] for line in (CACHE / "freq" / "count_1w.txt").read_text().splitlines()]
        top = [w for w in freq if w in cmu and re.fullmatch(r"[a-z]+", w)][:n]
        new = [w for w in top if w not in have]
        words += new
        WORDS.write_text("\n".join(words) + "\n", encoding="utf-8", newline="\n")
        print(f"added {len(new)} words; the list now has {len(words)}")
    # the first 20,000 words of the list are the original spoken-English selection: of those,
    # short words stay listed, because an all-caps token is read as a word only if it is one
    lines = []
    for k, w in enumerate(words):
        if w not in cmu:
            continue
        keep = k < 20000 and len(w) <= 4 or "'" in w
        lines.append(("!" if keep else "") + f"{w} {internal(cmu[w])}")
    truth = CACHE / "lexicon_truth.txt"
    truth.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    subprocess.run(["cargo", "run", "--release", "--quiet", "--example", "compact_lexicon", "--", str(truth), str(OUT)], check=True, cwd=ROOT)


if __name__ == "__main__":
    main()
