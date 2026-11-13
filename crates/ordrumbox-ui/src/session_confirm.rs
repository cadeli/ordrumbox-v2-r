//! Confirmations: what removes or replaces the user's work for good waits
//! for Enter in a notice; Esc or a click cancels it. An edit that Ctrl+Z
//! undoes is done at once, and its message says how to undo it.

use super::{Overlay, Session};
use std::path::PathBuf;

/// What the message of a change outside the library's history ends with.
pub(super) const NOT_UNDOABLE: &str = "not undoable";

/// What waits for the user's yes.
#[derive(Debug, Clone)]
pub(super) enum Pending {
    RemoveSample {
        kit: usize,
        sample: usize,
    },
    RemoveSynth(String),
    LoadSong(PathBuf),
    ImportSong(PathBuf),
    ImportLibrary(PathBuf),
    DemoLibrary,
    /// A backup's folder.
    RestoreUserData(PathBuf),
    /// The folder to use as the user data folder.
    UserFolder(PathBuf),
}

impl Session {
    /// Show the notice of `pending`.
    pub(super) fn ask(&mut self, pending: Pending) {
        self.chrome.overlay = Some(Overlay::Confirm { pending, yes: false });
    }

    /// Whether a confirmation is waiting for the user.
    pub(super) fn confirm_open(&self) -> bool {
        matches!(self.chrome.overlay, Some(Overlay::Confirm { .. }))
    }

    /// Whether a confirm (replace the library, replace a file, factory
    /// reset) is open: its keys are the buttons'.
    pub(super) fn notice_confirm_open(&self) -> bool {
        matches!(
            self.chrome.overlay,
            Some(Overlay::Confirm { .. } | Overlay::FactoryReset { .. } | Overlay::ConfirmOverwrite { .. })
        )
    }

    /// The button ENTER runs of the confirm open (`false`: keep, the default).
    pub(super) fn confirm_yes(&mut self) -> bool {
        match &mut self.chrome.overlay {
            Some(Overlay::Confirm { yes, .. })
            | Some(Overlay::FactoryReset { yes })
            | Some(Overlay::ConfirmOverwrite { yes, .. }) => *yes,
            _ => false,
        }
    }

    pub(super) fn set_confirm_yes(&mut self, yes: bool) {
        if let Some(slot) = match &mut self.chrome.overlay {
            Some(Overlay::Confirm { yes, .. }) | Some(Overlay::FactoryReset { yes }) => Some(yes),
            Some(Overlay::ConfirmOverwrite { yes, .. }) => Some(yes),
            _ => None,
        } {
            *slot = yes;
        }
    }

    /// Run what the confirm open asks for (its chosen button).
    pub(super) fn run_confirm(&mut self) {
        match self.chrome.overlay.take() {
            Some(Overlay::FactoryReset { .. }) => self.factory_reset(),
            Some(Overlay::ConfirmOverwrite { export, path, .. }) => self.write_export(export, &path),
            Some(Overlay::Confirm { pending, .. }) => self.run_pending(pending),
            _ => {}
        }
    }

    /// The user said yes.
    pub(super) fn run_pending(&mut self, pending: Pending) {
        match pending {
            Pending::RemoveSample { kit, sample } => self.kit_remove_now(kit, sample),
            Pending::RemoveSynth(key) => self.remove_synth(&key),
            Pending::LoadSong(path) => self.load_song_file(&path),
            Pending::ImportSong(path) => self.import_song_now(&path),
            Pending::ImportLibrary(path) => self.import_library_now(&path),
            Pending::DemoLibrary => self.load_demo_library_now(),
            Pending::RestoreUserData(path) => self.restore_user_data_now(&path),
            Pending::UserFolder(path) => self.change_user_folder_now(&path),
        }
    }

    /// Say that a change was made outside the library's history (the master,
    /// a synth preset, a kit), which Ctrl+Z leaves as it is.
    pub(super) fn not_undoable(&mut self, what: String) {
        self.message = format!("{what} ({NOT_UNDOABLE})");
    }

    /// Run `action`, an edit that Ctrl+Z (Cmd on macOS) undoes, at once
    /// instead of asking first; its message, when it edited, says how to undo it.
    pub(super) fn undoable(&mut self, action: impl FnOnce(&mut Self)) {
        let revision = self.editor.revision();
        action(self);
        if self.editor.revision() != revision {
            let key = if self.host.hosted { "Alt+Z".to_owned() } else { format!("{}+Z", crate::menu::command_label()) };
            self.message = format!("{} ({key}: undo)", self.message);
        }
    }

    /// The title, the lines and the action button of the notice of `pending`
    /// (the other button, the default, keeps).
    pub(super) fn confirm_text(&self, pending: &Pending) -> (&'static str, Vec<String>, String) {
        const NO_UNDO: &str = "This cannot be undone.";
        let (title, what, verb) = match pending {
            Pending::RemoveSample { kit, sample } => {
                let kit = self.kits.kits.get(*kit);
                let name = kit.and_then(|k| k.samples.get(*sample)).map_or("", |s| s.display_name.as_str());
                let what = format!("\"{name}\" from kit \"{}\". {NO_UNDO}", kit.map_or("", |k| k.name.as_str()));
                ("Remove the sample?", what, "remove")
            }
            Pending::RemoveSynth(key) => ("Delete the preset?", format!("\"{key}\". {NO_UNDO}"), "delete"),
            Pending::LoadSong(path) | Pending::ImportSong(path) | Pending::ImportLibrary(path) => {
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                ("Replace the library?", format!("{name} takes the place of every pattern and arrangement."), "replace")
            }
            Pending::DemoLibrary => (
                "Load the demo library?",
                "The demo takes the place of every pattern and arrangement.".to_owned(),
                "load",
            ),
            Pending::RestoreUserData(path) => {
                let what = format!("{} takes the place of the user files; they go to backups/ first.", path.display());
                ("Restore the backup?", what, "restore")
            }
            Pending::UserFolder(path) => ("Change the user data folder?", Self::user_folder_text(path), "change"),
        };
        // The default is to keep: ← → moves the buttons, ENTER runs the chosen.
        let action = format!("{} it", verb[..1].to_uppercase() + &verb[1..]);
        (title, vec![what, String::new(), "← →: choose   ENTER: the chosen".into()], action)
    }
}

#[cfg(test)]
mod tests {
    use ordrumbox_core::json::Value;

    use super::*;

    use crate::kits::KitSet;

    use crate::menu::Command;

    use crate::scale::Scale;

    use crate::view::Cursor;

    use ordrumbox_core::Library;

    use super::super::OverlayKey;

    fn session() -> Session {
        let (library, _) =
            Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{}]},{"name":"SNARE"}]}]}"#)
                .unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    #[test]
    fn an_edit_ctrl_z_undoes_is_done_at_once_and_says_so() {
        let mut s = session();
        let p = s.pattern_index;
        let tracks = s.pattern().unwrap().tracks.len();
        s.selection.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
        s.remove_track();
        assert!(!s.confirm_open());
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks - 1);
        assert_eq!(s.message, format!("Track deleted ({}+Z: undo)", crate::menu::command_label()));
        s.undo();
        assert_eq!(s.pattern().unwrap().tracks.len(), tracks);

        let c = Cursor { track: 0, beat: 0, step: 0 };
        s.selection.cursor = Some(c);
        assert!(s.editor.add_note(p, 0, 0, 0, 64.0).is_some());
        s.delete_step();
        assert!(!s.confirm_open());
        assert_eq!(s.step_note_count(c), 0);
        assert!(s.message.ends_with(&format!("({}+Z: undo)", crate::menu::command_label())), "{}", s.message);

        s.randomize_track();
        assert!(!s.confirm_open() && !s.track(0).unwrap().notes.is_empty());
        s.clear_track(0);
        assert!(!s.confirm_open() && s.track(0).unwrap().notes.is_empty());
        assert_eq!(s.message, format!("Cleared notes on \"KICK\" ({}+Z: undo)", crate::menu::command_label()));
        // Nothing done, nothing to undo.
        s.clear_track(0);
        assert_eq!(s.message, "No notes to clear");
        // A plugin's undo key is Alt+Z.
        s.host.hosted = true;
        s.randomize_track();
        assert!(s.message.ends_with("(Alt+Z: undo)"), "{}", s.message);
    }

    #[test]
    fn what_cannot_be_undone_waits_for_enter_and_a_click_cancels() {
        let mut s = session();
        s.run(Command::DemoLibrary);
        assert!(s.confirm_open());
        s.overlay_click(800, 600, Scale::ONE, 10, 10);
        assert!(!s.confirm_open());
        assert_eq!(s.pattern().unwrap().name.as_deref(), Some("p"));
    }

    #[test]
    fn a_master_change_says_ctrl_z_leaves_it() {
        let mut s = session();
        s.set_master_value("volume", 0.5);
        assert_eq!(s.message, "master: volume 0.5 (not undoable)");
        s.undo();
        assert_eq!(crate::master::get(&s.output_panel.master(), "volume"), 0.5);
    }

    #[test]
    fn the_mcp_tools_wait_for_the_answer() {
        let mut s = session();
        s.ask(Pending::DemoLibrary);
        assert!(s.mcp_call("listPatterns", &Value::Null).is_err());
        s.overlay_key(OverlayKey::Escape);
        assert!(s.mcp_call("listPatterns", &Value::Null).is_ok());
    }
}
