//! Development tool: what the simulated vocal folds (`src/folds.rs`) do.
//!
//!   cargo run --release --example folds
//!       tables of pitch, open quotient and flow over tension, lung pressure, rest gap and zip
//!   cargo run --release --example folds -- wave     male 0.75 464 0.00016 0.00012 0.7e-6 [peel]
//!       the flow through two cycles (the first number is the tension)
//!   cargo run --release --example folds -- spectrum male 110 464 0.00016 0.00012 0.7e-6 [peel]
//!       the harmonics of the source at a pitch, next to those of the equation pulse
//!   cargo run --release --example folds -- track    male 110 464 0.00016 0.00012 0.7e-6 [peel]
//!       does the pitch asked for come out, over the range of the voice?
//!   cargo run --release --example folds -- glide    male 110 464 0.00016 0.00012 0.7e-6 [peel]
//!       is the voice steady from cycle to cycle while the pitch moves?
//!
//! The numbers after the voice are: tension or pitch (Hz), lung pressure (Pa), half the gap
//! between the folds at rest (m), the zip (m), the area of the leak (m^2) and, optionally,
//! the peel (m).

use tts::folds::{steady, Drive, Folds, Larynx, Motion, Voicing};

const RATE: f64 = 24_000.0;
const SIM_RATE: f64 = 96_000.0;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(mode) = args.first().map(String::as_str) else {
        return tables();
    };
    let num = |i: usize| args.get(i).and_then(|v| v.parse::<f64>().ok());
    let (Some(first), Some(pressure), Some(gap), Some(zip), Some(leak)) = (num(2), num(3), num(4), num(5), num(6)) else {
        eprintln!("usage: folds [wave|spectrum|track|glide] male|female <tension or pitch> <pressure> <gap> <zip> <leak> [peel]");
        std::process::exit(2);
    };
    let female = args[1] == "female";
    let folds = if female { Folds::FEMALE } else { Folds::MALE };
    let usual = Drive { tension: 1.0, pressure, gap, zip, peel: num(7).unwrap_or(0.0), leak };
    match mode {
        "wave" => wave(&folds, &Drive { tension: first, ..usual }),
        "spectrum" => spectrum(&folds, &usual, first, female),
        "track" => track(&folds, &usual, first),
        "glide" => glide(&folds, &usual, first),
        _ => eprintln!("unknown mode {mode:?}"),
    }
}

/// The flow through two cycles.
fn wave(f: &Folds, d: &Drive) {
    let mut m = Motion { x1: 1e-5, ..Motion::default() };
    for _ in 0..(0.15 * SIM_RATE) as usize {
        m.step(f, d, 1.0 / SIM_RATE);
    }
    let mut prev = m.flow;
    for i in 0..(0.02 * SIM_RATE) as usize {
        let u = m.step(f, d, 1.0 / SIM_RATE);
        if i % 8 == 0 {
            println!("{:7.3} ms  flow {:7.1} cm3/s  rate {:9.1} L/s2  x1 {:6.3} mm  x2 {:6.3} mm", i as f64 / SIM_RATE * 1e3, u * 1e6, (u - prev) * SIM_RATE * 1e3, m.x1 * 1e3, m.x2 * 1e3);
        }
        prev = u;
    }
}

/// The harmonics of the source, next to those of the equation pulse of the default voice.
fn spectrum(f: &Folds, usual: &Drive, pitch: f64, female: bool) {
    let larynx = Larynx::new(*f, *usual, *usual, RATE, pitch);
    let mut v = Voicing::default();
    larynx.start(&mut v, pitch, usual.pressure, usual.gap, 0.05);
    let x: Vec<f64> = (0..RATE as usize).map(|_| larynx.tick(&mut v, pitch, usual.pressure, usual.gap, 1.0).0).skip(RATE as usize / 2).collect();
    let s = steady(f, &Drive { tension: larynx.tension_for(pitch), ..*usual }, SIM_RATE, 0.1, 0.1);
    println!("folds: {:.1} Hz  open {:.2}  peak flow {:.0} cm3/s  mean {:.0}", s.f0, s.open_quotient, s.peak_flow * 1e6, s.mean_flow * 1e6);
    print_harmonics("folds", &x, pitch);
    let (closure, warm) = if female { (0.25, 0.62 * 0.76) } else { (0.75, 0.56 * 0.64) };
    let table = tts::render::equation_pulse(0.66, closure, warm);
    let period = RATE / pitch;
    let y: Vec<f64> = (0..RATE as usize / 2).map(|i| table[((i as f64 / period).fract() * table.len() as f64) as usize % table.len()] as f64).collect();
    print_harmonics("equation", &y, pitch);
}

/// Levels of the harmonics of a steady source, in dB relative to the first.
fn print_harmonics(name: &str, x: &[f64], pitch: f64) {
    let level = |k: usize| {
        let w = std::f64::consts::TAU * pitch * k as f64 / RATE;
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &v) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / x.len() as f64).cos();
            re += v * win * (w * i as f64).cos();
            im += v * win * (w * i as f64).sin();
        }
        10.0 * (re * re + im * im).max(1e-30).log10()
    };
    let h1 = level(1);
    let near = |hz: f64| {
        let k = (hz / pitch).round().max(2.0) as usize;
        (level(k - 1) + level(k) + level(k + 1)) / 3.0 - h1
    };
    println!(
        "{name:9} H2 {:+5.1}  H3 {:+5.1}  H4 {:+5.1} | 0.5k {:+5.1}  1k {:+5.1}  2k {:+5.1}  3k {:+5.1}  4k {:+5.1}  5k {:+5.1}  7k {:+5.1}  (dB re the fundamental)",
        level(2) - h1, level(3) - h1, level(4) - h1, near(500.0), near(1000.0), near(2000.0), near(3000.0), near(4000.0), near(5000.0), near(7000.0)
    );
}

/// Does the pitch asked for come out, over the range of the voice?
fn track(f: &Folds, usual: &Drive, base: f64) {
    let larynx = Larynx::new(*f, *usual, *usual, RATE, base);
    println!("range {:.0} to {:.0} Hz", larynx.pitch_range().0, larynx.pitch_range().1);
    for st in [-8.0, -4.0, 0.0, 4.0, 8.0, 12.0] {
        let pitch = base * 2f64.powf(st / 12.0);
        let pressure = usual.pressure * 2f64.powf(0.5 * st / 12.0);
        let mut v = Voicing::default();
        larynx.start(&mut v, pitch, pressure, usual.gap, 0.02);
        let (mut last, mut periods, mut peak) = (0usize, Vec::new(), 0.0f64);
        for i in 0..RATE as usize {
            let (e, _, began) = larynx.tick(&mut v, pitch, pressure, usual.gap, 1.0);
            if began {
                if i > 6_000 {
                    periods.push((i - last) as f64);
                }
                last = i;
            }
            if i > 6_000 {
                peak = peak.max(e.abs());
            }
        }
        let mean = periods.iter().sum::<f64>() / periods.len().max(1) as f64;
        println!("  asked {pitch:6.1} Hz ({st:+3.0} st, {pressure:5.0} Pa): got {:6.1} Hz  peak excitation {peak:5.2}", RATE / mean);
    }
}

/// Is the voice steady from cycle to cycle while the pitch glides over an octave?
fn glide(f: &Folds, usual: &Drive, base: f64) {
    let larynx = Larynx::new(*f, *usual, *usual, RATE, base);
    let mut v = Voicing::default();
    larynx.start(&mut v, base, usual.pressure, usual.gap, 0.02);
    let (mut peak, mut last, mut rows) = (0.0f64, 0usize, Vec::new());
    for i in 0..2 * RATE as usize {
        let st = 6.0 * (std::f64::consts::TAU * i as f64 / RATE * 1.2).sin();
        let pitch = base * 2f64.powf(st / 12.0);
        let (e, _, began) = larynx.tick(&mut v, pitch, usual.pressure * 2f64.powf(0.5 * st / 12.0), usual.gap, 1.0);
        peak = peak.max(-e);
        if began {
            rows.push((st, (i - last) as f64, peak, RATE / pitch));
            last = i;
            peak = 0.0;
        }
    }
    let rows = &rows[4.min(rows.len())..];
    for (lo, hi) in [(-6.0, -3.0), (-3.0, 0.0), (0.0, 3.0), (3.0, 6.0)] {
        let part: Vec<&(f64, f64, f64, f64)> = rows.iter().filter(|r| r.0 >= lo && r.0 < hi).collect();
        let n = part.len().max(2) as f64;
        let mean_peak = part.iter().map(|r| r.2).sum::<f64>() / n;
        let shimmer = part.windows(2).map(|w| (w[1].2 - w[0].2).abs()).sum::<f64>() / (n - 1.0) / mean_peak.max(1e-9);
        let off = part.iter().map(|r| (r.1 / r.3 - 1.0).abs()).sum::<f64>() / n;
        println!("  {lo:+.0} to {hi:+.0} st: {:3} cycles  closing peak {mean_peak:5.2}  change from cycle to cycle {:4.1}%  period off the one asked for by {:4.1}%", part.len(), 100.0 * shimmer, 100.0 * off);
    }
}

/// Pitch, open quotient and flow over tension, lung pressure, rest gap, zip and peel.
fn tables() {
    for (name, f) in [("male", Folds::MALE), ("female", Folds::FEMALE)] {
        let base = Drive { tension: 1.0, pressure: 800.0, gap: 0.18e-3, zip: 0.0, peel: 0.0, leak: 0.0 };
        let row = |label: String, d: Drive| {
            let s = steady(&f, &d, SIM_RATE, 0.15, 0.1);
            println!("  {label:16} {:6.1} Hz  open {:4.2}  peak {:5.0} cm3/s  mean {:5.0}  sharpest fall {:8.1} L/s2", s.f0, s.open_quotient, s.peak_flow * 1e6, s.mean_flow * 1e6, s.max_fall * 1e3);
        };
        println!("=== {name} (800 Pa, gap 0.18 mm unless varied)");
        for q in [0.5, 0.7, 1.0, 1.4, 2.0, 2.8] {
            row(format!("tension {q}"), Drive { tension: q, ..base });
        }
        for ps in [200.0, 400.0, 600.0, 1200.0, 1600.0] {
            row(format!("{ps} Pa"), Drive { pressure: ps, ..base });
        }
        for gap in [-0.1e-3, 0.0, 0.1e-3, 0.3e-3, 0.5e-3] {
            row(format!("gap {:.2} mm", gap * 1e3), Drive { gap, ..base });
        }
        for zip in [0.05e-3, 0.1e-3, 0.2e-3] {
            row(format!("zip {:.2} mm", zip * 1e3), Drive { zip, ..base });
        }
        for peel in [0.2e-3, 0.4e-3] {
            row(format!("peel {:.1} mm", peel * 1e3), Drive { peel, ..base });
        }
    }
}
