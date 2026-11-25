//! The stages of a strip's sample loop, one sample at a time.

use std::f64::consts::TAU;

use super::blocks::{ClipBlock, CrushBlock, DelayBlock, FilterBlock, PanBlock, ReverbBlock, SaturationBlock};
use super::curves::{Saturation, clip_adaa, shape};
use super::{CLIP_CEILING, GATE_KEY, Strip, WOW_HZ, flush};
use crate::svf::output;

impl Strip {
    /// 1. The filter: one stage, or two in series.
    #[inline]
    pub(super) fn filter_stage(&mut self, f: &FilterBlock, mut l: f64, mut r: f64) -> (f64, f64) {
        if f.on {
            l = output(self.filter[0].step(l, &f.stage_1), f.mode);
            r = output(self.filter[1].step(r, &f.stage_1), f.mode);
        }
        if let (true, Some(stage_2)) = (f.on, &f.stage_2) {
            l = output(self.filter_2[0].step(l, stage_2), f.mode) * f.gain_2;
            r = output(self.filter_2[1].step(r, stage_2), f.mode) * f.gain_2;
        }
        (l, r)
    }

    /// 3b. The bitcrusher: a sample and hold, then fewer levels.
    #[inline]
    pub(super) fn crush_stage(&mut self, c: &CrushBlock, l: f64, r: f64) -> (f64, f64) {
        if self.crush_phase >= 1.0 {
            self.crush_phase -= 1.0;
            self.crush_hold = [(l * c.levels).round() / c.levels, (r * c.levels).round() / c.levels];
        }
        self.crush_phase += c.step;
        (self.crush_hold[0], self.crush_hold[1])
    }

    /// 3c. The hard clipper, last of the shaping: its ceiling holds.
    #[inline]
    pub(super) fn clip_stage(&mut self, c: &ClipBlock, l: f64, r: f64) -> (f64, f64) {
        let (u_l, u_r) = (l * c.gain, r * c.gain);
        let out = (clip_adaa(u_l, self.clip_prev[0]) * CLIP_CEILING, clip_adaa(u_r, self.clip_prev[1]) * CLIP_CEILING);
        self.clip_prev = [u_l, u_r];
        out
    }

    /// 4. The reverb's wet signal for the saturated input `l`, `r`.
    #[inline]
    pub(super) fn reverb_stage(&mut self, v: &ReverbBlock, l: f64, r: f64) -> (f64, f64) {
        let mut input = (l + r) * 0.0075;
        if v.predelay_frames > 0.0 {
            self.predelay.write(input);
            input = self.predelay.read(v.predelay_frames + 1.0);
        }
        if v.dispersion > 0.0 {
            for stage in &mut self.dispersion {
                input = stage.process(input, v.dispersion);
            }
        }
        let (mut comb_l, mut comb_r) = (0.0, 0.0);
        for (left, right) in self.combs_l.iter_mut().zip(&mut self.combs_r) {
            comb_l += left.process(input, v.damp1, v.damp2, v.room);
            comb_r += right.process(input, v.damp1, v.damp2, v.room);
        }
        for (left, right) in self.allpass_l.iter_mut().zip(&mut self.allpass_r) {
            comb_l = left.process(comb_l);
            comb_r = right.process(comb_r);
        }
        let mut wet_l = (comb_l * v.width1 + comb_r * v.width2) * v.mix;
        let mut wet_r = (comb_r * v.width1 + comb_l * v.width2) * v.mix;
        if v.gate_frames > 0.0 {
            // Keyed by the dry input: open on a hit, closed once it has
            // stayed under the key for `reverb_gate`.
            if l.abs() + r.abs() > GATE_KEY {
                self.gate_hold = v.gate_frames;
            } else if self.gate_hold > 0.0 {
                self.gate_hold -= 1.0;
            }
            let (target, speed) = if self.gate_hold > 0.0 { (1.0, v.gate_open) } else { (0.0, v.gate_close) };
            self.gate_gain += speed * (target - self.gate_gain);
            flush(&mut self.gate_gain);
            wet_l *= self.gate_gain;
            wet_r *= self.gate_gain;
        }
        (wet_l, wet_r)
    }

    /// 5. The delay's wet signal; the input `l`, `r` goes into its lines.
    #[inline]
    pub(super) fn delay_stage(&mut self, d: &DelayBlock, l: f64, r: f64) -> (f64, f64) {
        let (send_l, send_r) = if d.mono_in { ((l + r) * 0.5, 0.0) } else { (l, r) };
        if !d.on {
            self.delay_l.write(send_l);
            self.delay_r.write(send_r);
            return (0.0, 0.0);
        }
        let wow = if d.wow_frames > 0.0 {
            self.wow_phase = (self.wow_phase + WOW_HZ / d.sample_rate).fract();
            d.wow_frames * (TAU * self.wow_phase).sin()
        } else {
            0.0
        };
        let delayed_l = self.delay_l.read(d.time_l_frames + wow);
        let delayed_r = self.delay_r.read(d.time_r_frames + wow);
        self.delay_filter_l += d.tone * (delayed_l - self.delay_filter_l);
        self.delay_filter_r += d.tone * (delayed_r - self.delay_filter_r);
        flush(&mut self.delay_filter_l);
        flush(&mut self.delay_filter_r);
        let fb_l = shape(self.delay_filter_l * d.feedback, 1.5, Saturation::Tanh, 0.5, 1.0);
        let fb_r = shape(self.delay_filter_r * d.feedback, 1.5, Saturation::Tanh, 0.5, 1.0);
        if d.ping_pong {
            self.delay_l.write(send_l + fb_r);
            self.delay_r.write(send_r + fb_l);
        } else {
            self.delay_l.write(send_l + fb_l);
            self.delay_r.write(send_r + fb_r);
        }
        (delayed_l * d.mix, delayed_r * d.mix)
    }
}

/// 3. The saturation of one sample.
#[inline]
pub(super) fn saturate(sat: Option<&SaturationBlock>, x: f64) -> f64 {
    sat.map_or(x, |s| shape(x, s.drive, s.curve, s.mix, s.output))
}

/// The mix of the dry, reverb and delay signals, with volume and pan, kept
/// out of denormals and clamped.
#[inline]
pub(super) fn mix_out(pan: &PanBlock, volume: f64, dry: (f64, f64), rev: (f64, f64), wet: (f64, f64)) -> (f32, f32) {
    let mut left = (dry.0 + rev.0 + wet.0) * volume * pan.left * pan.compensation;
    let mut right = (dry.1 + rev.1 + wet.1) * volume * pan.right * pan.compensation;
    flush(&mut left);
    flush(&mut right);
    (left.clamp(-2.0, 2.0) as f32, right.clamp(-2.0, 2.0) as f32)
}
