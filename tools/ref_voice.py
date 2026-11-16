"""A reference voice from a speech engine on this computer (development tooling).

The CMU Arctic recordings used everywhere else are 16 kHz, read flatly, and all statements.
A good local text-to-speech engine with an OpenAI-style `/v1/audio/speech` endpoint (the
OAIY studio, for one) can read *any* sentence in one consistent voice at 24 kHz, which makes
it a second kind of reference: for the tone of a voice up to 12 kHz, and for how questions,
exclamations, lists and long sentences are spoken. Nothing recorded is shipped or committed;
only measurements are used.

  python tools/ref_voice.py fetch --voice receptionist            # record the reference set
  python tools/ref_voice.py split --name receptionist             # cut the paragraphs into sentences
  python tools/ref_voice.py fetch --name man --describe "A warm, clear man in his thirties ..."
  python tools/ref_voice.py list
"""
from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.request
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402

ROOT = E.CACHE / "refvoice"
URL = "http://127.0.0.1:8080"

# How people actually use a voice: the sentence types the read corpus lacks.
STYLE = [
    "Did you see the new bridge they built last year?",
    "Is it still raining outside?",
    "Can you help me carry these boxes upstairs?",
    "Are they coming to dinner on Friday?",
    "Have you ever been to the mountains in winter?",
    "Was that the last train to the city?",
    "Where did you put the keys to the car?",
    "What time does the meeting start tomorrow?",
    "Why would anyone leave such a beautiful place?",
    "How long have you been waiting here?",
    "Who told you about the surprise party?",
    "Which of these two colours do you like better?",
    "That is absolutely wonderful news!",
    "Watch out for the step!",
    "I can't believe we actually won!",
    "What a lovely day it has been!",
    "We need eggs, flour, butter, sugar, and a little milk.",
    "She packed a hat, two scarves, warm gloves, and her old boots.",
    "The train stops at Ashfield, Burwood, Strathfield, and Parramatta.",
    "First open the box, then read the note, and finally call me.",
    "You can pay by card, by cash, or by bank transfer.",
    "When the rain finally stopped, the children ran outside to play in the puddles that had formed along the road.",
    "Although he had studied all night, he still felt unsure about the questions that might come up in the morning.",
    "The woman who lives next door told me that her garden has never looked better than it does this spring.",
    "If you follow the river for about a mile, you will come to an old stone bridge with a small cafe beside it.",
    "Before we begin, I would like to thank everyone who travelled so far to be with us today.",
    "The report, which was finished only yesterday, shows that sales have grown steadily for three years.",
    "The meeting is at half past ten on the third of May.",
    "It costs about twenty five dollars, give or take a little.",
    "There were one hundred and forty people in the room.",
    "Call me back on Monday, after nine in the morning.",
    "Yes, of course.",
    "No, not really.",
    "Thank you very much.",
    "I'm not sure about that.",
    "See you tomorrow, then.",
    "He said, quite calmly, that he would rather walk home.",
    "My brother, the one who lives in Perth, is visiting next week.",
    "Well, I suppose we could try again later.",
    "Honestly, it was the best meal I have had in years.",
]


def corpus() -> dict[str, str]:
    """id -> text: the Arctic prompts used by the other tools, the Harvard and held-out
    sentences of eval.py, and the style set above."""
    from fit_durations import prompts

    out = {}
    arctic = prompts()
    for i in range(1, 117):
        out[f"a{i:04d}"] = arctic[i]
    for i, t in enumerate(E.HARVARD, 1):
        out[f"h{i:02d}"] = t
    for i, t in enumerate(E.HELDOUT, 1):
        out[f"x{i:02d}"] = t
    for i, t in enumerate(STYLE, 1):
        out[f"s{i:02d}"] = t
    out["sample"] = "Hello! This voice is computed entirely from a model of the human vocal tract. It can read numbers like 2026, ask questions, and sound warm or bright. Isn't that interesting?"
    return out


# (no proxy for a local address: looking one up can cost most of a minute per request)
OPENER = urllib.request.build_opener(urllib.request.ProxyHandler({}))


def post(path: str, body: dict, timeout: float = 300.0) -> bytes:
    req = urllib.request.Request(URL + path, data=json.dumps(body).encode("utf-8"), headers={"Content-Type": "application/json"})
    with OPENER.open(req, timeout=timeout) as res:
        return res.read()


def fetch(args) -> None:
    name = args.name or args.voice
    out = ROOT / name
    out.mkdir(parents=True, exist_ok=True)
    voice: object = args.voice
    if args.describe:
        # a described voice, designed once and handed back with every request; nothing is
        # saved in the speech engine
        keep = out / "voice.json"
        if not keep.exists():
            reply = json.loads(post("/v1/audio/voices", {"name": name, "description": args.describe, "keep": False, "seed": 7}))
            keep.write_text(json.dumps(reply["voice"]), encoding="utf-8")
        voice = json.loads(keep.read_text(encoding="utf-8"))
    texts = corpus()
    (out / "texts.json").write_text(json.dumps(texts, indent=1), encoding="utf-8")
    todo = [k for k in texts if not (out / f"{k}.wav").exists()]
    if args.only:
        todo = [k for k in todo if k.startswith(args.only)]
    # Several sentences per request: the engine may have to load its model for each one.
    groups = []
    for k in todo:
        if groups and len(groups[-1]) < args.group and groups[-1][0][0] == k[0] and k != "sample":
            groups[-1].append(k)
        else:
            groups.append([k])
    pending = [g for g in groups if not (out / f"para_{g[0]}.wav").exists()]
    print(f"{len(texts) - len(todo)} of {len(texts)} sentences recorded; fetching {len(pending)} paragraphs", flush=True)
    started = time.time()
    for n, group in enumerate(pending, 1):
        body = {"input": " ".join(texts[k] for k in group), "voice": voice, "response_format": "wav", "seed": 1 + n}
        try:
            data = post("/v1/audio/speech", body)
        except Exception as e:  # the engine may be busy with its owner's work
            print(f"  {group[0]}: {e}; skipped for now", flush=True)
            time.sleep(20)
            continue
        (out / f"para_{group[0]}.wav").write_bytes(data)
        (out / f"para_{group[0]}.json").write_text(json.dumps(group), encoding="utf-8")
        print(f"  {n} of {len(pending)} ({time.time() - started:.0f} s)", flush=True)
    print("fetched; now run: ref_voice.py split --name", name)


def split(args) -> None:
    """Cuts the recorded paragraphs into their sentences. The sounds of each sentence are
    located by a phoneme recognizer; the cut goes into the longest silence between the last
    sound heard of one sentence and the first of the next."""
    from align import locate
    from transplant import say

    out = ROOT / args.name
    texts = json.loads((out / "texts.json").read_text(encoding="utf-8"))
    for meta in sorted(out.glob("para_*.json")):
        group = json.loads(meta.read_text())
        if all((out / f"{k}.wav").exists() for k in group):
            continue
        x, sr = E.load(meta.with_suffix(".wav"))
        x16 = E.resample(x, sr, 16000)
        phones, first = [], []
        for k in group:
            first.append(len(phones))
            for line in say(["--voice", args.gender, "--variation", "0", "--timing", "--phones", texts[k]]).splitlines():
                parts = line.split()
                if len(parts) == 4 and parts[0].isdigit():
                    phones.append(parts[1])
        found = locate(x16, phones)
        hop = sr // 200
        n = len(x) // hop
        level = 10 * np.log10(np.mean(x[: n * hop].reshape(n, hop) ** 2, axis=1) + 1e-12)
        quiet = level < level.max() - 38
        cuts = [0.0]
        ok = True
        for j in range(1, len(group)):
            before = [t for i, t in found.items() if i < first[j]]
            after = [t for i, t in found.items() if i >= first[j]]
            if not before or not after or max(before) >= min(after):
                ok = False
                break
            a, b = int(max(before) * 200), int(min(after) * 200)
            # the longest run of quiet frames between the two
            best, run_start, best_span = None, None, 0
            for f in range(a, min(b, n) + 1):
                if f < n and quiet[f]:
                    run_start = f if run_start is None else run_start
                    if f - run_start + 1 > best_span:
                        best_span, best = f - run_start + 1, (run_start + f) / 2
                else:
                    run_start = None
            cuts.append((best if best is not None else (a + b) / 2) / 200)
        if not ok:
            print(f"  {meta.stem}: could not find every sentence; left as a paragraph")
            continue
        cuts.append(len(x) / sr)
        for k, c0, c1 in zip(group, cuts[:-1], cuts[1:]):
            import soundfile as sf

            sf.write(str(out / f"{k}.wav"), x[int(c0 * sr) : int(c1 * sr)], sr, subtype="PCM_16")
        gaps = [f"{1000 * (c1 - c0):.0f}" for c0, c1 in zip(cuts[1:-1], cuts[2:])]
        print(f"  {meta.stem}: {len(group)} sentences")
    print(len([k for k in texts if (out / f"{k}.wav").exists()]), "of", len(texts), "sentences available")


def load(name: str, key: str, sr: int = 16000) -> np.ndarray:
    x, sr0 = E.load(ROOT / name / f"{key}.wav")
    return E.resample(x, sr0, sr).astype(np.float64) if sr != sr0 else x.astype(np.float64)


def texts(name: str) -> dict[str, str]:
    return json.loads((ROOT / name / "texts.json").read_text(encoding="utf-8"))


def spans(name: str, key: str, gender: str) -> list[tuple[str, float, float, int]]:
    """Approximate (phone, start s, end s, index among the synthesizer's sounds) for one
    recording, without labels: the sounds the
    synthesizer would say for this text are located in the recording by a phoneme recognizer
    (`align.locate`, about 15 ms), and the time between two located sounds is divided in
    proportion to the synthesizer's own durations. Sounds that were not heard are left out."""
    from align import locate
    from transplant import say

    cache = ROOT / name / f"{key}.{gender}.spans.json"
    if cache.exists():
        return [tuple(s) for s in json.loads(cache.read_text())]
    text = texts(name)[key]
    toks = []
    for line in say(["--voice", gender, "--variation", "0", "--timing", "--phones", text]).splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[0].isdigit():
            toks.append((parts[1], int(parts[2]) / 1000, int(parts[3]) / 1000))
    x = load(name, key)
    found = locate(x, [p for p, _, _ in toks])
    out = []
    idx = sorted(found)
    for n, k in enumerate(idx):
        ph, a, b = toks[k]
        dur = b - a
        centre = found[k]
        # neighbours that were located right next to this one share the gap by their lengths
        start = centre - dur / 2
        end = centre + dur / 2
        if n > 0 and idx[n - 1] == k - 1:
            pa, pb = toks[k - 1][1], toks[k - 1][2]
            gap = centre - found[k - 1]
            start = found[k - 1] + gap * (pb - pa) / ((pb - pa) + dur)
        if n + 1 < len(idx) and idx[n + 1] == k + 1:
            na, nb = toks[k + 1][1], toks[k + 1][2]
            gap = found[k + 1] - centre
            end = centre + gap * dur / (dur + (nb - na))
        out.append((ph, max(0.0, float(start)), float(end), k))
    cache.write_text(json.dumps(out))
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    f = sub.add_parser("fetch")
    f.add_argument("--voice", default="alloy", help="a saved voice or an OpenAI voice name")
    f.add_argument("--name", help="folder name for the recordings (default: the voice)")
    f.add_argument("--describe", help="design a voice from this description instead")
    f.add_argument("--only", help="only ids starting with this (a, h, x, s)")
    f.add_argument("--group", type=int, default=8, help="sentences per request")
    sp = sub.add_parser("split")
    sp.add_argument("--name", required=True)
    sp.add_argument("--gender", default="female", help="which synthesizer voice supplies the expected sounds")
    sub.add_parser("list")
    args = ap.parse_args()
    if args.cmd == "fetch":
        fetch(args)
    elif args.cmd == "split":
        split(args)
    else:
        for d in sorted(ROOT.glob("*")):
            print(d.name, len(list(d.glob("*.wav"))), "recordings")


if __name__ == "__main__":
    main()
