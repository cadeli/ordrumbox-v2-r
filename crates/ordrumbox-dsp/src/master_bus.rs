//! The master bus: pre-gain, a
//! peak compressor with soft knee and make-up gain, a one-pole low cut and
//! high cut, the master gain, then a soft limit to ±1:
//! untouched under 0.9, rounded above.

use std::f64::consts::{LN_10, PI};

use crate::denormal::FLUSH;

/// Decibels to a natural-log ratio.
const LOG10_OVER_20: f64 = LN_10 / 20.0;
/// Level from which the output limit bends toward ±1.
const LIMIT_KNEE: f64 = 0.9;

/// `x` unchanged within ±[`LIMIT_KNEE`], then bent smoothly (same slope at
/// the knee) toward ±1, never reached.
#[inline]
fn soft_limit(x: f64) -> f64 {
    let level = x.abs();
    if level <= LIMIT_KNEE {
        return x;
    }
    let room = 1.0 - LIMIT_KNEE;
    (LIMIT_KNEE + room * ((level - LIMIT_KNEE) / room).tanh()).copysign(x)
}

audio_params! {
    /// The master bus parameters; the defaults are `MASTER_BUS_DEFAULTS`, which the app applies at start.
    pub struct MasterSettings {
        /// dB.
        threshold = -18.0, -60.0, 0.0;
        ratio = 8.0, 1.0, 20.0;
        /// dB.
        knee = 3.0, 0.0, 40.0;
        /// Seconds.
        attack = 0.002, 0.001, 1.0;
        /// Seconds.
        release = 0.08, 0.01, 1.0;
        /// dB.
        makeup = 8.0, 0.0, 24.0;
        /// Hz.
        lowcut = 35.0, 10.0, 500.0;
        /// Hz.
        hicut = 18_500.0, 1_000.0, 22_000.0;
        /// Linear.
        master = 1.0, 0.0, 2.0;
        /// 1 skips pre-gain, compressor and filters (not the master gain).
        bypass = 0.0, 0.0, 1.0;
        /// dB.
        pre_gain = 0.0, -20.0, 20.0;
    }
}

#[derive(Debug, Clone, Default)]
pub struct MasterBus {
    sample_rate: f64,
    env_db: f64,
    hp_y_l: f64,
    hp_x_prev_l: f64,
    hp_y_r: f64,
    hp_x_prev_r: f64,
    lp_y_l: f64,
    lp_y_r: f64,
}

/// The compressor's gain reduction in dB (positive) for a level `input_db`:
/// the curve the bus applies and the output panel draws.
#[must_use]
#[inline]
pub fn gain_reduction(input_db: f64, threshold: f64, ratio: f64, knee: f64) -> f64 {
    let over = input_db - threshold;
    if over <= -knee / 2.0 {
        return 0.0;
    }
    if knee > 0.0 && over < knee / 2.0 {
        let d = over + knee / 2.0;
        return (1.0 - 1.0 / ratio) * d * d / (2.0 * knee);
    }
    over * (1.0 - 1.0 / ratio)
}

impl MasterBus {
    pub fn new(sample_rate: f32) -> Self {
        Self { sample_rate: f64::from(sample_rate), ..Self::default() }
    }

    /// Process the stereo bus in place.
    pub fn process(&mut self, settings: &MasterSettings, left: &mut [f32], right: &mut [f32]) {
        let s = settings.clamped();
        let master = f64::from(s.master);
        let frames = left.iter_mut().zip(right.iter_mut());
        // Bypassed or not, decided once for the block: one loop per case.
        if s.bypass < 0.5 {
            let chain = Chain::new(&s, self.sample_rate);
            for (l, r) in frames {
                let (x_l, x_r) = self.chain(&chain, f64::from(*l), f64::from(*r));
                self.output(x_l, x_r, master, l, r);
            }
        } else {
            for (l, r) in frames {
                self.output(f64::from(*l), f64::from(*r), master, l, r);
            }
        }
    }

    /// Pre-gain, compressor, low cut and high cut on one frame.
    #[inline]
    fn chain(&mut self, c: &Chain, mut x_l: f64, mut x_r: f64) -> (f64, f64) {
        x_l *= c.pre_gain;
        x_r *= c.pre_gain;

        let peak = if x_l.abs() > x_r.abs() { x_l.abs() } else { x_r.abs() };
        let peak_db = if peak > 1e-10 { 20.0 * (peak.ln() / LN_10) } else { -100.0 };
        let target = -gain_reduction(peak_db, c.threshold, c.ratio, c.knee);
        let coeff = if target < self.env_db { c.att_coeff } else { c.rel_coeff };
        self.env_db = coeff * self.env_db + (1.0 - coeff) * target;
        let comp = (self.env_db * LOG10_OVER_20).exp();
        x_l *= comp * c.makeup;
        x_r *= comp * c.makeup;

        self.hp_y_l = c.hp_a * (self.hp_y_l + x_l - self.hp_x_prev_l);
        self.hp_x_prev_l = x_l;
        x_l = self.hp_y_l;
        self.hp_y_r = c.hp_a * (self.hp_y_r + x_r - self.hp_x_prev_r);
        self.hp_x_prev_r = x_r;
        x_r = self.hp_y_r;

        self.lp_y_l = c.lp_a * x_l + (1.0 - c.lp_a) * self.lp_y_l;
        self.lp_y_r = c.lp_a * x_r + (1.0 - c.lp_a) * self.lp_y_r;
        (self.lp_y_l, self.lp_y_r)
    }

    /// The end of every frame, bypassed or not: denormals flushed, the
    /// master gain, the soft limit.
    #[inline]
    fn output(&mut self, mut x_l: f64, mut x_r: f64, master: f64, l: &mut f32, r: &mut f32) {
        if x_l.abs() < FLUSH {
            x_l = 0.0;
            self.hp_y_l = 0.0;
            self.lp_y_l = 0.0;
        }
        if x_r.abs() < FLUSH {
            x_r = 0.0;
            self.hp_y_r = 0.0;
            self.lp_y_r = 0.0;
        }
        *l = soft_limit(x_l * master) as f32;
        *r = soft_limit(x_r * master) as f32;
    }
}

/// What the bus works out once per block for its chain (all but the master
/// gain and the limit), so that the sample loop only reads it.
#[derive(Debug, Clone, Copy)]
struct Chain {
    threshold: f64,
    ratio: f64,
    knee: f64,
    makeup: f64,
    pre_gain: f64,
    hp_a: f64,
    lp_a: f64,
    att_coeff: f64,
    rel_coeff: f64,
}

impl Chain {
    fn new(s: &MasterSettings, sr: f64) -> Self {
        let attack = f64::from(s.attack).max(0.0001);
        let release = f64::from(s.release).max(0.001);
        let lowcut = f64::from(s.lowcut).min(sr * 0.45).max(0.1);
        let hp_rc = 1.0 / (2.0 * PI * lowcut);
        let hicut = f64::from(s.hicut).min(sr * 0.45).max(0.1);
        let lp_rc = 1.0 / (2.0 * PI * hicut);
        Self {
            threshold: f64::from(s.threshold),
            ratio: f64::from(s.ratio).max(1.0),
            knee: f64::from(s.knee).max(0.0),
            makeup: (f64::from(s.makeup) * LOG10_OVER_20).exp(),
            pre_gain: (f64::from(s.pre_gain) * LOG10_OVER_20).exp(),
            hp_a: hp_rc / (hp_rc + 1.0 / sr),
            lp_a: (1.0 / sr) / (lp_rc + 1.0 / sr),
            att_coeff: (-1.0 / (attack * sr)).exp(),
            rel_coeff: (-1.0 / (release * sr)).exp(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knee_curve() {
        assert_eq!(gain_reduction(-30.0, -18.0, 8.0, 3.0), 0.0);
        assert!((gain_reduction(-6.0, -18.0, 8.0, 3.0) - 12.0 * 7.0 / 8.0).abs() < 1e-12);
        // Middle of the knee: (1 - 1/R) * (W/2)^2 / (2W).
        assert!((gain_reduction(-18.0, -18.0, 2.0, 4.0) - 0.5 * 4.0 / 8.0).abs() < 1e-12);
    }

    #[test]
    fn loud_input_is_held_under_one() {
        let mut bus = MasterBus::new(48_000.0);
        let mut l: Vec<f32> = (0..4800).map(|i| if i % 2 == 0 { 3.0 } else { -3.0 }).collect();
        let mut r = l.clone();
        bus.process(&MasterSettings::default(), &mut l, &mut r);
        assert!(l.iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn the_limit_bends_only_above_its_knee() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.9), -0.9);
        assert!((0.99..1.0).contains(&soft_limit(1.5)));
        assert!((-1.0..-0.99).contains(&soft_limit(-3.0)));
        // Continuous slope at the knee: no edge.
        let slope = (soft_limit(0.9 + 1e-6) - 0.9) / 1e-6;
        assert!((slope - 1.0).abs() < 1e-3);
    }

    #[test]
    fn bypass_keeps_only_master_gain() {
        let mut bus = MasterBus::new(48_000.0);
        let (mut l, mut r) = (vec![0.4; 8], vec![-0.4; 8]);
        bus.process(&MasterSettings { bypass: 1.0, master: 0.5, ..MasterSettings::default() }, &mut l, &mut r);
        assert_eq!((l[7], r[7]), (0.2, -0.2));
    }
}
