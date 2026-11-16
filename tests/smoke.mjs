// Smoke test of the built self-contained WebAssembly package (`pkg/tts.wasm` + `pkg/tts.js`).
//
//   npm run build
//   node tests/smoke.mjs
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const wasmBytes = fs.readFileSync(path.join(root, "pkg", "tts.wasm"));

// --- 1. Pure single-file `tts.wasm` test (ZERO `.js` files, ZERO external imports!) -------
const rawMod = new WebAssembly.Module(wasmBytes);
assert.equal(WebAssembly.Module.imports(rawMod).length, 0, "tts.wasm has zero external imports");
const rawInst = new WebAssembly.Instance(rawMod, {});
const w = rawInst.exports;
const internalGlue = new TextDecoder().decode(
  new Uint8Array(w.memory.buffer, w.tts_glue_ptr(), w.tts_glue_len())
);
const { TTS: StandaloneTTS, encodeWav: standaloneWav } = Function("wasm", internalGlue)(w);
const standalone = new StandaloneTTS();
const standaloneSpeech = standalone.speak("Hello from standalone WebAssembly!");
assert.equal(standaloneSpeech.sampleRate, 24000);
assert.ok(standaloneSpeech.samples.length > 12000);
assert.equal(String.fromCharCode(...standaloneWav(standaloneSpeech.samples.slice(0, 64)).slice(0, 4)), "RIFF");
standalone.free();

// Also verify raw C-ABI directly on `tts.wasm` without even running the JS glue:
const rawHandle = w.tts_new(0, 0);
assert.ok(rawHandle >= 1);
const msgBytes = new TextEncoder().encode("Direct C ABI.");
const inPtr = w.tts_alloc(msgBytes.length);
new Uint8Array(w.memory.buffer, inPtr, msgBytes.length).set(msgBytes);
assert.equal(w.tts_speak_wav(rawHandle, inPtr, msgBytes.length), 0);
const wavView = new Uint8Array(w.memory.buffer, w.tts_bytes_ptr(), w.tts_bytes_len());
assert.equal(String.fromCharCode(...wavView.slice(0, 4)), "RIFF");
w.tts_drop(rawHandle);

// --- 2. ES module loader (`pkg/tts.js`) ---------------------------------------------------
const { initSync, TTS, encodeWav } = await import(pathToFileURL(path.join(root, "pkg", "tts.js")).href);
initSync(wasmBytes);

const TEXT = "Hello! It is 3.5 degrees outside, isn't it?";
const peak = (x) => x.reduce((m, v) => Math.max(m, Math.abs(v)), 0);
const medianPitch = (speech) => {
  const x = speech.samples;
  const sr = speech.sampleRate;
  const win = Math.round(sr * 0.05);
  let best = 0, bestE = 0;
  for (let s = 0; s + 2 * win < x.length; s += win) {
    let e = 0;
    for (let i = 0; i < win; i++) e += x[s + i] ** 2;
    if (e > bestE) { bestE = e; best = s; }
  }
  let lagBest = 0, cBest = -Infinity;
  for (let lag = Math.floor(sr / 400); lag < Math.floor(sr / 60); lag++) {
    let c = 0;
    for (let i = 0; i < win; i++) c += x[best + i] * x[best + i + lag];
    if (c > cBest) { cBest = c; lagBest = lag; }
  }
  return sr / lagBest;
};

// --- voices -------------------------------------------------------------------------------
const male = new TTS();
const female = new TTS("female");
assert.equal(male.gender, "male");
assert.equal(female.settings.gender, "female");
assert.deepEqual(TTS.presets().map((p) => p.id), ["male", "deep", "husky", "female", "bright", "whisper"]);
assert.throws(() => new TTS("no_such_voice"), /unknown voice/);
assert.deepEqual(new TTS(TTS.presets()[3]).settings, female.settings);
assert.deepEqual(new TTS("clear_alto").settings, female.settings, "earlier preset ids still work");
assert.throws(() => new TTS({ gender: "robot" }), /gender/);

// --- one-shot synthesis -------------------------------------------------------------------
const m = male.speak(TEXT);
const f = female.speak(TEXT);
for (const speech of [m, f]) {
  assert.equal(speech.sampleRate, 24000);
  assert.ok(speech.samples instanceof Float32Array && speech.samples.length > 24000);
  assert.ok(Math.abs(speech.duration - speech.samples.length / 24000) < 1e-9);
  assert.ok(speech.samples.every((v) => Number.isFinite(v) && Math.abs(v) <= 1));
  assert.ok(peak(speech.samples) > 0.5);
  assert.equal(speech.text, "Hello! It is three point five degrees outside, isn't it?");
  assert.deepEqual(speech.words.map((w) => w.word), ["hello", "it", "is", "three", "point", "five", "degrees", "outside", "isn't", "it"]);
  assert.ok(speech.words.every((w, i, a) => w.start < w.end && (i === 0 || a[i - 1].end <= w.start + 1e-9)));
  assert.equal(speech.visemes.length % speech.visemeFields.length, 0);
  assert.equal(speech.visemes.length / speech.visemeFields.length, Math.round(speech.duration * speech.visemeRate));
  assert.equal(speech.acoustics.length / speech.acousticFields.length, speech.visemes.length / speech.visemeFields.length);
  assert.ok(Array.isArray(speech.phones) && speech.phones.length > speech.words.length);
  assert.ok(speech.phones.every((p, i, a) => p.start < p.end && p.ipa.length > 0 && (i === 0 || a[i - 1].end <= p.start + 1e-9)));
}
const [pm, pf] = [medianPitch(m), medianPitch(f)];
assert.ok(pm > 75 && pm < 210, `male pitch ${pm}`);
assert.ok(pf > 140 && pf < 420, `female pitch ${pf}`);
assert.deepEqual(Array.from(male.speak(TEXT).samples.slice(0, 5000)), Array.from(m.samples.slice(0, 5000)), "deterministic");
assert.equal(male.speak("").samples.length, 0);

// --- configuration ------------------------------------------------------------------------
male.configure({ speed: 1.5 });
assert.ok(male.speak(TEXT).duration < m.duration * 0.8);
male.configure("deep");
assert.equal(male.settings.pitchHz, 86);
const before = JSON.stringify(male.settings);
assert.throws(() => male.configure({ speed: 1.2, pitchHZ: 120 }), /unknown setting "pitchHZ"/);
assert.throws(() => male.configure({ speed: "1.2" }), /speed must be a finite number/);
assert.throws(() => male.configure({ speed: NaN }), /speed must be a finite number/);
assert.throws(() => male.configure({ preset: "female", gender: "x" }), /gender/);
assert.throws(() => male.configure({ pitchHz: 200, vowels: { XX: [1, 1, 1] } }), /vowel/);
assert.equal(JSON.stringify(male.settings), before);
male.configure({ speed: undefined, pitchHz: 86 });
assert.equal(JSON.stringify(male.settings), before);
male.setGender("female");
assert.equal(male.gender, "female");
male.configure({ pitchHz: 99999 });
assert.equal(male.settings.pitchHz, 350);

// --- natural variation: seeded, repeatable, re-rollable -----------------------------------
const takes = new TTS({ variation: 0.5, seed: 1 });
const take1 = takes.speak(TEXT);
assert.deepEqual(Array.from(takes.speak(TEXT).samples.slice(0, 8000)), Array.from(take1.samples.slice(0, 8000)));
takes.configure({ seed: 2 });
assert.equal(takes.settings.seed, 2);
const take2 = takes.speak(TEXT);
assert.notDeepEqual(Array.from(take2.samples.slice(0, 8000)), Array.from(take1.samples.slice(0, 8000)));
assert.ok(Math.abs(take2.duration / take1.duration - 1) < 0.2);
takes.configure({ variation: 0 });
const flat1 = takes.speak(TEXT);
takes.configure({ seed: 99 });
assert.deepEqual(Array.from(takes.speak(TEXT).samples.slice(0, 8000)), Array.from(flat1.samples.slice(0, 8000)));
takes.configure("deep_bass");
assert.equal(takes.settings.pitchHz, 86);
assert.equal(takes.settings.variation, 0, "a preset keeps the chosen variation");
takes.free();

// --- tone controls and voice profiles -----------------------------------------------------
assert.deepEqual(Array.from(TTS.toneBandsHz()), [125, 250, 500, 1000, 1500, 2200, 3000, 4000, 5500, 8000]);
const tonal = new TTS("male");
assert.deepEqual(tonal.settings.tone, new Array(10).fill(0));
const plain = tonal.speak(TEXT);
const treble = (x) => {
  let d = 0, e = 0;
  for (let i = 1; i < x.length; i++) { d += (x[i] - x[i - 1]) ** 2; e += x[i] ** 2; }
  return d / e;
};
tonal.configure({ presence: 9, brightness: 6 });
const bright = tonal.speak(TEXT);
assert.equal(bright.samples.length, plain.samples.length, "tone does not change timing");
assert.ok(treble(bright.samples) > treble(plain.samples) * 1.5, "presence and brightness add treble");
tonal.configure({ presence: 0, brightness: 0, bass: -40, tone: [0, 0, 0, 0, 0, 0, 99, 0, 0, 0] });
assert.equal(tonal.settings.bass, -12, "tone controls are clamped");
assert.equal(tonal.settings.tone[6], 18);
const copied = new TTS({ gender: "female", pitchHz: 180, tractLengthCm: 14.7, speed: 1.2, expressiveness: 0.8, tone: [-4, 2, 2, -1, 2, -4, -8, 3, 6, 6] });
assert.equal(copied.settings.tone[6], -8);
const copiedSpeech = copied.speak(TEXT);
assert.ok(copiedSpeech.samples.every((v) => Number.isFinite(v) && Math.abs(v) <= 1) && peak(copiedSpeech.samples) > 0.3);
tonal.free();
copied.free();

// --- formant shifts, speaker vowels, pauses, breaths & simulated folds --------------------
assert.deepEqual(TTS.vowelNames().slice(0, 3), ["IY", "IH", "EH"]);
const plainVoice = new TTS("male");
assert.deepEqual(plainVoice.settings.vowels, {});
const shaped = new TTS({ formants: [1.05, 0.95, 1, 1.1, 1.1], vowels: { uw: [1, 1.2, 1] }, pauses: 2, breaths: 0.8 });
assert.deepEqual(shaped.settings.formants, [1.05, 0.95, 1, 1.1, 1.1]);
assert.deepEqual(shaped.settings.vowels, { UW: [1, 1.2, 1] });
assert.equal(shaped.settings.pauses, 2);
assert.equal(shaped.settings.breaths, 0.8);
assert.throws(() => shaped.configure({ vowels: { XX: [1, 1, 1] } }), /vowel/);
const twoSentences = "One moment. Here is the next sentence.";
assert.ok(shaped.speak(twoSentences).duration > plainVoice.speak(twoSentences).duration + 0.4);
const restored = new TTS(shaped.settings);
assert.deepEqual(restored.settings, shaped.settings);
assert.equal(plainVoice.settings.openQuotient, 0.66);
assert.equal(plainVoice.settings.folds, false);

const folded = new TTS({ gender: "female", folds: true });
const foldedSpeech = folded.speak(TEXT);
assert.ok(foldedSpeech.samples.every(Number.isFinite));
assert.ok(Math.abs(foldedSpeech.duration - new TTS("female").speak(TEXT).duration) < 0.01);
folded.configure("deep");
assert.equal(folded.settings.folds, true);
assert.throws(() => folded.configure({ folds: "yes" }), /folds must be true or false/);
folded.free();
plainVoice.free();
shaped.free();
restored.free();

// --- streaming equals one-shot loudness and length ----------------------------------------
const stream = female.stream(TEXT);
const chunks = [];
while (!stream.done) {
  const c = stream.read(777);
  if (!c.length) break;
  chunks.push(c);
}
const total = chunks.reduce((n, c) => n + c.length, 0);
assert.ok(Math.abs(total / 24000 - stream.duration) < 1e-6);
assert.ok(Math.abs(total - f.samples.length) <= 240);
const streamed = new Float32Array(total);
chunks.reduce((off, c) => (streamed.set(c, off), off + c.length), 0);
assert.ok(Math.abs(peak(streamed) - peak(f.samples)) < 0.02);
stream.free();

// --- pronunciation tools and WAV ----------------------------------------------------------
assert.equal(female.arpabet("hello"), "HH EH0 L OW1");
female.addWord("hello", "HH AX L OW1");
assert.equal(female.arpabet("hello"), "HH AX0 L OW1");
assert.throws(() => female.addWord("x", "NOPE"));
assert.equal(female.loadDictionary("zzyzx Z AY1 Z IH0 K S\n"), 1);
assert.equal(female.arpabet("zzyzx"), "Z AY1 Z IH0 K S");
assert.ok(female.phonemize("thank you").startsWith("/"));
const wav = female.speakWav("Hi.");
assert.equal(String.fromCharCode(...wav.slice(0, 4)), "RIFF");
assert.equal(encodeWav(new Float32Array(10)).length, 44 + 20);

// --- random voice creation (partial constraints) & reusable named voices ------------------
const randFem = TTS.randomVoice({ gender: "female", speed: 1.24, seed: 42 });
assert.equal(randFem.gender, "female");
assert.equal(randFem.speed, 1.24);
assert.deepEqual(TTS.randomVoice({ gender: "female", speed: 1.24, seed: 42 }), randFem, "seeded randomVoice is deterministic");

const cast = new TTS();
const aliceProfile = cast.randomize({ name: "alice", gender: "female", warmth: 0.74, seed: 101 });
const bobProfile = cast.randomize({ name: "bob", gender: "male", pitchHz: 91, seed: 202 });
assert.equal(aliceProfile.gender, "female");
assert.equal(aliceProfile.warmth, 0.74);
assert.equal(bobProfile.gender, "male");
assert.equal(bobProfile.pitchHz, 91);
assert.ok("alice" in cast.voices() && "bob" in cast.voices());

// Reuse [alice] and [bob] across dialogue AND singing!
const randomScene = cast.speakScript(
  "[alice, excited] Hi Bob, my voice was generated dynamically! [bob, calm] And I can keep my exact voice in every scene."
);
assert.equal(randomScene.segments.length, 2);
assert.equal(randomScene.segments[0].gender, "female");
assert.equal(randomScene.segments[0].voice, "alice");
assert.equal(randomScene.segments[1].gender, "male");
assert.equal(randomScene.segments[1].voice, "bob");

// Configured voice on `cast` (currently `bob`) is used automatically when singing without overriding `voice`!
const bobSings = cast.sing("C3:1 E3:1 G3:2", "Sing my song", 112);
assert.equal(bobSings.segments[0].gender, "male");
assert.equal(bobSings.segments[0].tractLengthCm, bobProfile.tractLengthCm);
cast.free();

// --- scripted multi-voice dialogue & singing / chorus WASM API ----------------------------
assert.ok(Math.abs(TTS.noteToHz("A4") - 440) < 0.01);
assert.equal(TTS.noteToHz("R"), 0);
assert.ok(TTS.voiceIds().includes("soprano") && TTS.voiceIds().includes("woman_a"));
assert.ok(TTS.styleNames().includes("excited"));

const dialogue = female.speakScript(
  "[male, speed=0.95] Hello there! [pause=180ms] [female, excited, speed=1.18] Hi! Can we sing too? [soprano, sing bpm=120] C5:1=Yes G4:1.5=we C5:2=can! [/sing]"
);
assert.equal(dialogue.segments.length, 3);
assert.equal(dialogue.segments[0].gender, "male");
assert.equal(dialogue.segments[1].gender, "female");
assert.equal(dialogue.segments[2].singing, true);
assert.ok(dialogue.samples.length > 24000 * 2 && peak(dialogue.samples) > 0.5);

const cueDialogue = female.speakScript([
  { voice: "woman_a", style: "happy", text: "Structured JSON cues work out of the box." },
  { pauseMs: 150 },
  { voice: "man_b", warmth: 0.72, text: "Ideal for AI agents on any web page." },
]);
assert.equal(cueDialogue.segments.length, 2);
assert.equal(String.fromCharCode(...female.speakScriptWav("[female] Quick test.").slice(0, 4)), "RIFF");

const soloSong = female.sing("C4:1 C4:1 G4:1 G4:1", "Twinkle twinkle", 120);
assert.equal(soloSong.words.length, 4);
assert.ok(soloSong.duration > 1.8 && peak(soloSong.samples) > 0.5);

const objSong = female.sing({
  bpm: 116,
  voice: "soprano",
  notes: [
    { note: "E5", beats: 1, lyric: "Shine" },
    { rest: 0.5 },
    { note: "C5", beats: 2, lyric: "bright" },
  ],
});
assert.equal(objSong.words.length, 2);
assert.equal(String.fromCharCode(...female.singWav("C4:1=La G4:1=la").slice(0, 4)), "RIFF");

const choir = female.singChorus({
  bpm: 108,
  parts: [
    { name: "Soprano", voice: "soprano", notes: "E5:1 D5:1 C5:2", lyrics: "Sing in tune" },
    { name: "Alto", voice: "alto", notes: "C5:1 B4:1 G4:2", lyrics: "Sing in tune" },
    { name: "Tenor", voice: "tenor", notes: "G4:1 F4:1 E4:2", lyrics: "Sing in tune" },
    { name: "Bass", voice: "deep", notes: "C3:1 G3:1 C3:2", lyrics: "Sing in tune" },
  ],
});
assert.equal(choir.words.length, 3);
assert.ok(choir.segments[0].singing && peak(choir.samples) > 0.5);

const dslChoir = female.singChorus(`[bpm=102 lyrics="Twinkle twinkle"]
[part name="Soprano" voice=soprano]
C5:1 C5:1 G5:1 G5:1  // inline comments & shared header lyrics work cleanly!
[part name="Bass" voice=deep]
C3:1 C3:1 E3:1 C3:1  # hash comment too`);
assert.equal(dslChoir.words.length, 4);
assert.ok(peak(dslChoir.samples) > 0.5);

const bareLyricsScore = female.singChorus(`bpm: 108
lyrics: Shine on bright star
C5:1 D5:1 E5:1 C5:1`);
assert.equal(bareLyricsScore.words.length, 4);
assert.ok(peak(bareLyricsScore.samples) > 0.5);

const maleBase = new TTS("male");
const womanArc = maleBase.speakScript(
  "[woman_a, calm, speed=0.86] We can start slowly, warmly, and gently, taking our time on every word. [inhale] [excited, speed=1.22, pitch=+2st] Then suddenly speed right up with bright energy when something thrilling happens! [pause=280ms] [whisper, slow] Or drop to an intimate whisper... [woman_a, reset] before returning smoothly to normal."
);
assert.equal(womanArc.segments.length, 4);
assert.ok(womanArc.segments.every((s) => s.gender === "female" && s.voice === "woman_a"));
assert.equal(womanArc.segments[3].pitchHz, 186);
maleBase.free();

console.log(`ok: self-contained tts.wasm works (male ${pm.toFixed(0)} Hz, female ${pf.toFixed(0)} Hz, ${m.duration.toFixed(1)} s speech, dialogue ${dialogue.duration.toFixed(1)} s, choir ${choir.duration.toFixed(1)} s)`);
