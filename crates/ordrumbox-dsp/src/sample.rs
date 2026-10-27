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
    fn cubic_read_hits_the_samples_and_stays_smooth() {
        let line = [0.0, 1.0, 2.0, 3.0, 4.0];
        assert_eq!(SampleBuffer::read(&line, 2.0), 2.0);
        assert!((SampleBuffer::read(&line, 2.5) - 2.5).abs() < 1e-6);
        assert_eq!(SampleBuffer::read(&line, 10.0), 0.0);
        assert_eq!(SampleBuffer::read(&line, -3.0), 0.0);
    }
}
