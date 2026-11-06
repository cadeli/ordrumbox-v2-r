//! A track's channel strip (`worklets/processors/strip_source.js`): a stereo
//! TPT state-variable filter (12 dB/oct, or two in series for 24 dB/oct),
//! saturation, a Freeverb reverb (with a pre-delay, a spring's dispersion and a
//! gate), a delay (tape wow, ping-pong), then volume and compensated
//! equal-power pan.
//!
//! The settings added to the JS processor default to what it does: the JS
//! fixtures play them unset. On purpose, the reverb's tunings follow the
//! sample rate (the JS keeps Freeverb's 44.1 kHz lengths at any rate) and the
//! `tape` saturation is asymmetric (an `atan` in JS).
//!
//! The reverb and delay memories are allocated by [`Strip::new`];
//! [`Strip::process`] never allocates.

use std::f64::consts::{PI, TAU};

use crate::slope;

const COMB_TUNINGS_L: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const COMB_TUNINGS_R: [usize; 8] = [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640];
const ALLPASS_TUNINGS_L: [usize; 4] = [556, 441, 341, 225];
const ALLPASS_TUNINGS_R: [usize; 4] = [579, 464, 364, 248];
/// Rate the Freeverb tunings are given at.
const TUNING_RATE: f64 = 44_100.0;
/// Longest reverb pre-delay, seconds.
const PREDELAY_SECONDS: f64 = 0.1;
/// First-order allpasses in series that spread a spring's input in time.
const DISPERSION_STAGES: usize = 8;
/// Input level (|left| + |right|) that opens the reverb gate.
const GATE_KEY: f64 = 0.05;
/// Time constants of the reverb gate, seconds: opening, closing.
const GATE_OPEN_SECONDS: f64 = 0.001;
const GATE_CLOSE_SECONDS: f64 = 0.01;
/// Rate of the tape delay's wow, Hz.
const WOW_HZ: f64 = 0.6;
/// Bias of the tape saturation: the curve is asymmetric (even harmonics).
const TAPE_BIAS: f64 = 0.25;
/// Longest delay, seconds (the `dlyTime*` parameters stop at 2).
const DELAY_SECONDS: f64 = 2.1;
/// The JS delay line always holds 2.1 s at 48 kHz, whatever the rate.
const DELAY_MIN_RATE: f64 = 48_000.0;
/// Level below which states and outputs are flushed to zero.
const FLUSH: f64 = 1e-15;

audio_params! {
    /// The strip processor's parameters (its `parameterDescriptors`).
    pub struct StripSettings {
        /// 0 skips the filter (the `allpass` filter type), 1 filters.
        filter_on = 1.0, 0.0, 1.0;
        /// Filter cutoff, Hz.
        cutoff = crate::MAX_FILTER_FREQ as f32, crate::MIN_FILTER_FREQ as f32, crate::MAX_FILTER_FREQ as f32;
        q = 0.707, 0.707, 18.707;
        /// 0 lowpass, 1 highpass, 2 bandpass, 3 notch.
        filter_mode = 0.0, 0.0, 3.0;
        /// dB per octave: 12 (one stage) or 24 (two stages).
        filter_slope = 12.0, 12.0, 24.0;
        /// 0 tanh, 1 hard clip, 2 tape (an asymmetric tanh).
        saturation_type = 0.0, 0.0, 2.0;
        saturation_drive = 1.0, 1.0, 7.0;
        saturation_output = 1.0, 0.0, 1.0;
        saturation_mix = 0.0, 0.0, 1.0;
        reverb_room = 0.5, 0.0, 1.0;
        reverb_damping = 0.5, 0.0, 1.0;
        reverb_width = 1.0, 0.0, 1.0;
        reverb_mix = 0.0, 0.0, 1.0;
        /// Seconds before the reverb's input enters it.
        reverb_predelay = 0.0, 0.0, 0.1;
        /// Spread of a spring (allpass coefficient), 0 none.
        reverb_dispersion = 0.0, 0.0, 0.9;
        /// Seconds the reverb stays open after its input falls under the
        /// gate's key, then it closes (gated reverb); 0 no gate.
        reverb_gate = 0.0, 0.0, 1.0;
        /// Delay times, seconds.
        delay_time_l = 0.25, 0.0, 2.0;
        delay_time_r = 0.25, 0.0, 2.0;
        delay_feedback = 0.4, 0.0, 0.99;
        delay_mix = 0.0, 0.0, 1.0;
        /// 0 and 1 each side repeats itself, 2 the sides feed each other
        /// (ping-pong).
        delay_mode = 1.0, 0.0, 2.0;
        /// Coefficient of the one-pole lowpass in the feedback: lower is darker.
        delay_tone = 0.5, 0.05, 1.0;
        /// Depth of the tape wow, seconds of delay time.
        delay_wow = 0.0, 0.0, 0.005;
        /// 1 feeds the mono sum into the left line only (a ping-pong starts
        /// on the left), 0 each side into its line.
        delay_mono_in = 0.0, 0.0, 1.0;
        volume = 1.0, 0.0, 2.0;
        pan = 0.0, -1.0, 1.0;
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
        let position = crate::wrap(self.index as f64 - delay, len);
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

/// First-order allpass: `y = -a x + x[n-1] + a y[n-1]`.
#[derive(Debug, Clone, Copy, Default)]
struct Dispersion {
    x1: f64,
    y1: f64,
}

impl Dispersion {
    #[inline]
    fn process(&mut self, x: f64, a: f64) -> f64 {
        let mut y = -a * x + self.x1 + a * self.y1;
        flush(&mut y);
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// TPT state-variable filter state (one channel, one stage).
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    z1: f64,
    z2: f64,
}

/// Coefficients of one [`Svf`] stage at a cutoff and damping.
#[derive(Debug, Clone, Copy)]
struct SvfCoeffs {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
}

impl SvfCoeffs {
    /// `g` is `tan(pi * cutoff / sample_rate)`, `k` is `1 / q`.
    fn new(g: f64, k: f64) -> Self {
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        Self { k, a1, a2, a3: g * a2 }
    }
}

impl Svf {
    /// Filter `x` and return the output of `mode` (as `filter_mode`).
    #[inline]
    fn process(&mut self, x: f64, c: &SvfCoeffs, mode: f64) -> f64 {
        let v3 = x - self.z2;
        let v1 = c.a1 * self.z1 + c.a2 * v3;
        let v2 = self.z2 + c.a2 * self.z1 + c.a3 * v3;
        self.z1 = 2.0 * v1 - self.z1;
        self.z2 = 2.0 * v2 - self.z2;
        flush(&mut self.z1);
        flush(&mut self.z2);
        if mode < 0.5 {
            v2
        } else if mode < 1.5 {
            v3 - v1 * c.k
        } else if mode < 2.5 {
            v1
        } else {
            v3 - v1 * c.k + v2
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
        // Biased and re-centred (0 stays 0), unity slope at 0.
        let bias = TAPE_BIAS.tanh();
        ((d + TAPE_BIAS).tanh() - bias) / (1.0 - bias * bias)
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
    /// Filter stages: [left, right] of the first, then of the second
    /// (24 dB/oct only).
    filter: [Svf; 2],
    filter_2: [Svf; 2],
    combs_l: [Comb; 8],
    combs_r: [Comb; 8],
    allpass_l: [Allpass; 4],
    allpass_r: [Allpass; 4],
    predelay: DelayLine,
    dispersion: [Dispersion; DISPERSION_STAGES],
    /// Frames the gate stays open, and its gain.
    gate_hold: f64,
    gate_gain: f64,
    delay_l: DelayLine,
    delay_r: DelayLine,
    delay_filter_l: f64,
    delay_filter_r: f64,
    /// Phase of the wow, cycles.
    wow_phase: f64,
}

impl Strip {
    /// A silent strip at `sample_rate` (allocates its reverb and delay memories).
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = f64::from(sample_rate);
        let delay_len = (DELAY_SECONDS * sample_rate.max(DELAY_MIN_RATE)).ceil() as usize;
        // The same reverb times at any rate.
        let tuned = |len: usize| ((len as f64 * sample_rate / TUNING_RATE).round() as usize).max(1);
        Self {
            sample_rate,
            filter: [Svf::default(); 2],
            filter_2: [Svf::default(); 2],
            combs_l: COMB_TUNINGS_L.map(|len| Comb::new(tuned(len))),
            combs_r: COMB_TUNINGS_R.map(|len| Comb::new(tuned(len))),
            allpass_l: ALLPASS_TUNINGS_L.map(|len| Allpass::new(tuned(len))),
            allpass_r: ALLPASS_TUNINGS_R.map(|len| Allpass::new(tuned(len))),
            predelay: DelayLine::new((PREDELAY_SECONDS * sample_rate).ceil() as usize + 2),
            dispersion: [Dispersion::default(); DISPERSION_STAGES],
            gate_hold: 0.0,
            gate_gain: 0.0,
            delay_l: DelayLine::new(delay_len),
            delay_r: DelayLine::new(delay_len),
            delay_filter_l: 0.0,
            delay_filter_r: 0.0,
            wow_phase: 0.0,
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
        let (delay_tone, delay_wow) = (f64::from(s.delay_tone), f64::from(s.delay_wow) * sr);
        let delay_mono_in = s.delay_mono_in > 0.5;
        let vol = f64::from(s.volume);
        let filter_on = s.filter_on > 0.5;
        let predelay = f64::from(s.reverb_predelay) * sr;
        let dispersion = f64::from(s.reverb_dispersion);
        let gate_frames = f64::from(s.reverb_gate) * sr;
        let gate_open = 1.0 - (-1.0 / (GATE_OPEN_SECONDS * sr)).exp();
        let gate_close = 1.0 - (-1.0 / (GATE_CLOSE_SECONDS * sr)).exp();

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
        let (stage_1, stage_2, gain_2) = if slope::is_steep(f64::from(s.filter_slope)) {
            let (k1, k2, gain_2) = slope::stages(1.0 / q, mode);
            (SvfCoeffs::new(g, k1), Some(SvfCoeffs::new(g, k2)), gain_2)
        } else {
            (SvfCoeffs::new(g, 1.0 / q), None, 1.0)
        };
        if stage_2.is_none() {
            // A later switch to 24 dB/oct starts the second stage from silence.
            self.filter_2 = [Svf::default(); 2];
        }

        let frames = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        for i in 0..frames {
            // 1. Filter.
            let (mut dry_l, mut dry_r) = (f64::from(in_l[i]), f64::from(in_r[i]));
            if filter_on {
                dry_l = self.filter[0].process(dry_l, &stage_1, mode);
                dry_r = self.filter[1].process(dry_r, &stage_1, mode);
            }
            if let (true, Some(stage_2)) = (filter_on, &stage_2) {
                dry_l = self.filter_2[0].process(dry_l, stage_2, mode) * gain_2;
                dry_r = self.filter_2[1].process(dry_r, stage_2, mode) * gain_2;
            }

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
                let mut input = (sat_l + sat_r) * 0.0075;
                if predelay > 0.0 {
                    self.predelay.write(input);
                    input = self.predelay.read(predelay + 1.0);
                }
                if dispersion > 0.0 {
                    for stage in &mut self.dispersion {
                        input = stage.process(input, dispersion);
                    }
                }
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
                if gate_frames > 0.0 {
                    // Keyed by the dry input: open on a hit, closed once it
                    // has stayed under the key for `reverb_gate`.
                    if sat_l.abs() + sat_r.abs() > GATE_KEY {
                        self.gate_hold = gate_frames;
                    } else if self.gate_hold > 0.0 {
                        self.gate_hold -= 1.0;
                    }
                    let (target, speed) = if self.gate_hold > 0.0 { (1.0, gate_open) } else { (0.0, gate_close) };
                    self.gate_gain += speed * (target - self.gate_gain);
                    flush(&mut self.gate_gain);
                    rev_l *= self.gate_gain;
                    rev_r *= self.gate_gain;
                }
            }

            // 4. Delay.
            let (mut wet_l, mut wet_r) = (0.0, 0.0);
            let (send_l, send_r) = if delay_mono_in { ((sat_l + sat_r) * 0.5, 0.0) } else { (sat_l, sat_r) };
            if has_delay {
                let wow = if delay_wow > 0.0 {
                    self.wow_phase = (self.wow_phase + WOW_HZ / sr).fract();
                    delay_wow * (TAU * self.wow_phase).sin()
                } else {
                    0.0
                };
                let delayed_l = self.delay_l.read(delay_time_l * sr + wow);
                let delayed_r = self.delay_r.read(delay_time_r * sr + wow);
                self.delay_filter_l += delay_tone * (delayed_l - self.delay_filter_l);
                self.delay_filter_r += delay_tone * (delayed_r - self.delay_filter_r);
                let fb_l = shape(self.delay_filter_l * delay_feedback, 1.5, 0.0, 0.5, 1.0);
                let fb_r = shape(self.delay_filter_r * delay_feedback, 1.5, 0.0, 0.5, 1.0);
                if ping_pong {
                    self.delay_l.write(send_l + fb_r);
                    self.delay_r.write(send_r + fb_l);
                } else {
                    self.delay_l.write(send_l + fb_l);
                    self.delay_r.write(send_r + fb_r);
                }
                wet_l = delayed_l * delay_mix;
                wet_r = delayed_r * delay_mix;
            } else {
                self.delay_l.write(send_l);
                self.delay_r.write(send_r);
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

    /// Gain in dB of the strip on a sine of `freq` Hz, measured once settled.
    fn gain_db(settings: &StripSettings, freq: f64) -> f64 {
        let input: Vec<f32> = (0..24_000).map(|i| (TAU * freq * f64::from(i) / 48_000.0).sin() as f32).collect();
        let (l, _) = run(settings, &input);
        let energy = |s: &[f32]| s[12_000..].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        10.0 * (energy(&l) / energy(&input)).log10()
    }

    #[test]
    fn slopes_of_12_and_24_db() {
        for (mode, (near, far)) in [(0.0, (2_000.0, 4_000.0)), (1.0, (500.0, 250.0))] {
            let one = StripSettings { cutoff: 1_000.0, filter_mode: mode, ..StripSettings::default() };
            let two = StripSettings { filter_slope: 24.0, ..one };
            for (settings, slope) in [(one, 12.0), (two, 24.0)] {
                assert!((gain_db(&settings, 1_000.0) + 3.0).abs() < 0.3, "mode {mode}: -3 dB at the cutoff");
                let octave = gain_db(&settings, near) - gain_db(&settings, far);
                assert!((octave - slope).abs() < 1.5, "mode {mode}, {slope} dB/oct: {octave}");
            }
        }
    }

    #[test]
    fn resonance_peaks_as_high_at_24_db() {
        let one = StripSettings { cutoff: 1_000.0, q: 4.0, ..StripSettings::default() };
        let two = StripSettings { filter_slope: 24.0, ..one };
        assert!((gain_db(&one, 1_000.0) - gain_db(&two, 1_000.0)).abs() < 0.5);
        let bandpass = StripSettings { filter_mode: 2.0, ..two };
        assert!((gain_db(&bandpass, 1_000.0) - gain_db(&one, 1_000.0)).abs() < 0.5);
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

    /// Frame of the loudest sample of `s` from `from` on.
    fn loudest(s: &[f32], from: usize) -> usize {
        s[from..].iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0 + from
    }

    #[test]
    fn a_bypassed_filter_leaves_the_signal_as_it_is() {
        let input: Vec<f32> = (0..512).map(|i| ((i * 7919) % 200) as f32 / 100.0 - 1.0).collect();
        let settings = StripSettings { filter_on: 0.0, cutoff: 200.0, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        assert_eq!(l, input);
    }

    #[test]
    fn tape_saturation_keeps_silence_and_bends_one_side_more() {
        assert_eq!(shape(0.0, 4.0, 2.0, 1.0, 1.0), 0.0);
        let (up, down) = (shape(0.5, 4.0, 2.0, 1.0, 1.0), shape(-0.5, 4.0, 2.0, 1.0, 1.0));
        assert!(up > 0.0 && down < 0.0 && (up + down).abs() > 0.05, "{up} {down}");
    }

    #[test]
    fn reverb_times_follow_the_sample_rate() {
        let at = |rate: f32| Strip::new(rate).combs_l[0].buf.len();
        assert_eq!(at(44_100.0), COMB_TUNINGS_L[0]);
        assert_eq!(at(88_200.0), 2 * COMB_TUNINGS_L[0]);
    }

    #[test]
    fn predelay_holds_the_reverb_back() {
        let mut input = vec![0.0; 4000];
        input[0] = 1.0;
        let reverb = StripSettings { filter_on: 0.0, reverb_mix: 1.0, ..StripSettings::default() };
        let first =
            |settings: &StripSettings| run(settings, &input).0[1..].iter().position(|s| s.abs() > 1e-6).unwrap();
        let late = first(&StripSettings { reverb_predelay: 0.02, ..reverb });
        assert_eq!(late - first(&reverb), 960);
    }

    #[test]
    fn dispersion_spreads_a_click_and_keeps_its_energy() {
        let mut chain = [Dispersion::default(); DISPERSION_STAGES];
        let response: Vec<f64> = (0..4000)
            .map(|i| chain.iter_mut().fold(if i == 0 { 1.0 } else { 0.0 }, |x, stage| stage.process(x, 0.6)))
            .collect();
        let energy: f64 = response.iter().map(|x| x * x).sum();
        assert!((energy - 1.0).abs() < 1e-9, "{energy}");
        assert!(response[0].abs() < 0.1, "the click is spread, not passed");
    }

    #[test]
    fn a_gated_reverb_stops_after_its_hold() {
        let mut input = vec![0.0; 48_000];
        input[..200].fill(0.5);
        let reverb = StripSettings { reverb_room: 0.9, reverb_mix: 1.0, ..StripSettings::default() };
        let tail =
            |settings: &StripSettings| run(settings, &input).0[24_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let gated = StripSettings { reverb_gate: 0.25, ..reverb };
        assert!(tail(&reverb) > 1e-3);
        assert!(tail(&gated) < 1e-6, "{}", tail(&gated));
    }

    #[test]
    fn a_ping_pong_starts_left_then_alternates() {
        let mut input = vec![0.0; 3000];
        input[0] = 1.0;
        let settings = StripSettings {
            delay_time_l: 0.01,
            delay_time_r: 0.01,
            delay_mode: 2.0,
            delay_mono_in: 1.0,
            delay_feedback: 0.9,
            delay_mix: 1.0,
            ..StripSettings::default()
        };
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(&settings, &input, &input, &mut l, &mut r);
        // The dry hit is on both sides; then 480 left, 960 right, 1440 left.
        let echo = |s: &[f32], at: usize| s[at - 5..at + 5].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(echo(&l, 480) > 0.1 && echo(&r, 480) < 1e-6);
        assert!(echo(&r, 960) > 0.01 && echo(&l, 960) < 1e-6);
        assert!(echo(&l, 1440) > 0.001 && echo(&r, 1440) < 1e-6);
    }

    #[test]
    fn delay_repeats_after_its_time() {
        let mut input = vec![0.0; 2000];
        input[0] = 1.0;
        let settings =
            StripSettings { delay_time_l: 0.01, delay_time_r: 0.01, delay_mix: 1.0, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        let echo = loudest(&l[..600], 400);
        assert!((479..=482).contains(&echo), "echo at {echo}");
    }
}
