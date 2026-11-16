"""Prosody transplant experiment (development tooling).

Forces a real speaker's phone durations and/or pitch contour onto the synthesizer and
scores the result, to show how much of the gap to natural speech is timing / intonation
and how much is the voice itself. Uses the CMU Arctic phone labels.

  python tools/transplant.py            # prints a table: rules vs +durations vs +pitch vs both
"""
from __future__ import annotations

import os
import subprocess
import sys
import urllib.request
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402

LAB = E.REF / "lab"
MODES = os.environ.get("TP_MODES", "rules,dur,f0,both").split(",")
OUT = E.CACHE / "synth"


def labels(spk: str, idx: int) -> list[tuple[str, float, float]]:
    """(PHONE, start s, end s) for each phone of a reference recording, pauses dropped."""
    LAB.mkdir(parents=True, exist_ok=True)
    path = LAB / f"{spk}_a{idx:04d}.lab"
    if not path.exists():
        urllib.request.urlretrieve(f"{E.ARCTIC.format(spk=spk)}/lab/arctic_a{idx:04d}.lab", path)
    out, prev, started = [], 0.0, False
    for line in path.read_text().splitlines():
        if line.strip() == "#":
            started = True
            continue
        if not started or not line.strip():
            continue
        end, _, name = line.split()[:3]
        end = float(end)
        if name not in ("pau", "ssil", "h#"):
            out.append((name.upper(), prev, end))
        prev = end
    return out


def say(args: list[str], env_extra: dict | None = None) -> str:
    env = dict(os.environ)
    env.update(env_extra or {})
    res = subprocess.run([str(E.SAY), *args, "--dict", str(E.DICT)], check=True, capture_output=True, text=True, encoding="utf-8", env=env)
    return res.stdout


def synth_timing(voice: str, text: str, env: dict | None = None) -> tuple[list[tuple[int, str, float, float]], int]:
    tokens, frames = [], 0
    for line in say(["--voice", voice, "--timing", "--phones", text], env).splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[0].isdigit():
            tokens.append((int(parts[0]), parts[1], int(parts[2]) / 1000, int(parts[3]) / 1000))
        elif len(parts) == 2 and parts[0] == "frames":
            frames = int(parts[1])
    return tokens, frames


def match(nat: list, syn: list) -> list[tuple[int, int]]:
    """Pairs (natural index, synth index) of phones that correspond."""
    canon = lambda p: {"AX": "AH", "AO": "AA"}.get(p, p)  # noqa: E731
    pairs = E.align([canon(p[0]) for p in nat], [canon(t[1]) for t in syn])
    out, i, j = [], 0, 0
    for r, h in pairs:
        if r is not None and h is not None:
            out.append((i, j))
        i += r is not None
        j += h is not None
    return out


def natural_f0(spk: str, idx: int, gender: str):
    x, sr = E.load(E.REF / f"{spk}_a{idx:04d}.wav")
    f0, voiced = E.f0_track(E.resample(x, sr, 16000), 16000, gender)
    t = np.arange(len(f0)) * 0.005 + 0.02
    return t[voiced], f0[voiced]


def main() -> None:
    texts = E.arctic()
    work = E.CACHE / "transplant"
    work.mkdir(parents=True, exist_ok=True)
    rows = {}
    for gender, spk in E.SPEAKERS.items():
        base_pitch = 110.0 if gender == "male" else 186.0
        for mode in ("rules", "dur", "f0", "both"):
            (OUT / f"tp_{mode}").mkdir(parents=True, exist_ok=True)
        for i, text in enumerate(texts, start=1):
            nat = labels(spk, i)
            syn, _ = synth_timing(gender, text)
            pairs = match(nat, syn)
            dur_file = work / "dur.txt"
            # TP_SCALE=same keeps the synthesizer's overall speaking rate and transplants only the rhythm
            scale = 1.0
            if os.environ.get("TP_SCALE") == "same":
                scale = sum(syn[j][3] - syn[j][2] for _, j in pairs) / sum(nat[k][2] - nat[k][1] for k, _ in pairs)
            # TP_WHICH=vowels|cons limits the transplant to one class; TP_CLAMP=lo,hi limits how far
            # a duration may move from the rule-based value (as a ratio).
            which = os.environ.get("TP_WHICH", "all")
            lo, hi = (float(v) for v in os.environ.get("TP_CLAMP", "0,100").split(","))
            lines = []
            for k, j in pairs:
                is_vowel = syn[j][1] in E.VOWELS or syn[j][1] in ("AO", "AX")
                if (which == "vowels" and not is_vowel) or (which == "cons" and is_vowel):
                    continue
                rule = (syn[j][3] - syn[j][2]) * 1000
                want = (nat[k][2] - nat[k][1]) * 1000 * scale
                lines.append(f"{syn[j][0]} {min(max(want, rule * lo), rule * hi):.1f}")
            dur_file.write_text(chr(10).join(lines) + chr(10))
            tv, fv = natural_f0(spk, i, gender)
            median = float(np.median(fv))

            def f0_file(timing, n_frames, name):
                vals = np.zeros(n_frames)
                by_token = {t[0]: t for t in timing}
                for k, j in pairs:
                    tok = by_token.get(syn[j][0])
                    if tok is None:
                        continue
                    _, _, s0, s1 = tok
                    n0, n1 = nat[k][1], nat[k][2]
                    for fr in range(int(round(s0 / 0.005)), min(n_frames, int(round(s1 / 0.005)))):
                        u = (fr * 0.005 - s0) / max(s1 - s0, 1e-6)
                        vals[fr] = base_pitch * float(np.interp(n0 + u * (n1 - n0), tv, fv)) / median
                # hold the contour through pauses and unmatched phones
                idx_known = np.nonzero(vals)[0]
                if len(idx_known):
                    vals = np.interp(np.arange(n_frames), idx_known, vals[idx_known])
                path = work / name
                path.write_text("".join(f"{v:.2f}\n" for v in vals))
                return path

            f0_rules = f0_file(syn, synth_timing(gender, text)[1], "f0_rules_timing.txt")
            syn_d, frames_d = synth_timing(gender, text, {"VS_DUR_FILE": str(dur_file)})
            f0_both = f0_file(syn_d, frames_d, "f0_nat_timing.txt")
            envs = {
                "rules": {},
                "dur": {"VS_DUR_FILE": str(dur_file)},
                "f0": {"VS_F0_FILE": str(f0_rules)},
                "both": {"VS_DUR_FILE": str(dur_file), "VS_F0_FILE": str(f0_both)},
            }
            for mode, env in envs.items():
                if mode not in MODES:
                    continue
                say(["--voice", gender, text, str(OUT / f"tp_{mode}" / f"{gender}_a{i:04d}.wav")], env)
        arps = E.run_batch([(gender, "-", t) for t in texts])
        for mode in MODES:
            ms = [E.measure(OUT / f"tp_{mode}" / f"{gender}_a{i:04d}.wav", t, a, gender, None, True) for i, (t, a) in enumerate(zip(texts, arps), start=1)]
            rows[(gender, mode)] = E.summarize(ms)
    E.QUICK = True
    print("voice   prosody source            utmos   per   f0 sd(st)")
    names = {"rules": "synthesizer's own rules", "dur": "+ human phone durations", "f0": "+ human pitch contour", "both": "+ human durations and pitch"}
    for (gender, mode), sm in rows.items():
        print(f"{gender:7s} {names[mode]:28s} {sm['utmos']:.2f}  {sm['per']:.3f}  {sm['f0_sd_st']:.2f}")


if __name__ == "__main__":
    E.QUICK = True
    main()
