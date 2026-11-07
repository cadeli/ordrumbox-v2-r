//! What a hit sounds like once its track is applied
//! (`src/patterns/note_params.js`): stereo position, playback ratio, swing.

use crate::pattern_engine::Hit;
use crate::steps::semitone_ratio;
use crate::track::{STRAIGHT_SWING, Track};

impl Hit {
    /// Stereo position: the mean of the note and track pans, floored to 0.01
    ///.
    pub fn playback_pan(&self, track: &Track) -> f64 {
        ((self.pan + track.pan) / 2.0 * 100.0).floor() / 100.0
    }

    /// Playback-rate ratio of the note pitch plus the track pitch.
    ///
    /// Exact on purpose: `computePitch` floors the ratio to 0.01, which plays
    /// every interval flat (+1 semitone: 1.0595 → 1.05, −15.5 cents; +7:
    /// 1.4983 → 1.49, −9.8 cents; octaves stay exact).
    pub fn playback_ratio(&self, track: &Track) -> f64 {
        semitone_ratio(self.pitch + track.transpose)
    }

    /// Swing delay, in beats: with a swing grid of
    /// `swing_resolution` steps (1 = straight), the note on the second step of
    /// each group is pushed late by `swing_amount` beats (0.33 is a triplet feel).
    pub fn swing_beats(&self, track: &Track) -> f64 {
        let grid = track.swing_resolution.max(1);
        if grid == 1 || self.step % grid != 1 { 0.0 } else { track.swing_amount }
    }

    /// Delay of a swing set for every track (a plugin host's), in beats:
    /// in each pair of steps, the second one starts at `ratio` of the pair
    /// ([`STRAIGHT_SWING`] is straight, 2/3 a triplet feel). A track with an
    /// odd number of steps per beat has pairs across its beats: it is not swung.
    pub fn pair_swing_beats(&self, track: &Track, ratio: f64) -> f64 {
        if track.steps_per_beat % 2 != 0 || self.step % 2 != 1 {
            return 0.0;
        }
        (ratio - STRAIGHT_SWING).max(0.0) * 2.0 / f64::from(track.steps_per_beat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(beat_step: i32, pitch: f64, pan: f64) -> Hit {
        Hit { track: 0, source: Some(0), beat: 0, step: beat_step, pitch, velocity: 0.8, pan }
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
    fn swing_pushes_the_second_step_of_each_group() {
        let swung = Track { swing_resolution: 2, swing_amount: 0.33, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).swing_beats(&swung), 0.33);
        assert_eq!(note(3, 0.0, 0.0).swing_beats(&swung), 0.33);
        assert_eq!(note(2, 0.0, 0.0).swing_beats(&swung), 0.0);
        let straight = Track { swing_amount: 0.5, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).swing_beats(&straight), 0.0);
    }

    #[test]
    fn pair_swing_pushes_the_second_step_of_each_pair() {
        let sixteenths = Track::default();
        assert_eq!(note(1, 0.0, 0.0).pair_swing_beats(&sixteenths, STRAIGHT_SWING), 0.0);
        // A triplet feel: the second sixteenth of a pair on its last third.
        let triplet = note(3, 0.0, 0.0).pair_swing_beats(&sixteenths, 2.0 / 3.0);
        assert!((triplet - 1.0 / 12.0).abs() < 1e-12);
        assert_eq!(note(2, 0.0, 0.0).pair_swing_beats(&sixteenths, 0.75), 0.0);
        let triplets = Track { steps_per_beat: 3, ..Track::default() };
        assert_eq!(note(1, 0.0, 0.0).pair_swing_beats(&triplets, 0.75), 0.0);
    }
}
