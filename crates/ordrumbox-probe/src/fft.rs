//! A radix-2 FFT and the windowed power spectrum made from it.

use std::f64::consts::{PI, TAU};

/// In-place forward FFT; the length of `re` and `im` is a power of two.
fn transform(re: &mut [f64], im: &mut [f64]) {
    let size = re.len();
    let mut j = 0;
    for i in 1..size {
        let mut bit = size >> 1;
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
    let mut length = 2;
    while length <= size {
        for start in (0..size).step_by(length) {
            for k in 0..length / 2 {
                let angle = -TAU * k as f64 / length as f64;
                let (w_im, w_re) = angle.sin_cos();
                let (a, b) = (start + k, start + k + length / 2);
                let t_re = re[b] * w_re - im[b] * w_im;
                let t_im = re[b] * w_im + im[b] * w_re;
                (re[b], im[b]) = (re[a] - t_re, im[a] - t_im);
                (re[a], im[a]) = (re[a] + t_re, im[a] + t_im);
            }
        }
        length <<= 1;
    }
}

/// Power per bin (`size / 2 + 1` bins) of `segment` under a Hann window as
/// long as the segment, zero-padded to `size`, a power of two. A sine of
/// amplitude `a` centred on a bin reads `a * a`.
pub fn power_spectrum(segment: &[f64], size: usize) -> Vec<f64> {
    let used = segment.len().min(size);
    let (mut re, mut im) = (vec![0.0; size], vec![0.0; size]);
    let mut window_sum = 0.0;
    for (i, sample) in segment[..used].iter().enumerate() {
        let window = 0.5 - 0.5 * (2.0 * PI * (i as f64 + 0.5) / used as f64).cos();
        re[i] = sample * window;
        window_sum += window;
    }
    transform(&mut re, &mut im);
    let scale = if window_sum > 0.0 { 2.0 / window_sum } else { 0.0 };
    (0..=size / 2).map(|i| (re[i] * re[i] + im[i] * im[i]) * scale * scale).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sine_reads_its_amplitude_in_its_bin() {
        let size = 1024;
        let samples: Vec<f64> = (0..size).map(|i| 0.5 * (TAU * 64.0 * i as f64 / size as f64).sin()).collect();
        let power = power_spectrum(&samples, size);
        assert_eq!(power.len(), size / 2 + 1);
        assert!((power[64].sqrt() - 0.5).abs() < 0.01);
        assert!(power[200] < 1e-12);
    }
}
