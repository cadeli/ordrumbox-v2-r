//! The user's files from the File menu (`user_data.rs`): the library
//! exported to a file and imported from one, the demo library loaded again,
//! the user data folder backed up, restored, changed and opened.
//!
//! What replaces the library is one undo step. What replaces the user's
//! files moves the ones there to `backups/` first, then loads the session
//! again from the files, as at start.

use super::*;
use crate::store;
use crate::user_data;
use ordrumbox_core::json::Object;

/// The file File > Export library writes: never named as the library the
/// app saves to, so an export to the user data folder replaces nothing.
pub(super) const LIBRARY_FILE_NAME: &str = "ordrumbox-library.json";

pub(super) const NO_USER_FOLDER: &str = "no user data folder";

/// A library file read: the library, and how many values were repaired.
fn read_library(path: &Path) -> Result<(Library, usize), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let (library, report) = Library::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((library, report.issues.len()))
}

/// `message`, with the values repaired when there are some.
fn with_repaired(message: String, repaired: usize) -> String {
    if repaired == 0 { message } else { format!("{message} ({repaired} value(s) repaired)") }
}

impl Session {
    /// The user data folder: the library file's, else the one the store names.
    pub(super) fn user_folder(&self) -> Option<PathBuf> {
        self.saving.library_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(store::user_dir)
    }

    /// The assets' library file (the demo).
    fn demo_library_path(&self) -> Option<PathBuf> {
        Some(self.assets_dir.as_ref()?.join("data").join(assets::LIBRARY))
    }

    /// Whether the demo library can be loaded.
    pub(super) fn has_demo(&self) -> bool {
        self.assets_dir.is_some()
    }

    /// File > Export library: the library to `path`, as the app saves it.
    pub(super) fn export_library(&mut self, path: &Path) {
        self.message = match store::save(path, self.editor.library()) {
            Ok(()) => format!("library exported to {}", path.display()),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// File > Import library, once confirmed: its synth presets join the
    /// presets, replacing those of the same name; one undo step.
    pub(super) fn import_library_now(&mut self, path: &Path) {
        self.message = match read_library(path) {
            Ok((mut library, repaired)) => {
                let mut synths = self.editor.library().synths.clone().unwrap_or_default();
                let imported: Vec<_> =
                    library.synths.iter().flat_map(Object::iter).filter(|(_, p)| !p.is_null()).collect();
                let presets = match imported.len() {
                    0 => String::new(),
                    count => format!(", {count} synth preset(s)"),
                };
                for (key, preset) in imported {
                    synths.insert(key, preset.clone());
                }
                library.synths = Some(synths);
                self.put_library(library, "Import library");
                with_repaired(format!("library imported from {}{presets}", path.display()), repaired)
            }
            Err(e) => format!("not imported: {e}"),
        };
    }

    /// File > Load the demo library, once confirmed.
    pub(super) fn load_demo_library_now(&mut self) {
        let Some(path) = self.demo_library_path() else {
            self.message = "no assets folder: no demo library".into();
            return;
        };
        self.message = match read_library(&path) {
            Ok((library, repaired)) => {
                self.put_library(library, "Load the demo library");
                with_repaired("demo library loaded (Edit > Undo brings yours back)".to_owned(), repaired)
            }
            Err(e) => format!("demo library not loaded: {e}"),
        };
    }

    /// Write now what waits to be written (library, kits, synth presets,
    /// master), before the user's files are copied or moved.
    pub(super) fn write_pending(&mut self) {
        self.saving.expire_all();
        self.autosave();
        self.save_if_changed();
    }

    /// The session loaded again from the user's files, as at start: the
    /// sound output, the update check's waker, the MCP link and the action
    /// log kept; `message` shown.
    pub(super) fn reload(&mut self, message: String) {
        let action_log = std::mem::take(&mut self.action_log);
        let waker = self.updates.take_waker();
        let mcp = self.mcp.take();
        let start_output = self.playback.take_start_output().unwrap_or_else(|| Box::new(no_output));
        let offers = self.playback.offers();
        *self = Self::load(start_output);
        *self.playback.offers_mut() = offers;
        self.action_log = action_log;
        if let Some(waker) = waker {
            self.updates.keep_waker(waker);
        }
        // The folder may have changed: `ordrumbox --mcp` looks for the link in the new one.
        if let Some(mcp) = mcp {
            match store::mcp_endpoint_path().ok_or_else(|| NO_USER_FOLDER.to_owned()).and_then(|p| mcp.announce(&p)) {
                Ok(()) => {}
                Err(e) => self.status.push(format!("MCP not available: {e}")),
            }
            self.mcp = Some(mcp);
        }
        self.message = message;
    }

    /// File > Back up user data: the user's files to folder `to`.
    pub(super) fn back_up_user_data(&mut self, to: &Path) {
        let Some(dir) = self.user_folder() else {
            self.message = format!("not backed up: {NO_USER_FOLDER}");
            return;
        };
        self.write_pending();
        self.message = match user_data::back_up(&dir, to) {
            Ok(names) => format!("user data backed up to {} ({})", to.display(), names.join(", ")),
            Err(e) => format!("not backed up: {e}"),
        };
    }

    /// File > Restore user data: pick the backup's folder, from the files
    /// moved aside when there are some.
    pub(super) fn open_restore(&mut self) {
        let backups = self.user_folder().map(|d| d.join(user_data::BACKUPS_DIR)).filter(|d| d.is_dir());
        let start = backups.or_else(|| self.chrome.last_dir.clone());
        self.chrome.overlay = Some(Overlay::Picker(Picker::new(Purpose::RestoreUserData, start.as_deref())));
    }

    /// The backup's folder is picked: asked, unless it holds nothing to restore.
    pub(super) fn ask_restore(&mut self, backup: PathBuf) {
        if user_data::present(&backup).is_empty() {
            self.message = format!("not restored: no orDrumbox user files in {}", backup.display());
            return;
        }
        self.ask(Pending::RestoreUserData(backup));
    }

    /// File > Restore user data, once confirmed: the user's files moved
    /// aside, the backup's put in their place, the session loaded again.
    pub(super) fn restore_user_data_now(&mut self, backup: &Path) {
        let Some(dir) = self.user_folder() else {
            self.message = format!("not restored: {NO_USER_FOLDER}");
            return;
        };
        self.write_pending();
        match user_data::restore(backup, &dir, &user_data::now_stamp()) {
            Ok(restored) => {
                let kept = restored
                    .aside
                    .map_or_else(String::new, |a| format!("; the files before it are in {}", a.folder.display()));
                self.reload(format!("restored from {} ({}){kept}", backup.display(), restored.names.join(", ")));
            }
            Err(e) => self.message = format!("not restored: {e}"),
        }
    }

    /// Why the user data folder cannot be changed from the app, if it cannot.
    fn user_folder_refusal() -> Option<String> {
        store::env_library().map(|file| {
            format!(
                "{} names the library file ({}): its folder is the user data folder",
                store::ENV_VAR,
                file.display()
            )
        })
    }

    /// File > Change user data folder: pick it, from the current one's parent.
    pub(super) fn open_user_folder_picker(&mut self) {
        if let Some(refusal) = Self::user_folder_refusal() {
            self.message = refusal;
            return;
        }
        let start = self.user_folder().and_then(|d| d.parent().map(Path::to_path_buf));
        self.chrome.overlay = Some(Overlay::Picker(Picker::new(Purpose::UserFolder, start.as_deref())));
    }

    /// File > Use the default user data folder.
    pub(super) fn use_default_user_folder(&mut self) {
        if let Some(refusal) = Self::user_folder_refusal() {
            self.message = refusal;
            return;
        }
        match store::default_dir() {
            Some(dir) => self.ask_user_folder(dir),
            None => self.message = "no default user data folder on this system".into(),
        }
    }

    /// Folder `dir` is picked as the user data folder: asked, unless it is already.
    pub(super) fn ask_user_folder(&mut self, dir: PathBuf) {
        if self.user_folder().is_some_and(|current| store::same_folder(&current, &dir)) {
            self.message = format!("{} is already the user data folder", dir.display());
            return;
        }
        self.ask(Pending::UserFolder(dir));
    }

    /// The lines of the notice asking to use `dir` as the user data folder.
    pub(super) fn user_folder_text(dir: &Path) -> String {
        if user_data::present(dir).is_empty() {
            format!("The user files are copied to {}; the current folder stays as it is.", dir.display())
        } else {
            format!("The user files already in {} are used; the current folder stays as it is.", dir.display())
        }
    }

    /// The user data folder is `dir` from now on, once confirmed: the
    /// user's files copied there when it holds none, the session loaded
    /// again from it.
    pub(super) fn change_user_folder_now(&mut self, dir: &Path) {
        if let Some(refusal) = Self::user_folder_refusal() {
            self.message = refusal;
            return;
        }
        self.write_pending();
        let current = self.user_folder();
        let copied = match current.filter(|_| user_data::present(dir).is_empty()) {
            Some(current) => match user_data::copy_files(&current, dir) {
                Ok(names) => names,
                Err(e) => {
                    self.message = format!("folder not changed: {}: {e}", dir.display());
                    return;
                }
            },
            None => Vec::new(),
        };
        if let Err(e) = store::choose_dir(dir) {
            self.message = format!("folder not changed: {e}");
            return;
        }
        let how = if copied.is_empty() { "its own files".to_owned() } else { format!("copied: {}", copied.join(", ")) };
        self.reload(format!("user data folder: {} ({how})", dir.display()));
    }

    /// File > Open user data folder: in the Finder or the Explorer (made
    /// first if it is not there yet).
    pub(super) fn open_user_folder(&mut self) {
        let Some(dir) = self.user_folder() else {
            self.message = NO_USER_FOLDER.into();
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.message = format!("{}: {e}", dir.display());
            return;
        }
        self.message = match update::open_with_system(&dir.display().to_string()) {
            Ok(()) => format!("opened {}", dir.display()),
            Err(e) => e,
        };
    }

    /// Help > Factory reset, once confirmed: the user's files moved to
    /// `backups/`, the session loaded again from the assets, as at first start.
    pub fn factory_reset(&mut self) {
        if let Some(refusal) = Command::FactoryReset.hosted_refusal().filter(|_| self.hosted) {
            self.message = refusal.into();
            return;
        }
        self.chrome.close_popups();
        let Some(dir) = self.user_folder() else {
            self.message = format!("factory reset: {NO_USER_FOLDER}");
            return;
        };
        self.write_pending();
        match user_data::move_aside(&dir, &user_data::now_stamp(), "reset") {
            Ok(aside) => self.reload(match aside {
                None => "factory settings restored (no user files found)".into(),
                Some(aside) => format!(
                    "factory settings restored ({} moved to {}: File > Restore user data brings them back)",
                    aside.names.join(", "),
                    aside.folder.display()
                ),
            }),
            Err(e) => self.message = format!("factory reset failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::test_support::scratch_dir;

    const MINE: &str = r#"{"patterns":[{"name":"mine","tracks":[{"name":"KICK","notes":[{}]}]}]}"#;

    fn session() -> Session {
        let mut s = Session::new(Library::parse(MINE).unwrap().0, KitSet::empty(), Vec::new());
        s.assets_dir = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets"));
        s
    }

    fn first_pattern(s: &Session) -> Option<&str> {
        s.editor.library().patterns.first().and_then(|p| p.name.as_deref())
    }

    #[test]
    fn the_library_goes_out_to_a_file_and_comes_back_in_one_undo_step() {
        let dir = scratch_dir("session-data-library");
        let mut s = session();
        // A preset of mine: it goes out with the library.
        let lead = ordrumbox_core::json::parse(r#"{"vco1":{"gain":0.5}}"#).unwrap();
        s.put_synth("lead", Some(lead.clone()), "New lead".into(), None);
        s.open_export(Export::Library);
        s.export_to(Export::Library, &dir);
        let path = dir.join(LIBRARY_FILE_NAME);
        assert_eq!(fs::read_to_string(&path).unwrap(), s.library_text(), "{}", s.message);
        assert!(s.library_text().contains(r#""lead": {"#));

        // The demo in its place, after a yes; Undo brings mine back.
        s.run(Command::DemoLibrary);
        assert!(s.confirm_open());
        // ENTER alone keeps: nothing ran, the library is mine.
        s.overlay_key(OverlayKey::Enter);
        assert!(!s.overlay_open());
        assert_eq!(first_pattern(&s), Some("mine"));
        // Asked again, the action is chosen with ← → and runs with ENTER.
        s.run(Command::DemoLibrary);
        assert!(s.confirm_open());
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert!(s.message.starts_with("demo library loaded"), "{}", s.message);
        assert_ne!(first_pattern(&s), Some("mine"));
        assert_eq!(s.editor.history().next_undo(), Some("Load the demo library"));
        s.run(Command::Undo);
        assert_eq!(first_pattern(&s), Some("mine"));

        // The demo again, then the file exported: mine.
        s.load_demo_library_now();
        s.put_synth("lead", Some(ordrumbox_core::json::parse("{}").unwrap()), "Other lead".into(), None);
        s.picked(Purpose::ImportLibrary, path.clone());
        assert!(s.confirm_open());
        s.overlay_key(OverlayKey::Move(1));
        s.overlay_key(OverlayKey::Enter);
        assert_eq!(first_pattern(&s), Some("mine"), "{}", s.message);
        assert_eq!(s.editor.history().next_undo(), Some("Import library"));
        // Its preset comes back with it, in the place of the one of that name.
        assert!(s.message.ends_with(", 1 synth preset(s)"), "{}", s.message);
        assert_eq!(s.synths.get("lead"), Some(&lead));
        // Undone with the library.
        s.undo();
        assert_eq!(s.synths.get("lead").map(ordrumbox_core::json::Value::to_json).as_deref(), Some("{}"));
        s.redo();

        // A file that is not a library: said, the library kept.
        fs::write(&path, "{").unwrap();
        s.import_library_now(&path);
        assert!(s.message.starts_with("not imported: "), "{}", s.message);
        assert_eq!(first_pattern(&s), Some("mine"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_without_user_files_is_not_offered_for_a_restore() {
        let dir = scratch_dir("session-data-restore-empty");
        let mut s = session();
        s.picked(Purpose::RestoreUserData, dir.clone());
        assert!(!s.confirm_open());
        assert!(s.message.starts_with("not restored: no orDrumbox user files"), "{}", s.message);
        fs::write(dir.join("kits.json"), "[]").unwrap();
        s.picked(Purpose::RestoreUserData, dir.clone());
        assert!(s.confirm_open());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_plugin_leaves_the_user_data_folder_alone() {
        let mut s = session();
        s.hosted = true;
        for command in [
            Command::BackUpUserData,
            Command::RestoreUserData,
            Command::ChangeUserFolder,
            Command::DefaultUserFolder,
            Command::OpenUserFolder,
        ] {
            s.run(command);
            assert_eq!(s.message, menu::USER_DATA_IN_THE_APP, "{command:?}");
            assert!(!s.overlay_open());
        }
    }

    #[test]
    fn the_folder_asked_for_says_whether_its_files_are_used() {
        let dir = scratch_dir("session-data-ask");
        assert!(Session::user_folder_text(&dir).starts_with("The user files are copied to"));
        fs::write(dir.join("library.json"), MINE).unwrap();
        assert!(Session::user_folder_text(&dir).starts_with("The user files already in"));
        fs::remove_dir_all(dir).unwrap();
    }
}
