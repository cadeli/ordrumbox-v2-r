//! The files written or read on request: the WAV, MIDI and pattern JSON
//! exports, the MIDI import, the file picker that names them, and the files
//! dropped on the window.

use super::Session;
use super::session_confirm::Pending;
use super::session_overlay::Overlay;
use super::session_user_data;
use crate::file_request::FileRequest;
use crate::picker::{Export, Outcome, Picker, Purpose};
use crate::store;
use ordrumbox_core::{Library, Pattern, Rng};
use ordrumbox_engine::offline;
use ordrumbox_engine::wav;
use ordrumbox_logic::generate::{self, AutoGenerator};
use ordrumbox_logic::{compose, midi};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Why a WAV export is refused.
pub(super) const NOTHING_TO_RENDER: &str = "nothing to render with (no samples loaded)";

/// Why the synth sounds cannot be exported.
pub(super) const NO_SYNTHS: &str = "No synth sounds loaded";

/// What a WAV export renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WavSource {
    /// Pattern `index`, `loop_count` times (from 1).
    Pattern { index: usize, loop_count: u32 },
    /// Song `index`, its whole arrangement once.
    Song(usize),
}

/// The file the synth sounds are exported to.
pub(super) const SYNTHS_FILE_NAME: &str = "ordrumbox-synth-sounds.json";

/// The file a pattern is exported to as JSON (`ordrumbox-<name>.json`).
fn pattern_json_file_name(pattern: &Pattern) -> String {
    format!("ordrumbox-{}.json", store::safe_name(pattern.name.as_deref().unwrap_or("pattern")))
}

impl Session {
    /// Ctrl+E: the selected pattern, as many loops as chosen (with Shift: the
    /// selected song) to a WAV file at `path`.
    fn export_wav(&mut self, song: bool, path: &Path) {
        let source = if song {
            WavSource::Song(self.editor.selected_song())
        } else {
            WavSource::Pattern { index: self.pattern_index, loop_count: self.chrome.export_loop_count }
        };
        self.message = self.write_wav(source, path).unwrap_or_else(|e| e);
    }

    /// Ctrl+Shift+M: the selected pattern, as many loops as chosen, to a MIDI file at `path`.
    fn export_midi(&mut self, path: &Path) {
        if self.pattern().is_none() {
            return;
        }
        self.message = self.write_midi(self.pattern_index, self.chrome.export_loop_count, path).unwrap_or_else(|e| e);
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
        let start = self.chrome.last_dir.clone();
        self.ask_file(purpose, start.as_deref(), None);
    }

    /// Ask for a file or folder for `purpose`, opening in `start`: of the
    /// program's dialog if it shows the system's, else of the picker drawn in
    /// the window. `file_name` is what an export of a single file is called.
    pub(super) fn ask_file(&mut self, purpose: Purpose, start: Option<&Path>, file_name: Option<String>) {
        if self.program.native_files {
            self.program.file_request = Some(FileRequest::new(purpose, start, file_name));
        } else {
            self.chrome.overlay = Some(Overlay::Picker(Picker::new(purpose, start)));
        }
    }

    /// From now on files are asked of the program's dialogs
    /// ([`Self::take_file_request`]), not of the picker in the window.
    pub fn use_native_dialogs(&mut self) {
        self.program.native_files = true;
    }

    /// The dialog asked for since the last call; its answer goes to [`Self::file_chosen`].
    pub fn take_file_request(&mut self) -> Option<FileRequest> {
        self.program.file_request.take()
    }

    /// The dialog of `request` is answered with `path`; `None`: it was cancelled.
    /// A file to write was confirmed by the dialog if it replaces one.
    pub fn file_chosen(&mut self, request: &FileRequest, path: Option<PathBuf>) {
        let Some(path) = path else { return };
        self.chrome.last_dir = if path.is_dir() { Some(path.clone()) } else { path.parent().map(Path::to_path_buf) };
        match request.export_to_file() {
            Some(export) => self.write_export(export, &path),
            None => self.picked(request.purpose(), path),
        }
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
            Export::PatternStems => self.export_stems(false, path),
            Export::SongStems => self.export_stems(true, path),
            Export::PatternMidi => self.export_midi(path),
            Export::PatternJson => self.export_pattern_json(path),
            Export::Project => self.export_project(path),
            Export::ProjectSaveAs => self.save_project_as(path),
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
            Purpose::ImportProject => self.import_project(&path),
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
                "odbox" => return self.import_project(path),
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
                _ => Err(format!("{}: not a MIDI, WAV, project or kit file", path.display())),
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
    /// What `source` plays as a WAV file at `path`; what was written.
    pub(super) fn write_wav(&self, source: WavSource, path: &Path) -> Result<String, String> {
        let setup = self.render.as_ref().ok_or(NOTHING_TO_RENDER)?;
        let library = self.editor.library();
        let started = Instant::now();
        let rendered = match source {
            WavSource::Song(index) => {
                if library.songs.get(index).is_none() {
                    return Err("no song in the library".into());
                }
                offline::render_song(library, index, setup)
            }
            WavSource::Pattern { index, loop_count } => self.with_auto_loops(setup.seed, |hook| {
                offline::render_pattern(library, index, loop_count, setup, Some(hook))
            }),
        };
        let (left, right) = rendered.ok_or("nothing to export (no playable clip)")?;
        let bytes = wav::encode(&left, &right, setup.sample_rate);
        store::write_atomic(path, bytes).map_err(|e| format!("not exported: {}: {e}", path.display()))?;
        let loops = match source {
            WavSource::Pattern { loop_count: 1, .. } => "1 loop, ".to_owned(),
            WavSource::Pattern { loop_count, .. } => format!("{loop_count} loops, "),
            WavSource::Song(_) => String::new(),
        };
        Ok(format!(
            "exported {} ({loops}{:.1} s of audio in {:.1} s)",
            path.display(),
            left.len() as f64 / f64::from(setup.sample_rate),
            started.elapsed().as_secs_f64()
        ))
    }

    /// Run `render` with the hook that gives the tracks in auto mode their new
    /// lines loop after loop, on the render's copy of the library.
    pub(super) fn with_auto_loops<T>(&self, seed: u64, render: impl FnOnce(offline::BeforePass<'_>) -> T) -> T {
        let (scales, mut rng) = (&self.generators.scales, Rng::new(seed));
        let styles = compose::learn(&self.generators.styles, &self.editor.library().patterns);
        let mut loop_start = |library: &mut Library, p: usize, number: u64| {
            if let Some(pattern) = library.patterns.get_mut(p).filter(|p| generate::plays_auto(p)) {
                let mut rnd = || rng.next_f64();
                generate::run_loop(&mut AutoGenerator::new(scales, &styles, &mut rnd), pattern, number);
            }
        };
        render(&mut loop_start)
    }

    /// Pattern `p`, `loop_count` times, as a MIDI file at `path`; what was written.
    pub(super) fn write_midi(&self, p: usize, loop_count: u32, path: &Path) -> Result<String, String> {
        let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
        let mut rng = Rng::new(1);
        let bpm = self.editor.library().bpm;
        let bytes = midi::export(pattern, bpm, loop_count, &self.roots, &mut rng, &self.generators.scales);
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
        self.chrome.beat_window.rewind();
        Ok(message)
    }

    /// Ask for the folder to write `export` to: the last one picked, at first
    /// the exports folder. What cannot be exported is said at once.
    pub(super) fn open_export(&mut self, export: Export) {
        let refusal = match export {
            Export::PatternWav | Export::SongWav | Export::PatternStems | Export::SongStems
                if self.render.is_none() =>
            {
                Some(NOTHING_TO_RENDER)
            }
            Export::Synths if self.synths.presets.is_empty() => Some(NO_SYNTHS),
            _ => None,
        };
        if let Some(refusal) = refusal {
            self.message = refusal.into();
            return;
        }
        let start = self.chrome.last_dir.clone().filter(|d| d.is_dir()).or_else(store::exports_dir);
        let file_name = self.export_file_name(export);
        self.ask_file(Purpose::Export(export), start.as_deref(), file_name);
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
            Export::PatternStems => format!("ordrumbox-{}-stems", store::safe_name(&pattern?.id)),
            Export::SongStems => format!("ordrumbox-song-{}-stems", store::safe_name(&library.songs.get(s)?.id)),
            Export::PatternMidi => format!("ordrumbox-{}.mid", store::safe_name(&pattern?.id)),
            Export::PatternJson => pattern_json_file_name(pattern?),
            Export::Project | Export::ProjectSaveAs => self.project_file_name(),
            Export::Synths => SYNTHS_FILE_NAME.to_owned(),
            Export::KitMapping => self.kits.mapping_file_name(self.kit_index)?,
            Export::KitFolder => self.kits.folder_name(self.kit_index)?,
            Export::Library => session_user_data::LIBRARY_FILE_NAME.to_owned(),
            Export::UserData => crate::user_data::backup_name(&crate::user_data::now_stamp()),
        })
    }
}
