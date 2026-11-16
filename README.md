# TTS (`tts.computer`)

**TTS** ([tts.computer](https://tts.computer)) is a human voice and singing synthesizer written in pure Rust and compiled to a **single self-contained WebAssembly file (`tts.wasm`)**.

English text, multi-voice dialogue scripts, or musical notes & lyrics go in — 24 kHz audio comes out — computed entirely from a physical model of the human vocal tract. **Zero audio samples, zero neural networks, zero servers, and zero external dependencies.**

- **Single-file self-contained `tts.wasm`** (~1.4 MB raw, ~470 KB gzipped) — **0 external WASM imports**, with the complete JavaScript `TTS` SDK baked directly inside `tts.wasm` itself (grab it from [GitHub Releases](https://github.com/f2i-com/tts/releases))
- **~400–500× faster than real time** in the browser; outputs 24 kHz mono PCM or ready-to-save 16-bit WAV
- **Natural speech realism** (9% male / 8% female unseen phoneme error rate — matching human recordings — and 3.55 / 3.59 UTMOS naturalness)
- **Dynamic voice creation & reuse** (`TTS.randomVoice()`, `voice.randomize()`, `voice.registerVoice()`) — lock any parameters, randomize the rest within human anatomy, and reuse the voice across dialogue and song
- **Multi-voice scripted dialogue & singing** (`voice.speakScript()`, `voice.sing()`, `voice.singChorus()`) — inline speed, pitch, emotion styles, whispers, breaths, solo melodies, and 4-part harmonies
- **200 Hz lip-sync visemes, F0–F3 acoustics, and word/phoneme timings** included with every utterance

---

## Quick Start: Embed `tts.wasm` Anywhere

### Option 1: Single `.wasm` File Only (Zero `.js` Files!)

Download **`tts.wasm`** from [Releases](https://github.com/f2i-com/tts/releases) and drop it next to any HTML file. Because `tts.wasm` has **zero external imports** and carries its own JavaScript glue internally (`tts_glue_ptr` / `tts_glue_len`), 4 lines load the full `TTS` engine:

```html
<!doctype html>
<button id="say">Speak</button>

<script type="module">
  // Load ONLY tts.wasm — its complete JavaScript `TTS` class lives inside the WASM!
  async function loadTTS(url = "./tts.wasm") {
    const { instance: { exports: w } } = await WebAssembly.instantiateStreaming(fetch(url));
    return Function("wasm", new TextDecoder().decode(
      new Uint8Array(w.memory.buffer, w.tts_glue_ptr(), w.tts_glue_len())
    ))(w);
  }

  const { TTS } = await loadTTS("./tts.wasm");
  const voice = new TTS();

  document.getElementById("say").onclick = () => {
    voice.play("Hello! Everything you hear comes from a single WebAssembly file.");
  };
</script>
```

### Option 2: ES Module Loader (`tts.js` + `tts.wasm`)

If you use a bundler (Vite, Webpack, Rollup) or strict CSP (`script-src 'self'`), import `tts.js`:

```js
import init, { TTS, encodeWav } from "./tts.js";

await init(); // fetches ./tts.wasm (or pass a custom URL / Uint8Array)

const voice = new TTS(); // default male, or new TTS("female")
await voice.play("Hello! It is 3.5 degrees outside.");
```

### Option 3: Node.js / Bun / Deno

```js
import fs from "node:fs";
import { initSync, TTS } from "./pkg/tts.js";

initSync(fs.readFileSync("./pkg/tts.wasm"));

const voice = new TTS("female");
fs.writeFileSync("hello.wav", voice.speakWav("Hello from Node!"));
```

*(Or without `tts.js` in Node: `const { instance: { exports: w } } = await WebAssembly.instantiate(fs.readFileSync("./tts.wasm"));` and unpack `tts_glue_ptr` / `tts_glue_len` identically to Option 1.)*

---

## JavaScript & WebAssembly API

### 1. Basic Speech & Lip-Sync Data

```js
const voice = new TTS();                        // default male voice
const speech = voice.speak("Hello! It is $5.99 today.");

// Play in browser in 1 line (auto-manages AudioContext):
const handle = await voice.play("Hello there!"); // { speech, stop(), done }

// Or play `speech.samples` on your own Web Audio graph:
const ctx = new AudioContext();
const buf = ctx.createBuffer(1, speech.samples.length, speech.sampleRate);
buf.copyToChannel(speech.samples, 0);
const src = ctx.createBufferSource();
src.buffer = buf;
src.connect(ctx.destination);
src.start();
```

`voice.speak()` (as well as `speakScript`, `sing`, and `singChorus`) returns:

| Field | Type | Description |
|---|---|---|
| `samples` | `Float32Array` | Mono PCM audio in `-1..1` |
| `sampleRate` | `24000` | Output sample rate in Hz |
| `duration` | `number` | Length in seconds |
| `text` | `string` | Normalized spoken text (`"five dollars and ninety nine cents"`) |
| `ipa` | `string` | IPA transcription |
| `words` | `Array<{ word, start, end }>` | Word timestamps in seconds |
| `phones` | `Array<{ phone, ipa, start, end }>` | Phoneme timestamps in seconds |
| `segments` | `Array<{ start, end, voice, gender, pitchHz, tractLengthCm, singing }>` | Speaker/singing timeline (on `speakScript`, `sing`, `singChorus`) |
| `visemes` | `Float32Array` | 9 articulatory values per 5 ms frame (200 Hz): `[jaw, tongueHeight, tongueFront, tongueTip, lipRound, lipSpread, lipClose, velum, energy]` |
| `acoustics` | `Float32Array` | 6 acoustic values per 5 ms frame: `[f0, f1, f2, f3, voicing, noise]` |

Direct WAV helpers (return `Uint8Array` 16-bit 24 kHz `.wav` bytes):
- `voice.speakWav(text)`
- `voice.speakScriptWav(script)`
- `voice.singWav(score, lyrics?, bpm?)`
- `voice.singChorusWav(chorus)`
- `encodeWav(samples, sampleRate = 24000)`

---

### 2. Dynamic Random Voice Creation & Reuse

Create brand-new, anatomically coherent human voices on the fly — locking any parameters you care about while randomizing the rest — and reuse them across dialogue and singing:

```js
const voice = new TTS();

// A) Generate a reusable VoiceSettings JSON object (lock gender & speed, randomize the rest):
const heroProfile = TTS.randomVoice({ gender: "female", speed: 1.08 });
const villainProfile = TTS.randomVoice({ gender: "male", pitchHz: 88, roughness: 0.28, seed: 42 });

// Reuse the profile in a new TTS instance, configure(), or save to JSON / localStorage:
const heroVoice = new TTS(heroProfile);

// B) Randomize an instance in-place & register a reusable [tag] name in one step:
voice.randomize({ name: "alice", gender: "female", warmth: 0.72 });
voice.randomize({ name: "bob",   gender: "male",   pitchHz: 94 });

// Or register any saved/custom profile under a name:
voice.registerVoice("narrator", heroProfile);

// Now [alice], [bob], and [narrator] work everywhere — scripts, JSON cues, and choruses!
await voice.playScript(`
  [narrator, calm] Deep in the studio, two newly created voices met for the first time.
  [alice, excited] Hey Bob! My vocal tract was generated dynamically!
  [bob, serious] And we keep our exact voiceprints across every line of dialogue.
`);
```

---

### 3. Scripted Multi-Voice Dialogue & AI Control

Steer voices, speed, pitch, emotions, whispers, pauses, breaths, and mid-sentence singing from a single string or a structured JSON array (ideal for LLM / AI tool calls):

```js
// Inline-tagged script:
const scene = voice.speakScript(`
  [woman_a, calm, speed=0.88] We can start slowly and warmly...
  [inhale]
  [excited, speed=1.20, pitch=+2st] Then speed right up when something thrilling happens!
  [pause=260ms]
  [whisper, slow] Drop to an intimate whisper...
  [reset] and return cleanly to modal voice,
  [soprano, sing bpm=112] C5:1=or E5:1=burst G5:1.5=in- C5:2=to song! [/sing]
`);

// Or pass structured JSON cues from an AI agent:
const aiScene = voice.speakScript([
  { voice: "female", style: "cheerful", speed: 1.08, text: "Ready when you are!" },
  { pauseMs: 200 },
  { voice: "deep", style: "calm", warmth: 0.75, text: "Let's begin." },
]);
```

**Supported inline script tags (`[...]`):**
- **Speakers / Voices**: `male`, `female`, `deep`, `husky`, `bright`, `whisper`, `woman_a`, `woman_b`, `man_a`, `man_b`, `soprano`, `alto`, `tenor`, `baritone`, `bass_singer`, plus any custom voice registered with `voice.registerVoice(name, ...)` or `voice.randomize({ name, ... })`.
- **Styles**: `excited`, `happy`, `calm`, `sad`, `serious`, `dramatic`, `urgent`, `fast`, `slow`, `louder`, `softer`, `whisper`.
- **Relative / Exact Controls**: `speed=1.2`, `pitch=+3st` (or `pitch=195Hz`), `tract=14.5`, `expr=1.4`, `warmth=0.75`, `breathiness=0.3`, `roughness=0.2`, `vibrato=0.35`, `folds`, `pulse`.
- **Timing & Reset**: `[pause=300ms]`, `[inhale]`, `[gasp]`, `[reset]` (restores the active speaker's clean modal voice), `[/voice]` (reverts to the `TTS` instance's base voice), `[sing bpm=112] ... [/sing]`.

---

### 4. Musical Lyric Singing & Multi-Voice Chorus

Singing automatically uses the voice configured on your `TTS` instance unless you specify `voice`:

```js
// Create or configure any voice, then let it sing!
const singer = new TTS({ gender: "female", pitchHz: 210, vibrato: 0.35 });

// Multi-syllable English words ("Twinkle", "little", "wonder") split across notes automatically;
// use `~` to hold/slur a vowel across multiple pitches (melisma) and `R:1` for rests:
await singer.playSong(
  "C4:1 C4:1 G4:1 G4:1 | A4:1 A4:1 G4:2",
  "Twinkle twinkle little star",
  108
);

// Four-part SATB chorus mixed in sample-accurate sync with natural ensemble micro-detuning:
const choir = singer.singChorus({
  bpm: 102,
  parts: [
    { name: "Lead",    notes: "E4:1.5 D4:0.5 C4:2", lyrics: "Shine on bright" }, // uses `singer`'s voice!
    { name: "Soprano", voice: "soprano", notes: "G4:1.5 F4:0.5 E4:2", lyrics: "Shine on bright" },
    { name: "Tenor",   voice: "tenor",   notes: "C4:1.5 B3:0.5 G3:2", lyrics: "Shine on bright" },
    { name: "Bass",    voice: "deep",    notes: "C3:1.5 G2:0.5 C3:2", lyrics: "Shine on bright" },
  ],
});
```

---

### 5. Voice Controls & Presets

```js
new TTS();                                          // default male voice
new TTS("female");                                  // default female voice
new TTS("deep");                                    // built-in preset ("male", "deep", "husky", "female", "bright", "whisper")
new TTS({ gender: "female", pitchHz: 205, folds: true });

voice.setGender("female");                          // switch gender, keeping speed & volume
voice.configure({ breathiness: 0.2, warmth: 0.7 }); // atomic update (throws on unknown keys without mutating)
voice.settings;                                     // plain JSON-serializable object of all current settings
TTS.presets();                                      // [{ id, label, gender, pitchHz, … }]
TTS.voiceIds();                                     // all named preset + speaker + choir archetype IDs
TTS.styleNames();                                   // all expressive style names
TTS.noteToHz("F#4");                                // 369.994…
```

| Control | Range | Effect |
|---|---|---|
| `pitchHz` | `60–350` | Average fundamental frequency (male ≈ `85–145`, female ≈ `165–250`) |
| `tractLengthCm` | `12.2–21.5` | Vocal-tract length; shorter raises formants (male ≈ `17.5`, female ≈ `14.2`) |
| `speed` | `0.55–1.75` | Speaking rate multiplier |
| `expressiveness` | `0–3` | Intonation contour depth (`0` = monotone, `1` = natural lively default) |
| `warmth` | `0–1` | Softer vocal-fold closure |
| `chestResonance` | `0–1` | Sub-fundamental chest body |
| `breathiness` | `0–0.85` | Aspiration / glottal chink breath noise |
| `roughness` | `0–0.75` | Cycle-to-cycle jitter, shimmer & creak |
| `crispness` | `0.45–1.6` | Consonant burst/fricative energy |
| `nasality` | `0–0.8` | Velopharyngeal nasal coupling on vowels |
| `vibrato` | `0–1` | Pitch vibrato depth |
| `whisper` | `boolean` | Unvoiced whisper phonation |
| `folds` | `boolean` | Simulate physical 2-mass vocal folds instead of the LF equation pulse |
| `openQuotient` | `0.5–0.74` | Share of each glottal cycle the folds remain open |
| `closure` | `0–1` | Abruptness of vocal-fold closure (`0` = soft/dark, `1` = bright harmonics) |
| `subglottal` | `0–1` | Tracheal coupling while the glottis is open |
| `pitchCoupling` | `0–1` | Open-quotient widening as pitch rises |
| `air` | `0–1` | Upper vocal-tract resonances above 5 kHz |
| `room` | `0–1` | Subtle velvet-noise studio room reflection (`0` = dry, `0.25` = default) |
| `pauses` / `breaths` | `0.3–3` / `0–1` | Clause pause scaling and audible sentence-break inhalations |
| `bass` / `presence` / `brightness` | `-12–12` dB | Macro voiced tone shaping |
| `tone` | `10 × -18–18` dB | 10-band voiced EQ (`TTS.toneBandsHz()`: 125 Hz … 8 kHz) |
| `formants` | `5 × 0.75–1.35` | Global `[F1, F2, F3, F4, F5]` multipliers |
| `vowels` | `{ UW: [f1, f2, f3] }` | Per-vowel `[F1, F2, F3]` multipliers (`TTS.vowelNames()`) |
| `variation` / `seed` | `0–1` / `u32` | Deterministic human word-to-word tempo & pitch drift |
| `volume` | `0–4` | Output gain |

---

### 6. Streaming, Pronunciation & Speaker Profiles

```js
// Low-latency chunked streaming (loudness-calibrated to match speak()):
const stream = voice.stream(longArticleText);
while (!stream.done) {
  const pcmChunk = stream.read(4800); // 0.2 s Float32Array chunks at 24 kHz
}
stream.free();

// Custom pronunciations & CMUdict:
voice.addWord("tts", "T IY1 T IY1 EH1 S");
voice.phonemize("Hello world!"); // IPA
voice.arpabet("Hello world!");   // ARPAbet

// Load a fitted real-speaker profile (pure JSON settings, zero audio samples):
const slt = await (await fetch("./profiles/arctic_slt.json")).json();
const speaker = new TTS(slt);
```

---

### 7. Raw C-ABI WebAssembly Exports (Non-JS Hosts)

Because `tts.wasm` has **zero host imports**, any WebAssembly runtime (`wasmtime`, `wazero`, `wasmer`, Rust, Go, Python, C#) can call its linear-memory exports directly without JavaScript:

- `tts_new(opts_ptr, opts_len) -> handle` (`(0, 0)` for default male, or UTF-8 preset/JSON)
- `tts_alloc(len) -> ptr` (reusable UTF-8 input scratch buffer)
- `tts_speak_wav(handle, ptr, len) -> 0` / `tts_speak_script_wav` / `tts_sing_wav` / `tts_sing_chorus_wav`
- `tts_bytes_ptr() -> ptr`, `tts_bytes_len() -> len` (ready-to-play 16-bit 24 kHz RIFF `.wav`)
- `tts_drop(handle)`

---

## How It Works & Benchmarks

1. **Text & Normalization** (`src/text.rs`, `src/lexicon.rs`, `src/g2p.rs`, `src/plan.rs`): Expands currency, dates, times, fractions, units, abbreviations, and Roman numerals; looks up words in a compact 22k-exception lexicon + learned G2P rules covering the 50,000 most common English words at **99.75%** dictionary accuracy.
2. **Prosody & Allophones** (`src/durations.rs`, `src/timeline.rs`): Context-fitted phone durations, phrase-final lengthening, weak function-word reductions, alveolar flaps (`water`, `butter`), front/back velars, clear/dark `/l/`, and three-phase sentence breaths.
3. **Coarticulation & Intonation** (`src/frames.rs`, `src/sing.rs`): Class-specific formant transitions, diphthong trajectories, expressive statement/question/continuation F0 contours, plus singing portamento, bloom vibrato, and Sundberg high-pitch jaw/$F_1$ tracking.
4. **Glottal Source & Vocal-Tract Cascade** (`src/render.rs`, `src/folds.rs`): Band-limited Liljencrants-Fant glottal pulses (with female posterior-chink spectral tilt and vowel-offset abduction) or physical 2-mass Ishizaka-Flanagan vocal folds driving a 5-formant cascade, nasal pole-zero branch, fricative/burst resonators, and studio room reflection.

Measured on unseen held-out sentences against real human recordings (CMU Arctic):

| Metric | Original Port | TTS v1.0 (`tts.wasm`) | Human Recordings |
|---|---|---|---|
| Phoneme error rate (male / female) | 38% / 44% | **9% / 8%** | 8–10% |
| Predicted naturalness, UTMOS 1–5 (male / female) | 2.69 / 2.38 | **3.55 / 3.59** | 4.3 |

---

## Build, Test & Static Hosting Deployment

Requires Rust (with `rustup target add wasm32-unknown-unknown`) and Node.js 18+:

```sh
npm run build      # builds pkg/ AND an upload-ready static site in dist/
npm test           # runs Rust release unit tests + Node WASM smoke tests
npm run demo       # starts local studio at http://localhost:8080/demo/

# Native CLI example:
cargo run --release --example say -- --voice female "Hello from TTS!" hello.wav
cargo run --release --example say -- --script "[female, excited] Hi! [deep] Hello." scene.wav
cargo run --release --example say -- --sing "[bpm=112] C4:1=Shine E4:1=on G4:2=bright" song.wav
```

To deploy the demo to any static web host (Cloudflare Pages, Netlify, Vercel, S3, Apache/Nginx root, or a subfolder), upload the contents of **`dist/`** (`index.html`, `app.js`, `styles.css`, `tts.wasm`, `tts.js`, `tts.d.ts`, `_headers`, `profiles/`). Every asset path inside `dist/` is `./`-relative so it works at `/` or in any subdirectory with zero server configuration.

---

## Third-Party Data

`src/lexicon_common.txt` and `src/g2p_rules.txt` are derived offline from the CMU Pronouncing Dictionary (© Carnegie Mellon University, BSD 2-Clause — see `src/lexicon_common.LICENSE`). All synthesis, DSP, singing, scripting, and WebAssembly code is original to this project with zero runtime dependencies.
