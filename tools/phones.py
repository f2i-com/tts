"""How does each sound compare with the way two real readers make it? (development tooling)

The naturalness predictor of eval.py judges a whole sentence and cannot say where the fault
lies. This script looks inside the sounds instead, on the CMU Arctic sentences that come with
phone labels (`bdl`, a man, and `slt`, a woman), next to the synthesizer's rendering of the
same sentences with its own timing:

  levels    the level through each phone - in three bands (80-500, 500-4000, 4000-8000 Hz),
            in dB against the average vowel frame of the utterance, at ten points from the
            start of the phone to its end; optionally split by what stands before and after.
            This is what showed that the "th" of "the" after a pause or a consonant is a silent
            closure, that /z/ loses its voice before a consonant, that /v/ hardly hisses.
  formants  F1 F2 F3 at 25, 50 and 75 % of each phone: the readers' (Praat's tracker; needs
            `praat-parselmouth`) against the values the synthesizer is told to make (its own
            control frames, no tracker on that side). This is what showed the dark /l/ before
            vowels and the /u/ that starts far forward after t d n s.

Usage (from the crate root, after `cargo build --release --example say`):

  python tools/phones.py levels --phones DH,Z,V,T,D --context
  python tools/phones.py formants --phones L,UW,OW --n 300
  python tools/phones.py levels --tune "dh_late=0"        # with development hooks (--features dev-hooks)

Recordings and labels beyond those eval.py has fetched are downloaded on first use (about
100 KB per sentence, into tools/.cache/ref). The labels were aligned by machine: single
boundaries can be 20 ms off, which is why everything here is a median over many instances.
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
import urllib.request
from collections import defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402

VOWELS = {"IY", "IH", "EH", "AE", "AA", "AO", "UH", "UW", "AH", "AX", "ER", "EY", "AY", "OY", "AW", "OW"}
BANDS = [("low", 80, 500), ("mid", 500, 4000), ("high", 4000, 8000)]
WIN, HOP, POINTS = 160, 40, 10  # 10 ms window, 2.5 ms hop at 16 kHz
CONTEXTS = ("_V", "V_V", "V_", "other")


def prompts(n: int) -> list[str]:
    lines = (E.REF / "bdl.txt").read_text().splitlines()[:n]
    return [re.match(r'\( *\S+ +"(.*)" *\)', line).group(1) for line in lines]


def recording(spk: str, idx: int) -> Path:
    """The recording of Arctic sentence `idx`, fetched if it is not here yet (with its labels)."""
    wav, lab = E.REF / f"{spk}_a{idx:04d}.wav", E.REF / "lab" / f"{spk}_a{idx:04d}.lab"
    lab.parent.mkdir(parents=True, exist_ok=True)
    for kind, dest in (("wav", wav), ("lab", lab)):
        if not dest.exists():
            urllib.request.urlretrieve(f"{E.ARCTIC.format(spk=spk)}/{kind}/arctic_a{idx:04d}.{kind}", dest)
    return wav


def human_phones(spk: str, idx: int) -> list[tuple[str, float, float]]:
    """(PHONE, start s, end s) of a recording; pauses are PAU, the silence at either end SIL."""
    out, prev, started = [], 0.0, False
    for line in (E.REF / "lab" / f"{spk}_a{idx:04d}.lab").read_text().splitlines():
        if line.strip() == "#":
            started = True
            continue
        if not started or not line.strip():
            continue
        end, _, name = line.split()[:3]
        name = "PAU" if name in ("pau", "ssil", "h#") else name.upper()
        out.append((name, prev, float(end)))
        prev = float(end)
    for k in (0, -1):
        if out and out[k][0] == "PAU":
            out[k] = ("SIL", out[k][1], out[k][2])
    return out


def synth(say: str, voice: str, text: str, wav: Path, tune: str, frames: bool = False):
    """Speaks `text`; returns the phones with their times and, with `frames`, the control formants."""
    env = dict(os.environ)
    if tune:
        env["VS_TUNE"] = tune
    args = [say, "--voice", voice, "--variation", "0", "--timing", "--dict", str(E.DICT)] + (["--frames"] if frames else [])
    res = subprocess.run([*args, text, str(wav)], check=True, capture_output=True, text=True, encoding="utf-8", env=env)
    phones, ctl = [], []
    for line in res.stdout.splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[0].isdigit():
            phones.append((re.sub(r"\d", "", parts[1]), int(parts[2]) / 1000, int(parts[3]) / 1000))
        elif frames and len(parts) > 12 and parts[0].isdigit() and parts[1].isdigit():
            ctl.append((int(parts[1]) / 1000, float(parts[4]), float(parts[5]), float(parts[6])))
    return phones, np.array(ctl)


def context(seq, k: int) -> str:
    prev = seq[k - 1][0] if k > 0 else "SIL"
    nxt = seq[k + 1][0] if k + 1 < len(seq) else "SIL"
    return "V_V" if prev in VOWELS and nxt in VOWELS else "_V" if nxt in VOWELS else "V_" if prev in VOWELS else "other"


# ---------------------------------------------------------------------------- levels


def band_power(x16: np.ndarray) -> np.ndarray:
    n = 1 + max(0, (len(x16) - WIN) // HOP)
    idx = np.arange(WIN)[None, :] + HOP * np.arange(n)[:, None]
    p = np.abs(np.fft.rfft(x16[idx] * np.hanning(WIN)[None, :], 256, axis=1)) ** 2
    f = np.fft.rfftfreq(256, 1 / 16000)
    return np.stack([p[:, (f >= lo) & (f < hi)].sum(axis=1) for _, lo, hi in BANDS], axis=1)


def add_levels(store, lengths, x16, seq, wanted) -> None:
    bp = band_power(x16)
    t = (np.arange(len(bp)) * HOP + WIN / 2) / 16000
    vowel = np.zeros(len(bp), bool)
    for name, a, b in seq:
        if name in VOWELS:
            vowel |= (t >= a) & (t < b)
    if vowel.sum() < 5:
        return
    ref = bp[vowel].sum(axis=1).mean()
    for k, (name, a, b) in enumerate(seq):
        if name not in wanted or b - a < 0.03:
            continue
        row = np.zeros((POINTS, len(BANDS)))
        for q in range(POINTS):
            sel = (t >= a + (b - a) * q / POINTS) & (t < a + (b - a) * (q + 1) / POINTS)
            if not sel.any():
                sel[int(np.argmin(np.abs(t - (a + (b - a) * (q + 0.5) / POINTS))))] = True
            row[q] = 10 * np.log10(bp[sel].mean(axis=0) / ref + 1e-9)
        for key in (name, f"{name} {context(seq, k)}"):
            store[key].append(row)
            lengths[key].append(b - a)


def cmd_levels(args) -> None:
    wanted = set(args.phones.split(","))
    for voice in args.voices.split(","):
        spk = E.SPEAKERS[voice]
        hs, ss, hl, sl = (defaultdict(list) for _ in range(4))
        for i, text in enumerate(prompts(args.n), start=1):
            h, sr = E.load(recording(spk, i))
            add_levels(hs, hl, E.resample(h, sr, 16000), human_phones(spk, i), wanted)
            wav = E.CACHE / "synth" / f"phones_{voice}.wav"
            wav.parent.mkdir(parents=True, exist_ok=True)
            phones, _ = synth(args.say, voice, text, wav, args.tune)
            x, sr = E.load(wav)
            add_levels(ss, sl, E.resample(x, sr, 16000), phones, wanted)
        print(f"===== {voice}: level through the phone (dB against the utterance's vowels) at {POINTS} points from its start to its end; medians")
        keys = [f"{p} {c}" for p in args.phones.split(",") for c in CONTEXTS] if args.context else args.phones.split(",")
        for key in keys:
            if len(hs[key]) < args.min or len(ss[key]) < args.min:
                continue
            H, S = np.median(np.array(hs[key]), axis=0), np.median(np.array(ss[key]), axis=0)
            print(f"  {key:9s} readers n={len(hs[key])}, {np.median(hl[key]) * 1000:.0f} ms; synth n={len(ss[key])}, {np.median(sl[key]) * 1000:.0f} ms")
            for b, (band, _, _) in enumerate(BANDS):
                row = lambda v: " ".join(f"{x:4.0f}" for x in v)  # noqa: E731
                print(f"      {band:4s} reader {row(H[:, b])}   synth {row(S[:, b])}   synth-reader " + " ".join(f"{x:+4.0f}" for x in (S - H)[:, b]))


# ---------------------------------------------------------------------------- formants


def cmd_formants(args) -> None:
    import parselmouth

    wanted = args.phones.split(",")
    places = (0.25, 0.5, 0.75)
    for voice in args.voices.split(","):
        spk = E.SPEAKERS[voice]
        hs, ss = defaultdict(list), defaultdict(list)
        for i, text in enumerate(prompts(args.n), start=1):
            wav = E.CACHE / "synth" / f"phones_{voice}.wav"
            wav.parent.mkdir(parents=True, exist_ok=True)
            phones, ctl = synth(args.say, voice, text, wav, args.tune, frames=True)
            for name, a, b in phones:
                if name in wanted and (b - a) * 1000 >= args.min_ms:
                    ss[name].append([ctl[int(np.argmin(np.abs(ctl[:, 0] - (a + (b - a) * q)))), 1:] for q in places])
            snd = parselmouth.Sound(str(recording(spk, i)))
            track = snd.to_formant_burg(time_step=0.005, max_number_of_formants=5, maximum_formant=5000 if voice == "male" else 5500, window_length=0.025)
            for name, a, b in human_phones(spk, i):
                if name in wanted and (b - a) * 1000 >= args.min_ms:
                    row = np.array([[track.get_value_at_time(k, a + (b - a) * q) for k in (1, 2, 3)] for q in places], float)
                    if np.isfinite(row).all():
                        hs[name].append(row)
        print(f"===== {voice}: F1 F2 F3 (Hz) at 25 / 50 / 75 % of the phone, medians: readers | what the synthesizer is told to make | difference in %")
        for name in wanted:
            if len(hs[name]) < args.min or len(ss[name]) < args.min:
                continue
            H, S = np.median(np.array(hs[name]), axis=0), np.median(np.array(ss[name]), axis=0)
            cells = [" ".join(f"{v:4.0f}" for v in H[k]) + " | " + " ".join(f"{v:4.0f}" for v in S[k]) + " | " + " ".join(f"{(s / h - 1) * 100:+3.0f}" for s, h in zip(S[k], H[k])) for k in range(3)]
            print(f"  {name:3s} n={len(hs[name])}/{len(ss[name])}   " + "    ".join(cells))


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    for name in ("levels", "formants"):
        s = sub.add_parser(name)
        s.add_argument("--n", type=int, default=76, help="how many Arctic sentences (up to 593)")
        s.add_argument("--voices", default="male,female")
        s.add_argument("--say", default=str(E.SAY))
        s.add_argument("--tune", default="", help="VS_TUNE overrides for the synthesizer")
        s.add_argument("--min", type=int, default=8, help="instances a phone needs to be shown")
    sub.choices["levels"].add_argument("--phones", default="P,T,K,B,D,G,DH,TH,F,V,S,Z,SH,CH,JH,HH")
    sub.choices["levels"].add_argument("--context", action="store_true", help="split by neighbours: _V before a vowel, V_V between vowels, V_ after one")
    sub.choices["formants"].add_argument("--phones", default="IY,IH,EH,AE,AA,AO,AH,AX,UH,UW,ER,L,R,W,Y,M,N")
    sub.choices["formants"].add_argument("--min-ms", type=float, default=50.0)
    args = ap.parse_args()
    {"levels": cmd_levels, "formants": cmd_formants}[args.cmd](args)


if __name__ == "__main__":
    main()
