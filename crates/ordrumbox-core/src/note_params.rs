//! What a flat note sounds like once its track is applied
//! (`src/patterns/note_params.js`): stereo position, playback ratio, swing.

use crate::pattern_engine::FlatNote;
use crate::steps::semitone_ratio;
use crate::track::Track;

impl FlatNote {
    /// Stereo position: the mean of the note and track pans, floored to 0.01
    /// (`computePan`).
    pub fn playback_pan(&self, track: &Track) -> f64 {
        ((self.pan + track.pan) / 2.0 * 100.0).floor() / 100.0
    }

    /// Playback-rate ratio of the note pitch plus the track pitch.
    ///
    /// Exact on purpose: `computePitch` floors the ratio to 0.01, which plays
    /// every interval flat (+1 semitone: 1.0595 → 1.05, −15.5 cents; +7:
    /// 1.4983 → 1.49, −9.8 cents; octaves stay exact).
    pub fn playback_ratio(&self, track: &Track) -> f64 {
        semitone_ratio(self.pitch + track.pitch)
    }

    /// Swing delay, in beats (`computeSwingTime`): with a swing grid of
    /// `swing_resolution` steps (1 = straight), the note on the second step of
    /// each group is pushed late by `swing_amount` beats (0.33 is a triplet feel).
    pub fn swing_beats(&self, track: &Track) -> f64 {
        let grid = track.swing_resolution.max(1);
        if grid == 1 || self.beat_step % grid != 1 { 0.0 } else { track.swing_amount }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(beat_step: i32, pitch: f64, pan: f64) -> FlatNote {
        FlatNote { track: 0, source: Some(0), beat: 0, beat_step, pitch, velocity: 0.8, pan }
    }

    #[test]
    fn pan_is_the_floored_mean() {
        let track = Track { pan: 0.3, ..Track::default() };
        assert_eq!(note(0, 0.0, 0.0).playback_pan(&track), 0.15);
        assert_eq!(note(0, 0.0, -0.33).playback_pan(&track), -0.02); // floor of -0.015
    }

    #[test]
    fn ratio_is_exact() {
        let track = Track { pitch: -12.0, ..Track::default() };
        assert_eq!(note(0, 0.0, 0.0).playback_ratio(&track), 0.5);
        let ratio = note(0, 13.0, 0.0).playback_ratio(&track);
        assert!((ratio - 2f64.powf(1.0 / 12.0)).abs() < 1e-12);
    }

    #[test]
    fn swing_pushes_the_second_step_of_each_group() {
        let swung = Track { swing_resolution: 2, swing_amount: 0.33, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).swing_beats(&swung), 0.33);
        assert_eq!(note(3, 0.0, 0.0).swing_beats(&swung), 0.33);
        assert_eq!(note(2, 0.0, 0.0).swing_beats(&swung), 0.0);
        let straight = Track { swing_amount: 0.5, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).swing_beats(&straight), 0.0);
    }
}
