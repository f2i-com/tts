"""How close does a voice come to a reference voice, on the same sentences?

    python tools/closeness.py --ref receptionist --gender female "defaults||" "brighter||--closure 1"
    python tools/closeness.py --ref man --gender male --wavs "other=some/dir/{key}.wav"

A variant is "label|VS_TUNE overrides|options for the say example"; --wavs takes recordings made
by anything else (label=path with {key} in it). For each voice:

  spectrum   mel-cepstral distance to the reference after aligning the two in time, in dB
             (the level and the timing do not count; lower = closer), over the whole band
             to 11 kHz and over the part below 5 kHz alone
  speaker    similarity of speaker-verification embeddings (1 = the same speaker)
  natural    predicted naturalness (UTMOS, 1-5)
  misheard   sounds a phoneme recognizer hears differently from what was meant
  pitch      median in Hz and movement in semitones (standard deviation)
  rate       length of the speech relative to the reference's

The reference voices are recordings in tools/.cache/refvoice/ (see ref_voice.py).
"""
from __future__ import annotations

import argparse
import os
import subprocess
from pathlib import Path

import numpy as np

import eval as E
import ref_voice as R

SR = 24000
HOP = 240  # 10 ms
N_FFT = 1024
N_MEL = 40
N_CEP = 24


def _mel_filters() -> np.ndarray:
    def mel(f):
        return 2595.0 * np.log10(1.0 + f / 700.0)

    edges = 700.0 * (10 ** (np.linspace(mel(80.0), mel(11000.0), N_MEL + 2) / 2595.0) - 1.0)
    freqs = np.fft.rfftfreq(N_FFT, 1.0 / SR)
    fb = np.zeros((N_MEL, len(freqs)))
    for i in range(N_MEL):
        lo, mid, hi = edges[i], edges[i + 1], edges[i + 2]
        fb[i] = np.clip(np.minimum((freqs - lo) / (mid - lo), (hi - freqs) / (hi - mid)), 0.0, None)
    return fb


_FB = _mel_filters()
N_LOW = int(np.sum(_FB.argmax(axis=1) * SR / N_FFT < 5000.0))  # the mel bands centred below 5 kHz


def _dct(bands: int) -> np.ndarray:
    return np.array([[np.cos(np.pi * k * (2 * n + 1) / (2 * bands)) for n in range(bands)] for k in range(1, N_CEP + 1)]) * np.sqrt(2.0 / bands)


_DCT, _DCT_LOW = _dct(N_MEL), _dct(N_LOW)


def cepstra(x: np.ndarray, sr: int, low: bool = False) -> np.ndarray:
    """Mel cepstra (without the level term) of the frames that hold speech; `low`: of the
    part of the spectrum below 5 kHz only."""
    if sr != SR:
        x = E.resample(x, sr, SR)
    x = np.asarray(x, dtype=np.float64)
    n = 1 + max(0, (len(x) - N_FFT) // HOP)
    idx = np.arange(N_FFT)[None, :] + HOP * np.arange(n)[:, None]
    power = np.abs(np.fft.rfft(x[idx] * np.hanning(N_FFT), axis=1)) ** 2
    level = 10 * np.log10(power.sum(axis=1) + 1e-12)
    keep = level > level.max() - 40.0
    logmel = 0.5 * np.log(power[keep] @ _FB.T + 1e-12)
    return logmel[:, :N_LOW] @ _DCT_LOW.T if low else logmel @ _DCT.T


def distance(a: np.ndarray, b: np.ndarray) -> float:
    """Mean mel-cepstral distance in dB along the best time alignment of two cepstrum tracks."""
    cost = np.sqrt(np.maximum((a * a).sum(1)[:, None] + (b * b).sum(1)[None, :] - 2.0 * a @ b.T, 0.0))
    n, m = cost.shape
    inf = float("inf")
    prev_d, prev_n = [inf] * (m + 1), [0] * (m + 1)
    prev_d[0] = 0.0
    for i in range(n):
        row = cost[i].tolist()
        cur_d, cur_n = [inf] * (m + 1), [0] * (m + 1)
        for j in range(m):
            best, steps = prev_d[j], prev_n[j]  # diagonal
            if prev_d[j + 1] < best:
                best, steps = prev_d[j + 1], prev_n[j + 1]
            if cur_d[j] < best:
                best, steps = cur_d[j], cur_n[j]
            cur_d[j + 1] = best + row[j]
            cur_n[j + 1] = steps + 1
        prev_d, prev_n = cur_d, cur_n
    return (10.0 / np.log(10.0)) * np.sqrt(2.0) * prev_d[m] / max(prev_n[m], 1)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("variants", nargs="*", help='"label|VS_TUNE|say options"')
    ap.add_argument("--ref", default="receptionist")
    ap.add_argument("--gender", default="female")
    ap.add_argument("--wavs", action="append", default=[], help="label=path/with/{key}.wav")
    ap.add_argument("--keys", default="17-40", help="which of the reference's a-sentences (first-last)")
    ap.add_argument("--fast", action="store_true", help="skip the neural models")
    ap.add_argument("--quick", action="store_true", help="skip the naturalness predictor and the recognizer")
    args = ap.parse_args()

    first, last = (int(v) for v in args.keys.split("-"))
    keys = [f"a{i:04d}" for i in range(first, last + 1)]
    texts = R.texts(args.ref)
    ref_wavs = [R.ROOT / args.ref / f"{k}.wav" for k in keys]
    ref_cep = [cepstra(*E.load(w)) for w in ref_wavs]
    ref_low = [cepstra(*E.load(w), low=True) for w in ref_wavs]
    ref_len = sum(len(c) for c in ref_cep)
    targets = [E.target_phones(a) for a in E.run_batch([(args.gender, "-", texts[k]) for k in keys])]
    out_dir = E.CACHE / "closeness"
    out_dir.mkdir(exist_ok=True)

    voices: list[tuple[str, list[Path]]] = [(f"reference ({args.ref})", ref_wavs)]
    for spec in args.wavs:
        label, pattern = spec.split("=", 1)
        voices.append((label, [Path(pattern.format(key=k)) for k in keys]))
    for n, spec in enumerate(args.variants or ["defaults||"]):
        label, tune, extra = (spec.split("|") + ["", ""])[:3]
        env = dict(os.environ)
        if tune:
            env["VS_TUNE"] = tune
        wavs = []
        for k in keys:
            w = out_dir / f"{args.gender}_{n}_{k}.wav"
            subprocess.run([str(E.SAY), "--voice", args.gender, *extra.split(), texts[k], str(w), "--dict", str(E.DICT)], capture_output=True, env=env, check=True)
            wavs.append(w)
        voices.append((label, wavs))

    if not args.fast:
        import fit_voice as F

        ref_embedding = F.embed(ref_wavs)
    print(f"{len(keys)} sentences{'':24s} spectrum  (<5 kHz)   speaker   natural   misheard   pitch Hz / movement st   rate")
    for label, wavs in voices:
        missing = [w for w in wavs if not w.exists()]
        if missing:
            print(f"{label:36s} missing {len(missing)} recordings, e.g. {missing[0]}")
            continue
        loaded = [E.load(w) for w in wavs]
        ceps = [cepstra(x, sr) for x, sr in loaded]
        same = wavs is ref_wavs
        mcd = 0.0 if same else float(np.mean([distance(c, r) for c, r in zip(ceps, ref_cep)]))
        low = 0.0 if same else float(np.mean([distance(cepstra(x, sr, low=True), r) for (x, sr), r in zip(loaded, ref_low)]))
        rate = sum(len(c) for c in ceps) / ref_len
        pitch = [E.pitch_stats(x, sr, args.gender) for x, sr in loaded]
        f0 = float(np.median([p["f0_med"] for p in pitch]))
        move = float(np.mean([p["f0_sd_st"] for p in pitch]))
        if args.fast:
            sim = nat = per = float("nan")
        else:
            sim = F.cos(F.embed(wavs), ref_embedding)
            nat = per = float("nan")
        if not args.fast and not args.quick:
            x16 = [E.resample(x, sr, 16000) for x, sr in loaded]
            x16 = [(x / (np.abs(x).max() + 1e-9) * 0.8).astype(np.float32) for x in x16]
            nat = float(np.mean([E.utmos(x) for x in x16]))
            per = float(np.mean([E.phone_errors(t, E.recognize_phones(x))["per"] for t, x in zip(targets, x16)]))
        print(f"{label:36s} {mcd:7.2f}   {low:7.2f}   {sim:7.3f}   {nat:7.2f}   {100 * per:7.1f}%   {f0:9.0f} / {move:4.2f}   {rate:10.2f}", flush=True)


if __name__ == "__main__":
    main()
