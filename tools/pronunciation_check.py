"""How often does the built-in pronunciation differ from the CMU dictionary? (development tooling)

The naturalness measurements load the full dictionary, but the browser build has only the
embedded lexicon and the letter-to-sound rules. This counts, over the 1,131 sentences of the
CMU Arctic prompts (running literary English), the words the synthesizer pronounces
differently without the dictionary than with it.

  python tools/pronunciation_check.py [path to another say binary to compare]
"""
from __future__ import annotations

import collections
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / "tools" / ".cache"
SAY = ROOT / "target" / "release" / "examples" / ("say.exe" if sys.platform == "win32" else "say")
DICT = CACHE / "cmudict.dict"


def run(binary: Path, words: list[str], extra: list[str]) -> list[str]:
    tsv = CACHE / f"jobs_pron_{binary.stem}.tsv"
    tsv.write_text("".join(f"male\t-\t{w}\n" for w in words), encoding="utf-8")
    res = subprocess.run([str(binary), "--batch", str(tsv), *extra], capture_output=True, text=True, encoding="utf-8", check=True)
    return res.stdout.splitlines()


def main() -> None:
    counts: collections.Counter = collections.Counter()
    for line in (CACHE / "ref" / "bdl.txt").read_text(encoding="utf-8").splitlines():
        m = re.search(r'"(.*)"', line)
        if m:
            for w in re.findall(r"[A-Za-z']+", m.group(1)):
                counts[w.lower().strip("'")] += 1
    counts.pop("", None)
    known = {line.split(" ", 1)[0].split("(")[0] for line in DICT.read_text(encoding="utf-8").splitlines()}
    words = sorted(w for w in counts if w in known)
    total = sum(counts[w] for w in words)
    strip = lambda s: re.sub(r"\d", "", s).replace("AX", "AH")  # noqa: E731
    for binary in [SAY] + [Path(a) for a in sys.argv[1:]]:
        own = run(binary, words, [])
        full = run(binary, words, ["--dict", str(DICT)])
        diff = [w for w, a, b in zip(words, own, full) if strip(a) != strip(b)]
        stress = [w for w, a, b in zip(words, own, full) if strip(a) == strip(b) and a != b]
        print(f"{binary.name}: {len(words)} distinct words, {total} in running text")
        print(f"  different sounds: {len(diff)} words ({100 * len(diff) / len(words):.1f}%), {sum(counts[w] for w in diff)} in running text ({100 * sum(counts[w] for w in diff) / total:.2f}%, one in {total // max(1, sum(counts[w] for w in diff))})")
        print(f"  same sounds, different stress: {len(stress)} words ({100 * sum(counts[w] for w in stress) / total:.2f}% of running text)")


if __name__ == "__main__":
    main()
