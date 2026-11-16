"""Copy a voice as settings: fit a voice profile to recordings of a real speaker.

Nothing of the recording ends up in the profile - only a handful of numbers that the
synthesizer's own controls understand:

    pitchHz          average pitch
    expressiveness   how far the pitch moves
    speed            speaking rate
    tractLengthCm    vocal-tract length (shifts all resonances)
    tone             10-band tone curve of the voiced sound (dB)
    formants, warmth, breathiness, chestResonance      (only with --refine)
    vowels           the speaker's own vowels: F1/F2/F3 of each vowel relative to the default
                     (only with --vowels; found in the recording without any labels)

It works by analysis-by-synthesis: speak the same sentences with the synthesizer, measure
both, nudge the settings toward the recording, repeat. With --refine, a speaker-verification
model (the kind used to check "is this the same person?") then picks the tract length and
voice-quality settings that make the synthesizer most resemble the recording.

  python tools/fit_voice.py --out me.json  take1.wav "What I said in take one."  take2.wav "And in take two."
  python tools/fit_voice.py --arctic bdl                  # fit to a reference speaker (development)
  python tools/fit_voice.py --arctic slt --similarity     # ... and report speaker similarity before/after
  python tools/fit_voice.py --matrix bdl,rms,slt,clb      # does each profile resemble its own speaker?

Use the result with `new TTS(profile)` in JavaScript, or `say --voice male --pitch ... --tone ...`.
A few sentences (20-30 s of clean speech) are enough; more is steadier.
"""
from __future__ import annotations

import argparse
import os
import json
import sys
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402

TONE_HZ = [125.0, 250.0, 500.0, 1000.0, 1500.0, 2200.0, 3000.0, 4000.0, 5500.0, 8000.0]
DEFAULTS = {
    "male": {"pitchHz": 110.0, "tractLengthCm": 17.5, "speed": 1.0, "expressiveness": 1.0},
    "female": {"pitchHz": 208.0, "tractLengthCm": 13.9, "speed": 1.0, "expressiveness": 1.06},
}


def tone_edges():
    c = np.array(TONE_HZ)
    mid = np.sqrt(c[:-1] * c[1:])
    return np.concatenate(([60.0], mid, [7900.0]))


def measure(paths: list[Path], gender: str) -> dict:
    """Pitch, pace and voiced spectrum of a set of recordings."""
    from scipy.signal import welch

    f0_all, speech_time, acc, fine = [], 0.0, 0, 0
    edges = tone_edges()
    sr_min = 1e9
    for p in paths:
        x, sr = E.load(p)
        sr_min = min(sr_min, sr)
        x16 = E.resample(x, sr, 16000)
        f0, voiced = E.f0_track(x16, 16000, gender)
        f0_all.append(f0[voiced])
        act = E.activity(x16, 16000)
        speech_time += act["dur"] * act["speech_frac"]
        segs = [x16[i * 80 + 280 : i * 80 + 360] for i in range(len(voiced)) if voiced[i] and i * 80 + 360 < len(x16)]
        if len(segs) < 10:
            continue
        f, pw = welch(np.concatenate(segs), 16000, nperseg=512)
        acc = acc + np.array([pw[(f >= lo) & (f < hi)].mean() for lo, hi in zip(edges[:-1], edges[1:])])
        fine = fine + pw
    f0 = np.concatenate(f0_all)
    med = float(np.median(f0))
    st = 12 * np.log2(f0 / med)
    band_db = 10 * np.log10(acc)
    band_db -= band_db[2:6].mean()  # relative to the 500-2200 Hz region
    return {"f0": med, "f0_sd": float(st.std()), "speech_time": speech_time, "tone": band_db, "fine": 10 * np.log10(fine), "freqs": f, "sr": sr_min}


def tract_ratio(target: dict, synth: dict) -> float:
    """How much the target's resonances are shifted against the synthesizer's (1.0 = same)."""
    f = target["freqs"]
    keep = (f > 400) & (f < 4200)
    logf = np.log(f[keep])
    grid = np.linspace(logf[0], logf[-1], 400)

    def ripple(db):
        y = np.interp(grid, logf, db[keep])
        k = 60  # remove the overall tilt, keep the peaks and valleys
        smooth = np.convolve(np.pad(y, k, mode="edge"), np.ones(2 * k + 1) / (2 * k + 1), "valid")
        return y - smooth

    t, s = ripple(target["fine"]), ripple(synth["fine"])
    best, best_c = 1.0, -2.0
    step = grid[1] - grid[0]
    for ratio in np.linspace(0.86, 1.16, 61):
        shift = int(round(np.log(ratio) / step))
        a = t[max(0, shift) : len(t) + min(0, shift)]
        b = s[max(0, -shift) : len(s) + min(0, -shift)]
        c = np.corrcoef(a, b)[0, 1]
        if c > best_c:
            best, best_c = ratio, c
    return float(best)


def profile_args(profile: dict) -> list[str]:
    """A profile as command-line options of the `say` example."""
    args = [
        "--pitch", f"{profile['pitchHz']:.1f}",
        "--tract", f"{profile['tractLengthCm']:.2f}",
        "--speed", f"{profile['speed']:.3f}",
        "--expressiveness", f"{profile['expressiveness']:.3f}",
        "--variation", "0",
        "--tone", ",".join(f"{v:.1f}" for v in profile["tone"]),
    ]
    for key, flag in (("warmth", "--warmth"), ("breathiness", "--breathiness"), ("chestResonance", "--chest"), ("nasality", "--nasality"), ("roughness", "--roughness"), ("openQuotient", "--open"), ("subglottal", "--subglottal")):
        if key in profile:
            args += [flag, f"{profile[key]:.2f}"]
    if "formants" in profile:
        args += ["--formants", ",".join(f"{v:.3f}" for v in profile["formants"])]
    if profile.get("vowels"):
        args += ["--vowels", ",".join(f"{k}={v[0]:.3f}/{v[1]:.3f}/{v[2]:.3f}" for k, v in profile["vowels"].items())]
    return args


RUN = os.environ.get("FIT_RUN", "")  # suffix that keeps side-by-side runs out of each other's files
_RECORDING: dict = {}  # per recording: what the recognizer heard, and its formant tracks


def synthesize(profile: dict, texts: list[str], tag: str) -> list[Path]:
    out = E.CACHE / "synth" / (tag + RUN)
    wavs = [out / f"{i:03d}.wav" for i in range(len(texts))]
    E.run_batch([(profile["gender"], str(w), t) for w, t in zip(wavs, texts)], profile_args(profile))
    return wavs


VOWEL_STEP = 0.4  # share of the measured difference applied per pass (two passes)
VOWEL_RANGE = (0.85, 1.18)
VOWEL_NAMES = ["IY", "IH", "EH", "AE", "AA", "AO", "UH", "UW", "AH", "AX", "ER", "EY", "AY", "OY", "AW", "OW"]


def model_upper(profile: dict) -> tuple[float, float]:
    """The synthesizer's own F4 and F5 (Hz) for a profile, before any formant multipliers."""
    k = 17.5 / profile["tractLengthCm"]
    f4 = 3460.0 * (1 + (k - 1) * 0.78)
    f5 = (4420.0 if profile["gender"] == "female" else 3950.0) * (1 + (k - 1) * 0.72)
    return f4, f5


def vowel_ratios(profile: dict, recordings: list[Path], texts: list[str]) -> tuple[dict, dict, np.ndarray, list]:
    """How the speaker's vowels differ from the synthesizer's current ones: per vowel, the
    ratio speaker / synthesizer of F1, F2 and F3.

    The recording needs no labels: a phoneme recognizer finds where each expected vowel is
    (about a third of the way into it), a formant tracker measures the speaker there, and
    the synthesizer reports its own formants for the same vowel in the same sentence."""
    from align import locate
    from copy_synth import HOP, clean, formant_tracks
    from transplant import say

    work = E.CACHE / "copy"
    work.mkdir(parents=True, exist_ok=True)
    dump = work / f"fit_frames{RUN}_{os.getpid()}.txt"
    acc: dict[str, list] = {}
    upper: list[list[float]] = [[], []]  # the speaker's F4 and F5 wherever the tracker finds them
    for rec, text in zip(recordings, texts):
        out = say(["--voice", profile["gender"], *profile_args(profile), "--timing", "--phones", text], {"VS_FRAMES_DUMP": str(dump)})
        toks = [(q[1], int(q[2]) / 1000, int(q[3]) / 1000) for q in (line.split() for line in out.splitlines()) if len(q) == 4 and q[0].isdigit()]
        model = np.array([[float(v) for v in line.split()] for line in dump.read_text().splitlines()])
        if rec not in _RECORDING:
            x, sr = E.load(rec)
            _RECORDING[rec] = (E.recognize_phones_timed(E.resample(x, sr, 16000)), *formant_tracks(rec, profile["gender"], n=5))
        heard, th, tr5 = _RECORDING[rec]
        where = locate(None, [t[0] for t in toks], heard)
        tr = clean(tr5[:, :3])
        for j, (ph, s0, s1) in enumerate(toks):
            if ph not in VOWEL_NAMES or j not in where or s1 - s0 < 0.05:
                continue
            window = (th >= where[j] - 0.01) & (th <= where[j] + 0.04)
            h = tr[window]
            keep = (h > 0).all(axis=1)
            h = h[keep]
            if len(h) < 4:
                continue
            for k in (0, 1):
                vals = tr5[window][keep][:, 3 + k]
                vals = vals[~np.isnan(vals)]
                if len(vals) >= 4:
                    upper[k].append(float(np.median(vals)))
            a, b = int((s0 + 0.3 * (s1 - s0)) / HOP), int((s0 + 0.6 * (s1 - s0)) / HOP) + 1
            m = np.median(model[a:b, :3], axis=0)
            acc.setdefault(ph, []).append(np.log(np.median(h, axis=0) / m))
    everything = np.vstack([v for vs in acc.values() for v in vs])
    overall = np.median(everything, axis=0)
    ratios, counts = {}, {}
    for ph in VOWEL_NAMES:
        vs = acc.get(ph, [])
        n = len(vs)
        # a vowel heard only a few times leans on the speaker's overall shift
        w = n / (n + 3)
        ratios[ph] = np.exp(w * (np.median(vs, axis=0) if n else overall) + (1 - w) * overall)
        counts[ph] = n
    return ratios, counts, np.exp(overall), upper


def fit_vowels(profile: dict, recordings: list[Path], texts: list[str], verbose: bool) -> dict:
    """Moves each vowel's formants toward the speaker's."""
    try:
        ratios, counts, overall, upper = vowel_ratios(profile, recordings, texts)
    except ImportError as err:
        if verbose:
            print(f"  vowels: skipped ({err}); install praat-parselmouth to copy the speaker's vowels", flush=True)
        return profile
    # Copied with restraint. Measured vowels are those of running speech, already reduced;
    # copying them in full made the voice less clear without making it more alike, so each
    # vowel moves only part of the way and never far. The third formant of ER sits so close
    # to the second that trackers merge them, so it is left alone.
    vowels = dict(profile.get("vowels") or {})
    for ph, r in ratios.items():
        cur = np.array(vowels.get(ph, [1.0, 1.0, 1.0]))
        step = np.exp(VOWEL_STEP * np.log(np.clip(r, 0.75, 1.33)))
        if ph == "ER":
            step[2] = 1.0
        vowels[ph] = np.clip(cur * step, *VOWEL_RANGE).round(3).tolist()
    # The upper resonances F4 and F5 hardly change from vowel to vowel; they belong to the
    # speaker. F5 is only trusted when the tracker found it in most vowels.
    formants = list(profile.get("formants") or [1.0] * 5)
    total = sum(counts.values())
    note = ""
    if len(upper[0]) >= max(20, 0.5 * total):
        m4, m5 = model_upper(profile)
        formants[3] = float(np.clip(np.median(upper[0]) / m4, 0.85, 1.2))
        formants[4] = float(np.clip(np.median(upper[1]) / m5, 0.85, 1.2)) if len(upper[1]) >= 0.5 * total else formants[3]
        formants = [round(v, 3) for v in formants]
        note = f"; F4 {np.median(upper[0]):.0f} Hz (synth {m4:.0f})" + (f", F5 {np.median(upper[1]):.0f} Hz (synth {m5:.0f})" if len(upper[1]) >= 0.5 * total else "")
        profile = {**profile, "formants": formants}
    if verbose:
        spread = np.sqrt(np.mean([np.log(r) ** 2 for r in ratios.values()]))
        far = sorted(ratios, key=lambda v: -np.abs(np.log(ratios[v])).max())[:3]
        detail = ", ".join(f"{v} {'/'.join(f'{100 * (x - 1):+.0f}%' for x in ratios[v])}" for v in far)
        print(f"  vowels: {sum(counts.values())} measured, speaker vs. synthesizer {100 * spread:.0f}% apart (overall F1 {100 * (overall[0] - 1):+.0f}%, F2 {100 * (overall[1] - 1):+.0f}%, F3 {100 * (overall[2] - 1):+.0f}%; furthest: {detail}){note}", flush=True)
    return {**profile, "vowels": vowels}


def _set(key):
    return lambda p, v: {**p, key: v}


REFINE = [
    ("tractLengthCm", lambda p: [round(p["tractLengthCm"] * k, 2) for k in (0.94, 0.97, 1.03, 1.06)] + [DEFAULTS[p["gender"]]["tractLengthCm"]], _set("tractLengthCm")),
    ("upper resonances", lambda p: [0.94, 0.97, 1.03, 1.06], lambda p, v: {**p, "formants": [*(p.get("formants") or [1.0] * 5)[:3], round((p.get("formants") or [1.0] * 5)[3] * v, 3), round((p.get("formants") or [1.0] * 5)[4] * v, 3)]}),
    ("openQuotient", lambda p: [0.58, 0.62, 0.70], _set("openQuotient")),
    ("warmth", lambda p: [0.15, 0.4, 0.65, 0.9], _set("warmth")),
    ("breathiness", lambda p: [0.05, 0.2, 0.4], _set("breathiness")),
    ("chestResonance", lambda p: [0.1, 0.4, 0.7], _set("chestResonance")),
]
# Nasality and roughness were tried here too: they bought 0.01-0.02 of resemblance and cost
# 0.3 of predicted naturalness, so they are left to the ear.


def refine(profile: dict, recordings: list[Path], texts: list[str], verbose: bool) -> dict:
    """Try a few values of each remaining setting; keep what makes the synthesizer resemble
    the speaker more, as judged by a speaker-verification model."""
    target = embed(recordings)
    n = min(10, len(texts))

    def score(p):
        return cos(embed(synthesize(p, texts[:n], "fit_voice_refine")), target)

    best = start = score(profile)
    kept = []
    for label, candidates, apply in REFINE:
        if label == "tractLengthCm" and profile.get("vowels"):
            continue  # the vowels were measured with this tract length
        for value in candidates(profile):
            trial = apply(profile, value)
            if trial == profile:
                continue
            sc = score(trial)
            if sc > best + 0.005:
                best, profile = sc, trial
                kept = [k for k in kept if k[0] != label] + [(label, value)]
    if verbose:
        print(f"  refine: similarity {start:.3f} -> {best:.3f} with {dict(kept)}", flush=True)
    return profile


def fit(recordings: list[Path], texts: list[str], gender: str | None, rounds: int = 4, verbose: bool = True, refine_similarity: bool = False, vowels: bool = False, trace=None) -> dict:
    """`trace(stage, profile)` is called after each stage (to report how far the copy has got)."""
    if gender is None:
        probe = measure(recordings[:3], "female")  # wide search range first
        gender = "female" if probe["f0"] > 160 else "male"
    target = measure(recordings, gender)
    profile = {"gender": gender, **DEFAULTS[gender], "tone": [0.0] * len(TONE_HZ)}
    for r in range(rounds + (4 if (refine_similarity or vowels) else 0)):
        if r == rounds:
            if trace:
                trace("pitch, pace, size, tone", profile)
            if vowels:
                for _ in range(2):
                    profile = fit_vowels(profile, recordings, texts, verbose)
                if trace:
                    trace("+ the speaker's vowels and upper resonances", profile)
            if refine_similarity:
                profile = refine(profile, recordings, texts, verbose)
                if trace:
                    trace("+ voice quality chosen for resemblance", profile)
        synth = measure(synthesize(profile, texts, "fit_voice"), gender)
        diff = target["tone"] - synth["tone"]
        # a band the recording cannot carry (above 80% of its Nyquist) follows its neighbour
        usable = [i for i, hz in enumerate(TONE_HZ) if hz <= 0.4 * target["sr"]]
        for i in range(len(TONE_HZ)):
            if i not in usable:
                diff[i] = diff[usable[-1]]
        if verbose:
            print(
                f"  round {r}: pitch {synth['f0']:.0f}/{target['f0']:.0f} Hz, movement {synth['f0_sd']:.2f}/{target['f0_sd']:.2f} st, "
                f"speech {synth['speech_time']:.1f}/{target['speech_time']:.1f} s, tone error {np.sqrt(np.mean(diff**2)):.1f} dB",
                flush=True,
            )
        profile["pitchHz"] = float(np.clip(profile["pitchHz"] * target["f0"] / synth["f0"], 60, 350))
        profile["expressiveness"] = float(np.clip(profile["expressiveness"] * (target["f0_sd"] / max(synth["f0_sd"], 0.3)) ** 0.7, 0.2, 3.0))
        profile["speed"] = float(np.clip(profile["speed"] * synth["speech_time"] / target["speech_time"], 0.6, 1.6))
        if r < 2:
            ratio = tract_ratio(target, synth)
            profile["tractLengthCm"] = float(np.clip(profile["tractLengthCm"] / ratio**0.7, 12.5, 21.0))
        # The curve is for the broad colour of the voice, so it is kept smooth and moderate:
        # sharp 15 dB corrections chase resonances an equalizer cannot move, and measure worse.
        smooth = np.convolve(np.pad(diff, 1, mode="edge"), [0.25, 0.5, 0.25], "valid")
        profile["tone"] = np.clip(np.array(profile["tone"]) + 0.8 * smooth, -10, 10).round(1).tolist()
    for k in ("pitchHz", "tractLengthCm", "speed", "expressiveness"):
        profile[k] = round(profile[k], 2)
    if trace:
        trace("final (tone re-fitted)", profile)
    return profile


_SV = None


def embed(paths: list[Path]) -> np.ndarray:
    """Average speaker-verification embedding (WavLM x-vectors) of some recordings."""
    global _SV
    import torch
    from transformers import Wav2Vec2FeatureExtractor, WavLMForXVector

    if _SV is None:
        name = "microsoft/wavlm-base-plus-sv"
        _SV = (Wav2Vec2FeatureExtractor.from_pretrained(name), WavLMForXVector.from_pretrained(name).to(E.device()).eval())
    fe, model = _SV
    out = []
    for p in paths:
        x, sr = E.load(p)
        x16 = E.resample(x, sr, 16000)
        with torch.no_grad():
            inp = fe(x16, sampling_rate=16000, return_tensors="pt")
            e = model(inp.input_values.to(E.device())).embeddings[0]
        out.append(torch.nn.functional.normalize(e, dim=0).cpu().numpy())
    return np.mean(out, axis=0)


def cos(u, v) -> float:
    return float(np.dot(u, v) / (np.linalg.norm(u) * np.linalg.norm(v)))


def default_profile(gender: str) -> dict:
    return {"gender": gender, **DEFAULTS[gender], "tone": [0.0] * len(TONE_HZ)}


def similarity(profile: dict, texts: list[str], real: list[Path]) -> tuple[float, float]:
    """Speaker similarity (1 = same speaker) of the default voice and of the fitted profile
    to real recordings of the speaker."""
    ref = embed(real)
    a = embed(synthesize(default_profile(profile["gender"]), texts, "fit_voice_base"))
    b = embed(synthesize(profile, texts, "fit_voice_fit"))
    return cos(a, ref), cos(b, ref)


FIT_IDS, TEST_IDS = range(17, 41), range(41, 53)


def arctic_files(spk: str, ids) -> list[Path]:
    import urllib.request

    out = []
    for i in ids:
        f = E.REF / f"{spk}_a{i:04d}.wav"
        if not f.exists():
            urllib.request.urlretrieve(f"{E.ARCTIC.format(spk=spk)}/wav/arctic_a{i:04d}.wav", f)
        out.append(f)
    return out


def quality(wavs: list[Path], arps: list[str]) -> tuple[float, float]:
    u, e = [], []
    for w, arp in zip(wavs, arps):
        x, sr = E.load(w)
        x16 = E.resample(x, sr, 16000)
        u.append(E.utmos(x16))
        e.append(E.phone_errors(E.target_phones(arp), E.recognize_phones(x16))["per"])
    return float(np.mean(u)), float(np.mean(e))


def matrix(speakers: list[str]) -> None:
    """Fit a profile to each speaker, then ask: does each profile resemble *its* speaker more
    than the others, and more than the default voice does? Sentences used for the comparison
    are different from the ones used for fitting."""
    from fit_durations import prompts

    texts_all = prompts()
    fit_texts = [texts_all[i] for i in FIT_IDS]
    test_texts = [texts_all[i] for i in TEST_IDS]
    arps = E.run_batch([("male", "-", t) for t in test_texts])
    real, real_fit, profiles = {}, {}, {}
    for spk in speakers:
        print(f"== {spk}", flush=True)
        profiles[spk] = fit(arctic_files(spk, FIT_IDS), fit_texts, None, refine_similarity=True, vowels=bool(os.environ.get("FIT_VOWELS")))
        (E.CACHE / f"voice_{spk}{RUN}.json").write_text(json.dumps(profiles[spk], indent=2))
        print("  " + json.dumps(profiles[spk]), flush=True)
        real[spk] = embed(arctic_files(spk, TEST_IDS))
        real_fit[spk] = embed(arctic_files(spk, FIT_IDS))

    voices = {f"default {g}": default_profile(g) for g in ("male", "female")}
    voices.update({f"fitted to {spk}": profiles[spk] for spk in speakers})
    print("\nspeaker similarity (1 = same speaker) to real recordings of ...   | naturalness, phoneme errors")
    print(f"{'':22s}" + "".join(f"{spk:>7s}" for spk in speakers) + "   |  utmos   per")
    for spk in speakers:  # how alike two different sets of sentences from the same person are
        row = "".join(f"{cos(real_fit[spk], real[o]):7.2f}" for o in speakers)
        u, e = quality(arctic_files(spk, TEST_IDS), arps)
        print(f"{'real ' + spk:22s}{row}   |  {u:.2f}   {e:.3f}")
    for name, prof in voices.items():
        wavs = synthesize(prof, test_texts, "fit_voice_matrix")
        emb = embed(wavs)
        u, e = quality(wavs, arps)
        print(f"{name:22s}" + "".join(f"{cos(emb, real[o]):7.2f}" for o in speakers) + f"   |  {u:.2f}   {e:.3f}", flush=True)


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("items", nargs="*", help="recording.wav \"its text\" pairs")
    ap.add_argument("--arctic", help="fit to a CMU Arctic speaker (bdl, slt, rms, clb ...) instead of your own files")
    ap.add_argument("--gender", choices=["male", "female"])
    ap.add_argument("--out", help="write the profile JSON here")
    ap.add_argument("--similarity", action="store_true", help="report speaker similarity before / after")
    ap.add_argument("--vowels", action="store_true", help="also copy the speaker's vowels and upper resonances (experimental: needs praat-parselmouth, torch and transformers; measured gains are mixed)")
    ap.add_argument("--refine", action="store_true", help="also tune tract length and voice quality for speaker similarity (needs torch + transformers)")
    ap.add_argument("--matrix", help="comma-separated CMU Arctic speakers: fit each, then compare every profile with every speaker")
    args = ap.parse_args()

    if args.matrix:
        matrix(args.matrix.split(","))
        return
    from fit_durations import prompts

    if args.arctic:
        texts_all = prompts()
        recordings = arctic_files(args.arctic, FIT_IDS)
        texts = [texts_all[i] for i in FIT_IDS]
        held = arctic_files(args.arctic, TEST_IDS)
    else:
        if len(args.items) < 2 or len(args.items) % 2:
            ap.error("give pairs of: recording.wav \"the words spoken in it\"")
        recordings = [Path(p) for p in args.items[0::2]]
        texts = list(args.items[1::2])
        held = recordings

    trace = None
    if args.similarity:
        ref = embed(held)
        held_texts = [prompts()[i] for i in TEST_IDS] if args.arctic else texts
        base = cos(embed(synthesize(default_profile(args.gender or "male"), held_texts, "fit_voice_trace")), ref) if args.gender else None

        def trace(stage, p):
            print(f"  >> {stage}: resemblance {cos(embed(synthesize(p, held_texts, 'fit_voice_trace')), ref):.3f}", flush=True)

    profile = fit(recordings, texts, args.gender, refine_similarity=args.refine, vowels=args.vowels, trace=trace)
    print(json.dumps(profile))
    if args.out:
        Path(args.out).write_text(json.dumps(profile, indent=2))
    if args.similarity:
        before, after = similarity(profile, texts[:12], held)
        print(f"speaker similarity to the real voice (1 = same speaker): default voice {before:.2f} -> fitted profile {after:.2f}")


if __name__ == "__main__":
    main()
