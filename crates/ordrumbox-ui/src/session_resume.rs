//! Where the user left off: the pattern shown, the arrangement selected, the
//! view under the header (the song panel is song mode) and the panels shown
//! or folded. They are kept in `settings.json` (`resume`) at quit, and before
//! the session is loaded again, and are read back at start. The pattern and
//! the arrangement are kept by id, so they are found again after a reorder.
//! A plugin keeps none: the host's project holds its state.

use super::Session;
use super::session_types::Workspace;
use crate::store;
use ordrumbox_core::json::{Object, Value};

/// The key of the place in `settings.json`.
const KEY: &str = "resume";

impl Workspace {
    /// The name of the view in `settings.json`.
    const fn setting(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::PianoRoll => "pianoRoll",
            Self::Song => "song",
            Self::Synth => "synth",
            Self::Mixer => "mixer",
        }
    }

    /// The view named `name` in `settings.json`.
    fn from_setting(name: &str) -> Option<Self> {
        [Self::Grid, Self::PianoRoll, Self::Song, Self::Synth, Self::Mixer].into_iter().find(|w| w.setting() == name)
    }

    /// Its button among `view::VIEWS`.
    const fn view_button(self) -> u8 {
        match self {
            Self::Grid => 0,
            Self::PianoRoll => 1,
            Self::Song => 2,
            Self::Synth => 3,
            Self::Mixer => 4,
        }
    }
}

/// The panels that are shown or folded, by their key in `resume`.
const PANELS: [&str; 4] = ["trackEditor", "noteEditor", "vuMeters", "kitEditor"];

impl Session {
    /// The place of the settings file's `resume`, if it has one, shown: what
    /// it names that is gone or wrong is a status line, and left as it is.
    pub(super) fn load_resume(&mut self) {
        let Some(path) = self.saving.settings_file.clone() else { return };
        // `load_master` has reported a file that cannot be read.
        let Ok(Some(value)) = store::load_setting(&path, KEY) else { return };
        let Some(place) = value.as_object() else {
            self.status.push(format!("settings.json: {KEY}: {} is not an object, not used", value.to_json()));
            return;
        };
        let message = std::mem::take(&mut self.message);
        let mut wrong = Vec::new();
        self.resume_pattern(place, &mut wrong);
        self.resume_song(place, &mut wrong);
        self.resume_panels(place, &mut wrong);
        self.resume_view(place, &mut wrong);
        self.message = message;
        if !wrong.is_empty() {
            self.status.push(format!("settings.json: {KEY}: {}", wrong.join("; ")));
        }
    }

    fn resume_pattern(&mut self, place: &Object, wrong: &mut Vec<String>) {
        let Some(id) = place.get("pattern") else { return };
        let found = id.as_str().and_then(|id| self.editor.library().patterns.iter().position(|p| p.id == id));
        match found {
            Some(index) => self.pattern_index = index,
            None => wrong.push(format!("no pattern {}, the first one shown", id.to_json())),
        }
    }

    fn resume_song(&mut self, place: &Object, wrong: &mut Vec<String>) {
        let Some(id) = place.get("song") else { return };
        let found = id.as_str().and_then(|id| self.editor.library().songs.iter().position(|s| s.id == id));
        match found {
            Some(index) => {
                self.editor.set_selected_song(index);
            }
            None => wrong.push(format!("no arrangement {}, the first one selected", id.to_json())),
        }
    }

    fn resume_panels(&mut self, place: &Object, wrong: &mut Vec<String>) {
        for key in PANELS {
            let Some(value) = place.get(key) else { continue };
            let Some(shown) = value.as_bool() else {
                wrong.push(format!("{key}: {} is not true or false, left as it was", value.to_json()));
                continue;
            };
            if shown != self.panel_shown(key) {
                match key {
                    "trackEditor" => self.toggle_track_editor(),
                    "noteEditor" => self.toggle_note_editor(),
                    "vuMeters" => self.toggle_vus(),
                    _ => self.toggle_kit_editor(),
                }
            }
        }
    }

    fn resume_view(&mut self, place: &Object, wrong: &mut Vec<String>) {
        let Some(value) = place.get("view") else { return };
        match value.as_str().and_then(Workspace::from_setting) {
            Some(Workspace::Grid) => {}
            // The view button would close the kit editor: the view is set under it.
            Some(workspace) => {
                let kit_editor = self.kit_editor.sample;
                self.show_view(workspace.view_button());
                self.kit_editor.sample = kit_editor;
            }
            None => wrong.push(format!("view: {} is not a view, the grid shown", value.to_json())),
        }
    }

    /// Panel `key` of [`PANELS`] is shown.
    fn panel_shown(&self, key: &str) -> bool {
        match key {
            "trackEditor" => self.chrome.show_track_editor,
            "noteEditor" => self.chrome.show_note_editor,
            "vuMeters" => self.chrome.show_vus,
            _ => self.kit_editor.is_open(),
        }
    }

    /// Where the user is, as `resume` keeps it.
    fn resume_record(&self) -> Value {
        let mut place = Object::new();
        if let Some(pattern) = self.pattern() {
            place.insert("pattern", Value::from(pattern.id.as_str()));
        }
        if let Some(song) = self.song() {
            place.insert("song", Value::from(song.id.as_str()));
        }
        place.insert("view", Value::from(self.workspace.setting()));
        for key in PANELS {
            place.insert(key, Value::from(self.panel_shown(key)));
        }
        Value::from(place)
    }

    /// Keep where the user is in the settings file, to start there next
    /// time (at quit, before a reload); a plugin keeps nothing.
    pub fn save_resume(&mut self) {
        if self.host.hosted {
            return;
        }
        let Some(path) = &self.saving.settings_file else { return };
        if let Err(e) = store::save_setting(path, KEY, self.resume_record()) {
            self.message = format!("place not kept for next time: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use crate::test_support::scratch_dir;
    use ordrumbox_core::Library;
    use std::path::PathBuf;

    /// Three patterns, two arrangements, its settings in `path`.
    fn session(path: &std::path::Path) -> Session {
        let (library, _) = Library::parse(
            r#"{"songs":[{"name":"verse"},{"name":"chorus"}],
                "patterns":[{"name":"a"},{"name":"b"},{"name":"c"}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.saving.settings_file = Some(PathBuf::from(path));
        s
    }

    #[test]
    fn the_session_starts_where_it_was_left() {
        let dir = scratch_dir("resume");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"colorScheme":2}"#).unwrap();
        let mut s = session(&path);
        s.show_pattern(2);
        s.select_song(1);
        s.toggle_song_view();
        s.toggle_note_editor();
        s.toggle_vus();
        s.save_resume();
        assert!(store::load_setting(&path, "colorScheme").unwrap().is_some(), "the other settings are kept");

        let mut again = session(&path);
        again.load_resume();
        assert_eq!(again.status, Vec::<String>::new());
        assert_eq!((again.pattern_index, again.editor.selected_song()), (2, 1));
        assert_eq!(again.workspace, Workspace::Song);
        let panels = PANELS.map(|key| again.panel_shown(key));
        assert_eq!(panels, [true, false, false, false]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn every_view_and_the_kit_editor_over_it_come_back() {
        let dir = scratch_dir("resume-views");
        let path = dir.join("settings.json");
        for workspace in [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth, Workspace::Mixer] {
            let mut s = session(&path);
            s.show_view(workspace.view_button());
            s.toggle_kit_editor();
            s.save_resume();
            let mut again = session(&path);
            again.load_resume();
            assert_eq!((again.workspace, again.kit_editor.is_open()), (workspace, true));
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn what_is_gone_or_wrong_is_reported_and_left_as_it_was() {
        let dir = scratch_dir("resume-wrong");
        let path = dir.join("settings.json");
        std::fs::write(
            &path,
            r#"{"resume":{"pattern":"gone","song":3,"view":"tape","noteEditor":"no","vuMeters":false}}"#,
        )
        .unwrap();
        let mut s = session(&path);
        s.load_resume();
        assert_eq!(
            s.status,
            [
                r#"settings.json: resume: no pattern "gone", the first one shown; no arrangement 3, the first one selected; noteEditor: "no" is not true or false, left as it was; view: "tape" is not a view, the grid shown"#
            ]
        );
        assert_eq!((s.pattern_index, s.editor.selected_song(), s.workspace), (0, 0, Workspace::Grid));
        assert!(!s.chrome.show_vus, "what is right is used");
        std::fs::write(&path, r#"{"resume":1}"#).unwrap();
        let mut s = session(&path);
        s.load_resume();
        assert_eq!(s.status, ["settings.json: resume: 1 is not an object, not used"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_view_has_its_button() {
        let labels = [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth, Workspace::Mixer]
            .map(|w| crate::view::VIEWS[usize::from(w.view_button())]);
        assert_eq!(labels, ["GRID", "ROLL", "SONG", "SYNTH", "MIX"]);
    }

    #[test]
    fn a_plugin_keeps_no_place() {
        let dir = scratch_dir("resume-hosted");
        let path = dir.join("settings.json");
        let mut s = session(&path);
        s.host.hosted = true;
        s.save_resume();
        assert!(!path.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
