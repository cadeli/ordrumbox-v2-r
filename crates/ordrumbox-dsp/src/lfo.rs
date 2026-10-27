//! LFO waveforms (`getLfoWaveformValue()` in `src/audio/math.js`, inlined as
//! `lfoWave()` in the synth voice processor).

use std::f64::consts::TAU;

use crate::to_int32;

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

/// Deterministic pseudo-random value in [-1, 1) for cycle index `cycle`.
pub fn sample_and_hold(cycle: f64) -> f64 {
    let mut rng = to_int32(cycle * 1_234_567.0 + 890_123.0);
    rng ^= rng << 13;
    rng ^= rng >> 17;
    rng ^= rng << 5;
    f64::from(rng) / 2_147_483_648.0
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
    fn sample_and_hold_matches_js() {
        // (0 * 1234567 + 890123) | 0 = 890123, then xorshift in int32.
        let mut r: i32 = 890_123;
        r ^= r << 13;
        r ^= r >> 17;
        r ^= r << 5;
        assert_eq!(sample_and_hold(0.0), f64::from(r) / 2_147_483_648.0);
        assert!((-1.0..1.0).contains(&sample_and_hold(12_345.0)));
    }
}
