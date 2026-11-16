"""Line up the synthesizer's rendering of a sentence with a recording of the same words.

Dynamic time warping on cepstral features: for every moment of the synthetic sentence it
gives the matching moment of the recording, so the synthesizer's own knowledge of where each
sound starts and ends can be carried over to a recording that has no labels at all.

  python tools/align.py        # accuracy check against hand-corrected labels (CMU Arctic)
"""
from __future__ import annotations

import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))

HOP = 0.010


def features(x: np.ndarray, sr: int) -> np.ndarray:
    """13 mel cepstra + deltas per 10 ms, mean/variance normalised over the utterance."""
    from scipy.fft import dct

    x = np.append(x[0], x[1:] - 0.97 * x[:-1]).astype(np.float64)
    n, h, nfft = int(sr * 0.025), int(sr * HOP), 512
    count = max(1, 1 + (len(x) - n) // h)
    idx = np.arange(n)[None, :] + h * np.arange(count)[:, None]
    frames = np.pad(x, (0, n))[idx] * np.hamming(n)
    power = np.abs(np.fft.rfft(frames, nfft)) ** 2
    mel = lambda f: 2595 * np.log10(1 + f / 700)  # noqa: E731
    edges = 700 * (10 ** (np.linspace(mel(60), mel(min(7600, sr / 2)), 42) / 2595) - 1)
    bins = np.fft.rfftfreq(nfft, 1 / sr)
    bank = np.maximum(0, np.minimum((bins[None, :] - edges[:-2, None]) / (edges[1:-1, None] - edges[:-2, None]), (edges[2:, None] - bins[None, :]) / (edges[2:, None] - edges[1:-1, None])))
    ceps = dct(np.log(power @ bank.T + 1e-10), type=2, norm="ortho", axis=1)[:, 1:14]
    delta = np.gradient(ceps, axis=0)
    feats = np.hstack([ceps, delta])
    return (feats - feats.mean(axis=0)) / (feats.std(axis=0) + 1e-9)


def speech_span(x: np.ndarray, sr: int) -> tuple[float, float]:
    """Start and end (s) of the spoken part: where the level is within 40 dB of the peak."""
    h = int(sr * HOP)
    level = np.array([np.sqrt(np.mean(x[i : i + h] ** 2) + 1e-12) for i in range(0, max(1, len(x) - h), h)])
    loud = np.nonzero(level > level.max() * 0.01)[0]
    if len(loud) == 0:
        return 0.0, len(x) / sr
    return max(0.0, loud[0] * HOP - 0.02), min(len(x) / sr, (loud[-1] + 1) * HOP + 0.02)


def warp(synth: np.ndarray, sr_s: int, rec: np.ndarray, sr_r: int):
    """Returns a function mapping a time in the synthetic sentence to the matching time in
    the recording."""
    s0, s1 = speech_span(synth, sr_s)
    r0, r1 = speech_span(rec, sr_r)
    a = features(synth[int(s0 * sr_s) : int(s1 * sr_s)], sr_s)
    b = features(rec[int(r0 * sr_r) : int(r1 * sr_r)], sr_r)
    na, nb = len(a), len(b)
    cost = np.sqrt(((a[:, None, :] - b[None, :, :]) ** 2).sum(axis=2))
    # keep the path within a band around the diagonal
    band = max(30, int(0.25 * max(na, nb)))
    ii, jj = np.arange(na)[:, None], np.arange(nb)[None, :]
    cost[np.abs(ii * nb / na - jj) > band] = 1e6
    acc = np.full((na + 1, nb + 1), np.inf)
    acc[0, 0] = 0
    for i in range(1, na + 1):
        row, prev = acc[i], acc[i - 1]
        c = cost[i - 1]
        for j in range(1, nb + 1):
            best = prev[j - 1]
            if prev[j] < best:
                best = prev[j]
            if row[j - 1] < best:
                best = row[j - 1]
            row[j] = c[j - 1] + best
    i, j, path = na, nb, []
    while i > 0 and j > 0:
        path.append((i - 1, j - 1))
        step = np.argmin((acc[i - 1, j - 1], acc[i - 1, j], acc[i, j - 1]))
        if step == 0:
            i, j = i - 1, j - 1
        elif step == 1:
            i -= 1
        else:
            j -= 1
    path = np.array(path[::-1])
    # one recording time per synthetic frame: the middle of the frames it was matched with
    ts = np.arange(na) * HOP + s0
    tr = np.array([path[path[:, 0] == k, 1].mean() for k in range(na)]) * HOP + r0
    return lambda t: np.interp(t, ts, tr)


def locate(x16: np.ndarray, phones: list[str], heard: list | None = None) -> dict[int, float]:
    """Where in a recording each of the expected sounds was heard: {index in `phones`: time}.
    A phoneme recognizer listens to the recording; the sounds it reports are matched with the
    expected ones, and only exact matches are kept. No labels and no synthetic audio needed."""
    import eval as E

    canon = lambda p: {"AO": "AA", "AX": "AH"}.get(p, p)  # noqa: E731
    if heard is None:
        heard = E.recognize_phones_timed(x16)
    out, i, j = {}, 0, 0
    for want, got in E.align([canon(p) for p in phones], [h[0] for h in heard]):
        if want is not None and got is not None and want == got:
            out[i] = heard[j][1]
        i += want is not None
        j += got is not None
    return out


def main() -> None:
    import eval as E
    from fit_durations import prompts
    from transplant import labels, match, say, synth_timing

    texts = prompts()
    work = E.CACHE / "copy"
    work.mkdir(parents=True, exist_ok=True)
    print("speaker   method       sounds   median error   bias    within 20 ms   within 40 ms   (centre of each sound, vs. the labels)")
    for gender, spk in (("male", "bdl"), ("female", "slt"), ("male", "rms"), ("female", "clb")):
        errs, heard_errs, heard_vowel = [], [], []
        for i in range(1, 21):
            ref = E.REF / f"{spk}_a{i:04d}.wav"
            if not ref.exists():
                continue
            nat = labels(spk, i)
            syn, _ = synth_timing(gender, texts[i])
            wav = work / "align.wav"
            say(["--voice", gender, "--variation", "0", texts[i], str(wav)])
            xs, srs = E.load(wav)
            xr, srr = E.load(ref)
            to_rec = warp(xs, srs, xr, srr)
            where = locate(E.resample(xr, srr, 16000), [t[1] for t in syn])
            for k, j in match(nat, syn):
                want = (nat[k][1] + nat[k][2]) / 2
                got = float(to_rec((syn[j][2] + syn[j][3]) / 2))
                errs.append(got - want)
                if j in where:
                    # relative position inside the labelled sound: 0 = start, 1 = end
                    heard_errs.append(where[j] - want)
                    if syn[j][1] in E.VOWELS or syn[j][1] in ("AO", "AX"):
                        heard_vowel.append((where[j] - nat[k][1]) / max(nat[k][2] - nat[k][1], 1e-3))
        for name, e in (("warping", np.array(errs)), ("recognizer", np.array(heard_errs))):
            if len(e):
                print(f"{spk:8s}  {name:11s} {len(e):6d}   {1000 * np.median(np.abs(e)):9.0f} ms   {1000 * np.median(e):+4.0f} ms {100 * np.mean(np.abs(e) < 0.02):9.0f}%   {100 * np.mean(np.abs(e) < 0.04):9.0f}%")
        hv = np.array(heard_vowel)
        if len(hv):
            print(f"          vowels: heard at {np.median(hv):.2f} of the way through (quartiles {np.percentile(hv, 25):.2f}-{np.percentile(hv, 75):.2f}); inside the vowel: {100 * np.mean((hv > 0) & (hv < 1)):.0f}%")


if __name__ == "__main__":
    main()
