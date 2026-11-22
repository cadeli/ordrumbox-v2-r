//! What a song's automation lanes do to its notes as they start: a
//! transpose lane sets their pitch, a swing lane how late the second step of
//! a pair plays, a probability lane whether they play. A note takes the
//! lanes' values on its step; the track's own where they set nothing.

use std::borrow::Cow;

use ordrumbox_core::automation::{AutomationLane, AutomationTarget, lanes_value_at};
use ordrumbox_core::{Hit, Pattern, Rng, Track};

/// The steps of a swing pair: the grid a swing lane swings a straight track on.
const SWING_PAIR: i32 = 2;

/// `pattern` as a song with `lanes` expands it: a track with a probability
/// lane plays every note its own probability would, the lane then deciding
/// note by note ([`NoteLanes::plays`]); as it is without one.
pub(super) fn lane_pattern<'a>(lanes: &[AutomationLane], pattern: &'a Pattern) -> Cow<'a, Pattern> {
    let has_lane = |track: &Track| lanes.iter().any(|lane| lane.is_for(&track.name, AutomationTarget::Probability));
    if !pattern.tracks.iter().any(has_lane) {
        return Cow::Borrowed(pattern);
    }
    let mut pattern = pattern.clone();
    for track in pattern.tracks.iter_mut().filter(|track| has_lane(track)) {
        track.probability = 1.0;
    }
    Cow::Owned(pattern)
}

/// What the lanes set on a note of a track at a step, in their controls' units.
#[derive(Debug, Clone, Copy)]
pub(super) struct NoteLanes {
    transpose: Option<f64>,
    /// The chance it plays: the lane's, or the track's where it sets
    /// nothing; `None` when the track has no probability lane.
    probability: Option<f64>,
    swing: Option<f64>,
}

impl NoteLanes {
    /// The lanes of `lanes` for `track`, at `step` of the song.
    pub(super) fn at(lanes: &[AutomationLane], track: &Track, step: u32) -> Self {
        let value = |target: AutomationTarget| {
            lanes_value_at(lanes, &track.name, target, step).map(|value| target.to_control(value))
        };
        let has_probability = lanes.iter().any(|lane| lane.is_for(&track.name, AutomationTarget::Probability));
        let own_probability = track.effective_probability();
        Self {
            transpose: value(AutomationTarget::Transpose),
            probability: has_probability.then(|| value(AutomationTarget::Probability).unwrap_or(own_probability)),
            swing: value(AutomationTarget::Swing),
        }
    }

    /// Whether the note plays: drawn from `rng` with a probability lane, sure without one.
    pub(super) fn plays(&self, rng: &mut Rng) -> bool {
        self.probability.is_none_or(|probability| rng.chance(probability))
    }

    /// `note` of `track` with the lanes' transpose in place of the track's.
    pub(super) fn apply(&self, note: &Hit, track: &Track) -> Hit {
        let mut note = note.clone();
        if let Some(transpose) = self.transpose {
            note.pitch += transpose - track.transpose;
        }
        note
    }

    /// How late `note` of `track` swings, in beats: by the lane's amount
    /// (on pairs of steps when the track is straight), else the track's.
    pub(super) fn swing_beats(&self, note: &Hit, track: &Track) -> f64 {
        match self.swing {
            Some(amount) => {
                let resolution = if track.swing_resolution > 1 { track.swing_resolution } else { SWING_PAIR };
                note.swing_beats_with(resolution, amount)
            }
            None => note.swing_beats(track),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::AutomationStroke;

    fn lane(instrument: &str, target: AutomationTarget, from: u32, value: f64) -> AutomationLane {
        let mut lane = AutomationLane::new(instrument, target);
        lane.paint(from, u32::MAX, AutomationStroke::Fill(Some(value)));
        lane
    }

    fn hit(step: i32, pitch: f64) -> Hit {
        Hit { track: 0, source: Some(0), beat: 0, step, pitch, velocity: 0.8, pan: 0.0, length_steps: 1.0 }
    }

    #[test]
    fn a_probability_lane_leaves_the_draw_to_each_note() {
        let track = |name: &str| Track { name: name.into(), probability: 0.3, ..Track::default() };
        let pattern = Pattern { tracks: vec![track("KICK"), track("HAT")], ..Pattern::default() };
        assert!(matches!(lane_pattern(&[], &pattern), Cow::Borrowed(_)), "no lane: as it is");
        let lanes = [lane("HAT", AutomationTarget::Probability, 4, 0.0)];
        let expanded = lane_pattern(&lanes, &pattern);
        assert_eq!([expanded.tracks[0].probability, expanded.tracks[1].probability], [0.3, 1.0]);

        let mut rng = Rng::new(7);
        let hat = &pattern.tracks[1];
        assert!(!NoteLanes::at(&lanes, hat, 4).plays(&mut rng), "the lane's 0");
        let mut before = rng.clone();
        assert!(NoteLanes::at(&lanes, &pattern.tracks[0], 4).plays(&mut rng), "no lane: sure");
        assert_eq!(rng.next_u64(), before.next_u64(), "nothing drawn");
        assert_eq!(NoteLanes::at(&lanes, hat, 0).probability, Some(0.3), "before the lane's first point: the track's");
    }

    #[test]
    fn transpose_and_swing_lanes_set_each_note() {
        let track = Track { name: "BASS".into(), transpose: 5.0, ..Track::default() };
        let lanes =
            [lane("BASS", AutomationTarget::Transpose, 8, 0.75), lane("BASS", AutomationTarget::Swing, 8, 0.25)];
        let before = NoteLanes::at(&lanes, &track, 7);
        assert_eq!(before.apply(&hit(1, 2.0), &track).pitch, 2.0, "the track's transpose kept");
        assert_eq!(before.swing_beats(&hit(1, 0.0), &track), 0.0, "a straight track");
        let after = NoteLanes::at(&lanes, &track, 8);
        assert_eq!(after.apply(&hit(1, 2.0), &track).pitch + track.transpose, 14.0, "+12 in place of +5");
        let swing = [0, 1, 2, 3].map(|step| after.swing_beats(&hit(step, 0.0), &track));
        assert_eq!(swing, [0.0, 0.25, 0.0, 0.25], "a straight track swung in pairs");
        let triplets = Track { swing_resolution: 3, ..track };
        assert_eq!([1, 2, 4].map(|step| after.swing_beats(&hit(step, 0.0), &triplets)), [0.25, 0.0, 0.25]);
    }
}
