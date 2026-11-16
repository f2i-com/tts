// Embedded JavaScript SDK for TTS (tts.computer).
// Compiled directly inside `tts.wasm` (`tts_glue_ptr` / `tts_glue_len`) so a single `.wasm`
// file contains its own complete JS bindings with zero external glue dependencies.
//
// Evaluated via `Function("wasm", code)(instance.exports)` -> `{ TTS, SpeechStream, encodeWav }`.

const enc = new TextEncoder();
const dec = new TextDecoder();

function bindTTS(w) {
  if (!w || !w.memory || typeof w.tts_new !== "function") {
    throw new Error("Invalid TTS WebAssembly exports object");
  }

  const writeUtf8 = (str) => {
    const bytes = enc.encode(String(str));
    if (bytes.length === 0) return [0, 0];
    const ptr = w.tts_alloc(bytes.length);
    new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  };

  const readJsonText = () => {
    const len = w.tts_json_len();
    if (!len) return "";
    const ptr = w.tts_json_ptr();
    return dec.decode(new Uint8Array(w.memory.buffer, ptr, len));
  };

  const readJson = () => JSON.parse(readJsonText());

  const copyF32 = (ptrFn, lenFn) => {
    const len = lenFn();
    if (!len) return new Float32Array(0);
    const ptr = ptrFn();
    return new Float32Array(w.memory.buffer, ptr, len).slice();
  };

  const copyBytes = () => {
    const len = w.tts_bytes_len();
    if (!len) return new Uint8Array(0);
    const ptr = w.tts_bytes_ptr();
    return new Uint8Array(w.memory.buffer, ptr, len).slice();
  };

  // Preserve non-finite numbers as sentinel strings so Rust can report "<key> must be a finite number"
  // instead of JSON.stringify silently turning NaN/Infinity into null.
  const serializePayload = (val) =>
    JSON.stringify(val, (_k, v) =>
      typeof v === "number" && !Number.isFinite(v) ? "__TTS_NONFINITE__" : v
    );

  w.tts_meta_json();
  const META = readJson();

  let sharedAudioCtx = null;
  let rngCounter = ((Date.now ? Date.now() : 123456789) ^ 0x5f3759df) >>> 0;
  const nextSeed = () => {
    if (typeof crypto !== "undefined" && typeof crypto.getRandomValues === "function") {
      const a = new Uint32Array(1);
      crypto.getRandomValues(a);
      return a[0] >>> 0;
    }
    rngCounter = (Math.imul(rngCounter, 1664525) + 1013904223) >>> 0;
    return rngCounter;
  };

  const buildSpeechObject = () => {
    const meta = readJson();
    const out = {
      samples: copyF32(w.tts_samples_ptr, w.tts_samples_len),
      sampleRate: meta.sampleRate,
      duration: meta.duration,
      text: meta.text,
      ipa: meta.ipa,
      words: meta.words,
      phones: meta.phones,
      visemes: copyF32(w.tts_visemes_ptr, w.tts_visemes_len),
      visemeRate: META.visemeRate,
      visemeFields: META.visemeFields.slice(),
      acoustics: copyF32(w.tts_acoustics_ptr, w.tts_acoustics_len),
      acousticFields: META.acousticFields.slice(),
    };
    if (meta.segments) out.segments = meta.segments;
    return out;
  };

  const playSpeechInBrowser = async (speech, audioContext) => {
    const Ctx =
      audioContext ? null : (globalThis.AudioContext || globalThis.webkitAudioContext);
    const ctx = audioContext || (sharedAudioCtx ??= Ctx ? new Ctx() : null);
    if (!ctx) {
      throw new Error("Web Audio API (AudioContext) is not available in this environment");
    }
    if (ctx.state === "suspended") await ctx.resume();
    if (!speech || !speech.samples || speech.samples.length === 0) {
      return { speech, stop() {}, done: Promise.resolve(speech) };
    }
    const buffer = ctx.createBuffer(1, speech.samples.length, speech.sampleRate);
    buffer.copyToChannel(speech.samples, 0);
    const source = ctx.createBufferSource();
    source.buffer = buffer;
    source.connect(ctx.destination);
    const done = new Promise((resolve) => {
      source.onended = () => resolve(speech);
    });
    source.start();
    return {
      speech,
      source,
      stop() {
        try {
          source.stop();
        } catch {}
      },
      done,
    };
  };

  const instRegistry =
    typeof FinalizationRegistry !== "undefined"
      ? new FinalizationRegistry((h) => {
          if (h > 0) w.tts_drop(h);
        })
      : null;

  const streamRegistry =
    typeof FinalizationRegistry !== "undefined"
      ? new FinalizationRegistry((h) => {
          if (h > 0) w.tts_stream_drop(h);
        })
      : null;

  class SpeechStream {
    #handle;
    #meta;
    #visemes;

    constructor(handle, meta, visemes) {
      this.#handle = handle;
      this.#meta = meta;
      this.#visemes = visemes;
      if (streamRegistry) streamRegistry.register(this, handle, this);
    }

    read(maxSamples = 4800) {
      if (!this.#handle) return new Float32Array(0);
      const n = w.tts_stream_read(this.#handle, Math.max(0, Math.floor(Number(maxSamples) || 0)));
      if (n < 0) throw new Error(readJsonText());
      return copyF32(w.tts_samples_ptr, w.tts_samples_len);
    }

    get done() {
      return !this.#handle || w.tts_stream_done(this.#handle) !== 0;
    }

    get duration() {
      return this.#meta.duration;
    }

    get position() {
      return this.#handle ? w.tts_stream_position(this.#handle) : this.#meta.duration;
    }

    get sampleRate() {
      return this.#meta.sampleRate;
    }

    get text() {
      return this.#meta.text;
    }

    get ipa() {
      return this.#meta.ipa;
    }

    get words() {
      return this.#meta.words.map((x) => ({ ...x }));
    }

    get visemes() {
      return this.#visemes.slice();
    }

    free() {
      if (this.#handle) {
        if (streamRegistry) streamRegistry.unregister(this);
        w.tts_stream_drop(this.#handle);
        this.#handle = 0;
      }
    }
  }

  class TTS {
    #handle;

    constructor(options) {
      let ptr = 0;
      let len = 0;
      if (options !== undefined && options !== null) {
        [ptr, len] = writeUtf8(serializePayload(options));
      }
      const h = w.tts_new(ptr, len);
      if (h < 0) throw new Error(readJsonText());
      this.#handle = h;
      if (instRegistry) instRegistry.register(this, h, this);
    }

    #checkHandle() {
      if (!this.#handle) throw new Error("TTS instance has been freed");
      return this.#handle;
    }

    static presets() {
      return structuredClone(META.presets);
    }

    static vowelNames() {
      return META.vowelNames.slice();
    }

    static toneBandsHz() {
      return Float64Array.from(META.toneBandsHz);
    }

    static voiceIds() {
      return META.voiceIds.slice();
    }

    static styleNames() {
      return META.styleNames.slice();
    }

    static noteToHz(note) {
      const [ptr, len] = writeUtf8(String(note));
      const hz = w.tts_note_to_hz(ptr, len);
      if (hz < 0) throw new Error(readJsonText());
      return hz;
    }

    /**
     * Generates a brand-new, physically coherent voice profile object.
     * Any parameters passed in `options` (e.g. `{ gender: "female", speed: 1.1, seed: 42 }`)
     * are locked; all remaining voice parameters are randomized within natural human ranges.
     * Pass the returned object to `new TTS(profile)`, `voice.configure(profile)`, or
     * `voice.registerVoice("alice", profile)` to reuse it anywhere!
     */
    static randomVoice(options) {
      const seed =
        options && typeof options === "object" && Number.isFinite(options.seed)
          ? (Math.abs(Math.trunc(options.seed)) >>> 0)
          : nextSeed();
      let ptr = 0;
      let len = 0;
      if (options !== undefined && options !== null) {
        [ptr, len] = writeUtf8(serializePayload(options));
      }
      if (w.tts_random_voice(seed, ptr, len) < 0) {
        throw new Error(readJsonText());
      }
      return readJson();
    }

    configure(options) {
      const h = this.#checkHandle();
      if (options === undefined || options === null) return this.settings;
      const [ptr, len] = writeUtf8(serializePayload(options));
      if (w.tts_configure(h, ptr, len) < 0) {
        throw new Error(readJsonText());
      }
      return readJson();
    }

    /**
     * Randomizes this instance's voice while respecting any locked parameters in `options`
     * (e.g. `voice.randomize({ gender: "female", warmth: 0.7 })` or
     * `voice.randomize({ name: "alice", gender: "female" })` to also register `[alice]`).
     * Returns the resulting reusable `VoiceSettings` profile object.
     */
    randomize(options) {
      const h = this.#checkHandle();
      const seed =
        options && typeof options === "object" && Number.isFinite(options.seed)
          ? (Math.abs(Math.trunc(options.seed)) >>> 0)
          : nextSeed();
      let ptr = 0;
      let len = 0;
      if (options !== undefined && options !== null) {
        [ptr, len] = writeUtf8(serializePayload(options));
      }
      if (w.tts_randomize(h, seed, ptr, len) < 0) {
        throw new Error(readJsonText());
      }
      return readJson();
    }

    /**
     * Registers a named custom voice (e.g. from `TTS.randomVoice()` or custom settings) so it
     * can be referenced by `[name]` in `speakScript`, `sing`, `singChorus`, and `configure(name)`.
     */
    registerVoice(name, profile) {
      const h = this.#checkHandle();
      if (typeof name !== "string" || !name.trim()) {
        throw new Error("voice name must be a non-empty string");
      }
      const payload = serializePayload({
        name,
        voice: profile === undefined ? this.settings : profile,
      });
      const [ptr, len] = writeUtf8(payload);
      if (w.tts_register_voice(h, ptr, len) < 0) {
        throw new Error(readJsonText());
      }
      return readJson();
    }

    /** Returns a map `{ [name]: VoiceSettings }` of custom voices registered on this instance. */
    voices() {
      const h = this.#checkHandle();
      if (w.tts_list_voices(h) < 0) throw new Error(readJsonText());
      return readJson();
    }

    setGender(gender) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(gender));
      if (w.tts_set_gender(h, ptr, len) < 0) {
        throw new Error(readJsonText());
      }
    }

    get gender() {
      return this.settings.gender;
    }

    get settings() {
      const h = this.#checkHandle();
      if (w.tts_get_settings(h) < 0) throw new Error(readJsonText());
      return readJson();
    }

    get sampleRate() {
      return META.sampleRate;
    }

    speak(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      if (w.tts_speak(h, ptr, len) < 0) throw new Error(readJsonText());
      return buildSpeechObject();
    }

    speakWav(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      if (w.tts_speak_wav(h, ptr, len) < 0) throw new Error(readJsonText());
      return copyBytes();
    }

    speakScript(script) {
      const h = this.#checkHandle();
      const payload =
        typeof script === "string"
          ? script
          : Array.isArray(script)
            ? serializePayload({ __cues: script })
            : null;
      if (payload === null) {
        throw new Error("speakScript expects a script string or an array of cue objects");
      }
      const [ptr, len] = writeUtf8(payload);
      if (w.tts_speak_script(h, ptr, len) < 0) throw new Error(readJsonText());
      return buildSpeechObject();
    }

    speakScriptWav(script) {
      const h = this.#checkHandle();
      const payload =
        typeof script === "string"
          ? script
          : Array.isArray(script)
            ? serializePayload({ __cues: script })
            : null;
      if (payload === null) {
        throw new Error("speakScriptWav expects a script string or an array of cue objects");
      }
      const [ptr, len] = writeUtf8(payload);
      if (w.tts_speak_script_wav(h, ptr, len) < 0) throw new Error(readJsonText());
      return copyBytes();
    }

    #packSingArgs(score, lyrics, bpm) {
      if (typeof score === "string") {
        if (lyrics === undefined && bpm === undefined) return score;
        const o = { notes: score };
        if (lyrics !== undefined && String(lyrics).trim() !== "") o.lyrics = String(lyrics);
        else {
          delete o.notes;
          o.score = score;
        }
        if (bpm !== undefined) o.bpm = bpm;
        return serializePayload(o);
      }
      if (score && typeof score === "object" && !Array.isArray(score)) {
        const merged = { ...score };
        if (lyrics !== undefined && merged.lyrics === undefined) merged.lyrics = lyrics;
        if (bpm !== undefined && merged.bpm === undefined) merged.bpm = bpm;
        return serializePayload(merged);
      }
      throw new Error("sing expects a score string or an options object { bpm, notes, lyrics, … }");
    }

    sing(score, lyrics, bpm) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(this.#packSingArgs(score, lyrics, bpm));
      if (w.tts_sing(h, ptr, len) < 0) throw new Error(readJsonText());
      return buildSpeechObject();
    }

    singWav(score, lyrics, bpm) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(this.#packSingArgs(score, lyrics, bpm));
      if (w.tts_sing_wav(h, ptr, len) < 0) throw new Error(readJsonText());
      return copyBytes();
    }

    #packChorusArg(chorus) {
      if (typeof chorus === "string") return chorus;
      if (Array.isArray(chorus)) return serializePayload({ parts: chorus });
      if (chorus && typeof chorus === "object") return serializePayload(chorus);
      throw new Error("singChorus expects a chorus score string or `{ bpm, parts: [...] }`");
    }

    singChorus(chorus) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(this.#packChorusArg(chorus));
      if (w.tts_sing_chorus(h, ptr, len) < 0) throw new Error(readJsonText());
      return buildSpeechObject();
    }

    singChorusWav(chorus) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(this.#packChorusArg(chorus));
      if (w.tts_sing_chorus_wav(h, ptr, len) < 0) throw new Error(readJsonText());
      return copyBytes();
    }

    /** Speaks `text` directly through Web Audio in the browser. */
    async play(text, audioContext) {
      const speech = /\[[a-zA-Z_/]/.test(String(text))
        ? this.speakScript(String(text))
        : this.speak(String(text));
      return playSpeechInBrowser(speech, audioContext);
    }

    /** Performs a multi-voice / inline-controlled script directly through Web Audio. */
    async playScript(script, audioContext) {
      return playSpeechInBrowser(this.speakScript(script), audioContext);
    }

    /** Sings a solo melody directly through Web Audio. */
    async playSong(score, lyrics, bpm, audioContext) {
      return playSpeechInBrowser(this.sing(score, lyrics, bpm), audioContext);
    }

    /** Sings a multi-voice chorus directly through Web Audio. */
    async playChorus(chorus, audioContext) {
      return playSpeechInBrowser(this.singChorus(chorus), audioContext);
    }

    stream(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      const sh = w.tts_stream_new(h, ptr, len);
      if (sh < 0) throw new Error(readJsonText());
      const meta = readJson();
      const vis = copyF32(w.tts_visemes_ptr, w.tts_visemes_len);
      return new SpeechStream(sh, meta, vis);
    }

    phonemize(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      if (w.tts_phonemize(h, ptr, len) < 0) throw new Error(readJsonText());
      return readJsonText();
    }

    arpabet(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      if (w.tts_arpabet(h, ptr, len) < 0) throw new Error(readJsonText());
      return readJsonText();
    }

    normalize(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      if (w.tts_normalize(h, ptr, len) < 0) throw new Error(readJsonText());
      return readJsonText();
    }

    addWord(word, phones) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(`${word}\t${phones}`);
      if (w.tts_add_word(h, ptr, len) < 0) throw new Error(readJsonText());
    }

    loadDictionary(text) {
      const h = this.#checkHandle();
      const [ptr, len] = writeUtf8(String(text));
      const n = w.tts_load_dictionary(h, ptr, len);
      if (n < 0) throw new Error(readJsonText());
      return n;
    }

    free() {
      if (this.#handle) {
        if (instRegistry) instRegistry.unregister(this);
        w.tts_drop(this.#handle);
        this.#handle = 0;
      }
    }
  }

  function encodeWav(samples, sampleRate = META.sampleRate) {
    const arr = samples instanceof Float32Array ? samples : Float32Array.from(samples || []);
    const ptr = arr.length ? w.tts_alloc_f32(arr.length) : 0;
    if (arr.length) {
      new Float32Array(w.memory.buffer, ptr, arr.length).set(arr);
    }
    w.tts_encode_wav(ptr, arr.length, Math.max(1000, Math.floor(Number(sampleRate) || META.sampleRate)));
    return copyBytes();
  }

  return { TTS, SpeechStream, encodeWav };
}

if (typeof wasm !== "undefined" && wasm) {
  return bindTTS(wasm);
}
