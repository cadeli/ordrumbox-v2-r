//! Song files: Save to `songs/` in the user data folder, Save as and Export
//! to a folder picked, Load and Import, as `.odbox` files.

use std::time::{SystemTime, UNIX_EPOCH};

use ordrumbox_core::json::{self, Value};

use super::*;
use crate::panel::{self, Rect};

fn now_ms() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_millis() as f64)
}

impl Session {
    /// The song file of the library: its patterns, its arrangements, what is selected.
    pub(super) fn song_record(&self, stamp: &str) -> Value {
        let library = self.editor.library();
        library.song_record(&self.song_name(), self.pattern_index, self.editor.selected_song(), (stamp, now_ms()))
    }

    /// Save: the song to `songs/<name>.odbox` (`SongService.save`).
    pub(super) fn save_song(&mut self) {
        let Some(dir) = &self.saving.songs_dir else {
            self.message = "Save failed: no user data folder".into();
            return;
        };
        let name = self.song_name();
        let path = dir.join(format!("{}.odbox", store::safe_name(&name)));
        self.message = match store::save_song(&path, &self.song_record("savedAt")) {
            Ok(()) => format!("Song \"{name}\" saved"),
            Err(e) => format!("Save failed: {}: {e}", path.display()),
        };
    }

    /// Load: the songs saved, to choose from.
    pub(super) fn load_song_menu(&mut self) {
        let Some(dir) = &self.saving.songs_dir else {
            self.message = "No user data folder: no saved songs".into();
            return;
        };
        let files = match store::list_songs(dir) {
            Ok(files) => files,
            Err(e) => {
                self.message = format!("Songs not listed: {}: {e}", dir.display());
                return;
            }
        };
        if files.is_empty() {
            self.message = "No saved songs found".into();
            return;
        }
        let labels = files.iter().map(|f| f.file_stem().unwrap_or_default().to_string_lossy().into_owned()).collect();
        let values = files.iter().map(|f| format!("load:{}", f.display())).collect();
        let anchor = self.song_button_rect("load");
        self.chrome.widgets.dropdown = Some(Dropdown {
            title: Some("Load Song".into()),
            ..Dropdown::new(Id::SongMenu, anchor, values, labels, 0)
        });
    }

    /// Where a column button is (the Load list opens under it); without the
    /// song panel (the Song menu), the middle of the window under the header.
    pub(super) fn song_button_rect(&self, id: &'static str) -> Rect {
        let (width, height, scale) = self.chrome.panel_size;
        let layout = self.panel_layout(width, height, scale);
        panel::find(&layout.items, Id::SongButton(id))
            .map_or_else(|| Rect::new(width / 2 - 100 * scale, frame::body_top(scale), 200 * scale, 0), |i| i.rect)
    }

    /// The file a song is written to (`<name>.odbox`).
    pub(super) fn song_file_name(&self) -> String {
        format!("{}.odbox", store::safe_name(&self.song_name()))
    }

    /// Save as: the song to `path`.
    pub(super) fn save_song_as(&mut self, path: &Path) {
        let name = self.song_name();
        self.message = match store::save_song(path, &self.song_record("savedAt")) {
            Ok(()) => format!("Song \"{name}\" saved to {}", path.display()),
            Err(e) => format!("Save failed: {}: {e}", path.display()),
        };
    }

    /// Export: the song to `path`.
    pub(super) fn export_song(&mut self, path: &Path) {
        let name = self.song_name();
        self.message = match store::save_song(path, &self.song_record("exportedAt")) {
            Ok(()) => format!("Song \"{name}\" exported to {}", path.display()),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// A saved song chosen in the Load list.
    pub(super) fn load_song_file(&mut self, path: &Path) {
        match self.read_song_file(path) {
            Ok(name) => self.message = format!("Song \"{name}\" loaded"),
            Err(e) => self.message = e,
        }
    }

    /// Import (the file picker), or a `.odbox` dropped on the window.
    pub(super) fn import_song(&mut self, path: &Path) {
        self.ask(Pending::ImportSong(path.to_path_buf()));
    }

    pub(super) fn import_song_now(&mut self, path: &Path) {
        match self.read_song_file(path) {
            Ok(name) => self.message = format!("Song \"{name}\" imported"),
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

    /// Read a song file and put it in the place of the library
    /// (`parseImportedFile`, `applyToAppState`: one undo step); its name.
    pub(super) fn read_song_file(&mut self, path: &Path) -> Result<String, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Load failed: {}: {e}", path.display()))?;
        let value = json::parse(&text).map_err(|e| format!("Load failed: {e}"))?;
        let infos = &self.editor.library().info;
        let record = ordrumbox_core::Library::from_song_record(&value, infos).ok_or("Invalid song file")?;
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
    use super::*;
    use crate::session::session_song::tests::{clips, session};

    #[test]
    fn save_as_and_export_write_to_the_folder_picked() {
        let mut s = session();
        let dir = crate::test_support::scratch_dir("song-destination");
        s.chrome.last_dir = Some(dir.clone());
        let name = store::safe_name(&s.song_name());
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
        let path = dir.join(s.song_file_name());
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
    fn song_files_load_as_one_undo_step() {
        let mut s = session();
        let dir = crate::test_support::scratch_dir("song-load");
        let path = dir.join("lib.odbox");
        store::save_song(&path, &s.song_record("savedAt")).unwrap();
        s.editor.add_pattern_at_measure("a", 5, None, None).unwrap();
        s.commit_text(Id::SongName, "changed");
        s.load_song_file(&path);
        assert_eq!(s.message, "Song \"lib\" loaded");
        assert_eq!(clips(&s), [("b".into(), 0, 2.0)]);
        assert_eq!(s.editor.history().next_undo(), Some("Load song"));
        s.undo();
        assert_eq!(clips(&s).len(), 2);
        assert_eq!(s.song_name(), "changed");
        std::fs::write(&path, r#"{"songs":[]}"#).unwrap();
        s.import_song(&path);
        // The confirm's default keeps: ENTER closes it, the import did not run.
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        // Confirming it runs it, and the invalid file is reported.
        s.import_song(&path);
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(s.message, "Invalid song file");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
