//! The automation lanes of an arrangement: added, removed, and their on and
//! off points placed step by step, each edit one undo step.

use ordrumbox_core::automation::{AutomationLane, AutomationTarget, STEPS_PER_MEASURE};

use crate::editor::Editor;

/// `step` as shown: `measure.step`, both from 1.
fn step_label(step: u32) -> String {
    format!("{}.{}", step / STEPS_PER_MEASURE + 1, step % STEPS_PER_MEASURE + 1)
}

impl Editor {
    /// Add a lane switching `target` of `instrument` to arrangement `song`
    /// (else the selected one), after its other lanes; its index. An
    /// instrument has one lane per target.
    pub fn add_automation_lane(
        &mut self,
        song: Option<usize>,
        instrument: &str,
        target: AutomationTarget,
    ) -> Result<usize, String> {
        let s = self.require_song(song)?;
        let instrument = instrument.trim();
        if instrument.is_empty() {
            return Err("An automation lane needs an instrument".to_owned());
        }
        let lanes = &self.library.songs[s].automations;
        if lanes.iter().any(|lane| lane.is_for(instrument, target)) {
            return Err(format!("{instrument} already has a {} lane", target.label().to_lowercase()));
        }
        let index = lanes.len();
        let desc = format!("Add {} automation of {instrument}", target.label().to_lowercase());
        self.edit_songs(desc, None, |songs| songs[s].automations.push(AutomationLane::new(instrument, target)));
        Ok(index)
    }

    /// Lane `lane` set to `base` with steps `from` to `to` painted `is_on`
    /// ([`AutomationLane::paint`]): a click paints one step, a drag over a
    /// lane paints from the lane as it was pressed, the edits under one
    /// `coalesce` key one undo step.
    pub fn paint_automation(
        &mut self,
        song: Option<usize>,
        lane: usize,
        base: &AutomationLane,
        (from, to): (u32, u32),
        is_on: bool,
        coalesce: &str,
    ) -> Result<(), String> {
        let s = self.require_song(song)?;
        if lane >= self.library.songs[s].automations.len() {
            return Err("No such automation lane".to_owned());
        }
        let mut painted = base.clone();
        painted.paint(from, to, is_on);
        let state = if is_on { "on" } else { "off" };
        let (first, last) = (from.min(to), from.max(to));
        let zone = if first == last {
            format!("at {}", step_label(first))
        } else {
            format!("from {} to {}", step_label(first), step_label(last))
        };
        let desc = format!("{} {} {state} {zone}", base.instrument, base.target.label());
        self.edit_songs_coalesced(desc, None, Some(coalesce.to_owned()), |songs| songs[s].automations[lane] = painted);
        Ok(())
    }

    /// Every point of lane `lane` removed; whether it had some.
    pub fn clear_automation_lane(&mut self, song: Option<usize>, lane: usize) -> bool {
        let Some(s) = self.song_index(song) else { return false };
        let Some(current) = self.library.songs[s].automations.get(lane).filter(|l| !l.points().is_empty()) else {
            return false;
        };
        let desc = format!("Clear {} automation of {}", current.target.label().to_lowercase(), current.instrument);
        self.edit_songs(desc, None, |songs| songs[s].automations[lane].clear());
        true
    }

    /// Every lane of arrangement `song` (else the selected one) removed;
    /// whether it had some.
    pub fn remove_automation_lanes(&mut self, song: Option<usize>) -> bool {
        let Some(s) = self.song_index(song).filter(|&s| !self.library.songs[s].automations.is_empty()) else {
            return false;
        };
        self.edit_songs("Remove all automation lanes".to_owned(), None, |songs| songs[s].automations.clear());
        true
    }

    /// Remove lane `lane` of arrangement `song` (else the selected one);
    /// whether there was one.
    pub fn remove_automation_lane(&mut self, song: Option<usize>, lane: usize) -> bool {
        let Some(s) = self.song_index(song) else { return false };
        let Some(current) = self.library.songs[s].automations.get(lane) else { return false };
        let desc = format!("Remove {} automation of {}", current.target.label().to_lowercase(), current.instrument);
        self.edit_songs(desc, None, |songs| songs[s].automations.remove(lane));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn editor() -> Editor {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"a","tracks":[{"name":"KICK"}]}],
                "songs":[{"name":"s","clips":[{"pattern":"a","startMeasure":0,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let mut editor = Editor::new(library);
        editor.set_clock(Some(0.0));
        editor
    }

    fn lanes(e: &Editor) -> &[AutomationLane] {
        &e.library().songs[0].automations
    }

    #[test]
    fn lanes_and_points_are_undo_steps() {
        let mut e = editor();
        assert_eq!(e.add_automation_lane(None, "KICK", AutomationTarget::Mute), Ok(0));
        assert_eq!(
            e.add_automation_lane(None, "KICK", AutomationTarget::Mute),
            Err("KICK already has a mute lane".to_owned())
        );
        assert_eq!(e.add_automation_lane(None, "KICK", AutomationTarget::Solo), Ok(1));
        let base = lanes(&e)[0].clone();
        e.paint_automation(None, 0, &base, (20, 20), true, "click").unwrap();
        assert_eq!(e.undo().as_deref(), Some("KICK Mute on at 2.5"));
        assert!(lanes(&e)[0].points().is_empty());
        assert!(e.redo().is_some());
        assert_eq!(lanes(&e)[0].points().len(), 2, "on for one step, then back");
        assert!(e.paint_automation(None, 5, &base, (0, 0), true, "click").is_err());

        assert!(e.remove_automation_lane(None, 0));
        assert_eq!(lanes(&e).iter().map(|l| l.target).collect::<Vec<_>>(), [AutomationTarget::Solo]);
        assert_eq!(e.undo().as_deref(), Some("Remove mute automation of KICK"));
        assert_eq!(lanes(&e).len(), 2);
        assert!(!e.remove_automation_lane(None, 9));
    }

    #[test]
    fn a_painted_drag_is_one_step_and_lanes_clear() {
        let mut e = editor();
        e.add_automation_lane(None, "KICK", AutomationTarget::Mute).unwrap();
        e.add_automation_lane(None, "KICK", AutomationTarget::Solo).unwrap();
        let base = lanes(&e)[0].clone();
        for to in [6, 9, 7] {
            e.paint_automation(None, 0, &base, (4, to), true, "drag").unwrap();
        }
        let steps: Vec<_> = lanes(&e)[0].points().iter().map(|p| (p.step, p.is_on)).collect();
        assert_eq!(steps, [(4, true), (8, false)], "painted from the lane as pressed");
        assert_eq!(e.undo().as_deref(), Some("KICK Mute on from 1.5 to 1.8"));
        assert!(lanes(&e)[0].points().is_empty(), "the whole drag, one step");
        assert!(e.redo().is_some());

        assert!(e.clear_automation_lane(None, 0));
        assert!(!e.clear_automation_lane(None, 0), "nothing left to clear");
        assert!(e.remove_automation_lanes(None));
        assert!(lanes(&e).is_empty());
        assert!(!e.remove_automation_lanes(None));
        assert_eq!(e.undo().as_deref(), Some("Remove all automation lanes"));
        assert_eq!(lanes(&e).len(), 2);
    }

    #[test]
    fn lanes_are_saved_with_the_song() {
        let mut e = editor();
        e.add_automation_lane(None, "KICK", AutomationTarget::Solo).unwrap();
        let base = lanes(&e)[0].clone();
        e.paint_automation(None, 0, &base, (3, 6), true, "drag").unwrap();
        let (library, report) = Library::parse(&e.library().to_file_text()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(library.songs[0].automations, lanes(&e));
    }
}
