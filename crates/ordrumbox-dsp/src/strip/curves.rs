//! The waveshaping curves of a strip: the saturation's, and the hard
//! clipper's anti-aliased clip.

/// Bias of the tape saturation: the curve is asymmetric (even harmonics).
const TAPE_BIAS: f64 = 0.25;
/// Below this step between two samples, the clipper's anti-aliasing takes
/// the clip of their midpoint (the divided difference would lose precision).
const CLIP_ADAA_EPSILON: f64 = 1e-6;

/// The curve of the saturation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Saturation {
    Tanh,
    HardClip,
    /// An asymmetric tanh.
    Tape,
}

impl Saturation {
    /// The curve a `saturation_type` setting stands for: the nearest code,
    /// the tape above the last.
    pub(super) fn from_setting(setting: f64) -> Self {
        if setting < 0.5 {
            Self::Tanh
        } else if setting < 1.5 {
            Self::HardClip
        } else {
            Self::Tape
        }
    }
}

/// `#shape`: drive, waveshape, dry/wet, output gain.
///
/// The `match` runs for every sample: the curve of a block never changes, so
/// the branch is always the same one, and a loop per curve would triple the
/// strip's loop.
#[inline]
pub(super) fn shape(x: f64, drive: f64, curve: Saturation, mix: f64, out: f64) -> f64 {
    if mix <= 0.0 {
        return x;
    }
    let d = x * drive;
    let s = match curve {
        Saturation::Tanh => d.tanh(),
        Saturation::HardClip => d.clamp(-1.0, 1.0),
        Saturation::Tape => {
            // Biased and re-centred (0 stays 0), unity slope at 0.
            let bias = TAPE_BIAS.tanh();
            ((d + TAPE_BIAS).tanh() - bias) / (1.0 - bias * bias)
        }
    };
    (x * (1.0 - mix) + s * mix) * out
}

/// Antiderivative of the hard clip at ±1.
#[inline]
pub(super) fn clip_antiderivative(x: f64) -> f64 {
    if x.abs() <= 1.0 { 0.5 * x * x } else { x.abs() - 0.5 }
}

/// Hard clip at ±1 with first-order antiderivative anti-aliasing (ADAA):
/// the clip's mean over the segment from `prev` to `x`, which cuts the
/// aliasing of a plain clip, at the cost of half a sample of delay.
#[inline]
pub(super) fn clip_adaa(x: f64, prev: f64) -> f64 {
    let dx = x - prev;
    if dx.abs() < CLIP_ADAA_EPSILON {
        (0.5 * (x + prev)).clamp(-1.0, 1.0)
    } else {
        (clip_antiderivative(x) - clip_antiderivative(prev)) / dx
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tape_saturation_keeps_silence_and_bends_one_side_more() {
        assert_eq!(shape(0.0, 4.0, Saturation::Tape, 1.0, 1.0), 0.0);
        let (up, down) = (shape(0.5, 4.0, Saturation::Tape, 1.0, 1.0), shape(-0.5, 4.0, Saturation::Tape, 1.0, 1.0));
        assert!(up > 0.0 && down < 0.0 && (up + down).abs() > 0.05, "{up} {down}");
    }
}
