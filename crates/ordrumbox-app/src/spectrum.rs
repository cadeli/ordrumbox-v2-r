//! The output panel's spectrum (`spectrum_section.js`): what the JS master
//! bus `AnalyserNode` gives `getByteFrequencyData()` — computed here as the
//! Web Audio specification describes it, with the node's settings (`fftSize`
//! 4096, the default smoothing 0.8 and range -100 to -30 dB): a Blackman
//! window, an FFT, the magnitudes smoothed over time, in dB, as bytes.

use std::f64::consts::PI;

/// Samples per analysis (`analyser.fftSize`).
pub const FFT_SIZE: usize = ordrumbox_engine::SCOPE_SIZE;
/// Bins.
pub const BINS: usize = FFT_SIZE / 2;
const SMOOTHING: f64 = 0.8;
const MIN_DB: f64 = -100.0;
const MAX_DB: f64 = -30.0;

/// An analyser: the smoothed magnitudes of the last call.
#[derive(Debug, Clone)]
pub struct Analyser {
    smoothed: Vec<f64>,
    window: Vec<f64>,
}

impl Default for Analyser {
    fn default() -> Self {
        // The Blackman window, alpha 0.16.
        let (a0, a1, a2) = (0.42, 0.5, 0.08);
        let n = FFT_SIZE as f64;
        let window = (0..FFT_SIZE)
            .map(|i| a0 - a1 * (2.0 * PI * i as f64 / n).cos() + a2 * (4.0 * PI * i as f64 / n).cos())
            .collect();
        Self { smoothed: vec![0.0; BINS], window }
    }
}

impl Analyser {
    /// `getByteFrequencyData()` of `samples` (oldest first): one byte a bin.
    pub fn bytes(&mut self, samples: &[f32; FFT_SIZE]) -> Vec<u8> {
        let mut re: Vec<f64> = samples.iter().zip(&self.window).map(|(&s, w)| f64::from(s) * w).collect();
        let mut im = vec![0.0; FFT_SIZE];
        fft(&mut re, &mut im);
        let scale = 1.0 / FFT_SIZE as f64;
        (0..BINS)
            .map(|k| {
                let magnitude = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
                let smoothed = SMOOTHING * self.smoothed[k] + (1.0 - SMOOTHING) * magnitude;
                self.smoothed[k] = if smoothed.is_finite() { smoothed } else { 0.0 };
                let db = 20.0 * self.smoothed[k].log10();
                (255.0 / (MAX_DB - MIN_DB) * (db - MIN_DB)).floor().clamp(0.0, 255.0) as u8
            })
            .collect()
    }
}

/// In-place radix-2 FFT of `re` + i `im` (a power of two long).
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let angle = -2.0 * PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (angle * k as f64).sin_cos();
                let (a, b) = (start + k, start + k + len / 2);
                let (tr, ti) = (re[b] * c - im[b] * s, re[b] * s + im[b] * c);
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_matches_a_plain_dft() {
        let n = 64;
        let input: Vec<f64> = (0..n).map(|i| ((i * 7 % 13) as f64 - 6.0) / 6.0).collect();
        let (mut re, mut im) = (input.clone(), vec![0.0; n]);
        fft(&mut re, &mut im);
        for k in 0..n {
            let (mut dr, mut di) = (0.0, 0.0);
            for (t, x) in input.iter().enumerate() {
                let a = -2.0 * PI * (k * t) as f64 / n as f64;
                dr += x * a.cos();
                di += x * a.sin();
            }
            assert!((re[k] - dr).abs() < 1e-9 && (im[k] - di).abs() < 1e-9, "bin {k}");
        }
    }

    #[test]
    fn a_tone_lights_its_bin_and_silence_decays() {
        let mut analyser = Analyser::default();
        // Bin 100 exactly, at full scale.
        let mut samples = [0.0f32; FFT_SIZE];
        for (i, s) in samples.iter_mut().enumerate() {
            *s = (2.0 * PI * 100.0 * i as f64 / FFT_SIZE as f64).sin() as f32;
        }
        let mut bytes = Vec::new();
        for _ in 0..30 {
            bytes = analyser.bytes(&samples);
        }
        // The window spreads the tone over its neighbours: the peak is its bin.
        let peak = (0..BINS).max_by(|&a, &b| analyser.smoothed[a].total_cmp(&analyser.smoothed[b])).unwrap();
        assert_eq!(peak, 100);
        assert_eq!(bytes[100], 255);
        assert_eq!(bytes[1000], 0);
        let silence = [0.0f32; FFT_SIZE];
        for _ in 0..60 {
            bytes = analyser.bytes(&silence);
        }
        assert_eq!(bytes[100], 0);
    }
}
