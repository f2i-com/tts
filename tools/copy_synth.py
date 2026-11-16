"""Copy synthesis: is the remaining gap in how the voice MOVES or in how it is RENDERED?

The synthesizer's renderer is driven with more and more of a real speaker's performance of
the same sentence instead of the rules: phone durations, pitch contour, then the measured
movement of the first three formants, then the loudness contour. If handing it the real
movements makes it sound much more natural, the rules are the limit; if not, the renderer is.

  <python with parselmouth> tools/copy_synth.py            # table: naturalness / phoneme errors
  COPY_KEEP=1 ... tools/copy_synth.py                      # keep the wavs in tools/.cache/synth/copy_*

Needs `praat-parselmouth` (formant tracking) on top of the evaluation requirements.
"""
from __future__ import annotations

import os
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402
from transplant import labels, match, natural_f0, say, synth_timing  # noqa: E402

OUT = E.CACHE / "synth"
WORK = E.CACHE / "copy"
VOWELS = set(E.VOWELS) | {"AO", "AX"}
APPROX = {"L", "R", "W", "Y"}
NASALS = {"M", "N", "NG"}
HOP = 0.005


def formant_tracks(wav: Path, gender: str, n: int = 3):
    """Praat (Burg) formant tracks of a recording: times, [F1, F2, F3] (or the first `n`) per 5 ms."""
    import parselmouth

    snd = parselmouth.Sound(str(wav))
    fm = snd.to_formant_burg(time_step=HOP, max_number_of_formants=5, maximum_formant=5000 if gender == "male" else 5500)
    t = fm.xs()
    tracks = []
    for x in t:
        # A very wide "formant" is the tracker filling a gap in the spectrum, not a resonance;
        # skip it so the real ones keep their numbers.
        found = []
        for k in range(1, 6):
            f, bw = fm.get_value_at_time(k, x), fm.get_bandwidth_at_time(k, x)
            if f == f and bw == bw and f > 150 and bw < 700:
                found.append(f)
        tracks.append((found + [np.nan] * n)[:n])
    return t, np.array(tracks)


def clean(tracks: np.ndarray) -> np.ndarray:
    """Median-filter a formant track over 5 frames and drop implausible values."""
    from scipy.ndimage import median_filter

    out = tracks.copy()
    bad = np.isnan(out).any(axis=1)
    out[bad] = 0
    for k, (lo, hi) in enumerate(((180, 1150), (500, 3200), (1400, 4200))):
        col = median_filter(out[:, k], size=5, mode="nearest")
        col[(col < lo) | (col > hi)] = 0
        out[:, k] = col
    out[(out == 0).any(axis=1)] = 0
    return out


def frame_level(x: np.ndarray, sr: int, n: int) -> np.ndarray:
    """RMS per 5 ms frame (25 ms window)."""
    win = int(sr * 0.025)
    hop = int(sr * HOP)
    xp = np.pad(x.astype(np.float64), (win // 2, win))
    return np.array([np.sqrt(np.mean(xp[i * hop : i * hop + win] ** 2) + 1e-12) for i in range(n)])


def main() -> None:
    texts = E.arctic()
    WORK.mkdir(parents=True, exist_ok=True)
    modes = ["rules", "dur+f0", "+formants (vowels)", "+formants (vowels, glides)", "+formants (all sonorants)", "+loudness"]
    which = {"+formants (vowels)": VOWELS, "+formants (vowels, glides)": VOWELS | APPROX, "+formants (all sonorants)": VOWELS | APPROX | NASALS, "+loudness": VOWELS | APPROX}
    rows: dict = {}
    for gender, spk in E.SPEAKERS.items():
        arps = E.run_batch([(gender, "-", t) for t in texts])
        for i, text in enumerate(texts, start=1):
            ref = E.REF / f"{spk}_a{i:04d}.wav"
            nat = labels(spk, i)
            syn, _ = synth_timing(gender, text)
            pairs = match(nat, syn)
            dur_file = WORK / "dur.txt"
            dur_file.write_text("".join(f"{syn[j][0]} {(nat[k][2] - nat[k][1]) * 1000:.1f}\n" for k, j in pairs))
            env_d = {"VS_DUR_FILE": str(dur_file)}
            syn_d, n_frames = synth_timing(gender, text, env_d)
            by_token = {t[0]: t for t in syn_d}
            tv, fv = natural_f0(spk, i, gender)
            ft, ftracks = formant_tracks(ref, gender)
            ftracks = clean(ftracks)
            xh, srh = E.load(ref)
            n_h = int(len(xh) / srh / HOP)
            level_h = frame_level(xh, srh, n_h)

            # per synth frame: the matching moment of the recording (or -1)
            nat_time = np.full(n_frames, -1.0)
            phone_of = np.full(n_frames, "", dtype=object)
            for k, j in pairs:
                tok = by_token.get(syn[j][0])
                if tok is None:
                    continue
                _, ph, s0, s1 = tok
                n0, n1 = nat[k][1], nat[k][2]
                for fr in range(int(round(s0 / HOP)), min(n_frames, int(round(s1 / HOP)))):
                    u = (fr * HOP - s0) / max(s1 - s0, 1e-6)
                    nat_time[fr] = n0 + u * (n1 - n0)
                    phone_of[fr] = ph

            f0 = np.zeros(n_frames)
            known = nat_time >= 0
            f0[known] = np.interp(nat_time[known], tv, fv)
            idx = np.nonzero(f0)[0]
            f0 = np.interp(np.arange(n_frames), idx, f0[idx])
            f0_file = WORK / "f0.txt"
            f0_file.write_text("".join(f"{v:.2f}\n" for v in f0))
            env_p = {**env_d, "VS_F0_FILE": str(f0_file)}

            def formant_file(phones: set, level: np.ndarray | None, name: str) -> Path:
                lines = []
                for fr in range(n_frames):
                    vals = [0.0, 0.0, 0.0]
                    if nat_time[fr] >= 0 and phone_of[fr] in phones:
                        k = int(np.clip(round((nat_time[fr] - ft[0]) / HOP), 0, len(ft) - 1))
                        vals = list(ftracks[k])
                    lv = f" {level[fr]:.3f}" if level is not None else ""
                    lines.append(f"{vals[0]:.1f} {vals[1]:.1f} {vals[2]:.1f}{lv}\n")
                path = WORK / name
                path.write_text("".join(lines))
                return path

            envs = {"rules": {}, "dur+f0": env_p}
            for mode, phones in which.items():
                if mode != "+loudness":
                    envs[mode] = {**env_p, "VS_FORMANT_FILE": str(formant_file(phones, None, f"fm_{len(phones)}.txt"))}

            def render(mode: str, env: dict) -> Path:
                wav = OUT / f"copy_{modes.index(mode)}" / f"{gender}_a{i:04d}.wav"
                wav.parent.mkdir(parents=True, exist_ok=True)
                say(["--voice", gender, "--variation", "0", text, str(wav)], env)
                return wav

            wavs = {mode: render(mode, env) for mode, env in envs.items()}

            # loudness: scale the voicing so the frame level follows the recording's
            xs, srs = E.load(wavs["+formants (vowels, glides)"])
            level_s = frame_level(xs, srs, n_frames)
            voiced_like = np.array([p in VOWELS | APPROX | NASALS for p in phone_of]) & known
            if voiced_like.sum() > 10:
                lh = level_h[np.clip((nat_time / HOP).astype(int), 0, n_h - 1)]
                ratio = (lh / np.median(lh[voiced_like])) / (level_s / np.median(level_s[voiced_like]))
                gain = np.where(voiced_like, np.clip(ratio, 0.35, 2.8), 0.0)
                env_l = {**env_p, "VS_FORMANT_FILE": str(formant_file(which["+loudness"], gain, "fm_level.txt"))}
                wavs["+loudness"] = render("+loudness", env_l)

            for mode, wav in wavs.items():
                x, sr = E.load(wav)
                x16 = E.resample(x, sr, 16000)
                per = E.phone_errors(E.target_phones(arps[i - 1]), E.recognize_phones(x16))["per"]
                rows.setdefault((gender, mode), []).append((E.utmos(x16), per))
            print(f"  {gender} {i}/{len(texts)}", file=sys.stderr, flush=True)

    print("the synthesizer's renderer, driven by ...            male utmos  per     female utmos  per")
    names = {
        "rules": "its own rules",
        "dur+f0": "a real speaker's durations and pitch",
        "+formants (vowels)": "  + measured formant movement in vowels",
        "+formants (vowels, glides)": "  + ... in vowels and l r w y",
        "+formants (all sonorants)": "  + ... and in nasals",
        "+loudness": "  + (vowels, glides) and the loudness contour",
    }
    for mode in modes:
        cells = []
        for gender in ("male", "female"):
            r = rows.get((gender, mode), [])
            cells.append(f"{np.mean([u for u, _ in r]):.2f}        {np.mean([p for _, p in r]):.3f}" if r else "   -          -  ")
        print(f"{names[mode]:52s} {cells[0]}   {cells[1]}")


if __name__ == "__main__":
    main()
