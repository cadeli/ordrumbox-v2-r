//! Song automation: lanes that set a control of an instrument along an
//! song, at the resolution of a step (a sixteenth note): switch it on
//! and off (mute, solo) or set its level (volume, pan, filter cutoff and Q,
//! sends, effect amounts; transpose, probability and groove, which act on
//! each note as it starts: [`AutomationTarget`]).
//!
//! Every lane holds values from 0 to 1: a switch is off below one half and
//! on from it, a level is a control's position (a volume's top is 0 dB, a
//! pan's middle is the center, a cutoff's travel is logarithmic). A
//! point sets its value from its step until the next point, or gives the
//! control back to the pattern; a level glides over a point's step to the
//! next point when that point is on the next step and the two are part of a
//! run of three points or more on adjacent steps (a drawn line), so a drawn
//! line is smooth while a level held then changed, or set on one step alone,
//! holds for its step.
//! Before its first point a lane leaves the control as the pattern has it.
//! An instrument is named as its tracks are (KICK, SNARE...), so a lane
//! follows it through every pattern of the song.

mod file;
mod target;

use crate::num::{db_to_gain, floor_u32, gain_to_db};
use crate::{Pattern, Song, Track};

/// Steps of a beat on an automation lane.
pub const STEPS_PER_BEAT: u32 = 4;
/// Steps of a 4/4 measure on an automation lane.
pub const STEPS_PER_MEASURE: u32 = STEPS_PER_BEAT * crate::song::BEATS_PER_MEASURE;
/// Ticks of a step of an automation lane (or of a song loop's end).
pub const STEP_TICKS: u32 = crate::steps::TICK / STEPS_PER_BEAT;
/// The quietest a volume lane's level sets above silence, at its bottom.
const VOLUME_FLOOR_DB: f64 = -60.0;

/// The values a lane keeps apart: finer ones are rounded to it, so that a
/// drawn line does not leave a point on each step for nothing.
const VALUE_RESOLUTION: f64 = 1000.0;

/// The control a lane sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationTarget {
    Mute,
    Solo,
    Volume,
    Pan,
    /// The filter's cutoff.
    Cutoff,
    /// The filter's Q (resonance).
    Q,
    /// The send to the shared echo return.
    EchoSend,
    /// The send to the shared reverb return.
    ReverbSend,
    /// The track's own reverb: its amount.
    ReverbMix,
    /// The track's own delay: its amount.
    DelayMix,
    DelayFeedback,
    /// The saturation's amount.
    Saturation,
    Bitcrush,
    HardClip,
    /// The compressor's dry/wet.
    CompMix,
    /// The track's transpose, in whole semitones: set on each note as it starts.
    Transpose,
    /// The track's probability: whether each note plays.
    Probability,
    /// The track's groove amount: how much of its timing and accents play (a
    /// straight track swings in pairs of steps).
    Groove,
}

/// How a lane's values act on its control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationKind {
    /// Off below one half, on from it.
    Switch,
    /// The control's level, 0 to 1.
    Level,
}

/// `step` as shown: `measure.step`, both from 1.
#[must_use]
pub fn step_label(step: u32) -> String {
    format!("{}.{}", step / STEPS_PER_MEASURE + 1, step % STEPS_PER_MEASURE + 1)
}

/// The decibels a volume lane's `level` sets, as a fader's curve has them:
/// 0 dB at the top, finer near it, [`VOLUME_FLOOR_DB`] just above the
/// bottom; `None` (silent) at the bottom.
#[must_use]
pub fn volume_db(level: f64) -> Option<f64> {
    // Plus zero: the top is 0 dB, not -0 dB.
    (level > 0.0).then(|| VOLUME_FLOOR_DB * (1.0 - level.min(1.0)).powi(2) + 0.0)
}

/// The gain a volume lane's `level` sets ([`volume_db`]).
#[must_use]
pub fn volume_gain(level: f64) -> f64 {
    volume_db(level).map_or(0.0, db_to_gain)
}

/// The level of a volume lane that sets `gain` (the inverse of
/// [`volume_gain`]); 0 for a gain at or under [`VOLUME_FLOOR_DB`].
#[must_use]
pub fn volume_level(gain: f64) -> f64 {
    if gain <= 0.0 {
        return 0.0;
    }
    let db = gain_to_db(gain).clamp(VOLUME_FLOOR_DB, 0.0);
    1.0 - (db / VOLUME_FLOOR_DB).sqrt()
}

/// Whether a switch's `value` is on.
#[must_use]
pub fn is_on(value: f64) -> bool {
    value >= 0.5
}

/// The value a switch has when `is_on`.
#[must_use]
pub fn switch_value(is_on: bool) -> f64 {
    if is_on { 1.0 } else { 0.0 }
}

/// `value` as a lane keeps it: 0 to 1, rounded to [`VALUE_RESOLUTION`].
fn kept(value: f64) -> f64 {
    (value.clamp(0.0, 1.0) * VALUE_RESOLUTION).round() / VALUE_RESOLUTION
}

/// A point of a lane: from `step` on, the control has `value`, or is as the
/// pattern has it (`None`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutomationPoint {
    /// Steps from the start of the song.
    pub step: u32,
    pub value: Option<f64>,
}

/// What a press paints over a zone of a lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AutomationStroke {
    /// One value over the whole zone; `None` erases it (as the pattern has it).
    Fill(Option<f64>),
    /// A line from a step's value to another's, held level past its ends.
    Line((u32, f64), (u32, f64)),
}

impl AutomationStroke {
    /// The value it paints at `step`.
    #[must_use]
    pub fn value_at(self, step: u32) -> Option<f64> {
        match self {
            Self::Fill(value) => value,
            Self::Line((a, from), (b, to)) => {
                let ((a, from), (b, to)) = if a <= b { ((a, from), (b, to)) } else { ((b, to), (a, from)) };
                let share = if b == a { 1.0 } else { f64::from(step.clamp(a, b) - a) / f64::from(b - a) };
                Some(from + (to - from) * share)
            }
        }
    }

    /// The last step where what it paints can change: past it, it paints
    /// the same value.
    fn last_change(self) -> u32 {
        match self {
            Self::Fill(_) => 0,
            Self::Line((a, _), (b, _)) => a.max(b),
        }
    }
}

/// One lane: a control of an instrument, set by its points.
#[derive(Debug, Clone, PartialEq)]
pub struct AutomationLane {
    /// The track name it acts on, in every pattern of the song.
    instrument: String,
    target: AutomationTarget,
    /// Sorted by step, one per step at most.
    points: Vec<AutomationPoint>,
    /// How many of the song's rows it is drawn over,
    /// 1..=[`AutomationLane::MAX_ROWS`].
    row_count: u32,
}

impl AutomationLane {
    /// The most rows a lane is drawn over.
    pub const MAX_ROWS: u32 = 4;

    #[must_use]
    pub fn new(instrument: &str, target: AutomationTarget) -> Self {
        Self { instrument: instrument.to_owned(), target, points: Vec::new(), row_count: 1 }
    }

    /// The track name it acts on, in every pattern of the song.
    #[must_use]
    pub fn instrument(&self) -> &str {
        &self.instrument
    }

    /// The control it sets.
    #[must_use]
    pub fn target(&self) -> AutomationTarget {
        self.target
    }

    /// How many of the song's rows it is drawn over.
    #[must_use]
    pub fn row_count(&self) -> u32 {
        self.row_count
    }

    /// Drawn over `rows` rows, kept within 1..=[`Self::MAX_ROWS`].
    pub fn set_row_count(&mut self, rows: u32) {
        self.row_count = rows.clamp(1, Self::MAX_ROWS);
    }

    /// Its points, by step.
    #[must_use]
    pub fn points(&self) -> &[AutomationPoint] {
        &self.points
    }

    /// Whether this lane sets `target` of `instrument`.
    #[must_use]
    pub fn is_for(&self, instrument: &str, target: AutomationTarget) -> bool {
        self.target == target && self.instrument == instrument
    }

    /// What the lane sets at `step`: the last point at or before it; `None`
    /// before the first one, or where a point gives the control back to the
    /// pattern.
    #[must_use]
    pub fn value_at(&self, step: u32) -> Option<f64> {
        let after = self.points.partition_point(|point| point.step <= step);
        after.checked_sub(1).and_then(|i| self.points[i].value)
    }

    /// What the lane sets at `position` (steps from the start of the song,
    /// with their fractions): a level glides from a point to the next one
    /// when they are on adjacent steps of a run of three points or more (a
    /// drawn line); a switch, or a level set on one step alone, holds.
    #[must_use]
    pub fn value_at_position(&self, position: f64) -> Option<f64> {
        let step = floor_u32(position);
        let value = self.value_at(step)?;
        let at = self.points.partition_point(|point| point.step < step);
        let next = match self.points.get(at..) {
            Some([here, next, rest @ ..]) if here.step == step && step.checked_add(1) == Some(next.step) => {
                let is_after_one = at.checked_sub(1).is_some_and(|i| step.checked_sub(1) == Some(self.points[i].step));
                let is_before_one = rest.first().is_some_and(|after| next.step.checked_add(1) == Some(after.step));
                if is_after_one || is_before_one { next.value } else { None }
            }
            _ => None,
        };
        match (self.target.kind(), next) {
            (AutomationKind::Level, Some(to)) => Some(value + (to - value) * (position - f64::from(step))),
            _ => Some(value),
        }
    }

    /// Whether the lane switches its control on at `step`; `None` where it
    /// leaves it to the pattern.
    #[must_use]
    pub fn is_on_at(&self, step: u32) -> Option<bool> {
        self.value_at(step).map(is_on)
    }

    /// The steps `from` to `to` (either way round) painted with `stroke`,
    /// the lane going back after them to what it set there before. No point
    /// is left that repeats the value before it.
    pub fn paint(&mut self, from: u32, to: u32, stroke: AutomationStroke) {
        let (from, to) = (from.min(to), from.max(to));
        let resume = to.checked_add(1).map(|next| (next, self.value_at(next)));
        let mut previous = from.checked_sub(1).and_then(|step| self.value_at(step));
        let end = resume.map_or(to, |(next, _)| next);
        self.points.retain(|point| point.step < from || point.step > end);
        // Past its last change the stroke paints one value: one point at most.
        for step in from..=stroke.last_change().clamp(from, to) {
            let value = stroke.value_at(step).map(kept);
            if value != previous {
                self.points.push(AutomationPoint { step, value });
                previous = value;
            }
        }
        if let Some((next, resume)) = resume.filter(|&(_, resume)| resume != previous) {
            self.points.push(AutomationPoint { step: next, value: resume });
        }
        self.points.sort_by_key(|point| point.step);
    }

    /// The segment at `step`: from the point at or before it (else the
    /// song's start) to the step before the next point (else the last of
    /// the song's `step_count` steps).
    #[must_use]
    pub fn segment_at(&self, step: u32, step_count: u32) -> (u32, u32) {
        let after = self.points.partition_point(|point| point.step <= step);
        let first = after.checked_sub(1).map_or(0, |i| self.points[i].step);
        let end = self.points.get(after).map_or(step_count, |point| point.step);
        (first, end.saturating_sub(1).max(step))
    }

    /// Every point removed: the lane sets nothing.
    pub fn clear(&mut self) {
        self.points.clear();
    }
}

/// What `lanes` set `target` of `instrument` to at `step`: the first lane
/// for it that sets it, `None` when none does.
#[must_use]
pub fn lanes_value_at(lanes: &[AutomationLane], instrument: &str, target: AutomationTarget, step: u32) -> Option<f64> {
    lanes.iter().filter(|lane| lane.is_for(instrument, target)).find_map(|lane| lane.value_at(step))
}

/// What `lanes` set `target` of `instrument` to at `position` (steps, with
/// their fractions: [`AutomationLane::value_at_position`]).
#[must_use]
pub fn lanes_value_at_position(
    lanes: &[AutomationLane],
    instrument: &str,
    target: AutomationTarget,
    position: f64,
) -> Option<f64> {
    lanes.iter().filter(|lane| lane.is_for(instrument, target)).find_map(|lane| lane.value_at_position(position))
}

/// Whether track `t` of `tracks` is silent at `step` of a song with
/// `lanes`: muted, or silenced by a solo. A solo lane that is on silences
/// every instrument not soloed, in every pattern; without lanes this is the
/// pattern's own mute and solo: with a track soloed, only the soloed ones play.
#[must_use]
pub fn is_silent(tracks: &[Track], t: usize, lanes: &[AutomationLane], step: u32) -> bool {
    let Some(track) = tracks.get(t) else { return true };
    let switch = |track: &Track, target| lanes_value_at(lanes, &track.name, target, step).map(is_on);
    let solo = |track: &Track| switch(track, AutomationTarget::Solo).unwrap_or(track.solo);
    let song_solo = lanes.iter().any(|lane| lane.target == AutomationTarget::Solo && lane.is_on_at(step) == Some(true));
    if song_solo || tracks.iter().any(solo) {
        !solo(track)
    } else {
        switch(track, AutomationTarget::Mute).unwrap_or(track.mute)
    }
}

/// The instrument names `song` plays: the track names of the patterns it
/// places, each once, sorted.
#[must_use]
pub fn song_instruments(song: &Song, patterns: &[Pattern]) -> Vec<String> {
    let mut names: Vec<String> = patterns
        .iter()
        .filter(|p| song.clips.iter().any(|clip| clip.pattern_id == p.id))
        .flat_map(|p| p.tracks.iter().map(|track| track.name.clone()))
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    const ON: Option<f64> = Some(1.0);
    const OFF: Option<f64> = Some(0.0);

    fn lane(points: &[(u32, Option<f64>)]) -> AutomationLane {
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Mute);
        lane.points = points.iter().map(|&(step, value)| AutomationPoint { step, value }).collect();
        lane
    }

    fn steps(lane: &AutomationLane) -> Vec<(u32, Option<f64>)> {
        lane.points.iter().map(|p| (p.step, p.value)).collect()
    }

    fn fill(value: Option<f64>) -> AutomationStroke {
        AutomationStroke::Fill(value)
    }

    #[test]
    fn a_point_holds_until_the_next_one() {
        let lane = lane(&[(4, ON), (8, Some(0.2))]);
        let values: Vec<_> = [0, 3, 4, 7, 8, 100].map(|s| lane.is_on_at(s)).into();
        assert_eq!(values, [None, None, Some(true), Some(true), Some(false), Some(false)]);
        assert_eq!(lane.value_at(9), Some(0.2));
    }

    #[test]
    fn a_painted_zone_is_set_and_the_lane_goes_back_after_it() {
        let mut lane = lane(&[]);
        lane.paint(7, 4, fill(ON));
        assert_eq!(steps(&lane), [(4, ON), (8, None)], "either way round, no further, then the pattern's");
        lane.paint(5, 5, fill(OFF));
        assert_eq!(steps(&lane), [(4, ON), (5, OFF), (6, ON), (8, None)], "one step off inside");
        lane.paint(5, 5, fill(ON));
        assert_eq!(steps(&lane), [(4, ON), (8, None)], "and on again");
        lane.paint(6, 12, fill(ON));
        assert_eq!(steps(&lane), [(4, ON), (13, None)], "joined to the zone before");
        lane.paint(8, 9, fill(OFF));
        assert_eq!(steps(&lane), [(4, ON), (8, OFF), (10, ON), (13, None)]);
        lane.paint(0, 20, fill(OFF));
        assert_eq!(steps(&lane), [(0, OFF), (21, None)]);
        lane.paint(u32::MAX - 1, u32::MAX, fill(ON));
        assert_eq!(steps(&lane), [(0, OFF), (21, None), (u32::MAX - 1, ON)]);
        lane.clear();
        assert!(lane.points.is_empty());
    }

    #[test]
    fn a_segment_runs_from_its_point_to_the_next() {
        let lane = lane(&[(4, ON), (8, OFF)]);
        assert_eq!([0, 4, 6, 8, 20].map(|s| lane.segment_at(s, 32)), [(0, 3), (4, 7), (4, 7), (8, 31), (8, 31)]);
        assert_eq!(lane.segment_at(40, 32), (8, 40), "past the song's end: the step itself at least");
    }

    #[test]
    fn an_erased_zone_goes_back_to_the_pattern() {
        let mut lane = lane(&[(0, ON), (8, OFF)]);
        lane.paint(2, 5, fill(None));
        assert_eq!(steps(&lane), [(0, ON), (2, None), (6, ON), (8, OFF)]);
        assert_eq!([1, 2, 5, 6].map(|s| lane.is_on_at(s)), [Some(true), None, None, Some(true)]);
        lane.paint(0, 7, fill(None));
        assert_eq!(steps(&lane), [(8, OFF)], "nothing left to repeat the pattern's");
        lane.paint(8, 100, fill(None));
        assert_eq!(steps(&lane), [(101, OFF)], "the off held on resumes after the zone");
        lane.paint(0, u32::MAX, fill(None));
        assert!(lane.points.is_empty(), "erased whole");
    }

    #[test]
    fn a_line_sets_a_value_on_each_step_it_crosses() {
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Volume);
        lane.paint(2, 8, AutomationStroke::Line((6, 0.0), (2, 1.0)));
        let expected = [(2, Some(1.0)), (3, Some(0.75)), (4, Some(0.5)), (5, Some(0.25)), (6, Some(0.0)), (9, None)];
        assert_eq!(steps(&lane), expected, "held level past its end, to the zone's");
        lane.paint(3, 3, AutomationStroke::Line((3, 1.0 / 3.0), (3, 1.0 / 3.0)));
        assert_eq!(lane.value_at(3), Some(0.333), "rounded");
        lane.paint(0, u32::MAX, AutomationStroke::Line((0, 2.0), (0, 2.0)));
        assert_eq!(steps(&lane), [(0, Some(1.0))], "kept within 0 to 1, one point for a level zone");
    }

    #[test]
    fn reads_and_writes_lanes() {
        let raw = json::parse(
            r#"{"instrument":" KICK ","target":"solo","points":[{"step":16,"value":0},{"step":4.5,"value":1},
                {"step":-1},{"value":1},{"step":16,"value":1},{"step":20,"value":null},{"step":24,"value":true},
                {"step":28,"on":true},{"step":32,"value":1.5}]}"#,
        )
        .unwrap();
        let mut issues = Vec::new();
        let lane = AutomationLane::from_json(&raw, "a", &mut issues).unwrap();
        assert_eq!((lane.instrument.as_str(), lane.target), ("KICK", AutomationTarget::Solo));
        assert_eq!(steps(&lane), [(4, ON), (16, ON), (20, None), (32, ON)]);
        let at: Vec<_> = issues.iter().map(|i| i.at.as_str()).collect();
        let expected =
            ["a.points[1].step", "a.points[2]", "a.points[3]", "a.points[4]", "a.points[6].value", "a.points[7].value"];
        assert_eq!(at, [&expected[..], &["a.points[8].value"]].concat());
        let again = AutomationLane::from_json(&lane.to_json(), "a", &mut Vec::new()).unwrap();
        assert_eq!(again, lane);
        let mut tall = lane.clone();
        tall.set_row_count(9);
        assert_eq!(tall.row_count(), AutomationLane::MAX_ROWS);
        assert_eq!(AutomationLane::from_json(&tall.to_json(), "a", &mut Vec::new()).unwrap(), tall);
        let mut issues = Vec::new();
        let odd = json::parse(r#"{"instrument":"KICK","target":"mute","rows":2.5}"#).unwrap();
        assert_eq!(AutomationLane::from_json(&odd, "a", &mut issues).unwrap().row_count(), 1);
        assert_eq!(issues.iter().map(|i| i.at.as_str()).collect::<Vec<_>>(), ["a.rows"]);

        let mut issues = Vec::new();
        let bad = json::parse(r#"[{"instrument":"KICK","target":"tilt"},{"target":"mute"},1]"#).unwrap();
        for (i, raw) in bad.as_array().unwrap().iter().enumerate() {
            assert_eq!(AutomationLane::from_json(raw, &format!("l[{i}]"), &mut issues), None);
        }
        let at: Vec<_> = issues.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(at, ["l[0].target", "l[1].instrument", "l[2]"]);
    }

    #[test]
    fn lanes_mute_and_solo_instruments_across_patterns() {
        let track = |name: &str, mute: bool| Track { name: name.into(), mute, ..Track::default() };
        let tracks = [track("KICK", false), track("SNARE", true)];
        let silent = |lanes: &[AutomationLane], step| [0, 1].map(|t| is_silent(&tracks, t, lanes, step));
        assert_eq!(silent(&[], 0), [false, true], "the pattern's own mute");

        let mut mute = AutomationLane::new("KICK", AutomationTarget::Mute);
        mute.paint(4, u32::MAX, fill(ON));
        let mut unmute = AutomationLane::new("SNARE", AutomationTarget::Mute);
        unmute.points = vec![AutomationPoint { step: 0, value: OFF }];
        let lanes = [mute, unmute];
        assert_eq!(silent(&lanes, 0), [false, false]);
        assert_eq!(silent(&lanes, 4), [true, false]);

        // A solo of an instrument this pattern lacks silences all of it.
        let mut solo = AutomationLane::new("HAT", AutomationTarget::Solo);
        solo.paint(8, u32::MAX, fill(ON));
        let lanes = [solo];
        assert_eq!(silent(&lanes, 7), [false, true]);
        assert_eq!(silent(&lanes, 8), [true, true]);
        assert!(is_silent(&tracks, 5, &[], 0), "no such track");
    }

    #[test]
    fn a_volume_is_a_fader_in_decibels_and_glides_to_the_next_step() {
        assert_eq!([0.0, 0.5, 1.0].map(volume_db), [None, Some(-15.0), Some(0.0)]);
        assert_eq!((volume_gain(0.0), volume_gain(1.0)), (0.0, 1.0));
        assert!((volume_gain(0.5) - 0.1778).abs() < 1e-4, "-15 dB");
        let label = |v| AutomationTarget::Volume.value_label(v);
        assert_eq!([label(0.0), label(0.75), label(1.0)], ["silent", "-3.8 dB", "0.0 dB"]);
        assert_eq!(AutomationTarget::Mute.value_label(0.7), "on");
        for level in [0.0, 0.25, 0.5, 1.0] {
            assert!((volume_level(volume_gain(level)) - level).abs() < 1e-9, "{level}");
        }
        assert_eq!(step_label(21), "2.6");

        let mut lane = AutomationLane::new("KICK", AutomationTarget::Volume);
        lane.points = vec![
            AutomationPoint { step: 0, value: Some(0.0) },
            AutomationPoint { step: 1, value: Some(1.0) },
            AutomationPoint { step: 4, value: Some(0.5) },
            AutomationPoint { step: 5, value: None },
        ];
        lane.points.insert(2, AutomationPoint { step: 2, value: Some(0.5) });
        assert_eq!(lane.value_at_position(0.25), Some(0.25), "to the next step's point, in a line");
        assert_eq!(lane.value_at_position(1.5), Some(0.75), "on along the line");
        assert_eq!(lane.value_at_position(2.5), Some(0.5), "held: no point on the next step");
        assert_eq!(lane.value_at_position(3.5), Some(0.5), "a jump, not a glide over the hold");
        assert_eq!(lane.value_at_position(4.5), Some(0.5), "no glide into the pattern's");
        let mut click = AutomationLane::new("KICK", AutomationTarget::Volume);
        click.paint(6, 6, fill(Some(1.0)));
        click.paint(0, 5, fill(Some(0.2)));
        assert_eq!(steps(&click), [(0, Some(0.2)), (6, Some(1.0)), (7, None)]);
        click.paint(7, 9, fill(Some(0.2)));
        assert_eq!(click.value_at_position(6.5), Some(1.0), "a step set alone holds for its step");
        let mut mute = lane.clone();
        mute.target = AutomationTarget::Mute;
        assert_eq!(mute.value_at_position(0.75), Some(0.0), "a switch holds");
    }

    #[test]
    fn painting_is_idempotent_and_repainting_a_lane_s_own_values_changes_nothing() {
        let mut base = AutomationLane::new("KICK", AutomationTarget::Volume);
        base.paint(0, 40, fill(Some(0.5)));
        base.paint(8, 12, AutomationStroke::Line((8, 0.0), (12, 1.0)));
        base.paint(20, 23, fill(None));
        let strokes = [
            (4, 30, fill(Some(0.25))),
            (10, 10, fill(None)),
            (0, u32::MAX, fill(Some(1.0))),
            (30, 2, AutomationStroke::Line((30, 0.9), (2, 0.1))),
            (u32::MAX - 3, u32::MAX, AutomationStroke::Line((u32::MAX - 3, 0.0), (u32::MAX, 1.0))),
        ];
        for (from, to, stroke) in strokes {
            let mut once = base.clone();
            once.paint(from, to, stroke);
            let mut twice = once.clone();
            twice.paint(from, to, stroke);
            assert_eq!(twice, once, "painted twice from {from} to {to}: {stroke:?}");
        }
        // Each step painted with the value the lane already has there: the same points.
        for (from, to) in [(0, 40), (9, 11), (19, 25), (0, 100)] {
            let mut same = base.clone();
            for step in from..=to {
                same.paint(step, step, fill(base.value_at(step)));
            }
            assert_eq!(same, base, "its own values repainted from {from} to {to}");
        }
    }
}
