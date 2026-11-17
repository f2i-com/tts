// Builds the standalone, zero-import `pkg/tts.wasm` binary (with its JS `TTS` glue embedded
// directly inside the `.wasm`) plus the optional ES module loader `pkg/tts.js` and `pkg/tts.d.ts`.
//
//   node tools/build_wasm.mjs
import { execFileSync, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import zlib from "node:zlib";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const pkgDir = path.join(root, "pkg");
fs.mkdirSync(pkgDir, { recursive: true });

console.log("[tts] Compiling Rust -> wasm32-unknown-unknown (release)...");
execFileSync("cargo", ["build", "--target", "wasm32-unknown-unknown", "--release", "--lib"], {
  cwd: root,
  stdio: "inherit",
});

const rawWasmPath = path.join(root, "target", "wasm32-unknown-unknown", "release", "tts.wasm");
const outWasmPath = path.join(pkgDir, "tts.wasm");
fs.copyFileSync(rawWasmPath, outWasmPath);

// Run wasm-opt if installed on PATH for smaller binary size
const runWasmOpt = (args) =>
  process.platform === "win32"
    ? spawnSync("cmd.exe", ["/d", "/s", "/c", "wasm-opt", ...args], { stdio: args[0] === "--version" ? "ignore" : "inherit" })
    : spawnSync("wasm-opt", args, { stdio: args[0] === "--version" ? "ignore" : "inherit" });

const optProbe = runWasmOpt(["--version"]);
if (optProbe.status === 0) {
  console.log("[tts] Optimizing pkg/tts.wasm with wasm-opt -Oz...");
  const optRes = runWasmOpt([
    "-Oz",
    "--enable-bulk-memory",
    "--enable-nontrapping-float-to-int",
    "--enable-sign-ext",
    "--enable-mutable-globals",
    outWasmPath,
    "-o",
    outWasmPath,
  ]);
  if (optRes.status !== 0) {
    console.warn("[tts] wasm-opt returned non-zero; keeping unoptimized release wasm.");
    fs.copyFileSync(rawWasmPath, outWasmPath);
  }
}

// Verify zero external imports & embedded JS glue inside tts.wasm!
const wasmBytes = fs.readFileSync(outWasmPath);
const wasmMod = new WebAssembly.Module(wasmBytes);
const imports = WebAssembly.Module.imports(wasmMod);
if (imports.length !== 0) {
  throw new Error(`Expected 0 external imports in tts.wasm, found ${imports.length}: ${JSON.stringify(imports)}`);
}
const inst = new WebAssembly.Instance(wasmMod, {});
const glueLen = inst.exports.tts_glue_len();
if (glueLen < 500) {
  throw new Error(`Embedded JS glue inside tts.wasm looks truncated (${glueLen} bytes)`);
}

// Extract `bindTTS` source from `src/glue.js` (stripping the trailing `Function("wasm", ...)` return)
const glueSrc = fs
  .readFileSync(path.join(root, "src", "glue.js"), "utf8")
  .replace(/if\s*\(typeof wasm !== "undefined"[\s\S]*$/, "")
  .trim();

const esmLoader = `// Auto-generated ES module loader for tts.wasm (tts.computer).
// Note: tts.wasm is also 100% self-contained (zero imports, JS glue embedded inside tts_glue_ptr/tts_glue_len).

${glueSrc}

let _bound = null;

function requireBound() {
  if (!_bound) {
    throw new Error("TTS WebAssembly is not initialized yet: call await init() or initSync(wasmBytes) first.");
  }
  return _bound;
}

function resolveModuleInput(input) {
  if (input && typeof input === "object" && "module_or_path" in input) return input.module_or_path;
  if (input && typeof input === "object" && "module" in input) return input.module;
  return input;
}

/** Synchronously initializes TTS from a \`WebAssembly.Module\` or \`BufferSource\` (\`Uint8Array\` / \`ArrayBuffer\`). */
export function initSync(moduleOrBytes) {
  const raw = resolveModuleInput(moduleOrBytes);
  const mod = raw instanceof WebAssembly.Module ? raw : new WebAssembly.Module(raw);
  const instance = new WebAssembly.Instance(mod, {});
  _bound = bindTTS(instance.exports);
  return _bound;
}

/**
 * Loads \`tts.wasm\` (defaults to \`new URL("./tts.wasm", import.meta.url)\`) and initializes \`TTS\`.
 * Returns \`{ TTS, SpeechStream, encodeWav, exports }\`.
 */
export default async function init(moduleOrPath) {
  let src = resolveModuleInput(moduleOrPath);
  if (src === undefined) {
    src = new URL("./tts.wasm", import.meta.url);
  }
  let instance;
  if (src instanceof WebAssembly.Instance) {
    instance = src;
  } else if (src instanceof WebAssembly.Module) {
    instance = await WebAssembly.instantiate(src, {});
  } else if (ArrayBuffer.isView(src) || src instanceof ArrayBuffer) {
    ({ instance } = await WebAssembly.instantiate(src, {}));
  } else {
    const resp = src instanceof Response ? src : fetch(src);
    if (typeof WebAssembly.instantiateStreaming === "function") {
      try {
        ({ instance } = await WebAssembly.instantiateStreaming(resp, {}));
      } catch {
        const r = await (src instanceof Response ? src : fetch(src));
        ({ instance } = await WebAssembly.instantiate(await r.arrayBuffer(), {}));
      }
    } else {
      const r = await resp;
      ({ instance } = await WebAssembly.instantiate(await r.arrayBuffer(), {}));
    }
  }
  _bound = bindTTS(instance.exports);
  return { ..._bound, exports: instance.exports };
}

/** One-step helper: loads \`tts.wasm\` and returns \`{ TTS, SpeechStream, encodeWav, exports }\`. */
export const loadTTS = init;

export class TTS {
  #inner;
  constructor(options) {
    const { TTS: BoundTTS } = requireBound();
    this.#inner = new BoundTTS(options);
  }
  static presets() { return requireBound().TTS.presets(); }
  static vowelNames() { return requireBound().TTS.vowelNames(); }
  static toneBandsHz() { return requireBound().TTS.toneBandsHz(); }
  static voiceIds() { return requireBound().TTS.voiceIds(); }
  static styleNames() { return requireBound().TTS.styleNames(); }
  static sounds() { return requireBound().TTS.sounds(); }
  static soundNames() { return requireBound().TTS.soundNames(); }
  static noteToHz(note) { return requireBound().TTS.noteToHz(note); }
  static randomVoice(options) { return requireBound().TTS.randomVoice(options); }

  configure(options) { return this.#inner.configure(options); }
  randomize(options) { return this.#inner.randomize(options); }
  registerVoice(name, profile) { return this.#inner.registerVoice(name, profile); }
  voices() { return this.#inner.voices(); }
  setGender(gender) { return this.#inner.setGender(gender); }
  get gender() { return this.#inner.gender; }
  get settings() { return this.#inner.settings; }
  get sampleRate() { return this.#inner.sampleRate; }

  speak(text) { return this.#inner.speak(text); }
  speakWav(text) { return this.#inner.speakWav(text); }
  speakScript(script) { return this.#inner.speakScript(script); }
  speakScriptWav(script) { return this.#inner.speakScriptWav(script); }
  sound(name, options) { return this.#inner.sound(name, options); }
  soundWav(name, options) { return this.#inner.soundWav(name, options); }
  sing(score, lyrics, bpm) { return this.#inner.sing(score, lyrics, bpm); }
  singWav(score, lyrics, bpm) { return this.#inner.singWav(score, lyrics, bpm); }
  singChorus(chorus) { return this.#inner.singChorus(chorus); }
  singChorusWav(chorus) { return this.#inner.singChorusWav(chorus); }

  play(text, audioContext) { return this.#inner.play(text, audioContext); }
  playScript(script, audioContext) { return this.#inner.playScript(script, audioContext); }
  playSound(name, options, audioContext) { return this.#inner.playSound(name, options, audioContext); }
  playSong(score, lyrics, bpm, audioContext) { return this.#inner.playSong(score, lyrics, bpm, audioContext); }
  playChorus(chorus, audioContext) { return this.#inner.playChorus(chorus, audioContext); }

  stream(text) { return this.#inner.stream(text); }
  phonemize(text) { return this.#inner.phonemize(text); }
  arpabet(text) { return this.#inner.arpabet(text); }
  normalize(text) { return this.#inner.normalize(text); }
  addWord(word, phones) { return this.#inner.addWord(word, phones); }
  loadDictionary(text) { return this.#inner.loadDictionary(text); }
  free() { return this.#inner.free(); }
}

export function encodeWav(samples, sampleRate) {
  return requireBound().encodeWav(samples, sampleRate);
}
`;

const dts = `// TypeScript declarations for TTS (tts.computer)
export type Gender = "male" | "female";

export interface VoiceOptions {
  preset?: string;
  gender?: Gender;
  pitchHz?: number;
  tractLengthCm?: number;
  speed?: number;
  warmth?: number;
  chestResonance?: number;
  breathiness?: number;
  nasality?: number;
  roughness?: number;
  crispness?: number;
  expressiveness?: number;
  vibrato?: number;
  whisper?: boolean;
  folds?: boolean;
  volume?: number;
  variation?: number;
  seed?: number;
  bass?: number;
  presence?: number;
  brightness?: number;
  tone?: number[];
  formants?: number[];
  vowels?: Record<string, number[]>;
  pauses?: number;
  breaths?: number;
  openQuotient?: number;
  subglottal?: number;
  room?: number;
  pitchCoupling?: number;
  /** 0..1: how much the speaker is laughing while speaking (a smile in the voice, then the laugh breaking into the stressed vowels). */
  laughter?: number;
  /** 0..1: vocal fry as a phrase runs out (0 = none, 0.5 = the plain voices' amount, 1 = strong and early). */
  creak?: number;
  air?: number;
  closure?: number;
}

export interface RandomVoiceOptions extends VoiceOptions {
  /** Optional reusable speaker tag name (e.g. "alice") when calling \`voice.randomize({ name: "alice" })\`. */
  name?: string;
}

export type VoiceSettings = Required<Omit<VoiceOptions, "preset">>;

export interface VoicePreset extends VoiceSettings {
  id: string;
  label: string;
}

export interface WordTiming {
  word: string;
  start: number;
  end: number;
}

export interface PhoneTiming {
  phone: string;
  ipa: string;
  start: number;
  end: number;
}

export interface SegmentTiming {
  start: number;
  end: number;
  voice: string;
  gender: Gender;
  pitchHz: number;
  tractLengthCm: number;
  singing: boolean;
  /** For a sound that is not a word (a laugh, a sigh ...), its id; absent for speech and song. */
  sound?: string;
}

/** A sound a voice makes that is not a word: a laugh, a sigh, a cough, "mm-hm", a whistle ... */
export interface VocalSound {
  /** The script tag (\`[laugh]\`) and the name for \`voice.sound()\`. */
  id: string;
  label: string;
  group: "laughter" | "breath" | "reaction" | "voice" | "mouth" | "body";
  about: string;
}

export interface SoundOptions {
  /** How strongly: 0..1 (0.6 is the usual), or "faint" | "gentle" | "medium" | "strong" | "max". */
  intensity?: number | string;
  /** How long against the usual length (1): a factor 0.25..4, or "short" | "long" | "verylong". */
  length?: number | string;
  /** Or how long in milliseconds, as nearly as the sound allows (a hiccup stays a hiccup). */
  durationMs?: number;
}

export interface ScriptCueInput extends VoiceOptions, SoundOptions {
  voice?: string;
  style?: string;
  text?: string;
  pauseMs?: number;
  inhale?: number | boolean;
  /** A vocal sound made before any \`text\` of the same cue: "laugh", "sigh", "cough" ... */
  sound?: string;
  sing?: string;
  bpm?: number;
}

export interface SingNoteInput {
  note?: string;
  hz?: number;
  rest?: number;
  beats?: number;
  durationMs?: number;
  lyric?: string;
  slur?: boolean;
  velocity?: number;
  vibrato?: number;
}

export interface SingOptions extends VoiceOptions {
  voice?: string;
  bpm?: number;
  transpose?: number;
  legato?: number | boolean;
  score?: string;
  notes?: string | SingNoteInput[];
  lyrics?: string;
}

export interface ChorusPartInput extends SingOptions {
  name?: string;
  gain?: number;
  offsetMs?: number;
  detuneCents?: number;
}

export interface ChorusOptions {
  bpm?: number;
  parts: ChorusPartInput[];
}

export interface Speech {
  samples: Float32Array;
  sampleRate: number;
  duration: number;
  text: string;
  ipa: string;
  words: WordTiming[];
  phones: PhoneTiming[];
  segments?: SegmentTiming[];
  visemes: Float32Array;
  visemeRate: number;
  visemeFields: string[];
  acoustics: Float32Array;
  acousticFields: string[];
}

export interface PlaybackHandle {
  speech: Speech;
  stop(): void;
  done: Promise<Speech>;
}

export class SpeechStream {
  read(maxSamples?: number): Float32Array;
  readonly done: boolean;
  readonly duration: number;
  readonly position: number;
  readonly sampleRate: number;
  readonly text: string;
  readonly ipa: string;
  readonly words: WordTiming[];
  readonly visemes: Float32Array;
  free(): void;
}

export class TTS {
  constructor(options?: VoiceOptions | Gender | string);
  static presets(): VoicePreset[];
  static vowelNames(): string[];
  static toneBandsHz(): Float64Array;
  static voiceIds(): string[];
  static styleNames(): string[];
  /** The sounds that are not words, grouped: each \`id\` is a script tag and a name for \`sound()\`. */
  static sounds(): VocalSound[];
  static soundNames(): string[];
  static noteToHz(note: string): number;
  static randomVoice(options?: VoiceOptions | Gender | string): VoiceSettings;

  configure(options: VoiceOptions | Gender | string): VoiceSettings;
  randomize(options?: RandomVoiceOptions | Gender | string): VoiceSettings;
  registerVoice(name: string, profile?: VoiceOptions | Gender | string): VoiceSettings;
  voices(): Record<string, VoiceSettings>;
  setGender(gender: Gender | string): void;
  readonly gender: Gender;
  readonly settings: VoiceSettings;
  readonly sampleRate: number;

  speak(text: string): Speech;
  speakWav(text: string): Uint8Array;
  speakScript(script: string | ScriptCueInput[]): Speech;
  speakScriptWav(script: string | ScriptCueInput[]): Uint8Array;
  /** A sound that is not a word, in this voice: \`voice.sound("laugh")\`, \`voice.sound("sigh", { intensity: 0.9 })\`. */
  sound(name: string, options?: SoundOptions): Speech;
  soundWav(name: string, options?: SoundOptions): Uint8Array;
  sing(score: string | SingOptions, lyrics?: string, bpm?: number): Speech;
  singWav(score: string | SingOptions, lyrics?: string, bpm?: number): Uint8Array;
  singChorus(chorus: string | ChorusOptions | ChorusPartInput[]): Speech;
  singChorusWav(chorus: string | ChorusOptions | ChorusPartInput[]): Uint8Array;

  play(text: string, audioContext?: AudioContext): Promise<PlaybackHandle>;
  playScript(script: string | ScriptCueInput[], audioContext?: AudioContext): Promise<PlaybackHandle>;
  playSound(name: string, options?: SoundOptions, audioContext?: AudioContext): Promise<PlaybackHandle>;
  playSong(score: string | SingOptions, lyrics?: string, bpm?: number, audioContext?: AudioContext): Promise<PlaybackHandle>;
  playChorus(chorus: string | ChorusOptions | ChorusPartInput[], audioContext?: AudioContext): Promise<PlaybackHandle>;

  stream(text: string): SpeechStream;
  phonemize(text: string): string;
  arpabet(text: string): string;
  normalize(text: string): string;
  addWord(word: string, phones: string): void;
  loadDictionary(text: string): number;
  free(): void;
}

export function encodeWav(samples: Float32Array | number[], sampleRate?: number): Uint8Array;
export function initSync(moduleOrBytes: WebAssembly.Module | BufferSource | { module: WebAssembly.Module | BufferSource }): { TTS: typeof TTS; SpeechStream: typeof SpeechStream; encodeWav: typeof encodeWav };
export default function init(moduleOrPath?: RequestInfo | URL | Response | BufferSource | WebAssembly.Module): Promise<{ TTS: typeof TTS; SpeechStream: typeof SpeechStream; encodeWav: typeof encodeWav; exports: WebAssembly.Exports }>;
export const loadTTS: typeof init;
`;

fs.writeFileSync(path.join(pkgDir, "tts.js"), esmLoader);
fs.writeFileSync(path.join(pkgDir, "tts.d.ts"), dts);
fs.writeFileSync(
  path.join(pkgDir, "package.json"),
  JSON.stringify(
    {
      name: "tts",
      // (one version for the crate, the repository and the package: the root package.json)
      version: JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8")).version,
      license: "Apache-2.0",
      type: "module",
      description: "TTS (tts.computer): self-contained physical vocal-tract speech & singing synthesizer in WebAssembly.",
      homepage: "https://tts.computer",
      repository: { type: "git", url: "https://github.com/f2i-com/tts.git" },
      main: "tts.js",
      types: "tts.d.ts",
      files: ["tts.wasm", "tts.js", "tts.d.ts"],
    },
    null,
    2
  ) + "\n"
);

const gzSize = zlib.gzipSync(wasmBytes).length;
console.log(
  `[tts] Built self-contained pkg/tts.wasm (${(wasmBytes.length / 1024).toFixed(0)} KB raw, ${(gzSize / 1024).toFixed(0)} KB gzipped, 0 external imports, ${glueLen} bytes internal JS glue) + pkg/tts.js + pkg/tts.d.ts`
);
