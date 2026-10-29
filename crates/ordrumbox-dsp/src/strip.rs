//! A track's channel strip (`worklets/processors/strip_source.js`): a stereo
//! TPT state-variable filter, saturation, a Freeverb reverb, a delay, then
//! volume and compensated equal-power pan.
//!
//! The reverb and delay memories are allocated by [`Strip::new`];
//! [`Strip::process`] never allocates.

use std::f64::consts::PI;

const COMB_TUNINGS_L: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const COMB_TUNINGS_R: [usize; 8] = [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640];
const ALLPASS_TUNINGS_L: [usize; 4] = [556, 441, 341, 225];
const ALLPASS_TUNINGS_R: [usize; 4] = [579, 464, 364, 248];
/// Longest delay, seconds (the `dlyTime*` parameters stop at 2).
const DELAY_SECONDS: f64 = 2.1;
/// The JS delay line always holds 2.1 s at 48 kHz, whatever the rate.
const DELAY_MIN_RATE: f64 = 48_000.0;
/// Level below which states and outputs are flushed to zero.
const FLUSH: f64 = 1e-15;

audio_params! {
    /// The strip processor's parameters (its `parameterDescriptors`).
    pub struct StripSettings {
        /// Filter cutoff, Hz (20 000 with the `allpass` filter type).
        cutoff = 20_000.0, 20.0..=20_000.0;
        q = 0.707, 0.707..=18.707;
        /// 0 lowpass, 1 highpass, 2 bandpass, 3 notch.
        filter_mode = 0.0, 0.0..=3.0;
        /// 0 tanh, 1 hard clip, 2 atan.
        saturation_type = 0.0, 0.0..=2.0;
        saturation_drive = 1.0, 1.0..=7.0;
        saturation_output = 1.0, 0.0..=1.0;
        saturation_mix = 0.0, 0.0..=1.0;
        reverb_room = 0.5, 0.0..=1.0;
        reverb_damping = 0.5, 0.0..=1.0;
        reverb_width = 1.0, 0.0..=1.0;
        reverb_mix = 0.0, 0.0..=1.0;
        /// Delay times, seconds.
        delay_time_l = 0.25, 0.0..=2.0;
        delay_time_r = 0.25, 0.0..=2.0;
        delay_feedback = 0.4, 0.0..=0.99;
        delay_mix = 0.0, 0.0..=1.0;
        /// 0 slap, 1 tape, 2 ping-pong.
        delay_mode = 1.0, 0.0..=2.0;
        volume = 1.0, 0.0..=2.0;
        pan = 0.0, -1.0..=1.0;
    }
}

#[derive(Debug, Clone)]
struct Comb {
    buf: Vec<f32>,
    index: usize,
    store: f64,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0, store: 0.0 }
    }

    #[inline]
    fn process(&mut self, input: f64, d1: f64, d2: f64, feedback: f64) -> f64 {
        let out = f64::from(self.buf[self.index]);
        self.store = out * d2 + self.store * d1;
        self.buf[self.index] = (input + self.store * feedback) as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
struct Allpass {
    buf: Vec<f32>,
    index: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0 }
    }

    #[inline]
    fn process(&mut self, input: f64) -> f64 {
        let bout = f64::from(self.buf[self.index]);
        let out = -input + bout;
        self.buf[self.index] = (input + bout * 0.5) as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
        out
    }
}

#[derive(Debug, Clone)]
struct DelayLine {
    buf: Vec<f32>,
    index: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], index: 0 }
    }

    /// The sample `delay` frames ago, linearly interpolated.
    #[inline]
    fn read(&self, delay: f64) -> f64 {
        let len = self.buf.len() as f64;
        let mut position = self.index as f64 - delay;
        position = ((position % len) + len) % len;
        let i = position as usize;
        let fraction = position - i as f64;
        let next = if i + 1 >= self.buf.len() { 0 } else { i + 1 };
        f64::from(self.buf[i]) * (1.0 - fraction) + f64::from(self.buf[next]) * fraction
    }

    #[inline]
    fn write(&mut self, value: f64) {
        self.buf[self.index] = value as f32;
        self.index += 1;
        if self.index >= self.buf.len() {
            self.index = 0;
        }
    }
}

/// `#shape()`: drive, waveshape, dry/wet, output gain.
#[inline]
fn shape(x: f64, drive: f64, kind: f64, mix: f64, out: f64) -> f64 {
    if mix <= 0.0 {
        return x;
    }
    let d = x * drive;
    let s = if kind < 0.5 {
        d.tanh()
    } else if kind < 1.5 {
        d.clamp(-1.0, 1.0)
    } else {
        d.atan()
    };
    (x * (1.0 - mix) + s * mix) * out
}

#[inline]
fn flush(x: &mut f64) {
    if x.abs() < FLUSH {
        *x = 0.0;
    }
}

/// State of one strip.
#[derive(Debug, Clone)]
pub struct Strip {
    sample_rate: f64,
    z1l: f64,
    z2l: f64,
    z1r: f64,
    z2r: f64,
    combs_l: [Comb; 8],
    combs_r: [Comb; 8],
    allpass_l: [Allpass; 4],
    allpass_r: [Allpass; 4],
    delay_l: DelayLine,
    delay_r: DelayLine,
    delay_filter_l: f64,
    delay_filter_r: f64,
}

impl Strip {
    /// A silent strip at `sample_rate` (allocates its reverb and delay memories).
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = f64::from(sample_rate);
        let delay_len = (DELAY_SECONDS * sample_rate.max(DELAY_MIN_RATE)).ceil() as usize;
        Self {
            sample_rate,
            z1l: 0.0,
            z2l: 0.0,
            z1r: 0.0,
            z2r: 0.0,
            combs_l: COMB_TUNINGS_L.map(Comb::new),
            combs_r: COMB_TUNINGS_R.map(Comb::new),
            allpass_l: ALLPASS_TUNINGS_L.map(Allpass::new),
            allpass_r: ALLPASS_TUNINGS_R.map(Allpass::new),
            delay_l: DelayLine::new(delay_len),
            delay_r: DelayLine::new(delay_len),
            delay_filter_l: 0.0,
            delay_filter_r: 0.0,
        }
    }

    /// Process `in_l`/`in_r` with `settings` and ADD the result to
    /// `out_l`/`out_r` (all four the same length).
    pub fn process(
        &mut self,
        settings: &StripSettings,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let s = settings.clamped();
        let sr = self.sample_rate;
        let (cutoff, q, mode) = (f64::from(s.cutoff), f64::from(s.q), f64::from(s.filter_mode));
        let (saturation_drive, saturation_type, saturation_mix, saturation_output) = (
            f64::from(s.saturation_drive),
            f64::from(s.saturation_type),
            f64::from(s.saturation_mix),
            f64::from(s.saturation_output),
        );
        let (reverb_room, reverb_damping, reverb_width, reverb_mix) =
            (f64::from(s.reverb_room), f64::from(s.reverb_damping), f64::from(s.reverb_width), f64::from(s.reverb_mix));
        let (delay_mix, delay_feedback, delay_mode) =
            (f64::from(s.delay_mix), f64::from(s.delay_feedback), f64::from(s.delay_mode));
        let (delay_time_l, delay_time_r) = (f64::from(s.delay_time_l), f64::from(s.delay_time_r));
        let vol = f64::from(s.volume);

        let rm = reverb_room * 0.28 + 0.7;
        let damp1 = reverb_damping * 0.4;
        let damp2 = 1.0 - damp1;
        let width1 = reverb_width * 0.5 + 0.5;
        let width2 = (1.0 - reverb_width) * 0.5;
        let has_reverb = reverb_mix > 0.001;
        let has_delay = delay_mix > 0.001;
        let has_sat = saturation_mix > 0.0;
        let ping_pong = delay_mode > 1.5;

        let p = f64::from(s.pan);
        let pan_l = ((p + 1.0) * PI / 4.0).cos();
        let pan_r = ((p + 1.0) * PI / 4.0).sin();
        let pan_max = if pan_l > pan_r { pan_l } else { pan_r };
        let pan_comp = if pan_max > 0.001 { 1.0 / pan_max } else { 1.0 };

        let g = (PI * cutoff / sr).tan();
        let k = 1.0 / q;
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;

        let frames = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        for i in 0..frames {
            // 1. Filter.
            let v3l = f64::from(in_l[i]) - self.z2l;
            let v1l = a1 * self.z1l + a2 * v3l;
            let v2l = self.z2l + a2 * self.z1l + a3 * v3l;
            self.z1l = 2.0 * v1l - self.z1l;
            self.z2l = 2.0 * v2l - self.z2l;
            let v3r = f64::from(in_r[i]) - self.z2r;
            let v1r = a1 * self.z1r + a2 * v3r;
            let v2r = self.z2r + a2 * self.z1r + a3 * v3r;
            self.z1r = 2.0 * v1r - self.z1r;
            self.z2r = 2.0 * v2r - self.z2r;
            flush(&mut self.z1l);
            flush(&mut self.z2l);
            flush(&mut self.z1r);
            flush(&mut self.z2r);
            let (dry_l, dry_r) = if mode < 0.5 {
                (v2l, v2r)
            } else if mode < 1.5 {
                (v3l - v1l * k, v3r - v1r * k)
            } else if mode < 2.5 {
                (v1l, v1r)
            } else {
                (v3l - v1l * k + v2l, v3r - v1r * k + v2r)
            };

            // 2. Saturation.
            let sat_l = if has_sat {
                shape(dry_l, saturation_drive, saturation_type, saturation_mix, saturation_output)
            } else {
                dry_l
            };
            let sat_r = if has_sat {
                shape(dry_r, saturation_drive, saturation_type, saturation_mix, saturation_output)
            } else {
                dry_r
            };

            // 3. Reverb.
            let (mut rev_l, mut rev_r) = (0.0, 0.0);
            if has_reverb {
                let input = (sat_l + sat_r) * 0.0075;
                let (mut comb_l, mut comb_r) = (0.0, 0.0);
                for (left, right) in self.combs_l.iter_mut().zip(&mut self.combs_r) {
                    comb_l += left.process(input, damp1, damp2, rm);
                    comb_r += right.process(input, damp1, damp2, rm);
                }
                for (left, right) in self.allpass_l.iter_mut().zip(&mut self.allpass_r) {
                    comb_l = left.process(comb_l);
                    comb_r = right.process(comb_r);
                }
                rev_l = (comb_l * width1 + comb_r * width2) * reverb_mix;
                rev_r = (comb_r * width1 + comb_l * width2) * reverb_mix;
            }

            // 4. Delay.
            let (mut wet_l, mut wet_r) = (0.0, 0.0);
            if has_delay {
                let delayed_l = self.delay_l.read(delay_time_l * sr);
                let delayed_r = self.delay_r.read(delay_time_r * sr);
                self.delay_filter_l += 0.5 * (delayed_l - self.delay_filter_l);
                self.delay_filter_r += 0.5 * (delayed_r - self.delay_filter_r);
                let fb_l = shape(self.delay_filter_l * delay_feedback, 1.5, 0.0, 0.5, 1.0);
                let fb_r = shape(self.delay_filter_r * delay_feedback, 1.5, 0.0, 0.5, 1.0);
                if ping_pong {
                    self.delay_l.write(sat_l + fb_r);
                    self.delay_r.write(sat_r + fb_l);
                } else {
                    self.delay_l.write(sat_l + fb_l);
                    self.delay_r.write(sat_r + fb_r);
                }
                wet_l = delayed_l * delay_mix;
                wet_r = delayed_r * delay_mix;
            } else {
                self.delay_l.write(sat_l);
                self.delay_r.write(sat_r);
            }

            // 5. Volume and pan.
            let mut left = (sat_l + rev_l + wet_l) * vol * pan_l * pan_comp;
            let mut right = (sat_r + rev_r + wet_r) * vol * pan_r * pan_comp;
            flush(&mut left);
            flush(&mut right);
            out_l[i] += left.clamp(-2.0, 2.0) as f32;
            out_r[i] += right.clamp(-2.0, 2.0) as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(settings: &StripSettings, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(settings, input, input, &mut l, &mut r);
        (l, r)
    }

    #[test]
    fn defaults_pass_the_signal_through() {
        let input: Vec<f32> = (0..256).map(|i| if i % 50 == 0 { 0.5 } else { 0.0 }).collect();
        let (l, r) = run(&StripSettings::default(), &input);
        assert_eq!(l, r);
        // 20 kHz lowpass at 48 kHz: the energy stays.
        let energy = |s: &[f32]| s.iter().map(|x| x * x).sum::<f32>();
        assert!((energy(&l) / energy(&input) - 1.0).abs() < 0.2);
    }

    #[test]
    fn hard_pan_and_volume() {
        let input = vec![0.25; 64];
        let settings = StripSettings { pan: 1.0, volume: 2.0, ..StripSettings::default() };
        let (l, r) = run(&settings, &input);
        assert!(l[63].abs() < 1e-6);
        assert!((r[63] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn reverb_tail_outlasts_the_input() {
        let mut input = vec![0.0; 6000];
        input[0] = 1.0;
        let settings = StripSettings { reverb_mix: 0.5, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        assert!(l[3000..].iter().any(|s| s.abs() > 1e-4));
    }

    #[test]
    fn delay_repeats_after_its_time() {
        let mut input = vec![0.0; 2000];
        input[0] = 1.0;
        let settings =
            StripSettings { delay_time_l: 0.01, delay_time_r: 0.01, delay_mix: 1.0, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        let echo = l[400..600].iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0 + 400;
        assert!((479..=482).contains(&echo), "echo at {echo}");
    }
}
