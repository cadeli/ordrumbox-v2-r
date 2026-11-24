//! The pitch envelope of a note: how much of its offset is left, sample by
//! sample, as it falls back to the note's pitch.

use super::{cents_ratio, clamp};

/// The share of the offset left once the decay time has passed.
const LEFT_AFTER_DECAY: f64 = 0.01;
/// The shortest decay, seconds: a shorter one would be a click, not a sweep.
const MIN_DECAY_SECONDS: f64 = 0.001;
/// Under this share, what is left of the offset is dropped, before it reaches denormals.
const FLOOR: f64 = 1e-6;
/// The widest offset, semitones either way: four octaves.
pub const MAX_PITCH_ENV_SEMITONES: f64 = 48.0;

/// What one note's pitch envelope has left of its offset: 1 at the start, then less.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct PitchFall {
    left: f64,
}

impl PitchFall {
    /// The envelope at the start of a note: the whole offset.
    pub(super) const START: Self = Self { left: 1.0 };

    /// The frequency ratio of this sample for an offset of `semitones`, then
    /// moved on by a sample, each of which keeps `fall` of what is left.
    #[inline]
    pub(super) fn step(&mut self, semitones: f64, fall: f64) -> f64 {
        let cents = clamp(semitones, -MAX_PITCH_ENV_SEMITONES, MAX_PITCH_ENV_SEMITONES) * 100.0 * self.left;
        let next = self.left * fall;
        self.left = if next > FLOOR { next } else { 0.0 };
        cents_ratio(cents)
    }
}

/// The share of the offset each sample keeps, for a decay of `decay_seconds`.
#[must_use]
pub(super) fn fall_per_sample(decay_seconds: f64, sample_rate: f64) -> f64 {
    LEFT_AFTER_DECAY.powf(1.0 / (decay_seconds.max(MIN_DECAY_SECONDS) * sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offset_falls_to_a_hundredth_in_the_decay_time() {
        let fall = fall_per_sample(0.05, 1000.0);
        let mut envelope = PitchFall::START;
        // An octave up at the start.
        assert!((envelope.step(12.0, fall) - 2.0).abs() < 1e-12);
        for _ in 1..50 {
            envelope.step(12.0, fall);
        }
        // 50 samples later: a hundredth of an octave, 12 cents.
        assert!((envelope.step(12.0, fall) - cents_ratio(12.0)).abs() < 1e-9);
    }

    #[test]
    fn what_is_left_ends_at_zero() {
        let fall = fall_per_sample(0.0, 48_000.0);
        let mut envelope = PitchFall::START;
        for _ in 0..1000 {
            envelope.step(-24.0, fall);
        }
        assert_eq!(envelope, PitchFall { left: 0.0 });
        assert_eq!(envelope.step(-24.0, fall), 1.0);
    }

    #[test]
    fn the_offset_stays_within_four_octaves() {
        let mut envelope = PitchFall::START;
        assert!((envelope.step(1e9, 0.5) - 16.0).abs() < 1e-9);
    }
}
