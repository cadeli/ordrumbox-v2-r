//! What the end-to-end render tests share: a render written to a 16-bit WAV
//! file and read back, and the measurements made on it.

#![allow(clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_engine::offline::EXPORT_RATE;
use ordrumbox_engine::wav;

pub const RATE: f64 = EXPORT_RATE as f64;

/// The frame `seconds` into a render.
pub fn frame(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// A linear fade in and out of `fade` frames over `frames`, at frame `i`.
pub fn edge(i: usize, frames: usize, fade: usize) -> f64 {
    (i.min(frames - 1 - i) as f64 / fade as f64).min(1.0)
}

/// The stereo channels of a WAV file.
#[derive(Debug, Clone, PartialEq)]
pub struct Wav {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// `left` and `right` written to a 16-bit WAV file and read back, unclipped.
pub fn through_wav(left: &[f32], right: &[f32]) -> Wav {
    let file = wav::decode(&wav::encode(left, right, EXPORT_RATE)).expect("reads back");
    assert_eq!(file.sample_rate, EXPORT_RATE);
    let right = file.right.expect("a stereo file");
    assert!(file.left.iter().chain(&right).all(|s| s.abs() < 1.0), "no clipping");
    Wav { left: file.left, right }
}

pub fn energy(signal: &[f32]) -> f64 {
    signal.iter().map(|&s| f64::from(s).powi(2)).sum()
}

pub fn rms(signal: &[f32]) -> f64 {
    (energy(signal) / signal.len().max(1) as f64).sqrt()
}

pub fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The signal between two times, seconds.
pub fn span(signal: &[f32], from: f64, to: f64) -> &[f32] {
    &signal[frame(from)..frame(to)]
}

/// Energy of the first difference over energy: about 2 for white noise,
/// near 0 for a signal without highs.
pub fn brightness(signal: &[f32]) -> f64 {
    let diff: f64 = signal.windows(2).map(|w| f64::from(w[1] - w[0]).powi(2)).sum();
    diff / energy(signal)
}

/// Amplitude of the `hz` component of `signal` (a Goertzel filter).
pub fn amplitude_at(signal: &[f32], hz: f64) -> f64 {
    let coefficient = 2.0 * (std::f64::consts::TAU * hz / RATE).cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for &x in signal {
        let s = f64::from(x) + coefficient * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - coefficient * s1 * s2).sqrt() * 2.0 / signal.len() as f64
}
