//! LFO and oscillator waveforms.

use std::f64::consts::TAU;

/// The names of the waveforms, in the order of [`Wave::ALL`].
pub const WAVE_NAMES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];

/// A waveform of an oscillator or an LFO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wave {
    #[default]
    Sine,
    Triangle,
    Sawtooth,
    Square,
    /// One random value per cycle.
    Random,
}

impl Wave {
    /// Every waveform, each at its code.
    pub const ALL: [Self; 5] = [Self::Sine, Self::Triangle, Self::Sawtooth, Self::Square, Self::Random];

    /// Its code: its place in [`Self::ALL`] and [`WAVE_NAMES`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// Value at `phase` (in cycles, not wrapped). Every shape is read a quarter
/// cycle late, so the sine, the triangle and the square start at -1. Values
/// are in [-1, 1]. `random` holds one value per cycle.
#[inline]
#[must_use]
pub fn wave(phase: f64, wave: Wave) -> f64 {
    let p = (phase - 0.25) - (phase - 0.25).floor();
    match wave {
        Wave::Sine => (TAU * p).sin(),
        Wave::Triangle => {
            if p < 0.25 {
                p * 4.0
            } else if p < 0.75 {
                2.0 - p * 4.0
            } else {
                p * 4.0 - 4.0
            }
        }
        Wave::Sawtooth => p * 2.0 - 1.0,
        Wave::Square => {
            if p < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Wave::Random => sample_and_hold(phase.floor()),
    }
}

/// Deterministic pseudo-random value in [-1, 1) for cycle index `cycle`: the
/// splitmix64 finalizer of the integer cycle (saturated to i64; NaN is cycle 0),
/// its top 32 bits scaled to the range.
pub fn sample_and_hold(cycle: f64) -> f64 {
    // `as` saturates and maps NaN to 0; the cast to u64 keeps the bits.
    let mut x = (cycle as i64) as u64;
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    f64::from((x >> 32) as u32) / 2_147_483_648.0 - 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes() {
        assert!((wave(0.0, Wave::Sine) + 1.0).abs() < 1e-12);
        assert_eq!([wave(0.0, Wave::Sawtooth), wave(0.0, Wave::Square)], [0.5, -1.0]);
        assert!((wave(0.5, Wave::Sine) - 1.0).abs() < 1e-12);
        assert_eq!(
            [
                wave(0.0, Wave::Triangle),
                wave(0.25, Wave::Triangle),
                wave(0.5, Wave::Triangle),
                wave(0.75, Wave::Triangle)
            ],
            [-1.0, 0.0, 1.0, 0.0]
        );
        assert_eq!(wave(0.3, Wave::Random), wave(0.9, Wave::Random));
        assert_ne!(wave(0.3, Wave::Random), wave(1.3, Wave::Random));
        for (code, wave) in Wave::ALL.iter().enumerate() {
            assert_eq!(usize::from(wave.code()), code);
        }
    }

    #[test]
    fn sample_and_hold_holds_and_spreads() {
        assert_eq!(sample_and_hold(3.0), sample_and_hold(3.0));
        assert_eq!(sample_and_hold(f64::NAN), sample_and_hold(0.0));
        let values: Vec<f64> = (-500..500).map(|c| sample_and_hold(f64::from(c))).collect();
        assert!(values.iter().all(|v| (-1.0..1.0).contains(v)));
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        assert!(mean.abs() < 0.1, "mean {mean}");
        assert!(values.windows(2).all(|w| w[0] != w[1]));
    }
}
