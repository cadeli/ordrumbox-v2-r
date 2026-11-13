//! The 24 dB/oct filter shared by the channel strip and the synth voice: two
//! 12 dB/oct state-variable stages in series.

use std::f64::consts::FRAC_1_SQRT_2;

/// Q of the two stages of a 4-pole Butterworth: at the default Q (0.707)
/// the 24 dB/oct lowpass and highpass are flat up to the cutoff.
const BUTTERWORTH_Q1: f64 = 0.541_196_100_146_197;
const BUTTERWORTH_Q2: f64 = 1.306_562_964_876_376_5;
/// A slope (dB/oct) above this is 24, else 12.
const SLOPE_24_FROM: f64 = 18.0;

/// Whether a filter slope in dB/oct selects two stages.
#[inline]
pub fn is_steep(db_per_octave: f64) -> bool {
    db_per_octave > SLOPE_24_FROM
}

/// What a state-variable filter lets through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterMode {
    #[default]
    Lowpass,
    Highpass,
    Bandpass,
    Notch,
}

impl FilterMode {
    /// Every mode, each at its code.
    pub const ALL: [Self; 4] = [Self::Lowpass, Self::Highpass, Self::Bandpass, Self::Notch];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }

    /// The mode a strip's `filter_mode` setting stands for: the nearest code,
    /// the notch above the last.
    #[must_use]
    pub fn from_setting(setting: f64) -> Self {
        if setting < 0.5 {
            Self::Lowpass
        } else if setting < 1.5 {
            Self::Highpass
        } else if setting < 2.5 {
            Self::Bandpass
        } else {
            Self::Notch
        }
    }
}

/// Dampings (1/Q) of the two stages of a 24 dB/oct filter in `mode` whose
/// single stage would have damping `k`, and the gain applied to the second
/// stage's output.
///
/// The lowpass and highpass are a Butterworth pair whose second stage carries
/// the resonance, so the peak at the cutoff is as high as one stage's at the
/// same Q. The bandpass and notch repeat the stage; the second bandpass is
/// normalized (peak gain 1), so the peak stays the single stage's.
#[inline]
#[must_use]
pub fn stages(k: f64, mode: FilterMode) -> (f64, f64, f64) {
    match mode {
        FilterMode::Lowpass | FilterMode::Highpass => (1.0 / BUTTERWORTH_Q1, k * FRAC_1_SQRT_2 / BUTTERWORTH_Q2, 1.0),
        FilterMode::Bandpass => (k, k, k),
        FilterMode::Notch => (k, k, 1.0),
    }
}
