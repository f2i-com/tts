"""Objective listening tests for TTS (development tooling, not part of the library).

The synthesizer contains no AI. This script only *measures* its output, so that changes to
the engine can be judged by numbers and pictures rather than by one pair of ears:

  utmos      neural predictor of human naturalness ratings (1 = bad .. 5 = excellent)
  squim      a second, independent quality predictor (torchaudio SQUIM)
  wer        word error rate of a speech recognizer with no language model (wav2vec2)
  per        phoneme error rate of a phoneme recognizer, split into consonants / vowels,
             with the most frequent confusions listed (e.g. "K>T" = /k/ heard as /t/)
  f0 ...     pitch level, pitch movement (semitones), speaking rate, long-term spectrum

Everything is compared with recordings of real speakers reading the same sentences
(CMU Arctic `bdl` male and `slt` female).

Usage (from the crate root, after `cargo build --release --example say`):

  python tools/eval.py fetch                     # reference recordings + CMUdict (~7 MB)
  python tools/eval.py human                     # measure the human recordings
  python tools/eval.py score --tag baseline      # synthesize + measure
  python tools/eval.py score --tag try1 --against baseline
  python tools/eval.py spectro --sentence 8 --voice male --tags baseline,try1

Models are downloaded on first use into the normal torch / Hugging Face caches (~2 GB).
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
import urllib.request
from collections import Counter
from math import gcd
from pathlib import Path

import numpy as np

if os.environ.get("VS_EVAL_DEVICE", "").lower() == "cpu":
    # Run the models on the CPU without touching the GPU driver at all (slower; for when
    # the GPU is busy or its driver is misbehaving). Affects this process only.
    import torch

    torch.cuda.is_available = lambda: False

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")
os.environ.setdefault("HF_HUB_DISABLE_SYMLINKS_WARNING", "1")

ROOT = Path(__file__).resolve().parent.parent
CACHE = ROOT / "tools" / ".cache"
REF = CACHE / "ref"
RUNS = CACHE / "runs"
DICT = CACHE / "cmudict.dict"
# VS_SAY points the tools at another build of the `say` example (e.g. a frozen copy for a
# long-running measurement while the engine is being rebuilt).
SAY = Path(os.environ.get("VS_SAY") or ROOT / "target" / "release" / "examples" / ("say.exe" if sys.platform == "win32" else "say"))
SPEAKERS = {"male": "bdl", "female": "slt"}
N_ARCTIC = 16
ARCTIC = "http://festvox.org/cmu_arctic/cmu_arctic/cmu_us_{spk}_arctic"
TUNE = ""  # VS_TUNE string forwarded to the synthesizer (development parameter overrides)
QUICK = False
FEMALE_VOICES = {"female", "bright", "whisper", "clear_alto", "warm_mezzo", "bright_soprano", "young_bright", "soft_whisper"}

# Phonetically balanced "Harvard sentences" (IEEE 1969), used for intelligibility only.
HARVARD = [
    "The birch canoe slid on the smooth planks.",
    "Glue the sheet to the dark blue background.",
    "It's easy to tell the depth of a well.",
    "These days a chicken leg is a rare dish.",
    "Rice is often served in round bowls.",
    "The juice of lemons makes fine punch.",
    "The box was thrown beside the parked truck.",
    "The hogs were fed chopped corn and garbage.",
    "Four hours of steady work faced us.",
    "A large size in stockings is hard to sell.",
    "The boy was there when the sun rose.",
    "A rod is used to catch pink salmon.",
    "The source of the huge river is the clear spring.",
    "Kick the ball straight and follow through.",
    "Help the woman get back to her feet.",
    "A pot of tea helps to pass the evening.",
    "Smoky fires lack flame and heat.",
    "The soft cushion broke the man's fall.",
    "The salt breeze came across from the sea.",
    "The girl at the booth sold fifty bonds.",
    "The small pup gnawed a hole in the sock.",
    "The fish twisted and turned on the bent hook.",
    "Press the pants and sew a button on the vest.",
    "The swan dive was far short of perfect.",
    "The beauty of the view stunned the young boy.",
    "Two blue fish swam in the tank.",
    "Her purse was full of useless trash.",
    "The colt reared and threw the tall rider.",
    "It snowed, rained, and hailed the same morning.",
    "Read verse out loud for pleasure.",
]

# More Harvard sentences that were never used while tuning the engine (validation only).
HELDOUT = [
    "A king ruled the state in the early days.",
    "The ship was torn apart on the sharp reef.",
    "Sickness kept him home the third week.",
    "The wide road shimmered in the hot sun.",
    "The lazy cow lay in the cool grass.",
    "Lift the square stone over the fence.",
    "The rope will bind the seven books at once.",
    "Hop over the fence and plunge in.",
    "The friendly gang left the drug store.",
    "Mesh wire keeps chicks inside.",
    "The frosty air passed through the coat.",
    "The crooked maze failed to fool the mouse.",
    "Adding fast leads to wrong sums.",
    "The show was a flop from the very start.",
    "A saw is a tool used for making boards.",
    "The wagon moved on well oiled wheels.",
    "March the soldiers past the next hill.",
    "A cup of sugar makes sweet fudge.",
    "Place a rosebush near the porch steps.",
    "Both lost their lives in the raging storm.",
]


# ----------------------------------------------------------------------------- data
def fetch() -> None:
    REF.mkdir(parents=True, exist_ok=True)
    if not DICT.exists():
        urllib.request.urlretrieve("https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict", DICT)
    for gender, spk in SPEAKERS.items():
        base = ARCTIC.format(spk=spk)
        txt = REF / f"{spk}.txt"
        if not txt.exists():
            urllib.request.urlretrieve(f"{base}/etc/txt.done.data", txt)
        for i in range(1, N_ARCTIC + 1):
            wav = REF / f"{spk}_a{i:04d}.wav"
            if not wav.exists():
                urllib.request.urlretrieve(f"{base}/wav/arctic_a{i:04d}.wav", wav)
        print(f"{gender}: {spk} reference ready")


def arctic() -> list[str]:
    out = []
    for line in (REF / "bdl.txt").read_text().splitlines()[:N_ARCTIC]:
        out.append(re.match(r'\( *\S+ +"(.*)" *\)', line).group(1))
    return out


def load(path: Path) -> tuple[np.ndarray, int]:
    import soundfile as sf

    x, sr = sf.read(str(path), dtype="float32")
    return (x.mean(axis=1) if x.ndim > 1 else x), sr


def resample(x: np.ndarray, sr: int, to: int) -> np.ndarray:
    if sr == to:
        return x
    from scipy.signal import resample_poly

    g = gcd(sr, to)
    return resample_poly(x, to // g, sr // g).astype(np.float32)


def run_batch(jobs: list[tuple[str, str, str]], extra: list[str] | None = None, want_stderr: bool = False):
    """jobs: (voice, out path or '-', text). Returns the ARPAbet transcription of each."""
    CACHE.mkdir(parents=True, exist_ok=True)
    for _, out, _ in jobs:
        if out != "-":
            Path(out).parent.mkdir(parents=True, exist_ok=True)
    tsv = CACHE / f"jobs_{os.getpid()}.tsv"  # per process: measurements may run side by side
    tsv.write_text("".join(f"{v}\t{o}\t{t}\n" for v, o, t in jobs), encoding="utf-8")
    cmd = [str(SAY), "--batch", str(tsv), *(extra or [])]
    if DICT.exists():
        cmd += ["--dict", str(DICT)]
    env = dict(os.environ)
    if TUNE:
        env["VS_TUNE"] = TUNE
    res = subprocess.run(cmd, check=True, capture_output=True, text=True, encoding="utf-8", env=env)
    return res.stderr if want_stderr else res.stdout.splitlines()


# ----------------------------------------------------------------------------- models
_models: dict = {}


def device():
    import torch

    return "cuda" if torch.cuda.is_available() else "cpu"


def utmos(x16: np.ndarray) -> float:
    import torch

    if "utmos" not in _models:
        _models["utmos"] = torch.hub.load("tarepan/SpeechMOS:v1.2.0", "utmos22_strong", trust_repo=True, verbose=False).to(device()).eval()
    with torch.no_grad():
        return float(_models["utmos"](torch.from_numpy(x16).unsqueeze(0).to(device()), 16000).item())


def squim(x16: np.ndarray, ref16: np.ndarray) -> float:
    import torch
    import torchaudio

    if "squim" not in _models:
        _models["squim"] = torchaudio.pipelines.SQUIM_SUBJECTIVE.get_model().to(device()).eval()
    with torch.no_grad():
        a = torch.from_numpy(x16).unsqueeze(0).to(device())
        b = torch.from_numpy(ref16).unsqueeze(0).to(device())
        return float(_models["squim"](a, b).item())


def _ctc(name: str):
    import torch
    from huggingface_hub import hf_hub_download
    from transformers import Wav2Vec2ForCTC

    if name not in _models:
        vocab = json.load(open(hf_hub_download(name, "vocab.json"), encoding="utf-8"))
        _models[name] = ({v: k for k, v in vocab.items()}, Wav2Vec2ForCTC.from_pretrained(name).to(device()).eval())
    inv, model = _models[name]

    def run(x16: np.ndarray) -> list[str]:
        x = (x16 - x16.mean()) / (x16.std() + 1e-7)
        with torch.no_grad():
            ids = model(torch.from_numpy(x.astype(np.float32))[None].to(device())).logits.argmax(-1)[0].tolist()
        out, prev = [], None
        for i in ids:
            if i != prev and not inv[i].startswith("<"):
                out.append(inv[i])
            prev = i
        return out

    return run


def transcribe(x16: np.ndarray) -> str:
    return "".join(_ctc("facebook/wav2vec2-base-960h")(x16)).replace("|", " ").lower().strip()


# espeak IPA symbols (as emitted by the phoneme recognizer) -> reduced ARPAbet
IPA = {
    "p": "P", "b": "B", "t": "T", "d": "D", "k": "K", "ɡ": "G", "g": "G", "tʃ": "CH", "dʒ": "JH", "f": "F", "v": "V",
    "θ": "TH", "ð": "DH", "s": "S", "z": "Z", "ʃ": "SH", "ʒ": "ZH", "h": "HH", "m": "M", "n": "N", "ŋ": "NG", "l": "L",
    "ɹ": "R", "r": "R", "w": "W", "j": "Y", "ɾ": "D", "ʔ": "", "x": "K", "ɬ": "L", "ɲ": "N", "ç": "HH",
    "i": "IY", "iː": "IY", "ɪ": "IH", "ᵻ": "IH", "ɛ": "EH", "e": "EH", "eː": "EY", "æ": "AE", "a": "AA", "aː": "AA", "ɑ": "AA",
    "ɑː": "AA", "ɔ": "AA", "ɔː": "AA", "ɒ": "AA", "ʊ": "UH", "u": "UW", "uː": "UW", "ʉ": "UW", "ʌ": "AH", "ə": "AH", "ɐ": "AH",
    "ɚ": "ER", "ɜː": "ER", "ɝ": "ER", "ɜ": "ER", "eɪ": "EY", "aɪ": "AY", "ɔɪ": "OY", "aʊ": "AW", "oʊ": "OW", "o": "OW",
    "oː": "OW", "əʊ": "OW", "əl": "AH L", "ɑːɹ": "AA R", "ɔːɹ": "AA R", "oːɹ": "AA R", "ɪɹ": "IH R", "ɛɹ": "EH R", "ʊɹ": "UH R",
    "aɪɚ": "AY ER", "aɪə": "AY AH", "n̩": "AH N", "iə": "IY AH", "eə": "EH AH", "ʊə": "UH AH", "ɪə": "IH AH", "y": "UW", "ø": "ER",
}
VOWELS = {"IY", "IH", "EH", "AE", "AA", "UH", "UW", "AH", "ER", "EY", "AY", "OY", "AW", "OW"}


def recognize_phones(x16: np.ndarray) -> list[str]:
    out: list[str] = []
    for tok in _ctc("facebook/wav2vec2-lv-60-espeak-cv-ft")(x16):
        mapped = IPA.get(tok)
        if mapped is None:
            mapped = IPA.get(tok[0], "?") if tok else ""
        out.extend(mapped.split())
    return out


def recognize_phones_timed(x16: np.ndarray) -> list[tuple[str, float]]:
    """Like `recognize_phones`, with the moment (s) at which each sound was heard."""
    import torch

    name = "facebook/wav2vec2-lv-60-espeak-cv-ft"
    _ctc(name)
    inv, model = _models[name]
    x = (x16 - x16.mean()) / (x16.std() + 1e-7)
    with torch.no_grad():
        ids = model(torch.from_numpy(x.astype(np.float32))[None].to(device())).logits.argmax(-1)[0].tolist()
    out, k = [], 0
    while k < len(ids):
        j = k
        while j + 1 < len(ids) and ids[j + 1] == ids[k]:
            j += 1
        tok = inv[ids[k]]
        if not tok.startswith("<"):
            mapped = IPA.get(tok)
            if mapped is None:
                mapped = IPA.get(tok[0], "?") if tok else ""
            for sym in mapped.split():
                out.append((sym, (k + j) / 2 * 0.02 + 0.0125))  # 20 ms frames
        k = j + 1
    return out


def target_phones(arpabet: str) -> list[str]:
    out = []
    for p in arpabet.replace("|", " ").split():
        base = re.sub(r"\d", "", p)
        out.append({"AO": "AA", "AX": "AH"}.get(base, base))
    return out


def align(ref: list[str], hyp: list[str]) -> list[tuple[str | None, str | None]]:
    """Levenshtein alignment -> list of (ref symbol or None, hyp symbol or None)."""
    n, m = len(ref), len(hyp)
    d = np.zeros((n + 1, m + 1), dtype=np.int32)
    d[:, 0] = np.arange(n + 1)
    d[0, :] = np.arange(m + 1)
    for i in range(1, n + 1):
        for j in range(1, m + 1):
            d[i, j] = min(d[i - 1, j] + 1, d[i, j - 1] + 1, d[i - 1, j - 1] + (ref[i - 1] != hyp[j - 1]))
    out = []
    i, j = n, m
    while i > 0 or j > 0:
        if i > 0 and j > 0 and d[i, j] == d[i - 1, j - 1] + (ref[i - 1] != hyp[j - 1]):
            out.append((ref[i - 1], hyp[j - 1]))
            i, j = i - 1, j - 1
        elif i > 0 and d[i, j] == d[i - 1, j] + 1:
            out.append((ref[i - 1], None))
            i -= 1
        else:
            out.append((None, hyp[j - 1]))
            j -= 1
    return out[::-1]


def phone_errors(ref: list[str], hyp: list[str]) -> dict:
    pairs = align(ref, hyp)
    conf = Counter()
    cons_err = cons_n = vow_err = vow_n = ins = 0
    for r, h in pairs:
        if r is None:
            ins += 1
            conf[f"+{h}"] += 1
            continue
        is_v = r in VOWELS
        if is_v:
            vow_n += 1
        else:
            cons_n += 1
        # /t/ heard as /d/ is the American tap ("letter", "get it"): real speakers do it too
        if r != h and not (r == "T" and h == "D"):
            if is_v:
                vow_err += 1
            else:
                cons_err += 1
            conf[f"{r}>{h or '-'}"] += 1
    return {
        "per": (cons_err + vow_err + ins) / max(len(ref), 1),
        "cons_err": cons_err / max(cons_n, 1),
        "vowel_err": vow_err / max(vow_n, 1),
        "ins_rate": ins / max(len(ref), 1),
        "confusions": dict(conf),
    }


def norm_text(s: str) -> str:
    return re.sub(r"\s+", " ", re.sub(r"[^a-z' ]", " ", s.lower())).strip()


# ----------------------------------------------------------------------------- signal measures
def f0_track(x: np.ndarray, sr: int, gender: str):
    """Normalized-autocorrelation pitch tracker (5 ms hop). Returns (f0 Hz, voiced mask)."""
    from scipy.signal import medfilt

    lo, hi = (60, 280) if gender == "male" else (100, 420)
    win, hop = int(sr * 0.04), int(sr * 0.005)
    n = max(0, (len(x) - win) // hop + 1)
    if n == 0:
        return np.zeros(0), np.zeros(0, dtype=bool)
    idx = np.arange(win)[None, :] + hop * np.arange(n)[:, None]
    frames = x[idx] * np.hanning(win)[None, :]
    energy = (frames**2).sum(axis=1)
    spec = np.fft.rfft(frames, 2 * win, axis=1)
    ac = np.fft.irfft(spec * np.conj(spec), axis=1)[:, :win]
    wac = np.fft.irfft(np.abs(np.fft.rfft(np.hanning(win), 2 * win)) ** 2)[:win]
    nac = ac / (ac[:, :1] + 1e-12) / (wac / wac[0])[None, :]
    lmin, lmax = int(sr / hi), int(sr / lo)
    seg = nac[:, lmin:lmax]
    seg = seg - 0.15 * np.arange(seg.shape[1])[None, :] / seg.shape[1]  # prefer the shorter period
    best = seg.argmax(axis=1)
    peak = nac[np.arange(n), best + lmin]
    f0 = sr / (best + lmin).astype(float)
    voiced = (peak > 0.55) & (energy > 0.003 * np.percentile(energy, 98))
    f0 = medfilt(np.where(voiced, f0, 0.0), 5)
    return f0, voiced & (f0 > 0)


def pitch_stats(x: np.ndarray, sr: int, gender: str) -> dict:
    f0, voiced = f0_track(x, sr, gender)
    v = f0[voiced]
    if len(v) < 10:
        return {"f0_med": 0.0, "f0_sd_st": 0.0, "f0_range_st": 0.0, "voiced": 0.0}
    st = 12 * np.log2(v / np.median(v))
    return {
        "f0_med": float(np.median(v)),
        "f0_sd_st": float(np.std(st)),
        "f0_range_st": float(np.percentile(st, 95) - np.percentile(st, 5)),
        "voiced": float(np.mean(voiced)),
    }


BANDS = [(80, 250), (250, 500), (500, 1000), (1000, 2000), (2000, 3000), (3000, 4000), (4000, 5500), (5500, 7500)]


def ltas(x: np.ndarray, sr: int) -> np.ndarray:
    """Long-term average spectrum: energy (dB) in broad bands, relative to the 250-4000 Hz level."""
    from scipy.signal import welch

    f, p = welch(x, sr, nperseg=1024)
    band = np.array([p[(f >= lo) & (f < hi)].sum() + 1e-20 for lo, hi in BANDS])
    return 10 * np.log10(band) - 10 * np.log10(band[1:6].sum())


VBANDS = [(0, 500), (500, 1000), (1000, 1500), (1500, 2000), (2000, 2500), (2500, 3000), (3000, 3500), (3500, 4000), (4000, 5000), (5000, 6500), (6500, 8000)]


def voiced_spectrum(x16: np.ndarray, gender: str) -> np.ndarray | None:
    """Power in bands, summed over the voiced frames only."""
    from scipy.signal import welch

    f0, voiced = f0_track(x16, 16000, gender)
    segs = [x16[i * 80 + 280 : i * 80 + 360] for i in range(len(voiced)) if voiced[i] and i * 80 + 360 < len(x16)]
    if len(segs) < 20:
        return None
    f, p = welch(np.concatenate(segs), 16000, nperseg=512)
    return np.array([p[(f >= lo) & (f < hi)].sum() for lo, hi in VBANDS])


def activity(x: np.ndarray, sr: int) -> dict:
    hop = int(sr * 0.01)
    n = len(x) // hop
    e = np.sqrt((x[: n * hop].reshape(n, hop) ** 2).mean(axis=1) + 1e-12)
    active = e > np.percentile(e, 95) * 0.06
    return {"speech_frac": float(active.mean()), "dur": len(x) / sr}


def syllables(text: str) -> int:
    return sum(
        max(1, len(re.findall(r"[aeiouy]+", w.lower())) - (1 if w.lower().endswith("e") and len(w) > 3 else 0))
        for w in re.findall(r"[A-Za-z']+", text)
    )


def measure(path: Path, text: str, arpabet: str, gender: str, nmr16: np.ndarray | None, models: bool) -> dict:
    import jiwer

    x, sr = load(path)
    x16 = resample(x, sr, 16000)
    out: dict = {"file": path.name}
    if models:
        out["utmos"] = utmos(x16)
        if not QUICK:
            if nmr16 is not None:
                out["squim"] = squim(x16, nmr16)
            hyp = transcribe(x16)
            out["hyp"] = hyp
            out["wer"] = float(jiwer.wer(norm_text(text), hyp)) if hyp else 1.0
        ph = recognize_phones(x16)
        out["phones"] = " ".join(ph)
        out.update(phone_errors(target_phones(arpabet), ph))
    out.update(pitch_stats(x16, 16000, gender))
    out.update(activity(x16, 16000))
    out["syl_rate"] = syllables(text) / max(out["dur"] * out["speech_frac"], 1e-6)
    out["ltas"] = ltas(x16, 16000).tolist()
    return out


def summarize(rows: list[dict]) -> dict:
    keys = [k for k in rows[0] if isinstance(rows[0][k], float)]
    out = {k: float(np.mean([r[k] for r in rows if k in r])) for k in keys}
    out["ltas"] = np.mean([r["ltas"] for r in rows], axis=0).tolist()
    conf = Counter()
    for r in rows:
        conf.update(r.get("confusions", {}))
    out["confusions"] = dict(conf)
    return out


COLS = ["utmos", "squim", "wer", "per", "cons_err", "vowel_err", "ins_rate", "f0_med", "f0_sd_st", "f0_range_st", "syl_rate"]


def fmt(summary: dict) -> str:
    return "  ".join(f"{c}={summary[c]:.2f}" for c in COLS if c in summary)


def top_confusions(summary: dict, n: int = 22) -> str:
    conf = Counter(summary.get("confusions", {}))
    return "  ".join(f"{k}:{v}" for k, v in conf.most_common(n))


def gender_of(voice: str) -> str:
    return "female" if voice in FEMALE_VOICES else "male"


def cmd_score(args) -> None:
    voices = args.voices.split(",")
    arc = arctic()
    sets = {"arctic": arc, "harvard": HARVARD, "heldout": HELDOUT}
    use = ["arctic", "harvard"] if args.set == "all" else [args.set]
    extra = args.say.split() if args.say else []
    result: dict = {}
    for voice in voices:
        gender = gender_of(voice)
        nmr = resample(*load(REF / f"{SPEAKERS[gender]}_a0009.wav"), 16000)
        result[voice] = {}
        for set_name in use:
            texts = sets[set_name]
            prefix = "x" if set_name == "heldout" else set_name[0]
            wavs = [CACHE / "synth" / args.tag / f"{voice}_{prefix}{i + 1:04d}.wav" for i in range(len(texts))]
            if args.existing:
                # measure WAV files that are already there (e.g. rendered by another build)
                arps = run_batch([(voice, "-", t) for t in texts], extra)
            else:
                arps = run_batch([(voice, str(w), t) for w, t in zip(wavs, texts)], extra)
            rows = [measure(w, t, a, gender, nmr if set_name == "arctic" else None, not args.fast) for w, t, a in zip(wavs, texts, arps)]
            summary = summarize(rows)
            result[voice][set_name] = {"summary": summary, "rows": rows}
            print(f"{args.tag:>10s} {voice:7s} {set_name:7s} {fmt(summary)}")
            if args.verbose:
                for r, t in zip(rows, texts):
                    print(f"      {r.get('utmos', 0):.2f} per={r.get('per', 0):.2f} [{r.get('hyp', '')}]\n           {r.get('phones', '')}")
        if not args.fast:
            merged = Counter()
            for set_name in use:
                merged.update(result[voice][set_name]["summary"]["confusions"])
            print(f"           confusions: {top_confusions({'confusions': merged})}")
    RUNS.mkdir(parents=True, exist_ok=True)
    (RUNS / f"{args.tag}.json").write_text(json.dumps(result))

    if args.against and (RUNS / f"{args.against}.json").exists():
        old = json.loads((RUNS / f"{args.against}.json").read_text())
        for voice in voices:
            for set_name in use:
                if voice in old and set_name in old[voice]:
                    print(f"{args.against:>10s} {voice:7s} {set_name:7s} {fmt(old[voice][set_name]['summary'])}")
    human_file = RUNS / "human.json"
    if human_file.exists() and "arctic" in use:
        human = json.loads(human_file.read_text())
        print(f"  long-term spectrum vs human, dB in bands {' '.join(f'{lo}-{hi}' for lo, hi in BANDS)}")
        for voice in voices:
            g = gender_of(voice)
            d = np.array(result[voice]["arctic"]["summary"]["ltas"]) - np.array(human[g]["summary"]["ltas"])
            print(f"    {voice:7s}", "  ".join(f"{v:+.1f}" for v in d))


def cmd_human(args) -> None:
    arc = arctic()
    arps = run_batch([("male", "-", t) for t in arc])
    result = {}
    for gender, spk in SPEAKERS.items():
        other = resample(*load(REF / f"{SPEAKERS['female' if gender == 'male' else 'male']}_a0009.wav"), 16000)
        rows = [measure(REF / f"{spk}_a{i + 1:04d}.wav", t, a, gender, other, not args.fast) for i, (t, a) in enumerate(zip(arc, arps))]
        result[gender] = {"summary": summarize(rows), "rows": rows}
        print(f"{'human':>10s} {gender:7s} arctic  {fmt(result[gender]['summary'])}")
        print(f"           confusions: {top_confusions(result[gender]['summary'])}")
    RUNS.mkdir(parents=True, exist_ok=True)
    (RUNS / "human.json").write_text(json.dumps(result))


def cmd_probe(args) -> None:
    """Synthesize arbitrary text and print what the phoneme recognizer hears."""
    voices = args.voices.split(",")
    extra = args.say.split() if args.say else []
    texts = [t.strip() for t in args.text.split("|") if t.strip()]
    for text in texts:
        print(f"TEXT   {text}")
        for voice in voices:
            wav = CACHE / "probe" / f"{voice}.wav"
            arp = run_batch([(voice, str(wav), text)], extra)[0]
            x16 = resample(*load(wav), 16000)
            raw = " ".join(_ctc("facebook/wav2vec2-lv-60-espeak-cv-ft")(x16))
            err = phone_errors(target_phones(arp), recognize_phones(x16))
            if voice == voices[0]:
                print(f"TARGET {arp}")
            print(f"{voice:6s} {raw}   per={err['per']:.2f} utmos={utmos(x16):.2f}")
            bad = "  ".join(f"{k}:{v}" for k, v in Counter(err["confusions"]).most_common(12))
            if bad:
                print(f"       {bad}")


def cmd_sweep(args) -> None:
    """Compare several VS_TUNE settings: one compact line per setting and voice."""
    global TUNE, QUICK
    QUICK = True
    voices = args.voices.split(",")
    arc = arctic()
    texts = arc + HARVARD
    if not args.small:
        # 100 more Arctic prompts: differences of a point or two need more than 46 sentences
        lines = (REF / "bdl.txt").read_text().splitlines()[N_ARCTIC : N_ARCTIC + 100]
        texts = texts + [re.match(r'\( *\S+ +"(.*)" *\)', line).group(1) for line in lines]
    human = json.loads((RUNS / "human.json").read_text()) if (RUNS / "human.json").exists() else None
    print(f"{'setting':28s} voice   utmos  per   cons  vowel ins   | spectrum vs human (dB): {' '.join(f'{lo}-{hi}' for lo, hi in BANDS)}")
    for cfg in args.configs.split(";"):
        parts = [x.strip() for x in cfg.split(":")] + ["", ""]
        name, TUNE, extra = parts[0], parts[1], parts[2].split()
        for voice in voices:
            gender = gender_of(voice)
            wavs = [CACHE / "synth" / f"sweep_{os.getpid()}" / f"{voice}_{i:03d}.wav" for i in range(len(texts))]  # per process: sweeps may run side by side
            arps = run_batch([(voice, str(w), t) for w, t in zip(wavs, texts)], extra)
            rows = [measure(w, t, a, gender, None, True) for w, t, a in zip(wavs, texts, arps)]
            sm = summarize(rows)
            line = f"{name:28s} {voice:7s} {sm['utmos']:.2f}  {sm['per']:.3f} {sm['cons_err']:.3f} {sm['vowel_err']:.3f} {sm['ins_rate']:.3f} sd={sm['f0_sd_st']:.2f} syl/s={sm['syl_rate']:.2f}"
            if human:
                d = np.array(summarize(rows[: len(arc)])["ltas"]) - np.array(human[gender]["summary"]["ltas"])
                line += " | " + " ".join(f"{v:+5.1f}" for v in d)
            print(line, flush=True)
            if args.confusions:
                print(f"{'':28s}         {top_confusions(sm, 18)}")


# Average speech power of English sounds relative to the vowels (Fletcher 1953), in dB.
NATURAL_LEVELS = {
    "R": -3, "L": -6, "W": -5, "Y": -6, "SH": -7, "NG": -8, "M": -9, "CH": -10, "N": -10, "JH": -12, "ZH": -13, "Z": -14,
    "S": -14, "T": -14, "G": -14, "K": -15, "V": -15, "DH": -16, "B": -18, "D": -18, "P": -18, "F": -19, "TH": -24, "HH": -18,
}


def cmd_levels(args) -> None:
    global TUNE
    TUNE = args.tune
    texts = arctic() + HARVARD
    for voice in args.voices.split(","):
        jobs = [(voice, str(CACHE / "synth" / "levels" / f"{i:03d}.wav"), t) for i, t in enumerate(texts)]
        stderr = run_batch(jobs, ["--levels", *args.say.split()], want_stderr=True)
        print(f"--- {voice}: level re vowels (dB) | natural | difference | mean duration")
        for line in stderr.splitlines()[1:]:
            parts = line.split()
            name, db, ms = parts[0], float(parts[1]), parts[3]
            nat = NATURAL_LEVELS.get(name)
            if nat is not None:
                print(f"  {name:6s} {db:+6.1f}  {nat:+4d}  {db - nat:+6.1f}   {ms} ms")
            elif "." in name:
                print(f"  {name:6s} {db:+6.1f}                 {ms} ms")


def cmd_spectro(args) -> None:
    import matplotlib

    matplotlib.use("Agg")
    import matplotlib.pyplot as plt
    from scipy.signal import spectrogram

    idx = args.sentence
    texts = HARVARD if args.set == "harvard" else arctic()
    text = args.text or texts[idx - 1]
    gender = gender_of(args.voice)
    items = []
    if not args.no_human and not args.text and args.set == "arctic":
        items.append(("human " + SPEAKERS[gender], REF / f"{SPEAKERS[gender]}_a{idx:04d}.wav"))
    for tag in args.tags.split(","):
        wav = CACHE / "synth" / tag / (f"{args.voice}_{args.set[0]}{idx:04d}.wav" if not args.text else f"{args.voice}_custom.wav")
        if args.text or not wav.exists():
            run_batch([(args.voice, str(wav), text)])
        items.append((f"synth {tag}", wav))
    t0, t1 = args.t0, args.t1
    fig, axes = plt.subplots(len(items) * 2, 1, figsize=(15, 4.6 * len(items)), gridspec_kw={"height_ratios": [3, 1] * len(items)}, squeeze=False)
    axes = axes[:, 0]
    for k, (label, path) in enumerate(items):
        x, sr = load(path)
        x = x / (np.abs(x).max() + 1e-9)
        f, t, s = spectrogram(x, sr, nperseg=int(sr * 0.006), noverlap=int(sr * 0.005), nfft=1024, window="hann")
        db = 10 * np.log10(s + 1e-12)
        db -= db.max()
        ax = axes[2 * k]
        ax.pcolormesh(t, f, db, vmin=-70, vmax=0, cmap="magma", shading="auto")
        ax.set_ylim(0, min(args.fmax, sr / 2))
        ax.set_title(f"{label}: {text}", fontsize=10, loc="left")
        ax.set_ylabel("Hz")
        f0, voiced = f0_track(resample(x, sr, 16000), 16000, gender)
        ax2 = axes[2 * k + 1]
        ax2.plot(np.arange(len(f0)) * 0.005 + 0.02, np.where(voiced, f0, np.nan), ".", ms=2)
        ax2.set_ylim((50, 220) if gender == "male" else (110, 350))
        ax2.set_ylabel("F0 Hz")
        ax2.grid(alpha=0.3)
        for a in (ax, ax2):
            a.set_xlim(t0, t1 if t1 > 0 else len(x) / sr)
    plt.tight_layout()
    plt.savefig(args.out, dpi=70)
    print("wrote", args.out)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("fetch")
    s = sub.add_parser("score")
    s.add_argument("--tag", default="current")
    s.add_argument("--against", default="")
    s.add_argument("--voices", default="male,female")
    s.add_argument("--set", default="all", choices=["all", "arctic", "harvard", "heldout"])
    s.add_argument("--existing", action="store_true", help="score WAVs already in tools/.cache/synth/<tag>/")
    s.add_argument("--say", default="", help="extra options passed to the say example, e.g. '--expressiveness 1.4'")
    s.add_argument("--fast", action="store_true", help="skip the neural models (signal statistics only)")
    s.add_argument("--verbose", action="store_true")
    h = sub.add_parser("human")
    h.add_argument("--fast", action="store_true")
    pr = sub.add_parser("probe")
    pr.add_argument("text", help="one or more texts separated by |")
    pr.add_argument("--voices", default="male,female")
    pr.add_argument("--say", default="")
    sw = sub.add_parser("sweep")
    sw.add_argument("configs", help="'name:tune;name:tune' e.g. 'base:;hf:hf=5,muc_hz=0'")
    sw.add_argument("--voices", default="male,female")
    sw.add_argument("--confusions", action="store_true")
    sw.add_argument("--small", action="store_true", help="only the 46 core sentences (faster, noisier)")
    lv = sub.add_parser("levels", help="average level of each phone relative to vowels")
    lv.add_argument("--voices", default="male")
    lv.add_argument("--tune", default="")
    lv.add_argument("--say", default="", help="extra options for the say example, e.g. --say=--folds")
    sp = sub.add_parser("spectro")
    sp.add_argument("--sentence", type=int, default=1)
    sp.add_argument("--set", default="arctic", choices=["arctic", "harvard"])
    sp.add_argument("--text", default="")
    sp.add_argument("--voice", default="male")
    sp.add_argument("--tags", default="current")
    sp.add_argument("--out", default=str(CACHE / "spectro.png"))
    sp.add_argument("--fmax", type=float, default=8000)
    sp.add_argument("--t0", type=float, default=0.0)
    sp.add_argument("--t1", type=float, default=0.0)
    sp.add_argument("--no-human", action="store_true")
    for q in (s, pr, sp):
        q.add_argument("--tune", default="", help="VS_TUNE overrides, e.g. 'hf=4,rad=0'")
    s.add_argument("--quick", action="store_true", help="only UTMOS + phoneme errors")
    args = p.parse_args()
    global TUNE, QUICK
    TUNE = getattr(args, "tune", "")
    QUICK = getattr(args, "quick", False)
    {"fetch": lambda a: fetch(), "score": cmd_score, "human": cmd_human, "spectro": cmd_spectro, "probe": cmd_probe, "sweep": cmd_sweep, "levels": cmd_levels}[args.cmd](args)


if __name__ == "__main__":
    main()
