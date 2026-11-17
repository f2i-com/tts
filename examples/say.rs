//! Native command-line front end for the synthesizer.
//!
//!   cargo run --release --example say -- "Hello there!" hello.wav
//!   cargo run --release --example say -- --voice female "Hello there!" hello.wav
//!   cargo run --release --example say -- --voice deep --speed 0.9 "Hello." out.wav
//!   cargo run --release --example say -- --phones "Hello there!"      (print pronunciation only)
//!   cargo run --release --example say -- --batch jobs.tsv             (voice<TAB>out.wav<TAB>text per line)
//!
//! `--dict cmudict.dict` loads a full pronouncing dictionary first.
//! `--timing` prints `token phone start_ms end_ms` for every phone of the utterance.
//! `--frames` prints the control values of every 5 ms frame (what the renderer is told to do).
//! `--levels` (batch mode) prints the average level of every phone relative to the vowels.
//! In batch mode one line with the ARPAbet transcription is printed per job; an output
//! path of `-` skips synthesis (transcription only).

use std::time::Instant;
use tts::{Gender, Lexicon, TTS, VoiceConfig, PRESETS};

fn voice_by_name(name: &str) -> Option<VoiceConfig> {
    Gender::parse(name)
        .map(VoiceConfig::for_gender)
        .or_else(|| VoiceConfig::preset(name))
}

fn main() {
    if !tts::DEV_HOOKS {
        // a measurement must never run on silently ignored overrides
        for var in ["VS_TUNE", "VS_DUR_FILE", "VS_F0_FILE", "VS_FORMANT_FILE", "VS_FRAMES_DUMP", "VS_FEATURES_FILE"] {
            if std::env::var_os(var).is_some_and(|v| !v.is_empty()) {
                eprintln!("{var} is set, but this build has no development hooks: build with --features dev-hooks");
                std::process::exit(2);
            }
        }
    }
    let mut args = std::env::args().skip(1);
    let mut cfg = VoiceConfig::male();
    let mut positional: Vec<String> = Vec::new();
    let mut phones_only = false;
    let mut folds = false;
    let mut core_only = false;
    let mut script_mode = false;
    let mut sing_mode = false;
    let mut chorus_mode = false;
    let mut dict: Option<String> = None;
    let mut batch: Option<String> = None;
    let mut levels = false;
    let mut timing = false;
    let mut dump_frames = false;
    let mut overrides: Vec<(String, f64)> = Vec::new();
    let mut tone: Option<Vec<f64>> = None;
    let mut seed: Option<u32> = None;
    let mut formants: Option<Vec<f64>> = None;
    let mut vowels: Option<String> = None;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--voice" => {
                let name = args.next().expect("--voice needs male, female or a preset id");
                cfg = voice_by_name(&name).unwrap_or_else(|| {
                    let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
                    eprintln!("unknown voice {name:?}; presets: {}", ids.join(", "));
                    std::process::exit(2);
                });
            }
            "--speed" | "--pitch" | "--expressiveness" | "--breathiness" | "--warmth" | "--roughness"
            | "--vibrato" | "--tract" | "--nasality" | "--chest" | "--crispness" | "--bass" | "--presence"
            | "--brightness" | "--variation" | "--pauses" | "--breaths" | "--open" | "--subglottal" | "--room" | "--pitch-coupling" | "--air" | "--closure" | "--creak" => {
                let v: f64 = args.next().and_then(|v| v.parse().ok()).expect("option needs a number");
                overrides.push((arg[2..].to_string(), v));
            }
            // which random "take" to speak
            "--seed" => seed = args.next().and_then(|v| v.parse::<u32>().ok()),
            // ten comma-separated gains in dB, one per tone band
            "--tone" => {
                let list = args.next().expect("--tone a,b,c,...");
                tone = Some(list.split(',').filter_map(|v| v.trim().parse::<f64>().ok()).collect());
            }
            // five multipliers for F1..F5 of the whole voice
            "--formants" => {
                let list = args.next().expect("--formants f1,f2,f3,f4,f5");
                formants = Some(list.split(',').filter_map(|v| v.trim().parse::<f64>().ok()).collect());
            }
            // per-vowel multipliers: IY=1.02/0.97/1.00,UW=0.95/1.20/1.05
            "--vowels" => vowels = Some(args.next().expect("--vowels IY=f1/f2/f3,...")),
            "--folds" => folds = true,
            "--script" => script_mode = true,
            "--sing" => sing_mode = true,
            "--chorus" => chorus_mode = true,
            "--phones" => phones_only = true,
            "--levels" => levels = true,
            "--timing" => timing = true,
            "--frames" => dump_frames = true,
            // Use only the hand-tuned core lexicon + rules.
            "--core-lexicon" => core_only = true,
            "--dict" => dict = Some(args.next().expect("--dict <file>")),
            "--batch" => batch = Some(args.next().expect("--batch <file>")),
            _ => positional.push(arg),
        }
    }

    let apply = |cfg: &mut VoiceConfig| {
        for (key, v) in &overrides {
            match key.as_str() {
                "speed" => cfg.speed = *v,
                "pitch" => cfg.pitch_hz = *v,
                "expressiveness" => cfg.expressiveness = *v,
                "breathiness" => cfg.breathiness = *v,
                "warmth" => cfg.warmth = *v,
                "roughness" => cfg.roughness = *v,
                "vibrato" => cfg.vibrato = *v,
                "tract" => cfg.tract_length_cm = *v,
                "nasality" => cfg.nasality = *v,
                "chest" => cfg.chest_resonance = *v,
                "crispness" => cfg.crispness = *v,
                "bass" => cfg.bass = *v,
                "presence" => cfg.presence = *v,
                "brightness" => cfg.brightness = *v,
                "variation" => cfg.variation = *v,
                "pauses" => cfg.pauses = *v,
                "breaths" => cfg.breaths = *v,
                "open" => cfg.open_quotient = *v,
                "subglottal" => cfg.subglottal = *v,
                "room" => cfg.room = *v,
                "pitch-coupling" => cfg.pitch_coupling = *v,
                "air" => cfg.air = *v,
                "closure" => cfg.closure = *v,
                "creak" => cfg.creak = *v,
                _ => {}
            }
        }
        if folds {
            cfg.folds = true;
        }
        if let Some(s) = seed {
            cfg.seed = s;
        }
        if let Some(t) = &tone {
            for (dst, src) in cfg.tone.iter_mut().zip(t.iter()) {
                *dst = *src;
            }
        }
        if let Some(f) = &formants {
            for (dst, src) in cfg.formants.iter_mut().zip(f.iter()) {
                *dst = *src;
            }
        }
        if let Some(list) = &vowels {
            for item in list.split(',') {
                let mut parts = item.split(['=', '/']);
                let name = parts.next().unwrap_or("").trim().to_ascii_uppercase();
                let nums: Vec<f64> = parts.filter_map(|p| p.trim().parse().ok()).collect();
                let index = tts::phone::Ph::parse_bytes(name.as_bytes()).and_then(tts::config::vowel_index);
                if let (Some(i), 3) = (index, nums.len()) {
                    cfg.vowels[i] = [nums[0], nums[1], nums[2]];
                }
            }
        }
    };
    apply(&mut cfg);

    let mut lexicon = if core_only { Lexicon::core_only() } else { Lexicon::new() };
    if let Some(path) = &dict {
        let text = std::fs::read_to_string(path).expect("could not read dictionary");
        lexicon.load_dictionary(&text);
    }
    let mut synth = TTS::with_lexicon(cfg, lexicon);

    if let Some(path) = batch {
        let jobs = std::fs::read_to_string(&path).expect("could not read batch file");
        // phone name -> (sum of dB relative to the utterance's vowels, occurrences, frames)
        let mut stats: std::collections::BTreeMap<String, (f64, usize, usize)> = Default::default();
        for line in jobs.lines() {
            let mut parts = line.splitn(3, '\t');
            let (Some(voice), Some(out), Some(text)) = (parts.next(), parts.next(), parts.next()) else {
                continue;
            };
            let mut cfg = voice_by_name(voice).unwrap_or_else(|| panic!("unknown voice {voice}"));
            apply(&mut cfg);
            cfg.sanitize();
            synth.config = cfg;
            println!("{}", synth.plan(text).arpabet());
            if out != "-" {
                let speech = synth.speak(text);
                std::fs::write(out, speech.to_wav()).expect("could not write wav file");
                if levels {
                    let spf = tts::SAMPLES_PER_FRAME;
                    let mut per: std::collections::BTreeMap<String, (f64, usize)> = Default::default();
                    let (mut vowel_e, mut vowel_n) = (0.0f64, 0usize);
                    for (i, f) in speech.frames.iter().enumerate() {
                        let e: f64 = speech.samples[i * spf..(i + 1) * spf].iter().map(|&v| (v as f64).powi(2)).sum();
                        let mut keys = vec![f.phone.name().to_string()];
                        if matches!(f.phone.name(), "P" | "B" | "T" | "D" | "K" | "G" | "CH" | "JH") {
                            keys.push(format!("{}.{}", f.phone.name(), if f.is_closure { "cl" } else { "rel" }));
                        }
                        for key in keys {
                            let entry = per.entry(key).or_default();
                            entry.0 += e;
                            entry.1 += 1;
                        }
                        if f.phone.is_vowel() {
                            vowel_e += e;
                            vowel_n += 1;
                        }
                    }
                    if vowel_n > 0 && vowel_e > 0.0 {
                        let vowel_power = vowel_e / vowel_n as f64;
                        for (name, (e, n)) in per {
                            let db = 10.0 * ((e / n as f64).max(1e-12) / vowel_power).log10();
                            let st = stats.entry(name).or_default();
                            st.0 += db;
                            st.1 += 1;
                            st.2 += n;
                        }
                    }
                }
            }
        }
        if levels {
            eprintln!("phone level (dB re vowels of the same utterance), mean duration (ms)");
            for (name, (db, count, frames)) in &stats {
                eprintln!("{name:6} {:+6.1} dB  {:5.0} ms  n={count}", db / *count as f64, *frames as f64 * 5.0 / *count as f64);
            }
        }
        return;
    }

    let text = positional.first().cloned().unwrap_or_else(|| {
        "Hello! This voice is computed entirely from a model of the human vocal tract.".to_string()
    });

    if timing {
        let plan = synth.plan(&text);
        let timeline = tts::timeline::build_timeline(&plan, &synth.config);
        let mut frame = 0usize;
        let mut open: Option<(i32, &'static str, usize)> = None;
        for seg in &timeline.segments {
            let n = ((seg.s.dur / 5.0 + 0.5).floor() as usize).max(1);
            if let Some((tok, name, start)) = open {
                if tok != seg.token {
                    println!("{tok} {name} {} {}", start * 5, frame * 5);
                    open = None;
                }
            }
            if seg.token >= 0 && open.is_none() {
                open = Some((seg.token, seg.phone.name(), frame));
            }
            frame += n;
        }
        if let Some((tok, name, start)) = open {
            println!("{tok} {name} {} {}", start * 5, frame * 5);
        }
        println!("frames {frame}");
        // pitch (Hz) of every 5 ms frame, 0 where unvoiced
        let frames = tts::frames::rasterize(&timeline, &synth.config);
        let f0: Vec<String> = frames
            .iter()
            .map(|f| if f.av > 0.3 { format!("{:.1}", f.f0) } else { "0".to_string() })
            .collect();
        println!("f0 {}", f0.join(" "));
    }

    if phones_only {
        let plan = synth.plan(&text);
        println!("text:    {}", plan.normalized);
        println!("ipa:     {}", plan.ipa);
        println!("arpabet: {}", plan.arpabet());
        return;
    }

    let t0 = Instant::now();
    let speech = if chorus_mode {
        synth.sing_chorus(&text).expect("invalid chorus score").speech
    } else if sing_mode {
        synth.sing(&text).expect("invalid singing score")
    } else if script_mode {
        synth.speak_script(&text).speech
    } else {
        synth.speak(&text)
    };
    let elapsed = t0.elapsed().as_secs_f64();
    println!("text: {}", speech.text);
    println!("ipa:  {}", speech.ipa);
    if dump_frames {
        println!("frame  ms phone  f0    f1   f2   f3   b1  av    ah    af    avb   nasal lax  creak fricf1 g1   g2   hp   flags level");
        for (i, f) in speech.frames.iter().enumerate() {
            println!(
                "{i:5} {:4} {:5} {:5.1} {:4.0} {:4.0} {:4.0} {:4.0} {:.3} {:.3} {:.3} {:.3} {:.2}  {:.2} {:.2}  {:5.0} {:.2} {:.2} {:4.0} {}{} {:.3}",
                i * 5,
                f.phone.name(),
                f.f0,
                f.f1,
                f.f2,
                f.f3,
                f.b1,
                f.av,
                f.ah,
                f.af,
                f.avb,
                f.nasal,
                f.lax,
                f.creak,
                f.fric_f1,
                f.fric_g1,
                f.fric_g2,
                f.fric_hp,
                if f.is_closure { 'c' } else { '-' },
                if f.is_burst { 'b' } else { '-' },
                f.rms,
            );
        }
    }
    println!(
        "{:.2} s of audio rendered in {:.1} ms ({:.0}x real time)",
        speech.duration(),
        elapsed * 1000.0,
        speech.duration() / elapsed.max(1e-9)
    );
    if let Some(path) = positional.get(1) {
        std::fs::write(path, speech.to_wav()).expect("could not write wav file");
        println!("wrote {path}");
    }
}
