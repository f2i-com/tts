# TTS (`tts.computer`)

**TTS** ([tts.computer](https://tts.computer)) is a human voice and singing synthesizer written in pure Rust and compiled to a **single self-contained WebAssembly file (`tts.wasm`)**.

English text, multi-voice dialogue scripts, or musical notes & lyrics go in — 24 kHz audio comes out — computed entirely from a physical model of the human vocal tract. **Zero audio samples, zero neural networks, zero servers, and zero external dependencies.**

- **Single-file self-contained `tts.wasm`** (~1.7 MB raw, ~590 KB gzipped) — **0 external WASM imports**, with the complete JavaScript `TTS` SDK baked directly inside `tts.wasm` itself (grab it from [GitHub Releases](https://github.com/f2i-com/tts/releases))
- **~400–500× faster than real time** in the browser; outputs 24 kHz mono PCM or ready-to-save 16-bit WAV
- **Natural speech realism** (5% male / 6% female blind held-out phoneme error rate — well below human recordings — and 4.05 / 4.06 UTMOS naturalness on unseen held-out speech, 3.91 / 3.97 on CMU Arctic)
- **Dynamic voice creation & reuse** (`TTS.randomVoice()`, `voice.randomize()`, `voice.registerVoice()`) — lock any parameters, randomize the rest within human anatomy, and reuse the voice across dialogue and song
- **Multi-voice scripted dialogue & singing** (`voice.speakScript()`, `voice.sing()`, `voice.singChorus()`) — inline speed, pitch, emotion styles, whispers, breaths, solo melodies, and 4-part harmonies
- **53 vocal sounds, ten of them kinds of laughter** (`[laugh]`, `[giggle]`, `[sigh]`, `[gasp]`, `[cough]`, `[hmm]`, `[uh-huh]`, `[shh]`, `[whistle]` …, or `voice.sound("laugh")`) — performed by the same vocal tract, so every voice laughs, sighs and hums as itself; each for as long as you ask (`[laugh=3s]`), and a voice can laugh *while* it speaks (`[laughing]`, or the `laughter` setting)
- **200 Hz lip-sync visemes, F0–F3 acoustics, and word/phoneme timings** included with every utterance

### What's New in v1.3

Vocal-tract phase dispersion, open-glottis subglottal coupling, breath-group pitch declination, and sound-by-sound coarticulation bring both voices past **4.05 UTMOS** on blind unseen sentences (3.78 / 3.78 → **3.91 / 3.97** on CMU Arctic, **4.05 / 4.06** on unseen held-out, **3.93 / 3.99** on conversational speech; SQUIM up to **4.25 / 4.75**), while cutting phoneme error rates below human studio recordings (**5.4% / 6.2%** on unseen held-out, **6.5% / 5.4%** on conversational speech) and zero-LM word error down to **7.7% / 9.4%** on conversational speech.

- **Voiced-only phase dispersion and per-gender vocal-tract tone curves.** Four causal all-pass sections (~1 ms round-trip delay of a 14–17.5 cm tract) relax the zero-phase peakiness of the glottal wavetable on the voiced path alone, leaving stop bursts and fricatives crisp at the lips. Each gender now carries its own 10-band voiced tone curve paired with calibrated `F4/F5/F6` laryngeal speaker's formant clusters (3451 / 3941 / 4489 Hz male, 3960 / 5060 Hz female), instead of sharing one curve that notched the male `F3`.
- **Open-glottis subglottal coupling and smooth laryngeal drift.** While the folds are open, the trachea damps the first resonance and couples its own pole-zero pair (`subglottal` 0.28 male, 0.34 female); uncorrelated per-cycle step jitter is removed from the male pulse in favour of smooth physiological flutter and walk, and female closure is firmer (`closure` 0.43).
- **Breath-group pitch declination and lively contour range.** Subglottal pressure drifts gently down across each clause (`decl` 0.38 / 0.40) while phrase-final loudness decay is softened so trailing words stay clear; short comma clauses share their sentence's pitch centre, bringing pitch movement (~3.1 st SD) into line with real studio readers.
- **Rhotic `[ɚ]` offglides, `/ɑr/` vs `/ɑ/`, and post-liquid coarticulation.** Post-vocalic `/r/` (*rare*, *there*, *start*) uses a vocalic `[ɚ]` offglide rather than lip-rounded onset `[ɹʷ]` (which made *rare* and *there* sound like *rever* and *very*); `/ɑr/` (*start*, *large*, *tomorrow*) retracts cleanly away from `[æɹ]` while non-rhotic `/ɑ/` (*pot*, *sock*) keeps a high `F3`; and coda `/l r/` supply proper left-hand formant context to following stops (*helps*, *garbage*).
- **Crisper stop clusters, velar loci, and word junctures.** `/tr/` (*trail*, *train*) gets postalveolar affrication, `/spr/` (*spring*) caps its labial burst below alveolar hiss, utterance-initial `/b d g/` carry light pre-voicing (*Gad*, *Gregson*), pre-velar `/ɪ/` raises before `/ŋ g/` (*raining*, *pink*), and pre-tonic `tə-` (*tomorrow*, *today*) coheres as one word.
- **A truer woman's voice and warmer man's voice.** Her third formants are brought down to a real woman's (they stood 5–15% above hers in every vowel), and the second formants of her front vowels half way down; his voice source rolls off above about 850 Hz to match his reader's balance.
- **"go", "got", "but", "could" and "joy" sound like themselves.** Back-velar `/k g/` bursts sit as a narrow peak just above the vowel's `F2`; `/b p m/` follow the vowel half way instead of gliding up from `/w/`; `/ʊ/` (*could*, *good*) is closer to the readers'; `/ɔɪ/` (*joy*, *voice*, *boy*) holds its rounded start for 40% of the vowel and glides all the way front; and the man's `/iː/` (*keep*, *each*) no longer drifts toward *cape*.
- **Words the dictionary does not know are pronounced properly.** The letter-to-sound rules for the start of a word (7,700 of them, keyed `#.h.i HH`) were being skipped as comments, so an unknown word lost its first sounds. With them, 74% of rare dictionary words come out exactly right instead of 66%, the embedded lexicon shrinks from 21,700 to 16,100 entries for the same 50,000 words, and `tts.wasm` is ~150 KB smaller (~591 KB gzipped).
- **Everyday talk, dates, phone numbers, `creak`, richer `randomVoice()`, and four new voices:** casual contractions and slang (*hiya*, *g'day*, *y'know*, *c'mon*, *outta*, *tryna*, *AI*, *iOS*, *arvo*), drawn-out spellings (*sooo*, *pleeease*), natural times/dates/phone numbers/version strings (`3:30pm`, `17/11/2026`, `+61 412 345 678`, `v1.3.0`), true breathy HNR drop on `breathiness`, a new `creak` vocal-fry control (`[creak=0.9]`), anatomical `TTS.randomVoice()` diversity, and four new presets (`crisp`, `elder`, `soft`, `mellow`).

### What's New in v1.2

Consonants and voices fitted to how two real readers speak, measured sound by sound (`tools/phones.py`): 3.61 / 3.63 → 3.78 / 3.78 predicted naturalness and 8% / 8% → 7% / 7% phoneme error rate.

- **Sharper upper formants**: the second and third resonances of every sound are narrower, more so in the male voice, so the formants stand out of the spectrum as a real voice's do.
- **The "th" of *the*, *that*, *this*** after a pause or a consonant is a soft stop — silent for most of its length, the voice setting in at the release — instead of a buzzing fricative.
- **/z/ really loses its voice** before a pause or another consonant (the rule existed; the voice bar kept buzzing through it), and before voiced consonants too.
- **Dark /l/ before vowels** (*lazy*, *blue*), as American speakers have it, not a clear European one.
- **/u/ after t d n s sh ch j y** (*too*, *do*, *new*, *you*) starts with the tongue still forward and glides back.
- **Less hiss in /v/ and /f/**, and an /h/ between voiced sounds that keeps the voice going (male voice).
- **Default voices**: tract length 17.0 cm (male, was 17.5) and 14.2 cm (female, was 13.9), closer to the readers' formants. The singer voices and presets with their own tract length are unchanged, and sung vowels keep their earlier resonance widths.

### What's New in v1.1

- **Laughter and vocal sounds**: 53 sounds in six groups — laughs, breaths, reactions, voice, mouth, body — as script tags (`[laugh]`, `[sigh]`, `[cough]`, `[hmm]`, `[whistle]` …) or from code (`voice.sound("laugh")`, `voice.playSound()`, `voice.soundWav()`, `TTS.sounds()`), each with a strength, a length, or a set duration (`[laugh=strong]`, `[laugh=3s]`, `{ durationMs: 3000 }`). See [Laughter & Vocal Sounds](#4-laughter--vocal-sounds).
- **Laughing while speaking**: the new `laughter` setting and the `[chuckling]` / `[laughing]` / `[giggling]` deliveries change how the words themselves are said, from a smile in the voice to laughter breaking through them.
- **More natural speech**: pronouns and auxiliaries are no longer accented and dwelt on, the duration model is refitted, /k g/ bursts are compact and loudness is steadier from word to word — 9% / 8% → 8% / 8% phoneme error rate and 3.55 / 3.59 → 3.61 / 3.63 predicted naturalness (see [benchmarks](#how-it-works--benchmarks)).
- **Timings for sounds**: every sound takes its place in `words` and `segments` (`sound: "laugh"`), so captions and lip-sync follow it.
- **Redesigned demo studio**: a sound palette that inserts or auditions any sound, a "Laughing while speaking" slider, and worked examples in the pronunciation trainer.
- **Apache-2.0 licence** (`LICENSE`).

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
| `segments` | `Array<{ start, end, voice, gender, pitchHz, tractLengthCm, singing, sound? }>` | Speaker/singing timeline (on `speakScript`, `sing`, `singChorus`, `sound`); `sound` is the id of a laugh, sigh … |
| `visemes` | `Float32Array` | 9 articulatory values per 5 ms frame (200 Hz): `[jaw, tongueHeight, tongueFront, tongueTip, lipRound, lipSpread, lipClose, velum, energy]` |
| `acoustics` | `Float32Array` | 6 acoustic values per 5 ms frame: `[f0, f1, f2, f3, voicing, noise]` |

Direct WAV helpers (return `Uint8Array` 16-bit 24 kHz `.wav` bytes):
- `voice.speakWav(text)`
- `voice.speakScriptWav(script)`
- `voice.soundWav(name, { intensity?, length?, durationMs? })`
- `voice.singWav(score, lyrics?, bpm?)`
- `voice.singChorusWav(chorus)`
- `encodeWav(samples, sampleRate = 24000)`

---

### 2. Dynamic Random Voice Creation & Reuse

Create brand-new, anatomically coherent human voices on the fly — locking any parameters you care about while randomizing the rest — and reuse them across dialogue and singing. Each random voice gets a body size (tract length and pitch together), its own resonances and vowels, a kind of voice (soft and breathy, pressed and bright, gravelly or plain) and its own speaking style:

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
  [chuckle] laugh about it, [sigh]
  [soprano, sing bpm=112] C5:1=or E5:1=burst G5:1.5=in- C5:2=to song! [/sing]
`);

// Or pass structured JSON cues from an AI agent:
const aiScene = voice.speakScript([
  { voice: "female", style: "cheerful", speed: 1.08, text: "Ready when you are!" },
  { pauseMs: 200 },
  { voice: "deep", style: "calm", warmth: 0.75, text: "Let's begin." },
  { sound: "chuckle", intensity: 0.8 },
]);
```

**Supported inline script tags (`[...]`):**
- **Speakers / Voices**: `male`, `female`, `deep`, `husky`, `bright`, `whisper`, `woman_a`, `woman_b`, `man_a`, `man_b`, `soprano`, `alto`, `tenor`, `baritone`, `bass_singer`, plus any custom voice registered with `voice.registerVoice(name, ...)` or `voice.randomize({ name, ... })`.
- **Styles**: `excited`, `happy`, `calm`, `sad`, `serious`, `dramatic`, `urgent`, `fast`, `slow`, `louder`, `softer`, `whisper`, and laughing while speaking: `chuckling`, `laughing`, `giggling` (ended by `[/laughing]` or `[reset]`).
- **Relative / Exact Controls**: `speed=1.2`, `pitch=+3st` (or `pitch=195Hz`), `tract=14.5`, `expr=1.4`, `warmth=0.75`, `breathiness=0.3`, `roughness=0.2`, `vibrato=0.35`, `creak=0.8`, `laughter=0.6`, `folds`, `pulse`.
- **Timing & Reset**: `[pause=300ms]`, `[inhale]`, `[reset]` (restores the active speaker's clean modal voice), `[/voice]` (reverts to the `TTS` instance's base voice), `[sing bpm=112] ... [/sing]`.
- **Laughter & vocal sounds**: `[laugh]`, `[giggle]`, `[sigh]`, `[gasp]`, `[cough]`, `[hmm]` …, optionally with a strength and a duration (`[laugh=strong]`, `[laugh=3s]`, `[sigh duration=1.5s]`) — see the next section.

---

### 4. Laughter & Vocal Sounds

Sounds that are not words are performed by the same vocal tract that speaks: a sequence of poses (pitch, mouth shape, voice, breath, hiss) the renderer is driven through. No sound is a recording, so each one follows the voice that makes it — any pitch, any tract length, a whisper.

```js
// In a script (or in plain `voice.play()` text) they are tags, made by whoever is speaking:
await voice.playScript(`
  [female, cheerful] You will never guess what happened at the meeting. [giggle]
  [deep, calm] [hmm] Go on.
  [female] The projector fell off the table, [gasp] and everyone just carried on! [laugh]
  [deep] [chuckle] Of course they did. [sigh] Mondays.
`);

// How strongly (0..1, or faint / gentle / strong / max) and how long (a factor, or short / long):
voice.speakScript("[laugh=strong] [sigh length=1.5] [giggle intensity=0.9 length=2]");

// ... or for a set time: a number with a unit, on its own or as duration= / dur= / for=
voice.speakScript("[laugh=3s] [sigh 2s] [giggle duration=4s] [shh=2500ms] [snore for=5s]");

// On their own, from code:
const laugh = voice.sound("laugh");                                   // the same Speech object as speak()
await voice.playSound("guffaw", { intensity: 0.9, length: 1.5 });
await voice.playSound("laugh", { durationMs: 3000 });                 // three seconds of it
voice.speakScript([{ sound: "sigh", durationMs: 1500 }, { text: "Fine." }]);
TTS.sounds();   // [{ id: "laugh", label: "Laugh", group: "laughter", about: "An open, easy laugh: ha-ha-ha-ha." }, …]
```

| Group | Sounds |
|---|---|
| Laughter | `laugh` `chuckle` `giggle` `guffaw` `cackle` `snicker` `hoho` `hehe` `nervous` `evil` |
| Breath | `inhale` `exhale` `gasp` `sigh` `yawn` `sniff` `pant` `phew` |
| Reactions | `hmm` `mm` `mhm` `uh-huh` `uh-uh` `uh-oh` `uh` `um` `huh` `aha` `oh` `ooh` `aww` `ahh` `ugh` `eww` |
| Voice | `hum` `groan` `scream` `sob` `whimper` |
| Mouth | `shh` `psst` `tsk` `kiss` `brr` `grr` `raspberry` `whistle` `fweet` |
| Body | `cough` `ahem` `hiccup` `sneeze` `snore` |

A duration is met by performing more or less of the sound, not by playing it slower: three seconds of laughter has more calls in it, a five-second snore is more breaths, a two-second sigh is one long sigh. What cannot last longer stays what it is (a hiccup is a hiccup).

**Laughing while speaking.** `laughter` (0–1) is a setting of the voice like any other, and `[chuckling]`, `[laughing]`, `[giggling]` are deliveries that turn it on in a script. It changes how the words are said rather than adding a sound beside them: a little is a smile in the voice (the pitch lifts, the vowels brighten as the lips spread, breath leaks through); more, and the laugh breaks into the stressed vowels as real laugh calls and spills out after the sentence. The words and their timings stay in `words`, so captions and lip-sync still follow.

```js
const amused = new TTS({ gender: "female", laughter: 0.6 });
await amused.play("I can't believe you actually did that!");

await voice.playScript(`
  [female, laughing] And then he tried to pick it up, and dropped it again! [/laughing]
  [female] Anyway. [laughter=0.9] Stop it, I can't breathe! [reset] Fine.
`);
```

Other names work too (`[haha]`, `[tee-hee]`, `[clearthroat]`, `[mwahaha]`). Each sound takes its place in `words` (as `"[laugh]"`) and in `segments` (with `sound: "laugh"`), so captions and lip-sync follow it; two laughs in one script are never the same laugh. The laugh follows what is measured in people — a train of short voiced calls about five a second, each followed by a puff of breath, the pitch around twice the speaking voice and sinking — and its numbers were set against an AudioSet tagger (`tools/sounds.py`), which hears laughter in all 20 laughs here (ten kinds, two voices) and puts the right name on 48 of the 68 renderings it is asked about. The cough, the breaths, the sneeze and the snore were reshaped against measurements of real recordings (how long, how fast they rise and die away, where their energy lies): a cough turned out to be air torn through the throat with very little voice in it, and it and the throat-clearing are now recognised. The sneeze, the snore, the sob and the sniff are not yet: they have the measured shape but are still stylised.

---

### 5. Musical Lyric Singing & Multi-Voice Chorus

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

### 6. Voice Controls & Presets

```js
new TTS();                                          // default male voice
new TTS("female");                                  // default female voice
new TTS("deep");                                    // built-in preset: "male", "deep", "husky", "crisp", "elder", "female", "bright", "soft", "mellow", "whisper"
new TTS({ gender: "female", pitchHz: 205, folds: true });

voice.setGender("female");                          // switch gender, keeping speed & volume
voice.configure({ breathiness: 0.2, warmth: 0.7 }); // atomic update (throws on unknown keys without mutating)
voice.settings;                                     // plain JSON-serializable object of all current settings
TTS.presets();                                      // [{ id, label, gender, pitchHz, … }]
TTS.voiceIds();                                     // all named preset + speaker + choir archetype IDs
TTS.styleNames();                                   // all expressive style names
TTS.sounds();                                       // laughs, sighs, coughs … [{ id, label, group, about }]
TTS.noteToHz("F#4");                                // 369.994…
```

| Control | Range | Effect |
|---|---|---|
| `pitchHz` | `60–350` | Average fundamental frequency (male ≈ `85–145`, female ≈ `165–250`) |
| `tractLengthCm` | `12.2–21.5` | Vocal-tract length; shorter raises formants (male ≈ `17.0`, female ≈ `14.2`) |
| `speed` | `0.55–1.75` | Speaking rate multiplier |
| `expressiveness` | `0–3` | Intonation contour depth (`0` = monotone, `1` = natural lively default) |
| `warmth` | `0–1` | Softer vocal-fold closure |
| `chestResonance` | `0–1` | Sub-fundamental chest body |
| `breathiness` | `0–0.85` | Breath in the voice: above the plain voices' `0.12` / `0.2` the pulse slackens and breath streams through, up to a really breathy voice |
| `roughness` | `0–0.75` | Cycle-to-cycle jitter, shimmer & creak |
| `crispness` | `0.45–1.6` | Consonant burst/fricative energy |
| `nasality` | `0–0.8` | Velopharyngeal nasal coupling on vowels |
| `vibrato` | `0–1` | Pitch vibrato depth |
| `creak` | `0–1` | Vocal fry as a phrase runs out (`0` = none, `0.5` = default, `1` = strong and early) |
| `whisper` | `boolean` | Unvoiced whisper phonation |
| `folds` | `boolean` | Simulate physical 2-mass vocal folds instead of the LF equation pulse |
| `openQuotient` | `0.5–0.74` | Share of each glottal cycle the folds remain open |
| `closure` | `0–1` | Abruptness of vocal-fold closure (`0` = soft/dark, `1` = bright harmonics) |
| `subglottal` | `0–1` | Tracheal coupling while the glottis is open |
| `pitchCoupling` | `0–1` | Open-quotient widening as pitch rises |
| `air` | `0–1` | Upper vocal-tract resonances above 5 kHz |
| `room` | `0–1` | Subtle velvet-noise studio room reflection (`0` = dry, `0.25` = default) |
| `pauses` / `breaths` | `0.3–3` / `0–1` | Clause pause scaling and audible sentence-break inhalations |
| `laughter` | `0–1` | Laughing while speaking: `0.3` a smile in the voice, `0.6` laughing through the words, `0.9` helpless |
| `bass` / `presence` / `brightness` | `-12–12` dB | Macro voiced tone shaping |
| `tone` | `10 × -18–18` dB | 10-band voiced EQ (`TTS.toneBandsHz()`: 125 Hz … 8 kHz) |
| `formants` | `5 × 0.75–1.35` | Global `[F1, F2, F3, F4, F5]` multipliers |
| `vowels` | `{ UW: [f1, f2, f3] }` | Per-vowel `[F1, F2, F3]` multipliers (`TTS.vowelNames()`) |
| `variation` / `seed` | `0–1` / `u32` | Deterministic human word-to-word tempo & pitch drift |
| `volume` | `0–4` | Output gain |

---

### 7. Streaming, Pronunciation & Speaker Profiles

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

### 8. Raw C-ABI WebAssembly Exports (Non-JS Hosts)

Because `tts.wasm` has **zero host imports**, any WebAssembly runtime (`wasmtime`, `wazero`, `wasmer`, Rust, Go, Python, C#) can call its linear-memory exports directly without JavaScript:

- `tts_new(opts_ptr, opts_len) -> handle` (`(0, 0)` for default male, or UTF-8 preset/JSON)
- `tts_alloc(len) -> ptr` (reusable UTF-8 input scratch buffer)
- `tts_speak_wav(handle, ptr, len) -> 0` / `tts_speak_script_wav` / `tts_sing_wav` / `tts_sing_chorus_wav`
- `tts_bytes_ptr() -> ptr`, `tts_bytes_len() -> len` (ready-to-play 16-bit 24 kHz RIFF `.wav`)
- `tts_drop(handle)`

---

## How It Works & Benchmarks

1. **Text & Normalization** (`src/text.rs`, `src/lexicon.rs`, `src/g2p.rs`, `src/plan.rs`): Expands currency, dates, times, fractions, units, abbreviations, and Roman numerals; looks up words in a compact 16k-exception lexicon + learned G2P rules that together pronounce the 50,000 most common English words exactly as the CMU dictionary does, and three in four rarer ones; a word drawn out in writing ("sooo") is read as the word it stands for.
2. **Prosody & Allophones** (`src/plan.rs`, `src/durations.rs`, `src/timeline.rs`): Three weights of word — content words are said in full and accented, pronouns and auxiliaries (`he`, `would`, `have`) are hurried through like `the` and `of`, longer prepositions and conjunctions keep their form but not an accent, and a pronoun at the end of a phrase leaves the main accent to the verb (`I *saw* him`) — with phone durations fitted to two readers for each, phrase-final lengthening, alveolar flaps (`water`, `butter`), front/back velars with a compact burst, a dark `/l/` in onset and coda, the stop-like "th" of `the` after a pause or consonant, `/z/` that gives up its voice before a consonant, a `/u/` that glides back from the front after `t d n s`, and three-phase sentence breaths.
3. **Coarticulation & Intonation** (`src/frames.rs`, `src/sing.rs`): Class-specific formant transitions, diphthong trajectories, expressive statement/question/continuation F0 contours, plus singing portamento, bloom vibrato, and Sundberg high-pitch jaw/$F_1$ tracking.
4. **Glottal Source & Vocal-Tract Cascade** (`src/render.rs`, `src/folds.rs`): Band-limited Liljencrants-Fant glottal pulses (with female posterior-chink spectral tilt and vowel-offset abduction) or physical 2-mass Ishizaka-Flanagan vocal folds driving a 5-formant cascade (its second and third resonances as sharp as a real voice's, not the textbook widths), nasal pole-zero branch, fricative/burst resonators, and studio room reflection.
5. **Laughter & Vocal Sounds** (`src/vocal.rs`, `src/frames.rs`): Each sound is a short performance for the same renderer — poses for pitch, mouth, voice, breath and hiss — plus a whistled tone for the two sounds made by the air in the mouth rather than the folds. A duration is met by performing more or less of it. Laughing while speaking works on the frames of the speech itself: the pitch lifts, the formants move as the lips spread, and laugh calls are cut into the stressed vowels and added after the sentence.

Measured on held-out sentences against real human recordings (`tools/eval.py` & `tools/closeness.py`):

| Metric (male / female) | Original Port | TTS v1.0 | TTS v1.1 | TTS v1.2 | TTS v1.3 | Human Recordings |
|---|---|---|---|---|---|---|
| Phoneme error rate — CMU Arctic | 38% / 44% | 9% / 8% | 8% / 8% | 7% / 7% | **8% / 7%** | 8–10% |
| Phoneme error rate — blind unseen held-out | — | — | — | 7% / 7% | **5.4% / 6.2%** | 6–9% |
| Predicted naturalness, UTMOS 1–5 — CMU Arctic | 2.69 / 2.38 | 3.55 / 3.59 | 3.61 / 3.63 | 3.78 / 3.78 | **3.91 / 3.97** | 4.3 |
| Predicted naturalness, UTMOS 1–5 — blind unseen held-out | — | — | — | 3.77 / 3.86 | **4.05 / 4.06** | 4.3–4.4 |

The step from v1.0 comes from how words are weighted (pronouns and auxiliaries no longer accented and dwelt on), a duration model refitted for it, compact /k g/ bursts and steadier loudness from word to word. Those held-out sentences were looked at while it was made; on 30 further sentences that no tuning has seen, the same step is +0.03 / +0.08 naturalness and 1.8 / 2.6 points fewer misheard sounds.

The step from v1.1 to v1.2 comes from looking inside the sounds rather than at whole sentences (`tools/phones.py`): the level through every consonant, and the formants of every vowel and glide, in two readers and in the synthesizer on up to 300 of the same sentences. Where both readers differed from the synthesizer in the same way, the rule was changed and kept only if the two judges above agreed (several faithful changes were not kept: unreleased stops before consonants lost intelligibility, louder nasals lost naturalness). On 30 newly written sentences that none of it has seen, the step is +0.17 / +0.09 naturalness and 2.0 / 0.0 points fewer misheard sounds.

The step from v1.2 to v1.3 separates the voiced tract path from lip/tooth turbulence so that per-gender 10-band tone curves and 4-section causal phase dispersion (~1 ms round-trip tract delay) warm and disperse the glottal harmonics without dulling bursts, turns on open-phase subglottal tracheal coupling and breath-group pitch declination, aligns the `F4/F5/F6` laryngeal speaker's formant cluster, and refines rhotic `[ɚ]` offglides, `/ɑr/` vs `/ɑ/`, coda `/l r/` coarticulation, and `/tr/`/`/spr/` stop clusters. On 20 blind unseen held-out sentences both voices cross 4.05 UTMOS (5.4% / 6.2% PER), and on 40 conversational sentences they reach 3.93 / 3.99 UTMOS, 4.21 / 4.78 SQUIM, and 7.7% / 9.4% zero-LM word error rate.

---

## Build, Test & Static Hosting Deployment

Requires Rust (with `rustup target add wasm32-unknown-unknown`) and Node.js 18+:

```sh
npm run build      # builds pkg/ AND an upload-ready static site in dist/
npm test           # runs Rust release unit tests + Node WASM smoke tests
npm run demo       # starts local studio at http://localhost:8080/demo/
python tools/sounds.py check   # what an AudioSet tagger hears in every laugh, sigh and cough
python tools/phones.py levels  # each consonant's level through time, next to two real readers

# Native CLI example:
cargo run --release --example say -- --voice female "Hello from TTS!" hello.wav
cargo run --release --example say -- --script "[female, excited] Hi! [deep] Hello." scene.wav
cargo run --release --example say -- --script "[female] That is ridiculous! [giggle] [deep] [hmm] Quite. [sigh]" sounds.wav
cargo run --release --example say -- --script "[female, laughing] You should have seen his face! [/laughing] [sigh=1.5s]" laughing.wav
cargo run --release --example say -- --sing "[bpm=112] C4:1=Shine E4:1=on G4:2=bright" song.wav
```

To deploy the demo to any static web host (Cloudflare Pages, Netlify, Vercel, S3, Apache/Nginx root, or a subfolder), upload the contents of **`dist/`** (`index.html`, `app.js`, `styles.css`, `tts.wasm`, `tts.js`, `tts.d.ts`, `_headers`, `profiles/`). Every asset path inside `dist/` is `./`-relative so it works at `/` or in any subdirectory with zero server configuration.

---

## License

TTS is created by [f2i.com](https://f2i.com) and released as open source under the [Apache License 2.0](LICENSE).

### Third-Party Data

`src/lexicon_common.txt` and `src/g2p_rules.txt` are derived offline from the CMU Pronouncing Dictionary (© Carnegie Mellon University, BSD 2-Clause — see `src/lexicon_common.LICENSE`). All synthesis, DSP, singing, scripting, and WebAssembly code is original to this project with zero runtime dependencies.
