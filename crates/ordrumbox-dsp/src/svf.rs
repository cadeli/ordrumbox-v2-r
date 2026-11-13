//! The TPT state-variable filter (12 dB/oct) of the channel strip and of the
//! synth voice: one stage, its coefficients, and the output a mode takes.

use crate::slope::FilterMode;

/// A state below this is flushed to zero, against denormals.
const FLUSH: f64 = 1e-15;

/// How much of the lowpass, highpass and bandpass outputs `mode` lets
/// through, for [`tap`].
pub(crate) const fn tap_weights(mode: FilterMode) -> [f64; 3] {
    match mode {
        FilterMode::Lowpass => [1.0, 0.0, 0.0],
        FilterMode::Highpass => [0.0, 1.0, 0.0],
        FilterMode::Bandpass => [0.0, 0.0, 1.0],
        FilterMode::Notch => [1.0, 1.0, 0.0],
    }
}

/// The output of a filter whose `weights` are those of its mode.
#[inline]
pub(crate) fn tap([lowpass, highpass, bandpass]: [f64; 3], weights: [f64; 3]) -> f64 {
    lowpass * weights[0] + highpass * weights[1] + bandpass * weights[2]
}

/// The output of a filter in `mode`, for a caller whose mode holds for the
/// whole block: the branch is always the same one, and costs less than the
/// three products of [`tap`] (measured on the strip: 5 ns a frame less).
#[inline]
pub(crate) fn output([lowpass, highpass, bandpass]: [f64; 3], mode: FilterMode) -> f64 {
    match mode {
        FilterMode::Lowpass => lowpass,
        FilterMode::Highpass => highpass,
        FilterMode::Bandpass => bandpass,
        FilterMode::Notch => lowpass + highpass,
    }
}

/// Coefficients of one [`Svf`] stage at a cutoff and damping.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SvfCoeffs {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl SvfCoeffs {
    /// `g` is `tan(pi * cutoff / sample_rate)`, `k` is `1 / q`.
    #[inline]
    pub(crate) fn new(g: f64, k: f64) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { k, a1, a2, a3: g * a2 }
    }
}

/// TPT state-variable filter state (one channel, one stage).
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Svf {
    z1: f64,
    z2: f64,
}

impl Svf {
    /// One sample `x` through the filter: (lowpass, highpass, bandpass).
    #[inline]
    pub(crate) fn step(&mut self, x: f64, c: &SvfCoeffs) -> [f64; 3] {
        let v3 = x - self.z2;
        let v1 = c.a1 * self.z1 + c.a2 * v3;
        let v2 = self.z2 + c.a2 * self.z1 + c.a3 * v3;
        self.z1 = 2.0 * v1 - self.z1;
        self.z2 = 2.0 * v2 - self.z2;
        flush(&mut self.z1);
        flush(&mut self.z2);
        [v2, x - c.k * v1 - v2, v1]
    }
}

/// Zero a `state` that is tiny or not a number: a filter never stays stuck
/// on a denormal, an infinity or a NaN.
///
/// A branch, not a selected value: it is almost never taken, and a select
/// would sit on the filter's feedback path (measured: 2 ns a frame).
#[inline]
fn flush(state: &mut f64) {
    if state.abs() < FLUSH || !state.is_finite() {
        *state = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn coefficients(cutoff: f64, k: f64) -> SvfCoeffs {
        SvfCoeffs::new((PI * cutoff / 48_000.0).tan(), k)
    }

    #[test]
    fn the_filter_splits_a_step_into_its_bands() {
        let mut filter = Svf::default();
        let c = coefficients(1_000.0, 1.0);
        let mut last = [0.0; 3];
        for _ in 0..4_800 {
            last = filter.step(1.0, &c);
        }
        // A constant input: all of it in the lowpass, none above.
        assert!((last[0] - 1.0).abs() < 1e-9 && last[1].abs() < 1e-9 && last[2].abs() < 1e-9, "{last:?}");
        assert_eq!(tap(last, tap_weights(FilterMode::Notch)), last[0] + last[1]);
        assert_eq!(tap(last, tap_weights(FilterMode::Bandpass)), last[2]);
        for mode in FilterMode::ALL {
            assert_eq!(output(last, mode), tap(last, tap_weights(mode)), "{mode:?}");
        }
    }

    #[test]
    fn the_three_bands_add_up_to_the_input() {
        let mut filter = Svf::default();
        let c = coefficients(5_000.0, 0.5);
        for i in 0..200 {
            let x = f64::from(i % 7) - 3.0;
            let [lowpass, highpass, bandpass] = filter.step(x, &c);
            assert!((lowpass + highpass + 0.5 * bandpass - x).abs() < 1e-12);
        }
    }

    #[test]
    fn a_filter_recovers_from_an_input_that_is_not_a_number() {
        let mut filter = Svf::default();
        let c = coefficients(1_000.0, 1.0);
        filter.step(f64::NAN, &c);
        assert!(filter.step(1.0, &c).iter().all(|band| band.is_finite()));
    }
}
