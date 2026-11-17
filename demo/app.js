import init, { TTS, encodeWav } from "../pkg/tts.js";

const $ = (id) => document.getElementById(id);

const PROMPTS_SPEECH = [
  [
    "Welcome",
    "Hello! This voice is computed entirely in your browser from a physical model of the human vocal tract. Please pass a little water and butter — it can read numbers like 2026 or $5.99, ask rising questions, and switch between male and female speakers. Isn't that interesting?",
  ],
  [
    "Questions",
    "Did you see the new bridge they built last year? Where did they put the keys? Honestly, I can't believe how natural that sounded, can you?",
  ],
  [
    "Connected",
    "A little water and fresh butter make the batter much better than before. Kick the ball straight, and make sure Gregson sees the chicken.",
  ],
  [
    "Lists",
    "We need eggs, flour, butter, sugar, and a little milk. First open the box, then read the note, and finally call me back.",
  ],
  [
    "Numbers",
    "At 10:30 AM on 2026-11-16, the 16 GB module cost €5.50 — about 3/4 of Dr. Smith's estimate on Elm St.",
  ],
  [
    "Story",
    "When the rain finally stopped, the children ran outside to play in the puddles along the road. What a lovely evening it had been!",
  ],
];

const PROMPTS_SCRIPT = [
  [
    "Laughs and sounds",
    `[female, cheerful] You will never guess what happened at the meeting. [giggle]\n[deep, calm] [hmm] Go on.\n[female] The projector fell off the table, [gasp] and everyone just carried on talking! [laugh]\n[female, laughing] And then he tried to pick it up, and dropped it again!\n[deep] [chuckle] Of course he did. [sigh=1.2s] Mondays.\n[female, whisper] [shh] Here he comes.`,
  ],
  [
    "Two speakers",
    `[female, cheerful, speed=1.06] Hey! Have you heard how natural this vocal tract sounds now?\n[male, calm, speed=0.96] I just listened — no neural networks or audio samples at all?\n[bright, excited, speed=1.15] Zero! Every consonant, breath, and pitch rise is pure physics in WebAssembly.\n[pause=240ms]\n[deep, serious, speed=0.92] Wait... [whisper, softer] can an AI change my voice and speed mid-sentence too?\n[female, happy] It just did!`,
  ],
  [
    "Pace and emotion",
    `[woman_a, calm, speed=0.86] We can start slowly, warmly, and gently, taking our time on every word. [inhale] [excited, speed=1.22, pitch=+2st] Then suddenly speed right up with bright energy when something thrilling happens! [pause=280ms] [whisper, slow] Or drop to an intimate whisper... [woman_a, reset] before returning smoothly to normal.`,
  ],
  [
    "Roundtable",
    `[woman_a] Welcome back to our studio roundtable! Let's check in with everyone.\n[man_a, speed=1.08] Everything on the WebAssembly build is running over 400 times real-time.\n[woman_b, calm] And speaker profiles are just a handful of physical numbers — zero megabytes of weights.\n[man_b, serious] Ready to ship anywhere a browser runs.`,
  ],
  [
    "Breaks into song",
    `[male] Wait a second — can a rule-based vocal tract actually sing a melody?\n[female, happy] Watch the vocal folds and jaw track the notes!\n[soprano, sing bpm=112] C5:1=Shine D5:1=on E5:1.5=bright C5:0.5=star | G4:1=guide A4:1=us C5:2=home [/sing]\n[deep, excited] Okay, that was genuinely impressive!`,
  ],
];

const PROMPTS_SING = [
  {
    title: "Twinkle",
    view: "solo",
    bpm: 108,
    transpose: 0,
    soloNotes: "C4:1 C4:1 G4:1 G4:1 | A4:1 A4:1 G4:2 | F4:1 F4:1 E4:1 E4:1 | D4:1 D4:1 C4:2",
    soloLyrics: "Twinkle twinkle little star how I wonder what you are",
  },
  {
    title: "Aria",
    view: "solo",
    bpm: 96,
    transpose: 0,
    soloNotes: "G3:1 C4:1.5 D4:0.5 E4:1 G4:1 | F4:0.5 E4:0.5 D4:1 C4:2 | R:0.5 E4:1 F4:0.5 G4:1 A4:0.5 G4:0.5 | E4:1 D4:1 C4:2.5",
    soloLyrics: "Sing out ~ with pure joy | let the voice ring | far a- ~ cross ~ ~ | all the star-light",
  },
  {
    title: "Four-part chorus",
    view: "chorus",
    bpm: 102,
    transpose: 0,
    lyrics: "Shine on bright star through the night sky guide our way home",
    parts: [
      { active: true, name: "Lead",    voice: "current", gain: 1.0, notes: "E4:1.5 D4:0.5 C4:1 G3:1 | A3:1 C4:1 B3:2 | C4:1 D4:1 E4:1.5 C4:0.5 | D4:1 B3:1 C4:2.5" },
      { active: true, name: "Soprano", voice: "soprano", gain: 0.9, notes: "G4:1.5 F4:0.5 E4:1 E4:1 | F4:1 A4:1 G4:2 | G4:1 G4:1 G4:1.5 E4:0.5 | F4:1 D4:1 E4:2.5" },
      { active: true, name: "Tenor",   voice: "tenor",   gain: 0.85, notes: "C4:1.5 B3:0.5 G3:1 C4:1 | C4:1 F4:1 D4:2 | E4:1 B3:1 C4:1.5 G3:0.5 | A3:1 G3:1 G3:2.5" },
      { active: true, name: "Bass",    voice: "deep",    gain: 0.95, notes: "C3:1.5 G2:0.5 A2:1 E2:1 | F2:1 F2:1 G2:2 | C3:1 G2:1 A2:1.5 A2:0.5 | F2:1 G2:1 C3:2.5" },
    ],
  },
  {
    title: "Quartet",
    view: "chorus",
    bpm: 96,
    transpose: 0,
    lyrics: "Should old ac-quain-tance be for-got and ne-ver brought to mind",
    parts: [
      { active: true, name: "Lead",    voice: "current", gain: 1.0, notes: "C4:1 | F4:1.5 E4:0.5 F4:1 A4:1 | G4:1.5 F4:0.5 G4:1 A4:1 | F4:1.5 F4:0.5 A4:1 C5:1 | D5:3" },
      { active: true, name: "Alto",    voice: "woman_b", gain: 0.9, notes: "C4:1 | C4:1.5 C4:0.5 C4:1 F4:1 | E4:1.5 D4:0.5 E4:1 E4:1 | C4:1.5 C4:0.5 F4:1 F4:1 | F4:3" },
      { active: true, name: "Tenor",   voice: "tenor",   gain: 0.85, notes: "E3:1 | A3:1.5 G3:0.5 A3:1 C4:1 | C4:1.5 C4:0.5 C4:1 C4:1 | A3:1.5 A3:0.5 C4:1 A3:1 | Bb3:3" },
      { active: true, name: "Bass",    voice: "deep",    gain: 0.95, notes: "C3:1 | F2:1.5 C3:0.5 F2:1 F2:1 | C3:1.5 C3:0.5 C3:1 A2:1 | F2:1.5 F2:0.5 F2:1 F2:1 | Bb2:3" },
    ],
  },
];

const SINGER_VOICE_OPTIONS = [
  ["current", "Your voice"],
  ["soprano", "Soprano"],
  ["alto", "Alto"],
  ["bright", "Bright"],
  ["female", "Female"],
  ["woman_a", "US woman A"],
  ["woman_b", "US woman B"],
  ["tenor", "Tenor"],
  ["baritone", "Baritone"],
  ["male", "Male"],
  ["deep", "Deep bass"],
  ["man_a", "US man A"],
  ["man_b", "US man B"],
];

const SLIDERS_ESSENTIALS = [
  ["pitchHz", "Pitch", 60, 350, 1, " Hz"],
  ["tractLengthCm", "Vocal tract", 12.2, 21.5, 0.1, " cm"],
  ["speed", "Speaking rate", 0.55, 1.75, 0.01, "×"],
  ["expressiveness", "Expressiveness", 0, 2.5, 0.02, ""],
  ["warmth", "Warmth", 0, 1, 0.01, ""],
  ["breathiness", "Breathiness", 0, 0.85, 0.01, ""],
  ["crispness", "Consonant crispness", 0.45, 1.6, 0.01, ""],
  ["variation", "Unevenness", 0, 1, 0.01, ""],
  ["pauses", "Pause length", 0.3, 3, 0.05, "×"],
  ["breaths", "Sentence breaths", 0, 1, 0.01, ""],
  ["room", "Room ambience", 0, 1, 0.01, ""],
  ["volume", "Output volume", 0, 2, 0.01, ""],
  ["laughter", "Laughing while speaking", 0, 1, 0.05, ""],
];

const SLIDERS_PHYSICS = [
  ["closure", "Closure sharpness", 0, 1, 0.05, ""],
  ["openQuotient", "Open quotient", 0.5, 0.74, 0.01, ""],
  ["pitchCoupling", "Pitch coupling", 0, 1, 0.01, ""],
  ["subglottal", "Subglottal airway", 0, 1, 0.01, ""],
  ["chestResonance", "Chest body", 0, 1, 0.01, ""],
  ["roughness", "Roughness", 0, 0.75, 0.01, ""],
  ["nasality", "Nasality", 0, 0.8, 0.01, ""],
  ["vibrato", "Vibrato depth", 0, 1, 0.01, ""],
  ["air", "Air above 5 kHz", 0, 1, 0.01, ""],
  ["formants.0", "Vowel openness (F1)", 0.8, 1.25, 0.01, "×"],
  ["formants.1", "Tongue frontness (F2)", 0.8, 1.25, 0.01, "×"],
  ["formants.2", "Third formant (F3)", 0.8, 1.25, 0.01, "×"],
  ["formants.3,4", "Upper cluster (F4, F5)", 0.8, 1.25, 0.01, "×"],
];

const SLIDERS_TONE_MACRO = [
  ["bass", "Bass", -12, 12, 0.5, " dB"],
  ["presence", "Presence", -12, 12, 0.5, " dB"],
  ["brightness", "Brightness", -12, 12, 0.5, " dB"],
];

// [file, name, notes shown under the name, speaking pitch in Hz]
const PROFILES = [
  ["arctic_bdl", "US man A", ["113 Hz", "brisk"], 113],
  ["arctic_rms", "US man B", ["92 Hz", "deep"], 92],
  ["arctic_slt", "US woman A", ["186 Hz", "clear"], 186],
  ["arctic_clb", "US woman B", ["184 Hz", "warm"], 184],
];

const PRESET_META = {
  male: ["110 Hz", "17.0 cm"],
  deep: ["86 Hz", "18.8 cm"],
  husky: ["106 Hz", "raspy"],
  female: ["208 Hz", "14.2 cm"],
  bright: ["222 Hz", "lively"],
  whisper: ["no voicing"],
};

// Lettering drawn on the plates (the same faces the page uses).
const UI_FONT = 'Bahnschrift, "DIN Alternate", "Avenir Next", "Segoe UI", system-ui, sans-serif';
const IPA_FONT = '"Charis SIL", "Doulos SIL", "Gentium Plus", Cambria, "Palatino Linotype", "Times New Roman", serif';

// Plate palette: film base, bone and soft tissue as a radiograph shows them, and three
// meanings - rose is tissue that articulates, gold is voicing, ice is moving air.
const FILM = {
  base: "#0b141c",
  deep: "#070d13",
  grid: "rgba(196, 220, 238, 0.06)",
  line: "rgba(196, 220, 238, 0.14)",
  bone: "#dbe7f0",
  label: "rgba(203, 221, 235, 0.6)",
  rose: "#f0668a",
  gold: "#f2b84b",
  ice: "#bfe3f5",
};

// Position (0..1) of a pitch between the lowest and highest speaking voices, by octaves.
function pitchPosition(hz) {
  return Math.max(0.02, Math.min(0.98, Math.log2(hz / 70) / Math.log2(280 / 70)));
}

function voiceCardHtml(name, notes, pitchHz) {
  const meta = notes.map((n) => `<span>${n}</span>`).join("");
  const marker = pitchHz ? `<span class="vc-pitch" style="--p:${pitchPosition(pitchHz).toFixed(3)}" title="${Math.round(pitchHz)} Hz"></span>` : "";
  return `<span class="vc-name">${name}</span><span class="vc-meta">${meta}</span>${marker}`;
}

// Base acoustic positions (normalized 0..1 in F2 front->back and F1 close->open) & carrier words for the 16 vowels
const VOWEL_INFO = {
  IY: { ipa: "iː", word: "heed", x: 0.12, y: 0.14 },
  IH: { ipa: "ɪ", word: "hid", x: 0.24, y: 0.28 },
  EY: { ipa: "eɪ", word: "hay", x: 0.20, y: 0.40 },
  EH: { ipa: "ɛ", word: "head", x: 0.30, y: 0.54 },
  AE: { ipa: "æ", word: "had", x: 0.28, y: 0.76 },
  AA: { ipa: "ɑː", word: "hod", x: 0.68, y: 0.86 },
  AO: { ipa: "ɔː", word: "hawed", x: 0.84, y: 0.68 },
  OW: { ipa: "oʊ", word: "hoe", x: 0.82, y: 0.46 },
  UH: { ipa: "ʊ", word: "hood", x: 0.74, y: 0.30 },
  UW: { ipa: "uː", word: "who'd", x: 0.80, y: 0.15 },
  AH: { ipa: "ʌ", word: "hud", x: 0.58, y: 0.66 },
  AX: { ipa: "ə", word: "sofa", x: 0.50, y: 0.48 },
  ER: { ipa: "ɝ", word: "heard", x: 0.54, y: 0.36 },
  AY: { ipa: "aɪ", word: "hide", x: 0.52, y: 0.80 },
  AW: { ipa: "aʊ", word: "how", x: 0.62, y: 0.82 },
  OY: { ipa: "ɔɪ", word: "boy", x: 0.76, y: 0.58 },
};

let voice;
let audioCtx;
let sourceNode = null;
let lastSpeech = null;
let rafId = 0;
let playStartedAt = 0;
let playOffsetSec = 0;
let isPlaying = false;
let activeCardId = "male";
let baseSettingsSnapshot = null;
let phoneticMode = "ipa"; // "ipa" | "arpabet"
let studioMode = "speech"; // "speech" | "script" | "sing"
let singSubView = "solo"; // "solo" | "chorus" | "raw"
let chorusPartsState = structuredClone(PROMPTS_SING[2].parts);
let pinnedVoiceCounter = 0;
let focusedNotesInput = null;
let savedSpeechText = PROMPTS_SPEECH[0][1];
let savedScriptText = PROMPTS_SCRIPT[0][1];
let vizMode = "spec"; // "spec" | "wave"
let animMode = "split"; // "split" | "face" | "tract"
let showFormants = true;
let scrubTimeSec = -1;
let cachedAcousticBitmap = null;
let cachedF0Track = null;
let autoSpeakTimer = 0;
let compareQueue = null;
let selectedVowel = "IY";
let stickySegment = null;
const taughtWords = new Map();

function clearStickySegment() {
  stickySegment = null;
  anim.activeGender = null;
  anim.activeTract = null;
  anim.activeBasePitch = null;
  anim.activeVoiceLabel = "";
  anim.activeSinging = false;
}

// Continuous 60/120 fps articulatory & expressive avatar state
const anim = {
  jaw: 0.06,
  tH: 0.46,
  tF: 0.50,
  tTip: 0.16,
  lipR: 0.04,
  lipS: 0.04,
  lipC: 0.18,
  velum: 0.0,
  energy: 0.0,
  voicing: 0.0,
  noise: 0.0,
  f0: 110,
  f1: 500,
  f2: 1500,
  f3: 2500,
  pitchSt: 0,
  phone: "",
  ipa: "",
  activeGender: null,
  activeTract: null,
  activeBasePitch: null,
  activeVoiceLabel: "",
  activeSinging: false,
  glotPhase: 0,
  wavePhase: 0,
  lastTickMs: 0,
  blinkStartMs: -1000,
  nextBlinkMs: 1800,
  gazeX: 0,
  gazeY: 0,
  targetGazeX: 0,
  targetGazeY: 0,
  nextGazeMs: 1200,
};

const AIR_PARTICLES = Array.from({ length: 44 }, () => ({
  active: false,
  x: 0,
  y: 0,
  vx: 0,
  vy: 0,
  life: 0,
  maxLife: 1,
  size: 1.5,
}));

// Theme state
const THEMES = ["auto", "light", "dark"];
let themeIdx = 0;
function cycleTheme() {
  themeIdx = (themeIdx + 1) % THEMES.length;
  const t = THEMES[themeIdx];
  if (t === "auto") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", t);
  $("themeBtn").textContent = `Theme: ${t}`;
  drawVowelSpace();
}

// Helpers for reading/writing nested settings
function readSetting(settings, key) {
  const [name, idx] = key.split(".");
  return idx === undefined ? settings[name] : settings[name][Number(idx.split(",")[0])];
}

function writeSetting(key, value) {
  const [name, idx] = key.split(".");
  if (idx === undefined) {
    voice.configure({ [name]: value });
  } else {
    const values = Array.from(voice.settings[name]);
    for (const i of idx.split(",")) values[Number(i)] = value;
    voice.configure({ [name]: values });
  }
}

function formatVal(val, step, unit) {
  // as many decimals as the step needs (0.05 steps show 0.75, not 0.8), without trailing zeros
  const decimals = step < 0.1 ? 2 : step < 1 ? 1 : 0;
  const num = Number(val).toFixed(decimals).replace(/\.0+$/, "").replace(/(\.\d)0$/, "$1");
  return `${val > 0 && unit === " dB" ? "+" : ""}${num}${unit}`;
}

function scheduleAutoSpeak() {
  if (!$("autoSpeakCheck").checked) return;
  clearTimeout(autoSpeakTimer);
  autoSpeakTimer = setTimeout(() => speak(0), 220);
}

// How far along its track a range input is (0..1), for a given value or its own.
function rangeFraction(input, value = Number(input.value)) {
  const min = Number(input.min);
  const max = Number(input.max);
  return max > min ? Math.max(0, Math.min(1, (value - min) / (max - min))) : 0;
}

// Fills a slider's track up to its thumb (WebKit has no native way). A control that runs
// either side of zero (dB gains) is filled from zero, so boost and cut read differently.
function paintRange(input) {
  const at = rangeFraction(input) * 100;
  const zero = Number(input.min) < 0 && Number(input.max) > 0 ? rangeFraction(input, 0) * 100 : 0;
  input.style.setProperty("--lo", `${Math.min(at, zero).toFixed(2)}%`);
  input.style.setProperty("--hi", `${Math.max(at, zero).toFixed(2)}%`);
}

// Build sliders for a container. One entry per control:
//   [setting key, label, min, max, step, unit suffix]
// A key may address array members: "formants.0", or "formants.3,4" to move several together.
function populateSliders(containerId, defs) {
  const host = $(containerId);
  for (const [key, label, min, max, step, unit] of defs) {
    const wrap = document.createElement("label");
    wrap.className = "slider";
    wrap.innerHTML = `<span class="s-name">${label}</span><output></output><span class="s-track"><input class="rng" type="range" min="${min}" max="${max}" step="${step}"></span>`;
    const input = wrap.querySelector("input");
    const out = wrap.querySelector("output");
    input.dataset.key = key;
    input.dataset.step = String(step);
    input.dataset.unit = unit;
    input.addEventListener("input", () => {
      const v = Number(input.value);
      writeSetting(key, v);
      out.textContent = formatVal(v, step, unit);
      paintRange(input);
      markCustomCard();
      scheduleAutoSpeak();
    });
    input.addEventListener("dblclick", () => {
      if (!baseSettingsSnapshot) return;
      const defVal = readSetting(baseSettingsSnapshot, key);
      input.value = defVal;
      writeSetting(key, defVal);
      out.textContent = formatVal(defVal, step, unit);
      paintRange(input);
      scheduleAutoSpeak();
    });
    host.append(wrap);
  }
}

// Build 10-band EQ faders
function buildEqBars() {
  const bands = TTS.toneBandsHz();
  const host = $("eqBars");
  bands.forEach((hz, idx) => {
    const col = document.createElement("div");
    col.className = "eq-col";
    const hzText = hz >= 1000 ? `${(hz / 1000).toFixed(hz % 1000 ? 1 : 0)}k` : String(hz);
    col.innerHTML = `
      <output id="eqOut${idx}">0</output>
      <div class="eq-vslider-wrap">
        <input class="rng" type="range" min="-18" max="18" step="0.5" value="0" data-band="${idx}" aria-label="${hz} Hz" title="${hz} Hz (double-click for 0 dB)">
      </div>
      <span class="eq-hz">${hzText}</span>
    `;
    const input = col.querySelector("input");
    const out = col.querySelector("output");
    input.addEventListener("input", () => {
      const tone = Array.from(voice.settings.tone);
      tone[idx] = Number(input.value);
      voice.configure({ tone });
      out.textContent = `${tone[idx] > 0 ? "+" : ""}${tone[idx]}`;
      paintRange(input);
      markCustomCard();
      scheduleAutoSpeak();
    });
    input.addEventListener("dblclick", () => {
      const tone = Array.from(voice.settings.tone);
      tone[idx] = 0;
      voice.configure({ tone });
      input.value = 0;
      out.textContent = "0";
      paintRange(input);
      scheduleAutoSpeak();
    });
    paintRange(input);
    host.append(col);
  });
}

// Build prompt chips & studio mode composer
function buildPromptChips() {
  const host = $("promptChips");
  host.replaceChildren();
  if (studioMode === "sing") {
    PROMPTS_SING.forEach((preset, idx) => {
      const btn = document.createElement("button");
      btn.type = "button";
      btn.className = `chip${idx === 0 ? " active" : ""}`;
      btn.textContent = preset.title;
      btn.onclick = () => {
        for (const c of host.children) c.classList.remove("active");
        btn.classList.add("active");
        loadSingPreset(preset);
        speak(0);
      };
      host.append(btn);
    });
    return;
  }

  const list = studioMode === "script" ? PROMPTS_SCRIPT : PROMPTS_SPEECH;
  list.forEach(([title, text], idx) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `chip${idx === 0 ? " active" : ""}`;
    btn.textContent = title;
    btn.onclick = () => {
      for (const c of host.children) c.classList.remove("active");
      btn.classList.add("active");
      $("text").value = text;
      if (studioMode === "script") savedScriptText = text;
      else savedSpeechText = text;
      updateLivePhonetics();
      updateApiSnippet();
      speak(0);
    };
    host.append(btn);
  });
}

function setSingSubView(view) {
  const prev = singSubView;
  singSubView = view;
  $("singViewChorusBtn").classList.toggle("active", view === "chorus");
  $("singViewSoloBtn").classList.toggle("active", view === "solo");
  $("singViewRawBtn").classList.toggle("active", view === "raw");
  $("chorusEditorPane").hidden = view !== "chorus";
  $("soloEditorPane").hidden = view !== "solo";
  $("rawScorePane").hidden = view !== "raw";
  if (view === "raw" && prev !== "raw") {
    $("rawScoreInput").value = prev === "solo"
      ? buildSoloDslFromState()
      : buildChorusDslFromState();
  }
  updateLivePhonetics();
  updateApiSnippet();
}

function buildSoloDslFromState() {
  const bpm = Number($("singBpmInput").value) || 104;
  const tr = Number($("singTransposeInput").value) || 0;
  const lyr = $("soloLyricsInput").value.trim().replace(/"/g, "");
  const notes = $("soloNotesInput").value.trim();
  return `[bpm=${bpm}${tr ? ` transpose=${tr}` : ""} lyrics="${lyr}"]\n${notes}`;
}

function loadSingPreset(preset) {
  $("singBpmInput").value = preset.bpm || 104;
  $("singTransposeInput").value = preset.transpose || 0;
  if (preset.view === "solo") {
    // Keep the user's configured Studio Voice untouched so any custom or random voice can sing!
    $("soloNotesInput").value = preset.soloNotes;
    $("soloLyricsInput").value = preset.soloLyrics;
    $("rawScoreInput").value = buildSoloDslFromState();
    setSingSubView("solo");
  } else {
    $("chorusLyricsInput").value = preset.lyrics;
    chorusPartsState = structuredClone(preset.parts);
    renderChorusTracks();
    $("rawScoreInput").value = buildChorusDslFromState();
    setSingSubView("chorus");
  }
  updateApiSnippet();
}

function buildChorusDslFromState() {
  const bpm = Number($("singBpmInput").value) || 104;
  const tr = Number($("singTransposeInput").value) || 0;
  const lyr = $("chorusLyricsInput").value.trim().replace(/"/g, "");
  const lines = [`[bpm=${bpm} lyrics="${lyr}"]`];
  for (const p of chorusPartsState) {
    if (!p.active) continue;
    const voiceAttr = p.voice && p.voice !== "current" ? ` voice=${p.voice}` : "";
    lines.push(`[part name="${p.name}"${voiceAttr} gain=${p.gain.toFixed(2)}${tr ? ` transpose=${tr}` : ""}]`);
    lines.push(p.notes.trim());
  }
  return lines.join("\n");
}

function renderChorusTracks() {
  const host = $("chorusTracksHost");
  host.replaceChildren();
  chorusPartsState.forEach((part, idx) => {
    const row = document.createElement("div");
    row.className = `chorus-row${part.active ? "" : " muted"}`;

    const chk = document.createElement("input");
    chk.type = "checkbox";
    chk.checked = part.active;
    chk.title = "Include this part";
    chk.onchange = () => {
      part.active = chk.checked;
      row.classList.toggle("muted", !part.active);
      updateApiSnippet();
      scheduleAutoSpeak();
    };

    const nameIn = document.createElement("input");
    nameIn.type = "text";
    nameIn.value = part.name;
    nameIn.title = "Part name";
    nameIn.oninput = () => {
      part.name = nameIn.value;
      updateApiSnippet();
    };

    const voiceSel = document.createElement("select");
    voiceSel.title = "Who sings this part";
    for (const [vid, vlabel] of SINGER_VOICE_OPTIONS) {
      voiceSel.append(new Option(vlabel, vid, false, vid === part.voice));
    }
    voiceSel.onchange = () => {
      part.voice = voiceSel.value;
      updateApiSnippet();
      scheduleAutoSpeak();
    };

    const notesIn = document.createElement("input");
    notesIn.type = "text";
    notesIn.className = "c-notes";
    notesIn.value = part.notes;
    notesIn.placeholder = "C4:1 E4:1 G4:2";
    notesIn.onfocus = () => {
      for (const r of host.querySelectorAll(".chorus-row")) r.classList.remove("focused");
      row.classList.add("focused");
      focusedNotesInput = { el: notesIn, part };
    };
    notesIn.oninput = () => {
      part.notes = notesIn.value;
      updateApiSnippet();
    };

    const gainIn = document.createElement("input");
    gainIn.type = "range";
    gainIn.className = "c-gain rng";
    gainIn.min = "0.2";
    gainIn.max = "1.5";
    gainIn.step = "0.05";
    gainIn.value = String(part.gain);
    gainIn.title = `Level of this part (${Math.round(part.gain * 100)}%)`;
    paintRange(gainIn);
    gainIn.oninput = () => {
      part.gain = Number(gainIn.value);
      gainIn.title = `Level of this part (${Math.round(part.gain * 100)}%)`;
      paintRange(gainIn);
      scheduleAutoSpeak();
    };

    const delBtn = document.createElement("button");
    delBtn.type = "button";
    delBtn.className = "quiet sm";
    delBtn.textContent = "×";
    delBtn.title = "Remove this part";
    delBtn.setAttribute("aria-label", "Remove this part");
    delBtn.disabled = chorusPartsState.length <= 1;
    delBtn.onclick = () => {
      if (chorusPartsState.length <= 1) return;
      chorusPartsState.splice(idx, 1);
      renderChorusTracks();
      updateApiSnippet();
      scheduleAutoSpeak();
    };

    row.append(chk, nameIn, voiceSel, notesIn, gainIn, delBtn);
    host.append(row);
    if (idx === 0 && !focusedNotesInput) {
      focusedNotesInput = { el: notesIn, part };
    }
  });
}

function buildPianoStrip() {
  const host = $("pianoStrip");
  if (!host) return;
  const keys = [
    "C3", "D3", "E3", "F3", "G3", "A3", "B3",
    "C4", "C#4", "D4", "Eb4", "E4", "F4", "F#4", "G4", "Ab4", "A4", "Bb4", "B4",
    "C5", "D5", "E5", "F5", "G5",
  ];
  for (const note of keys) {
    const btn = document.createElement("button");
    btn.type = "button";
    const isSharp = note.includes("#") || note.includes("b");
    btn.className = `pkey${isSharp ? " sharp" : ""}`;
    btn.textContent = note;
    btn.title = `${note}, ${Math.round(TTS.noteToHz(note))} Hz: hear it and add it to the melody`;
    btn.onclick = () => {
      const beats = $("noteDurSelect").value || "1";
      insertMusicToken(`${note}:${beats}`);
      auditionSingleNote(note);
    };
    host.append(btn);
  }
  for (const [tok, label, tip] of [
    ["R", "Rest", "Add a rest of the chosen length"],
    ["~", "Hold ~", "Carry the previous vowel over onto the next note"],
  ]) {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "pkey util";
    btn.textContent = label;
    btn.title = tip;
    btn.onclick = () => {
      const beats = $("noteDurSelect").value || "1";
      if (tok === "R") insertMusicToken(`R:${beats}`);
      else if (singSubView === "solo") {
        $("soloLyricsInput").value = `${$("soloLyricsInput").value.trim()} ~`;
      } else if (singSubView === "chorus") {
        $("chorusLyricsInput").value = `${$("chorusLyricsInput").value.trim()} ~`;
      }
      updateApiSnippet();
    };
    host.append(btn);
  }
}

function insertMusicToken(token) {
  if (singSubView === "solo") {
    const el = $("soloNotesInput");
    el.value = `${el.value.trim()} ${token}`.trim();
  } else if (singSubView === "raw") {
    const el = $("rawScoreInput");
    el.value = `${el.value.trim()} ${token}=la`.trim();
  } else if (focusedNotesInput && focusedNotesInput.el) {
    focusedNotesInput.el.value = `${focusedNotesInput.el.value.trim()} ${token}`.trim();
    focusedNotesInput.part.notes = focusedNotesInput.el.value;
  }
  updateApiSnippet();
}

function auditionSingleNote(note) {
  try {
    stopPlayback(true);
    audioCtx ??= new AudioContext();
    audioCtx.resume();
    const partVoice = singSubView === "chorus" ? focusedNotesInput?.part?.voice : "current";
    const opts = partVoice && partVoice !== "current"
      ? { voice: partVoice, bpm: 125, score: `${note}:0.65=la` }
      : { bpm: 125, score: `${note}:0.65=la` };
    const sp = voice.sing(opts);
    if (sp && sp.samples.length) {
      lastSpeech = sp;
      buildAcousticCache(sp);
      playBufferFrom(sp, 0);
    }
  } catch {}
}

const IS_MAC = /Mac|iPhone|iPad/.test(navigator.platform || "");

function readyText() {
  return `Ready. Press ${$("speakLabel").textContent}, or ${IS_MAC ? "⌘" : "Ctrl"}+Enter while typing.`;
}

function setStudioMode(mode) {
  if (studioMode === "speech") savedSpeechText = $("text").value;
  else if (studioMode === "script") savedScriptText = $("text").value;

  studioMode = mode;
  $("modeSpeechBtn").classList.toggle("active", mode === "speech");
  $("modeScriptBtn").classList.toggle("active", mode === "script");
  $("modeSingBtn").classList.toggle("active", mode === "sing");

  $("scriptToolbar").hidden = mode !== "script";
  $("soundBar").hidden = mode !== "script";
  $("textComposerWrap").hidden = mode === "sing";
  $("singComposerWrap").hidden = mode !== "sing";

  $("speakLabel").textContent = mode === "sing" ? "Sing" : mode === "script" ? "Perform" : "Speak";
  if ($("status").textContent.startsWith("Ready.")) $("status").textContent = readyText();

  if (mode === "speech") {
    $("text").value = savedSpeechText;
    if (!/\[[a-zA-Z_/]/.test(savedSpeechText)) {
      clearStickySegment();
    }
  } else if (mode === "script") {
    $("text").value = savedScriptText;
  }
  buildPromptChips();
  updateLivePhonetics();
  updateApiSnippet();
}

function buildVoiceCards() {
  const select = $("preset");
  select.append(new Option("Custom", ""));
  const builtinHost = $("builtinCards");
  for (const p of TTS.presets()) {
    select.append(new Option(p.label, p.id));
    const card = document.createElement("button");
    card.type = "button";
    card.className = "voice-card";
    card.dataset.cardId = p.id;
    card.innerHTML = voiceCardHtml(p.label, PRESET_META[p.id] || [`${Math.round(p.pitchHz)} Hz`], p.id === "whisper" ? 0 : p.pitchHz);
    card.onclick = () => selectBuiltinPreset(p.id);
    builtinHost.append(card);
  }

  const profileHost = $("profileCards");
  for (const [id, title, notes, pitchHz] of PROFILES) {
    const cardId = `profile:${id}`;
    select.append(new Option(title, cardId));
    const card = document.createElement("button");
    card.type = "button";
    card.className = "voice-card";
    card.dataset.cardId = cardId;
    card.innerHTML = voiceCardHtml(title, notes, pitchHz);
    card.onclick = () => loadBuiltinProfile(id, title, cardId);
    profileHost.append(card);
  }
}

function markCustomCard() {
  $("preset").value = "";
  for (const el of document.querySelectorAll(".voice-card")) {
    el.classList.toggle("active", false);
  }
}

function highlightActiveCard(cardId) {
  activeCardId = cardId;
  $("preset").value = cardId;
  for (const el of document.querySelectorAll(".voice-card")) {
    el.classList.toggle("active", el.dataset.cardId === cardId);
  }
}

function selectBuiltinPreset(id) {
  clearStickySegment();
  voice.configure(id);
  baseSettingsSnapshot = structuredCloneSettings(voice.settings);
  syncControls(id);
  $("profileStatus").textContent = `${id[0].toUpperCase()}${id.slice(1)} voice, with ${voice.settings.folds ? "simulated vocal folds" : "the equation pulse"}.`;
  scheduleAutoSpeak();
}

async function loadBuiltinProfile(id, label, cardId) {
  try {
    const res = await fetch(`../profiles/${id}.json`);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const profile = await res.json();
    applyProfileObject(profile, label, cardId);
    scheduleAutoSpeak();
  } catch (err) {
    $("profileStatus").textContent = `That voice could not be loaded (${err.message}).`;
  }
}

function structuredCloneSettings(s) {
  return JSON.parse(JSON.stringify(s));
}

function applyProfileObject(profile, label, cardId = "") {
  if (profile.gender !== "male" && profile.gender !== "female") {
    throw new Error('Profile requires "gender": "male" or "female"');
  }
  clearStickySegment();
  const { volume, variation, seed, folds } = voice.settings;
  voice.configure(profile.gender);
  voice.configure({ ...profile, volume, variation, seed, folds: profile.folds ?? folds });
  baseSettingsSnapshot = structuredCloneSettings(voice.settings);
  syncControls(cardId);
  $("profileStatus").textContent = `${label}: settings fitted to a real speaker. No audio of them is used.`;
}

function syncControls(cardId) {
  const s = voice.settings;
  for (const input of document.querySelectorAll("label.slider input[type='range']")) {
    const val = readSetting(s, input.dataset.key);
    input.value = val;
    const row = input.closest("label.slider");
    row.querySelector("output").textContent = formatVal(val, Number(input.dataset.step), input.dataset.unit);
    paintRange(input);
    // the tick under the track: this voice's own value
    const own = baseSettingsSnapshot ? readSetting(baseSettingsSnapshot, input.dataset.key) : val;
    row.querySelector(".s-track").style.setProperty("--def", rangeFraction(input, Number(own)).toFixed(4));
  }
  s.tone.forEach((g, idx) => {
    const input = document.querySelector(`#eqBars input[data-band="${idx}"]`);
    const out = $(`eqOut${idx}`);
    if (input && out) {
      input.value = g;
      out.textContent = `${g > 0 ? "+" : ""}${Math.round(g * 10) / 10}`;
      paintRange(input);
    }
  });
  $("whisper").checked = s.whisper;
  $("folds").checked = s.folds;
  $("seedInput").value = s.seed;
  $("srcEquationBtn").classList.toggle("active", !s.folds);
  $("srcFoldsBtn").classList.toggle("active", s.folds);
  $("male").setAttribute("aria-pressed", String(s.gender === "male"));
  $("female").setAttribute("aria-pressed", String(s.gender === "female"));
  if (cardId !== undefined) highlightActiveCard(cardId);
  drawVowelSpace();
  updateApiSnippet();
}

// Live text normalization + IPA / ARPAbet preview
function updateLivePhonetics() {
  if (!voice) return;
  if (studioMode === "sing") {
    const lyr = singSubView === "solo" ? $("soloLyricsInput").value : $("chorusLyricsInput").value;
    const activeCount = chorusPartsState.filter((p) => p.active).length;
    $("normPreview").textContent = singSubView === "chorus"
      ? `${activeCount} part${activeCount === 1 ? "" : "s"} at ${$("singBpmInput").value} bpm`
      : `Solo at ${$("singBpmInput").value} bpm`;
    $("ipa").classList.remove("arpa");
    $("ipa").textContent = lyr.trim() ? voice.phonemize(lyr.replace(/[~|-]/g, " ")) : "";
    return;
  }
  const raw = $("text").value;
  if (!raw.trim()) {
    $("ipa").textContent = "";
    $("normPreview").textContent = "0 words";
    return;
  }
  const stripped = raw.replace(/\[[^\]]+\]/g, " ").replace(/\s+/g, " ").trim();
  const norm = voice.normalize(stripped || raw);
  const wordCount = norm.trim() ? norm.trim().split(/\s+/).length : 0;
  $("normPreview").textContent = `${wordCount} word${wordCount === 1 ? "" : "s"} when read out`;
  $("ipa").classList.toggle("arpa", phoneticMode !== "ipa");
  $("ipa").textContent = phoneticMode === "ipa" ? voice.phonemize(stripped || raw) : voice.arpabet(stripped || raw);
}

function updateApiSnippet() {
  const box = $("apiSnippetBox");
  if (!box) return;
  const loaderHeader = `// tts.wasm is the whole engine: its JavaScript wrapper is stored inside it.
const { instance: { exports: w } } = await WebAssembly.instantiateStreaming(fetch("./tts.wasm"));
const { TTS, encodeWav } = Function("wasm", new TextDecoder().decode(
  new Uint8Array(w.memory.buffer, w.tts_glue_ptr(), w.tts_glue_len())
))(w);`;

  if (studioMode === "sing") {
    if (singSubView === "solo") {
      const n = $("soloNotesInput").value.trim().replace(/\s+/g, " ");
      const l = $("soloLyricsInput").value.trim().replace(/"/g, '\\"');
      const bpm = Number($("singBpmInput").value) || 104;
      box.textContent = `<script type="module">
${loaderHeader}

const voice = new TTS(); // or new TTS("female") / new TTS(TTS.randomVoice())
await voice.playSong("${n}", "${l}", ${bpm});
</script>`;
    } else {
      const bpm = Number($("singBpmInput").value) || 104;
      const lyr = $("chorusLyricsInput").value.trim().replace(/"/g, '\\"');
      const partsCode = chorusPartsState
        .filter((p) => p.active)
        .map((p) => {
          const vProp = p.voice && p.voice !== "current" ? `voice: "${p.voice}", ` : "";
          return `    { name: "${p.name}", ${vProp}gain: ${p.gain}, notes: "${p.notes.trim()}", lyrics }`;
        })
        .join(",\n");
      box.textContent = `<script type="module">
${loaderHeader}

const voice = new TTS();
const lyrics = "${lyr}";
await voice.playChorus({
  bpm: ${bpm},
  parts: [
${partsCode}
  ]
});
</script>`;
    }
  } else if (studioMode === "script") {
    const sampleScript = $("text").value.trim().replace(/`/g, "\\`");
    box.textContent = `<script type="module">
${loaderHeader}

const voice = new TTS();

// Invent named characters (fix what matters, the rest is drawn at random):
voice.randomize({ name: "hero", gender: "female", speed: 1.06 });
voice.randomize({ name: "guide", gender: "male", pitchHz: 94 });

// Perform the script; [hero] and [guide] now work as voice tags in it:
await voice.playScript(\`${sampleScript}\`);
</script>`;
  } else {
    const said = JSON.stringify($("text").value.trim() || "Hello!");
    const who = voice && voice.settings.gender === "female" ? '"female"' : "";
    box.textContent = `<script type="module">
${loaderHeader}

const voice = new TTS(${who}); // or a preset id, or a settings object
await voice.play(${said});

// Invent a voice (fix what matters, the rest is drawn at random) and reuse it:
const custom = TTS.randomVoice({ gender: "female", warmth: 0.7 });
await new TTS(custom).play("The same settings always give the same voice.");
</script>`;
  }
}

// ============================================================================
// Fast Radix-2 FFT Spectrogram + Autocorrelation F0 Pitch Track
// ============================================================================
function fftRadix2(re, im) {
  const n = re.length;
  for (let i = 1, j = 0; i < n; i++) {
    let bit = n >> 1;
    for (; j & bit; bit >>= 1) j ^= bit;
    j ^= bit;
    if (i < j) {
      let t = re[i]; re[i] = re[j]; re[j] = t;
      t = im[i]; im[i] = im[j]; im[j] = t;
    }
  }
  for (let len = 2; len <= n; len <<= 1) {
    const ang = (-2 * Math.PI) / len;
    const wlenRe = Math.cos(ang);
    const wlenIm = Math.sin(ang);
    for (let i = 0; i < n; i += len) {
      let wRe = 1;
      let wIm = 0;
      const half = len >> 1;
      for (let j = 0; j < half; j++) {
        const uRe = re[i + j];
        const uIm = im[i + j];
        const vRe = re[i + j + half] * wRe - im[i + j + half] * wIm;
        const vIm = re[i + j + half] * wIm + im[i + j + half] * wRe;
        re[i + j] = uRe + vRe;
        im[i + j] = uIm + vIm;
        re[i + j + half] = uRe - vRe;
        im[i + j + half] = uIm - vIm;
        const nextRe = wRe * wlenRe - wIm * wlenIm;
        wIm = wRe * wlenIm + wIm * wlenRe;
        wRe = nextRe;
      }
    }
  }
}

// Spectrogram ramp: film base through steel blue to near white, the way a radiograph shows
// density. One hue, so the coloured traces drawn over it (pitch, formants) stay readable.
const FILM_RAMP = [
  [0.0, 11, 20, 28],
  [0.22, 19, 42, 60],
  [0.5, 54, 102, 138],
  [0.76, 150, 192, 216],
  [1.0, 246, 251, 254],
];

function filmColor(t) {
  const x = Math.max(0, Math.min(1, t));
  for (let i = 1; i < FILM_RAMP.length; i++) {
    const b = FILM_RAMP[i];
    if (x <= b[0]) {
      const a = FILM_RAMP[i - 1];
      const u = (x - a[0]) / (b[0] - a[0]);
      return [a[1] + (b[1] - a[1]) * u, a[2] + (b[2] - a[2]) * u, a[3] + (b[3] - a[3]) * u];
    }
  }
  return [246, 251, 254];
}

function computePitchTrack(samples, sr, gender) {
  const hop = Math.round(sr * 0.01); // 10 ms
  const win = Math.round(sr * 0.032);
  const fMin = gender === "female" ? 110 : 62;
  const fMax = gender === "female" ? 440 : 290;
  const lagMin = Math.floor(sr / fMax);
  const lagMax = Math.ceil(sr / fMin);
  const count = Math.max(1, Math.floor((samples.length - win) / hop));
  const out = new Float32Array(count);
  let maxE = 1e-9;
  const energies = new Float32Array(count);
  for (let k = 0; k < count; k++) {
    const off = k * hop;
    let e = 0;
    for (let i = 0; i < win; i += 2) e += samples[off + i] * samples[off + i];
    energies[k] = e;
    if (e > maxE) maxE = e;
  }
  for (let k = 0; k < count; k++) {
    if (energies[k] < maxE * 0.012) continue;
    const off = k * hop;
    let bestLag = 0;
    let bestCorr = 0;
    for (let lag = lagMin; lag <= lagMax; lag += 1) {
      let corr = 0;
      let normA = 0;
      let normB = 0;
      for (let i = 0; i < win - lag; i += 3) {
        const a = samples[off + i];
        const b = samples[off + i + lag];
        corr += a * b;
        normA += a * a;
        normB += b * b;
      }
      const r = (corr / Math.sqrt(normA * normB + 1e-12)) * (1 - 0.12 * ((lag - lagMin) / (lagMax - lagMin)));
      if (r > bestCorr) {
        bestCorr = r;
        bestLag = lag;
      }
    }
    if (bestCorr > 0.52 && bestLag > 0) out[k] = sr / bestLag;
  }
  return { track: out, fMin, fMax };
}

function buildAcousticCache(speech) {
  const canvas = $("waveCanvas");
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(320, Math.floor(canvas.clientWidth * dpr));
  const h = Math.max(160, Math.floor(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const off = document.createElement("canvas");
  off.width = w;
  off.height = h;
  const ctx = off.getContext("2d");
  ctx.fillStyle = FILM.base;
  ctx.fillRect(0, 0, w, h);
  // Traces get finer when many seconds share few pixels (a long text, a narrow screen)
  const crowd = Math.max(0.6, Math.min(1, (0.03 * Math.max(1, canvas.clientWidth)) / Math.max(0.5, speech.duration)));
  const lw = dpr * crowd;

  const samples = speech.samples;
  const sr = speech.sampleRate;
  if (!samples || samples.length < 256) {
    cachedAcousticBitmap = off;
    return;
  }

  if (vizMode === "spec") {
    const nfft = 512;
    const maxBin = Math.min(nfft >> 1, Math.floor((8000 / sr) * nfft));
    const cols = Math.min(w, 420);
    const specImg = document.createElement("canvas");
    specImg.width = cols;
    specImg.height = maxBin;
    const sctx = specImg.getContext("2d");
    const img = sctx.createImageData(cols, maxBin);
    const win = new Float32Array(nfft);
    for (let i = 0; i < nfft; i++) win[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * i) / (nfft - 1));

    const re = new Float32Array(nfft);
    const im = new Float32Array(nfft);
    const mags = new Float32Array(cols * maxBin);
    let peakDb = -100;

    for (let x = 0; x < cols; x++) {
      const center = Math.floor((x / Math.max(1, cols - 1)) * Math.max(0, samples.length - nfft));
      for (let i = 0; i < nfft; i++) {
        // +6 dB/oct pre-emphasis makes upper formants & bursts crisp
        const cur = samples[center + i] || 0;
        const prev = i > 0 ? samples[center + i - 1] || 0 : cur;
        re[i] = (cur - 0.92 * prev) * win[i];
        im[i] = 0;
      }
      fftRadix2(re, im);
      for (let b = 0; b < maxBin; b++) {
        const db = 10 * Math.log10(re[b] * re[b] + im[b] * im[b] + 1e-12);
        mags[x * maxBin + b] = db;
        if (db > peakDb) peakDb = db;
      }
    }

    const dynRange = 64;
    for (let x = 0; x < cols; x++) {
      for (let b = 0; b < maxBin; b++) {
        const norm = Math.max(0, (mags[x * maxBin + b] - (peakDb - dynRange)) / dynRange);
        const [r, g, bl] = filmColor(Math.pow(norm, 0.82));
        const row = maxBin - 1 - b;
        const p = (row * cols + x) * 4;
        img.data[p] = r;
        img.data[p + 1] = g;
        img.data[p + 2] = bl;
        img.data[p + 3] = 255;
      }
    }
    sctx.putImageData(img, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.drawImage(specImg, 0, 0, w, h);
  } else {
    // Waveform + RMS envelope
    ctx.strokeStyle = FILM.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(0, h / 2);
    ctx.lineTo(w, h / 2);
    ctx.stroke();

    const spp = samples.length / w;
    ctx.fillStyle = "rgba(166, 203, 224, 0.62)";
    ctx.strokeStyle = FILM.ice;
    ctx.lineWidth = 1.2 * dpr;
    ctx.beginPath();
    for (let x = 0; x < w; x++) {
      const i0 = Math.floor(x * spp);
      const i1 = Math.min(samples.length, Math.ceil((x + 1) * spp));
      let lo = 1;
      let hi = -1;
      for (let i = i0; i < i1; i++) {
        const v = samples[i];
        if (v < lo) lo = v;
        if (v > hi) hi = v;
      }
      const yTop = ((1 - hi * 0.92) * 0.5) * h;
      const yBot = ((1 - lo * 0.92) * 0.5) * h;
      ctx.fillRect(x, yTop, 1, Math.max(1, yBot - yTop));
    }
  }

  // Frequency lines (lettered on the spectrogram) + a tick where each word starts
  ctx.strokeStyle = FILM.line;
  ctx.lineWidth = 1;
  for (let k = 1; k <= 3; k++) {
    const y = Math.round((k / 4) * h) + 0.5;
    ctx.beginPath();
    ctx.moveTo(0, y);
    ctx.lineTo(w, y);
    ctx.stroke();
  }
  for (const wd of speech.words) {
    const x = (wd.start / speech.duration) * w;
    ctx.strokeStyle = "rgba(219, 231, 240, 0.22)";
    ctx.beginPath();
    ctx.moveTo(x, h - 9 * dpr);
    ctx.lineTo(x, h);
    ctx.stroke();
  }

  // Overlay F1–F3 formant tracks on Spectrogram when enabled
  const ac = speech.acoustics;
  const acStride = speech.acousticFields ? speech.acousticFields.length : 0;
  const acCount = ac && acStride >= 6 ? Math.floor(ac.length / acStride) : 0;

  if (vizMode === "spec" && showFormants && acCount > 1) {
    const fMaxSpec = 8000;
    // the resonances are what the articulators make: all three in the tissue colour
    const colors = [
      [1, "rgba(240, 102, 138, 0.86)"],
      [2, "rgba(240, 102, 138, 0.86)"],
      [3, "rgba(240, 102, 138, 0.86)"],
    ];
    ctx.save();
    ctx.lineWidth = 1.15 * lw;
    ctx.lineJoin = "round";
    ctx.lineCap = "round";
    for (const [fIdx, color] of colors) {
      ctx.strokeStyle = color;
      let pen = false;
      ctx.beginPath();
      for (let k = 0; k < acCount; k++) {
        const v = ac[k * acStride + 4];
        const n = ac[k * acStride + 5];
        const fk = ac[k * acStride + fIdx];
        if (v < 0.14 && n < 0.18) {
          pen = false;
          continue;
        }
        const x = (k / Math.max(1, acCount - 1)) * w;
        const y = h - Math.max(0.02, Math.min(0.98, fk / fMaxSpec)) * h;
        if (!pen) {
          ctx.moveTo(x, y);
          pen = true;
        } else {
          ctx.lineTo(x, y);
        }
      }
      ctx.stroke();
    }
    ctx.restore();
  }

  // Overlay Pitch (F0) contour in glowing cyan (auto-fits multi-voice & singing ranges!)
  let fMin = voice.settings.gender === "female" ? 110 : 62;
  let fMax = voice.settings.gender === "female" ? 440 : 290;
  if (acCount > 1) {
    let obsMin = Infinity;
    let obsMax = 0;
    for (let k = 0; k < acCount; k++) {
      const f0 = ac[k * acStride];
      const v = ac[k * acStride + 4];
      if (v >= 0.18 && f0 > 45) {
        if (f0 < obsMin) obsMin = f0;
        if (f0 > obsMax) obsMax = f0;
      }
    }
    if (obsMax > obsMin) {
      fMin = Math.max(50, obsMin * 0.78);
      fMax = Math.min(1050, Math.max(fMin * 1.8, obsMax * 1.22));
    }
  }
  ctx.save();
  ctx.strokeStyle = FILM.gold;
  ctx.shadowColor = "rgba(7, 13, 19, 0.7)";
  ctx.shadowBlur = 3 * dpr;
  ctx.lineWidth = 2 * lw;
  ctx.lineJoin = "round";
  ctx.lineCap = "round";
  let penDown = false;
  ctx.beginPath();
  if (acCount > 1) {
    for (let k = 0; k < acCount; k++) {
      const f0 = ac[k * acStride];
      const v = ac[k * acStride + 4];
      if (v < 0.18 || f0 <= 0) {
        penDown = false;
        continue;
      }
      const x = (k / Math.max(1, acCount - 1)) * w;
      const normLog = Math.log2(f0 / fMin) / Math.log2(fMax / fMin);
      const y = h - (0.10 + 0.78 * Math.max(0, Math.min(1, normLog))) * h;
      if (!penDown) {
        ctx.moveTo(x, y);
        penDown = true;
      } else {
        ctx.lineTo(x, y);
      }
    }
  } else {
    cachedF0Track = computePitchTrack(samples, sr, voice.settings.gender);
    const { track } = cachedF0Track;
    for (let k = 0; k < track.length; k++) {
      const f0 = track[k];
      if (f0 <= 0) {
        penDown = false;
        continue;
      }
      const x = (k / Math.max(1, track.length - 1)) * w;
      const normLog = Math.log2(f0 / fMin) / Math.log2(fMax / fMin);
      const y = h - (0.10 + 0.78 * Math.max(0, Math.min(1, normLog))) * h;
      if (!penDown) {
        ctx.moveTo(x, y);
        penDown = true;
      } else {
        ctx.lineTo(x, y);
      }
    }
  }
  ctx.stroke();
  ctx.restore();

  // Letter the frequency lines last, over the traces
  if (vizMode === "spec") {
    ctx.save();
    ctx.font = `${10.5 * dpr}px ${UI_FONT}`;
    ctx.textBaseline = "bottom";
    ctx.shadowColor = "rgba(7, 13, 19, 0.9)";
    ctx.shadowBlur = 3 * dpr;
    ctx.fillStyle = "rgba(219, 231, 240, 0.82)";
    for (let k = 1; k <= 3; k++) {
      ctx.fillText(`${8 - 2 * k} kHz`, 7 * dpr, Math.round((k / 4) * h) - 2 * dpr);
    }
    ctx.restore();
  }

  cachedAcousticBitmap = off;
}

function renderAcousticFrame(playSec) {
  const canvas = $("waveCanvas");
  const ctx = canvas.getContext("2d");
  const w = canvas.width;
  const h = canvas.height;
  if (!cachedAcousticBitmap || !lastSpeech) {
    ctx.fillStyle = FILM.base;
    ctx.fillRect(0, 0, w, h);
    return;
  }
  ctx.drawImage(cachedAcousticBitmap, 0, 0, w, h);

  if (playSec >= 0 && lastSpeech.duration > 0) {
    // what is still to come is held back a little; the cursor is plain white
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const x = Math.max(0, Math.min(w, (playSec / lastSpeech.duration) * w));
    ctx.fillStyle = "rgba(7, 13, 19, 0.38)";
    ctx.fillRect(x, 0, w - x, h);
    ctx.strokeStyle = "#ffffff";
    ctx.lineWidth = 1.5 * dpr;
    ctx.beginPath();
    ctx.moveTo(x, 0);
    ctx.lineTo(x, h);
    ctx.stroke();
  }
}

// ============================================================================
// Live Expressive Avatar + Anatomical Midsagittal Vocal-Tract Visualizer
// ============================================================================
function sampleSpeechAt(speech, tSec) {
  if (!speech || !speech.visemes || !speech.visemes.length || tSec < 0 || tSec > speech.duration) {
    return null;
  }
  const stride = speech.visemeFields.length;
  const total = Math.floor(speech.visemes.length / stride);
  if (total <= 0) return null;

  const exact = Math.max(0, Math.min(total - 1, tSec * speech.visemeRate));
  const i0 = Math.floor(exact);
  const i1 = Math.min(total - 1, i0 + 1);
  const a = exact - i0;
  const v = speech.visemes;
  const o0 = i0 * stride;
  const o1 = i1 * stride;
  const lerp = (x, y) => x + (y - x) * a;

  const out = {
    jaw: lerp(v[o0], v[o1]),
    tH: lerp(v[o0 + 1], v[o1 + 1]),
    tF: lerp(v[o0 + 2], v[o1 + 2]),
    tTip: lerp(v[o0 + 3], v[o1 + 3]),
    lipR: lerp(v[o0 + 4], v[o1 + 4]),
    lipS: lerp(v[o0 + 5], v[o1 + 5]),
    lipC: lerp(v[o0 + 6], v[o1 + 6]),
    velum: lerp(v[o0 + 7], v[o1 + 7]),
    energy: lerp(v[o0 + 8], v[o1 + 8]),
    f0: voice?.settings.pitchHz ?? 110,
    f1: 500,
    f2: 1500,
    f3: 2500,
    voicing: 0,
    noise: 0,
    phone: "",
    ipa: "",
    word: "",
  };

  if (speech.acoustics && speech.acousticFields && speech.acousticFields.length >= 6) {
    const as = speech.acousticFields.length;
    const ac = speech.acoustics;
    const a0 = i0 * as;
    const a1 = i1 * as;
    if (a1 + 5 < ac.length) {
      out.f0 = lerp(ac[a0], ac[a1]);
      out.f1 = lerp(ac[a0 + 1], ac[a1 + 1]);
      out.f2 = lerp(ac[a0 + 2], ac[a1 + 2]);
      out.f3 = lerp(ac[a0 + 3], ac[a1 + 3]);
      out.voicing = lerp(ac[a0 + 4], ac[a1 + 4]);
      out.noise = lerp(ac[a0 + 5], ac[a1 + 5]);
    }
  } else {
    out.voicing = voice?.settings.whisper ? 0 : out.energy;
    out.noise = voice?.settings.whisper ? out.energy * 0.9 : 0;
  }

  if (speech.phones) {
    for (let k = 0; k < speech.phones.length; k++) {
      const ph = speech.phones[k];
      if (tSec >= ph.start && tSec < ph.end) {
        out.phone = ph.phone;
        out.ipa = ph.ipa;
        break;
      }
    }
  }
  if (speech.words) {
    for (let k = 0; k < speech.words.length; k++) {
      const wd = speech.words[k];
      if (tSec >= wd.start && tSec < wd.end) {
        out.word = wd.word;
        break;
      }
    }
  }
  if (speech.segments && speech.segments.length) {
    // Hold the active/preceding speaker across pauses, breaths, and lead-in/tail so
    // single-speaker arcs and dialogues never flicker to the default panel avatar!
    let chosen = speech.segments[0];
    for (let k = 0; k < speech.segments.length; k++) {
      const sg = speech.segments[k];
      if (tSec >= sg.start && tSec <= sg.end) {
        chosen = sg;
        break;
      }
      if (tSec >= sg.start) {
        chosen = sg;
      }
    }
    out.segGender = chosen.gender;
    out.segTract = chosen.tractLengthCm;
    out.segPitch = chosen.pitchHz;
    out.segVoice = chosen.voice;
    out.segSinging = Boolean(chosen.singing);
    // a laugh or a sigh only while it lasts
    out.segSound = chosen.sound && tSec >= chosen.start && tSec <= chosen.end ? chosen.sound : "";
  }
  return out;
}

// Writes text only when it changes (these run every animation frame).
function setText(el, text) {
  if (el && el.textContent !== text) el.textContent = text;
}

// The readout slots keep their place when empty, so nothing shifts as the words go by.
function updateLiveBadges(word, ipa, phone, f0, voicing, speakerLabel = "", singing = false, sound = "") {
  const sb = $("activeSpeakerBadge");
  if (sb) {
    sb.hidden = !speakerLabel;
    const doing = sound ? `, ${SOUND_LABELS.get(sound) || sound}` : singing ? ", singing" : "";
    if (speakerLabel) setText(sb, `${speakerLabel}${doing}`);
  }
  setText($("activeWordBadge"), word || "");
  setText($("activePhoneBadge"), ipa && phone && phone !== "SIL" ? `/${ipa}/ ${phone}` : "");
  setText($("activeF0Badge"), voicing > 0.16 && f0 > 45 ? `${Math.round(f0)} Hz` : "");
}

function constrictionPoint(phone, lipProtrude, upperLipY, lowerLipY, tipX, tipY, dorsumX, dorsumY) {
  switch (phone) {
    case "P":
    case "B":
    case "F":
    case "V":
      return [62 - lipProtrude, (upperLipY + lowerLipY) * 0.5, -52, (Math.random() - 0.5) * 18];
    case "TH":
    case "DH":
      return [70, upperLipY + 4, -56, (Math.random() - 0.5) * 16];
    case "S":
    case "Z":
    case "T":
    case "D":
      return [80, 56, -68, 6 + (Math.random() - 0.5) * 20];
    case "SH":
    case "ZH":
    case "CH":
    case "JH":
      return [94, 52, -60, 8 + (Math.random() - 0.5) * 22];
    case "K":
    case "G":
      return [dorsumX, dorsumY - 5, -54, (Math.random() - 0.5) * 18];
    default:
      // Glottal / whisper / aspiration
      return [184, 138, -14 + (Math.random() - 0.5) * 14, -54 - Math.random() * 20];
  }
}

function stepStagePhysics(nowMs, target) {
  const dt = anim.lastTickMs > 0 ? Math.min(0.05, (nowMs - anim.lastTickMs) / 1000) : 0.016;
  anim.lastTickMs = nowMs;

  const basePitch = target?.segPitch || stickySegment?.pitchHz || anim.activeBasePitch || (voice?.settings.pitchHz ?? 110);
  if (target) {
    if (target.segGender) {
      stickySegment = {
        gender: target.segGender,
        tractLengthCm: target.segTract,
        pitchHz: target.segPitch,
        voice: target.segVoice || "",
        singing: Boolean(target.segSinging),
      };
    }
    anim.activeGender = target.segGender || stickySegment?.gender || null;
    anim.activeTract = target.segTract ?? stickySegment?.tractLengthCm ?? null;
    anim.activeBasePitch = target.segPitch ?? stickySegment?.pitchHz ?? null;
    anim.activeVoiceLabel = target.segVoice || "";
    anim.activeSinging = Boolean(target.segSinging);
  } else if (!isPlaying && scrubTimeSec < 0) {
    anim.activeGender = stickySegment?.gender || null;
    anim.activeTract = stickySegment?.tractLengthCm ?? null;
    anim.activeBasePitch = stickySegment?.pitchHz ?? null;
    anim.activeVoiceLabel = "";
    anim.activeSinging = false;
  }
  const idleBreath = 0.5 + 0.5 * Math.sin(nowMs * 0.0018);

  const goal = target || {
    jaw: 0.045 + 0.014 * idleBreath,
    tH: 0.46,
    tF: 0.50,
    tTip: 0.16,
    lipR: 0.04,
    lipS: 0.05,
    lipC: 0.72, // at rest the lips are together
    velum: 0.0,
    energy: 0.0,
    voicing: 0.0,
    noise: 0.0,
    f0: basePitch,
    f1: 500,
    f2: 1500,
    f3: 2500,
    phone: "",
    ipa: "",
  };

  const kFast = 1 - Math.exp(-dt * (target ? 36 : 9.5));
  const kMed = 1 - Math.exp(-dt * (target ? 22 : 7.5));
  anim.jaw += (goal.jaw - anim.jaw) * kFast;
  anim.tH += (goal.tH - anim.tH) * kFast;
  anim.tF += (goal.tF - anim.tF) * kFast;
  anim.tTip += (goal.tTip - anim.tTip) * kFast;
  anim.lipR += (goal.lipR - anim.lipR) * kFast;
  anim.lipS += (goal.lipS - anim.lipS) * kFast;
  anim.lipC += (goal.lipC - anim.lipC) * kFast;
  anim.velum += (goal.velum - anim.velum) * kMed;
  anim.energy += (goal.energy - anim.energy) * kFast;
  anim.voicing += (goal.voicing - anim.voicing) * kFast;
  anim.noise += (goal.noise - anim.noise) * kFast;
  anim.f0 += (goal.f0 - anim.f0) * kMed;
  anim.f1 += (goal.f1 - anim.f1) * kMed;
  anim.f2 += (goal.f2 - anim.f2) * kMed;
  anim.f3 += (goal.f3 - anim.f3) * kMed;
  anim.phone = goal.phone;
  anim.ipa = goal.ipa;

  const rawSt = anim.voicing > 0.12 ? 12 * Math.log2(Math.max(50, anim.f0) / Math.max(50, basePitch)) : 0;
  anim.pitchSt += (Math.max(-6, Math.min(9, rawSt)) - anim.pitchSt) * kMed;

  // Visual vocal-fold oscillation & acoustic wave propagation
  anim.glotPhase = (anim.glotPhase + dt * (6 + anim.f0 * 0.055)) % 1;
  anim.wavePhase = (anim.wavePhase + dt * (1.7 + anim.energy * 1.4)) % 1;

  // Natural blinking & subtle eye micro-saccades
  if (nowMs >= anim.nextBlinkMs) {
    anim.blinkStartMs = nowMs;
    anim.nextBlinkMs = nowMs + 2600 + Math.random() * 2800;
  }
  if (nowMs >= anim.nextGazeMs) {
    anim.targetGazeX = (Math.random() - 0.5) * 2.4;
    anim.targetGazeY = (Math.random() - 0.5) * 1.4;
    anim.nextGazeMs = nowMs + 1100 + Math.random() * 2100;
  }
  anim.gazeX += (anim.targetGazeX - anim.gazeX) * (1 - Math.exp(-dt * 6));
  anim.gazeY += (anim.targetGazeY - anim.gazeY) * (1 - Math.exp(-dt * 6));

  // Update turbulent airflow particles in sagittal space
  const lipProtrude = anim.lipR * 11 - anim.lipS * 3.5;
  const lipGap = Math.max(0, (0.04 + anim.jaw * 0.85) * (1 - anim.lipC * 0.98) * 27);
  const upperLipY = 82 + anim.lipC * 4;
  const lowerLipY = upperLipY + lipGap;
  const jawDrop = anim.jaw * 22;
  const dorsumX = 166 - anim.tF * 54;
  const dorsumY = 110 + jawDrop * 0.42 - anim.tH * 46;
  const tipX = 82 + (1 - anim.tF) * 14;
  const tipY = 98 + jawDrop * 0.62 - anim.tTip * 36;

  const spawnRate = anim.noise > 0.05 ? Math.min(4, Math.ceil(anim.noise * 4.5)) : 0;
  let spawned = 0;
  for (let i = 0; i < AIR_PARTICLES.length; i++) {
    const p = AIR_PARTICLES[i];
    if (p.active) {
      p.life += dt;
      if (p.life >= p.maxLife) {
        p.active = false;
        continue;
      }
      // Guide laryngeal breath up pharynx then out mouth
      if (p.x > 165 && p.y > 70) {
        p.vy -= 90 * dt;
        p.vx -= 45 * dt;
      } else if (p.x > 66) {
        p.vx -= 85 * dt;
        p.vy += (upperLipY + lipGap * 0.5 - p.y) * 3.2 * dt;
      }
      p.x += p.vx * dt;
      p.y += p.vy * dt;
    } else if (spawned < spawnRate) {
      const [sx, sy, vx, vy] = constrictionPoint(
        anim.phone,
        lipProtrude,
        upperLipY,
        lowerLipY,
        tipX,
        tipY,
        dorsumX,
        dorsumY
      );
      p.active = true;
      p.x = sx + (Math.random() - 0.5) * 4;
      p.y = sy + (Math.random() - 0.5) * 4;
      p.vx = vx;
      p.vy = vy;
      p.life = 0;
      p.maxLife = 0.28 + Math.random() * 0.26;
      p.size = 1.2 + Math.random() * 1.8;
      spawned++;
    }
  }

  // Update meter bars smoothly
  $("mJaw").style.width = `${Math.round(Math.min(1, anim.jaw) * 100)}%`;
  $("mTongue").style.width = `${Math.round(Math.min(1, Math.max(anim.tH, anim.tTip * 0.85)) * 100)}%`;
  $("mLips").style.width = `${Math.round(Math.min(1, Math.max(anim.lipR, anim.lipC, anim.lipS)) * 100)}%`;
  $("mVelum").style.width = `${Math.round(Math.min(1, anim.velum) * 100)}%`;
  $("mEnergy").style.width = `${Math.round(Math.min(1, anim.voicing) * 100)}%`;
  $("mAir").style.width = `${Math.round(Math.min(1, anim.noise) * 100)}%`;
}

function drawExpressiveFace(ctx, x0, y0, boxW, boxH, nowMs) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x0, y0, boxW, boxH);
  ctx.clip();

  const cx = x0 + boxW * 0.5;
  const cy = y0 + boxH * 0.55;
  const sc = Math.min(boxW / 228, boxH / 226);

  const activeGender = anim.activeGender || stickySegment?.gender || voice?.settings.gender;
  const isFem = activeGender === "female";
  const tract = anim.activeTract ?? stickySegment?.tractLengthCm ?? voice?.settings.tractLengthCm ?? (isFem ? 14.2 : 17.0);
  const sizeNorm = Math.max(-1, Math.min(1, (tract - 15.6) / 2.6)); // -1 compact .. +1 deep
  const isWhispering = (anim.voicing < 0.05 && anim.noise > 0.08) || Boolean(voice?.settings.whisper);

  // A lit backdrop, so the speaker reads as a person in a room and not a head on black
  const backdrop = ctx.createRadialGradient(cx, cy - 14 * sc, 10 * sc, cx, cy - 4 * sc, Math.max(boxW, boxH) * 0.72);
  backdrop.addColorStop(0, "#2b4257");
  backdrop.addColorStop(0.55, "#182a3a");
  backdrop.addColorStop(1, "rgba(11, 20, 28, 0)");
  ctx.fillStyle = backdrop;
  ctx.fillRect(x0, y0, boxW, boxH);

  // The voice as a glow behind the head: gold when voiced, ice for breath alone
  const haloAlpha = anim.voicing * 0.2 + anim.noise * 0.1;
  const halo = ctx.createRadialGradient(cx, cy - 8 * sc, 12 * sc, cx, cy - 8 * sc, 104 * sc);
  halo.addColorStop(0, isWhispering ? `rgba(191, 227, 245, ${haloAlpha})` : `rgba(242, 184, 75, ${haloAlpha})`);
  halo.addColorStop(1, "rgba(11, 20, 28, 0)");
  ctx.fillStyle = halo;
  ctx.fillRect(x0, y0, boxW, boxH);

  ctx.translate(cx, cy);
  ctx.scale(sc, sc);

  const timeSec = nowMs * 0.001;
  const nodY = anim.energy * 2.4 - anim.pitchSt * 0.55 + Math.sin(timeSec * 1.45) * 0.85;
  const headTilt = Math.sin(timeSec * 0.82) * 0.015 + anim.pitchSt * 0.006 * Math.cos(timeSec * 1.9);
  const headW = 53 + sizeNorm * 3.2 - (isFem ? 2.0 : 0);
  const cheekW = headW + (isFem ? 1.2 : 0.4);
  const jawW = 41.5 + sizeNorm * 5.0 - (isFem ? 4.8 : 0);
  const chinDrop = anim.jaw * 13.5 * (1 - anim.lipC * 0.55);
  const chinY = 57 + chinDrop;

  // 1. Back hair volume BEHIND neck & shoulders (no seam across throat!)
  if (isFem) {
    ctx.save();
    ctx.translate(0, nodY * 0.7);
    ctx.rotate(headTilt * 0.65);
    const backHairGrad = ctx.createLinearGradient(0, -86, 0, 86);
    backHairGrad.addColorStop(0, "#4b3542");
    backHairGrad.addColorStop(0.55, "#2d1f28");
    backHairGrad.addColorStop(1, "#191117");
    ctx.fillStyle = backHairGrad;
    ctx.beginPath();
    ctx.moveTo(-headW - 11, -10);
    ctx.bezierCurveTo(-headW - 15, -84, headW + 15, -84, headW + 11, -10);
    ctx.bezierCurveTo(headW + 20, 30, headW + 16, 66, headW + 4, 86);
    ctx.lineTo(-headW - 4, 86);
    ctx.bezierCurveTo(-headW - 16, 66, -headW - 20, 30, -headW - 11, -10);
    ctx.closePath();
    ctx.fill();
    ctx.restore();
  }

  // 2. Shoulders, clavicles & neck with cast jaw shadow
  const neckHalf = 20 + sizeNorm * 4.5 - (isFem ? 2.8 : 0);
  const neckGrad = ctx.createLinearGradient(0, 40, 0, 98);
  neckGrad.addColorStop(0, isFem ? "#c78972" : "#b97c65");
  neckGrad.addColorStop(0.34, isFem ? "#e5af97" : "#d79f86");
  neckGrad.addColorStop(1, isFem ? "#c0846d" : "#b0755f");
  ctx.fillStyle = neckGrad;
  ctx.beginPath();
  ctx.moveTo(-neckHalf, 38);
  ctx.quadraticCurveTo(-neckHalf - 2, 70, -neckHalf - 34, 98);
  ctx.lineTo(neckHalf + 34, 98);
  ctx.quadraticCurveTo(neckHalf + 2, 70, neckHalf, 38);
  ctx.closePath();
  ctx.fill();

  // Cast shadow of mandible onto neck
  const jawShadow = ctx.createRadialGradient(0, 53 + nodY, 4, 0, 57 + nodY, neckHalf * 1.38);
  jawShadow.addColorStop(0, "rgba(58, 28, 22, 0.44)");
  jawShadow.addColorStop(1, "rgba(58, 28, 22, 0)");
  ctx.fillStyle = jawShadow;
  ctx.beginPath();
  ctx.ellipse(0, 57 + nodY, neckHalf * 1.22, 16, 0, 0, Math.PI * 2);
  ctx.fill();

  // Tailored studio jacket / crew-neck sweater anchored to bottom of stage
  const coatGrad = ctx.createLinearGradient(0, 70, 0, 118);
  coatGrad.addColorStop(0, isFem ? "#3a4f63" : "#33475a");
  coatGrad.addColorStop(1, "#16232f");
  ctx.fillStyle = coatGrad;
  ctx.strokeStyle = "rgba(219, 231, 240, 0.16)";
  ctx.lineWidth = 1.4;
  ctx.beginPath();
  ctx.moveTo(-neckHalf - 56, 118);
  ctx.quadraticCurveTo(-neckHalf - 26, 77, -neckHalf + 1, 71);
  ctx.quadraticCurveTo(0, 86, neckHalf - 1, 71);
  ctx.quadraticCurveTo(neckHalf + 26, 77, neckHalf + 56, 118);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // 3. Head group (tilts & nods organically with speech prosody)
  ctx.save();
  ctx.translate(0, nodY);
  ctx.rotate(headTilt);

  if (!isFem) {
    // Male back crown hair silhouette behind ears
    const mBackGrad = ctx.createLinearGradient(0, -86, 0, 20);
    mBackGrad.addColorStop(0, "#383e52");
    mBackGrad.addColorStop(1, "#191c26");
    ctx.fillStyle = mBackGrad;
    ctx.beginPath();
    ctx.moveTo(-headW - 6, -10);
    ctx.bezierCurveTo(-headW - 11, -74, -22, -88, 4, -86);
    ctx.bezierCurveTo(32, -85, headW + 11, -72, headW + 6, -10);
    ctx.quadraticCurveTo(headW + 4, 10, headW - 2, 20);
    ctx.lineTo(-headW + 2, 20);
    ctx.quadraticCurveTo(-headW - 4, 10, -headW - 6, -10);
    ctx.closePath();
    ctx.fill();
  }

  // Sculpted Ears with helix & tragus shading
  for (const dir of [-1, 1]) {
    const earX = dir * (headW - 0.5);
    const earGrad = ctx.createLinearGradient(earX, -16, earX, 14);
    earGrad.addColorStop(0, isFem ? "#eab39b" : "#dead94");
    earGrad.addColorStop(1, isFem ? "#c5856d" : "#b77861");
    ctx.fillStyle = earGrad;
    ctx.beginPath();
    ctx.ellipse(earX, -3, 8.6, 16, dir * -0.09, 0, Math.PI * 2);
    ctx.fill();
    ctx.fillStyle = "rgba(112, 58, 44, 0.28)";
    ctx.beginPath();
    ctx.ellipse(earX - dir * 1.2, -2, 4.1, 9.0, dir * -0.08, 0, Math.PI * 2);
    ctx.fill();
  }

  // Sculpted Cranium, Zygomatic Cheekbones, Mandible & Chin
  const skinGrad = ctx.createRadialGradient(-15, -28, 8, 0, 4, 90);
  skinGrad.addColorStop(0, isFem ? "#f9d2be" : "#f1c5ae");
  skinGrad.addColorStop(0.55, isFem ? "#e8b299" : "#dca48b");
  skinGrad.addColorStop(0.88, isFem ? "#d69d86" : "#c99079");
  skinGrad.addColorStop(1, isFem ? "#c38a74" : "#b67d67");

  ctx.fillStyle = skinGrad;
  ctx.beginPath();
  ctx.moveTo(-headW, -20);
  ctx.bezierCurveTo(-headW, -67, headW, -67, headW, -20);
  // Temple -> cheekbone -> jawline -> chin
  ctx.bezierCurveTo(cheekW + 1.5, -2, cheekW, 14, jawW + 4, 34 + chinDrop * 0.45);
  ctx.bezierCurveTo(jawW - 4, 48 + chinDrop * 0.78, 18 - (isFem ? 4 : 0), chinY - 1, 0, chinY + 3.2);
  ctx.bezierCurveTo(-18 + (isFem ? 4 : 0), chinY - 1, -jawW + 4, 48 + chinDrop * 0.78, -jawW - 4, 34 + chinDrop * 0.45);
  ctx.bezierCurveTo(-cheekW, 14, -cheekW - 1.5, -2, -headW, -20);
  ctx.closePath();
  ctx.fill();

  // Temple & jawline 3D rim contour shading
  ctx.strokeStyle = "rgba(95, 48, 36, 0.22)";
  ctx.lineWidth = 1.3;
  ctx.stroke();

  // Subtle cheekbone & forehead studio key-light sheen
  const foreheadGlow = ctx.createRadialGradient(-10, -34, 2, -6, -32, 28);
  foreheadGlow.addColorStop(0, "rgba(255, 246, 240, 0.28)");
  foreheadGlow.addColorStop(1, "rgba(255, 246, 240, 0)");
  ctx.fillStyle = foreheadGlow;
  ctx.beginPath();
  ctx.ellipse(-6, -32, 28, 15, -0.1, 0, Math.PI * 2);
  ctx.fill();

  // Cheek warmth & cheekbone highlight (lifts with smile/spread vowels)
  const smileLift = anim.lipS * 3.5;
  for (const dir of [-1, 1]) {
    const blush = ctx.createRadialGradient(dir * 30, 9 - smileLift * 0.5, 2, dir * 30, 9 - smileLift * 0.5, 19);
    blush.addColorStop(0, `rgba(232, 108, 114, ${isFem ? 0.22 : 0.12})`);
    blush.addColorStop(1, "rgba(232, 108, 114, 0)");
    ctx.fillStyle = blush;
    ctx.beginPath();
    ctx.arc(dir * 30, 9 - smileLift * 0.5, 19, 0, Math.PI * 2);
    ctx.fill();
  }

  // Layered Crown Hair, Natural Side-Swept Fringe & Sideburns
  const hairGrad = ctx.createLinearGradient(-22, -88, 22, -16);
  hairGrad.addColorStop(0, isFem ? "#5a3f50" : "#40475e");
  hairGrad.addColorStop(0.52, isFem ? "#382632" : "#272c3b");
  hairGrad.addColorStop(1, isFem ? "#21161e" : "#181b25");
  ctx.fillStyle = hairGrad;
  ctx.beginPath();
  if (isFem) {
    // Fuller crown + soft side-swept wave framing temples & cheeks
    ctx.moveTo(-headW - 8, -8);
    ctx.bezierCurveTo(-headW - 12, -86, headW + 14, -86, headW + 8, -8);
    ctx.quadraticCurveTo(headW + 4, 14, headW - 1, 24);
    ctx.bezierCurveTo(headW - 2, -16, headW - 8, -36, 12, -44);
    ctx.bezierCurveTo(-4, -49, -18, -42, -26, -53); // natural side part
    ctx.bezierCurveTo(-36, -40, -headW + 3, -18, -headW + 1, 24);
    ctx.quadraticCurveTo(-headW - 4, 14, -headW - 8, -8);
  } else {
    // Fuller textured side-parted hairstyle with natural wave & neat temple tapers
    ctx.moveTo(-headW - 5, -10);
    ctx.bezierCurveTo(-headW - 10, -78, -22, -90, 6, -87);
    ctx.bezierCurveTo(34, -84, headW + 10, -74, headW + 5, -10);
    ctx.lineTo(headW + 0.5, 5);
    ctx.lineTo(headW - 4.2, 5);
    ctx.quadraticCurveTo(headW - 4.5, -24, headW - 10, -39);
    // Natural swept front hairline wave (higher at left part, gentle dip on right)
    ctx.bezierCurveTo(24, -42, -4, -50, -24, -51);
    ctx.bezierCurveTo(-36, -46, -headW + 6, -32, -headW + 4.2, 5);
    ctx.lineTo(-headW - 0.5, 5);
  }
  ctx.closePath();
  ctx.fill();

  // Hair crown specular ribbon highlight
  ctx.strokeStyle = "rgba(255, 255, 255, 0.05)";
  ctx.lineWidth = 2.8;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(-24, -63);
  ctx.quadraticCurveTo(2, -71, 28, -59);
  ctx.stroke();

  // Eyes + Orbital Sockets + Eyelid Creases + Tapered Feathered Eyebrows
  const blinkElapsed = nowMs - anim.blinkStartMs;
  const blinkAmt = blinkElapsed >= 0 && blinkElapsed < 145
    ? Math.sin((blinkElapsed / 145) * Math.PI)
    : 0;
  const browLift = Math.max(-2.5, Math.min(6.8, anim.pitchSt * 0.85 + anim.energy * 2.3));

  for (const dir of [-1, 1]) {
    const ex = dir * 22;
    const ey = -12.5;
    const openH = Math.max(0.65, 6.3 * (1 - blinkAmt * 0.94));

    // Soft orbital socket shading
    const sockGrad = ctx.createRadialGradient(ex, ey - 1, 3, ex, ey - 1, 15);
    sockGrad.addColorStop(0, "rgba(128, 68, 52, 0.16)");
    sockGrad.addColorStop(1, "rgba(128, 68, 52, 0)");
    ctx.fillStyle = sockGrad;
    ctx.beginPath();
    ctx.ellipse(ex, ey - 1, 15, 11, 0, 0, Math.PI * 2);
    ctx.fill();

    // Upper eyelid fold crease
    ctx.strokeStyle = "rgba(115, 60, 46, 0.34)";
    ctx.lineWidth = 1.1;
    ctx.beginPath();
    ctx.moveTo(ex - 11, ey - 3.2 - openH * 0.55);
    ctx.quadraticCurveTo(ex, ey - 6.2 - openH * 0.85, ex + 11, ey - 2.8 - openH * 0.55);
    ctx.stroke();

    // Eye white (sclera) with subtle top lid shadow
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(ex - 11.5, ey + 0.5);
    ctx.quadraticCurveTo(ex, ey - openH * 1.46, ex + 11.5, ey + 0.5);
    ctx.quadraticCurveTo(ex, ey + openH * 1.16, ex - 11.5, ey + 0.5);
    ctx.closePath();
    const scleraGrad = ctx.createLinearGradient(ex, ey - openH, ex, ey + openH);
    scleraGrad.addColorStop(0, "#c9cdd3");
    scleraGrad.addColorStop(0.4, "#eceae6");
    scleraGrad.addColorStop(1, "#dcd9d4");
    ctx.fillStyle = scleraGrad;
    ctx.fill();
    ctx.clip();

    // Iris with dark limbal ring + pupil
    const ix = ex + anim.gazeX;
    const iy = ey + anim.gazeY * 0.7;
    const irisGrad = ctx.createRadialGradient(ix - 1.2, iy - 1.5, 0.8, ix, iy, 5.8);
    irisGrad.addColorStop(0, isFem ? "#6bc0be" : "#5a92bf");
    irisGrad.addColorStop(0.70, isFem ? "#2b616d" : "#264668");
    irisGrad.addColorStop(1, "#111927");
    ctx.fillStyle = irisGrad;
    ctx.beginPath();
    ctx.arc(ix, iy, 5.6, 0, Math.PI * 2);
    ctx.fill();

    ctx.fillStyle = "#090c12";
    ctx.beginPath();
    ctx.arc(ix, iy, 2.45, 0, Math.PI * 2);
    ctx.fill();

    // Dual corneal catchlights
    ctx.fillStyle = "rgba(255,255,255,0.82)";
    ctx.beginPath();
    ctx.arc(ix - 1.7, iy - 1.8, 1.15, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // Lower water-line & upper lash line
    ctx.strokeStyle = "rgba(120, 68, 56, 0.28)";
    ctx.lineWidth = 1.0;
    ctx.beginPath();
    ctx.moveTo(ex - 10.5, ey + 0.8);
    ctx.quadraticCurveTo(ex, ey + openH * 1.18, ex + 10.5, ey + 0.8);
    ctx.stroke();

    ctx.strokeStyle = "#3a2a2c";
    ctx.lineWidth = isFem ? 1.9 : 1.35;
    ctx.lineCap = "round";
    ctx.beginPath();
    ctx.moveTo(ex - 11.8, ey + 0.6);
    ctx.quadraticCurveTo(ex, ey - openH * 1.48, ex + (isFem ? 13.0 : 11.6), ey + (isFem ? -0.5 : 0.6));
    ctx.stroke();

    // Sculpted, tapered eyebrow (filled anatomical brow shape, not a stick line)
    const by = ey - 13.5 - browLift;
    const thick = isFem ? 2.6 : 3.6;
    ctx.fillStyle = isFem ? "#332331" : "#252938";
    ctx.beginPath();
    ctx.moveTo(ex - dir * 12, by + 1.8);
    ctx.quadraticCurveTo(ex + dir * 2, by - 4.2 - browLift * 0.18, ex + dir * 13.8, by + 1.6);
    ctx.quadraticCurveTo(ex + dir * 2, by - 4.2 - browLift * 0.18 + thick, ex - dir * 11.5, by + 1.8 + thick * 0.85);
    ctx.closePath();
    ctx.fill();
  }

  // ========================================================================
  // Full 3D Sculpted Nose (bridge planes, tip ball, alar wings, nostrils, philtrum)
  // ========================================================================
  // Soft side-bridge shadow on shadow side
  const noseShade = ctx.createLinearGradient(-8, -14, 5, 12);
  noseShade.addColorStop(0, "rgba(130, 70, 54, 0.05)");
  noseShade.addColorStop(1, "rgba(130, 70, 54, 0.24)");
  ctx.fillStyle = noseShade;
  ctx.beginPath();
  ctx.moveTo(1, -13);
  ctx.quadraticCurveTo(6.5, 1, 9.5, 9.5);
  ctx.quadraticCurveTo(3, 14.5, 0, 13.5);
  ctx.closePath();
  ctx.fill();

  // Underside of nose tip & alar wings shadow
  ctx.fillStyle = "rgba(115, 58, 44, 0.28)";
  ctx.beginPath();
  ctx.moveTo(-9.5, 10.2);
  ctx.quadraticCurveTo(0, 16.2, 9.5, 10.2);
  ctx.quadraticCurveTo(5, 13.2, 0, 13.2);
  ctx.quadraticCurveTo(-5, 13.2, -9.5, 10.2);
  ctx.closePath();
  ctx.fill();

  // Shaded nostrils
  ctx.fillStyle = "rgba(68, 28, 22, 0.62)";
  ctx.beginPath();
  ctx.ellipse(-4.6, 11.3, 2.3, 1.15, 0.28, 0, Math.PI * 2);
  ctx.fill();
  ctx.beginPath();
  ctx.ellipse(4.6, 11.3, 2.3, 1.15, -0.28, 0, Math.PI * 2);
  ctx.fill();

  // Alar wing contours
  ctx.strokeStyle = "rgba(122, 64, 48, 0.36)";
  ctx.lineWidth = 1.25;
  ctx.beginPath();
  ctx.arc(-6.4, 9.6, 3.3, Math.PI * 0.55, Math.PI * 1.45);
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(6.4, 9.6, 3.3, -Math.PI * 0.45, Math.PI * 0.45);
  ctx.stroke();

  // Nasal bridge & rounded tip key-light highlight
  ctx.strokeStyle = "rgba(255, 244, 236, 0.36)";
  ctx.lineWidth = 2.2;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(-0.8, -10);
  ctx.lineTo(-1.0, 5.5);
  ctx.stroke();
  ctx.fillStyle = "rgba(255, 244, 236, 0.34)";
  ctx.beginPath();
  ctx.ellipse(-0.6, 8.8, 3.2, 2.4, 0, 0, Math.PI * 2);
  ctx.fill();

  // Subtle philtrum dimple between nose and Cupid's bow
  const philGrad = ctx.createLinearGradient(0, 13.5, 0, 24);
  philGrad.addColorStop(0, "rgba(120, 62, 48, 0.20)");
  philGrad.addColorStop(1, "rgba(120, 62, 48, 0.03)");
  ctx.fillStyle = philGrad;
  ctx.beginPath();
  ctx.ellipse(0, 19, 2.8, 5, 0, 0, Math.PI * 2);
  ctx.fill();

  // Subtle amber nasal glow at nostrils when velum is open (/m, n, ng/)
  if (anim.velum > 0.12 && anim.voicing > 0.1) {
    ctx.fillStyle = `rgba(242, 184, 75, ${Math.min(0.5, anim.velum * 0.45)})`;
    ctx.beginPath();
    ctx.ellipse(-4.8, 11.5, 3.4, 1.9, 0, 0, Math.PI * 2);
    ctx.ellipse(4.8, 11.5, 3.4, 1.9, 0, 0, Math.PI * 2);
    ctx.fill();
  }

  // ========================================================================
  // 3D Articulatory Mouth (lips, teeth, tongue apex/dorsum, chin groove)
  // ========================================================================
  const my = 31 + chinDrop * 0.30;
  const isLabiodental = anim.phone === "F" || anim.phone === "V";
  const roundAmt = anim.lipR;
  const spreadAmt = anim.lipS;
  const closeAmt = isLabiodental ? Math.min(0.68, anim.lipC) : anim.lipC;

  const halfW = (20.5 + spreadAmt * 8.5 - roundAmt * 7.2) * (isFem ? 0.95 : 1.02);
  const rawOpen = (1.2 + anim.jaw * 20.5) * (1 - closeAmt * 0.97);
  const openH = isLabiodental ? Math.max(2.6, rawOpen) : Math.max(0.25, rawOpen);
  const cornerY = my - smileLift * 0.85 + roundAmt * 1.5;
  const upperInnerY = my - openH * 0.44;
  const lowerInnerY = my + openH * 0.56;

  // Labiomental crease shadow + chin pad highlight below lower lip
  const lowerThick = (isLabiodental ? 4.2 : 5.8) + roundAmt * 2.5 - spreadAmt * 0.9;
  ctx.fillStyle = "rgba(112, 56, 44, 0.22)";
  ctx.beginPath();
  ctx.ellipse(0, lowerInnerY + lowerThick + 3.2, halfW * 0.52, 2.4, 0, 0, Math.PI * 2);
  ctx.fill();
  ctx.fillStyle = "rgba(255, 242, 234, 0.18)";
  ctx.beginPath();
  ctx.ellipse(-1.5, chinY - 7, 9, 4.5, 0, 0, Math.PI * 2);
  ctx.fill();

  // Inner oral cavity (clipped to mouth opening)
  if (openH > 0.8) {
    ctx.save();
    ctx.beginPath();
    ctx.moveTo(-halfW + 1.5, cornerY);
    ctx.quadraticCurveTo(0, upperInnerY - 2.2, halfW - 1.5, cornerY);
    ctx.quadraticCurveTo(0, lowerInnerY + 2.5, -halfW + 1.5, cornerY);
    ctx.closePath();
    ctx.clip();

    // Oral depth shadow
    const cavGrad = ctx.createLinearGradient(0, upperInnerY - 4, 0, lowerInnerY + 6);
    cavGrad.addColorStop(0, "#1c0b12");
    cavGrad.addColorStop(0.55, "#33121d");
    cavGrad.addColorStop(1, "#1e0a11");
    ctx.fillStyle = cavGrad;
    ctx.fillRect(-halfW - 4, upperInnerY - 8, halfW * 2 + 8, openH + 18);

    // Visible Tongue inside oral aperture (rises with tH & tTip!)
    const tongueLift = Math.max(anim.tH * 0.72, anim.tTip * 0.95);
    const tongueTopY = lowerInnerY + 2 - tongueLift * (openH * 0.88 + 3.5);
    const tongueHalfW = halfW * (anim.tTip > 0.6 ? 0.58 : 0.78);
    const tGrad = ctx.createRadialGradient(0, tongueTopY + 5, 2, 0, tongueTopY + 8, tongueHalfW);
    tGrad.addColorStop(0, "#f48498");
    tGrad.addColorStop(1, "#b84a62");
    ctx.fillStyle = tGrad;
    ctx.beginPath();
    ctx.ellipse(0, tongueTopY + 8, tongueHalfW, 9.5, 0, Math.PI, Math.PI * 2);
    ctx.fill();

    // Upper incisors (fixed to maxilla, subtle tooth divisions)
    const teethW = halfW * 0.78;
    const upperTeethBottom = upperInnerY + 3.6 - roundAmt * 1.2;
    ctx.fillStyle = "#f5f4ef";
    ctx.beginPath();
    ctx.roundRect(-teethW, upperInnerY - 5, teethW * 2, upperTeethBottom - (upperInnerY - 5), 2.2);
    ctx.fill();
    ctx.strokeStyle = "rgba(160, 152, 142, 0.35)";
    ctx.lineWidth = 0.7;
    for (const tx of [-teethW * 0.48, 0, teethW * 0.48]) {
      ctx.beginPath();
      ctx.moveTo(tx, upperInnerY - 4);
      ctx.lineTo(tx, upperTeethBottom);
      ctx.stroke();
    }

    // Lower incisors (move with jaw)
    if (openH > 5.5 && !isLabiodental) {
      const lowerTeethTop = lowerInnerY - 2.4 + anim.jaw * 1.4;
      ctx.fillStyle = "#e8e6df";
      ctx.beginPath();
      ctx.roundRect(-teethW * 0.88, lowerTeethTop, teethW * 1.76, 6, 1.8);
      ctx.fill();
    }
    ctx.restore();
  }

  // Sculpted Upper Lip (Cupid's bow) & Lower Vermilion Cushion
  const lipBase = isFem ? "#cf6a78" : "#bb7f7a";
  const lipDark = isFem ? "#a64c5b" : "#9a605d";
  const upperThick = 4.6 + roundAmt * 2.2 - spreadAmt * 0.8;

  // Upper lip
  ctx.fillStyle = lipDark;
  ctx.beginPath();
  ctx.moveTo(-halfW, cornerY);
  ctx.bezierCurveTo(-halfW * 0.55, upperInnerY - upperThick, -5.5, upperInnerY - upperThick - 1.6, 0, upperInnerY - upperThick + 0.9);
  ctx.bezierCurveTo(5.5, upperInnerY - upperThick - 1.6, halfW * 0.55, upperInnerY - upperThick, halfW, cornerY);
  ctx.quadraticCurveTo(0, upperInnerY - (openH < 0.9 ? 0 : 1.6), -halfW, cornerY);
  ctx.closePath();
  ctx.fill();

  // Lower lip
  const lowLipOffset = isLabiodental ? -1.8 : 0;
  ctx.fillStyle = lipBase;
  ctx.beginPath();
  ctx.moveTo(-halfW, cornerY);
  ctx.quadraticCurveTo(0, lowerInnerY + (openH < 0.9 ? 0.2 : 1.8) + lowLipOffset, halfW, cornerY);
  ctx.bezierCurveTo(halfW * 0.62, lowerInnerY + lowerThick + lowLipOffset, -halfW * 0.62, lowerInnerY + lowerThick + lowLipOffset, -halfW, cornerY);
  ctx.closePath();
  ctx.fill();

  // Lower-lip studio specular highlight + corner commissures
  ctx.strokeStyle = "rgba(255, 235, 238, 0.38)";
  ctx.lineWidth = 1.5;
  ctx.beginPath();
  ctx.moveTo(-7.5, lowerInnerY + lowerThick * 0.48 + lowLipOffset);
  ctx.quadraticCurveTo(0, lowerInnerY + lowerThick * 0.62 + lowLipOffset, 7.5, lowerInnerY + lowerThick * 0.48 + lowLipOffset);
  ctx.stroke();

  for (const dir of [-1, 1]) {
    ctx.fillStyle = "rgba(75, 24, 34, 0.42)";
    ctx.beginPath();
    ctx.arc(dir * (halfW + 0.4), cornerY, 1.5, 0, Math.PI * 2);
    ctx.fill();
  }

  // Lip seal crease when closed (/p, b, m/ or rest)
  if (openH < 1.3) {
    ctx.strokeStyle = "rgba(72, 22, 34, 0.68)";
    ctx.lineWidth = 1.4;
    ctx.beginPath();
    ctx.moveTo(-halfW + 1, cornerY);
    ctx.quadraticCurveTo(0, my + 0.4, halfW - 1, cornerY);
    ctx.stroke();
  }

  // Radiating soundwave arcs near mouth when voicing or whispering
  if (anim.energy > 0.08) {
    for (let k = 0; k < 2; k++) {
      const u = (anim.wavePhase + k * 0.5) % 1;
      const r = halfW + 8 + u * 22;
      const alpha = (1 - u) * Math.min(0.45, anim.energy * 0.5);
      ctx.strokeStyle = isWhispering
        ? `rgba(191, 227, 245, ${alpha})`
        : `rgba(242, 184, 75, ${alpha})`;
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      ctx.arc(0, my, r, -0.45, 0.45);
      ctx.stroke();
      ctx.beginPath();
      ctx.arc(0, my, r, Math.PI - 0.45, Math.PI + 0.45);
      ctx.stroke();
    }
  }

  ctx.restore(); // head group
  ctx.restore(); // clip
}

function drawMidsagittalTract(ctx, x0, y0, boxW, boxH) {
  ctx.save();
  ctx.beginPath();
  ctx.rect(x0, y0, boxW, boxH);
  ctx.clip();

  // Faint measuring grid
  ctx.strokeStyle = FILM.grid;
  ctx.lineWidth = 1;
  const step = Math.max(18, Math.round(boxH / 8));
  for (let gx = x0 + step; gx < x0 + boxW; gx += step) {
    ctx.beginPath();
    ctx.moveTo(gx, y0);
    ctx.lineTo(gx, y0 + boxH);
    ctx.stroke();
  }
  for (let gy = y0 + step; gy < y0 + boxH; gy += step) {
    ctx.beginPath();
    ctx.moveTo(x0, gy);
    ctx.lineTo(x0 + boxW, gy);
    ctx.stroke();
  }

  // Fit 254 x 192 sagittal head coordinate space cleanly inside box
  const sc = Math.min(boxW / 254, boxH / 192);
  const ox = x0 + (boxW - 254 * sc) * 0.5;
  const oy = y0 + (boxH - 192 * sc) * 0.5 + 2 * sc;
  ctx.translate(ox, oy);
  ctx.scale(sc, sc);

  const jaw = anim.jaw;
  const tH = anim.tH;
  const tF = anim.tF;
  const tTip = anim.tTip;
  const lipR = anim.lipR;
  const lipS = anim.lipS;
  const lipC = anim.lipC;
  const velum = anim.velum;
  const isLabiodental = anim.phone === "F" || anim.phone === "V";
  const isRhotic = anim.phone === "R" || anim.phone === "ER";

  const jawDrop = jaw * 22;
  const lipProtrude = lipR * 11 - lipS * 3.5;
  const lipGap = isLabiodental
    ? Math.max(1.8, (0.04 + jaw * 0.6) * 14)
    : Math.max(0, (0.04 + jaw * 0.85) * (1 - lipC * 0.98) * 27);
  const upperLipY = 82 + lipC * 4;
  const lowerLipY = upperLipY + lipGap;

  // 1. Complete Side-Profile Human Head & Neck Silhouette (X-Ray cutaway)
  const headFill = ctx.createRadialGradient(135, 76, 18, 135, 86, 118);
  headFill.addColorStop(0, "rgba(196, 220, 238, 0.05)");
  headFill.addColorStop(0.75, "rgba(196, 220, 238, 0.10)");
  headFill.addColorStop(1, "rgba(196, 220, 238, 0.17)");
  ctx.fillStyle = headFill;
  ctx.strokeStyle = "rgba(203, 221, 235, 0.44)";
  ctx.lineWidth = 1.7;
  ctx.lineJoin = "round";
  ctx.beginPath();
  // Start at upper lip philtrum -> nose -> brow -> forehead -> skull crown -> occiput -> neck -> throat -> chin -> lower lip
  ctx.moveTo(60 - lipProtrude * 0.7, upperLipY - 4);
  ctx.lineTo(57, 67); // base of nose (subnasale)
  ctx.quadraticCurveTo(40, 65, 41, 59); // rounded nose tip
  ctx.quadraticCurveTo(52, 45, 57, 33); // nasal bridge -> nasion
  ctx.quadraticCurveTo(55, 25, 62, 16); // brow ridge -> forehead
  ctx.bezierCurveTo(82, 2, 168, 0, 206, 26); // cranial dome
  ctx.bezierCurveTo(228, 46, 226, 94, 212, 126); // occiput (back of skull)
  ctx.quadraticCurveTo(208, 148, 212, 178); // posterior neck
  ctx.lineTo(144, 178); // base of neck
  ctx.quadraticCurveTo(142, 152, 108, 145 + jawDrop * 0.82); // anterior throat -> submental jaw
  ctx.quadraticCurveTo(64, 146 + jawDrop * 0.90, 56, 128 + jawDrop * 0.88); // chin pad (mentum)
  ctx.quadraticCurveTo(60, lowerLipY + 12, 60 - lipProtrude * 0.7, lowerLipY + 5); // labiomental fold
  // Step cleanly inside oral cavity to hollow out the vocal tract airway
  ctx.lineTo(176, 154);
  ctx.lineTo(198, 170);
  ctx.lineTo(198, 54);
  ctx.quadraticCurveTo(156, 28, 70, 68);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // Subtle side-profile eye indicator for anatomical orientation
  ctx.strokeStyle = "rgba(203, 221, 235, 0.3)";
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  ctx.moveTo(74, 34);
  ctx.quadraticCurveTo(66, 37, 74, 40);
  ctx.stroke();

  // 2. Nasal Cavity Chamber + Velopharyngeal Resonance Stream
  ctx.fillStyle = velum > 0.06
    ? `rgba(242, 184, 75, ${0.08 + velum * 0.3})`
    : "rgba(7, 13, 19, 0.5)";
  ctx.strokeStyle = velum > 0.12 ? "rgba(242, 184, 75, 0.7)" : "rgba(203, 221, 235, 0.26)";
  ctx.lineWidth = 1.6;
  ctx.beginPath();
  ctx.moveTo(58, 62);
  ctx.bezierCurveTo(86, 30, 154, 26, 196, 54);
  ctx.lineTo(196, 68);
  ctx.quadraticCurveTo(158, 46, 74, 58);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  if (velum > 0.10 && anim.voicing > 0.08) {
    // Flowing amber nasal soundwaves out the nostril
    for (let k = 0; k < 3; k++) {
      const u = (anim.wavePhase + k * 0.33) % 1;
      const wx = 54 - u * 22;
      const wy = 63 + u * 4;
      ctx.strokeStyle = `rgba(242, 184, 75, ${(1 - u) * velum * 0.65})`;
      ctx.lineWidth = 1.6;
      ctx.beginPath();
      ctx.arc(wx, wy, 4 + u * 9, Math.PI * 0.72, Math.PI * 1.32);
      ctx.stroke();
    }
  }

  // 3. Posterior Pharyngeal Wall + Hard Palate + Soft Palate (Velum Valve)
  ctx.strokeStyle = FILM.bone;
  ctx.lineWidth = 3;
  ctx.lineCap = "round";
  ctx.beginPath();
  ctx.moveTo(198, 170);
  ctx.lineTo(198, 56);
  ctx.stroke();

  // Shaded Hard Palate & Alveolar Ridge Vault
  const palGrad = ctx.createLinearGradient(70, 44, 164, 62);
  palGrad.addColorStop(0, "rgba(219, 231, 240, 0.34)");
  palGrad.addColorStop(1, "rgba(219, 231, 240, 0.18)");
  ctx.fillStyle = palGrad;
  ctx.strokeStyle = FILM.bone;
  ctx.lineWidth = 2.4;
  ctx.beginPath();
  ctx.moveTo(70, 68);
  ctx.quadraticCurveTo(84, 52, 116, 46); // alveolar dome -> palatal vault
  ctx.quadraticCurveTo(142, 44, 160, 48); // hard-soft junction
  ctx.lineTo(160, 55);
  ctx.quadraticCurveTo(120, 52, 88, 66); // oral roof
  ctx.quadraticCurveTo(78, 72, 70, 74);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // Hinged Velum / Uvula (seals against pharynx at 197 when velum == 0, drops forward when velum > 0)
  const velDrop = velum * 15;
  const velTipX = 196 - velum * 16;
  const velTipY = 66 + velDrop;
  ctx.fillStyle = velum > 0.12 ? "rgba(240, 102, 138, 0.42)" : "rgba(240, 102, 138, 0.24)";
  ctx.strokeStyle = velum > 0.12 ? FILM.rose : "rgba(240, 102, 138, 0.72)";
  ctx.lineWidth = 2.6;
  ctx.beginPath();
  ctx.moveTo(158, 48);
  ctx.quadraticCurveTo(182, 50 + velDrop * 0.4, velTipX, velTipY);
  ctx.arc(velTipX - 2, velTipY + 1.5, 3.2, -0.3, Math.PI * 0.9);
  ctx.quadraticCurveTo(174, 58 + velDrop * 0.45, 156, 55);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // 4. Upper & Lower Central Incisors (enamel ivory)
  ctx.fillStyle = "#f4f9fc";
  ctx.strokeStyle = "rgba(203, 221, 235, 0.5)";
  ctx.lineWidth = 1;
  // Upper incisor
  ctx.beginPath();
  ctx.moveTo(71, 70);
  ctx.lineTo(75, 70);
  ctx.lineTo(73.5, 81);
  ctx.lineTo(70.5, 80.5);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // Lower incisor (tracks mandible jawDrop)
  const lowToothY = 86 + jawDrop * 0.86;
  ctx.beginPath();
  ctx.moveTo(72, lowToothY);
  ctx.lineTo(75.5, lowToothY + 0.5);
  ctx.lineTo(76, lowToothY + 10);
  ctx.lineTo(71.5, lowToothY + 10);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // 5. Propagating Glottal Wavefronts up the Pharynx & Oral Cavity
  if (anim.voicing > 0.08) {
    for (let k = 0; k < 4; k++) {
      const u = (anim.wavePhase + k * 0.25) % 1;
      const alpha = (1 - u) * Math.min(0.45, anim.voicing * 0.48);
      ctx.strokeStyle = `rgba(242, 184, 75, ${alpha})`;
      ctx.lineWidth = 1.6;
      // Radiating past the lips
      const rx = 56 - lipProtrude - u * 34;
      const ry = (upperLipY + lowerLipY) * 0.5;
      ctx.beginPath();
      ctx.arc(rx + 12, ry, 8 + u * 24, Math.PI * 0.72, Math.PI * 1.28);
      ctx.stroke();
    }
  }

  // 6. Multi-Segment Muscular Tongue (Root -> Dorsum -> Blade -> Tip -> Floor)
  const dorsumX = 166 - tF * 54 + (isRhotic ? 8 : 0);
  const dorsumY = 110 + jawDrop * 0.42 - tH * 46;
  const bladeX = (dorsumX + 84) * 0.5 - tF * 5;
  const bladeY = Math.min(dorsumY, 96 + jawDrop * 0.5 - tTip * 30) - (isRhotic ? -5 : 3);
  const tipX = isRhotic ? 92 : 76 + (1 - tF) * 12 - (anim.phone === "TH" || anim.phone === "DH" ? 6 : 0);
  const tipY = 97 + jawDrop * 0.62 - tTip * 37;

  const tGrad = ctx.createRadialGradient(132, 124 + jawDrop * 0.3, 8, 128, 118, 68);
  tGrad.addColorStop(0, "rgba(244, 128, 158, 0.6)");
  tGrad.addColorStop(0.7, "rgba(240, 102, 138, 0.42)");
  tGrad.addColorStop(1, "rgba(184, 62, 98, 0.32)");

  ctx.fillStyle = tGrad;
  ctx.strokeStyle = FILM.rose;
  ctx.lineWidth = 2.6;
  ctx.lineJoin = "round";
  ctx.beginPath();
  ctx.moveTo(176, 154); // tongue root above larynx
  ctx.bezierCurveTo(184, 126, 180, dorsumY + 12, dorsumX, dorsumY);
  ctx.quadraticCurveTo(bladeX, bladeY, tipX, tipY);
  ctx.quadraticCurveTo(tipX - 2, tipY + 6, 82, lowToothY + 9); // sublingual frenulum
  ctx.quadraticCurveTo(116, 140 + jawDrop * 0.65, 176, 154);
  ctx.closePath();
  ctx.fill();
  ctx.stroke();

  // Subtle internal lingual muscle fiber arcs
  ctx.strokeStyle = "rgba(255, 205, 220, 0.20)";
  ctx.lineWidth = 1.2;
  ctx.beginPath();
  ctx.moveTo(162, 146);
  ctx.quadraticCurveTo(dorsumX + 4, dorsumY + 14, tipX + 8, tipY + 6);
  ctx.stroke();

  // 7. Upper & Lower Lip Vermilion Cushions (Sagittal Profile)
  ctx.fillStyle = "rgba(240, 102, 138, 0.62)";
  ctx.strokeStyle = FILM.rose;
  ctx.lineWidth = 2;
  // Upper lip cushion
  ctx.beginPath();
  ctx.ellipse(63 - lipProtrude, upperLipY - 2.5, 6.5 + lipR * 2.5, 4.5, -0.2, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();

  // Lower lip cushion (tucks back to upper incisor on F/V)
  const lowLipX = isLabiodental ? 69 : 63 - lipProtrude * 0.9;
  const lowLipDrawY = isLabiodental ? 83.5 : lowerLipY + 2.5;
  ctx.beginPath();
  ctx.ellipse(lowLipX, lowLipDrawY, 6.5 + lipR * 2.5, 4.8, 0.2, 0, Math.PI * 2);
  ctx.fill();
  ctx.stroke();

  // 8. Animated 2-Mass Vocal Folds at the Larynx
  const glotX = 187;
  const glotY = 158;
  const foldVib = anim.voicing > 0.04
    ? Math.sin(anim.glotPhase * Math.PI * 2) * (1.2 + anim.voicing * 3.2)
    : (voice?.settings.whisper ? 2.6 : 0.8);
  const foldGap = Math.max(0.4, 1.6 + foldVib + anim.noise * 1.8);

  ctx.fillStyle = anim.voicing > 0.06 ? FILM.gold : "rgba(203, 221, 235, 0.6)";
  ctx.beginPath();
  ctx.roundRect(glotX - 8 - foldGap * 0.5, glotY - 3, 6.5, 6, 2.5);
  ctx.roundRect(glotX + 1.5 + foldGap * 0.5, glotY - 3, 6.5, 6, 2.5);
  ctx.fill();

  if (anim.voicing > 0.06) {
    const gGlow = ctx.createRadialGradient(glotX, glotY, 1, glotX, glotY, 16);
    gGlow.addColorStop(0, `rgba(242, 184, 75, ${Math.min(0.65, anim.voicing * 0.65)})`);
    gGlow.addColorStop(1, "rgba(242, 184, 75, 0)");
    ctx.fillStyle = gGlow;
    ctx.beginPath();
    ctx.arc(glotX, glotY, 16, 0, Math.PI * 2);
    ctx.fill();
  }

  // 9. Turbulent Airflow Jet Particles (Fricatives, Plosive Bursts, Aspiration, Whisper)
  for (let i = 0; i < AIR_PARTICLES.length; i++) {
    const p = AIR_PARTICLES[i];
    if (!p.active) continue;
    const u = p.life / p.maxLife;
    const alpha = Math.sin(u * Math.PI) * Math.min(0.85, 0.25 + anim.noise * 0.75);
    ctx.fillStyle = `rgba(191, 227, 245, ${alpha})`;
    ctx.beginPath();
    ctx.arc(p.x, p.y, p.size, 0, Math.PI * 2);
    ctx.fill();
  }

  // What is what
  ctx.fillStyle = FILM.label;
  ctx.font = `8.5px ${UI_FONT}`;
  ctx.fillText("nasal cavity", 104, 40);
  ctx.fillText("velum", 203, 66);
  ctx.fillText("vocal folds", 202, 161);

  ctx.restore();
}

function renderVocalTract(_unusedSlice = null, nowMs = performance.now()) {
  const canvas = $("tractCanvas");
  if (!canvas) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(260, Math.floor(canvas.clientWidth * dpr));
  const h = Math.max(170, Math.floor(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const ctx = canvas.getContext("2d");

  // Film base
  const bg = ctx.createLinearGradient(0, 0, 0, h);
  bg.addColorStop(0, "#0e1a25");
  bg.addColorStop(1, FILM.base);
  ctx.fillStyle = bg;
  ctx.fillRect(0, 0, w, h);

  if (animMode === "face") {
    drawExpressiveFace(ctx, 0, 0, w, h, nowMs);
  } else if (animMode === "tract") {
    drawMidsagittalTract(ctx, 0, 0, w, h);
  } else {
    const leftW = Math.floor(w * 0.44);
    drawExpressiveFace(ctx, 0, 0, leftW, h, nowMs);
    drawMidsagittalTract(ctx, leftW, 0, w - leftW, h);

    // Where the camera view ends and the X-ray begins
    ctx.strokeStyle = FILM.line;
    ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(leftW, 12 * dpr);
    ctx.lineTo(leftW, h - 12 * dpr);
    ctx.stroke();
  }
}

let shownProgress = "0";

function startStageLoop() {
  const frame = (nowMs) => {
    if (!document.hidden) {
      let activeT = -1;
      if (isPlaying && lastSpeech && audioCtx) {
        activeT = playOffsetSec + Math.max(0, audioCtx.currentTime - playStartedAt);
      } else if (scrubTimeSec >= 0 && lastSpeech) {
        activeT = scrubTimeSec;
      }

      // the Speak button fills as the audio plays
      const progress = isPlaying && lastSpeech && lastSpeech.duration > 0
        ? Math.min(100, Math.max(0, (activeT / lastSpeech.duration) * 100)).toFixed(1)
        : "0";
      if (progress !== shownProgress) {
        shownProgress = progress;
        $("speak").style.setProperty("--progress", `${progress}%`);
      }

      if (activeT >= 0 && lastSpeech) {
        const clampedT = Math.min(activeT, lastSpeech.duration);
        setText($("timeReadout"), `${clampedT.toFixed(2)} / ${lastSpeech.duration.toFixed(2)} s`);
        const sampled = sampleSpeechAt(lastSpeech, activeT);
        stepStagePhysics(nowMs, sampled);
        updateLiveBadges(
          sampled?.word || "",
          sampled?.ipa || "",
          sampled?.phone || "",
          anim.f0,
          anim.voicing,
          sampled?.segVoice || "",
          Boolean(sampled?.segSinging),
          sampled?.segSound || ""
        );

        const spans = $("words").querySelectorAll("span[data-idx]");
        lastSpeech.words.forEach((wd, i) => {
          if (spans[i]) spans[i].classList.toggle("on", activeT >= wd.start && activeT < wd.end);
        });
        renderAcousticFrame(clampedT);
      } else {
        stepStagePhysics(nowMs, null);
        updateLiveBadges("", "", "", 0, 0);
      }
      renderVocalTract(null, nowMs);
    }
    rafId = requestAnimationFrame(frame);
  };
  cancelAnimationFrame(rafId);
  rafId = requestAnimationFrame(frame);
}

// ============================================================================
// Interactive 2D Speaker Vowel Space Editor
// ============================================================================
function getVowelMult(vName) {
  const cur = voice?.settings.vowels?.[vName];
  return cur ? [cur[0], cur[1], cur[2]] : [1, 1, 1];
}

function vowelCanvasCoords(vName, w, h) {
  const info = VOWEL_INFO[vName];
  const [m1, m2] = getVowelMult(vName);
  const padX = 28;
  const padY = 24;
  // Higher F2 multiplier = further front (left); higher F1 multiplier = more open (down)
  const nx = Math.max(0.04, Math.min(0.96, info.x - (m2 - 1) * 0.85));
  const ny = Math.max(0.06, Math.min(0.94, info.y + (m1 - 1) * 0.85));
  return [padX + nx * (w - 2 * padX), padY + ny * (h - 2 * padY)];
}

function drawVowelSpace() {
  const canvas = $("vowelCanvas");
  if (!canvas || !voice) return;
  const dpr = Math.min(window.devicePixelRatio || 1, 2);
  const w = Math.max(260, Math.floor(canvas.clientWidth * dpr));
  const h = Math.max(160, Math.floor(canvas.clientHeight * dpr));
  if (canvas.width !== w || canvas.height !== h) {
    canvas.width = w;
    canvas.height = h;
  }
  const ctx = canvas.getContext("2d");
  ctx.fillStyle = FILM.base;
  ctx.fillRect(0, 0, w, h);

  // Vowel trapezoid guide
  ctx.strokeStyle = "rgba(203, 221, 235, 0.2)";
  ctx.lineWidth = 1 * dpr;
  ctx.beginPath();
  ctx.moveTo(0.12 * w, 0.14 * h);
  ctx.lineTo(0.84 * w, 0.14 * h);
  ctx.lineTo(0.84 * w, 0.86 * h);
  ctx.lineTo(0.28 * w, 0.86 * h);
  ctx.closePath();
  ctx.stroke();

  // tongue forward on the left, mouth open at the bottom, as vowel charts are drawn
  ctx.fillStyle = FILM.label;
  ctx.font = `${11 * dpr}px ${UI_FONT}`;
  ctx.fillText("front", 10 * dpr, 15 * dpr);
  ctx.textAlign = "right";
  ctx.fillText("back", w - 10 * dpr, 15 * dpr);
  ctx.textAlign = "left";
  ctx.fillText("open", 10 * dpr, h - 9 * dpr);

  for (const name of TTS.vowelNames()) {
    if (!VOWEL_INFO[name]) continue;
    const [x, y] = vowelCanvasCoords(name, w, h);
    const [m1, m2] = getVowelMult(name);
    const shifted = Math.abs(m1 - 1) > 0.005 || Math.abs(m2 - 1) > 0.005;
    const isSel = name === selectedVowel;

    if (shifted) {
      const padX = 28;
      const padY = 24;
      const bx = padX + VOWEL_INFO[name].x * (w - 2 * padX);
      const by = padY + VOWEL_INFO[name].y * (h - 2 * padY);
      ctx.strokeStyle = "rgba(242, 184, 75, 0.55)";
      ctx.lineWidth = 1.2 * dpr;
      ctx.beginPath();
      ctx.moveTo(bx, by);
      ctx.lineTo(x, y);
      ctx.stroke();
    }

    ctx.beginPath();
    ctx.arc(x, y, (isSel ? 7 : 5) * dpr, 0, Math.PI * 2);
    ctx.fillStyle = isSel ? FILM.rose : shifted ? FILM.gold : "rgba(203, 221, 235, 0.3)";
    ctx.fill();
    ctx.strokeStyle = isSel ? "#ffffff" : "rgba(219, 231, 240, 0.7)";
    ctx.lineWidth = 1.2 * dpr;
    ctx.stroke();

    ctx.fillStyle = FILM.bone;
    ctx.font = `${14 * dpr}px ${IPA_FONT}`;
    ctx.fillText(VOWEL_INFO[name].ipa, x + 10 * dpr, y + 4.5 * dpr);
  }
}

function setupVowelCanvasInteraction() {
  const canvas = $("vowelCanvas");
  let dragging = null;
  let moved = false;

  const pickVowel = (clientX, clientY) => {
    const rect = canvas.getBoundingClientRect();
    const dpr = canvas.width / rect.width;
    const mx = (clientX - rect.left) * dpr;
    const my = (clientY - rect.top) * dpr;
    let best = null;
    let bestD = (22 * dpr) ** 2;
    for (const name of TTS.vowelNames()) {
      if (!VOWEL_INFO[name]) continue;
      const [vx, vy] = vowelCanvasCoords(name, canvas.width, canvas.height);
      const d = (mx - vx) ** 2 + (my - vy) ** 2;
      if (d < bestD) {
        bestD = d;
        best = name;
      }
    }
    return { best, mx, my, w: canvas.width, h: canvas.height };
  };

  canvas.addEventListener("pointerdown", (e) => {
    const { best } = pickVowel(e.clientX, e.clientY);
    if (!best) return;
    selectedVowel = best;
    dragging = best;
    moved = false;
    canvas.setPointerCapture(e.pointerId);
    const [m1, m2] = getVowelMult(best);
    $("vowelReadout").textContent = `/${VOWEL_INFO[best].ipa}/ as in "${VOWEL_INFO[best].word}": F1 ×${m1.toFixed(2)}, F2 ×${m2.toFixed(2)}`;
    drawVowelSpace();
  });

  canvas.addEventListener("pointermove", (e) => {
    if (!dragging) return;
    moved = true;
    const rect = canvas.getBoundingClientRect();
    const nx = Math.max(0.04, Math.min(0.96, (e.clientX - rect.left) / rect.width));
    const ny = Math.max(0.06, Math.min(0.94, (e.clientY - rect.top) / rect.height));
    const info = VOWEL_INFO[dragging];
    const m2 = Math.max(0.7, Math.min(1.35, 1 + (info.x - nx) / 0.85));
    const m1 = Math.max(0.7, Math.min(1.35, 1 + (ny - info.y) / 0.85));
    const [, , m3] = getVowelMult(dragging);
    voice.configure({ vowels: { [dragging]: [m1, m2, m3] } });
    $("vowelReadout").textContent = `/${info.ipa}/ as in "${info.word}": F1 ×${m1.toFixed(2)}, F2 ×${m2.toFixed(2)}`;
    markCustomCard();
    drawVowelSpace();
  });

  canvas.addEventListener("pointerup", () => {
    if (!dragging) return;
    const v = dragging;
    dragging = null;
    previewShortPhrase(VOWEL_INFO[v].word);
  });
}

// ============================================================================
// Synthesis, Playback, Scrubbing & Fold Comparison
// ============================================================================
function stopPlayback(clearCompare = true) {
  if (clearCompare) compareQueue = null;
  if (sourceNode) {
    sourceNode.onended = null;
    try { sourceNode.stop(); } catch {}
    sourceNode = null;
  }
  isPlaying = false;
  scrubTimeSec = -1;
  $("stop").disabled = true;
  document.documentElement.removeAttribute("data-playing");
  updateLiveBadges("", "", "", 0, 0);
  for (const el of $("words").querySelectorAll("span[data-idx]")) el.classList.remove("on");
  if (lastSpeech) {
    renderAcousticFrame(-1);
    $("timeReadout").textContent = `0.00 / ${lastSpeech.duration.toFixed(2)} s`;
  }
}

function previewShortPhrase(phrase) {
  stopPlayback(true);
  clearStickySegment();
  audioCtx ??= new AudioContext();
  audioCtx.resume();
  const sp = voice.speak(phrase);
  if (!sp.samples.length) return;
  lastSpeech = sp;
  buildAcousticCache(sp);
  playBufferFrom(sp, 0);
}

// ============================================================================
// Laughs and the other sounds that are not words
// ============================================================================
const SOUND_LABELS = new Map();
const SOUND_GROUP_NAMES = {
  laughter: "Laughter",
  breath: "Breath",
  reaction: "Reactions",
  voice: "Voice",
  mouth: "Mouth",
  body: "Body",
};
const SOUND_QUICK = ["laugh", "chuckle", "giggle", "sigh", "gasp", "hmm", "cough", "whistle"];

// Plays one sound on its own in the voice of the Voice panel, and shows it on the stage.
function auditionSound(id) {
  stopPlayback(true);
  clearStickySegment();
  audioCtx ??= new AudioContext();
  audioCtx.resume();
  let sp;
  try {
    sp = voice.sound(id);
  } catch (err) {
    $("status").textContent = `That sound could not be made: ${err.message || err}`;
    return;
  }
  if (!sp.samples.length) return;
  lastSpeech = sp;
  buildAcousticCache(sp);
  renderTranscriptWords(sp);
  $("ipa").textContent = "";
  $("status").textContent = `[${id}] ${SOUND_LABELS.get(id) || id}, ${sp.duration.toFixed(2)} s in this voice.`;
  playBufferFrom(sp, 0);
}

function insertTagAtCursor(tag) {
  const ta = $("text");
  const start = ta.selectionStart ?? ta.value.length;
  const end = ta.selectionEnd ?? ta.value.length;
  ta.value = ta.value.slice(0, start) + tag + ta.value.slice(end);
  ta.focus();
  ta.selectionStart = ta.selectionEnd = start + tag.length;
  if (studioMode === "script") savedScriptText = ta.value;
  updateLivePhonetics();
  updateApiSnippet();
}

function buildSoundPalette() {
  const sounds = TTS.sounds();
  for (const snd of sounds) SOUND_LABELS.set(snd.id, snd.label);
  const pill = (snd) => {
    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = "tag-pill";
    btn.textContent = snd.id;
    btn.title = `${snd.label}: ${snd.about}`;
    btn.onclick = () => {
      insertTagAtCursor(`[${snd.id}] `);
      if ($("soundHearCheck").checked) auditionSound(snd.id);
    };
    return btn;
  };
  $("soundQuick").replaceChildren(...SOUND_QUICK.map((id) => sounds.find((x) => x.id === id)).filter(Boolean).map(pill));
  const groups = [];
  for (const [key, name] of Object.entries(SOUND_GROUP_NAMES)) {
    const members = sounds.filter((x) => x.group === key);
    if (!members.length) continue;
    const row = document.createElement("div");
    row.className = "sound-group";
    const label = document.createElement("span");
    label.className = "dir-label";
    label.textContent = name;
    const pills = document.createElement("div");
    pills.className = "sound-pills";
    pills.append(...members.map(pill));
    row.append(label, pills);
    groups.push(row);
  }
  const note = document.createElement("p");
  note.className = "field-note sound-note";
  note.innerHTML = "Made by whoever is speaking at that point. Stronger: <code>[laugh=strong]</code>. For a set time: <code>[laugh=3s]</code>, <code>[sigh=1.5s]</code>. To laugh while speaking, use the delivery <code>[laughing]</code>.";
  $("soundPalette").replaceChildren(...groups, note);
  $("soundMoreBtn").textContent = `All ${sounds.length} sounds`;
  $("soundMoreBtn").onclick = () => {
    const open = $("soundPalette").hidden;
    $("soundPalette").hidden = !open;
    $("soundMoreBtn").setAttribute("aria-expanded", String(open));
    $("soundMoreBtn").textContent = open ? "Fewer" : `All ${sounds.length} sounds`;
  };
}

function renderTranscriptWords(speech) {
  const nodes = speech.words.flatMap((w, i) => {
    const span = document.createElement("span");
    span.textContent = w.word;
    if (w.word.startsWith("[")) span.classList.add("is-sound");
    span.dataset.idx = String(i);
    span.title = `${w.start.toFixed(2)} to ${w.end.toFixed(2)} s. Click to play from here.`;
    span.onclick = () => playBufferFrom(speech, w.start);
    return [span, " "];
  });
  $("words").replaceChildren(...nodes);
}

function playBufferFrom(speech, offsetSec = 0) {
  stopPlayback(false);
  audioCtx ??= new AudioContext();
  audioCtx.resume();
  if (!speech || speech.samples.length === 0) return;

  const clampedOffset = Math.max(0, Math.min(Math.max(0, speech.duration - 0.02), offsetSec));
  const buffer = audioCtx.createBuffer(1, speech.samples.length, speech.sampleRate);
  buffer.copyToChannel(speech.samples, 0);

  sourceNode = audioCtx.createBufferSource();
  sourceNode.buffer = buffer;
  sourceNode.connect(audioCtx.destination);

  const startAt = audioCtx.currentTime + 0.02;
  playStartedAt = startAt;
  playOffsetSec = clampedOffset;
  isPlaying = true;
  $("stop").disabled = false;
  document.documentElement.setAttribute("data-playing", "");

  sourceNode.start(startAt, clampedOffset);
  sourceNode.onended = () => {
    if (compareQueue && compareQueue.length > 0) {
      const nextStep = compareQueue.shift();
      nextStep();
      return;
    }
    if ($("loopCheck").checked && isPlaying) {
      playBufferFrom(speech, 0);
      return;
    }
    stopPlayback();
  };
}

function synthesizeCurrentText(labelPrefix = "") {
  const t0 = performance.now();
  let speech;
  try {
    if (studioMode === "sing") {
      const bpm = Number($("singBpmInput").value) || 104;
      const transpose = Number($("singTransposeInput").value) || 0;
      if (singSubView === "solo") {
        speech = voice.sing({
          bpm,
          transpose,
          notes: $("soloNotesInput").value,
          lyrics: $("soloLyricsInput").value,
        });
      } else if (singSubView === "raw") {
        speech = voice.singChorus($("rawScoreInput").value);
      } else {
        const activeParts = chorusPartsState.filter((p) => p.active && p.notes.trim());
        const sharedLyrics = $("chorusLyricsInput").value;
        speech = voice.singChorus({
          bpm,
          parts: activeParts.map((p) => ({
            name: p.name,
            ...(p.voice && p.voice !== "current" ? { voice: p.voice } : {}),
            gain: p.gain,
            transpose,
            notes: p.notes,
            lyrics: sharedLyrics,
          })),
        });
      }
    } else if (studioMode === "script" || /\[[a-zA-Z_/]/.test($("text").value)) {
      speech = voice.speakScript($("text").value);
    } else {
      speech = voice.speak($("text").value);
    }
  } catch (err) {
    $("status").textContent = `The ${studioMode === "sing" ? "score" : "script"} could not be read: ${err.message || err}`;
    return null;
  }
  const ms = Math.max(0.2, performance.now() - t0);
  lastSpeech = speech;
  $("download").disabled = speech.samples.length === 0;

  if (speech.samples.length === 0) {
    $("status").textContent = "Nothing to say yet. Type some words first.";
    return null;
  }

  if (speech.segments && speech.segments.length > 0) {
    stickySegment = speech.segments[0];
    anim.activeGender = stickySegment.gender;
    anim.activeTract = stickySegment.tractLengthCm;
    anim.activeBasePitch = stickySegment.pitchHz;
  } else {
    clearStickySegment();
  }

  const rtf = Math.round(speech.duration / (ms / 1000));
  $("perfBadge").textContent = `24 kHz mono, ${rtf}× real time`;
  const modeDesc = studioMode === "sing"
    ? (singSubView === "chorus" ? `${chorusPartsState.filter((p) => p.active).length}-part chorus` : "sung melody")
    : (speech.segments && speech.segments.length > 1 ? `${speech.segments.length} script cues` : (voice.settings.folds ? "simulated folds" : "equation pulse"));
  $("status").textContent = `${labelPrefix}${speech.duration.toFixed(2)} s rendered in ${ms.toFixed(1)} ms, ${rtf}× real time (${modeDesc})`;
  const showIpa = phoneticMode === "ipa" || studioMode !== "speech";
  $("ipa").classList.toggle("arpa", !showIpa);
  $("ipa").textContent = showIpa ? speech.ipa : voice.arpabet($("text").value);

  renderTranscriptWords(speech);
  buildAcousticCache(speech);
  renderAcousticFrame(0);
  return speech;
}

function speak(offsetSec = 0) {
  stopPlayback(true);
  const speech = synthesizeCurrentText();
  if (speech) playBufferFrom(speech, offsetSec);
}

function runFoldComparison() {
  stopPlayback(true);
  const originalFolds = voice.settings.folds;
  voice.configure({ folds: false });
  syncControls();
  const spEq = synthesizeCurrentText("Equation pulse, 1 of 2: ");
  if (!spEq) return;

  compareQueue = [
    () => {
      voice.configure({ folds: true });
      syncControls();
      const spFolds = synthesizeCurrentText("Simulated folds, 2 of 2: ");
      compareQueue = [
        () => {
          voice.configure({ folds: originalFolds });
          syncControls();
          stopPlayback();
          $("status").textContent = "Compared: the equation pulse, then the simulated folds.";
        },
      ];
      if (spFolds) playBufferFrom(spFolds, 0);
    },
  ];
  playBufferFrom(spEq, 0);
}

// ============================================================================
// Custom Word Trainer & CMUdict
// ============================================================================
function updateCustomWordInspector() {
  if (!voice) return;
  const word = $("customWordInput").value.trim().toLowerCase().replace(/[^a-z0-9']/g, "");
  if (!word) {
    $("customWordPreview").textContent = "";
    return;
  }
  const arpa = voice.arpabet(word);
  const ipa = voice.phonemize(word);
  if (!document.activeElement || document.activeElement !== $("customPhonesInput")) {
    $("customPhonesInput").value = arpa;
  }
  $("customWordPreview").textContent = `Said ${ipa} at the moment.`;
}

function renderTaughtWords() {
  const host = $("customWordsList");
  host.replaceChildren();
  for (const [w, phones] of taughtWords.entries()) {
    const pill = document.createElement("span");
    pill.className = "cw-pill";
    pill.innerHTML = `<b>${w}</b> <code>${phones}</code>`;
    pill.style.cursor = "pointer";
    pill.title = "Click to hear it";
    pill.onclick = () => previewShortPhrase(w);
    host.append(pill);
  }
}

// ============================================================================
// Main initialization
// ============================================================================
async function main() {
  await init();
  voice = new TTS("male");
  baseSettingsSnapshot = structuredCloneSettings(voice.settings);

  buildPromptChips();
  buildVoiceCards();
  buildPianoStrip();
  renderChorusTracks();
  $("rawScoreInput").value = buildSoloDslFromState();
  populateSliders("slidersEssentials", SLIDERS_ESSENTIALS);
  populateSliders("slidersPhysics", SLIDERS_PHYSICS);
  populateSliders("slidersToneMacro", SLIDERS_TONE_MACRO);
  buildEqBars();
  setupVowelCanvasInteraction();
  syncControls("male");
  updateLivePhonetics();
  updateCustomWordInspector();
  updateApiSnippet();
  renderVocalTract(null, 0);

  // Studio synthesis mode switcher (Speech / Scripted Dialogue / Sing & Chorus)
  $("modeSpeechBtn").onclick = () => setStudioMode("speech");
  $("modeScriptBtn").onclick = () => setStudioMode("script");
  $("modeSingBtn").onclick = () => setStudioMode("sing");

  buildSoundPalette();

  // Words the letter-to-sound rules get wrong, with what they should be
  for (const btn of document.querySelectorAll("#wordExamples .chip")) {
    btn.onclick = () => {
      $("customWordInput").value = btn.dataset.word;
      updateCustomWordInspector();
      $("customPhonesInput").value = btn.dataset.phones;
      $("customWordPreview").textContent += ` Teach it as ${btn.dataset.phones}, then hear it again.`;
    };
  }

  // Script director inline tag palette
  for (const btn of document.querySelectorAll("#scriptToolbar .tag-pill")) {
    btn.onclick = () => {
      const ta = $("text");
      const tag = btn.dataset.tag || "";
      const start = ta.selectionStart ?? ta.value.length;
      const end = ta.selectionEnd ?? ta.value.length;
      ta.value = ta.value.slice(0, start) + tag + ta.value.slice(end);
      ta.focus();
      ta.selectionStart = ta.selectionEnd = start + tag.length;
      updateLivePhonetics();
      updateApiSnippet();
    };
  }

  // Singing arrangement switcher & chorus controls
  $("singViewChorusBtn").onclick = () => setSingSubView("chorus");
  $("singViewSoloBtn").onclick = () => setSingSubView("solo");
  $("singViewRawBtn").onclick = () => setSingSubView("raw");
  $("addChorusPartBtn").onclick = () => {
    const idx = chorusPartsState.length + 1;
    chorusPartsState.push({
      active: true,
      name: `Voice ${idx}`,
      voice: idx % 2 === 0 ? "tenor" : "alto",
      gain: 0.85,
      notes: chorusPartsState[0]?.notes || "C4:1 E4:1 G4:2",
    });
    renderChorusTracks();
    updateApiSnippet();
  };
  for (const id of ["singBpmInput", "singTransposeInput", "chorusLyricsInput", "soloNotesInput", "soloLyricsInput", "rawScoreInput"]) {
    $(id).addEventListener("input", () => {
      updateLivePhonetics();
      updateApiSnippet();
    });
  }

  $("copyApiSnippetBtn").onclick = async () => {
    await navigator.clipboard.writeText($("apiSnippetBox").textContent);
    $("copyApiSnippetBtn").textContent = "Copied";
    setTimeout(() => {
      $("copyApiSnippetBtn").textContent = "Copy snippet";
    }, 1500);
  };

  // Pre-render visual plot of default greeting without playing audio yet
  const initialSpeech = voice.speak($("text").value);
  if (initialSpeech.samples.length > 0) {
    lastSpeech = initialSpeech;
    $("download").disabled = false;
    $("timeReadout").textContent = `0.00 / ${initialSpeech.duration.toFixed(2)} s`;
    renderTranscriptWords(initialSpeech);
    buildAcousticCache(initialSpeech);
    renderAcousticFrame(-1);
  }
  startStageLoop();

  // Stage animation view switcher (Split / Face / X-Ray)
  const setAnimMode = (mode, label) => {
    animMode = mode;
    $("animSplitBtn").classList.toggle("active", mode === "split");
    $("animFaceBtn").classList.toggle("active", mode === "face");
    $("animTractBtn").classList.toggle("active", mode === "tract");
    $("stageModeLabel").textContent = label;
    renderVocalTract();
  };
  $("animSplitBtn").onclick = () => setAnimMode("split", "Speaker and vocal tract side by side");
  $("animFaceBtn").onclick = () => setAnimMode("face", "The speaker's face");
  $("animTractBtn").onclick = () => setAnimMode("tract", "Side view through the vocal tract");

  // Theme & Tabs
  $("themeBtn").onclick = cycleTheme;
  for (const btn of document.querySelectorAll(".tab-btn")) {
    btn.onclick = () => {
      for (const b of document.querySelectorAll(".tab-btn")) {
        b.classList.toggle("active", b === btn);
        b.setAttribute("aria-selected", String(b === btn));
      }
      for (const pane of document.querySelectorAll(".tab-pane")) {
        pane.classList.toggle("active", pane.id === btn.dataset.tab);
      }
      if (btn.dataset.tab === "tabToneVowels") drawVowelSpace();
    };
  }

  // Phonetic toggle (IPA vs ARPAbet)
  $("showIpaBtn").onclick = () => {
    phoneticMode = "ipa";
    $("showIpaBtn").classList.add("active");
    $("showArpaBtn").classList.remove("active");
    updateLivePhonetics();
  };
  $("showArpaBtn").onclick = () => {
    phoneticMode = "arpabet";
    $("showArpaBtn").classList.add("active");
    $("showIpaBtn").classList.remove("active");
    updateLivePhonetics();
  };

  const updateVizLabel = () => {
    $("vizModeLabel").textContent = vizMode === "spec" ? "Spectrogram, 0 to 8 kHz" : "Waveform";
    $("legendFormants").hidden = !(vizMode === "spec" && showFormants);
  };

  $("toggleFormantsBtn").onclick = () => {
    showFormants = !showFormants;
    $("toggleFormantsBtn").classList.toggle("active", showFormants);
    updateVizLabel();
    if (lastSpeech) {
      buildAcousticCache(lastSpeech);
      renderAcousticFrame(isPlaying ? playOffsetSec : -1);
    }
  };

  // Acoustic view switcher (Spectrogram vs Waveform)
  $("viewSpecBtn").onclick = () => {
    vizMode = "spec";
    $("viewSpecBtn").classList.add("active");
    $("viewWaveBtn").classList.remove("active");
    $("toggleFormantsBtn").hidden = false;
    updateVizLabel();
    if (lastSpeech) {
      buildAcousticCache(lastSpeech);
      renderAcousticFrame(-1);
    }
  };
  $("viewWaveBtn").onclick = () => {
    vizMode = "wave";
    $("viewWaveBtn").classList.add("active");
    $("viewSpecBtn").classList.remove("active");
    $("toggleFormantsBtn").hidden = true;
    updateVizLabel();
    if (lastSpeech) {
      buildAcousticCache(lastSpeech);
      renderAcousticFrame(-1);
    }
  };

  // Click-and-drag scrubbing + seek on acoustic canvas
  let scrubbingWave = false;
  const waveTimeFromEvent = (e) => {
    const rect = $("waveCanvas").getBoundingClientRect();
    const frac = Math.max(0, Math.min(0.995, (e.clientX - rect.left) / rect.width));
    return frac * (lastSpeech?.duration || 0);
  };
  $("waveCanvas").addEventListener("pointerdown", (e) => {
    if (!lastSpeech || !lastSpeech.duration) return;
    scrubbingWave = true;
    $("waveCanvas").setPointerCapture(e.pointerId);
    stopPlayback(false);
    scrubTimeSec = waveTimeFromEvent(e);
  });
  $("waveCanvas").addEventListener("pointermove", (e) => {
    if (!scrubbingWave || !lastSpeech) return;
    scrubTimeSec = waveTimeFromEvent(e);
  });
  $("waveCanvas").addEventListener("pointerup", (e) => {
    if (!scrubbingWave || !lastSpeech) return;
    scrubbingWave = false;
    const seekTo = waveTimeFromEvent(e);
    scrubTimeSec = -1;
    playBufferFrom(lastSpeech, seekTo);
  });

  // Transport controls
  $("speak").onclick = () => speak(0);
  $("stop").onclick = () => stopPlayback(true);
  $("retake").onclick = () => {
    const nextSeed = Math.floor(Math.random() * 1e9);
    voice.configure({ seed: nextSeed });
    $("seedInput").value = nextSeed;
    speak(0);
  };
  $("compareBtn").onclick = runFoldComparison;
  $("seedInput").onchange = (e) => {
    voice.configure({ seed: Math.max(0, Math.floor(Number(e.target.value) || 0)) });
    $("seedInput").value = voice.settings.seed;
    scheduleAutoSpeak();
  };

  $("text").addEventListener("input", () => {
    for (const c of $("promptChips").children) c.classList.remove("active");
    updateLivePhonetics();
    updateApiSnippet();
  });
  document.querySelector(".composer").addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      speak(0);
    }
  });

  // Gender, Reset, Source & Preset controls
  $("male").onclick = () => {
    clearStickySegment();
    voice.setGender("male");
    baseSettingsSnapshot = structuredCloneSettings(voice.settings);
    syncControls("male");
    scheduleAutoSpeak();
  };
  $("female").onclick = () => {
    clearStickySegment();
    voice.setGender("female");
    baseSettingsSnapshot = structuredCloneSettings(voice.settings);
    syncControls("female");
    scheduleAutoSpeak();
  };
  $("randomVoiceBtn").onclick = (e) => {
    clearStickySegment();
    const opts = e.shiftKey ? { gender: voice.settings.gender } : undefined;
    const prof = voice.randomize(opts);
    baseSettingsSnapshot = structuredCloneSettings(prof);
    syncControls("");
    const nextIdx = pinnedVoiceCounter + 1;
    $("profileStatus").textContent = `A new ${prof.gender} voice: ${Math.round(prof.pitchHz)} Hz, ${prof.tractLengthCm.toFixed(1)} cm tract. Pin it to keep it as [voice_${nextIdx}].`;
    updateApiSnippet();
    speak(0);
  };
  $("pinVoiceBtn").onclick = () => {
    const idx = ++pinnedVoiceCounter;
    const id = `voice_${idx}`;
    const label = `Voice ${idx}`;
    const saved = voice.registerVoice(id, voice.settings);
    $("pinnedVoicesWrap").hidden = false;
    $("preset").append(new Option(`${label} ([${id}])`, id));

    const card = document.createElement("button");
    card.type = "button";
    card.className = "voice-card active";
    card.dataset.cardId = id;
    card.innerHTML = voiceCardHtml(label, [`${Math.round(saved.pitchHz)} Hz`, saved.gender], saved.pitchHz);
    card.title = `Use as [${id}] in dialogue and chorus`;
    card.onclick = () => {
      clearStickySegment();
      voice.configure(id);
      baseSettingsSnapshot = structuredCloneSettings(voice.settings);
      syncControls(id);
      $("profileStatus").textContent = `${label}. Use it as [${id}] in dialogue and chorus.`;
      scheduleAutoSpeak();
    };
    $("pinnedCards").append(card);
    highlightActiveCard(id);

    // Add `[voice_N]` tag pill to Scripted Dialogue toolbar & Chorus voice selector!
    const tagBtn = document.createElement("button");
    tagBtn.type = "button";
    tagBtn.className = "tag-pill";
    tagBtn.dataset.tag = `[${id}] `;
    tagBtn.textContent = id;
    tagBtn.onclick = () => {
      const ta = $("text");
      const start = ta.selectionStart ?? ta.value.length;
      const end = ta.selectionEnd ?? ta.value.length;
      ta.value = ta.value.slice(0, start) + `[${id}] ` + ta.value.slice(end);
      ta.focus();
      ta.selectionStart = ta.selectionEnd = start + id.length + 3;
      updateLivePhonetics();
      updateApiSnippet();
    };
    $("scriptToolbar").append(tagBtn);

    SINGER_VOICE_OPTIONS.splice(1, 0, [id, `${label} [${id}]`]);
    renderChorusTracks();
    $("profileStatus").textContent = `Pinned as ${label}. Use it as [${id}] in dialogue and chorus.`;
  };
  $("resetVoiceBtn").onclick = () => {
    if (activeCardId && activeCardId.startsWith("profile:")) {
      const id = activeCardId.slice(8);
      const found = PROFILES.find((p) => p[0] === id);
      loadBuiltinProfile(id, found ? found[1] : id, activeCardId);
    } else {
      selectBuiltinPreset(activeCardId || voice.settings.gender);
    }
  };
  $("preset").onchange = (e) => {
    const id = e.target.value;
    if (!id) return;
    if (id.startsWith("profile:")) {
      const pid = id.slice(8);
      const found = PROFILES.find((p) => p[0] === pid);
      loadBuiltinProfile(pid, found ? found[1] : pid, id);
    } else {
      selectBuiltinPreset(id);
    }
  };

  const setFolds = (on) => {
    voice.configure({ folds: on });
    syncControls();
    scheduleAutoSpeak();
  };
  $("srcEquationBtn").onclick = () => setFolds(false);
  $("srcFoldsBtn").onclick = () => setFolds(true);
  $("folds").onchange = (e) => setFolds(e.target.checked);
  $("whisper").onchange = (e) => {
    voice.configure({ whisper: e.target.checked });
    markCustomCard();
    scheduleAutoSpeak();
  };

  // EQ & Vowel resets
  $("flatEqBtn").onclick = () => {
    voice.configure({ tone: new Array(10).fill(0), bass: 0, presence: 0, brightness: 0 });
    syncControls();
    scheduleAutoSpeak();
  };
  $("resetVowelsBtn").onclick = () => {
    const neutral = {};
    for (const v of TTS.vowelNames()) neutral[v] = [1, 1, 1];
    voice.configure({ vowels: neutral });
    $("vowelReadout").textContent = "The vowels are back where this voice has them.";
    drawVowelSpace();
    scheduleAutoSpeak();
  };

  // Custom word trainer
  $("customWordInput").addEventListener("input", updateCustomWordInspector);
  $("sayWordBtn").onclick = () => {
    const w = $("customWordInput").value.trim();
    if (w) previewShortPhrase(w);
  };
  $("addWordBtn").onclick = () => {
    const w = $("customWordInput").value.trim().toLowerCase();
    const phones = $("customPhonesInput").value.trim();
    if (!w || !phones) return;
    try {
      voice.addWord(w, phones);
      taughtWords.set(w, voice.arpabet(w));
      renderTaughtWords();
      updateCustomWordInspector();
      updateLivePhonetics();
      previewShortPhrase(w);
    } catch (err) {
      $("customWordPreview").textContent = `Those phones were not understood: ${err.message}`;
    }
  };

  // Dictionary loader (tries local dev cache first, falls back to GitHub)
  $("dict").onclick = async () => {
    $("dict").disabled = true;
    $("dictStatus").textContent = "Loading the dictionary…";
    try {
      let text = null;
      if (location.hostname === "localhost" || location.hostname === "127.0.0.1") {
        try {
          const localRes = await fetch("../tools/.cache/cmudict.dict");
          if (localRes.ok) text = await localRes.text();
        } catch {}
      }
      if (!text) {
        const remoteRes = await fetch("https://raw.githubusercontent.com/cmusphinx/cmudict/master/cmudict.dict");
        if (!remoteRes.ok) throw new Error(`HTTP ${remoteRes.status}`);
        text = await remoteRes.text();
      }
      const count = voice.loadDictionary(text);
      $("dictStatus").textContent = `${count.toLocaleString()} pronunciations loaded.`;
      updateLivePhonetics();
      updateCustomWordInspector();
    } catch (err) {
      $("dictStatus").textContent = `The dictionary could not be loaded (${err.message}).`;
      $("dict").disabled = false;
    }
  };

  // WAV download & JSON profiles
  $("download").onclick = () => {
    if (!lastSpeech) return;
    const wavBytes = encodeWav(lastSpeech.samples, lastSpeech.sampleRate);
    const url = URL.createObjectURL(new Blob([wavBytes], { type: "audio/wav" }));
    Object.assign(document.createElement("a"), {
      href: url,
      download: `tts-${voice.gender}-${voice.settings.seed}.wav`,
    }).click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  $("saveProfile").onclick = () => {
    const { volume, variation, seed, ...profile } = voice.settings;
    const url = URL.createObjectURL(
      new Blob([JSON.stringify(profile, null, 2)], { type: "application/json" })
    );
    Object.assign(document.createElement("a"), {
      href: url,
      download: `voice-${voice.gender}.json`,
    }).click();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  $("copyProfileBtn").onclick = async () => {
    const { volume, variation, seed, ...profile } = voice.settings;
    await navigator.clipboard.writeText(JSON.stringify(profile, null, 2));
    $("profileStatus").textContent = "This voice is on the clipboard as JSON.";
  };

  $("loadProfile").onclick = () => $("profileFile").click();
  $("profileFile").onchange = async (e) => {
    const file = e.target.files[0];
    if (!file) return;
    try {
      applyProfileObject(JSON.parse(await file.text()), file.name, "");
      scheduleAutoSpeak();
    } catch (err) {
      $("profileStatus").textContent = `${file.name} could not be used: ${err.message}`;
    }
    e.target.value = "";
  };

  window.addEventListener("resize", () => {
    if (lastSpeech) {
      buildAcousticCache(lastSpeech);
      renderAcousticFrame(-1);
    }
    renderVocalTract(null, 0);
    drawVowelSpace();
  });

  $("speak").disabled = false;
  $("retake").disabled = false;
  $("compareBtn").disabled = false;
  $("modKey").textContent = IS_MAC ? "⌘" : "Ctrl";
  $("status").textContent = readyText();
  document.documentElement.dataset.ready = "1";
}

main().catch((err) => {
  $("status").textContent = `The synthesizer could not start: ${err.message}`;
  console.error(err);
});
