// Phoneme acoustic tables. Started from the SoftN Voice Simulator's values and since retuned
// against recordings of real speakers (see tools/eval.py).
// Adult male 17.5 cm reference tract; scaled at render time by tract length.

use crate::phone::Ph;

/// Acoustic + articulatory target for one steady-state phone.
#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub f1: f64,
    pub f2: f64,
    pub f3: f64,
    pub b1: f64,
    pub b2: f64,
    pub b3: f64,
    pub dur: f64,
    pub av: f64,
    pub ah: f64,
    pub af: f64,
    pub avb: f64,
    pub nasal: f64,
    pub oral_seal: f64,
    pub fnz: f64,
    pub fric_f1: f64,
    pub fric_b1: f64,
    pub fric_f2: f64,
    pub fric_b2: f64,
    pub fric_g1: f64,
    pub fric_g2: f64,
    /// Frication noise is cut off below this frequency (0 = no cut). Real /s/ and /sh/
    /// have almost nothing below their main resonance.
    pub fric_hp: f64,
    /// ... and above this one (0 = no cut): the burst of /k g/ is one compact band with
    /// little above it, where /t d/ hiss to the top.
    pub fric_lp: f64,
    pub stiff: f64,
    pub jaw: f64,
    pub t_h: f64,
    pub t_f: f64,
    pub t_tip: f64,
    pub lip_r: f64,
    pub lip_s: f64,
    pub lip_c: f64,
    pub velum: f64,
}

/// Two-target diphthong (nucleus -> offglide).
#[derive(Clone, Copy, Debug)]
pub struct Diph {
    pub a: Ph,
    pub b: Ph,
    pub dur: f64,
    pub split: f64,
    pub f1a: f64,
    pub f2a: f64,
    pub f1b: f64,
    pub f2b: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Labial,
    Alveolar,
    Velar,
    Other,
}

/// Plosive / affricate sub-segment anatomy (closure, burst, voice-onset time).
#[derive(Clone, Copy, Debug)]
pub struct Stop {
    pub voiced: bool,
    pub close_ms: f64,
    pub burst_ms: f64,
    pub vot_ms: f64,
    pub f1: f64,
    pub f2: f64,
    pub f3: f64,
    pub b_f1: f64,
    pub b_b1: f64,
    pub b_f2: f64,
    pub b_b2: f64,
    pub burst_af: f64,
    pub vot_ah: f64,
    pub jaw: f64,
    pub t_h: f64,
    pub t_f: f64,
    pub t_tip: f64,
    pub lip_r: f64,
    pub lip_c: f64,
    pub place: Place,
}

pub static SPEC_SIL: Spec = Spec { f1: 500.0, f2: 1480.0, f3: 2480.0, b1: 110.0, b2: 130.0, b3: 185.0, dur: 85.0, av: 0.0, ah: 0.0, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2000.0, fric_b1: 1000.0, fric_f2: 4500.0, fric_b2: 1500.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.05, jaw: 0.04, t_h: 0.45, t_f: 0.5, t_tip: 0.15, lip_r: 0.04, lip_s: 0.0, lip_c: 0.16, velum: 0.0 };
pub static SPEC_IY: Spec = Spec { f1: 310.0, f2: 2320.0, f3: 3000.0, b1: 56.0, b2: 92.0, b3: 155.0, dur: 128.0, av: 1.0, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2500.0, fric_b1: 900.0, fric_f2: 5000.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.96, jaw: 0.18, t_h: 0.92, t_f: 0.94, t_tip: 0.25, lip_r: 0.0, lip_s: 0.78, lip_c: 0.0, velum: 0.0 };
pub static SPEC_IH: Spec = Spec { f1: 425.0, f2: 2030.0, f3: 2680.0, b1: 62.0, b2: 96.0, b3: 158.0, dur: 84.0, av: 0.98, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2200.0, fric_b1: 900.0, fric_f2: 4800.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.93, jaw: 0.3, t_h: 0.74, t_f: 0.8, t_tip: 0.22, lip_r: 0.0, lip_s: 0.46, lip_c: 0.0, velum: 0.0 };
pub static SPEC_EH: Spec = Spec { f1: 570.0, f2: 1800.0, f3: 2600.0, b1: 66.0, b2: 100.0, b3: 162.0, dur: 100.0, av: 1.0, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2000.0, fric_b1: 900.0, fric_f2: 4600.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.93, jaw: 0.48, t_h: 0.52, t_f: 0.72, t_tip: 0.2, lip_r: 0.0, lip_s: 0.35, lip_c: 0.0, velum: 0.0 };
pub static SPEC_AE: Spec = Spec { f1: 630.0, f2: 1880.0, f3: 2580.0, b1: 72.0, b2: 106.0, b3: 165.0, dur: 134.0, av: 1.0, ah: 0.014, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1900.0, fric_b1: 900.0, fric_f2: 4500.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.95, jaw: 0.74, t_h: 0.26, t_f: 0.66, t_tip: 0.18, lip_r: 0.0, lip_s: 0.42, lip_c: 0.0, velum: 0.0 };
pub static SPEC_AA: Spec = Spec { f1: 750.0, f2: 1220.0, f3: 2520.0, b1: 74.0, b2: 96.0, b3: 162.0, dur: 136.0, av: 1.0, ah: 0.014, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1400.0, fric_b1: 900.0, fric_f2: 4200.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.95, jaw: 0.86, t_h: 0.14, t_f: 0.18, t_tip: 0.12, lip_r: 0.06, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_AO: Spec = Spec { f1: 620.0, f2: 940.0, f3: 2530.0, b1: 70.0, b2: 88.0, b3: 160.0, dur: 132.0, av: 1.0, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1200.0, fric_b1: 900.0, fric_f2: 4000.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.6, t_h: 0.32, t_f: 0.14, t_tip: 0.12, lip_r: 0.68, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_UH: Spec = Spec { f1: 465.0, f2: 1120.0, f3: 2430.0, b1: 66.0, b2: 92.0, b3: 152.0, dur: 84.0, av: 0.97, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1300.0, fric_b1: 900.0, fric_f2: 3800.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.91, jaw: 0.32, t_h: 0.68, t_f: 0.24, t_tip: 0.14, lip_r: 0.6, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_UW: Spec = Spec { f1: 350.0, f2: 1000.0, f3: 2340.0, b1: 60.0, b2: 88.0, b3: 152.0, dur: 128.0, av: 0.99, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1200.0, fric_b1: 900.0, fric_f2: 3600.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.95, jaw: 0.18, t_h: 0.88, t_f: 0.14, t_tip: 0.12, lip_r: 0.92, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_AH: Spec = Spec { f1: 620.0, f2: 1220.0, f3: 2550.0, b1: 70.0, b2: 96.0, b3: 160.0, dur: 94.0, av: 0.99, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1600.0, fric_b1: 900.0, fric_f2: 4300.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.91, jaw: 0.56, t_h: 0.34, t_f: 0.4, t_tip: 0.16, lip_r: 0.06, lip_s: 0.05, lip_c: 0.0, velum: 0.0 };
pub static SPEC_AX: Spec = Spec { f1: 500.0, f2: 1480.0, f3: 2520.0, b1: 76.0, b2: 106.0, b3: 166.0, dur: 54.0, av: 0.88, ah: 0.015, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1800.0, fric_b1: 900.0, fric_f2: 4400.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.58, jaw: 0.3, t_h: 0.48, t_f: 0.48, t_tip: 0.18, lip_r: 0.08, lip_s: 0.06, lip_c: 0.0, velum: 0.0 };
pub static SPEC_ER: Spec = Spec { f1: 475.0, f2: 1370.0, f3: 1700.0, b1: 66.0, b2: 98.0, b3: 130.0, dur: 126.0, av: 0.99, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1700.0, fric_b1: 900.0, fric_f2: 3600.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.36, t_h: 0.6, t_f: 0.46, t_tip: 0.78, lip_r: 0.38, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_W: Spec = Spec { f1: 295.0, f2: 640.0, f3: 2200.0, b1: 68.0, b2: 92.0, b3: 155.0, dur: 74.0, av: 0.8, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1000.0, fric_b1: 800.0, fric_f2: 3200.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.14, t_h: 0.86, t_f: 0.12, t_tip: 0.12, lip_r: 0.95, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_Y: Spec = Spec { f1: 275.0, f2: 2320.0, f3: 3020.0, b1: 66.0, b2: 95.0, b3: 155.0, dur: 70.0, av: 0.82, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2500.0, fric_b1: 900.0, fric_f2: 5200.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.15, t_h: 0.94, t_f: 0.95, t_tip: 0.28, lip_r: 0.0, lip_s: 0.68, lip_c: 0.0, velum: 0.0 };
pub static SPEC_L: Spec = Spec { f1: 350.0, f2: 1180.0, f3: 2620.0, b1: 70.0, b2: 112.0, b3: 165.0, dur: 72.0, av: 0.78, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1600.0, fric_b1: 900.0, fric_f2: 4000.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.88, jaw: 0.3, t_h: 0.62, t_f: 0.56, t_tip: 0.94, lip_r: 0.05, lip_s: 0.12, lip_c: 0.0, velum: 0.0 };
pub static SPEC_R: Spec = Spec { f1: 405.0, f2: 1180.0, f3: 1580.0, b1: 70.0, b2: 104.0, b3: 132.0, dur: 74.0, av: 0.92, ah: 0.012, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1600.0, fric_b1: 900.0, fric_f2: 3400.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.92, jaw: 0.28, t_h: 0.64, t_f: 0.44, t_tip: 0.82, lip_r: 0.44, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_M: Spec = Spec { f1: 480.0, f2: 1180.0, f3: 2130.0, b1: 60.0, b2: 240.0, b3: 220.0, dur: 82.0, av: 0.82, ah: 0.0, af: 0.0, avb: 0.0, nasal: 1.0, oral_seal: 1.0, fnz: 450.0, fric_f1: 1000.0, fric_b1: 800.0, fric_f2: 3000.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.08, t_h: 0.46, t_f: 0.45, t_tip: 0.2, lip_r: 0.08, lip_s: 0.0, lip_c: 1.0, velum: 0.94 };
pub static SPEC_N: Spec = Spec { f1: 480.0, f2: 1500.0, f3: 2500.0, b1: 60.0, b2: 280.0, b3: 300.0, dur: 74.0, av: 0.82, ah: 0.0, af: 0.0, avb: 0.0, nasal: 1.0, oral_seal: 1.0, fnz: 450.0, fric_f1: 1700.0, fric_b1: 900.0, fric_f2: 4000.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.18, t_h: 0.68, t_f: 0.7, t_tip: 0.96, lip_r: 0.0, lip_s: 0.12, lip_c: 0.0, velum: 0.94 };
pub static SPEC_NG: Spec = Spec { f1: 480.0, f2: 1900.0, f3: 2500.0, b1: 60.0, b2: 250.0, b3: 300.0, dur: 80.0, av: 0.82, ah: 0.0, af: 0.0, avb: 0.0, nasal: 1.0, oral_seal: 1.0, fnz: 450.0, fric_f1: 2000.0, fric_b1: 900.0, fric_f2: 4200.0, fric_b2: 1400.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.94, jaw: 0.24, t_h: 0.92, t_f: 0.3, t_tip: 0.18, lip_r: 0.0, lip_s: 0.1, lip_c: 0.0, velum: 0.95 };
pub static SPEC_HH: Spec = Spec { f1: 500.0, f2: 1500.0, f3: 2500.0, b1: 340.0, b2: 210.0, b3: 250.0, dur: 62.0, av: 0.32, ah: 0.22, af: 0.0, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 1800.0, fric_b1: 1200.0, fric_f2: 4200.0, fric_b2: 1600.0, fric_g1: 0.0, fric_g2: 0.0, fric_hp: 0.0, fric_lp: 0.0, stiff: 0.02, jaw: 0.4, t_h: 0.5, t_f: 0.5, t_tip: 0.2, lip_r: 0.05, lip_s: 0.1, lip_c: 0.0, velum: 0.0 };
pub static SPEC_F: Spec = Spec { f1: 330.0, f2: 980.0, f3: 2260.0, b1: 135.0, b2: 150.0, b3: 210.0, dur: 88.0, av: 0.0, ah: 0.04, af: 0.06, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 3500.0, fric_b1: 4000.0, fric_f2: 7500.0, fric_b2: 3500.0, fric_g1: 0.5, fric_g2: 0.6, fric_hp: 900.0, fric_lp: 0.0, stiff: 0.72, jaw: 0.12, t_h: 0.45, t_f: 0.45, t_tip: 0.2, lip_r: 0.08, lip_s: 0.1, lip_c: 0.68, velum: 0.0 };
pub static SPEC_V: Spec = Spec { f1: 300.0, f2: 980.0, f3: 2260.0, b1: 88.0, b2: 125.0, b3: 185.0, dur: 64.0, av: 0.45, ah: 0.02, af: 0.03, avb: 0.34, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 3500.0, fric_b1: 4000.0, fric_f2: 7400.0, fric_b2: 3500.0, fric_g1: 0.5, fric_g2: 0.6, fric_hp: 900.0, fric_lp: 0.0, stiff: 0.72, jaw: 0.12, t_h: 0.45, t_f: 0.45, t_tip: 0.2, lip_r: 0.08, lip_s: 0.1, lip_c: 0.68, velum: 0.0 };
pub static SPEC_TH: Spec = Spec { f1: 350.0, f2: 1520.0, f3: 2480.0, b1: 135.0, b2: 150.0, b3: 210.0, dur: 86.0, av: 0.0, ah: 0.03, af: 0.04, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 3500.0, fric_b1: 4000.0, fric_f2: 7000.0, fric_b2: 3500.0, fric_g1: 0.4, fric_g2: 0.5, fric_hp: 900.0, fric_lp: 0.0, stiff: 0.72, jaw: 0.16, t_h: 0.58, t_f: 0.72, t_tip: 0.88, lip_r: 0.0, lip_s: 0.18, lip_c: 0.0, velum: 0.0 };
pub static SPEC_DH: Spec = Spec { f1: 320.0, f2: 1600.0, f3: 2500.0, b1: 88.0, b2: 125.0, b3: 185.0, dur: 54.0, av: 0.45, ah: 0.02, af: 0.015, avb: 0.34, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2350.0, fric_b1: 1900.0, fric_f2: 4500.0, fric_b2: 3200.0, fric_g1: 0.3, fric_g2: 0.3, fric_hp: 900.0, fric_lp: 0.0, stiff: 0.7, jaw: 0.16, t_h: 0.58, t_f: 0.72, t_tip: 0.88, lip_r: 0.0, lip_s: 0.18, lip_c: 0.0, velum: 0.0 };
pub static SPEC_S: Spec = Spec { f1: 320.0, f2: 1760.0, f3: 2560.0, b1: 130.0, b2: 145.0, b3: 205.0, dur: 94.0, av: 0.0, ah: 0.02, af: 0.30, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 5500.0, fric_b1: 980.0, fric_f2: 7500.0, fric_b2: 1550.0, fric_g1: 0.64, fric_g2: 0.46, fric_hp: 3300.0, fric_lp: 0.0, stiff: 0.76, jaw: 0.12, t_h: 0.74, t_f: 0.78, t_tip: 0.9, lip_r: 0.0, lip_s: 0.36, lip_c: 0.0, velum: 0.0 };
pub static SPEC_Z: Spec = Spec { f1: 300.0, f2: 1740.0, f3: 2540.0, b1: 85.0, b2: 120.0, b3: 180.0, dur: 70.0, av: 0.5, ah: 0.02, af: 0.15, avb: 0.26, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 5400.0, fric_b1: 980.0, fric_f2: 7300.0, fric_b2: 1550.0, fric_g1: 0.64, fric_g2: 0.46, fric_hp: 3300.0, fric_lp: 0.0, stiff: 0.76, jaw: 0.12, t_h: 0.74, t_f: 0.78, t_tip: 0.9, lip_r: 0.0, lip_s: 0.36, lip_c: 0.0, velum: 0.0 };
pub static SPEC_SH: Spec = Spec { f1: 330.0, f2: 1960.0, f3: 2620.0, b1: 130.0, b2: 145.0, b3: 205.0, dur: 94.0, av: 0.0, ah: 0.02, af: 0.36, avb: 0.0, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2900.0, fric_b1: 800.0, fric_f2: 5200.0, fric_b2: 2200.0, fric_g1: 0.68, fric_g2: 0.5, fric_hp: 1900.0, fric_lp: 0.0, stiff: 0.78, jaw: 0.16, t_h: 0.78, t_f: 0.82, t_tip: 0.74, lip_r: 0.58, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };
pub static SPEC_ZH: Spec = Spec { f1: 310.0, f2: 1940.0, f3: 2600.0, b1: 85.0, b2: 120.0, b3: 180.0, dur: 70.0, av: 0.5, ah: 0.02, af: 0.18, avb: 0.26, nasal: 0.0, oral_seal: 0.0, fnz: 500.0, fric_f1: 2850.0, fric_b1: 800.0, fric_f2: 5100.0, fric_b2: 2200.0, fric_g1: 0.68, fric_g2: 0.5, fric_hp: 1900.0, fric_lp: 0.0, stiff: 0.78, jaw: 0.16, t_h: 0.78, t_f: 0.82, t_tip: 0.74, lip_r: 0.58, lip_s: 0.0, lip_c: 0.0, velum: 0.0 };

/// Steady-state spec for a phone, if it has one (diphthongs and stops are built from parts).
pub fn spec(ph: Ph) -> Option<&'static Spec> {
    match ph {
        Ph::SIL => Some(&SPEC_SIL),
        Ph::IY => Some(&SPEC_IY),
        Ph::IH => Some(&SPEC_IH),
        Ph::EH => Some(&SPEC_EH),
        Ph::AE => Some(&SPEC_AE),
        Ph::AA => Some(&SPEC_AA),
        Ph::AO => Some(&SPEC_AO),
        Ph::UH => Some(&SPEC_UH),
        Ph::UW => Some(&SPEC_UW),
        Ph::AH => Some(&SPEC_AH),
        Ph::AX => Some(&SPEC_AX),
        Ph::ER => Some(&SPEC_ER),
        Ph::W => Some(&SPEC_W),
        Ph::Y => Some(&SPEC_Y),
        Ph::L => Some(&SPEC_L),
        Ph::R => Some(&SPEC_R),
        Ph::M => Some(&SPEC_M),
        Ph::N => Some(&SPEC_N),
        Ph::NG => Some(&SPEC_NG),
        Ph::HH => Some(&SPEC_HH),
        Ph::F => Some(&SPEC_F),
        Ph::V => Some(&SPEC_V),
        Ph::TH => Some(&SPEC_TH),
        Ph::DH => Some(&SPEC_DH),
        Ph::S => Some(&SPEC_S),
        Ph::Z => Some(&SPEC_Z),
        Ph::SH => Some(&SPEC_SH),
        Ph::ZH => Some(&SPEC_ZH),
        _ => None,
    }
}

pub static DIPH_EY: Diph = Diph { a: Ph::EH, b: Ph::IY, dur: 150.0, split: 0.54, f1a: 480.0, f2a: 2050.0, f1b: 360.0, f2b: 2260.0};
pub static DIPH_AY: Diph = Diph { a: Ph::AA, b: Ph::IH, dur: 168.0, split: 0.58, f1a: 740.0, f2a: 1260.0, f1b: 440.0, f2b: 1950.0};
pub static DIPH_OY: Diph = Diph { a: Ph::AO, b: Ph::IH, dur: 172.0, split: 0.58, f1a: 560.0, f2a: 880.0, f1b: 420.0, f2b: 1900.0};
pub static DIPH_AW: Diph = Diph { a: Ph::AA, b: Ph::UH, dur: 170.0, split: 0.6, f1a: 740.0, f2a: 1350.0, f1b: 470.0, f2b: 1020.0};
pub static DIPH_OW: Diph = Diph { a: Ph::AO, b: Ph::UW, dur: 152.0, split: 0.56, f1a: 500.0, f2a: 1010.0, f1b: 390.0, f2b: 880.0};

pub fn diph(ph: Ph) -> Option<&'static Diph> {
    match ph {
        Ph::EY => Some(&DIPH_EY),
        Ph::AY => Some(&DIPH_AY),
        Ph::OY => Some(&DIPH_OY),
        Ph::AW => Some(&DIPH_AW),
        Ph::OW => Some(&DIPH_OW),
        _ => None,
    }
}

pub static STOP_P: Stop = Stop { voiced: false, close_ms: 48.0, burst_ms: 12.0, vot_ms: 36.0, f1: 245.0, f2: 740.0, f3: 2120.0, b_f1: 500.0, b_b1: 500.0, b_f2: 2400.0, b_b2: 3000.0, burst_af: 0.10, vot_ah: 0.14, jaw: 0.06, t_h: 0.45, t_f: 0.45, t_tip: 0.18, lip_r: 0.05, lip_c: 1.0, place: Place::Labial };
pub static STOP_B: Stop = Stop { voiced: true, close_ms: 42.0, burst_ms: 10.0, vot_ms: 9.0, f1: 235.0, f2: 740.0, f3: 2120.0, b_f1: 450.0, b_b1: 500.0, b_f2: 2300.0, b_b2: 3000.0, burst_af: 0.05, vot_ah: 0.04, jaw: 0.06, t_h: 0.45, t_f: 0.45, t_tip: 0.18, lip_r: 0.05, lip_c: 1.0, place: Place::Labial };
pub static STOP_T: Stop = Stop { voiced: false, close_ms: 46.0, burst_ms: 13.0, vot_ms: 40.0, f1: 290.0, f2: 1790.0, f3: 2620.0, b_f1: 4300.0, b_b1: 1600.0, b_f2: 6600.0, b_b2: 2400.0, burst_af: 0.15, vot_ah: 0.30, jaw: 0.14, t_h: 0.72, t_f: 0.76, t_tip: 0.98, lip_r: 0.0, lip_c: 0.0, place: Place::Alveolar };
pub static STOP_D: Stop = Stop { voiced: true, close_ms: 38.0, burst_ms: 9.0, vot_ms: 8.0, f1: 275.0, f2: 1790.0, f3: 2600.0, b_f1: 4000.0, b_b1: 1600.0, b_f2: 6200.0, b_b2: 2400.0, burst_af: 0.08, vot_ah: 0.04, jaw: 0.14, t_h: 0.72, t_f: 0.76, t_tip: 0.98, lip_r: 0.0, lip_c: 0.0, place: Place::Alveolar };
pub static STOP_K: Stop = Stop { voiced: false, close_ms: 48.0, burst_ms: 16.0, vot_ms: 42.0, f1: 295.0, f2: 1850.0, f3: 2240.0, b_f1: 1950.0, b_b1: 480.0, b_f2: 5200.0, b_b2: 2600.0, burst_af: 0.36, vot_ah: 0.24, jaw: 0.22, t_h: 0.95, t_f: 0.36, t_tip: 0.16, lip_r: 0.04, lip_c: 0.0, place: Place::Velar };
pub static STOP_G: Stop = Stop { voiced: true, close_ms: 38.0, burst_ms: 10.0, vot_ms: 9.0, f1: 280.0, f2: 1850.0, f3: 2240.0, b_f1: 1900.0, b_b1: 480.0, b_f2: 5000.0, b_b2: 2600.0, burst_af: 0.2, vot_ah: 0.05, jaw: 0.22, t_h: 0.95, t_f: 0.36, t_tip: 0.16, lip_r: 0.04, lip_c: 0.0, place: Place::Velar };
pub static STOP_CH: Stop = Stop { voiced: false, close_ms: 42.0, burst_ms: 54.0, vot_ms: 14.0, f1: 310.0, f2: 1960.0, f3: 2620.0, b_f1: 2900.0, b_b1: 800.0, b_f2: 5000.0, b_b2: 2200.0, burst_af: 0.30, vot_ah: 0.18, jaw: 0.16, t_h: 0.8, t_f: 0.82, t_tip: 0.86, lip_r: 0.52, lip_c: 0.0, place: Place::Other };
pub static STOP_JH: Stop = Stop { voiced: true, close_ms: 36.0, burst_ms: 38.0, vot_ms: 8.0, f1: 295.0, f2: 1940.0, f3: 2600.0, b_f1: 2800.0, b_b1: 800.0, b_f2: 4800.0, b_b2: 2200.0, burst_af: 0.15, vot_ah: 0.04, jaw: 0.16, t_h: 0.8, t_f: 0.82, t_tip: 0.86, lip_r: 0.52, lip_c: 0.0, place: Place::Other };

pub fn stop(ph: Ph) -> Option<&'static Stop> {
    match ph {
        Ph::P => Some(&STOP_P),
        Ph::B => Some(&STOP_B),
        Ph::T => Some(&STOP_T),
        Ph::D => Some(&STOP_D),
        Ph::K => Some(&STOP_K),
        Ph::G => Some(&STOP_G),
        Ph::CH => Some(&STOP_CH),
        Ph::JH => Some(&STOP_JH),
        _ => None,
    }
}
