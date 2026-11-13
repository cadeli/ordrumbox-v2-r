//! The files written or read on request: the WAV, MIDI and pattern JSON
//! exports, the MIDI import, the file picker that names them, and the files
//! dropped on the window.

use super::Session;
use super::session_confirm::Pending;
use super::session_overlay::Overlay;
use super::session_user_data;
use crate::picker::{Export, Outcome, Picker, Purpose};
use crate::store;
use ordrumbox_core::{Library, Pattern, Rng};
use ordrumbox_engine::offline;
use ordrumbox_engine::{Mode, wav};
use ordrumbox_logic::generate::{self, AutoGenerator};
use ordrumbox_logic::{compose, midi};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Why a WAV export is refused.
pub(super) const NOTHING_TO_RENDER: &str = "nothing to render with (no samples loaded)";

/// Why the synth sounds cannot be exported.
pub(super) const NO_SYNTHS: &str = "No synth sounds loaded";

/// The file the synth sounds are exported to.
pub(super) const SYNTHS_FILE_NAME: &str = "ordrumbox-synth-sounds.json";

/// The file a pattern is exported to as JSON (`ordrumbox-<name>.json`).
fn pattern_json_file_name(pattern: &Pattern) -> String {
    format!("ordrumbox-{}.json", store::safe_name(pattern.name.as_deref().unwrap_or("pattern")))
}

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
        // A folder made for the export is never written into one already there.
        if path.exists() && matches!(export, Export::KitFolder | Export::UserData) {
            self.message = format!("not exported: {} already exists", path.display());
        } else if path.exists() {
            self.chrome.overlay = Some(Overlay::ConfirmOverwrite { export, path, yes: false });
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
            Export::KitFolder => self.message = self.kits.export_folder(self.kit_index, path).unwrap_or_else(|e| e),
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
                "wav" => match self.kit_editor.sample {
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
                self.kit_editor.sample = Some(sample);
                self.kits_changed(message);
            }
            Err(e) => self.message = e,
        }
    }
}

impl Session {
    /// What `mode` plays (a pattern once, a song whole) as a WAV file at
    /// `path`; what was written.
    pub(super) fn write_wav(&self, mode: Mode, path: &Path) -> Result<String, String> {
        let setup = self.render.as_ref().ok_or(NOTHING_TO_RENDER)?;
        let library = self.editor.library();
        let started = Instant::now();
        let rendered = match mode {
            Mode::Song(index) => {
                if library.songs.get(index).is_none() {
                    return Err("no song in the library".into());
                }
                offline::render_song(library, index, setup)
            }
            Mode::Pattern(p) => {
                // Tracks in auto mode get their new lines loop after loop, on the render's copy.
                let (scales, mut rng) = (&self.generators.scales, Rng::new(setup.seed));
                let styles = compose::learn(&self.generators.styles, &library.patterns);
                let mut loop_start = |library: &mut Library, p: usize, number: u64| {
                    if let Some(pattern) = library.patterns.get_mut(p).filter(|p| generate::plays_auto(p)) {
                        let mut rnd = || rng.next_f64();
                        generate::run_loop(&mut AutoGenerator::new(scales, &styles, &mut rnd), pattern, number);
                    }
                };
                offline::render_pattern(library, p, 1, setup, Some(&mut loop_start))
            }
        };
        let (left, right) = rendered.ok_or("nothing to export (no playable clip)")?;
        let bytes = wav::encode(&left, &right, setup.sample_rate);
        store::write_atomic(path, bytes).map_err(|e| format!("not exported: {}: {e}", path.display()))?;
        Ok(format!(
            "exported {} ({:.1} s of audio in {:.1} s)",
            path.display(),
            left.len() as f64 / f64::from(setup.sample_rate),
            started.elapsed().as_secs_f64()
        ))
    }

    /// Pattern `p` as a MIDI file at `path`; what was written.
    pub(super) fn write_midi(&self, p: usize, path: &Path) -> Result<String, String> {
        let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
        let mut rng = Rng::new(1);
        let bpm = self.editor.library().bpm;
        let bytes = midi::export(pattern, bpm, 1, &self.roots, &mut rng, &self.generators.scales);
        store::write_atomic(path, &bytes).map_err(|e| format!("not exported: {}: {e}", path.display()))?;
        Ok(format!("exported {} ({} bytes)", path.display(), bytes.len()))
    }

    /// Pattern `p` as JSON at `path`.
    pub(super) fn write_pattern_json(&self, p: usize, path: &Path) -> Result<(), String> {
        let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
        let text = pattern.to_json().to_json_pretty(2) + "\n";
        store::write_atomic(path, text).map_err(|e| format!("not exported: {}: {e}", path.display()))
    }

    /// New patterns from the MIDI file at `path`, the last one shown; what
    /// the import says.
    pub(super) fn import_midi_file(&mut self, path: &Path) -> Result<String, String> {
        let name = path.file_name().map_or_else(|| "midi".into(), |n| n.to_string_lossy().into_owned());
        let bytes = std::fs::read(path).map_err(|e| format!("not imported: {}: {e}", path.display()))?;
        let (index, message) =
            self.editor.import_midi(&name, &bytes, &self.roots).map_err(|reason| format!("{name}: {reason}"))?;
        self.pattern_index = index;
        self.deselect();
        self.edited();
        self.message.clone_from(&message);
        self.chrome.current_page = 0;
        Ok(message)
    }

    /// Ask for the folder to write `export` to: the last one picked, at first
    /// the exports folder. What cannot be exported is said at once.
    pub(super) fn open_export(&mut self, export: Export) {
        let refusal = match export {
            Export::PatternWav | Export::SongWav if self.render.is_none() => Some(NOTHING_TO_RENDER),
            Export::Synths if self.synths.presets.is_empty() => Some(NO_SYNTHS),
            _ => None,
        };
        if let Some(refusal) = refusal {
            self.message = refusal.into();
            return;
        }
        let start = self.chrome.last_dir.clone().filter(|d| d.is_dir()).or_else(store::exports_dir);
        self.chrome.overlay = Some(Overlay::Picker(Picker::new(Purpose::Export(export), start.as_deref())));
    }

    /// The name of the file `export` writes for pattern `p` or song `s`.
    pub(super) fn export_file_name_of(&self, export: Export, p: usize, s: usize) -> Option<String> {
        let library = self.editor.library();
        let pattern = library.patterns.get(p);
        // Ids come as they are from the files loaded: one with `/` or `..`
        // must not lead the write out of the folder picked.
        Some(match export {
            Export::PatternWav => format!("ordrumbox-{}.wav", store::safe_name(&pattern?.id)),
            Export::SongWav => format!("ordrumbox-song-{}.wav", store::safe_name(&library.songs.get(s)?.id)),
            Export::PatternMidi => format!("ordrumbox-{}.mid", store::safe_name(&pattern?.id)),
            Export::PatternJson => pattern_json_file_name(pattern?),
            Export::Song | Export::SongSaveAs => self.song_file_name(),
            Export::Synths => SYNTHS_FILE_NAME.to_owned(),
            Export::KitMapping => self.kits.mapping_file_name(self.kit_index)?,
            Export::KitFolder => self.kits.folder_name(self.kit_index)?,
            Export::Library => session_user_data::LIBRARY_FILE_NAME.to_owned(),
            Export::UserData => crate::user_data::backup_name(&crate::user_data::now_stamp()),
        })
    }
}
