//! A decoded sample: one or two channels of `f32` at the file's sample rate.

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SampleBuffer {
    pub left: Vec<f32>,
    /// `None` for a mono sample.
    pub right: Option<Vec<f32>>,
    /// Frames per second the sample was recorded at.
    pub sample_rate: u32,
}

impl SampleBuffer {
    pub fn mono(samples: Vec<f32>, sample_rate: u32) -> Self {
        Self { left: samples, right: None, sample_rate }
    }

    pub fn frames(&self) -> usize {
        self.left.len()
    }

    pub fn is_stereo(&self) -> bool {
        self.right.is_some()
    }

    /// The duration, seconds (0 at a rate of 0).
    #[must_use]
    pub fn seconds(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / f64::from(self.sample_rate) }
    }

    /// The zero crossing nearest to `seconds`, within `window` seconds either
    /// side, in seconds: the frame where the signal (both channels summed)
    /// changes sign or is 0. `seconds` itself when there is none in reach, so
    /// that a point set there does not click as little as it can.
    #[must_use]
    pub fn nearest_zero_crossing(&self, seconds: f64, window: f64) -> f64 {
        let rate = f64::from(self.sample_rate);
        if rate <= 0.0 || self.frames() < 2 || !seconds.is_finite() {
            return seconds;
        }
        let level = |i: usize| self.left[i] + self.right.as_ref().map_or(0.0, |r| r.get(i).copied().unwrap_or(0.0));
        let last = self.frames() - 1;
        // Whole frames, within the buffer: the casts only drop fractions.
        let center = (seconds * rate).round().clamp(0.0, last as f64) as usize;
        let reach = (window.max(0.0) * rate).round() as usize;
        let crosses = |i: usize| i >= 1 && (level(i) == 0.0 || (level(i - 1) < 0.0) != (level(i) < 0.0));
        (0..=reach)
            .flat_map(|d| [center.checked_sub(d), center.checked_add(d).filter(|&i| i <= last)])
            .flatten()
            .find(|&i| crosses(i))
            .map_or(seconds, |i| i as f64 / rate)
    }

    /// Value at a fractional frame position, 4-point cubic (Catmull-Rom)
    /// interpolation; silence outside the buffer.
    #[inline]
    pub(crate) fn read(channel: &[f32], position: f64) -> f32 {
        let index = position.floor();
        let frac = (position - index) as f32;
        let at = |offset: i64| {
            let i = index as i64 + offset;
            if i < 0 { 0.0 } else { channel.get(i as usize).copied().unwrap_or(0.0) }
        };
        let (y0, y1, y2, y3) = (at(-1), at(0), at(1), at(2));
        let c1 = 0.5 * (y2 - y0);
        let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
        let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
        ((c3 * frac + c2) * frac + c1) * frac + y1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nearest_zero_crossing_is_found_within_reach() {
        // Positive up to frame 9, negative from 10 on, at 1000 frames a second.
        let samples: Vec<f32> = (0..30).map(|i| if i < 10 { 0.5 } else { -0.5 }).collect();
        let buffer = SampleBuffer::mono(samples, 1000);
        assert_eq!(buffer.nearest_zero_crossing(0.012, 0.005), 0.010, "two frames before");
        assert_eq!(buffer.nearest_zero_crossing(0.020, 0.005), 0.020, "none in reach: where it was");
        assert_eq!(buffer.nearest_zero_crossing(0.5, 0.005), 0.5, "past the end: where it was");
        assert_eq!(buffer.seconds(), 0.03);
        // A silent frame counts as a crossing.
        let buffer = SampleBuffer::mono(vec![0.3, 0.3, 0.0, 0.3, 0.3], 1000);
        assert_eq!(buffer.nearest_zero_crossing(0.003, 0.002), 0.002);
    }

    #[test]
    fn cubic_read_hits_the_samples_and_stays_smooth() {
        let line = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(SampleBuffer::read(&line, 2.0), 2.0);
        assert!((SampleBuffer::read(&line, 2.5) - 2.5).abs() < 1e-6);
        assert_eq!(SampleBuffer::read(&line, 10.0), 0.0);
        assert_eq!(SampleBuffer::read(&line, -3.0), 0.0);
    }
}
