//! Opening the session: its library and assets found, its first state built.

use super::session_chrome::Chrome;
use super::session_playback::Playback;
use super::session_saving::Saving;
use super::session_selection::Selection;
use super::{
    DEMO_NOT_SAVED, Flash, LibrarySource, Opened, Session, Workspace, repaired_status, session_output, session_roll,
    session_song, session_synth, session_update,
};
use crate::action_log::ActionLog;
use crate::audio_output::StartOutput;
use crate::editors::EditorState;
use crate::kits::KitSet;
use crate::{assets, store};
use ordrumbox_core::{Library, Rng, ScaleList, Scales, Styles};
use ordrumbox_engine::offline::RenderSetup;
use ordrumbox_engine::{SynthPresets, offline};
use ordrumbox_logic::Editor;
use std::path::PathBuf;
use std::sync::Arc;

impl Session {
    pub fn load(start_output: StartOutput) -> Self {
        Self::load_from(start_output, LibrarySource::UserFile)
    }

    /// The same with its library from `source`.
    pub fn load_from(start_output: StartOutput, source: LibrarySource) -> Self {
        Self::load_with_assets(start_output, source, &[])
    }

    /// The same, the assets searched in `first` before the usual folders (a
    /// plugin's bundle).
    pub fn load_with_assets(mut start_output: StartOutput, source: LibrarySource, first: &[PathBuf]) -> Self {
        let Some(dir) = assets::find(first) else {
            let searched: Vec<String> = assets::candidates(first).iter().map(|d| d.display().to_string()).collect();
            return Self::new(
                Library::default(),
                KitSet::empty(),
                vec![
                    format!(
                        "assets folder not found: copy this repository's assets folder next to the program, or set {}",
                        assets::ENV_VAR
                    ),
                    format!("searched: {}", searched.join(", ")),
                ],
            );
        };
        let assets = match assets::load(&dir) {
            Ok(assets) => assets,
            Err(e) => return Self::new(Library::default(), KitSet::empty(), vec![e]),
        };
        let mut status: Vec<String> = assets.errors.iter().map(|e| format!("not loaded: {e}")).collect();
        status.extend(assets.style_issues.iter().map(|i| format!("styles.json: {}: {}", i.at, i.message)));
        let (kits_file, kit_issues) = &assets.kit_issues;
        if let Some(first) = kit_issues.first() {
            status.push(format!("{kits_file}: {} skipped, first: {}: {}", kit_issues.len(), first.at, first.message));
        }
        if let Some(first) = assets.sample_errors.first() {
            status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
        }

        let hosted = matches!(source, LibrarySource::Host(_));
        let Opened { mut library, file: library_path, on_disk, message, demo } = match source {
            LibrarySource::UserFile => {
                if let Some(problem) = store::location_problem() {
                    status.push(format!("{problem}: File > Change user data folder"));
                }
                Self::open_library(store::path(), assets.library, &mut status)
            }
            LibrarySource::Host(text) => Self::open_host_library(text, assets.library, &mut status),
        };
        if demo {
            status.extend(repaired_status(assets::LIBRARY, &assets.report));
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
        session.asset_synths = assets.synths;
        session.synths = synths.clone();
        session.synths_revision = Some(session.editor.revision());
        session.hosted = hosted;
        if hosted {
            session.chrome.fold_editors();
        }
        session.assets_dir = Some(dir.clone());
        session.saving.songs_dir = store::songs_dir();
        session.scales = assets.scales.clone();
        session.styles = assets.styles.clone();
        session.scale_list = assets.scale_list.clone();
        session.roots =
            assets.bank.samples.iter().filter_map(|s| Some((s.sample.key.clone(), s.sample.root_midi?))).collect();
        session.saving.library_path = library_path;
        session.saving.on_disk = on_disk;
        session.message = message;
        let bank = Arc::new(assets.bank);
        if !bank.samples.is_empty() {
            session.render = Some(RenderSetup {
                bank: bank.clone(),
                scales: assets.scales.clone(),
                synths: synths.clone(),
                kit: session.kits.kits.first().map(|k| k.name.clone()).unwrap_or_default(),
                sample_rate: offline::EXPORT_RATE,
                seed: 1,
            });
        }
        match start_output(bank, assets.scales, synths) {
            Ok((output, controller)) => session.playback = Playback::new(output, controller),
            Err(e) => session.status.push(format!("no sound: {e}")),
        }
        session.playback.keep_start_output(start_output);
        // A plugin shows no master panel: the host mixes, the default master applies.
        if !hosted {
            session.load_master();
        }
        session.load_update_settings();
        session.load_color_scheme(store::settings_path());
        session.load_zoom(store::settings_path());
        session
    }

    /// The library to start with: the user's file at `path` when there is
    /// one, else `demo`, written to that file at the first edit (no file: the
    /// reason pushed on `status`).
    pub(super) fn open_library(path: Option<PathBuf>, demo: Library, status: &mut Vec<String>) -> Opened {
        let Some(path) = path else {
            status.push("no user data folder: edits are not saved".into());
            return Opened::demo(demo, DEMO_NOT_SAVED);
        };
        match store::load(&path) {
            Ok(Some((saved, report))) => {
                status.extend(repaired_status(&path.display().to_string(), &report));
                Opened {
                    library: saved,
                    file: Some(path),
                    on_disk: true,
                    message: "library loaded".into(),
                    demo: false,
                }
            }
            Ok(None) => Opened {
                file: Some(path),
                ..Opened::demo(demo, "your library starts as a copy of the demo: edits are saved")
            },
            Err(e) => {
                status.push(format!("library not loaded, not saved over: {e}"));
                Opened::demo(demo, DEMO_NOT_SAVED)
            }
        }
    }

    /// The library of a plugin: the host's `text` when it kept one, else the
    /// user's file (read, never written), else `demo`; no file to save to.
    pub(super) fn open_host_library(text: Option<String>, demo: Library, status: &mut Vec<String>) -> Opened {
        const DEMO: &str = "demo library, saved with the host project";
        if let Some(text) = text {
            return match Library::parse(&text) {
                Ok((library, report)) => {
                    status.extend(repaired_status("library of the host project", &report));
                    Opened::unsaved(library, "library from the host project")
                }
                Err(e) => {
                    status.push(format!("library of the host project not loaded: {e}"));
                    Opened::demo(demo, DEMO)
                }
            };
        }
        let Some(path) = store::path() else {
            return Opened::demo(demo, DEMO);
        };
        match store::load(&path) {
            Ok(Some((saved, report))) => {
                status.extend(repaired_status(&path.display().to_string(), &report));
                Opened::unsaved(saved, "your library, copied into the host project")
            }
            Err(e) => {
                status.push(format!("library not loaded: {e}"));
                Opened::demo(demo, DEMO)
            }
            Ok(None) => Opened::demo(demo, DEMO),
        }
    }

    /// The library as its file holds it, for a plugin host to keep in its project.
    /// Changes made to the library so far: compare to know when to ask
    /// [`library_text`](Self::library_text) again.
    pub fn library_revision(&self) -> u64 {
        self.editor.revision()
    }

    /// The library's file text, with the synth presets saved with it.
    pub fn library_text(&self) -> String {
        self.editor.library().to_file_text()
    }

    pub(super) fn new(library: Library, kits: KitSet, status: Vec<String>) -> Self {
        Self {
            editor: Editor::new(library),
            scales: Scales::default(),
            styles: Styles::default(),
            learned_styles: (Styles::default(), None),
            gen_style: None,
            rng: Rng::from_time(),
            roots: Vec::new(),
            kits,
            kit_index: 0,
            kit_editor: None,
            analysis: None,
            kit_files: (String::new(), None),
            synths: SynthPresets::default(),
            asset_synths: SynthPresets::default(),
            synths_revision: None,
            pattern_index: 0,
            workspace: Workspace::Grid,
            song_cursor: 0,
            song_loop: None,
            metronome: false,
            recording: false,
            taps: Vec::new(),
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
            quit: false,
            midi_changed: false,
            fullscreen: false,
            fullscreen_request: None,
            scale_list: ScaleList::default(),
            editors: EditorState::default(),
            color_scheme: 1,
            hosted: false,
            host_muted: 0,
            host_tempo_outside: None,
            host_shown: None,
            output_panel: session_output::OutputPanel::default(),
            updates: session_update::Updates::default(),
            mcp: None,
            synth_editor: session_synth::SynthEditor::new(),
            track_analysis: std::cell::RefCell::default(),
        }
    }
}
