//! The automation lanes of an arrangement: added, removed, and their on and
//! off points placed step by step, each edit one undo step.

use ordrumbox_core::automation::{AutomationLane, AutomationStroke, AutomationTarget, step_label};

use crate::editor::Editor;

/// What `stroke` does to a lane of `target`, as an undo step says it.
fn stroke_label(target: AutomationTarget, stroke: AutomationStroke) -> String {
    match stroke {
        AutomationStroke::Fill(None) => "erased".to_owned(),
        AutomationStroke::Fill(Some(value)) => target.value_label(value),
        AutomationStroke::Line((_, from), (_, to)) if from == to => target.value_label(from),
        AutomationStroke::Line(..) => "drawn".to_owned(),
    }
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

    /// Lane `lane` set to `base` with steps `from` to `to` painted with
    /// `stroke` ([`AutomationLane::paint`]): a click paints one step, a drag over a
    /// lane paints from the lane as it was pressed, the edits under one
    /// `coalesce` key one undo step.
    pub fn paint_automation(
        &mut self,
        song: Option<usize>,
        lane: usize,
        base: &AutomationLane,
        (from, to): (u32, u32),
        stroke: AutomationStroke,
        coalesce: &str,
    ) -> Result<(), String> {
        let s = self.require_song(song)?;
        if lane >= self.library.songs[s].automations.len() {
            return Err("No such automation lane".to_owned());
        }
        let mut painted = base.clone();
        painted.paint(from, to, stroke);
        let state = stroke_label(base.target, stroke);
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

    /// Lane `lane` drawn over `rows` rows (1 to [`AutomationLane::MAX_ROWS`]);
    /// whether that changed it. The edits under one `coalesce` key are one
    /// undo step (a drag of its edge).
    pub fn set_automation_lane_rows(&mut self, song: Option<usize>, lane: usize, rows: u32, coalesce: &str) -> bool {
        let Some(s) = self.song_index(song) else { return false };
        let rows = rows.clamp(1, AutomationLane::MAX_ROWS);
        let Some(current) = self.library.songs[s].automations.get(lane).filter(|l| l.row_count() != rows) else {
            return false;
        };
        let desc = format!("{} {} lane {rows} row(s) tall", current.instrument, current.target.label());
        self.edit_songs_coalesced(desc, None, Some(coalesce.to_owned()), |songs| {
            songs[s].automations[lane].set_row_count(rows);
        });
        true
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

    const ON: AutomationStroke = AutomationStroke::Fill(Some(1.0));

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
        e.paint_automation(None, 0, &base, (20, 20), ON, "click").unwrap();
        assert_eq!(e.undo().as_deref(), Some("KICK Mute on at 2.5"));
        assert!(lanes(&e)[0].points().is_empty());
        assert!(e.redo().is_some());
        assert_eq!(lanes(&e)[0].points().len(), 2, "on for one step, then back");
        assert!(e.paint_automation(None, 5, &base, (0, 0), ON, "click").is_err());

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
            e.paint_automation(None, 0, &base, (4, to), ON, "drag").unwrap();
        }
        let steps: Vec<_> = lanes(&e)[0].points().iter().map(|p| (p.step, p.value)).collect();
        assert_eq!(steps, [(4, Some(1.0)), (8, None)], "painted from the lane as pressed");
        assert_eq!(e.undo().as_deref(), Some("KICK Mute on from 1.5 to 1.8"));
        assert!(lanes(&e)[0].points().is_empty(), "the whole drag, one step");
        assert!(e.redo().is_some());
        let base = lanes(&e)[0].clone();
        e.paint_automation(None, 0, &base, (6, 4), AutomationStroke::Fill(None), "erase").unwrap();
        assert_eq!(e.undo().as_deref(), Some("KICK Mute erased from 1.5 to 1.7"));
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
        e.paint_automation(None, 0, &base, (3, 6), ON, "drag").unwrap();
        let (library, report) = Library::parse(&e.library().to_file_text()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(library.songs[0].automations, lanes(&e));
    }

    #[test]
    fn a_volume_lane_is_drawn_and_said_in_decibels() {
        let mut e = editor();
        e.add_automation_lane(None, "KICK", AutomationTarget::Volume).unwrap();
        let base = lanes(&e)[0].clone();
        let line = AutomationStroke::Line((0, 1.0), (4, 0.0));
        e.paint_automation(None, 0, &base, (0, 4), line, "draw").unwrap();
        assert_eq!(lanes(&e)[0].value_at(2), Some(0.5));
        assert_eq!(e.undo().as_deref(), Some("KICK Volume drawn from 1.1 to 1.5"));
        let flat = AutomationStroke::Line((3, 0.8), (3, 0.8));
        e.paint_automation(None, 0, &base, (3, 3), flat, "click").unwrap();
        assert_eq!(e.undo().as_deref(), Some("KICK Volume -2.4 dB at 1.4"));
    }

    #[test]
    fn a_lane_is_made_taller_in_one_step() {
        let mut e = editor();
        e.add_automation_lane(None, "KICK", AutomationTarget::Volume).unwrap();
        assert!(e.set_automation_lane_rows(None, 0, 2, "drag"));
        assert!(e.set_automation_lane_rows(None, 0, 9, "drag"));
        assert!(!e.set_automation_lane_rows(None, 0, 4, "drag"), "already 4, the most");
        assert_eq!(lanes(&e)[0].row_count(), 4);
        assert_eq!(e.undo().as_deref(), Some("KICK Volume lane 4 row(s) tall"));
        assert_eq!(lanes(&e)[0].row_count(), 1, "the drag, one step");
    }
}
