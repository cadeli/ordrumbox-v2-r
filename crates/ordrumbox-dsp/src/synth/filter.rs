//! The coefficients of a synth voice's filters (the main one and the noise's),
//! whose cutoff and resonance move from one sample to the next.

use std::f64::consts::PI;

use super::clamp;
use crate::svf::SvfCoeffs;

/// Highest filter cutoff, as a fraction of the sample rate.
pub(super) const MAX_CUTOFF_RATIO: f64 = 0.45;

/// Filter coefficient `g` of a cutoff of `freq` Hz, kept in the audible
/// range and under [`MAX_CUTOFF_RATIO`].
pub(super) fn cutoff_coefficient(freq: f64, sample_rate: f64) -> f64 {
    let freq = clamp(freq, crate::MIN_FILTER_FREQ, crate::MAX_FILTER_FREQ).min(sample_rate * MAX_CUTOFF_RATIO);
    (PI * freq / sample_rate).tan()
}

/// The coefficients of a stage at coefficient `g` and damping `k`, which a
/// modulation may have pushed out of range.
#[inline]
pub(super) fn coefficients(g: f64, k: f64) -> SvfCoeffs {
    SvfCoeffs::new(clamp(g, 0.0001, 10.0), clamp(k, 0.05, 10.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cutoff_stops_below_half_the_sample_rate() {
        assert!((cutoff_coefficient(1e9, 8_000.0) - (PI * MAX_CUTOFF_RATIO).tan()).abs() < 1e-9);
        assert_eq!(cutoff_coefficient(0.0, 48_000.0), (PI * crate::MIN_FILTER_FREQ / 48_000.0).tan());
    }
}
