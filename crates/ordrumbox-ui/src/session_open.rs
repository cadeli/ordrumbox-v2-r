//! Opening the session: its library and assets found, its first state built.

use super::session_chrome::Chrome;
use super::session_clock::Followed;
use super::session_commands::Program;
use super::session_generators::Generators;
use super::session_kit::KitEditor;
use super::session_playback::Playback;
use super::session_resume::is_first_start;
use super::session_saving::Saving;
use super::session_selection::Selection;
use super::session_toast::Flash;
use super::session_transport::Transport;
use super::session_types::{PlayMode, Workspace};
use super::{Session, session_mixer, session_output, session_roll, session_song, session_synth, session_update};
use crate::action_log::ActionLog;
use crate::audio_output::StartOutput;
use crate::editors::EditorState;
use crate::kits::KitSet;
use crate::{assets, store};
use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::offline;
use ordrumbox_engine::offline::RenderSetup;
use ordrumbox_engine::{SampleBank, SynthPresets};
use ordrumbox_logic::Editor;
use std::path::PathBuf;
use std::sync::Arc;

/// Where a session's library comes from, and where its edits go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibrarySource {
    /// The user's `library.json` (else the starter library), saved a moment after each edit.
    UserFile,
    /// A library given as text (the starter library when it cannot be
    /// parsed), held in memory: never saved to a file, and the user's
    /// settings (master, where the user was) left alone. For tests and tools,
    /// which read it back with [`Session::library_text`].
    Text(String),
}

/// A library opened at start.
#[derive(Debug)]
pub(super) struct Opened {
    pub(super) library: Library,
    /// The file edits are saved to; `None`: not saved.
    pub(super) file: Option<PathBuf>,
    /// The library was read from that file (else it is written at the first edit).
    pub(super) on_disk: bool,
    /// The message to show.
    pub(super) message: String,
    /// The library is the starter library (its repairs are those of the assets' file).
    pub(super) starter: bool,
}

impl Opened {
    /// `library`, not saved to any file.
    fn unsaved(library: Library, message: &str) -> Self {
        Self { library, file: None, on_disk: false, message: message.to_owned(), starter: false }
    }

    /// The starter library, not saved to any file.
    fn starter(library: Library, message: &str) -> Self {
        Self { starter: true, ..Self::unsaved(library, message) }
    }
}

/// The root note of each sample of `bank` that has one (key, MIDI note), for MIDI files.
fn root_notes(bank: &SampleBank) -> Vec<(String, f64)> {
    bank.samples.iter().filter_map(|s| Some((s.sample.key.clone(), s.sample.root_midi?))).collect()
}

/// The message of the starter library when no file keeps its edits.
const STARTER_NOT_SAVED: &str = "starter library: edits are not saved";

impl Session {
    pub fn load(start_output: StartOutput) -> Self {
        Self::load_from(start_output, LibrarySource::UserFile)
    }

    /// The same with its library from `source`.
    pub fn load_from(start_output: StartOutput, source: LibrarySource) -> Self {
        let Some(dir) = assets::find() else { return Self::without_assets() };
        let assets = match assets::load(&dir) {
            Ok(assets) => assets,
            Err(e) => return Self::new(Library::default(), KitSet::empty(), vec![e]),
        };
        let mut status = loading_status(&assets);

        let in_memory = matches!(source, LibrarySource::Text(_));
        let Opened { mut library, file: library_path, on_disk, message, starter } = match source {
            LibrarySource::UserFile => {
                if let Some(problem) = store::location_problem() {
                    status.push(format!("{problem}: File > Change user data folder"));
                }
                Self::open_library(store::path(), assets.library, &mut status)
            }
            LibrarySource::Text(text) => Self::open_text_library(&text, assets.library, &mut status),
        };
        if starter {
            status.extend(assets.report.summary(assets::LIBRARY));
        }

        if let Some(e) = &assets.kits_error {
            status.push(format!("kits.json not loaded, kit edits not saved: {e}"));
        }
        let kits_file = if assets.kits_error.is_none() { store::kits_path() } else { None };
        let kits =
            KitSet::new(assets.kits, &assets.bank, Some(assets.sources.clone()), kits_file, store::samples_dir());
        // A library saved before the synth presets were saved with it takes
        // those of the presets file.
        if library.synths.is_none() {
            library.synths = Self::read_user_synths(store::saved_synths_path(), &assets.synths, &mut status);
        }
        let synths = session_synth::merged_synths(library.synths.as_ref(), &assets.synths);
        let mut session = Self::new(library, kits, status);
        session.saving.first_start = !in_memory && is_first_start(store::settings_path().as_deref());
        session.synths.assets = assets.synths;
        session.synths.presets = synths.clone();
        session.synths.revision = Some(session.editor.revision());
        session.assets_dir = Some(dir);
        session.demo_styles = assets.demos;
        session.saving.projects_dir = store::projects_dir();
        session.generators.scales = assets.scales.clone();
        session.generators.styles = assets.styles.clone();
        session.generators.scale_list = assets.scale_list.clone();
        session.roots = root_notes(&assets.bank);
        session.saving.library_path = library_path;
        session.saving.on_disk = on_disk;
        session.message = message;
        session.attach_sound(assets.bank, assets.scales, synths, start_output);
        if !in_memory {
            session.load_master();
        }
        session.load_update_settings();
        session.load_color_scheme(store::settings_path());
        session.load_controls();
        session.load_zoom(store::settings_path());
        if !in_memory {
            session.load_resume();
        }
        session
    }

    /// The session of a program that found no assets folder: an empty
    /// library, and what was searched.
    fn without_assets() -> Self {
        let searched: Vec<String> = assets::candidates().iter().map(|d| d.display().to_string()).collect();
        Self::new(
            Library::default(),
            KitSet::empty(),
            vec![
                format!(
                    "assets folder not found: copy this repository's assets folder next to the program, or set {}",
                    assets::ENV_VAR
                ),
                format!("searched: {}", searched.join(", ")),
            ],
        )
    }

    /// The sound output on `bank`, and the offline renders' setup (none
    /// without a sample); a failure to start is a status line.
    fn attach_sound(&mut self, bank: SampleBank, scales: Scales, synths: SynthPresets, mut start_output: StartOutput) {
        let bank = Arc::new(bank);
        if !bank.samples.is_empty() {
            self.render = Some(RenderSetup {
                bank: bank.clone(),
                scales: scales.clone(),
                synths: synths.clone(),
                kit: self.kits.kits().first().map(|k| k.name.clone()).unwrap_or_default(),
                sample_rate: offline::EXPORT_RATE,
                seed: 1,
                returns: Default::default(),
            });
        }
        match start_output(bank, scales, synths) {
            Ok((output, controller)) => self.playback = Playback::new(output, controller),
            Err(e) => self.status.push(format!("no sound: {e}")),
        }
        self.playback.keep_start_output(start_output);
    }

    /// The library to start with: the user's file at `path` when there is
    /// one, else `starter`, written to that file at the first edit (no file: the
    /// reason pushed on `status`).
    pub(super) fn open_library(path: Option<PathBuf>, starter: Library, status: &mut Vec<String>) -> Opened {
        let Some(path) = path else {
            status.push("no user data folder: edits are not saved".into());
            return Opened::starter(starter, STARTER_NOT_SAVED);
        };
        match store::load(&path) {
            Ok(Some((saved, report))) => {
                status.extend(report.summary(&path.display().to_string()));
                Opened {
                    library: saved,
                    file: Some(path),
                    on_disk: true,
                    message: "library loaded".into(),
                    starter: false,
                }
            }
            Ok(None) => Opened {
                file: Some(path),
                ..Opened::starter(starter, "your library starts as a copy of the starter library: edits are saved")
            },
            Err(e) => {
                status.push(format!("library not loaded, not saved over: {e}"));
                Opened::starter(starter, STARTER_NOT_SAVED)
            }
        }
    }

    /// The library of `text`, else `starter`; no file to save to.
    pub(super) fn open_text_library(text: &str, starter: Library, status: &mut Vec<String>) -> Opened {
        match Library::parse(text) {
            Ok((library, report)) => {
                status.extend(report.summary("library given"));
                Opened::unsaved(library, "library given, not saved")
            }
            Err(e) => {
                status.push(format!("library given not loaded: {e}"));
                Opened::starter(starter, STARTER_NOT_SAVED)
            }
        }
    }

    /// Changes made to the library so far: compare to know whether it changed.
    pub(crate) fn library_revision(&self) -> u64 {
        self.editor.revision()
    }

    /// The library's file text, with the synth presets saved with it.
    pub fn library_text(&self) -> String {
        self.editor.library().to_file_text()
    }

    pub(super) fn new(library: Library, kits: KitSet, status: Vec<String>) -> Self {
        Self {
            editor: Editor::new(library),
            generators: Generators::default(),
            roots: Vec::new(),
            kits,
            kit_index: 0,
            kit_revision: None,
            kit_editor: KitEditor::default(),
            synths: session_synth::Synths::default(),
            pattern_index: 0,
            workspace: Workspace::Grid,
            mode: PlayMode::Pattern,
            transport: Transport::default(),
            song_panel: session_song::SongPanel::default(),
            roll: session_roll::RollState::default(),
            render: None,
            playback: Playback::default(),
            selection: Selection::default(),
            saving: Saving::default(),
            chrome: Chrome::default(),
            status,
            message: String::new(),
            flash: Flash::default(),
            action_log: ActionLog::default(),
            assets_dir: None,
            demo_styles: Vec::new(),
            program: Program::default(),
            editors: EditorState::default(),
            followed: Followed::default(),
            controls: ordrumbox_engine::ControlMap::new(),
            output_panel: session_output::OutputPanel::default(),
            mixer: session_mixer::Mixer::default(),
            automation_live: Default::default(),
            updates: session_update::Updates::default(),
            mcp: None,
            synth_editor: session_synth::SynthEditor::new(),
            track_analysis: std::cell::RefCell::default(),
        }
    }
}

impl Session {
    /// A session of `library` with no kit.
    #[cfg(test)]
    pub(crate) fn for_tests(library: Library) -> Self {
        Self::new(library, KitSet::empty(), Vec::new())
    }
}

/// What the loaded assets say went wrong, a status line each.
fn loading_status(assets: &assets::Assets) -> Vec<String> {
    let mut status: Vec<String> = assets.errors.iter().map(|e| format!("not loaded: {e}")).collect();
    status.extend(assets.style_issues.iter().map(|i| format!("styles.json: {}: {}", i.at, i.message)));
    for (source, kit_issues) in &assets.kit_issues {
        if let Some(first) = kit_issues.first() {
            status.push(format!("{source}: {} skipped, first: {}: {}", kit_issues.len(), first.at, first.message));
        }
    }
    if let Some(first) = assets.sample_errors.first() {
        status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
    }
    status
}
