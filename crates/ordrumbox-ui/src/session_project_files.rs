//! Project files: Save to `songs/` in the user data folder, Save as and Export
//! to a folder picked, Load and Import, as `.odbox` files.

use std::time::{SystemTime, UNIX_EPOCH};

use ordrumbox_core::json::{self, Value};

use super::Session;
use super::session_confirm::Pending;
use crate::frame;
use crate::menu_action::MenuAction;
use crate::panel::{self, Rect};
use crate::panel::{Dropdown, Id};
use crate::store;
use ordrumbox_core::Library;
use std::path::Path;

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

impl Session {
    /// The project file of the library: its patterns, its songs, what is selected.
    pub(super) fn project_record(&self, stamp: &str) -> Value {
        let library = self.editor.library();
        library.project_record(&self.project_name(), self.pattern_index, self.editor.selected_song(), (stamp, now_ms()))
    }

    /// Save: the project to `songs/<name>.odbox` (`SongService.save`).
    pub(super) fn save_project(&mut self) {
        let Some(dir) = &self.saving.projects_dir else {
            self.message = "Save failed: no user data folder".into();
            return;
        };
        let name = self.project_name();
        let path = dir.join(format!("{}.odbox", store::safe_name(&name)));
        self.message = match store::save_project(&path, &self.project_record("savedAt")) {
            Ok(()) => format!("Project \"{name}\" saved"),
            Err(e) => format!("Save failed: {}: {e}", path.display()),
        };
    }

    /// Load: the projects saved, to choose from.
    pub(super) fn load_project_menu(&mut self) {
        let Some(dir) = &self.saving.projects_dir else {
            self.message = "No user data folder: no saved projects".into();
            return;
        };
        let files = match store::list_projects(dir) {
            Ok(files) => files,
            Err(e) => {
                self.message = format!("Projects not listed: {}: {e}", dir.display());
                return;
            }
        };
        if files.is_empty() {
            self.message = "No saved projects found".into();
            return;
        }
        let lines = files.into_iter().map(|f| {
            let label = f.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            (MenuAction::LoadProject(f), label)
        });
        let anchor = self.song_button_rect("load");
        self.chrome.widgets.open_dropdown(Dropdown {
            title: Some("Open Project".into()),
            ..Dropdown::from_actions(Id::SongMenu, anchor, lines, 0)
        });
    }

    /// Where a column button is (the Load list opens under it); without the
    /// song panel (the File menu), the middle of the window under the header.
    pub(super) fn song_button_rect(&self, id: &'static str) -> Rect {
        let (width, height, scale) = self.chrome.panel_size;
        let layout = self.panel_layout(width, height, scale);
        panel::find(&layout.items, Id::SongButton(id))
            .map_or_else(|| Rect::new(width / 2 - 100 * scale, frame::body_top(scale), 200 * scale, 0), |i| i.rect)
    }

    /// The file a project is written to (`<name>.odbox`).
    pub(super) fn project_file_name(&self) -> String {
        format!("{}.odbox", store::safe_name(&self.project_name()))
    }

    /// Save as: the project to `path`.
    pub(super) fn save_project_as(&mut self, path: &Path) {
        self.write_project(path, "savedAt", ("saved", "Save failed"));
    }

    /// Export: the project to `path`.
    pub(super) fn export_project(&mut self, path: &Path) {
        self.write_project(path, "exportedAt", ("exported", "not exported"));
    }

    /// The project to `path`, stamped under `stamp`; the message says it was
    /// `done` or that `failure` happened.
    fn write_project(&mut self, path: &Path, stamp: &str, (done, failure): (&str, &str)) {
        let name = self.project_name();
        self.message = match store::save_project(path, &self.project_record(stamp)) {
            Ok(()) => format!("Project \"{name}\" {done} to {}", path.display()),
            Err(e) => format!("{failure}: {}: {e}", path.display()),
        };
    }

    /// A saved project chosen in the Open list.
    pub(super) fn load_project_file(&mut self, path: &Path) {
        match self.read_project_file(path) {
            Ok(name) => self.message = format!("Project \"{name}\" opened"),
            Err(e) => self.message = e,
        }
    }

    /// Import (the file picker), or a `.odbox` dropped on the window.
    pub(super) fn import_project(&mut self, path: &Path) {
        self.ask(Pending::ImportProject(path.to_path_buf()));
    }

    pub(super) fn import_project_now(&mut self, path: &Path) {
        match self.read_project_file(path) {
            Ok(name) => self.message = format!("Project \"{name}\" imported"),
            Err(e) => self.message = e,
        }
    }

    /// `library` in the place of the library: one undo step, `description`.
    pub(super) fn put_library(&mut self, library: Library, description: &str) {
        self.editor.replace_library(library, 0, description);
        self.pattern_index = 0;
        self.song_panel.scroll = (0, 0);
        self.after_history(None);
    }

    /// Read a project file and put it in the place of the library
    /// (one undo step); its name.
    pub(super) fn read_project_file(&mut self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Load failed: {}: {e}", path.display()))?;
        let value = json::parse(&text).map_err(|e| format!("Load failed: {e}"))?;
        let infos = &self.editor.library().info;
        let record = ordrumbox_core::Library::from_project_record(&value, infos).ok_or("Invalid project file")?;
        let fallback = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let name = record.name.clone().unwrap_or(fallback);
        let mut library = record.library;
        library.info.insert("name", Value::from(name.as_str()));
        self.editor.load_library(library, record.selected_song);
        self.pattern_index = record.selected_pattern;
        self.song_panel.scroll = (0, 0);
        self.after_history(None);
        let dropped = &record.report.dropped_clips;
        if !dropped.is_empty() {
            let names: Vec<&str> = dropped.iter().map(|d| d.pattern.as_str()).collect();
            self.status.push(format!(
                "Ignored {} song clip(s) referencing an unknown pattern: {}",
                dropped.len(),
                names.join(", ")
            ));
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::super::OverlayKey;
    use super::*;
    use crate::session::session_song::tests::{clips, session};

    #[test]
    fn save_as_and_export_write_to_the_folder_picked() {
        let mut s = session();
        let dir = crate::test_support::scratch_dir("song-destination");
        s.chrome.last_dir = Some(dir.clone());
        let name = store::safe_name(&s.project_name());
        let path = dir.join(format!("{name}.odbox"));

        // Escape writes nothing.
        s.song_button("saveas");
        assert!(s.overlay_open());
        s.overlay_key(OverlayKey::Escape);
        assert!(!path.exists());

        for (id, stamp) in [("saveas", "savedAt"), ("export", "exportedAt")] {
            s.song_button(id);
            s.overlay_key(OverlayKey::Enter);
            assert!(!s.overlay_open());
            assert!(s.message.contains(&path.display().to_string()), "{}", s.message);
            assert!(std::fs::read_to_string(&path).unwrap().contains(stamp), "{id}");
            std::fs::remove_file(&path).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn replacing_a_file_is_confirmed() {
        let mut s = session();
        let dir = crate::test_support::scratch_dir("song-replace");
        s.chrome.last_dir = Some(dir.clone());
        let path = dir.join(s.project_file_name());
        std::fs::write(&path, "old").unwrap();

        // Escape keeps the file.
        s.song_button("export");
        s.overlay_key(OverlayKey::Enter);
        assert!(s.overlay_open());
        s.overlay_key(OverlayKey::Escape);
        assert!(!s.overlay_open());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");

        // Enter is the default button: it keeps too.
        s.song_button("export");
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");

        // Arrows move to "Replace it", Enter runs it.
        s.song_button("export");
        s.overlay_key(OverlayKey::Enter);
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        assert!(std::fs::read_to_string(&path).unwrap().contains("exportedAt"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn project_files_load_as_one_undo_step() {
        let mut s = session();
        let dir = crate::test_support::scratch_dir("song-load");
        let path = dir.join("lib.odbox");
        store::save_project(&path, &s.project_record("savedAt")).unwrap();
        s.editor.add_pattern_at_measure("a", 5, None, None).unwrap();
        s.commit_text(Id::ProjectName, "changed");
        s.load_project_file(&path);
        assert_eq!(s.message, "Project \"lib\" opened");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        assert_eq!(s.editor.history().next_undo(), Some("Open project"));
        s.undo();
        assert_eq!(clips(&s).len(), 2);
        assert_eq!(s.project_name(), "changed");
        std::fs::write(&path, r#"{"songs":[]}"#).unwrap();
        s.import_project(&path);
        // The confirm's default keeps: ENTER closes it, the import did not run.
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        // Confirming it runs it, and the invalid file is reported.
        s.import_project(&path);
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.message, "Invalid project file");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
