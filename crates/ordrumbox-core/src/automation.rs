//! Song automation: lanes that switch a control of an instrument on and off
//! along an arrangement, at the resolution of a step (a sixteenth note).
//!
//! A lane holds points, each turning its control on or off from its step
//! until the next point. Before its first point a lane leaves the control
//! as the pattern has it. An instrument is named as its tracks are (KICK,
//! SNARE...), so a lane follows it through every pattern of the song.

use crate::json::{Object, Value};
use crate::record::Issue;
use crate::{Pattern, Song, Track};

/// Steps of a beat on an automation lane.
pub const STEPS_PER_BEAT: u32 = 4;
/// Steps of a 4/4 measure on an automation lane.
pub const STEPS_PER_MEASURE: u32 = STEPS_PER_BEAT * 4;

/// The control a lane switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutomationTarget {
    Mute,
    Solo,
}

impl AutomationTarget {
    pub const ALL: [Self; 2] = [Self::Mute, Self::Solo];

    /// Its key in a file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Mute => "mute",
            Self::Solo => "solo",
        }
    }

    /// Its name as shown.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Solo => "Solo",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.key() == key)
    }
}

/// A point of a lane: from `step` on, the control is on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomationPoint {
    /// Steps from the start of the song.
    pub step: u32,
    pub is_on: bool,
}

/// One lane: a control of an instrument, switched by its points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationLane {
    /// The track name it acts on, in every pattern of the song.
    pub instrument: String,
    pub target: AutomationTarget,
    /// Sorted by step, one per step at most.
    points: Vec<AutomationPoint>,
}

impl AutomationLane {
    #[must_use]
    pub fn new(instrument: &str, target: AutomationTarget) -> Self {
        Self { instrument: instrument.to_owned(), target, points: Vec::new() }
    }

    /// Its points, by step.
    #[must_use]
    pub fn points(&self) -> &[AutomationPoint] {
        &self.points
    }

    /// Whether this lane switches `target` of `instrument`.
    #[must_use]
    pub fn is_for(&self, instrument: &str, target: AutomationTarget) -> bool {
        self.target == target && self.instrument == instrument
    }

    /// What the lane sets at `step`: the last point at or before it, `None`
    /// before the first one.
    #[must_use]
    pub fn value_at(&self, step: u32) -> Option<bool> {
        let after = self.points.partition_point(|point| point.step <= step);
        after.checked_sub(1).map(|i| self.points[i].is_on)
    }

    /// The steps `from` to `to` (either way round) set `is_on`, the lane
    /// going back after them to what it set there before (off where it set
    /// nothing). No point is left that repeats the value before it.
    pub fn paint(&mut self, from: u32, to: u32, is_on: bool) {
        let (from, to) = (from.min(to), from.max(to));
        let resume = to.checked_add(1).map(|next| (next, self.value_at(next).unwrap_or(false)));
        let before = from.checked_sub(1).and_then(|step| self.value_at(step));
        let end = resume.map_or(to, |(next, _)| next);
        self.points.retain(|point| point.step < from || point.step > end);
        if before != Some(is_on) {
            self.points.push(AutomationPoint { step: from, is_on });
        }
        if let Some((next, resume)) = resume.filter(|&(_, resume)| resume != is_on) {
            self.points.push(AutomationPoint { step: next, is_on: resume });
        }
        self.points.sort_by_key(|point| point.step);
    }

    /// Every point removed: the lane sets nothing.
    pub fn clear(&mut self) {
        self.points.clear();
    }

    /// A lane read from a file (`at` its JSON path): points with no usable
    /// step left out, a repeated step kept once (the last one), all reported.
    pub fn from_json(raw: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let Some(object) = raw.as_object() else {
            issues.push(Issue::new(at, "an automation lane must be an object, skipped"));
            return None;
        };
        let instrument = object.get("instrument").and_then(Value::to_text).unwrap_or_default();
        let instrument = instrument.trim();
        if instrument.is_empty() {
            issues.push(Issue::new(format!("{at}.instrument"), "no instrument, lane skipped"));
            return None;
        }
        let key = object.get("target").and_then(Value::to_text).unwrap_or_default();
        let Some(target) = AutomationTarget::from_key(&key) else {
            issues.push(Issue::new(format!("{at}.target"), format!("unknown target \"{key}\", lane skipped")));
            return None;
        };
        let mut lane = Self::new(instrument, target);
        let points = match object.get("points").filter(|v| !v.is_null()) {
            None => &[][..],
            Some(Value::Array(items)) => items.as_slice(),
            Some(other) => {
                issues.push(Issue::new(format!("{at}.points"), format!("not an array ({other}), no points")));
                &[][..]
            }
        };
        for (i, raw_point) in points.iter().enumerate() {
            let point_at = format!("{at}.points[{i}]");
            let step = raw_point.as_object().and_then(|p| p.get("step")).and_then(Value::to_number);
            let Some(step) = step.filter(|s| s.is_finite() && *s >= 0.0 && *s <= f64::from(u32::MAX)) else {
                issues.push(Issue::new(point_at, "no usable step, point skipped"));
                continue;
            };
            let is_on = raw_point.as_object().and_then(|p| p.get("on")).and_then(Value::as_bool).unwrap_or(false);
            let step = step.floor() as u32;
            match lane.points.binary_search_by_key(&step, |point| point.step) {
                Ok(index) => {
                    issues.push(Issue::new(point_at, format!("step {step} repeated, the last point kept")));
                    lane.points[index].is_on = is_on;
                }
                Err(index) => lane.points.insert(index, AutomationPoint { step, is_on }),
            }
        }
        Some(lane)
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("instrument", Value::from(self.instrument.as_str()));
        object.insert("target", Value::from(self.target.key()));
        let points = self.points.iter().map(|point| {
            let mut object = Object::new();
            object.insert("step", Value::from(point.step));
            object.insert("on", Value::from(point.is_on));
            Value::Object(object)
        });
        object.insert("points", Value::Array(points.collect()));
        Value::Object(object)
    }
}

/// What `lanes` set `target` of `instrument` to at `step`: the first lane
/// for it that sets it, `None` when none does.
#[must_use]
pub fn automated(lanes: &[AutomationLane], instrument: &str, target: AutomationTarget, step: u32) -> Option<bool> {
    lanes.iter().filter(|lane| lane.is_for(instrument, target)).find_map(|lane| lane.value_at(step))
}

/// Whether track `t` of `tracks` is silent at `step` of a song with
/// `lanes`: muted, or silenced by a solo. A solo lane that is on silences
/// every instrument not soloed, in every pattern; without lanes this is the
/// pattern's own mute and solo (`shouldTrackPlay()`).
#[must_use]
pub fn is_silent(tracks: &[Track], t: usize, lanes: &[AutomationLane], step: u32) -> bool {
    let Some(track) = tracks.get(t) else { return true };
    let solo = |track: &Track| automated(lanes, &track.name, AutomationTarget::Solo, step).unwrap_or(track.solo);
    let song_solo = lanes.iter().any(|lane| lane.target == AutomationTarget::Solo && lane.value_at(step) == Some(true));
    if song_solo || tracks.iter().any(solo) {
        !solo(track)
    } else {
        automated(lanes, &track.name, AutomationTarget::Mute, step).unwrap_or(track.mute)
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

    fn lane(points: &[(u32, bool)]) -> AutomationLane {
        let mut lane = AutomationLane::new("KICK", AutomationTarget::Mute);
        lane.points = points.iter().map(|&(step, is_on)| AutomationPoint { step, is_on }).collect();
        lane
    }

    #[test]
    fn a_point_holds_until_the_next_one() {
        let lane = lane(&[(4, true), (8, false)]);
        let values: Vec<_> = [0, 3, 4, 7, 8, 100].map(|s| lane.value_at(s)).into();
        assert_eq!(values, [None, None, Some(true), Some(true), Some(false), Some(false)]);
    }

    #[test]
    fn a_painted_zone_is_set_and_the_lane_goes_back_after_it() {
        let steps = |lane: &AutomationLane| lane.points.iter().map(|p| (p.step, p.is_on)).collect::<Vec<_>>();
        let mut lane = lane(&[]);
        lane.paint(7, 4, true);
        assert_eq!(steps(&lane), [(4, true), (8, false)], "either way round, no further");
        lane.paint(5, 5, false);
        assert_eq!(steps(&lane), [(4, true), (5, false), (6, true), (8, false)], "one step off inside");
        lane.paint(5, 5, true);
        assert_eq!(steps(&lane), [(4, true), (8, false)], "and on again");
        lane.paint(6, 12, true);
        assert_eq!(steps(&lane), [(4, true), (13, false)], "joined to the zone before");
        lane.paint(8, 9, false);
        assert_eq!(steps(&lane), [(4, true), (8, false), (10, true), (13, false)]);
        lane.paint(0, 20, false);
        assert_eq!(steps(&lane), [(0, false)]);
        lane.paint(u32::MAX - 1, u32::MAX, true);
        assert_eq!(steps(&lane), [(0, false), (u32::MAX - 1, true)]);
        lane.clear();
        assert!(lane.points.is_empty());
    }

    #[test]
    fn reads_and_writes_lanes() {
        let raw = json::parse(
            r#"{"instrument":" KICK ","target":"solo","points":[{"step":16,"on":false},{"step":4.5,"on":true},
                {"step":-1},{"on":true},{"step":16,"on":true}]}"#,
        )
        .unwrap();
        let mut issues = Vec::new();
        let lane = AutomationLane::from_json(&raw, "a", &mut issues).unwrap();
        assert_eq!((lane.instrument.as_str(), lane.target), ("KICK", AutomationTarget::Solo));
        assert_eq!(lane.points.iter().map(|p| (p.step, p.is_on)).collect::<Vec<_>>(), [(4, true), (16, true)]);
        let at: Vec<_> = issues.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(at, ["a.points[2]", "a.points[3]", "a.points[4]"]);
        let again = AutomationLane::from_json(&lane.to_json(), "a", &mut Vec::new()).unwrap();
        assert_eq!(again, lane);

        let mut issues = Vec::new();
        let bad = json::parse(r#"[{"instrument":"KICK","target":"volume"},{"target":"mute"},1]"#).unwrap();
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
        mute.paint(4, u32::MAX, true);
        let mut unmute = AutomationLane::new("SNARE", AutomationTarget::Mute);
        unmute.points = vec![AutomationPoint { step: 0, is_on: false }];
        let lanes = [mute, unmute];
        assert_eq!(silent(&lanes, 0), [false, false]);
        assert_eq!(silent(&lanes, 4), [true, false]);

        // A solo of an instrument this pattern lacks silences all of it.
        let mut solo = AutomationLane::new("HAT", AutomationTarget::Solo);
        solo.paint(8, u32::MAX, true);
        let lanes = [solo];
        assert_eq!(silent(&lanes, 7), [false, true]);
        assert_eq!(silent(&lanes, 8), [true, true]);
        assert!(is_silent(&tracks, 5, &[], 0), "no such track");
    }
}
