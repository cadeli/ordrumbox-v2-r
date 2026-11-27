//! The filter curve of the synth editor: the response of the filter, and of
//! the second filter with it (after it, or mixed with it in parallel), as
//! the LFOs move them now.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::num::gain_to_db;
use ordrumbox_dsp::synth::balance_gains;

use super::{lfo_mod_on, number, playing_lfos, routes_of, slope, text};
use crate::synth_preset::{
    FILTER2_BALANCE_DEFAULT, FILTER2_FREQ_DEFAULT, FILTER2_Q_DEFAULT, FILTER2_ROUTINGS, FILTER2_TYPE_DEFAULT,
};

/// The lowest and highest gain the curve shows, dB.
const FLOOR_DB: f64 = -30.0;
const CEILING_DB: f64 = 12.0;
/// The resonance range of the curve.
const Q_RANGE: (f64, f64) = (0.1, 24.0);

/// A complex gain: a filter's response at one frequency, its phase included,
/// so that two filters in parallel add up as they sound.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Gain {
    re: f64,
    im: f64,
}

impl Gain {
    const ONE: Self = Self { re: 1.0, im: 0.0 };

    fn times(self, other: Self) -> Self {
        Self { re: self.re * other.re - self.im * other.im, im: self.re * other.im + self.im * other.re }
    }

    fn over(self, other: Self) -> Self {
        let norm = (other.re * other.re + other.im * other.im).max(f64::MIN_POSITIVE);
        Self {
            re: (self.re * other.re + self.im * other.im) / norm,
            im: (self.im * other.re - self.re * other.im) / norm,
        }
    }

    fn scaled(self, k: f64) -> Self {
        Self { re: self.re * k, im: self.im * k }
    }

    fn plus(self, other: Self) -> Self {
        Self { re: self.re + other.re, im: self.im + other.im }
    }

    fn magnitude(self) -> f64 {
        self.re.hypot(self.im)
    }
}

/// One filter as the curve draws it: its mode, cutoff, damping (1/Q) and
/// whether it is two stages (24 dB/oct).
#[derive(Debug, Clone, Copy)]
struct Stage {
    mode: slope::FilterMode,
    fc: f64,
    k: f64,
    is_steep: bool,
}

impl Stage {
    fn new(kind: &str, fc: f64, q: f64, slope_db: f64) -> Self {
        let mode = match kind {
            "highpass" => slope::FilterMode::Highpass,
            "bandpass" => slope::FilterMode::Bandpass,
            "notch" => slope::FilterMode::Notch,
            _ => slope::FilterMode::Lowpass,
        };
        Self { mode, fc, k: 1.0 / q.clamp(Q_RANGE.0, Q_RANGE.1), is_steep: slope::is_steep(slope_db) }
    }

    /// Its gain at `f` Hz, as the synth plays it: a state-variable stage (a
    /// bandpass peaks at its Q), or the product of its two and the second's gain.
    fn at(self, f: f64) -> Gain {
        let w = f / self.fc;
        let one = |k: f64| {
            let numerator = match self.mode {
                slope::FilterMode::Lowpass => Gain::ONE,
                slope::FilterMode::Highpass => Gain { re: -w * w, im: 0.0 },
                slope::FilterMode::Bandpass => Gain { re: 0.0, im: w },
                slope::FilterMode::Notch => Gain { re: 1.0 - w * w, im: 0.0 },
            };
            numerator.over(Gain { re: 1.0 - w * w, im: k * w })
        };
        if !self.is_steep {
            return one(self.k);
        }
        let (k1, k2, gain_2) = slope::stages(self.k, self.mode);
        one(k1).times(one(k2)).scaled(gain_2)
    }
}

/// The second filter as it sounds now: itself, and where it goes.
#[derive(Debug, Clone, Copy)]
enum Second {
    /// After the first filter.
    Serial(Stage),
    /// Beside it, the two mixed at these gains (first, second).
    Parallel(Stage, [f64; 2]),
}

/// The second filter of `draft` with the LFOs at `time`; `None` when it is off.
fn second_filter(draft: &Object, (time, bpm): (f64, f64)) -> Option<Second> {
    draft.get("filter2")?.as_object()?;
    if draft.get("bypassFilter2").is_some_and(Value::is_true) {
        return None;
    }
    let range = (ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ);
    let fc =
        number(draft, "filter2.freq").unwrap_or(FILTER2_FREQ_DEFAULT) + lfo_mod_on(draft, "filter2.freq", time, bpm);
    let q = number(draft, "filter2.Q").unwrap_or(FILTER2_Q_DEFAULT);
    let kind = text(draft, "filter2.type").unwrap_or(FILTER2_TYPE_DEFAULT);
    let stage = Stage::new(kind, fc.clamp(range.0, range.1), q, 12.0);
    if text(draft, "filter2.routing") != Some(FILTER2_ROUTINGS[1]) {
        return Some(Second::Serial(stage));
    }
    let balance = number(draft, "filter2.balance").unwrap_or(FILTER2_BALANCE_DEFAULT)
        + lfo_mod_on(draft, "filter2.balance", time, bpm);
    Some(Second::Parallel(stage, balance_gains(balance)))
}

/// The response of the filters of `draft`, dB at `points + 1` frequencies
/// from 20 Hz to 20 kHz (log): the filter (`kind`, Q `q`, slope `slope_db`,
/// flat when bypassed) at cutoff `fc`, with the second filter after it or
/// mixed with it, as the LFOs move it at `time`.
#[must_use]
pub fn filters_response(
    draft: &Object,
    (kind, q, slope_db): (&str, f64, f64),
    fc: f64,
    (time, bpm): (f64, f64),
    points: usize,
) -> Vec<f64> {
    let is_bypassed = draft.get("bypassFilter").is_some_and(Value::is_true);
    let first = Stage::new(kind, fc, q, slope_db);
    let second = second_filter(draft, (time, bpm));
    (0..=points)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / points as f64);
            let one = if is_bypassed { Gain::ONE } else { first.at(f) };
            let gain = match second {
                None => one,
                Some(Second::Serial(stage)) => one.times(stage.at(f)),
                Some(Second::Parallel(stage, [g1, g2])) => one.scaled(g1).plus(stage.at(f).scaled(g2)),
            };
            gain_to_db(gain.magnitude().max(1e-10)).clamp(FLOOR_DB, CEILING_DB)
        })
        .collect()
}

/// The filter curve's type, centre, Q with the LFOs at `time` and slope, and
/// the band an LFO on the frequency sweeps.
pub fn filter_now(draft: &Object, time: f64, bpm: f64) -> (String, f64, f64, f64, Option<(f64, f64)>) {
    let kind = text(draft, "filter.type").unwrap_or("lowpass").to_owned();
    let slope_db = number(draft, "filter.slope").unwrap_or(12.0);
    let fc = number(draft, "filter.freq")
        .unwrap_or(400.0)
        .clamp(ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ);
    let q = number(draft, "filter.Q").unwrap_or(1.0).clamp(Q_RANGE.0, Q_RANGE.1);
    let fc = (fc + lfo_mod_on(draft, "filter.freq", time, bpm))
        .clamp(ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ);
    let q = (q + lfo_mod_on(draft, "filter.Q", time, bpm)).clamp(Q_RANGE.0, Q_RANGE.1);
    let depth = playing_lfos(draft)
        .flat_map(|(_, source)| routes_of(draft, source))
        .filter(|(target, _)| *target == "filter.freq")
        .map(|(_, amount)| amount.abs())
        .fold(0.0f64, f64::max);
    let band = (depth > 0.0).then(|| {
        (
            (fc - depth * 1000.0).max(ordrumbox_dsp::MIN_FILTER_FREQ),
            (fc + depth * 1000.0).min(ordrumbox_dsp::MAX_FILTER_FREQ),
        )
    });
    (kind, fc, q, slope_db, band)
}

#[cfg(test)]
mod tests {
    use super::super::{defaults, set};
    use super::*;

    /// The point of frequency `f`: 200 points span 3 decades from 20 Hz.
    fn at(f: f64) -> usize {
        (200.0 * (f / 20.0f64).log10() / 3.0).round() as usize
    }

    fn response(draft: &Object, kind: &str, q: f64, slope_db: f64) -> Vec<f64> {
        filters_response(draft, (kind, q, slope_db), 1000.0, (0.0, 120.0), 200)
    }

    #[test]
    fn one_filter_is_its_own_curve() {
        let d = defaults();
        let low = response(&d, "lowpass", 1.0, 12.0);
        assert!(low[0] > -1.0 && low[200] == FLOOR_DB);
        let high = response(&d, "highpass", 1.0, 12.0);
        assert!(high[0] == FLOOR_DB && high[200] > -1.0);
        let steep = response(&d, "lowpass", std::f64::consts::FRAC_1_SQRT_2, 24.0);
        assert!((steep[at(1000.0)] + 3.0).abs() < 0.5 && steep[at(2000.0)] < -23.0, "{}", steep[at(2000.0)]);
        let band = response(&d, "bandpass", 4.0, 12.0);
        assert!((band[at(1000.0)] - gain_to_db(4.0)).abs() < 0.5, "a bandpass peaks at its Q: {}", band[at(1000.0)]);
        let notch = response(&d, "notch", 1.0, 12.0);
        assert!(notch[0] > -1.0 && notch[200] > -1.0 && notch[at(1000.0)] < -20.0, "a notch, not a lowpass");
    }

    #[test]
    fn the_second_filter_follows_the_first_or_is_mixed_with_it() {
        let mut d = defaults();
        set(&mut d, "filter2.type", Value::from("highpass"));
        set(&mut d, "filter2.freq", Value::from(5000.0));
        // In series, a lowpass at 1 kHz then a highpass at 5 kHz: nothing gets through.
        let serial = response(&d, "lowpass", 0.7, 12.0);
        assert!(serial[at(100.0)] < -20.0 && serial[at(10_000.0)] < -10.0, "{serial:?}");
        // In parallel, both ends pass, the middle dips.
        set(&mut d, "filter2.routing", Value::from(FILTER2_ROUTINGS[1]));
        let parallel = response(&d, "lowpass", 0.7, 12.0);
        assert!(parallel[at(100.0)] > -1.0 && parallel[at(15_000.0)] > -2.0, "{parallel:?}");
        assert!(parallel[at(2500.0)] < parallel[at(100.0)] - 3.0, "{parallel:?}");
        // The balance at an end: one filter alone.
        set(&mut d, "filter2.balance", Value::from(0.0));
        assert_eq!(response(&d, "lowpass", 0.7, 12.0), response(&defaults(), "lowpass", 0.7, 12.0));
        // Bypassed, the second filter is gone.
        set(&mut d, "filter2.balance", Value::from(1.0));
        d.insert("bypassFilter2", Value::Bool(true));
        assert_eq!(response(&d, "lowpass", 0.7, 12.0), response(&defaults(), "lowpass", 0.7, 12.0));
    }
}
