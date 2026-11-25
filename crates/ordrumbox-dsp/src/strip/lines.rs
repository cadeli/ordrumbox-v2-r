//! The memories of a strip's reverb and delay: Freeverb's comb and allpass
//! filters, a delay line read between samples, and the first-order allpass
//! a spring's dispersion is made of.

use crate::denormal::flush;

#[derive(Debug, Clone)]
pub(super) struct Comb {
    buf: Vec<f32>,
    index: usize,
    store: f64,
}

impl Comb {
    pub(super) fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0, store: 0.0 }
    }

    /// Frames the comb holds.
    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.buf.len()
    }

    #[inline]
    pub(super) fn process(&mut self, input: f64, d1: f64, d2: f64, feedback: f64) -> f64 {
        let out = f64::from(self.buf[self.index]);
        self.store = out * d2 + self.store * d1;
        flush(&mut self.store);
        let mut write = input + self.store * feedback;
        flush(&mut write);
        self.buf[self.index] = write as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
pub(super) struct Allpass {
    buf: Vec<f32>,
    index: usize,
}

impl Allpass {
    pub(super) fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0 }
    }

    #[inline]
    pub(super) fn process(&mut self, input: f64) -> f64 {
        let bout = f64::from(self.buf[self.index]);
        let out = -input + bout;
        let mut write = input + bout * 0.5;
        flush(&mut write);
        self.buf[self.index] = write as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
pub(super) struct DelayLine {
    buf: Vec<f32>,
    index: usize,
}

impl DelayLine {
    pub(super) fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0 }
    }

    /// The sample `delay` frames ago, linearly interpolated.
    #[inline]
    pub(super) fn read(&self, delay: f64) -> f64 {
        let len = self.buf.len() as f64;
        let position = crate::wrap(self.index as f64 - delay, len);
        let i = position as usize;
        let fraction = position - i as f64;
        let next = if i + 1 >= self.buf.len() { 0 } else { i + 1 };
        f64::from(self.buf[i]) * (1.0 - fraction) + f64::from(self.buf[next]) * fraction
    }

    /// Push `value`, flushed to 0 when it is tiny: a line that feeds back
    /// would otherwise fade into denormals.
    #[inline]
    pub(super) fn write(&mut self, mut value: f64) {
        flush(&mut value);
        self.buf[self.index] = value as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
    }
}

/// First-order allpass: `y = -a x + x[n-1] + a y[n-1]`.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Dispersion {
    x1: f64,
    y1: f64,
}

impl Dispersion {
    #[inline]
    pub(super) fn process(&mut self, x: f64, a: f64) -> f64 {
        let mut y = -a * x + self.x1 + a * self.y1;
        flush(&mut y);
        self.x1 = x;
        self.y1 = y;
        y
    }
}
