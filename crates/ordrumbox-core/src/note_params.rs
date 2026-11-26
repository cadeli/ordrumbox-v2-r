//! What a hit sounds like once its track is applied: stereo position, playback ratio, groove.

use crate::pattern_engine::Hit;
use crate::steps::semitone_ratio;
use crate::track::Track;

impl Hit {
    /// Stereo position: the mean of the note and track pans, floored to 0.01.
    pub fn playback_pan(&self, track: &Track) -> f64 {
        ((self.pan + track.pan) / 2.0 * 100.0).floor() / 100.0
    }

    /// Playback-rate ratio of the note pitch plus the track pitch.
    ///
    /// Exact on purpose: a ratio floored to 0.01 would play
    /// every interval flat (+1 semitone: 1.0595 → 1.05, −15.5 cents; +7:
    /// 1.4983 → 1.49, −9.8 cents; octaves stay exact).
    pub fn playback_ratio(&self, track: &Track) -> f64 {
        semitone_ratio(self.pitch + track.transpose)
    }

    /// How late the track's groove plays the hit, in beats, at the track's amount.
    #[must_use]
    pub fn groove_beats(&self, track: &Track) -> f64 {
        track.groove.timing_beats(self.step, track.groove_amount)
    }

    /// The hit with the velocity the track's groove gives its step at `amount`, kept within 0–1.
    #[must_use]
    pub fn grooved(&self, track: &Track, amount: f64) -> Self {
        let delta = track.groove.velocity_delta(self.step, amount);
        Self { velocity: (self.velocity + delta).clamp(0.0, 1.0), ..self.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::groove::Groove;

    fn note(beat_step: i32, pitch: f64, pan: f64) -> Hit {
        Hit { track: 0, source: Some(0), beat: 0, step: beat_step, pitch, velocity: 0.8, pan, length_steps: 1.0 }
    }

    #[test]
    fn pan_is_the_floored_mean() {
        let track = Track { pan: 0.3, ..Track::default() };
        assert_eq!(note(0, 0.0, 0.0).playback_pan(&track), 0.15);
        assert_eq!(note(0, 0.0, -0.33).playback_pan(&track), -0.02); // floor of -0.015
    }

    #[test]
    fn ratio_is_exact() {
        let track = Track { transpose: -12.0, ..Track::default() };
        assert_eq!(note(0, 0.0, 0.0).playback_ratio(&track), 0.5);
        let ratio = note(0, 13.0, 0.0).playback_ratio(&track);
        assert!((ratio - 2f64.powf(1.0 / 12.0)).abs() < 1e-12);
    }

    #[test]
    fn the_groove_pushes_and_accents_its_steps() {
        let swung = Track { groove: Groove::swing(2), groove_amount: 0.33, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).groove_beats(&swung), 0.33);
        assert_eq!(note(3, 0.0, 0.0).groove_beats(&swung), 0.33);
        assert_eq!(note(2, 0.0, 0.0).groove_beats(&swung), 0.0);
        let straight = Track { groove_amount: 0.5, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).groove_beats(&straight), 0.0);
        let accent = Track { groove: Groove::preset("Accent").unwrap(), ..Track::default() };
        assert_eq!(note(0, 0.0, 0.0).grooved(&accent, 1.0).velocity, 1.0, "0.8 + 0.5, kept to 1");
        assert!((note(1, 0.0, 0.0).grooved(&accent, 0.5).velocity - 0.65).abs() < 1e-12);
        assert_eq!(note(1, 0.0, 0.0).grooved(&accent, 0.0).velocity, 0.8);
    }
}
