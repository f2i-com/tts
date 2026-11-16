//! The vocal folds, simulated.
//!
//! Each fold is two small masses on springs - its lower and its upper edge - coupled by a
//! third spring (Ishizaka & Flanagan 1972, in the form of Steinecke & Herzel 1995). The lung
//! pressure blows them apart, their elasticity and the suction of the air stream pull them
//! back, and because the upper edge follows the lower one late, the stream feeds the motion
//! instead of damping it: they oscillate by themselves. Nothing here says what a glottal
//! pulse looks like. It comes out of the tension and mass of the folds, how firmly they are
//! held together, the lung pressure, and four things added to the classic model:
//!
//! * the air above and below the folds has inertia, so the flow lags the opening and is cut
//!   off late and fast: the pulse leans to the right, as measured pulses do;
//! * the folds do not meet along their whole length at once, they close like a zip, which
//!   rounds the corner at closure by an amount that can be set;
//! * they part more gradually than they close - they peel apart - so that the pulse has one
//!   sharp edge, the closing one, and not a second at the opening;
//! * a gap at the back never closes, and lets a steady stream of air through beside the
//!   slit between the folds (breathiness).
//!
//! Everything is in SI units. The simulation runs at a multiple of the audio rate
//! ([`OVERSAMPLE`]) and its output, the rate of change of the flow, is band-limited before
//! it is handed over as the excitation of the vocal tract.

/// Density of air, kg/m^3, and its viscosity, Pa s.
const RHO: f64 = 1.13;
const VISCOSITY: f64 = 1.86e-5;
/// Simulation steps per audio sample.
pub const OVERSAMPLE: usize = 4;

/// One speaker's folds and the air column they work against.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Folds {
    /// Length of the vibrating part, m.
    pub length: f64,
    /// Mass of the lower and of the upper edge, kg.
    pub m1: f64,
    pub m2: f64,
    /// Their springs and the spring between them, N/m.
    pub k1: f64,
    pub k2: f64,
    pub kc: f64,
    /// Their damping, N s/m.
    pub r1: f64,
    pub r2: f64,
    /// Thickness of the lower and of the upper edge, m.
    pub d1: f64,
    pub d2: f64,
    /// Stiffness of two folds pressing on each other, as a multiple of their own.
    pub collision: f64,
    /// Inertance of the air the flow has to set in motion, kg/m^4, and its resistance.
    pub inertance: f64,
    pub resistance: f64,
}

impl Folds {
    /// A man's folds: the standard values of the two-mass model, a little less damped and
    /// with a thinner lower edge (a search on the naturalness predictor and the phoneme
    /// recognizer preferred both), against a lighter column of air.
    pub const MALE: Folds = Folds {
        length: 0.014,
        m1: 1.25e-4,
        m2: 2.5e-5,
        k1: 80.0,
        k2: 8.0,
        kc: 25.0,
        r1: 0.0162,
        r2: 0.0162,
        d1: 2.04e-3,
        d2: 0.5e-3,
        collision: 3.0,
        inertance: 870.0,
        resistance: 2.0e4,
    };
    /// A woman's: shorter, thinner and lighter.
    pub const FEMALE: Folds = Folds {
        length: 0.010,
        m1: 0.6e-4,
        m2: 1.2e-5,
        k1: 80.0,
        k2: 8.0,
        kc: 25.0,
        r1: 0.014,
        r2: 0.014,
        d1: 2.0e-3,
        d2: 0.4e-3,
        collision: 3.0,
        inertance: 1400.0,
        resistance: 2.5e4,
    };
}

/// What the speaker does with the folds at this instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drive {
    /// Tension: 1 = at rest; the pitch rises in proportion (stiffer and, stretched, lighter).
    pub tension: f64,
    /// Lung pressure, Pa.
    pub pressure: f64,
    /// Half the gap between the folds at rest, m: negative = pressed together.
    pub gap: f64,
    /// How much wider the gap is at the back than at the front, m: the folds close like a
    /// zip over this distance (0 = all at once).
    pub zip: f64,
    /// The same for the way they part, m, on top of `zip`: folds peel apart more gradually
    /// than they slap shut, which is why a real pulse has one sharp edge, not two.
    pub peel: f64,
    /// Area of the gap that never closes, m^2.
    pub leak: f64,
}

/// Area of the opening and mean depth of contact for a mean half-gap `g` that varies by
/// `zip` along a length `l`. Both are smooth in `g`.
#[inline]
fn opening(g: f64, zip: f64, l: f64) -> (f64, f64) {
    let (hi, lo) = (g + 0.5 * zip, g - 0.5 * zip);
    if lo >= 0.0 {
        (2.0 * l * g, 0.0)
    } else if hi <= 0.0 {
        (0.0, -g)
    } else {
        (l * hi * hi / zip, lo * lo / (2.0 * zip))
    }
}

/// Where the folds are and how much air is passing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub x1: f64,
    pub v1: f64,
    pub x2: f64,
    pub v2: f64,
    /// Flow through the glottis, m^3/s, and the pressure across it, Pa.
    pub flow: f64,
    pub across: f64,
}

impl Motion {
    /// 1 while the narrower of the two edges is moving outwards, 0 while it moves inwards.
    #[inline]
    fn parting(&self) -> f64 {
        let v = if self.x1 < self.x2 { self.v1 } else { self.v2 };
        (0.5 + v / 0.1).clamp(0.0, 1.0)
    }

    /// Advances by `dt` seconds; returns the flow.
    #[inline]
    pub fn step(&mut self, f: &Folds, d: &Drive, dt: f64) -> f64 {
        // (parting or closing: by the edge that is the narrower, the one the air has to pass)
        let zip = d.zip + d.peel * self.parting();
        let (a1, pen1) = opening(self.x1 + d.gap, zip, f.length);
        let (a2, pen2) = opening(self.x2 + d.gap, zip, f.length);
        let a_min = a1.min(a2).max(0.0);

        // The flow. The slit between the folds and the leak behind them share the pressure
        // across the glottis: in the slit it is lost in the jet and, as the slit narrows, to
        // friction (which rounds off the end of each pulse); in the leak in the jet alone.
        // What is left of the lung pressure moves the air column above. One equation in
        // that pressure, solved at every step (stable however fast the slit shuts).
        let b = f.inertance / dt + f.resistance;
        let drive = d.pressure + f.inertance * self.flow / dt;
        let (jet, slit) = if a_min > 1e-12 {
            (0.5 * RHO / (a_min * a_min), 12.0 * VISCOSITY * (f.d1 + f.d2) * f.length * f.length / (a_min * a_min * a_min))
        } else {
            (0.0, 0.0)
        };
        let leak = d.leak * (2.0 / RHO).sqrt();
        let (mut across, mut through) = (0.0, 0.0);
        if drive > 0.0 && (a_min > 1e-12 || d.leak > 0.0) {
            // (in the square root of the pressure the equation is convex and rising, so
            // Newton's method cannot overshoot: a few steps from the last value settle it)
            let top = drive.sqrt();
            let mut y = if self.across > 0.0 { self.across.sqrt().min(top) } else { top };
            for _ in 0..6 {
                let root = (slit * slit + 4.0 * jet * y * y).sqrt();
                let (flow_slit, slope_slit) = if a_min > 1e-12 { (2.0 * y * y / (slit + root), 2.0 * y / root) } else { (0.0, 0.0) };
                let error = flow_slit + leak * y - (drive - y * y) / b;
                let step = error / (slope_slit + leak + 2.0 * y / b);
                y = (y - step).clamp(0.0, top);
                if step.abs() < 1e-9 * top {
                    break;
                }
            }
            across = y * y;
            if a_min > 1e-12 {
                through = 2.0 * across / (slit + (slit * slit + 4.0 * jet * across).sqrt());
            }
        }
        let flow = through + leak * across.sqrt();
        let above = d.pressure - across;
        self.across = across;
        self.flow = flow;

        // Pressure on the lower edge: the full lung pressure while the upper edge is shut,
        // less where the stream is fast (Bernoulli), nothing once the stream has left the wall.
        let p1 = if a1 <= 0.0 {
            0.0
        } else if a_min <= 0.0 {
            d.pressure
        } else {
            (above + 0.5 * RHO * through * through * (1.0 / (a_min * a_min) - 1.0 / (a1 * a1))).min(d.pressure)
        };
        let p2 = if a2 > 0.0 { above } else { 0.0 };

        let q = d.tension;
        let (k1, k2, kc) = (f.k1 * q, f.k2 * q, f.kc * q);
        let f1 = f.length * f.d1 * p1 - k1 * self.x1 - f.r1 * self.v1 - kc * (self.x1 - self.x2) + f.collision * k1 * pen1;
        let f2 = f.length * f.d2 * p2 - k2 * self.x2 - f.r2 * self.v2 - kc * (self.x2 - self.x1) + f.collision * k2 * pen2;
        self.v1 += dt * f1 * q / f.m1;
        self.x1 += dt * self.v1;
        self.v2 += dt * f2 * q / f.m2;
        self.x2 += dt * self.v2;
        flow
    }
}

/// One simulated stretch of steady voice, measured.
#[derive(Clone, Copy, Debug, Default)]
pub struct Steady {
    /// Pitch, Hz (0 = the folds did not oscillate).
    pub f0: f64,
    /// Share of the cycle with air flowing through the folds themselves.
    pub open_quotient: f64,
    /// Peak and mean flow, m^3/s.
    pub peak_flow: f64,
    pub mean_flow: f64,
    /// Root mean square of the flow's rate of change, m^3/s^2, and its sharpest fall.
    pub rms_rate: f64,
    pub max_fall: f64,
}

/// Lets the folds settle for `settle` seconds and measures the next `measure` seconds.
pub fn steady(f: &Folds, d: &Drive, rate: f64, settle: f64, measure: f64) -> Steady {
    let dt = 1.0 / rate;
    let mut m = Motion { x1: 1e-5, ..Motion::default() };
    for _ in 0..(settle * rate) as usize {
        m.step(f, d, dt);
    }
    let n = (measure * rate) as usize;
    let (mut open, mut peak, mut sum, mut sq, mut fall) = (0usize, 0.0f64, 0.0, 0.0, 0.0f64);
    let (mut mean_x, mut above, mut first, mut last, mut cycles) = (m.x1, false, None, 0usize, 0usize);
    let mut prev = m.flow;
    for i in 0..n {
        let u = m.step(f, d, dt);
        let zip = d.zip + d.peel * m.parting();
        let (a1, _) = opening(m.x1 + d.gap, zip, f.length);
        let (a2, _) = opening(m.x2 + d.gap, zip, f.length);
        if a1.min(a2) > 0.0 {
            open += 1;
        }
        peak = peak.max(u);
        sum += u;
        let rate_of_change = (u - prev) * rate;
        prev = u;
        sq += rate_of_change * rate_of_change;
        fall = fall.max(-rate_of_change);
        // cycles: the lower edge passing its own average on the way out
        mean_x += (m.x1 - mean_x) * (40.0 * dt);
        let now = m.x1 > mean_x;
        if now && !above {
            if first.is_none() {
                first = Some(i);
            } else {
                cycles += 1;
            }
            last = i;
        }
        above = now;
    }
    let swing = peak - sum / n as f64;
    let f0 = match first {
        Some(start) if cycles >= 2 && swing > 1e-6 => cycles as f64 * rate / (last - start) as f64,
        _ => 0.0,
    };
    Steady {
        f0,
        open_quotient: open as f64 / n as f64,
        peak_flow: peak,
        mean_flow: sum / n as f64,
        rms_rate: (sq / n as f64).sqrt(),
        max_fall: fall,
    }
}

/// Length of the low-pass that brings the simulation down to the audio rate, and its delay
/// in audio samples.
const TAPS: usize = 24 * OVERSAMPLE + 1;
const FLOW_DELAY: usize = (TAPS - 1) / (2 * OVERSAMPLE);

/// The folds of one voice, ready for the renderer: which tension gives which pitch, and
/// how loud they are.
#[derive(Clone, Debug)]
pub struct Larynx {
    folds: Folds,
    /// The usual lung pressure, rest gap, zip and leak of the voice.
    pub usual: Drive,
    /// (pitch in Hz, tension) for the usual drive, by rising pitch.
    pitch_map: Vec<(f64, f64)>,
    /// Brings the rate of change of the flow to a root mean square of 1 at the usual drive.
    scale: f64,
    /// The usual peak flow, m^3/s.
    peak_flow: f64,
    taps: [f64; TAPS],
    step: f64,
}

/// The folds in motion: carried from one audio sample to the next.
#[derive(Clone, Copy, Debug)]
pub struct Voicing {
    motion: Motion,
    pressure: f64,
    gap: f64,
    /// The last `TAPS` rates of change of the flow, at the simulation rate.
    history: [f64; TAPS],
    at: usize,
    /// The flow of the last few audio samples: it is handed out as late as the low-pass
    /// makes its rate of change.
    flows: [f64; FLOW_DELAY],
    flow_at: usize,
    /// Pitch keeping: the running average of the lower edge, which side of it the edge is
    /// on, steps since it last passed outwards, the pitch asked for over those steps, and
    /// the correction to the tension.
    mean_x: f64,
    outside: bool,
    since: f64,
    asked: f64,
    trim: f64,
    /// The pitch last asked for and the tension that gives it (looked up only when it changes).
    pitch: f64,
    tension: f64,
    /// Counts the cycles (the renderer makes every other one late and weak in creak).
    pub cycle: u32,
}

impl Default for Voicing {
    fn default() -> Self {
        Voicing {
            motion: Motion::default(),
            pressure: 0.0,
            gap: 1e-3,
            history: [0.0; TAPS],
            at: 0,
            flows: [0.0; FLOW_DELAY],
            flow_at: 0,
            mean_x: 0.0,
            outside: false,
            since: 0.0,
            asked: 0.0,
            trim: 1.0,
            pitch: 0.0,
            tension: 1.0,
            cycle: 0,
        }
    }
}

impl Larynx {
    /// Calibrates a voice: `rate` is the audio rate, `pitch` the pitch of the voice in Hz,
    /// `standard` a drive at which these folds are known to work (it bounds how far the
    /// loudness is made up for when the usual drive leaves them barely sounding).
    pub fn new(folds: Folds, usual: Drive, standard: Drive, rate: f64, pitch: f64) -> Larynx {
        let sim_rate = rate * OVERSAMPLE as f64;
        // which tension gives which pitch
        let mut pitch_map: Vec<(f64, f64)> = Vec::new();
        let mut tension = 0.3;
        while tension < 6.0 {
            let s = steady(&folds, &Drive { tension, ..usual }, sim_rate, 0.06, 0.06);
            if s.f0 > 0.0 && pitch_map.last().is_none_or(|&(f, _)| s.f0 > f * 1.01) {
                pitch_map.push((s.f0, tension));
            }
            tension *= 1.18;
        }
        if pitch_map.is_empty() {
            pitch_map.push((pitch, 1.0));
        }
        let mut larynx = Larynx { folds, usual, pitch_map, scale: 1.0, peak_flow: 1e-4, taps: [0.0; TAPS], step: 1.0 / sim_rate };
        let at_pitch = steady(&folds, &Drive { tension: larynx.tension_for(pitch), ..usual }, sim_rate, 0.08, 0.08);
        let reference = steady(&folds, &Drive { tension: pitch / 150.0, ..standard }, sim_rate, 0.08, 0.08);
        larynx.scale = 1.0 / at_pitch.rms_rate.max(0.3 * reference.rms_rate).max(1e-6);
        larynx.peak_flow = at_pitch.peak_flow.max(0.3 * reference.peak_flow).max(1e-9);
        // a windowed-sinc low-pass just under half the audio rate (Kaiser window, -60 dB)
        let cutoff = 0.45 / OVERSAMPLE as f64;
        let bessel = |x: f64| {
            let (mut sum, mut term, mut k) = (1.0, 1.0, 1.0);
            while term > 1e-12 * sum {
                term *= (x / (2.0 * k)) * (x / (2.0 * k));
                sum += term;
                k += 1.0;
            }
            sum
        };
        let (beta, mid) = (5.65, (TAPS - 1) as f64 / 2.0);
        let mut total = 0.0;
        for (i, tap) in larynx.taps.iter_mut().enumerate() {
            let t = i as f64 - mid;
            let sinc = if t == 0.0 { 2.0 * cutoff } else { (std::f64::consts::TAU * cutoff * t).sin() / (std::f64::consts::PI * t) };
            let window = bessel(beta * (1.0 - (t / mid) * (t / mid)).max(0.0).sqrt()) / bessel(beta);
            *tap = sinc * window;
            total += *tap;
        }
        for tap in larynx.taps.iter_mut() {
            *tap /= total;
        }
        larynx
    }

    /// The tension that gives a pitch at the usual drive (between the calibrated points the
    /// pitch is taken to rise as a power of the tension).
    pub fn tension_for(&self, pitch: f64) -> f64 {
        let map = &self.pitch_map;
        if map.len() < 2 {
            return map[0].1;
        }
        let pitch = pitch.clamp(map[0].0, map[map.len() - 1].0);
        let hi = map.iter().position(|&(f, _)| f >= pitch).unwrap_or(map.len() - 1).clamp(1, map.len() - 1);
        let ((f_lo, t_lo), (f_hi, t_hi)) = (map[hi - 1], map[hi]);
        t_lo * (t_hi / t_lo).powf((pitch / f_lo).ln() / (f_hi / f_lo).ln())
    }

    /// The range of pitch the folds can be asked for.
    pub fn pitch_range(&self) -> (f64, f64) {
        (self.pitch_map[0].0, self.pitch_map[self.pitch_map.len() - 1].0)
    }

    /// One audio sample. `pitch` is the pitch asked for, `pressure` and `gap` the drive at
    /// this instant (the folds follow them with a little delay), `skew` a factor on the
    /// tension of this cycle. Returns the excitation (rate of change of the flow, 1 = the
    /// usual loudness), the flow relative to its usual peak, and whether a new cycle began.
    #[inline]
    pub fn tick(&self, v: &mut Voicing, pitch: f64, pressure: f64, gap: f64, skew: f64) -> (f64, f64, bool) {
        let dt = self.step;
        if pitch != v.pitch {
            v.pitch = pitch;
            v.tension = self.tension_for(pitch);
        }
        let tension = v.tension * v.trim * skew;
        let mut began = false;
        for _ in 0..OVERSAMPLE {
            // lungs and the muscles that hold the folds together are slow: 4 and 8 ms
            v.pressure += (pressure - v.pressure) * (dt / 0.004);
            v.gap += (gap - v.gap) * (dt / 0.008);
            let drive = Drive { tension, pressure: v.pressure, gap: v.gap, ..self.usual };
            let before = v.motion.flow;
            let flow = v.motion.step(&self.folds, &drive, dt);
            v.at = if v.at + 1 == TAPS { 0 } else { v.at + 1 };
            v.history[v.at] = (flow - before) / dt;

            // pitch keeping: time each cycle and correct the tension for the next
            v.mean_x += (v.motion.x1 - v.mean_x) * (60.0 * dt);
            v.since += 1.0;
            v.asked += pitch;
            let outside = v.motion.x1 > v.mean_x;
            if outside && !v.outside {
                let period = v.since * dt;
                if period > 0.0008 && period < 0.03 {
                    let error = v.asked / v.since * period;
                    v.trim = (v.trim * error.clamp(0.8, 1.25).powf(0.4)).clamp(0.75, 1.35);
                }
                v.since = 0.0;
                v.asked = 0.0;
                v.cycle = v.cycle.wrapping_add(1);
                began = true;
            } else if v.since * dt > 0.04 {
                // no oscillation: forget the correction
                v.trim = 1.0;
                v.since = 0.0;
                v.asked = 0.0;
            }
            v.outside = outside;
        }
        // band-limit to the audio rate
        let mut rate = 0.0;
        let mut k = v.at;
        for tap in self.taps.iter() {
            rate += tap * v.history[k];
            k = if k == 0 { TAPS - 1 } else { k - 1 };
        }
        let flow = v.flows[v.flow_at];
        v.flows[v.flow_at] = v.motion.flow;
        v.flow_at = if v.flow_at + 1 == FLOW_DELAY { 0 } else { v.flow_at + 1 };
        ((rate * self.scale).clamp(-30.0, 30.0), (flow / self.peak_flow).min(1.5), began)
    }

    /// Whether the folds are at rest: no air is passing and nothing moves.
    pub fn at_rest(&self, v: &Voicing) -> bool {
        v.pressure < 0.05 * self.usual.pressure
    }

    /// Starts the voice: folds at rest are set in motion and run ahead for `seconds`, unheard,
    /// so that the voice does not take several cycles to swell (a speaker starts the breath
    /// before the sound is due).
    pub fn start(&self, v: &mut Voicing, pitch: f64, pressure: f64, gap: f64, seconds: f64) {
        if v.motion.x1.abs() < 1e-6 && v.motion.v1.abs() < 1e-4 {
            v.motion.x1 = 2e-5;
        }
        v.pressure = pressure;
        v.gap = gap;
        for _ in 0..(seconds / (self.step * OVERSAMPLE as f64)) as usize {
            self.tick(v, pitch, pressure, gap, 1.0);
        }
    }
}
