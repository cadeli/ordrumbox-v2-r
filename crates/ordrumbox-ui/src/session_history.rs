//! The edits' undo steps and the library's saving.

use super::*;

impl Session {
    /// After an edit: remember to save, show what happened.
    pub(super) fn edited(&mut self) {
        self.saving.library.touch();
        self.refresh_synths();
        if let Some(desc) = self.editor.history().next_undo() {
            self.message = desc.to_owned();
        }
    }

    /// Keep the pattern, cursor and selection inside the (changed) library.
    pub(super) fn clamp_selection(&mut self) {
        let count = self.editor.library().patterns.len();
        self.pattern_index = self.pattern_index.min(count.saturating_sub(1));
        let pattern = self.editor.library().patterns.get(self.pattern_index);
        self.selection.clamp(pattern);
    }

    pub fn undo(&mut self) {
        let shown = self.pattern().map(|p| p.id.clone());
        self.message = match self.editor.undo() {
            Some(desc) => format!("undone: {desc}"),
            None => "nothing to undo".into(),
        };
        self.after_history(shown);
    }

    pub fn redo(&mut self) {
        let shown = self.pattern().map(|p| p.id.clone());
        self.message = match self.editor.redo() {
            Some(desc) => format!("redone: {desc}"),
            None => "nothing to redo".into(),
        };
        self.after_history(shown);
    }

    /// After an undo or a redo: the pattern `shown` before stays shown where
    /// it moved to (a pattern moved, added or removed).
    pub(super) fn after_history(&mut self, shown: Option<String>) {
        if let Some(index) = shown.and_then(|id| self.editor.library().patterns.iter().position(|p| p.id == id)) {
            self.pattern_index = index;
        }
        self.selection.selected = None;
        self.clamp_selection();
        self.saving.library.touch();
        self.refresh_synths();
    }

    /// File > Save: the library written now.
    pub fn save(&mut self) {
        if self.saving.library_path.is_none() {
            self.message = if let Some(refusal) = Command::Save.hosted_refusal().filter(|_| self.hosted) {
                refusal.into()
            } else {
                "not saved: no library file (see the messages below)".into()
            };
            return;
        }
        if self.write_library() {
            self.message = "library saved".into();
        }
    }

    /// The library written to its file, without a word when it is (the menu
    /// bar shows it saved); whether it was. A failure is the message.
    pub(super) fn write_library(&mut self) -> bool {
        let Some(path) = self.saving.library_path.clone() else { return false };
        match store::save(&path, self.editor.library()) {
            Ok(()) => {
                self.saving.library_saved(self.library_revision());
                true
            }
            Err(e) => {
                self.saving.failed = true;
                self.message = format!("library not saved: {}: {e}", path.display());
                false
            }
        }
    }

    /// Where the library's edits stand, as the menu bar shows it.
    pub(crate) fn library_state(&self) -> menu::LibraryState {
        self.saving.library_state(self.library_revision(), self.hosted)
    }

    pub fn save_if_changed(&mut self) {
        if self.saving.library_unsaved(self.library_revision()) {
            self.write_library();
        }
    }

    /// Show an audio output error reported since the last call; whether there was one.
    pub fn check_audio(&mut self) -> bool {
        let Some(error) = self.playback.take_output_error() else { return false };
        self.message = format!("audio output: {error}");
        true
    }

    /// Save what is due (the library, kit knob edits); when the next save is.
    pub fn autosave(&mut self) -> Option<Instant> {
        let due = [self.flush_kits(), self.flush_master(), self.autosave_library()];
        due.into_iter().flatten().min()
    }

    pub(super) fn autosave_library(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.library_ready(self.library_revision()) {
            return next;
        }
        self.write_library();
        None
    }
}
