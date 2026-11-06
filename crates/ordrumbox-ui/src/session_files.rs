//! The files written or read on request: the WAV, MIDI and pattern JSON
//! exports, the MIDI import, the file picker that names them, and the files
//! dropped on the window.

use super::*;

impl Session {
    /// Ctrl+E: the selected pattern (with Shift: the selected song) to a WAV
    /// file at `path`.
    fn export_wav(&mut self, song: bool, path: &Path) {
        let mode = if song { Mode::Song(self.editor.selected_song()) } else { Mode::Pattern(self.pattern_index) };
        self.message = self.write_wav(mode, path).unwrap_or_else(|e| e);
    }

    /// Ctrl+M: the selected pattern to a MIDI file at `path`.
    fn export_midi(&mut self, path: &Path) {
        if self.pattern().is_none() {
            return;
        }
        self.message = self.write_midi(self.pattern_index, path).unwrap_or_else(|e| e);
    }

    /// Ctrl+Shift+S: the pattern as `Exporter.export()`
    /// writes it, to `path`.
    fn export_pattern_json(&mut self, path: &Path) {
        self.message = match self.write_pattern_json(self.pattern_index, path) {
            Ok(()) => format!(
                "Pattern \"{}\" exported to {}",
                self.pattern().and_then(|p| p.name.as_deref()).unwrap_or("pattern"),
                path.display()
            ),
            Err(e) => e,
        };
    }

    /// A MIDI file dropped on the window (or given on the command line):
    /// new patterns from it, the last one selected.
    pub fn import_midi(&mut self, path: &Path) {
        if let Err(e) = self.import_midi_file(path) {
            self.message = e;
        }
    }

    pub(super) fn open_picker(&mut self, purpose: Purpose) {
        self.chrome.overlay = Some(Overlay::Picker(Picker::new(purpose, self.chrome.last_dir.as_deref())));
    }

    /// The name of the file `export` writes; `None` when what it writes is gone.
    pub(super) fn export_file_name(&self, export: Export) -> Option<String> {
        self.export_file_name_of(export, self.pattern_index, self.editor.selected_song())
    }

    /// The folder is picked: write `export` there, once confirmed if that
    /// replaces a file.
    pub(super) fn export_to(&mut self, export: Export, dir: &Path) {
        let Some(name) = self.export_file_name(export) else {
            self.message = "nothing to export".into();
            return;
        };
        let path = dir.join(name);
        if path.exists() {
            self.chrome.overlay = Some(Overlay::ConfirmOverwrite { export, path });
        } else {
            self.write_export(export, &path);
        }
    }

    pub(super) fn write_export(&mut self, export: Export, path: &Path) {
        match export {
            Export::PatternWav => self.export_wav(false, path),
            Export::SongWav => self.export_wav(true, path),
            Export::PatternMidi => self.export_midi(path),
            Export::PatternJson => self.export_pattern_json(path),
            Export::Song => self.export_song(path),
            Export::SongSaveAs => self.save_song_as(path),
            Export::Synths => self.export_synths(path),
            Export::KitMapping => self.kit_export(path),
            Export::Library => self.export_library(path),
            Export::UserData => self.back_up_user_data(path),
        }
    }

    pub(super) fn picker_outcome(&mut self, outcome: Outcome) {
        let Outcome::Picked(path) = outcome else { return };
        let Some(Overlay::Picker(picker)) = self.chrome.overlay.take() else { return };
        self.chrome.last_dir = picker.dir.clone();
        self.picked(picker.purpose, path);
    }

    /// `path` is picked for `purpose`.
    pub(super) fn picked(&mut self, purpose: Purpose, path: PathBuf) {
        match purpose {
            Purpose::ImportMidi => self.import_midi(&path),
            Purpose::ImportSong => self.import_song(&path),
            Purpose::ImportSynth => self.import_synths(&path),
            Purpose::ReplaceTrackSample => self.replace_track_sample(&path),
            Purpose::ReplaceSample => self.drop_path(&path, true),
            Purpose::AddSample | Purpose::LoadMapping | Purpose::ImportFolder => self.drop_path(&path, false),
            Purpose::Export(export) => self.export_to(export, &path),
            Purpose::ImportLibrary => self.ask(Pending::ImportLibrary(path)),
            Purpose::RestoreUserData => self.ask_restore(path),
            Purpose::UserFolder => self.ask_user_folder(path),
        }
    }

    /// A file or folder dropped on the window (Shift: a WAV replaces the
    /// selected sample of the kit editor).
    pub fn drop_path(&mut self, path: &Path, shift: bool) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let result = if path.is_dir() {
            self.kits.import_folder(path).map(|(kit, message)| (kit, 0, message))
        } else {
            match ext.as_str() {
                "mid" | "midi" => return self.import_midi(path),
                "odbox" => return self.import_song(path),
                "json" => self.kits.load_mapping(path).map(|(kit, message)| (kit, 0, message)),
                "wav" => match self.kit_editor {
                    Some(sample) if shift => self
                        .kits
                        .replace_sample(self.kit_index, sample, path)
                        .map(|message| (self.kit_index, sample, message)),
                    _ => self
                        .kits
                        .add_sample(self.kit_index, path)
                        .map(|(sample, message)| (self.kit_index, sample, message)),
                },
                _ => Err(format!("{}: not a MIDI, WAV, song or kit file", path.display())),
            }
        };
        match result {
            Ok((kit, sample, message)) => {
                self.kit_index = kit;
                self.kit_editor = Some(sample);
                self.kits_changed(message);
            }
            Err(e) => self.message = e,
        }
    }
}
