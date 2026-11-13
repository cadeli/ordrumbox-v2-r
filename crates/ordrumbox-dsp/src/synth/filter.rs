//! The TPT state-variable filter of a synth voice: the main filter's stages
//! and the noise filter.

use std::f64::consts::PI;

use super::clamp;
use crate::slope::FilterMode;

/// A state below this is flushed to zero, against denormals.
const FLUSH: f64 = 1e-15;
/// Highest filter cutoff, as a fraction of the sample rate.
pub(super) const MAX_CUTOFF_RATIO: f64 = 0.45;

/// How much of the lowpass, highpass and bandpass outputs `mode` lets
/// through: chosen once per block, so no sample has to ask for the mode.
pub(super) const fn tap_weights(mode: FilterMode) -> [f64; 3] {
    match mode {
        FilterMode::Lowpass => [1.0, 0.0, 0.0],
        FilterMode::Highpass => [0.0, 1.0, 0.0],
        FilterMode::Bandpass => [0.0, 0.0, 1.0],
        FilterMode::Notch => [1.0, 1.0, 0.0],
    }
}

/// The output of a filter whose `weights` are those of its mode.
#[inline]
pub(super) fn tap([lowpass, highpass, bandpass]: [f64; 3], weights: [f64; 3]) -> f64 {
    lowpass * weights[0] + highpass * weights[1] + bandpass * weights[2]
}

/// Filter coefficient `g` of a cutoff of `freq` Hz, kept in the audible
/// range and under [`MAX_CUTOFF_RATIO`].
pub(super) fn cutoff_coefficient(freq: f64, sample_rate: f64) -> f64 {
    let freq = clamp(freq, crate::MIN_FILTER_FREQ, crate::MAX_FILTER_FREQ).min(sample_rate * MAX_CUTOFF_RATIO);
    (PI * freq / sample_rate).tan()
}

/// TPT state-variable filter state.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Tpt {
    z1: f64,
    z2: f64,
}

impl Tpt {
    /// One sample `x` through the filter of coefficient `g` and damping `k`,
    /// which a modulation may have pushed out of range: (lowpass, highpass,
    /// bandpass).
    #[inline]
    pub(super) fn step(&mut self, x: f64, g: f64, k: f64) -> [f64; 3] {
        if !self.z1.is_finite() {
            self.z1 = 0.0;
        }
        if !self.z2.is_finite() {
            self.z2 = 0.0;
        }
        let g = clamp(g, 0.0001, 10.0);
        let k = clamp(k, 0.05, 10.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - self.z2;
        let v1 = a1 * self.z1 + a2 * v3;
        let v2 = self.z2 + a2 * self.z1 + a3 * v3;
        self.z1 = 2.0 * v1 - self.z1;
        self.z2 = 2.0 * v2 - self.z2;
        if self.z1.abs() < FLUSH || !self.z1.is_finite() {
            self.z1 = 0.0;
        }
        if self.z2.abs() < FLUSH || !self.z2.is_finite() {
            self.z2 = 0.0;
        }
        [v2, x - k * v1 - v2, v1]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_splits_a_step_into_its_bands() {
        let mut filter = Tpt::default();
        let g = cutoff_coefficient(1_000.0, 48_000.0);
        let mut last = [0.0; 3];
        for _ in 0..4_800 {
            last = filter.step(1.0, g, 1.0);
        }
        // A constant input: all of it in the lowpass, none above.
        assert!((last[0] - 1.0).abs() < 1e-9 && last[1].abs() < 1e-9 && last[2].abs() < 1e-9, "{last:?}");
        assert_eq!(tap(last, tap_weights(FilterMode::Notch)), last[0] + last[1]);
        assert_eq!(tap(last, tap_weights(FilterMode::Bandpass)), last[2]);
    }

    #[test]
    fn the_three_bands_add_up_to_the_input() {
        let mut filter = Tpt::default();
        let g = cutoff_coefficient(5_000.0, 48_000.0);
        for i in 0..200 {
            let x = f64::from(i % 7) - 3.0;
            let [lowpass, highpass, bandpass] = filter.step(x, g, 0.5);
            assert!((lowpass + highpass + 0.5 * bandpass - x).abs() < 1e-12);
        }
    }

    #[test]
    fn the_cutoff_stops_below_half_the_sample_rate() {
        assert!((cutoff_coefficient(1e9, 8_000.0) - (PI * MAX_CUTOFF_RATIO).tan()).abs() < 1e-9);
        assert_eq!(cutoff_coefficient(0.0, 48_000.0), (PI * crate::MIN_FILTER_FREQ / 48_000.0).tan());
    }
}
