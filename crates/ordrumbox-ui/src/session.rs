//! Everything the window drives: the library being edited, the playback, the
//! generators, MIDI files, the grid cursor and selection, the clipboard, saving. Edits go through the
//! [`Editor`] (each one an undo step); the playing pass is rebuilt after each.
//!
//! The song panel (F3) and the piano roll (F4) take the grid's place:
//! `session_song.rs`, `session_roll.rs`.
//!
//! The grid follows the JS pattern panel (`pointer_section.js`,
//! `keyboard_section.js`, `clipboard_section.js`): a click on an empty step
//! adds a note, on a note selects it, on the selected note deletes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_core::{Library, Note, Pattern, Rng, ScaleList, Scales, Styles, Track};
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
#[path = "session_confirm.rs"]
mod session_confirm;
#[path = "session_grid.rs"]
mod session_grid;
#[path = "session_zoom.rs"]
mod session_zoom;

/// A style's name in the menus: a learned one says from how many patterns.
pub fn style_label(style: &ordrumbox_core::Style) -> String {
    match style.learned {
        Some((patterns, measures)) => format!("{} (learned: {patterns} patterns, {measures} measures)", style.name),
        None => style.name.clone(),
    }
}

#[cfg(test)]
#[path = "session_library_file.rs"]
mod session_library_file;
#[path = "session_mcp.rs"]
mod session_mcp;
#[path = "session_output.rs"]
mod session_output;
#[path = "session_panel.rs"]
mod session_panel;
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
#[path = "session_synth.rs"]
mod session_synth;
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
    synths: SynthPresets,
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

/// Every key, for Help > Keyboard shortcuts.
const SHORTCUTS: [&str; 59] = [
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1 F2 F3 F4 F5 F6      this list, track editor, song, piano roll, grid, synth (Ctrl+K: kit editor)",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          switch the step on (a note) or off (all its notes)",
    "  Shift+click            select the step's note for the note editor, and hear it",
    "  Shift+Arrows           velocity (up/down), pitch (left/right) of the selected note",
    "  Delete                 clear the cursor step        Esc  deselect",
    "  L                      accent the cursor step's notes, or take the accent off",
    "  right-click            a track's name: track menu   a step: notes menu",
    "  F, Shift+G, V          random pattern, random drumkit, VU meters on/off",
    "  C                      next colour scheme",
    "  Shift+H, J, K          every track: synth sounds, auto-assigned, random samples",
    "PLAY",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10 (any view)",
    "  Space / Shift+Space    play or stop the pattern / the song",
    "  [ ] / Shift+[ ]        previous / next pattern / song (also PageUp PageDown)",
    "  - = (Shift: x10)       tempo",
    "  , .                    previous / next kit",
    "FILE AND EDIT",
    "  Ctrl+S                 save the library now (it is also saved on its own)",
    "  Ctrl+Shift+S           export the pattern as JSON",
    "  Ctrl+O, Ctrl+Q         import a MIDI file, quit",
    "  Ctrl+E, Ctrl+M         export WAV (Shift: the song), export MIDI",
    "  Ctrl+Z, Ctrl+Y         undo, redo (also Ctrl+Shift+Z)",
    "  Ctrl+C, Ctrl+V         copy, paste the step (Shift: the track)",
    "  Ctrl+N, Ctrl+D         new pattern, duplicate pattern",
    "  Ctrl+T, Ctrl+Backspace add a KICK track, remove a track",
    "  Ctrl+G, Ctrl+L         next grid, loop one step shorter",
    "  Ctrl+- Ctrl+=          pattern length",
    "  Ctrl++ Ctrl+_ Ctrl+0   zoom in, out, back to the display's size (also Ctrl and keypad + -)",
    "GENERATE",
    "  B, Shift+B             the pattern again in its style, a new pattern in the style",
    "  D, S, A                drums, bass, chords in auto mode (again: off)",
    "  Z, X                   track in auto mode, random notes on the track",
    "KNOBS, SLIDERS, LISTS (track editor, note editor, kit editor, volumes)",
    "  Tab / Shift+Tab        focus the next / previous control      Esc  back to the grid",
    "  Arrows, - =, wheel     change (Shift or Alt: finely, Ctrl: x10); a list: next option",
    "  drag                   a knob up / down, a slider along (Shift: finely)",
    "  double-click           default value     right-click or Enter: type a value",
    "  Enter or Space         open a list, press a button",
    "PIANO ROLL (F4)",
    "  click / drag / right-click a cell: add, select, delete; velocity (up/down) or pitch; menu",
    "  Arrows, Shift+Arrows   the cursor; the selected note's velocity / pitch    Enter, Delete",
    "  a key: hear it, right-click: clear, chord progression     wheel (Shift: pages)",
    "SYNTH EDITOR (F6)",
    "  OSC, FLT, MOD, ENV     the track's preset: knobs, waves, lists, power buttons (saved)",
    "OUTPUT PANEL (bottom strip, always shown)",
    "  Volume, Comp, Filter   the master volume and spectrum, the compressor, the filters (saved)",
    "SONG PANEL (F3)",
    "  click / right-click    the ruler: aim the cursor; a pattern, row, clip, the grid: menu",
    "  Left Right, Up Down    the cursor a measure back / on, previous / next pattern",
    "  Space, wheel           play the song from the cursor; scroll (Shift: the rows), or drag the bars",
    "KIT EDITOR (Ctrl+K)",
    "  Up Down                sample      Enter  hear      Delete  remove",
    "  N, E                   normalize, export the mapping",
    "  drop files             a .wav joins the kit (Shift: replaces the sample), a folder",
    "                         becomes a kit, a .json mapping loads, a .mid imports",
];

/// Every key of a plugin, which leaves Space, the Ctrl/Cmd keys and the songs
/// to the host (`Ui::key`).
const PLUGIN_SHORTCUTS: &[&str] = &[
    "THE HOST'S KEYS",
    "  Space, every Ctrl/Cmd key   the host's: its transport, its save, undo, zoom...",
    "  Alt + the app's Ctrl key    orDrumbox's: Alt+Z undo, Alt+C copy, Alt+K kit editor...",
    "                              (the menus show them; greyed out: the host's or the app's)",
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1 F2 F4 F5 F6         this list, track editor, piano roll, grid, synth",
    "  songs, tempo, saving   the app's or the host's: no F3, no - = (the tempo is HOST's)",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          switch the step on (a note) or off (all its notes)",
    "  Shift+click            select the step's note for the note editor, and hear it",
    "  Shift+Arrows           velocity (up/down), pitch (left/right) of the selected note",
    "  Delete                 clear the cursor step        Esc  deselect",
    "  L                      accent the cursor step's notes, or take the accent off",
    "  right-click            a track's name: track menu   a step: notes menu",
    "  F, Shift+G, V          random pattern, random drumkit, VU meters on/off",
    "  C                      next colour scheme",
    "  Shift+H, J, K          every track: synth sounds, auto-assigned, random samples",
    "PLAY",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10 (any view)",
    "  [ ]                    previous / next pattern (also PageUp PageDown)",
    "  , .                    previous / next kit",
    "GENERATE",
    "  B, Shift+B             the pattern again in its style, a new pattern in the style",
    "  D, S, A                drums, bass, chords in auto mode (again: off)",
    "  Z, X                   track in auto mode, random notes on the track",
    "KNOBS, SLIDERS, LISTS (track editor, note editor, kit editor, volumes)",
    "  Tab / Shift+Tab        focus the next / previous control      Esc  back to the grid",
    "  Arrows, - =, wheel     change (Shift or Alt: finely); a list: next option",
    "  drag                   a knob up / down, a slider along (Shift: finely)",
    "  double-click           default value     right-click or Enter: type a value",
    "  Enter                  open a list, press a button",
    "PIANO ROLL (F4)",
    "  click / drag / right-click a cell: add, select, delete; velocity (up/down) or pitch; menu",
    "  Arrows, Shift+Arrows   the cursor; the selected note's velocity / pitch    Enter, Delete",
    "  a key: hear it, right-click: clear, chord progression     wheel (Shift: pages)",
    "SYNTH EDITOR (F6)",
    "  OSC, FLT, MOD, ENV     the track's preset: knobs, waves, lists, power buttons (saved)",
    "KIT EDITOR (Alt+K)",
    "  Up Down                sample      Enter  hear      Delete  remove",
    "  N, E                   normalize, export the mapping",
    "  drop files             a .wav joins the kit (Shift: replaces the sample), a folder",
    "                         becomes a kit, a .json mapping loads, a .mid imports",
];

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
        let mut assets = match assets::load(&dir) {
            Ok(assets) => assets,
            Err(e) => return Self::new(Library::default(), KitSet::empty(), vec![e]),
        };
        let mut status: Vec<String> = assets.errors.iter().map(|e| format!("not loaded: {e}")).collect();
        status.extend(assets.style_issues.iter().map(|i| format!("styles.json: {}: {}", i.at, i.message)));
        if let Some(first) = assets.sample_errors.first() {
            status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
        }

        let hosted = matches!(source, LibrarySource::Host(_));
        let (library, library_path, message) = match source {
            LibrarySource::UserFile => {
                if let Some(problem) = store::location_problem() {
                    status.push(format!("{problem}: File > Change user data folder"));
                }
                Self::open_library(store::path(), assets.library, &mut status)
            }
            LibrarySource::Host(text) => Self::open_host_library(text, assets.library, &mut status),
        };
        if library_path.is_none() && !assets.report.issues.is_empty() {
            status.push(format!("library.json: {} value(s) repaired", assets.report.issues.len()));
        }

        if let Some(e) = &assets.kits_error {
            status.push(format!("kits.json not loaded, kit edits not saved: {e}"));
        }
        let kits_file = if assets.kits_error.is_none() { store::kits_path() } else { None };
        let kits =
            KitSet::new(assets.kits, &assets.bank, Some(assets.sources.clone()), kits_file, store::samples_dir());
        // The synth presets edited, over the assets' ones.
        let synths_file =
            Self::load_user_synths(store::saved_synths_path(), store::synths_path(), &mut assets.synths, &mut status);
        let mut session = Self::new(library, kits, status);
        session.hosted = hosted;
        session.assets_dir = Some(dir.clone());
        session.saving.songs_dir = store::songs_dir();
        session.scales = assets.scales.clone();
        session.styles = assets.styles.clone();
        session.scale_list = assets.scale_list.clone();
        session.synths = assets.synths.clone();
        session.roots =
            assets.bank.samples.iter().filter_map(|s| Some((s.sample.key.clone(), s.sample.root_midi?))).collect();
        session.saving.library_path = library_path;
        session.saving.synths_file = synths_file;
        session.message = message;
        let bank = Arc::new(assets.bank);
        if !bank.samples.is_empty() {
            session.render = Some(RenderSetup {
                bank: bank.clone(),
                scales: assets.scales.clone(),
                synths: assets.synths.clone(),
                kit: session.kits.kits.first().map(|k| k.name.clone()).unwrap_or_default(),
                sample_rate: offline::EXPORT_RATE,
                seed: 1,
            });
        }
        match start_output(bank, assets.scales, assets.synths) {
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
    /// one, else `demo`; the file edits are saved to (`None`: not saved, the
    /// reason pushed on `status`); the message to show.
    fn open_library(
        path: Option<PathBuf>,
        demo: Library,
        status: &mut Vec<String>,
    ) -> (Library, Option<PathBuf>, String) {
        let demo_message = "demo library (assets/data/library.json)".to_owned();
        match path {
            None => {
                status.push("no user data folder: edits are not saved".into());
                (demo, None, demo_message)
            }
            Some(path) => match store::load(&path) {
                Ok(Some(saved)) => {
                    let message = format!("library {}", path.display());
                    (saved, Some(path), message)
                }
                Ok(None) => (demo, Some(path), demo_message),
                Err(e) => {
                    status.push(format!("library not loaded, not saved over: {e}"));
                    (demo, None, demo_message)
                }
            },
        }
    }

    /// The library of a plugin: the host's `text` when it kept one, else the
    /// user's file (read, never written), else `demo`; no file to save to; the
    /// message to show.
    fn open_host_library(
        text: Option<String>,
        demo: Library,
        status: &mut Vec<String>,
    ) -> (Library, Option<PathBuf>, String) {
        if let Some(text) = text {
            return match Library::parse(&text) {
                Ok((library, _)) => (library, None, "library from the host project".to_owned()),
                Err(e) => {
                    status.push(format!("library of the host project not loaded: {e}"));
                    (demo, None, "demo library (assets/data/library.json)".to_owned())
                }
            };
        }
        match store::path().map(|path| (store::load(&path), path)) {
            Some((Ok(Some(saved)), path)) => {
                (saved, None, format!("library {} (copied into the host project)", path.display()))
            }
            Some((Err(e), _)) => {
                status.push(format!("library not loaded: {e}"));
                (demo, None, "demo library (assets/data/library.json)".to_owned())
            }
            Some((Ok(None), _)) | None => (demo, None, "demo library (assets/data/library.json)".to_owned()),
        }
    }

    /// The library as its file holds it, for a plugin host to keep in its project.
    /// Changes made to the library so far: compare to know when to ask
    /// [`library_text`](Self::library_text) again.
    pub fn library_revision(&self) -> u64 {
        self.editor.revision()
    }

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
            synths: SynthPresets::default(),
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
            action_log: ActionLog::default(),
            assets_dir: None,
            quit: false,
            scale_list: ScaleList::default(),
            editors: EditorState::default(),
            color_scheme: 1,
            hosted: false,
            host_muted: 0,
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

    /// What the last action said.
    /// Show `text` on the status line.
    pub fn say(&mut self, text: &str) {
        self.message = text.into();
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    /// The window split for `size` (a real window size): the main editor
    /// region (top-left) and the output strip (bottom).
    pub(super) fn body(&self, size: (i32, i32, i32)) -> view::Body {
        let (width, height, scale) = size;
        view::Body::new(width, height, scale, self.track_editor_width(width, scale), self.status.len(), !self.hosted)
    }

    pub fn playing(&self) -> bool {
        self.playback.is_playing()
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

    // ─── Transport ───────────────────────────────────────────────────────────

    /// Space: play or stop the selected pattern (in the song panel: the song).
    pub fn toggle_play(&mut self) {
        if self.workspace == Workspace::Song {
            return self.toggle_play_song();
        }
        self.toggle(Mode::Pattern(self.pattern_index), 0);
    }

    /// Shift+Space: play or stop the selected song (its arrangement), from the cursor.
    pub fn toggle_play_song(&mut self) {
        if self.hosted {
            self.message = menu::SONGS_IN_THE_APP.into();
            return;
        }
        if self.editor.library().songs.is_empty() {
            self.message = "no song in the library".into();
            return;
        }
        let start = self.song_start();
        self.toggle(Mode::Song(self.editor.selected_song()), start);
    }

    /// Stop, or play `mode` from pass `first`.
    fn toggle(&mut self, mode: Mode, first: u64) {
        if self.playback.controller().is_none() {
            return;
        }
        if self.playback.is_playing() {
            self.playback.stop();
        } else {
            if let Mode::Pattern(p) = mode {
                auto_loop(&mut self.editor, &mut self.learned_styles, &self.styles, &self.scales, &mut self.rng, p, 0);
            }
            self.playback.play_from(&self.editor, mode, first);
        }
    }

    /// Shift+[ ]: previous / next song.
    pub fn step_song(&mut self, delta: isize) {
        let count = self.editor.library().songs.len();
        if count == 0 {
            self.message = "no song in the library".into();
            return;
        }
        let index = (self.editor.selected_song() as isize + delta).rem_euclid(count as isize) as usize;
        self.select_song(index);
    }

    /// Ctrl+E: the selected pattern (with Shift: the selected song) to a WAV
    /// file at `path`.
    fn export_wav(&mut self, song: bool, path: &Path) {
        let mode = if song { Mode::Song(self.editor.selected_song()) } else { Mode::Pattern(self.pattern_index) };
        self.message = self.write_wav(mode, path).unwrap_or_else(|e| e);
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

    /// Ctrl+M: the selected pattern to a MIDI file at `path`.
    fn export_midi(&mut self, path: &Path) {
        if self.pattern().is_none() {
            return;
        }
        self.message = self.write_midi(self.pattern_index, path).unwrap_or_else(|e| e);
    }

    /// Pattern `p` as a MIDI file at `path`; what was written.
    pub(super) fn write_midi(&self, p: usize, path: &Path) -> Result<String, String> {
        let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
        let mut rng = Rng::new(1);
        let bytes = midi::export(pattern, 1, &self.roots, &mut rng, &self.scales);
        store::write_atomic(path, &bytes).map_err(|e| format!("not exported: {}: {e}", path.display()))?;
        Ok(format!("exported {} ({} bytes)", path.display(), bytes.len()))
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

    /// Pattern `p` as JSON at `path`.
    pub(super) fn write_pattern_json(&self, p: usize, path: &Path) -> Result<(), String> {
        let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
        let text = pattern.to_json().to_json_pretty(2) + "\n";
        store::write_atomic(path, text).map_err(|e| format!("not exported: {}: {e}", path.display()))
    }

    /// A MIDI file dropped on the window (or given on the command line):
    /// new patterns from it, the last one selected.
    pub fn import_midi(&mut self, path: &Path) {
        if let Err(e) = self.import_midi_file(path) {
            self.message = e;
        }
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

    pub fn step_pattern(&mut self, delta: isize) {
        let count = self.editor.library().patterns.len();
        if count > 0 {
            self.show_pattern((self.pattern_index as isize + delta).rem_euclid(count as isize) as usize);
        }
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

    pub fn step_kit(&mut self, delta: isize) {
        let count = self.kits.kits.len();
        if count == 0 {
            return;
        }
        self.use_kit((self.kit_index as isize + delta).rem_euclid(count as isize) as usize);
    }

    /// The pattern shown, from 0.
    pub fn pattern_index(&self) -> usize {
        self.pattern_index
    }

    /// The kit used, from 0.
    pub fn kit_index(&self) -> usize {
        self.kit_index
    }

    /// Pattern `index` (from 0) shown, and played in pattern mode, as a
    /// plugin host's parameter asks. False, with a message, when the library
    /// has no such pattern. In song mode the song plays on, and the message says so.
    pub fn host_select_pattern(&mut self, index: usize) -> bool {
        let count = self.editor.library().patterns.len();
        if index >= count {
            self.message = format!("host: no pattern {} (the library has {count})", index + 1);
            return false;
        }
        if index != self.pattern_index {
            self.show_pattern(index);
            if matches!(self.playback.mode(), Some(Mode::Song(_))) {
                self.message = format!("host: {}, shown only: a song plays", self.message);
            }
        }
        true
    }

    /// Kit `index` (from 0) used, as a plugin host's parameter asks. False,
    /// with a message, when there is no such kit.
    pub fn host_select_kit(&mut self, index: usize) -> bool {
        let count = self.kits.kits.len();
        if index >= count {
            self.message = format!("host: no kit {} (there are {count})", index + 1);
            return false;
        }
        if index != self.kit_index {
            self.use_kit(index);
        }
        true
    }

    /// The tracks a plugin host mutes, bit `t` for track `t`, shown on their
    /// mute boxes; true when they changed.
    pub fn set_host_muted(&mut self, muted: u64) -> bool {
        std::mem::replace(&mut self.host_muted, muted) != muted
    }

    /// Show `message` as the outcome of the last action.
    pub fn tell(&mut self, message: String) {
        self.message = message;
    }

    /// The names of the library's patterns, in order (`None`: no name).
    pub fn pattern_names(&self) -> impl Iterator<Item = Option<&str>> + Clone {
        self.editor.library().patterns.iter().map(|p| p.name.as_deref())
    }

    /// The names of the kits, in order.
    pub fn kit_names(&self) -> impl Iterator<Item = &str> + Clone {
        self.kits.kits.iter().map(|k| k.name.as_str())
    }

    /// Use kit `index` and say so.
    fn use_kit(&mut self, index: usize) {
        self.select_kit(index);
        if let Some(kit) = self.kits.kits.get(index) {
            self.message = format!("kit {}", kit.name);
        }
    }

    /// Use kit `index` (its sounds are assigned again).
    fn select_kit(&mut self, index: usize) {
        let Some(kit) = self.kits.kits.get(index) else { return };
        self.kit_index = index;
        // A kit change affects every track's sound: rebuild the pass
        // playing now so it is heard at once (not at the next loop).
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_kit(&kit.name);
            controller.refresh(self.editor.library());
        }
        if let Some(render) = &mut self.render {
            render.kit.clone_from(&kit.name);
        }
        if let Some(sample) = self.kit_editor {
            self.kit_editor = Some(sample.min(kit.samples.len().saturating_sub(1)));
        }
        self.update_analysis();
    }

    /// F: a random pattern.
    pub fn select_random_pattern(&mut self) {
        let count = self.editor.library().patterns.len();
        if count == 0 {
            return;
        }
        let index = self.rng.below(count as u64) as usize;
        self.pattern_index = index;
        self.deselect();
        self.edited();
        self.chrome.current_page = 0;
        self.message = format!("pattern {}", self.pattern().and_then(|p| p.name.as_deref()).unwrap_or_default());
    }

    /// G: a random drumkit.
    pub fn select_random_drumkit(&mut self) {
        let count = self.kits.kits.len();
        if count == 0 {
            return;
        }
        let index = self.rng.below(count as u64) as usize;
        self.use_kit(index);
    }

    /// V: show/hide the VU meters on the track stripes.
    pub fn toggle_vus(&mut self) {
        self.chrome.show_vus = !self.chrome.show_vus;
        self.message = if self.chrome.show_vus { "VU meters on".into() } else { "VU meters off".into() };
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

    /// Generation on (tracks in auto mode): drums, bass, chords.
    fn generation(&self) -> [bool; 3] {
        let generating = |kind: fn(InstrumentFamily) -> bool| {
            self.pattern()
                .is_some_and(|p| p.tracks.iter().any(|t| generate::is_auto_on(t) && kind(instrument_family(&t.name))))
        };
        [
            generating(InstrumentFamily::is_drum),
            generating(|k| k == InstrumentFamily::Bass),
            generating(|k| k == InstrumentFamily::Piano),
        ]
    }

    /// What the menus show now.
    fn menu_state(&self) -> MenuState {
        let history = self.editor.history();
        let pattern = self.pattern();
        let focus = self.focus_track().and_then(|t| self.track(t)).map(|t| (t.auto_mode, t.mute, t.solo));
        MenuState {
            can_save: self.saving.library_path.is_some(),
            can_render: self.render.is_some(),
            has_pattern: pattern.is_some(),
            undo: history.next_undo().map(str::to_owned),
            redo: history.next_redo().map(str::to_owned),
            has_cursor: self.selection.cursor.is_some(),
            focus_track: focus,
            can_remove_track: focus.is_some() && pattern.is_some_and(|p| p.tracks.len() > 1),
            clipboard_step: matches!(self.selection.clipboard, Some(Clipboard::Step(_))),
            clipboard_track: matches!(self.selection.clipboard, Some(Clipboard::Track(_))),
            has_selection: self.selection.selected.is_some(),
            playing: self.playing(),
            has_songs: !self.editor.library().songs.is_empty(),
            generation: self.generation(),
            auto_all: self.pattern_auto(),
            locked: self.focus_track().and_then(|t| self.track(t)).is_some_and(|t| t.compose_lock),
            kits: self.kits.kits.iter().map(|k| k.name.clone()).collect(),
            kit_index: self.kit_index,
            kit_editor: self.kit_editor.is_some(),
            track_editor: self.chrome.show_track_editor,
            grid: self.workspace == Workspace::Grid && self.kit_editor.is_none(),
            show_vus: self.chrome.show_vus,
            song_view: self.song_view_open(),
            piano_roll: self.piano_roll_open(),
            synth: self.synth_open(),
            can_save_song: self.saving.songs_dir.is_some(),
            saved_songs: self
                .saving
                .songs_dir
                .as_ref()
                .is_some_and(|d| store::list_songs(d).is_ok_and(|s| !s.is_empty())),
            kit_sample: self.kit_list().is_some_and(|(s, n)| s < n),
            checking_updates: self.updates.checking(),
            newer_release: self.updates.newer_release().map(|r| r.version.clone()),
            hosted: self.hosted,
            has_demo: self.has_demo(),
            user_folder: self.user_folder().is_some(),
        }
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

    pub fn menus(&self) -> Vec<menu::Menu> {
        menu::menus(&self.menu_state())
    }

    /// The open menu, if any.
    pub fn open_menu(&self) -> Option<menu::Open> {
        self.chrome.menu
    }

    pub fn set_menu(&mut self, open: Option<menu::Open>) {
        self.chrome.menu = open;
    }

    /// File > Quit was chosen.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Run a menu command (or its shortcut).
    pub fn run(&mut self, command: Command) {
        use Command as C;
        self.chrome.menu = None;
        if let Some(refusal) = command.hosted_refusal().filter(|_| self.hosted) {
            self.message = refusal.into();
            return;
        }
        match command {
            C::Save => self.save(),
            C::ImportMidi => self.open_picker(Purpose::ImportMidi),
            C::ExportWav => self.open_export(Export::PatternWav),
            C::ExportSongWav => self.open_export(Export::SongWav),
            C::ExportMidi => self.open_export(Export::PatternMidi),
            C::Quit => self.quit = true,
            C::Undo => self.undo(),
            C::Redo => self.redo(),
            C::CopyStep => self.copy(false),
            C::CopyTrack => self.copy(true),
            C::Paste => self.paste(false),
            C::PasteTrack => self.paste(true),
            C::ClearStep => self.delete_step(),
            C::ToggleAccent => self.toggle_accent(),
            C::Deselect => self.deselect(),
            C::NewPattern => self.new_pattern(),
            C::DuplicatePattern => self.duplicate_pattern(),
            C::RenamePattern => self.text_entry(Id::SongPattern(self.pattern_index)),
            C::PatternDescription => self.text_entry(Id::PatternDescription),
            C::PreviousPattern => self.step_pattern(-1),
            C::NextPattern => self.step_pattern(1),
            C::TempoDown => self.step_tempo(-1.0),
            C::TempoUp => self.step_tempo(1.0),
            C::LengthDown => self.change_length(-1),
            C::LengthUp => self.change_length(1),
            C::AddTrack => self.add_track(),
            C::ChooseTrack => self.add_track_menu_at_button(),
            C::RemoveTrack => self.remove_track(),
            C::NextGrid => self.next_grid(),
            C::ShortenLoop => self.shorten_loop(),
            C::CompactTracks => self.compact_tracks(),
            C::GeneratePattern => self.regenerate_pattern(),
            C::GenerateDrums => self.toggle_generation(None),
            C::GenerateBass => self.toggle_generation(Some(MelodicKind::Bass)),
            C::GenerateChords => self.toggle_generation(Some(MelodicKind::Chords)),
            C::TrackAuto => self.toggle_auto(),
            C::RandomTrack => self.randomize_track(),
            C::PlayPattern => self.toggle_play(),
            C::PlaySong => self.toggle_play_song(),
            C::PreviousSong => self.step_song(-1),
            C::NextSong => self.step_song(1),
            C::Mute | C::Solo => {
                if let Some(t) = self.focus_track() {
                    self.toggle_row(t, if command == C::Mute { "mute" } else { "solo" });
                }
            }
            C::NewGeneratedPattern => self.generate_pattern(),
            C::AutoAll => self.toggle_pattern_auto(),
            C::LockTrack => self.toggle_lock(),
            C::Kit(i) => self.use_kit(i),
            C::PreviousKit => self.step_kit(-1),
            C::NextKit => self.step_kit(1),
            C::KitEditor => self.toggle_kit_editor(),
            C::TrackEditor => self.toggle_track_editor(),
            C::SongView => self.toggle_song_view(),
            C::PianoRoll => self.toggle_piano_roll(),
            C::Grid => self.show_grid(),
            C::SynthEditor => self.toggle_synth(),
            C::NewSong => self.song_button("new"),
            C::RemoveSong => self.song_button("delete"),
            C::RenameSong => self.text_entry(Id::SongName),
            C::SongDescription => self.text_entry(Id::SongDescription),
            C::SaveSong => self.song_button("save"),
            C::SaveSongAs => self.song_button("saveas"),
            C::LoadSong => self.song_button("load"),
            C::ExportSong => self.song_button("export"),
            C::ImportSong => self.song_button("import"),
            C::NormalizeKit => self.kit_normalize(),
            C::ExportMapping => self.open_export(Export::KitMapping),
            C::LoadMapping => self.open_picker(Purpose::LoadMapping),
            C::AddSample => self.open_picker(Purpose::AddSample),
            C::ReplaceSample => self.open_picker(Purpose::ReplaceSample),
            C::ImportFolder => self.open_picker(Purpose::ImportFolder),
            C::SelectRandomPattern => self.select_random_pattern(),
            C::SelectRandomDrumkit => self.select_random_drumkit(),
            C::ToggleVus => self.toggle_vus(),
            C::NextColorScheme => self.next_color_scheme(),
            C::ZoomIn => self.zoom(1),
            C::ZoomOut => self.zoom(-1),
            C::ZoomReset => self.zoom(0),
            C::ExportPatternJson => self.open_export(Export::PatternJson),
            C::ConvertToSynth => self.convert_to_generated_sounds(self.pattern_index),
            C::AutoAssignAll => self.auto_assign_all_tracks(self.pattern_index),
            C::RandomSamples => self.assign_random_samples(self.pattern_index),
            C::PreviewTrack => {
                if let Some(t) = self.focus_track() {
                    self.audition_row(t);
                }
            }
            C::Shortcuts => self.chrome.overlay = Some(Overlay::Shortcuts { scroll: 0 }),
            C::About => self.chrome.overlay = Some(Overlay::About),
            C::FactoryReset => self.chrome.overlay = Some(Overlay::FactoryReset),
            C::ExportLibrary => self.open_export(Export::Library),
            C::ImportLibrary => self.open_picker(Purpose::ImportLibrary),
            C::DemoLibrary => self.ask(Pending::DemoLibrary),
            C::BackUpUserData => self.open_export(Export::UserData),
            C::RestoreUserData => self.open_restore(),
            C::ChangeUserFolder => self.open_user_folder_picker(),
            C::DefaultUserFolder => self.use_default_user_folder(),
            C::OpenUserFolder => self.open_user_folder(),
            C::CheckUpdates => self.check_updates(true),
            C::DownloadPage => self.open_download_page(),
        }
    }

    fn open_picker(&mut self, purpose: Purpose) {
        self.chrome.overlay = Some(Overlay::Picker(Picker::new(purpose, self.chrome.last_dir.as_deref())));
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

    /// The name of the file `export` writes; `None` when what it writes is gone.
    fn export_file_name(&self, export: Export) -> Option<String> {
        self.export_file_name_of(export, self.pattern_index, self.editor.selected_song())
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
            Export::Library => session_user_data::LIBRARY_FILE_NAME.to_owned(),
            Export::UserData => crate::user_data::backup_name(&crate::user_data::now_stamp()),
        })
    }

    /// The folder is picked: write `export` there, once confirmed if that
    /// replaces a file.
    fn export_to(&mut self, export: Export, dir: &Path) {
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

    fn write_export(&mut self, export: Export, path: &Path) {
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

    /// A panel (file picker, key list, about) is over the window.
    pub fn overlay_open(&self) -> bool {
        self.chrome.overlay.is_some()
    }

    pub fn close_overlay(&mut self) {
        self.chrome.overlay = None;
    }

    /// A click on the status line: what was done so far.
    pub fn open_history(&mut self) {
        self.note_actions();
        self.chrome.overlay = Some(Overlay::History { scroll: 0 });
    }

    /// Add what the last events did to the log.
    fn note_actions(&mut self) {
        let history = self.editor.history();
        self.action_log.note(Seen {
            message: &self.message,
            revision: self.editor.revision(),
            steps: history.undo_len(),
            undo: history.next_undo(),
            dragging: self.chrome.widgets.drag.is_some(),
        });
    }

    /// The lines of the keyboard shortcuts panel.
    fn shortcut_lines(&self) -> &'static [&'static str] {
        if self.hosted { PLUGIN_SHORTCUTS } else { &SHORTCUTS }
    }

    /// Scroll the shortcuts panel, if open, for `key`; whether it was.
    fn shortcuts_key(&mut self, key: &OverlayKey) -> bool {
        let count = self.shortcut_lines().len();
        let Some(Overlay::Shortcuts { scroll }) = &mut self.chrome.overlay else { return false };
        let (_, height, scale) = self.chrome.panel_size;
        let max = count.saturating_sub(view::notice_rows(height, scale));
        match key {
            OverlayKey::Move(delta) => *scroll = (*scroll).min(max).saturating_add_signed(*delta).min(max),
            OverlayKey::Edge(end) => *scroll = if *end { max } else { 0 },
            _ => self.chrome.overlay = None,
        }
        true
    }

    /// Scroll the history panel, if open, for `key`; whether it was.
    fn history_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::History { scroll }) = &mut self.chrome.overlay else { return false };
        let (width, height, scale) = self.chrome.panel_size;
        let max = LogLayout::new(width, height, scale).max_scroll(&self.action_log);
        match key {
            OverlayKey::Move(delta) => *scroll = (*scroll).min(max).saturating_add_signed(*delta).min(max),
            OverlayKey::Edge(end) => *scroll = if *end { max } else { 0 },
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Enter | OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// A key while a panel is open: the file picker's keys, or any key closes it.
    pub fn overlay_key(&mut self, key: OverlayKey) {
        if self.history_key(&key) || self.shortcuts_key(&key) {
            return;
        }
        let is_factory = matches!(self.chrome.overlay, Some(Overlay::FactoryReset));
        let Some(Overlay::Picker(picker)) = &mut self.chrome.overlay else {
            if is_factory && matches!(key, OverlayKey::Enter) {
                self.factory_reset();
                return;
            }
            if matches!(key, OverlayKey::Enter) {
                match self.chrome.overlay.take() {
                    Some(Overlay::ConfirmOverwrite { export, path }) => self.write_export(export, &path),
                    Some(Overlay::Confirm(pending)) => self.run_pending(pending),
                    _ => {}
                }
                return;
            }
            if matches!(key, OverlayKey::Escape) {
                self.chrome.overlay = None;
            }
            return;
        };
        let outcome = match key {
            OverlayKey::Escape => {
                self.chrome.overlay = None;
                return;
            }
            OverlayKey::Enter => picker.activate(),
            OverlayKey::Back => {
                picker.up();
                Outcome::Browsing
            }
            OverlayKey::Move(delta) => {
                picker.step(delta);
                Outcome::Browsing
            }
            OverlayKey::Edge(end) => {
                picker.home_end(end);
                Outcome::Browsing
            }
            OverlayKey::Letter(c) => {
                picker.jump(c);
                Outcome::Browsing
            }
        };
        self.picker_outcome(outcome);
    }

    /// A click while a panel is open.
    pub fn overlay_click(&mut self, width: i32, height: i32, scale: i32, x: i32, y: i32) {
        if matches!(self.chrome.overlay, Some(Overlay::History { .. })) {
            if !LogLayout::new(width, height, scale).contains(x, y) {
                self.chrome.overlay = None;
            }
            return;
        }
        let Some(Overlay::Picker(picker)) = &mut self.chrome.overlay else {
            self.chrome.overlay = None;
            return;
        };
        let layout = PickerLayout::new(width, height, scale);
        if !layout.contains(x, y) {
            self.chrome.overlay = None;
            return;
        }
        if let Some(i) = layout.hit(picker, x, y) {
            let outcome = picker.click(i);
            self.picker_outcome(outcome);
        }
    }

    fn picker_outcome(&mut self, outcome: Outcome) {
        let Outcome::Picked(path) = outcome else { return };
        let Some(Overlay::Picker(picker)) = self.chrome.overlay.take() else { return };
        self.chrome.last_dir = picker.dir.clone();
        self.picked(picker.purpose, path);
    }

    /// `path` is picked for `purpose`.
    fn picked(&mut self, purpose: Purpose, path: PathBuf) {
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

    /// Lines of Help > About.
    fn about(&self) -> Vec<String> {
        let shown = |p: Option<PathBuf>| p.map_or_else(|| "-".to_owned(), |p| p.display().to_string());
        vec![
            format!("orDrumbox {} - native port of ordrumbox-v2", update::BUILD),
            "MIT - https://www.ordrumbox.com".into(),
            String::new(),
            format!("library   {}", shown(self.saving.library_path.clone())),
            format!("user data {}{}", shown(self.user_folder()), self.user_folder_origin()),
            format!("exports   {}", shown(store::exports_dir())),
            format!("assets    {}", shown(self.assets_dir.clone())),
        ]
    }

    /// Where the user data folder comes from, after its path in Help > About.
    fn user_folder_origin(&self) -> &'static str {
        if store::env_library().is_some() {
            " (ORDRUMBOX_LIBRARY)"
        } else if self.user_folder().zip(store::default_dir()).is_some_and(|(d, default)| d != default) {
            " (chosen in the app)"
        } else {
            ""
        }
    }

    /// Lines of Help > Factory reset.
    fn factory_reset_lines(&self) -> Vec<String> {
        vec![
            "Restore the factory settings?".into(),
            String::new(),
            "The user files are moved to backups/ in the user data folder:".into(),
            "  library.json, kits.json, settings.json,".into(),
            "  synth_presets.json, samples/, songs/".into(),
            "File > Restore user data brings them back.".into(),
            String::new(),
            format!("folder: {}", self.user_folder().map_or_else(|| "-".to_owned(), |p| p.display().to_string())),
            String::new(),
            "ENTER: restore     ESC or click: cancel".into(),
        ]
    }

    // ─── Kit editor ──────────────────────────────────────────────────────────

    pub fn kit_editor_open(&self) -> bool {
        self.kit_editor.is_some()
    }

    /// The kit editor's selected sample and the kit's sample count, when open.
    pub fn kit_list(&self) -> Option<(usize, usize)> {
        let sample = self.kit_editor?;
        Some((sample, self.kits.kits.get(self.kit_index).map_or(0, |k| k.samples.len())))
    }

    /// Ctrl+K: the kit editor (the drumkit manager), open or closed.
    pub fn toggle_kit_editor(&mut self) {
        self.kit_editor = match self.kit_editor {
            Some(_) => None,
            None => Some(0),
        };
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.update_analysis();
    }

    /// The analysis of the sample shown, computed when it changes.
    fn update_analysis(&mut self) {
        let url = self.kit_editor.and_then(|s| self.kits.sample(self.kit_index, s)).map(|i| i.path.clone());
        match url {
            Some(url) if self.analysis.as_ref().is_some_and(|(u, _)| *u == url) => {}
            Some(url) => {
                self.analysis = self.kits.buffer(&url).map(|b| (url.clone(), analysis::analyze(b)));
            }
            None => self.analysis = None,
        }
    }

    /// The kits changed: a new bank for playback and exports, the root
    /// notes, the kits saved, the selection kept inside.
    fn kits_changed(&mut self, message: String) {
        self.saving.kits.clear();
        self.track_analysis.borrow_mut().clear();
        let (bank, errors) = self.kits.bank();
        self.roots = bank.samples.iter().filter_map(|s| Some((s.sample.key.clone(), s.sample.root_midi?))).collect();
        let bank = Arc::new(bank);
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_bank(bank.clone());
            controller.refresh(self.editor.library());
        }
        let kit = self.kits.kits.get(self.kit_index).map(|k| k.name.clone()).unwrap_or_default();
        self.render = (!bank.samples.is_empty()).then(|| RenderSetup {
            bank: bank.clone(),
            scales: self.scales.clone(),
            synths: self.synths.clone(),
            kit,
            sample_rate: offline::EXPORT_RATE,
            seed: 1,
        });
        self.message = match (self.kits.save(), errors.first()) {
            (Err(e), _) => e,
            (Ok(()), Some(first)) => format!("{message} ({} sample(s) not loaded, first: {first})", errors.len()),
            (Ok(()), None) => message,
        };
        self.kit_index = self.kit_index.min(self.kits.kits.len().saturating_sub(1));
        self.analysis = None;
        self.select_kit(self.kit_index);
    }

    /// ↑↓: the previous / next sample of the kit.
    pub fn kit_select(&mut self, delta: isize) {
        let Some(sample) = self.kit_editor else { return };
        let count = self.kits.kits.get(self.kit_index).map_or(0, |k| k.samples.len());
        if count > 0 {
            self.kit_editor = Some((sample as isize + delta).rem_euclid(count as isize) as usize);
            self.update_analysis();
        }
    }

    /// A click on the kit editor's list: that sample, heard.
    pub fn kit_click(&mut self, sample: usize) {
        if self.kit_editor.is_none() {
            return;
        }
        self.kit_editor = Some(sample);
        self.update_analysis();
        self.kit_audition();
    }

    /// Enter: the selected sample, as it is.
    pub fn kit_audition(&mut self) {
        let Some(sample) = self.kit_editor else { return };
        let Some(url) = self.kits.sample(self.kit_index, sample).map(|i| i.path.clone()) else { return };
        if let Some(controller) = self.playback.controller_mut()
            && let Some(index) = controller.bank().find_path(&url)
        {
            controller.audition_sample(index);
        }
    }

    /// Delete: the selected sample out of the kit.
    pub fn kit_remove(&mut self) {
        let Some(sample) = self.kit_editor else { return };
        let kit = self.kit_index;
        if self.kits.kits.get(kit).is_some_and(|k| sample < k.samples.len()) {
            self.ask(Pending::RemoveSample { kit, sample });
        }
    }

    fn kit_remove_now(&mut self, kit: usize, sample: usize) {
        if let Some(message) = self.kits.remove(kit, sample) {
            self.kits_changed(message);
        }
    }

    /// N: every sample of the kit to a 0 dB peak.
    pub fn kit_normalize(&mut self) {
        match self.kits.normalize(self.kit_index) {
            Ok(0) => self.message = "No samples to normalize".into(),
            Ok(n) => self.kits_changed(format!("Normalized {n} sample(s)")),
            Err(e) => self.message = e,
        }
    }

    /// E: the kit's mapping to a JSON file at `path`.
    fn kit_export(&mut self, path: &Path) {
        self.message = self.kits.export_mapping(self.kit_index, path).unwrap_or_else(|e| e);
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

    /// Keep the engine fed and the playing pass current; call every frame
    /// while playing. The next pass of a pattern is built with the new lines
    /// of its tracks in auto mode (the JS loop start); they replace the library's
    /// when that pass starts.
    pub fn update(&mut self) {
        // What the library's tags teach, learned again after an edit (the header and menus read it).
        learned_styles(&mut self.learned_styles, &self.styles, &self.editor);
        self.playback.update(&mut self.editor, self.pattern_index, |editor, p, pass| {
            auto_loop(editor, &mut self.learned_styles, &self.styles, &self.scales, &mut self.rng, p, pass)
        });
    }

    fn audition(&mut self, t: usize, note: Option<usize>) {
        if let Some(controller) = self.playback.controller_mut() {
            controller.preview(self.editor.library(), self.pattern_index, t, note);
        }
    }

    pub fn audition_row(&mut self, t: usize) {
        if self.track(t).is_some() {
            self.audition(t, None);
        }
    }

    // ─── Editing ─────────────────────────────────────────────────────────────

    /// After an edit: remember to save, show what happened.
    fn edited(&mut self) {
        self.saving.library.touch();
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
    }

    pub fn click(&mut self, hit: Hit) {
        match hit {
            Hit::Mute(t) => self.toggle_row(t, "mute"),
            Hit::Solo(t) => self.toggle_row(t, "solo"),
            Hit::Name(t) => {
                self.selection.put_cursor(Cursor {
                    track: t,
                    beat: self.selection.cursor.map_or(0, |c| c.beat),
                    step: 0,
                });
                self.selection.note_focus = Some(NoteFocus::Track(t));
                self.audition(t, None);
            }
            Hit::Cell { track, beat, step } => {
                self.selection.cursor = Some(Cursor { track, beat, step });
                self.toggle_note(track, beat, step);
            }
            Hit::AddTrack => self.add_track_menu_at_button(),
        }
    }

    /// A grid click / Enter: a step is on or off. An empty step gets a note
    /// (selected); a step with notes loses them all, in one undo step and
    /// without asking. Shift+click selects a note instead ([`Self::select_step_note`]).
    fn toggle_note(&mut self, t: usize, beat: i32, step: i32) {
        let p = self.pattern_index;
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.step == step);
        match at_step {
            Some(_) => {
                self.selection.cursor = Some(Cursor { track: t, beat, step });
                self.delete_step_now();
                self.selection.note_focus = Some(NoteFocus::Step { track: t, beat, step });
            }
            None => {
                self.selection.selected = self.editor.add_note(p, t, beat, step, 0.0).map(|n| (t, n));
                self.edited();
                self.audition(t, self.selection.selected.map(|(_, n)| n));
            }
        }
    }

    /// Shift+click on a grid cell: the cursor goes there and the step's note
    /// is selected for the note editor, and heard; nothing is added or deleted.
    pub fn select_step_note(&mut self, t: usize, beat: i32, step: i32) {
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.step == step);
        self.selection.cursor = Some(Cursor { track: t, beat, step });
        self.selection.note_focus = Some(NoteFocus::Step { track: t, beat, step });
        self.selection.selected = at_step.map(|n| (t, n));
        if let Some(n) = at_step {
            self.audition(t, Some(n));
        }
    }

    pub fn enter(&mut self) {
        let Some(c) = self
            .selection
            .cursor
            .or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| Cursor { track: 0, beat: 0, step: 0 }))
        else {
            return;
        };
        self.selection.cursor = Some(c);
        self.toggle_note(c.track, c.beat, c.step);
    }

    pub fn move_cursor(&mut self, direction: Move) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let tracks = pattern.tracks.len();
        if tracks == 0 {
            return;
        }
        let beats = pattern.beat_count.max(1);
        let Some(mut c) = self.selection.cursor else {
            self.selection.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
            return;
        };
        let spb = pattern.tracks[c.track.min(tracks - 1)].steps_per_beat.max(1);
        match direction {
            Move::Right => {
                c.step += 1;
                if c.step >= spb {
                    c.step = 0;
                    c.beat = (c.beat + 1) % beats;
                }
            }
            Move::Left => {
                c.step -= 1;
                if c.step < 0 {
                    c.step = spb - 1;
                    c.beat = (c.beat - 1).rem_euclid(beats);
                }
            }
            Move::Up => c.track = c.track.saturating_sub(1),
            Move::Down => c.track = (c.track + 1).min(tracks - 1),
        }
        let spb = pattern.tracks[c.track].steps_per_beat.max(1);
        c.step = c.step.min(spb - 1);
        self.selection.cursor = Some(c);
        self.selection.note_focus = Some(NoteFocus::Step { track: c.track, beat: c.beat, step: c.step });
        let note = pattern.tracks[c.track].notes.iter().position(|n| n.beat == c.beat && n.step == c.step);
        self.selection.selected = note.map(|n| (c.track, n));
        if let Some(n) = note {
            self.audition(c.track, Some(n));
        }
    }

    /// Shift+arrows on the selected note: ↑↓ velocity, ←→ pitch.
    pub fn nudge(&mut self, direction: Move) {
        let Some((t, n)) = self.selection.selected else {
            self.message = "select a note first".into();
            return;
        };
        let nudge = match direction {
            Move::Up => Nudge::VelocityUp,
            Move::Down => Nudge::VelocityDown,
            Move::Right => Nudge::PitchUp,
            Move::Left => Nudge::PitchDown,
        };
        if self.editor.nudge_note(self.pattern_index, t, n, nudge) {
            self.edited();
            if let Some(note) = self.track(t).and_then(|track| track.notes.get(n)) {
                self.message = format!("velocity {}  pitch {}", note.velocity, note.pitch);
            }
            self.audition(t, Some(n));
        }
    }

    /// Pattern > Compact tracks (the tools panel's button): every track
    /// shortened to its shortest loop, when its notes repeat one.
    pub fn compact_tracks(&mut self) {
        let can_compact = |track: &Track| track.clone().compact_loop();
        if self.pattern().is_some_and(|p| p.tracks.iter().any(can_compact)) {
            self.undoable(Self::compact_tracks_now);
        } else {
            self.compact_tracks_now();
        }
    }

    fn compact_tracks_now(&mut self) {
        let count = self.editor.compact_pattern(self.pattern_index);
        if count == 0 {
            self.message = "no track repeats a shorter loop".into();
            return;
        }
        self.selection.selected = None;
        self.clamp_selection();
        self.edited();
        self.message = format!("compacted {count} track{}", if count == 1 { "" } else { "s" });
    }

    /// Clear the cursor step (Ctrl+Z undoes it).
    pub fn delete_step(&mut self) {
        if self.selection.cursor.is_some() {
            self.undoable(Self::delete_step_now);
        }
    }

    /// L: accent the notes of the cursor step, or take their accent off.
    pub fn toggle_accent(&mut self) {
        let Some(c) = self.selection.cursor else { return };
        let name = self.track(c.track).map(|track| track.name.clone()).unwrap_or_default();
        let at = beat_label(c.beat, c.step);
        match self.editor.toggle_accent(self.pattern_index, c.track, c.beat, c.step) {
            Some(accent) => {
                self.edited();
                self.message = format!("{} - {name} @ {at}", if accent { "Accent" } else { "No accent" });
            }
            None => self.message = format!("no note to accent - {name} @ {at}"),
        }
    }

    /// How many notes step `c` holds.
    fn step_note_count(&self, c: Cursor) -> usize {
        self.track(c.track)
            .map_or(0, |track| track.notes.iter().filter(|n| n.beat == c.beat && n.step == c.step).count())
    }

    fn delete_step_now(&mut self) {
        let Some(c) = self.selection.cursor else { return };
        let count = self.step_note_count(c);
        let name = self.track(c.track).map(|track| track.name.clone()).unwrap_or_default();
        if self.editor.delete_step(self.pattern_index, c.track, c.beat, c.step) {
            self.edited();
            self.message = format!("Deleted {} - {name} @ {}", notes_label(count), beat_label(c.beat, c.step));
        }
        self.selection.selected = None;
    }

    pub fn deselect(&mut self) {
        self.selection.clear();
    }

    /// The track the cursor is on, else the selected note's.
    fn focus_track(&self) -> Option<usize> {
        self.selection.focus_track().filter(|&t| self.track(t).is_some())
    }

    pub fn toggle_row(&mut self, t: usize, key: &str) {
        if self.track(t).is_some() && self.editor.toggle_track(self.pattern_index, t, key) {
            self.edited();
        }
    }

    /// The tempo a plugin plays at: the host's, inside the tempo bounds
    /// (`None` before the host's transport was seen).
    pub(super) fn host_bpm(&self) -> Option<f64> {
        let host = self.playback.host()?;
        Some(ordrumbox_core::song::clamp_bpm(host.bpm.round()))
    }

    pub fn step_tempo(&mut self, delta: f64) {
        let Some(bpm) = self.pattern().map(|p| p.bpm) else { return };
        self.set_tempo(bpm + delta);
    }

    /// The pattern at `bpm` (whole, inside the tempo bounds), a run of
    /// changes one undo step.
    pub(super) fn set_tempo(&mut self, bpm: f64) {
        let bpm = ordrumbox_core::song::clamp_bpm(bpm.round());
        if self.editor.set_pattern_bpm(self.pattern_index, bpm, true) {
            self.edited();
        }
    }

    pub fn change_length(&mut self, delta: i32) {
        let Some(beats) = self.pattern().map(|p| p.beat_count) else { return };
        self.set_length(beats + delta);
    }

    /// The pattern `beats` long (1–16).
    pub(super) fn set_length(&mut self, beats: i32) {
        let beats = beats.clamp(1, ordrumbox_logic::editor::MAX_BEATS);
        if self.editor.set_pattern_beat_count(self.pattern_index, f64::from(beats)) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    fn clamp_cursor_to_grid(&mut self) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let beats = pattern.beat_count.max(1);
        if let Some(c) = &mut self.selection.cursor {
            c.beat = c.beat.min(beats - 1);
            let spb = pattern.tracks.get(c.track).map_or(1, |t| t.steps_per_beat.max(1));
            c.step = c.step.min(spb - 1);
        }
        self.clamp_selection();
    }

    pub fn next_grid(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.incr_steps_per_beat(self.pattern_index, t) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    pub fn shorten_loop(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.shorten_loop(self.pattern_index, t) {
            self.edited();
        }
    }

    /// Ctrl+T: a new track named [`DEFAULT_TRACK_NAME`] (the menus and the
    /// button let the user choose the instrument: `session_grid.rs`).
    pub fn add_track(&mut self) {
        self.add_track_named(DEFAULT_TRACK_NAME);
    }

    /// A new track named `name`, the cursor on it.
    pub fn add_track_named(&mut self, name: &str) {
        if self.pattern().is_none() {
            return;
        }
        if !self.editor.has_track_room(self.pattern_index) {
            self.message = format!("a pattern holds at most {MAX_TRACKS} tracks");
            return;
        }
        if let Some(t) = self.editor.add_track(self.pattern_index, name, 4) {
            self.selection.put_cursor(Cursor { track: t, beat: 0, step: 0 });
            self.edited();
        }
    }

    /// Remove the focused track, never the last one.
    pub fn remove_track(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.pattern().is_some_and(|p| p.tracks.len() <= 1) {
            self.message = "Cannot delete the last track".into();
        } else {
            self.undoable(|s| s.remove_track_now(t));
        }
    }

    fn remove_track_now(&mut self, t: usize) {
        if self.editor.remove_track(self.pattern_index, t) {
            self.selection.selected = None;
            self.edited();
            self.clamp_selection();
            self.message = "Track deleted".into();
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

    fn clear_track_now(&mut self, t: usize) {
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return };
        if self.editor.clean_track(self.pattern_index, t) {
            self.selection.selected = None;
            self.edited();
            self.message = format!("Cleared notes on \"{name}\"");
        }
    }

    // ─── Generation ──────────────────────────────────────────────────────────

    /// The styles the generation follows now: `styles.json` with what the
    /// library's tags teach.
    fn generation_styles(&mut self) -> Styles {
        learned_styles(&mut self.learned_styles, &self.styles, &self.editor).clone()
    }

    /// The header's style list: style `name` (empty: a drawn one) for the
    /// patterns to come, and the current pattern made again in it.
    pub fn choose_style(&mut self, name: &str) {
        self.gen_style = Some(name.to_owned()).filter(|n| !n.is_empty());
        let style = self.gen_style.clone().or_else(|| {
            let styles = self.generation_styles();
            let names = styles.names();
            let i = (self.rng.next_f64() * names.len() as f64) as usize;
            names.get(i.min(names.len().saturating_sub(1))).map(|n| (*n).to_owned())
        });
        self.regenerate_pattern_in(style.as_deref());
    }

    /// B, ⟳: the pattern made again in its style (or the chosen one when it
    /// has none); no pattern: a new one.
    pub fn regenerate_pattern(&mut self) {
        let has_style =
            self.pattern().is_some_and(|p| p.extra.get(generate::GENRE).is_some_and(|g| g.as_str().is_some()));
        let style = if has_style { None } else { self.gen_style.clone() };
        self.regenerate_pattern_in(style.as_deref());
    }

    fn regenerate_pattern_in(&mut self, style: Option<&str>) {
        if self.pattern().is_none() {
            return self.generate_pattern();
        }
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let p = self.pattern_index;
        if self.editor.regenerate_pattern(p, &self.scales, &styles, style, &mut || rng.next_f64()) {
            self.clamp_selection();
            self.edited();
        }
        self.message = format!("generated in {}", self.pattern_style_label(&styles));
    }

    /// The pattern's style as the messages name it.
    fn pattern_style_label(&self, styles: &Styles) -> String {
        let genre = self.pattern().and_then(|p| p.extra.get(generate::GENRE)?.as_str()).unwrap_or_default();
        styles.get(genre).map_or_else(|| genre.to_owned(), style_label)
    }

    /// A header macro (`composeEnergy`…) of the pattern set to `value`.
    pub fn set_macro(&mut self, key: &'static str, value: f64) {
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let sources = (&self.scales, &styles);
        if self.editor.set_pattern_macro(self.pattern_index, key, value, sources, &mut || rng.next_f64()) {
            self.edited();
        }
    }

    /// Whether the pattern's unlocked tracks play in auto mode (AUTO lit).
    pub fn pattern_auto(&self) -> bool {
        self.pattern().is_some_and(|p| p.tracks.iter().any(|t| !t.compose_lock && generate::is_auto_on(t)))
    }

    /// AUTO: every unlocked track in auto mode, or all out of it.
    pub fn toggle_pattern_auto(&mut self) {
        let Some(on) = self.editor.toggle_pattern_auto(self.pattern_index) else { return };
        self.edited();
        self.message = if on { "auto: the pattern moves as it plays".into() } else { "auto off".into() };
    }

    /// The focused track locked or unlocked (the generation leaves a locked one as it is).
    pub fn toggle_lock(&mut self) {
        let Some(t) = self.editor_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "composeLock") {
            self.edited();
        }
    }

    /// Shift+B: a new pattern, generated (style, tracks and lines).
    pub fn generate_pattern(&mut self) {
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let style = self.gen_style.as_deref();
        self.pattern_index = self.editor.generate_pattern(&self.scales, &styles, style, &mut || rng.next_f64());
        self.deselect();
        self.edited();
        self.chrome.current_page = 0;
        if let Some(pattern) = self.pattern() {
            let genre = pattern.extra.get(generate::GENRE).and_then(|g| g.as_str()).unwrap_or_default();
            let label = styles.get(genre).map_or_else(|| genre.to_owned(), style_label);
            self.message = format!("generated {} ({label})", pattern.name.as_deref().unwrap_or_default());
        }
    }

    /// D, S, A: ↻ Drum / Bass / Chords — new lines in auto mode, or auto mode off.
    pub fn toggle_generation(&mut self, part: Option<MelodicKind>) {
        let p = self.pattern_index;
        let styles = self.generation_styles();
        let rng = &mut self.rng;
        let rnd = &mut || rng.next_f64();
        let on = match part {
            None => self.editor.toggle_drums(p, &self.scales, &styles, self.gen_style.as_deref(), rnd),
            Some(kind) => self.editor.toggle_melodic(p, kind, &self.scales, &styles, rnd),
        };
        let Some(on) = on else { return };
        self.clamp_selection();
        self.edited();
        let name = match part {
            None => "drums",
            Some(MelodicKind::Bass) => "bass",
            Some(MelodicKind::Chords) => "chords",
        };
        self.message = if on {
            let genre = self.pattern().and_then(|p| p.extra.get(generate::GENRE)?.as_str()).unwrap_or_default();
            let label = styles.get(genre).map_or_else(|| genre.to_owned(), style_label);
            format!("{name}: generated, new lines every loop ({label})")
        } else {
            format!("{name}: auto off")
        };
    }

    /// ↻ in the Gen tab: a new line on the edited track, in the pattern's style.
    pub fn regenerate_track(&mut self, t: usize) {
        let styles = self.generation_styles();
        let name = self.track(t).map(|tr| tr.name.clone()).unwrap_or_default();
        if self.track(t).is_some_and(|tr| tr.compose_lock) {
            self.message = format!("{name} is locked: unlock it for a new line");
            return;
        }
        let rng = &mut self.rng;
        if self.editor.regenerate_track(self.pattern_index, t, &self.scales, &styles, &mut || rng.next_f64()) {
            self.edited();
            self.message = format!("new line on {name}");
        }
    }

    /// Z: auto mode of the focused track (a new line every loop).
    pub fn toggle_auto(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "autoMode") {
            self.edited();
        }
    }

    /// X: random notes on the focused track.
    pub fn randomize_track(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        self.undoable(|s| s.randomize_track_now(t));
    }

    fn randomize_track_now(&mut self, t: usize) {
        let rng = &mut self.rng;
        if self.editor.randomize_track(self.pattern_index, t, &mut || rng.next_f64()) {
            self.selection.selected = None;
            self.edited();
            let name = self.track(t).map(|track| track.name.clone()).unwrap_or_default();
            self.message = format!("Randomized \"{name}\"");
        }
    }

    pub fn new_pattern(&mut self) {
        self.pattern_index = self.editor.add_pattern(None);
        self.deselect();
        self.edited();
        self.chrome.current_page = 0;
    }

    pub fn duplicate_pattern(&mut self) {
        if let Some(index) = self.editor.duplicate_pattern(self.pattern_index) {
            self.pattern_index = index;
            self.deselect();
            self.edited();
            self.chrome.current_page = 0;
        }
    }

    /// Ctrl+C: the notes of the cursor step (with Shift, or no cursor: the track).
    pub fn copy(&mut self, track: bool) {
        let Some(t) = self.focus_track() else { return };
        let Some(source) = self.track(t).cloned() else { return };
        let name = source.name.clone();
        if track || self.selection.cursor.is_none() {
            self.message = format!("copied track \"{name}\" ({} notes)", source.notes.len());
            self.selection.clipboard = Some(Clipboard::Track(Box::new(source)));
        } else if let Some(c) = self.selection.cursor {
            let notes: Vec<Note> =
                source.notes.iter().filter(|n| n.beat == c.beat && n.step == c.step).cloned().collect();
            self.message = format!("copied {} note(s) — {name} @ {}.{}", notes.len(), c.beat + 1, c.step + 1);
            self.selection.clipboard = Some(Clipboard::Step(notes));
        }
    }

    /// Ctrl+V: the copied step at the cursor, or the copied track after the
    /// focused one (with Shift, only a track).
    pub fn paste(&mut self, track_only: bool) {
        let p = self.pattern_index;
        match self.selection.clipboard.clone() {
            None => self.message = "clipboard is empty".into(),
            Some(Clipboard::Track(_)) if !self.editor.has_track_room(p) => {
                self.message = format!("a pattern holds at most {MAX_TRACKS} tracks");
            }
            Some(Clipboard::Track(track)) => {
                let after = self.focus_track().map_or_else(|| self.pattern().map_or(0, |p| p.tracks.len()), |t| t + 1);
                if let Some(t) = self.editor.paste_track(p, after, &track) {
                    self.selection.put_cursor(Cursor { track: t, beat: 0, step: 0 });
                    self.edited();
                }
            }
            Some(Clipboard::Step(_)) if track_only => self.message = "no track copied".into(),
            Some(Clipboard::Step(notes)) => {
                let c = self.selection.cursor.unwrap_or(Cursor { track: 0, beat: 0, step: 0 });
                if self.editor.paste_step_notes(p, c.track, c.beat, c.step, &notes) {
                    self.selection.put_cursor(c);
                    self.edited();
                }
            }
        }
    }

    // ─── Saving ──────────────────────────────────────────────────────────────

    pub fn save(&mut self) {
        let Some(path) = self.saving.library_path.clone() else {
            self.message = if let Some(refusal) = Command::Save.hosted_refusal().filter(|_| self.hosted) {
                refusal.into()
            } else {
                "not saved: no library file (see the messages below)".into()
            };
            return;
        };
        match store::save(&path, self.editor.library()) {
            Ok(()) => {
                self.saving.library_saved(self.editor.revision());
                self.message = format!("saved {}", path.display());
            }
            Err(e) => self.message = format!("not saved: {}: {e}", path.display()),
        }
    }

    pub fn save_if_changed(&mut self) {
        if self.saving.library_unsaved(self.editor.revision()) {
            self.save();
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
        let due = [self.flush_kits(), self.flush_master(), self.flush_synths(), self.autosave_library()];
        due.into_iter().flatten().min()
    }

    fn autosave_library(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.library_ready(self.editor.revision()) {
            return next;
        }
        self.save();
        None
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    /// The window's size and scale (where the lists opened from the menus go).
    pub fn set_size(&mut self, size: (i32, i32, i32)) {
        self.chrome.panel_size = size;
    }

    pub fn draw(&mut self, canvas: &mut Canvas, scale: i32) {
        // The scheme is per thread: a plugin draws on another than it loads on.
        theme::set_scheme(self.color_scheme);
        self.note_actions();
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
        let beat_count = self.pattern().map(|p| p.beat_count.max(1)).unwrap_or(1);
        let page_count = (beat_count as usize).div_ceil(view::BEATS_PER_PAGE as usize).max(1);
        let cursor_page = self.selection.cursor.map(|c| (c.beat.max(0) as usize) / view::BEATS_PER_PAGE as usize);
        // The pass playing is another pattern's while the one shown waits.
        let waiting = self.waiting_blink();
        let playhead_page = if self.playing() && waiting.is_none() {
            position.map(|p| {
                let frac = p.frame as f64 / p.frames.max(1) as f64;
                let global_beat = (frac * beat_count as f64).floor() as usize;
                global_beat / view::BEATS_PER_PAGE as usize
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
            sounds: pattern.map_or_else(Vec::new, |p| (0..p.tracks.len()).map(|t| self.track_sound_label(t)).collect()),
            arrangement: self.song_view_open().then(|| self.song_view_data(&rows, &song_layout)),
            piano_roll: roll,
            output: self.output_view(),
            synth: self.synth_open(),
            kit_view: self.kit_view(layout.details_top),
            right: layout.right,
            current_page: self.chrome.current_page,
            levels,
            show_vus: self.chrome.show_vus,
            tempo_label: if self.hosted { "HOST" } else { "BPM" },
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
                    let start_beat = self.chrome.current_page as f64 * view::BEATS_PER_PAGE as f64;
                    let page_beats = view::BEATS_PER_PAGE as f64;
                    ((global_beat - start_beat) / page_beats).clamp(0.0, 1.0)
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
        self.draw_tooltip(canvas, scale, layout.right);
        menu::draw(canvas, &self.menus(), self.chrome.menu, scale);
        if let Some(dropdown) = &self.chrome.widgets.dropdown {
            dropdown.draw(canvas, scale);
        }
        if let Some(entry) = &self.chrome.widgets.entry {
            entry.draw(canvas, scale);
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

    /// What the kit editor shows; its sample's details start at `details_top`.
    fn kit_view(&self, details_top: i32) -> Option<KitView<'_>> {
        let sample = self.kit_editor?;
        let kit = self.kits.kits.get(self.kit_index)?;
        let samples = kit.samples.iter().map(|i| (i.display_name.as_str(), i.key.as_str())).collect();
        let selected = kit.samples.get(sample);
        let analysis =
            self.analysis.as_ref().filter(|(url, _)| selected.is_some_and(|i| i.path == *url)).map(|(_, a)| a);
        let info = analysis.map(|a| {
            let db = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "-".into() };
            let note = a.note.map_or_else(|| "-".to_owned(), |n| n.to_string());
            format!("{note}  {:.0} ms  {} dB peak  {} dB RMS", a.duration_sec * 1000.0, db(a.peak_db), db(a.rms_db))
        });
        let detected = selected.map(|i| {
            let name = if i.display_name.is_empty() { &i.path } else { &i.display_name };
            ordrumbox_core::instruments::catalog()
                .from_file_name(name)
                .map_or_else(|| "No instrument detected".to_owned(), |d| format!("Detected: {}", d.id))
        });
        let decay_marker = match (selected, analysis) {
            (Some(i), Some(a)) if a.duration_sec > 0.0 => Some((i.decay_ms() / 1000.0 / a.duration_sec).min(1.0)),
            _ => None,
        };
        // Which tracks of the pattern play each sample.
        let paths: Vec<(String, String)> = (0..self.pattern().map_or(0, |p| p.tracks.len()))
            .filter_map(|t| Some((self.track_sample_path(t)?, self.track(t)?.name.clone())))
            .collect();
        let users = kit
            .samples
            .iter()
            .map(|s| paths.iter().filter(|(path, _)| *path == s.path).map(|(_, name)| name.clone()).collect())
            .collect();
        Some(KitView {
            kit: &kit.name,
            samples,
            users,
            selected: sample,
            details_top,
            info,
            detected,
            envelope: analysis.map_or(&[][..], |a| &a.envelope),
            decay_marker,
        })
    }
}
