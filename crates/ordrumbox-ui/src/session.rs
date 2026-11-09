//! Everything the window drives: the library being edited, the playback, the
//! generators, MIDI files, the grid cursor and selection, the clipboard, saving. Edits go through the
//! [`Editor`] (each one an undo step); the playing pass is rebuilt after each.
//!
//! This file holds the session's state, its loading, undo and saving, and
//! its drawing; the rest is in `session_*.rs`, one `impl Session` each:
//! `session_transport.rs` (play, the pattern, kit and tempo), `session_edit.rs`
//! (the grid's edits), `session_generate.rs`, `session_commands.rs` (the
//! menus), `session_overlay.rs`, `session_kit.rs`, `session_files.rs`
//! (exports, imports, dropped files), and `session_panel*.rs` (the editors'
//! widgets).
//!
//! The song panel (F3) and the piano roll (F4) take the grid's place:
//! `session_song*.rs`, `session_roll.rs`.
//!
//! The grid follows the JS pattern panel (`pointer_section.js`,
//! `keyboard_section.js`, `clipboard_section.js`): a click on an empty step
//! adds a note, on a note selects it, on the selected note deletes it.

use crate::scale::Scale;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_core::{Library, LoadReport, Note, Pattern, Rng, ScaleList, Scales, Styles, Track};
use ordrumbox_engine::analysis::{self, Analysis};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{Mode, SynthPresets, wav};
use ordrumbox_logic::compose;
use ordrumbox_logic::generate::{self, AutoGenerator, MelodicKind};
use ordrumbox_logic::midi;
use ordrumbox_logic::tools::learned_styles;
use ordrumbox_logic::{Editor, Nudge};

use crate::action_log::{self, ActionLog, LogLayout, Seen};
use crate::assets;
use crate::audio_output::StartOutput;
use crate::canvas::Canvas;
use crate::editors::EditorState;
use crate::frame;
use crate::kits::KitSet;
use crate::mcp_link;
use crate::menu::{self, Command, MenuState};
use crate::panel::Rect;
use crate::panel::{Dropdown, Entry, Id};
use crate::picker::{self, Export, Outcome, Picker, PickerLayout, Purpose};
use crate::store;
use crate::theme;
use crate::update;
use crate::view::{self, Cursor, Hit, KitView, View};
use session_chrome::Chrome;
use session_confirm::Pending;
use session_playback::Playback;
use session_saving::Saving;
use session_selection::{Clipboard, NoteFocus, Selection};

#[path = "session_chrome.rs"]
mod session_chrome;
#[path = "session_color.rs"]
mod session_color;
#[path = "session_commands.rs"]
mod session_commands;
#[path = "session_confirm.rs"]
mod session_confirm;
#[path = "session_edit.rs"]
mod session_edit;
#[path = "session_files.rs"]
mod session_files;
#[path = "session_generate.rs"]
mod session_generate;
#[path = "session_grid.rs"]
mod session_grid;
#[path = "session_lane.rs"]
mod session_lane;
#[path = "session_zoom.rs"]
mod session_zoom;

/// A style's name in the menus: a learned one says from how many patterns.
pub fn style_label(style: &ordrumbox_core::Style) -> String {
    match style.learned {
        Some((patterns, measures)) => format!("{} (learned: {patterns} patterns, {measures} measures)", style.name),
        None => style.name.clone(),
    }
}

#[path = "session_kit.rs"]
mod session_kit;
#[cfg(test)]
#[path = "session_library_file.rs"]
mod session_library_file;
#[path = "session_mcp.rs"]
mod session_mcp;
#[path = "session_output.rs"]
mod session_output;
#[path = "session_overlay.rs"]
mod session_overlay;
#[path = "session_panel.rs"]
mod session_panel;
#[path = "session_panel_edit.rs"]
mod session_panel_edit;
#[path = "session_panel_input.rs"]
mod session_panel_input;
#[path = "session_panel_views.rs"]
mod session_panel_views;
#[path = "session_playback.rs"]
mod session_playback;
#[path = "session_roll.rs"]
mod session_roll;
#[path = "session_saving.rs"]
mod session_saving;
#[cfg(test)]
#[path = "session_scenarios.rs"]
mod session_scenarios;
#[path = "session_selection.rs"]
mod session_selection;
#[path = "session_song.rs"]
mod session_song;
#[path = "session_song_files.rs"]
mod session_song_files;
#[path = "session_song_menu.rs"]
mod session_song_menu;
#[path = "session_song_mouse.rs"]
mod session_song_mouse;
#[path = "session_synth.rs"]
mod session_synth;
#[path = "session_transport.rs"]
mod session_transport;
#[path = "session_update.rs"]
mod session_update;
#[path = "session_user_data.rs"]
mod session_user_data;
pub use session_panel::{Button, PanelKey};

/// What the main editor (top-left) shows under the header. The kit editor
/// opens over any of them; the track editor (right) and the output strip
/// (bottom) are always available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Workspace {
    Grid,
    /// The piano roll (F4): the track editor's track.
    PianoRoll,
    /// The song panel (F3): Space plays the song.
    Song,
    /// The synth editor (F6): the track editor's track's preset.
    Synth,
}

/// Why a WAV export is refused.
const NOTHING_TO_RENDER: &str = "nothing to render with (no samples loaded)";
/// Why the synth sounds cannot be exported.
const NO_SYNTHS: &str = "No synth sounds loaded";
/// The file the synth sounds are exported to.
const SYNTHS_FILE_NAME: &str = "ordrumbox-synth-sounds.json";

/// The name of the track Ctrl+T adds.
const DEFAULT_TRACK_NAME: &str = "KICK";

/// The file a pattern is exported to as JSON (`ordrumbox-<name>.json`).
fn pattern_json_file_name(pattern: &Pattern) -> String {
    format!("ordrumbox-{}.json", store::safe_name(pattern.name.as_deref().unwrap_or("pattern")))
}

/// `beat 2.3`: a cursor position.
fn beat_label(beat: i32, step: i32) -> String {
    format!("beat {}.{}", beat + 1, step + 1)
}

/// `3 notes`.
fn notes_label(count: usize) -> String {
    format!("{count} note{}", if count == 1 { "" } else { "s" })
}

/// A direction (arrow key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

/// Keys a panel over the window reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Enter,
    /// Backspace: the parent folder.
    Back,
    Move(isize),
    /// Home (false) / End (true).
    Edge(bool),
    Letter(char),
}

/// Where a session's library comes from, and where its edits go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LibrarySource {
    /// The user's `library.json` (else the demo), saved a moment after each edit.
    UserFile,
    /// A plugin's: the text its host kept in the project (`None`: a new
    /// instance, which starts from the user's library, else the demo). It is
    /// never saved to a file: the host asks [`Session::library_text`].
    Host(Option<String>),
}

/// A library opened at start.
#[derive(Debug)]
struct Opened {
    library: Library,
    /// The file edits are saved to; `None`: not saved.
    file: Option<PathBuf>,
    /// The library was read from that file (else it is written at the first edit).
    on_disk: bool,
    /// The message to show.
    message: String,
    /// The library is the demo (its repairs are those of the assets' file).
    demo: bool,
}

impl Opened {
    /// `library`, not saved to any file.
    fn unsaved(library: Library, message: &str) -> Self {
        Self { library, file: None, on_disk: false, message: message.to_owned(), demo: false }
    }

    /// The demo library, not saved to any file.
    fn demo(library: Library, message: &str) -> Self {
        Self { demo: true, ..Self::unsaved(library, message) }
    }
}

/// The status line saying what loading `file` repaired, `None` when nothing:
/// how many values took their default and song clips were dropped, and the
/// first of each.
fn repaired_status(file: &str, report: &LoadReport) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(first) = report.issues.first() {
        parts.push(format!("{} value(s) repaired, first: {}: {}", report.issues.len(), first.at, first.message));
    }
    if let Some(first) = report.dropped_clips.first() {
        parts.push(format!(
            "{} song clip(s) dropped, first: {}: {}",
            report.dropped_clips.len(),
            first.pattern,
            first.reason
        ));
    }
    (!parts.is_empty()).then(|| format!("{file}: {}", parts.join("; ")))
}

/// The message of the demo library when no file keeps its edits.
const DEMO_NOT_SAVED: &str = "demo library: edits are not saved";

pub struct Session {
    /// What the window shows around the library (`session_chrome.rs`).
    chrome: Chrome,
    /// The cursor, the selected note, the clipboard (`session_selection.rs`).
    selection: Selection,
    /// Where the files go and when (`session_saving.rs`).
    saving: Saving,
    editor: Editor,
    /// What the generators read (`scales.json`).
    scales: Scales,
    /// The styles of `styles.json`.
    styles: Styles,
    /// Those styles with what the library's tags teach, and the library
    /// revision they were learned at.
    learned_styles: (Styles, Option<u64>),
    /// The style B and D generate in (Generate menu); `None`: the pattern's, else a drawn one.
    gen_style: Option<String>,
    /// Random draws of the generators.
    rng: Rng,
    /// Root note of the loaded samples that have one (key, MIDI note), for MIDI files.
    roots: Vec<(String, f64)>,
    kits: KitSet,
    kit_index: usize,
    /// The kit editor, when open: selected sample of the current kit.
    kit_editor: Option<usize>,
    /// Analysis of the sample the kit editor shows, by url.
    analysis: Option<(String, Analysis)>,
    /// Where the kit editor's kit and sample are read from (folder, file),
    /// found with the analysis: looking costs a file system call a sample.
    kit_files: (String, Option<PathBuf>),
    /// The synth presets played and shown: the library's own over the assets'.
    synths: SynthPresets,
    /// The assets' synth presets.
    asset_synths: SynthPresets,
    /// The editor revision `synths` was made at.
    synths_revision: Option<u64>,
    pattern_index: usize,
    /// What is shown under the header.
    workspace: Workspace,
    /// The measure the song plays from.
    song_cursor: u32,
    /// The song panel's scrolls and what the mouse holds in it.
    song_panel: session_song::SongPanel,
    /// The piano roll's view and what the mouse holds in it.
    roll: session_roll::RollState,
    /// What offline renders (WAV exports) use; `None` without samples.
    render: Option<RenderSetup>,
    /// The engine and the sound output (`session_playback.rs`).
    playback: Playback,
    /// Startup problems, shown until quit.
    status: Vec<String>,
    /// Outcome of the last action.
    message: String,
    /// The message as last shown, and the toasts on screen.
    flash: Flash,
    /// What was done so far, shown by a click on the status line.
    action_log: ActionLog,
    /// The assets folder in use.
    assets_dir: Option<PathBuf>,
    /// File > Quit was chosen.
    quit: bool,
    /// The scale names, in file order (arpeggios).
    scale_list: ScaleList,
    /// Tabs and selection of the track and note editors.
    editors: EditorState,
    /// The scheme in use, from 1, set again on the thread that draws.
    color_scheme: usize,
    /// The library lives in a plugin host's project, not in a file.
    hosted: bool,
    /// The tracks a plugin host mutes, bit `t` for track `t`.
    host_muted: u64,
    /// The host's tempo last said to be outside the tempo bounds
    /// (`report_host_tempo`), so the message is given once.
    host_tempo_outside: Option<f64>,
    /// The output panel: the master bus, its tab, the spectrum.
    output_panel: session_output::OutputPanel,
    /// The update check (`session_update.rs`).
    updates: session_update::Updates,
    /// The MCP link (`mcp_link.rs`): the calls of `ordrumbox --mcp`.
    mcp: Option<mcp_link::Server>,
    /// The synth editor (`session_synth.rs`).
    synth_editor: session_synth::SynthEditor,
    /// Sample analyses of the track editor, by url.
    track_analysis: std::cell::RefCell<std::collections::HashMap<String, Analysis>>,
}

/// A drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Drag {
    id: Id,
    spec: crate::control::Spec,
    start: f64,
    x: i32,
    y: i32,
    /// A slider's bar (left, width), when it follows the mouse.
    bar: Option<(i32, i32)>,
    /// A slider dragged finely: from where.
    fine: bool,
}

/// What the mouse and the keyboard hold among the widgets of every panel.
#[derive(Debug, Default)]
struct Widgets {
    /// The widget with the keyboard focus.
    focus: Option<Id>,
    /// A knob or slider being dragged.
    drag: Option<Drag>,
    dropdown: Option<Dropdown>,
    entry: Option<Entry>,
    /// The mouse, at its last move or press: where a text entry opens.
    pointer: (i32, i32),
    /// Last click on a widget, for double-clicks.
    last_click: Option<(Id, Instant)>,
}

/// What is shown over the window.
#[derive(Debug)]
enum Overlay {
    Picker(Picker),
    /// The keyboard shortcuts, from line `scroll` on.
    Shortcuts {
        scroll: usize,
    },
    About,
    FactoryReset,
    /// What was done; the first row shown.
    History {
        scroll: usize,
    },
    /// The file an export would replace, until the user says so.
    ConfirmOverwrite {
        export: Export,
        path: PathBuf,
    },
    /// What removes or replaces the user's work, until the user says so.
    Confirm(Pending),
}

/// The loop start of pattern `p` (loop `pass` of the playback): new lines for
/// its tracks in auto mode, in the styles the library teaches; whether any changed.
fn auto_loop(
    editor: &mut Editor,
    learned: &mut (Styles, Option<u64>),
    styles: &Styles,
    scales: &Scales,
    rng: &mut Rng,
    p: usize,
    pass: u64,
) -> bool {
    let styles = learned_styles(learned, styles, editor).clone();
    editor.auto_loop(p, pass, scales, &styles, &mut || rng.next_f64())
}

/// The output of a session that had none: none again.
fn no_output(_: Arc<ordrumbox_engine::SampleBank>, _: Scales, _: SynthPresets) -> crate::audio_output::StartedOutput {
    Err("no audio output given".into())
}

// A plugin shares its session between the editor and a timer thread.
const _: fn() = || {
    fn send<T: Send>() {}
    send::<Session>();
};

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session").field("pattern_index", &self.pattern_index).finish_non_exhaustive()
    }
}

/// The lower edge of the toast in a window `height` high.
fn toast_bottom(height: i32, scale: Scale) -> i32 {
    height - 2 * crate::frame::margin(scale)
}

/// How long a new message is shown as a toast.
const FLASH: Duration = Duration::from_secs(1);

/// How many toasts are on screen at most; a new one beyond pushes out the oldest.
const TOAST_LIMIT: usize = 5;

/// The words of a toast for `text` shown `repeat_count` times in a row: the
/// count follows them from the second time on.
#[must_use]
fn toast_text(text: &str, repeat_count: u32) -> String {
    if repeat_count > 1 { format!("{text} (×{repeat_count})") } else { text.to_owned() }
}

/// A message shown as a toast, until a time.
#[derive(Debug, Clone)]
struct Toast {
    text: String,
    until: Instant,
}

/// The message last shown on the status line, how many times in a row, and
/// the toasts still on screen, the oldest first.
#[derive(Debug, Clone, Default)]
struct Flash {
    text: String,
    repeat_count: u32,
    toasts: Vec<Toast>,
    /// No toast is shown (View > Toasts unchecked); messages still go on the status line.
    hidden: bool,
}

impl Flash {
    /// `text` is shown at `now`: it stands out for [`FLASH`] in a toast of
    /// its own, under the ones still there, or not at all when empty. The same
    /// words again get a toast of their own too, which counts the repeats.
    fn shown(&mut self, text: &str, now: Instant) {
        self.repeat_count = if text == self.text { self.repeat_count.saturating_add(1) } else { 1 };
        self.text = text.to_owned();
        if text.is_empty() || self.hidden {
            return;
        }
        if self.toasts.len() == TOAST_LIMIT {
            self.toasts.remove(0);
        }
        let text = toast_text(text, self.repeat_count);
        self.toasts.push(Toast { text, until: now + FLASH });
    }

    /// Each toast with its rectangle in a window `width` × `height`: the
    /// newest at the bottom, the older ones stacked above it.
    fn rects(&self, width: i32, height: i32, scale: Scale) -> Vec<(Rect, &Toast)> {
        let mut bottom = toast_bottom(height, scale);
        let mut rects: Vec<(Rect, &Toast)> = self
            .toasts
            .iter()
            .rev()
            .map(|toast| {
                let rect = view::toast_rect(width, &toast.text, bottom, scale);
                bottom = rect.y - view::toast_gap(scale);
                (rect, toast)
            })
            .collect();
        rects.reverse();
        rects
    }
}

impl Session {
    /// The session for the assets found and the user's files, its sound
    /// started by `start_output`.
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
    fn open_library(path: Option<PathBuf>, demo: Library, status: &mut Vec<String>) -> Opened {
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
    fn open_host_library(text: Option<String>, demo: Library, status: &mut Vec<String>) -> Opened {
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

    fn new(library: Library, kits: KitSet, status: Vec<String>) -> Self {
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
            scale_list: ScaleList::default(),
            editors: EditorState::default(),
            color_scheme: 1,
            hosted: false,
            host_muted: 0,
            host_tempo_outside: None,
            output_panel: session_output::OutputPanel::default(),
            updates: session_update::Updates::default(),
            mcp: None,
            synth_editor: session_synth::SynthEditor::new(),
            track_analysis: std::cell::RefCell::default(),
        }
    }

    pub fn pattern(&self) -> Option<&Pattern> {
        self.editor.library().patterns.get(self.pattern_index)
    }

    fn track(&self, t: usize) -> Option<&Track> {
        self.pattern()?.tracks.get(t)
    }

    pub fn status_lines(&self) -> usize {
        self.status.len()
    }

    /// Show `text` on the status line.
    pub fn say(&mut self, text: &str) {
        self.message = text.into();
    }

    /// What the last action said.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Before an input: the message shown, taken away so that what the
    /// input says, even the same words again, is known as new
    /// ([`Self::input_done`]).
    pub(crate) fn input_starts(&mut self) -> String {
        std::mem::take(&mut self.message)
    }

    /// After an input: a message it gave stands out; without one, `before`
    /// (from [`Self::input_starts`]) stays as it was.
    pub(crate) fn input_done(&mut self, before: String) {
        if self.message.is_empty() {
            self.message = before;
        } else {
            self.flash.shown(&self.message, Instant::now());
        }
    }

    /// A message that changed outside an input (an MCP call, a drag, an
    /// update check) stands out too.
    pub(crate) fn note_message(&mut self) {
        if self.message != self.flash.text {
            self.flash.shown(&self.message, Instant::now());
        }
    }

    /// When the next toast goes; `None` when there is none.
    pub(crate) fn flash_until(&self) -> Option<Instant> {
        self.flash.toasts.iter().map(|toast| toast.until).min()
    }

    /// The toasts on screen, the oldest first.
    #[cfg(test)]
    pub(crate) fn toast_texts(&self) -> Vec<&str> {
        self.flash.toasts.iter().map(|toast| toast.text.as_str()).collect()
    }

    /// A press at `(x, y)` on a toast hides it at once; whether there was
    /// one there.
    pub(crate) fn toast_click(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let (width, height, scale) = size;
        let hit = self.flash.rects(width, height, scale).iter().position(|(rect, _)| rect.contains(x, y));
        if let Some(index) = hit {
            self.flash.toasts.remove(index);
        }
        hit.is_some()
    }

    /// The toasts whose time is over go; whether one did.
    pub(crate) fn end_flash(&mut self, now: Instant) -> bool {
        let count = self.flash.toasts.len();
        self.flash.toasts.retain(|toast| toast.until > now);
        self.flash.toasts.len() != count
    }

    /// The window split for `size` (a real window size): the main editor
    /// region (top-left) and the output strip (bottom).
    pub(super) fn body(&self, size: (i32, i32, Scale)) -> view::Body {
        let (width, height, scale) = size;
        let right = self.track_editor_width(width, scale);
        view::Body::new(width, height, scale, right, self.status.len(), self.output_shown(), self.note_editor_shown())
    }

    /// The note editor is shown, under the grid or the piano roll: the
    /// views that edit notes.
    pub(super) fn note_editor_shown(&self) -> bool {
        self.chrome.show_note_editor
            && self.kit_editor.is_none()
            && matches!(self.workspace, Workspace::Grid | Workspace::PianoRoll)
    }

    /// The master strip (the output panel) is shown along the bottom: in
    /// the app, unless View > Master folded it.
    pub(super) fn output_shown(&self) -> bool {
        !self.hosted && self.chrome.show_master_strip
    }

    /// What the window shows that sets the room the layout needs.
    pub fn shown(&self) -> view::Shown {
        view::Shown {
            output: self.output_shown(),
            track_editor: self.chrome.show_track_editor,
            grid: self.kit_editor.is_none() && self.workspace == Workspace::Grid,
            note_editor: self.chrome.show_note_editor,
        }
    }

    /// While the pattern shown waits for the one playing to end its loop:
    /// the blink of its marker, on in the first half of each beat.
    pub(super) fn waiting_blink(&self) -> Option<bool> {
        let playing = self.playback.controller()?.playing_pattern()?;
        if playing == self.pattern_index {
            return None;
        }
        let at = self.playback.position()?;
        let beat_count = self.editor.library().patterns.get(playing).map_or(1, |p| p.beat_count.max(1));
        let beat = at.frame as f64 / at.frames.max(1) as f64 * beat_count as f64;
        Some(beat.fract() < 0.5)
    }

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
                let (scales, mut rng) = (&self.scales, Rng::new(setup.seed));
                let styles = compose::learn(&self.styles, &library.patterns);
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
        let bytes = midi::export(pattern, bpm, 1, &self.roots, &mut rng, &self.scales);
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

    /// Show pattern `index` (it must exist), and play it if a pattern plays.
    pub(super) fn show_pattern(&mut self, index: usize) {
        self.pattern_index = index;
        self.deselect();
        self.chrome.current_page = 0;
        self.message = format!("pattern {}", self.pattern().and_then(|p| p.name.as_deref()).unwrap_or_default());
        // In pattern mode the new pattern plays once the one playing ends its loop.
        if let Some(controller) = self.playback.controller_mut()
            && let Mode::Pattern(_) = controller.mode()
        {
            controller.select_pattern(self.pattern_index);
            let library = self.editor.library();
            if let Some(name) = controller
                .playing_pattern()
                .filter(|&p| p != index)
                .and_then(|p| library.patterns.get(p))
                .map(|p| p.name.as_deref().unwrap_or_default())
            {
                self.message = format!("{} - next, after {name}", self.message);
            }
        }
    }

    /// V: show/hide the VU meters on the track stripes.
    pub fn toggle_vus(&mut self) {
        self.chrome.show_vus = !self.chrome.show_vus;
        self.message = if self.chrome.show_vus { "VU meters on".into() } else { "VU meters off".into() };
    }

    /// View > Toasts: show or hide the messages that pop up over the window.
    pub fn toggle_toasts(&mut self) {
        self.flash.hidden = !self.flash.hidden;
        if self.flash.hidden {
            self.flash.toasts.clear();
        }
        self.message = if self.flash.hidden { "toasts off".into() } else { "toasts on".into() };
    }

    // ─── Menus and commands ──────────────────────────────────────────────────

    /// A view button (`view::VIEWS`): show that view, unless it is shown.
    /// The last button (KIT) toggles the kit editor over the main editor.
    pub(super) fn show_view(&mut self, i: u8) {
        if i == 4 {
            self.toggle_kit_editor();
            return;
        }
        let target = [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth];
        let Some(&target) = target.get(usize::from(i)) else { return };
        if self.workspace == target && self.kit_editor.is_none() {
            return;
        }
        self.kit_editor = None;
        if self.workspace != target {
            match target {
                Workspace::Grid => {
                    self.synth_editor.close();
                    self.set_workspace(Workspace::Grid);
                }
                Workspace::PianoRoll => self.toggle_piano_roll(),
                Workspace::Song => self.toggle_song_view(),
                Workspace::Synth => self.toggle_synth(),
            }
        }
    }

    /// Show the grid (View > Grid, F5).
    pub(super) fn show_grid(&mut self) {
        self.synth_editor.close();
        self.set_workspace(Workspace::Grid);
    }

    /// The session is a plugin's: the host plays, saves and has the keys it
    /// leaves out of the menus.
    pub fn hosted(&self) -> bool {
        self.hosted
    }

    /// A session of `library` with no kit, a plugin's when `hosted`.
    #[cfg(test)]
    pub(crate) fn for_tests(library: Library, hosted: bool) -> Self {
        let mut session = Self::new(library, KitSet::empty(), Vec::new());
        session.hosted = hosted;
        session
    }

    /// Ask for the folder to write `export` to: the last one picked, at first
    /// the exports folder. What cannot be exported is said at once.
    pub(super) fn open_export(&mut self, export: Export) {
        let refusal = match export {
            Export::PatternWav | Export::SongWav if self.render.is_none() => Some(NOTHING_TO_RENDER),
            Export::Synths if self.synths.is_empty() => Some(NO_SYNTHS),
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

    /// Keep the engine fed and the playing pass current; call every frame
    /// while playing. The next pass of a pattern is built with the new lines
    /// of its tracks in auto mode (the JS loop start); they replace the library's
    /// when that pass starts.
    pub fn update(&mut self) {
        // What the library's tags teach, learned again after an edit (the header and menus read it).
        learned_styles(&mut self.learned_styles, &self.styles, &self.editor);
        self.refresh_synths();
        self.playback.update(&mut self.editor, self.pattern_index, |editor, p, pass| {
            auto_loop(editor, &mut self.learned_styles, &self.styles, &self.scales, &mut self.rng, p, pass)
        });
        self.report_host_tempo();
    }

    // ─── Editing ─────────────────────────────────────────────────────────────

    /// After an edit: remember to save, show what happened.
    fn edited(&mut self) {
        self.saving.library.touch();
        self.refresh_synths();
        if let Some(desc) = self.editor.history().next_undo() {
            self.message = desc.to_owned();
        }
    }

    /// Keep the pattern, cursor and selection inside the (changed) library.
    fn clamp_selection(&mut self) {
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
    fn after_history(&mut self, shown: Option<String>) {
        if let Some(index) = shown.and_then(|id| self.editor.library().patterns.iter().position(|p| p.id == id)) {
            self.pattern_index = index;
        }
        self.selection.selected = None;
        self.clamp_selection();
        self.saving.library.touch();
        self.refresh_synths();
    }

    /// The tempo a plugin plays at: the host's, inside the tempo bounds
    /// (`None` before the host's transport was seen).
    pub(super) fn host_bpm(&self) -> Option<f64> {
        let host = self.playback.host()?;
        Some(ordrumbox_core::song::clamp_bpm(host.bpm.round()))
    }

    /// The tempo the tracks play at: the host's in a plugin, else what
    /// plays (the song's, the library's), else the header's.
    pub(super) fn playing_bpm(&self) -> f64 {
        let library = self.editor.library();
        let own = match self.playback.mode().filter(|_| self.playback.is_playing()) {
            Some(Mode::Song(s)) => library.songs.get(s).map(|song| song.bpm),
            Some(Mode::Pattern(_)) => Some(library.bpm),
            None => None,
        };
        self.host_bpm().or(own).unwrap_or_else(|| self.tempo())
    }

    /// That tempo at `bpm` (whole, inside the tempo bounds), a run of
    /// changes one undo step.
    pub(super) fn set_tempo(&mut self, bpm: f64) {
        let bpm = ordrumbox_core::song::clamp_bpm(bpm.round());
        let changed = match self.tempo_song() {
            Some(s) => self.editor.set_song_bpm(s, bpm, true),
            None => self.editor.set_bpm(bpm, true),
        };
        if changed {
            self.edited();
        }
    }

    /// The pattern `beats` long (1–16).
    pub(super) fn set_length(&mut self, beats: i32) {
        let beats = beats.clamp(1, ordrumbox_logic::editor::MAX_BEATS);
        if self.editor.set_pattern_beat_count(self.pattern_index, f64::from(beats)) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    /// Remove every note of track `t`.
    pub(super) fn clear_track(&mut self, t: usize) {
        let Some(track) = self.track(t) else { return };
        if track.notes.is_empty() {
            self.message = "No notes to clear".into();
        } else {
            self.undoable(|s| s.clear_track_now(t));
        }
    }

    // ─── Saving ──────────────────────────────────────────────────────────────

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
    fn write_library(&mut self) -> bool {
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
    pub(super) fn library_state(&self) -> menu::LibraryState {
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

    fn autosave_library(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.library_ready(self.library_revision()) {
            return next;
        }
        self.write_library();
        None
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    /// The window's size and scale (where the lists opened from the menus go).
    pub fn set_size(&mut self, size: (i32, i32, Scale)) {
        self.chrome.panel_size = size;
    }

    pub fn draw(&mut self, canvas: &mut Canvas, scale: Scale) {
        // The scheme is per thread: a plugin draws on another than it loads on.
        theme::set_scheme(self.color_scheme);
        self.note_actions();
        self.note_message();
        let layout = self.panel_layout(canvas.width(), canvas.height(), scale);
        let position = self.playback.position();
        let history = self.editor.history();
        let library = self.editor.library();
        let song_mode = match self.playback.mode() {
            Some(Mode::Song(s)) => library.songs.get(s),
            _ => None,
        };
        let song = match (song_mode, position) {
            (Some(song), Some(p)) => {
                let total = song.length_measures().max(1.0);
                let measure = (p.pass as f64 + p.frame as f64 / p.frames.max(1) as f64) % total;
                Some(format!("SONG {}  measure {}/{}", song.name, measure.floor() + 1.0, total.ceil()))
            }
            _ => library.songs.get(self.editor.selected_song()).map(|s| format!("song {}", s.name)),
        };
        // Auto-page: keep the cursor / playhead on the current page.
        let size = (canvas.width(), canvas.height(), scale);
        let page_beats = view::page_beats(self.body(size).main_w, scale) as usize;
        let beat_count = self.pattern().map(|p| p.beat_count.max(1)).unwrap_or(1);
        let page_count = (beat_count as usize).div_ceil(page_beats).max(1);
        let cursor_page = self.selection.cursor.map(|c| (c.beat.max(0) as usize) / page_beats);
        // The pass playing is another pattern's while the one shown waits.
        let waiting = self.waiting_blink();
        let playhead_page = if self.playing() && waiting.is_none() {
            position.map(|p| {
                let frac = p.frame as f64 / p.frames.max(1) as f64;
                let global_beat = (frac * beat_count as f64).floor() as usize;
                global_beat / page_beats
            })
        } else {
            None
        };
        let new_page = playhead_page.or(cursor_page).unwrap_or(self.chrome.current_page);
        self.chrome.current_page = new_page.min(page_count.saturating_sub(1));

        // Smoothed per-track levels for the VU meter stripe (peak-hold + decay).
        let levels = self.chrome.smooth_levels(&self.playback.levels()).to_vec();
        let pattern = self.pattern();
        let song_layout = self.song_layout((canvas.width(), canvas.height(), scale));
        let rows = if self.workspace == Workspace::Song { self.song_rows() } else { Vec::new() };
        let roll_layout = self.roll_layout((canvas.width(), canvas.height(), scale));
        let roll = self.piano_roll_open().then(|| Some((self.roll_view(&roll_layout)?, self.roll_title()))).flatten();
        let view = View {
            pattern,
            sounds: pattern.map_or_else(Vec::new, |p| (0..p.tracks.len()).map(|t| self.track_sound_lines(t)).collect()),
            arrangement: self.song_view_open().then(|| self.song_view_data(&rows, &song_layout)),
            piano_roll: roll,
            output: self.output_view(),
            synth: self.synth_open(),
            kit_view: self.kit_view(layout.details_top),
            right: layout.right,
            current_page: self.chrome.current_page,
            lane: (self.editor_track(), usize::from(self.editors.lane)),
            note_editor: self.note_editor_shown(),
            levels,
            show_vus: self.chrome.show_vus,
            transport: !self.hosted,
            host_muted: self.host_muted,
            track_drop: self.track_drop((canvas.width(), canvas.height(), scale)),
            waiting,
            playhead: if song_mode.is_some() || waiting.is_some() {
                None
            } else {
                position.map(|p| {
                    let global_frac = p.frame as f64 / p.frames.max(1) as f64;
                    let total_beats = self.pattern().map(|pat| pat.beat_count.max(1)).unwrap_or(1) as f64;
                    let global_beat = global_frac * total_beats;
                    let start_beat = (self.chrome.current_page * page_beats) as f64;
                    let page_end = (start_beat + page_beats as f64).min(total_beats);
                    ((global_beat - start_beat) / (page_end - start_beat).max(1.0)).clamp(0.0, 1.0)
                })
            },
            song: song.as_deref().filter(|_| !self.hosted),
            cursor: self.selection.cursor,
            selected: self.selection.selected,
            undo: history.next_undo(),
            redo: history.next_redo(),
            message: &self.message,
            status: &self.status,
        };
        view::draw(canvas, &view, scale);
        self.draw_synth(canvas, scale);
        crate::panel::draw(canvas, &layout.items, self.chrome.widgets.focus, scale);
        self.draw_tooltip(canvas, scale);
        let menus = self.menus();
        menu::draw(canvas, &menus, self.chrome.menu, scale);
        // In the bar, which the open menu's panel hangs under.
        menu::draw_library(canvas, &menus, &self.song_name(), self.library_state(), scale);
        if let Some(dropdown) = &self.chrome.widgets.dropdown {
            dropdown.draw(canvas, scale);
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            entry.draw(canvas, scale);
        }
        for (rect, toast) in self.flash.rects(canvas.width(), canvas.height(), scale) {
            view::draw_toast(canvas, &toast.text, rect.y + rect.h, scale);
        }
        match &self.chrome.overlay {
            Some(Overlay::Picker(p)) => {
                let layout = PickerLayout::new(canvas.width(), canvas.height(), scale);
                picker::draw(canvas, p, &layout);
            }
            Some(Overlay::Shortcuts { scroll }) => {
                let lines: Vec<String> = self.shortcut_lines().iter().map(|l| (*l).to_owned()).collect();
                view::draw_notice_scrolled(canvas, "Keyboard shortcuts", &lines, *scroll, scale);
            }
            Some(Overlay::About) => view::draw_notice(canvas, "About orDrumbox", &self.about(), scale),
            Some(Overlay::History { scroll }) => {
                let layout = LogLayout::new(canvas.width(), canvas.height(), scale);
                action_log::draw(canvas, &self.action_log, *scroll, &layout);
            }
            Some(Overlay::FactoryReset) => {
                view::draw_notice(canvas, "Factory reset", &self.factory_reset_lines(), scale)
            }
            Some(Overlay::ConfirmOverwrite { path, .. }) => {
                // The name alone: the folder was just picked, and a long path would not fit.
                let name = path.file_name().unwrap_or(path.as_os_str()).to_string_lossy();
                let lines = [
                    format!("{name} already exists in this folder."),
                    String::new(),
                    "ENTER: replace it   ESC: keep it".to_owned(),
                ];
                view::draw_notice(canvas, "Replace the file?", &lines, scale)
            }
            Some(Overlay::Confirm(pending)) => {
                let (title, lines) = self.confirm_text(pending);
                view::draw_notice(canvas, title, &lines, scale)
            }
            None => {}
        }
    }
}
