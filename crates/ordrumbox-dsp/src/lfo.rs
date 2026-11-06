//! LFO waveforms (`getLfoWaveformValue()` in `src/audio/math.js`, inlined as
//! `lfoWave()` in the synth voice processor).

use std::f64::consts::TAU;

/// Waveform codes: 0 sine, 1 triangle, 2 sawtooth, 3 square, 4 random.
pub const WAVE_NAMES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];

/// Value at `phase` (in cycles, not wrapped). Every shape is read a quarter
/// cycle late, so the sine and the square start at -1. Values are in [-1, 1]
/// except for the triangle, which the JS formula takes from -2 to 0, then from
/// 2 to 0 (kept: it is what the app plays). `random` holds one value per cycle.
pub fn wave(phase: f64, wave: f64) -> f64 {
    let p = (phase - 0.25) - (phase - 0.25).floor();
    if wave < 0.5 {
        (TAU * p).sin()
    } else if wave < 1.5 {
        if p < 0.25 {
            p * 4.0 - 1.0
        } else if p < 0.75 {
            3.0 - p * 4.0
        } else {
            p * 4.0 - 5.0
        }
    } else if wave < 2.5 {
        p * 2.0 - 1.0
    } else if wave < 3.5 {
        if p < 0.5 { 1.0 } else { -1.0 }
    } else {
        sample_and_hold(phase.floor())
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
        assert!((wave(0.0, 0.0) + 1.0).abs() < 1e-12);
        assert_eq!([wave(0.0, 2.0), wave(0.0, 3.0)], [0.5, -1.0]);
        assert!((wave(0.5, 0.0) - 1.0).abs() < 1e-12);
        assert_eq!([wave(0.0, 1.0), wave(0.25, 1.0), wave(0.5, 1.0), wave(0.75, 1.0)], [-2.0, -1.0, 2.0, 1.0]);
        assert_eq!(wave(0.3, 4.0), wave(0.9, 4.0));
        assert_ne!(wave(0.3, 4.0), wave(1.3, 4.0));
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
