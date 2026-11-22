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

    /// A click on `step` of lane `lane`: a point placed there, or removed
    /// ([`AutomationLane::toggle_at`]); what the lane sets there after it.
    pub fn toggle_automation_point(&mut self, song: Option<usize>, lane: usize, step: u32) -> Result<bool, String> {
        let s = self.require_song(song)?;
        let Some(current) = self.library.songs[s].automations.get(lane) else {
            return Err("No such automation lane".to_owned());
        };
        let mut edited = current.clone();
        let value = edited.toggle_at(step).unwrap_or(false);
        let state = if value { "on" } else { "off" };
        let desc = format!("{} {} {state} at {}", edited.instrument, edited.target.label(), step_label(step));
        self.edit_songs(desc, None, |songs| songs[s].automations[lane] = edited);
        Ok(value)
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
        assert_eq!(e.toggle_automation_point(None, 0, 20), Ok(true));
        assert_eq!(e.undo().as_deref(), Some("KICK Mute on at 2.5"));
        assert!(lanes(&e)[0].points().is_empty());
        assert!(e.redo().is_some());
        assert_eq!(e.toggle_automation_point(None, 0, 24), Ok(false));
        assert_eq!(lanes(&e)[0].points().len(), 2);
        assert!(e.toggle_automation_point(None, 5, 0).is_err());

        assert!(e.remove_automation_lane(None, 0));
        assert_eq!(lanes(&e).iter().map(|l| l.target).collect::<Vec<_>>(), [AutomationTarget::Solo]);
        assert_eq!(e.undo().as_deref(), Some("Remove mute automation of KICK"));
        assert_eq!(lanes(&e).len(), 2);
        assert!(!e.remove_automation_lane(None, 9));
    }

    #[test]
    fn lanes_are_saved_with_the_song() {
        let mut e = editor();
        e.add_automation_lane(None, "KICK", AutomationTarget::Solo).unwrap();
        e.toggle_automation_point(None, 0, 3).unwrap();
        let (library, report) = Library::parse(&e.library().to_file_text()).unwrap();
        assert!(report.issues.is_empty(), "{:?}", report.issues);
        assert_eq!(library.songs[0].automations, lanes(&e));
    }
}
