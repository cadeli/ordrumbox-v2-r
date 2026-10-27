//! A track's channel strip (`worklets/processors/strip_source.js`).
//!
//! Phase 3 keeps only its last stage: volume and compensated pan. Filter,
//! saturation, reverb and delay come with the DSP port (phase 4).

use std::f32::consts::FRAC_PI_4;

/// Settings of one strip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripParams {
    /// Track volume (`track.velocity`), 0–2.
    pub volume: f32,
    /// Track pan, -1–1.
    pub pan: f32,
    /// Muted, or silenced by another track's solo.
    pub silent: bool,
}

impl Default for StripParams {
    fn default() -> Self {
        Self { volume: 1.0, pan: 0.0, silent: false }
    }
}

impl StripParams {
    /// Left/right gains: equal-power pan scaled so the louder side is at unity
    /// (centre stays at 1/1), times the volume.
    pub fn gains(&self) -> (f32, f32) {
        if self.silent {
            return (0.0, 0.0);
        }
        let p = self.pan.clamp(-1.0, 1.0);
        let (left, right) = (((p + 1.0) * FRAC_PI_4).cos(), ((p + 1.0) * FRAC_PI_4).sin());
        let max = left.max(right);
        let compensation = if max > 0.001 { 1.0 / max } else { 1.0 };
        (left * compensation * self.volume, right * compensation * self.volume)
    }

    /// Mix a strip's buffers into the output buffers.
    pub fn mix_into(&self, left: &[f32], right: &[f32], out_left: &mut [f32], out_right: &mut [f32]) {
        let (gain_l, gain_r) = self.gains();
        if gain_l == 0.0 && gain_r == 0.0 {
            return;
        }
        for (out, input) in out_left.iter_mut().zip(left) {
            *out += input * gain_l;
        }
        for (out, input) in out_right.iter_mut().zip(right) {
            *out += input * gain_r;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_is_unity_and_hard_pan_keeps_one_side() {
        let (l, r) = StripParams::default().gains();
        assert!((l - 1.0).abs() < 1e-6 && (r - 1.0).abs() < 1e-6);
        let (l, r) = StripParams { pan: 1.0, volume: 0.5, silent: false }.gains();
        assert!(l.abs() < 1e-6 && (r - 0.5).abs() < 1e-6);
        assert_eq!(StripParams { silent: true, ..StripParams::default() }.gains(), (0.0, 0.0));
    }
}
