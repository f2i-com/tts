"""Do the sounds that are not words come across as what they are meant to be? (development tooling)

`src/vocal.rs` makes laughter, sighs, coughs, hums and whistles with the same vocal tract that
speaks. Nobody can tune fifty sounds by one pair of ears, so this script asks a tagger trained
on two million clips of real sound (the Audio Spectrogram Transformer on AudioSet, 527 classes)
what it hears in each of them:

  check   every sound, both voices: whether the classes it should evoke are among the top
          three the tagger hears, what it hears instead, and the level against a spoken sentence
  sweep   development hooks (VS_TUNE) of one sound against the share of what the tagger hears
          that belongs to the family asked for - how the numbers of the laugh were set

Usage (from the crate root, after `cargo build --release --example say --features dev-hooks`):

  python tools/sounds.py check
  python tools/sounds.py check --only laugh,giggle,cough --voices female
  python tools/sounds.py sweep --sound laugh --configs "now:;softer:lv_attack=40;airy:lv_air=0.13"
  python tools/sounds.py sweep --sound cough --family "Cough|Throat clearing" --configs "now:;more voice:cough_voice=2"

The tagger is downloaded on first use into the Hugging Face cache (350 MB). It has never heard
a synthesizer, and for a sound made by one its confidence is low: read the ranks and the
changes, not the probabilities. Several sounds it does not place at all (see `check`).
"""
from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from math import gcd
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import eval as E  # noqa: E402

OUT = E.CACHE / "sounds"
REFERENCE = "We are all here now, you know."
LAUGHTER = ["Laughter", "Giggle", "Chuckle, chortle", "Snicker", "Belly laugh", "Baby laughter"]

# What the tagger should hear: any of these among its top three counts. (Sounds with no
# class of their own in AudioSet - "uh-huh", "aww", a kiss - are listed without a verdict.)
EXPECT = {
    "laugh": LAUGHTER, "chuckle": LAUGHTER, "giggle": LAUGHTER, "guffaw": LAUGHTER, "cackle": LAUGHTER,
    "snicker": LAUGHTER + ["Snort"], "hoho": LAUGHTER, "hehe": LAUGHTER, "nervous": LAUGHTER, "evil": LAUGHTER,
    "inhale": ["Breathing", "Gasp"], "exhale": ["Breathing", "Sigh"], "gasp": ["Gasp", "Breathing"], "sigh": ["Sigh", "Breathing"],
    "yawn": ["Sigh", "Breathing", "Groan"], "sniff": ["Sniff"], "pant": ["Pant", "Breathing"], "phew": ["Sigh", "Breathing"],
    "hum": ["Humming"], "groan": ["Groan", "Grunt", "Wail, moan"], "scream": ["Screaming", "Shout", "Yell", "Baby cry, infant cry"],
    "sob": ["Crying, sobbing", "Whimper"], "whimper": ["Whimper", "Crying, sobbing"],
    "shh": ["Whispering", "Hiss"], "tsk": ["Tick", "Clicking", "Finger snapping"], "grr": ["Growling", "Grunt", "Groan", "Snoring"],
    "raspberry": ["Fart", "Burping, eructation", "Snort"], "whistle": ["Whistling", "Whistle"], "fweet": ["Whistling", "Whistle"],
    "cough": ["Cough", "Throat clearing"], "ahem": ["Throat clearing", "Cough"], "hiccup": ["Hiccup"], "sneeze": ["Sneeze"], "snore": ["Snoring"],
}

_tagger = None


def tagger():
    global _tagger
    if _tagger is None:
        import torch
        from transformers import ASTFeatureExtractor, ASTForAudioClassification

        name = "MIT/ast-finetuned-audioset-10-10-0.4593"
        model = ASTForAudioClassification.from_pretrained(name).eval()
        if torch.cuda.is_available():
            model = model.cuda()
        _tagger = (ASTFeatureExtractor.from_pretrained(name), model, model.config.id2label)
    return _tagger


def tags(x: np.ndarray, sr: int) -> list[tuple[str, float]]:
    """(class, probability) of everything the tagger hears, most likely first."""
    import torch
    from scipy.signal import resample_poly

    fe, model, id2label = tagger()
    if sr != 16000:
        g = gcd(sr, 16000)
        x = resample_poly(x, 16000 // g, sr // g)
    x = x.astype(np.float32)
    x = x / (np.abs(x).max() + 1e-9) * 0.7
    inputs = fe(x, sampling_rate=16000, return_tensors="pt")
    with torch.no_grad():
        p = torch.sigmoid(model(**{k: v.to(model.device) for k, v in inputs.items()}).logits[0]).cpu().numpy()
    return [(id2label[int(i)], float(p[i])) for i in np.argsort(-p)]


def sound_ids() -> list[str]:
    return re.findall(r'sound!\("([a-z-]+)"', (E.ROOT / "src" / "vocal.rs").read_text(encoding="utf-8"))


def say(voice: str, script: str, wav: Path, tune: str = "") -> tuple[np.ndarray, int]:
    import soundfile as sf

    env = dict(os.environ)
    if tune:
        env["VS_TUNE"] = tune
    subprocess.run([str(E.SAY), "--voice", voice, "--script", script, str(wav)], check=True, capture_output=True, env=env)
    return sf.read(str(wav))


def active_rms(x: np.ndarray) -> float:
    e = np.sqrt(np.convolve(x**2, np.ones(480) / 480, mode="same"))
    loud = e > e.max() * 0.1
    return float(np.sqrt(np.mean(x[loud] ** 2))) if loud.any() else 0.0


def cmd_check(args) -> None:
    import soundfile as sf

    OUT.mkdir(parents=True, exist_ok=True)
    ids = [i for i in sound_ids() if not args.only or i in args.only.split(",")]
    placed = judged = 0
    for voice in args.voices.split(","):
        ref, sr = say(voice, REFERENCE, OUT / "ref.wav")
        print(f"===== {voice}: length, level against a spoken sentence, and what the tagger hears")
        for sid in ids:
            # after a sentence, so that both are brought to one level together as in a script
            x, sr = say(voice, f"{REFERENCE} [pause=300ms] [{sid} {args.args}]", OUT / "both.wav")
            snd = x[len(ref) + int(0.25 * sr):]
            sf.write(str(OUT / f"{voice}_{sid}.wav"), snd, sr)
            db = 20 * np.log10(active_rms(snd) / active_rms(x[: len(ref)]) + 1e-9)
            heard = tags(snd, sr)
            names = [n for n, _ in heard]
            line = f"  {sid:10s} {len(snd) / sr:4.2f} s {db:+4.0f} dB  "
            want = EXPECT.get(sid)
            if want:
                rank, best = min((names.index(w), w) for w in want if w in names)
                judged += 1
                placed += rank < 3
                line += f"{'ok' if rank < 3 else '--'}  {best} is #{rank + 1}".ljust(34)
            else:
                line += " " * 34
            print(line + "hears: " + ", ".join(f"{n} {p:.2f}" for n, p in heard[: args.top]), flush=True)
    if judged:
        print(f"placed among the top three: {placed} of {judged}")


def cmd_sweep(args) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    family = [x.strip() for x in args.family.split("|")] if args.family else LAUGHTER
    voices, seeds = args.voices.split(","), args.seeds.split(",")
    print(f"share of what the tagger hears that is {' / '.join(family)}")
    print(f"{'setting':30s} " + " ".join(f"{v[:1]}{s}".rjust(6) for v in voices for s in seeds) + "    mean   (what else)")
    for cfg in args.configs.split(";"):
        name, tune = (cfg.split(":", 1) + [""])[:2]
        shares, other = [], {}
        for voice in voices:
            for seed in seeds:
                x, sr = say(voice, f"[seed={seed}] [{args.sound} {args.args}]", OUT / "sweep.wav", tune)
                heard = tags(x, sr)
                p = dict(heard)
                shares.append(sum(p.get(f, 0.0) for f in family))
                for n, pr in heard[:3]:
                    if n not in family:
                        other[n] = other.get(n, 0.0) + pr / (len(voices) * len(seeds))
        rest = ", ".join(f"{n} {pr:.2f}" for n, pr in sorted(other.items(), key=lambda kv: -kv[1])[:3])
        print(f"{name:30s} " + " ".join(f"{v:6.2f}" for v in shares) + f"   {np.mean(shares):.3f}   {rest}", flush=True)


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("check")
    c.add_argument("--only", default="", help="comma-separated sound ids")
    c.add_argument("--top", type=int, default=4)
    s = sub.add_parser("sweep")
    s.add_argument("--sound", default="laugh")
    s.add_argument("--family", default="", help="classes that count, separated by | (default: the laughter classes)")
    s.add_argument("--configs", default="now:", help="'name:tune;name:tune', e.g. 'now:;softer:lv_attack=40'")
    s.add_argument("--seeds", default="0,1")
    for q in (c, s):
        q.add_argument("--voices", default="male,female")
        q.add_argument("--args", default="", help="what follows the sound in its tag, e.g. 'intensity=0.9'")
    args = p.parse_args()
    {"check": cmd_check, "sweep": cmd_sweep}[args.cmd](args)


if __name__ == "__main__":
    main()
