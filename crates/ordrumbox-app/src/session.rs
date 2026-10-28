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

use ordrumbox_core::drum_taxonomy::{TrackType, detect_track_type};
use ordrumbox_core::{Library, Note, Pattern, Rng, Scales, Track};
use ordrumbox_engine::analysis::{self, Analysis};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{Controller, Mode, SynthPresets, wav};
use ordrumbox_logic::generate::{self, AutoGen, MelodicKind};
use ordrumbox_logic::midi;
use ordrumbox_logic::{Editor, Nudge};

use crate::assets;
use crate::audio;
use crate::canvas::Canvas;
use crate::editors::{EditorState, ScaleList};
use crate::kits::KitSet;
use crate::menu::{self, Command, MenuState};
use crate::panel::{Dropdown, Entry, Id};
use crate::picker::{self, Outcome, Picker, PickerLayout, Purpose};
use crate::store;
use crate::view::{self, Cursor, Hit, KitView, View};

#[path = "session_output.rs"]
mod session_output;
#[path = "session_panel.rs"]
mod session_panel;
#[path = "session_roll.rs"]
mod session_roll;
#[cfg(test)]
#[path = "session_scenarios.rs"]
mod session_scenarios;
#[path = "session_song.rs"]
mod session_song;
#[path = "session_synth.rs"]
mod session_synth;
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

/// Save this long after the last edit.
const SAVE_DELAY: Duration = Duration::from_secs(1);

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

#[derive(Debug, Clone)]
enum Clipboard {
    Step(Vec<Note>),
    Track(Box<Track>),
}

pub struct Session {
    editor: Editor,
    /// What the generators read (`scales.json`).
    scales: Scales,
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
    /// The measure the song plays from (`songCursorMeasure`).
    song_cursor: u32,
    /// First measure and first row of the arrangement shown.
    song_scroll: (u32, usize),
    /// The clip under the mouse (its tooltip).
    hovered_clip: Option<usize>,
    /// The piano roll's page, highest row shown (`None`: centered on the
    /// track's pitch), cursor (step, row), note pressed, the edited value
    /// shown over the note, next progression, note under the mouse.
    roll_page: i32,
    roll_top: Option<i32>,
    roll_cursor: Option<(i64, i32)>,
    roll_drag: Option<session_roll::RollDrag>,
    roll_gauge: Option<String>,
    roll_sequence: i64,
    roll_hover: Option<usize>,
    /// Window size and scale of the last mouse or key event on the widgets.
    panel_size: (i32, i32, i32),
    /// What offline renders (WAV exports) use; `None` without samples.
    render: Option<RenderSetup>,
    /// `None` when no audio device could be opened.
    controller: Option<Controller>,
    _output: Option<audio::Output>,
    /// Startup problems, shown until quit.
    status: Vec<String>,
    /// Outcome of the last action.
    message: String,
    cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    selected: Option<(usize, usize)>,
    clipboard: Option<Clipboard>,
    /// Where the library is saved; `None`: not saved.
    library_path: Option<PathBuf>,
    saved_revision: u64,
    /// Editor revision the playing pass was built from.
    played_revision: u64,
    changed_at: Option<Instant>,
    /// The open menu, if any.
    menu: Option<menu::Open>,
    /// A panel over everything: the file picker, the key list, about.
    overlay: Option<Overlay>,
    /// Folder the file picker last picked in.
    last_dir: Option<PathBuf>,
    /// The assets folder in use.
    assets_dir: Option<PathBuf>,
    /// File > Quit was chosen.
    quit: bool,
    /// The scale names, in file order (arpeggios).
    scale_list: ScaleList,
    /// Tabs and selection of the track and note editors.
    editors: EditorState,
    /// The track editor (and its note editor) shown on the right.
    show_track_editor: bool,
    /// Smoothed per-track levels for the VU meter stripe.
    track_levels: Vec<f32>,
    /// The widget with the keyboard focus.
    focus: Option<Id>,
    /// A knob or slider being dragged.
    drag: Option<Drag>,
    dropdown: Option<Dropdown>,
    entry: Option<Entry>,
    /// Last click on a widget, for double-clicks.
    last_click: Option<(Id, Instant)>,
    /// What the note editor follows when no note is selected.
    note_focus: Option<NoteFocus>,
    /// The default note the note editor shows for a track without notes
    /// (edits stay in it, as in the JS): its track and the note.
    detached: Option<(usize, Note)>,
    /// Kit knob edits not yet applied to the bank and saved.
    kits_dirty: Option<Instant>,
    /// The master bus settings (the output panel's; the grid's master
    /// volume is their volume), and when they changed, not saved yet.
    master: ordrumbox_engine::MasterSettings,
    master_dirty: Option<Instant>,
    /// The output panel's tab: vol, Comp, Flt.
    output_tab: u8,
    /// The synth editor: the preset edited, the tab, presets not saved yet.
    synth: Option<session_synth::SynthEdit>,
    synth_tab: u8,
    synths_dirty: Option<Instant>,
    /// When the session started (the synth editor's LFO clock).
    started: Instant,
    /// The spectrum's analyser and its last bins.
    analyser: std::cell::RefCell<crate::spectrum::Analyser>,
    spectrum: std::cell::RefCell<Vec<u8>>,
    /// The grid step under the mouse: track, beat, step (its note's tooltip).
    hover: Option<(usize, i32, i32)>,
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

/// What the note editor shows when no note is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NoteFocus {
    /// A track was picked: its first note.
    Track(usize),
    /// The grid cursor moved to a step: its note, or none.
    Step { track: usize, beat: i32, step: i32 },
}

/// What is shown over the window.
#[derive(Debug)]
enum Overlay {
    Picker(Picker),
    Shortcuts,
    About,
    FactoryReset,
}

/// Every key, for Help > Keyboard shortcuts.
const SHORTCUTS: [&str; 50] = [
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1 F2 F3 F4 F5 F6      this list, track editor, song, piano roll, grid, synth (Ctrl+K: kit editor)",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          add a note, select it, delete the selected one",
    "  Shift+Arrows           velocity (up/down), pitch (left/right) of the selected note",
    "  Delete                 clear the cursor step        Esc  deselect",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10",
    "PLAY",
    "  Space / Shift+Space    play or stop the pattern / the song",
    "  [ ] / Shift+[ ]        previous / next pattern / song (also PageUp PageDown)",
    "  - = (Shift: x10)       tempo",
    "  , .                    previous / next kit",
    "FILE AND EDIT",
    "  Ctrl+S, Ctrl+O, Ctrl+Q save, import a MIDI file, quit",
    "  Ctrl+E, Ctrl+M         export WAV (Shift: the song), export MIDI",
    "  Ctrl+Z, Ctrl+Y         undo, redo (also Ctrl+Shift+Z)",
    "  Ctrl+C, Ctrl+V         copy, paste the step (Shift: the track)",
    "  Ctrl+N, Ctrl+D         new pattern, duplicate pattern",
    "  Ctrl+T, Ctrl+Backspace add, remove a track",
    "  Ctrl+G, Ctrl+L         next grid, loop point one step back",
    "  Ctrl+- Ctrl+=          pattern length",
    "GENERATE",
    "  B                      generate a pattern",
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
    "  vol, Comp, Flt         the master volume and spectrum, the compressor, the filters (saved)",
    "SONG PANEL (F3)",
    "  click / right-click    the ruler: aim the cursor; a pattern, row, clip, the grid: menu",
    "  Left Right, Up Down    the cursor a measure back / on, previous / next pattern",
    "  Space, wheel           play the song from the cursor; scroll (Shift: the rows)",
    "KIT EDITOR (Ctrl+K)",
    "  Up Down                sample      Enter  hear      Delete  remove",
    "  N, R, E                normalize, assign the sounds again, export the mapping",
    "  drop files             a .wav joins the kit (Shift: replaces the sample), a folder",
    "                         becomes a kit, a .json mapping loads, a .mid imports",
];

impl Session {
    pub fn load() -> Self {
        let Some(dir) = assets::find() else {
            let searched: Vec<String> = assets::candidates().iter().map(|d| d.display().to_string()).collect();
            return Self::new(
                Library::default(),
                KitSet::empty(),
                vec![
                    format!(
                        "assets folder not found: copy ordrumbox-v2/assets next to the program, or set {}",
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
        let mut status = Vec::new();
        if let Some(first) = assets.sample_errors.first() {
            status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
        }

        // The user's library when there is one, else the assets' song.json.
        let mut library = assets.library;
        let mut message = "demo library (assets/data/song.json)".to_owned();
        let library_path = match store::path() {
            None => {
                status.push("no user data folder: edits are not saved".into());
                None
            }
            Some(path) => match store::load(&path) {
                Ok(Some(saved)) => {
                    library = saved;
                    message = format!("library {}", path.display());
                    Some(path)
                }
                Ok(None) => Some(path),
                Err(e) => {
                    status.push(format!("library not loaded, not saved over: {e}"));
                    None
                }
            },
        };
        if library_path.is_none() && !assets.report.issues.is_empty() {
            status.push(format!("song.json: {} value(s) repaired", assets.report.issues.len()));
        }

        if let Some(e) = &assets.kits_error {
            status.push(format!("kits.json not loaded, kit edits not saved: {e}"));
        }
        let kits_file = if assets.kits_error.is_none() { store::kits_path() } else { None };
        let kits =
            KitSet::new(assets.kits, &assets.bank, Some(assets.sources.clone()), kits_file, store::samples_dir());
        // The synth presets edited, over the assets' ones.
        Self::load_user_synths(&mut assets.synths, &mut status);
        let mut session = Self::new(library, kits, status);
        session.assets_dir = Some(dir.clone());
        session.scales = assets.scales.clone();
        session.scale_list = assets.scale_list.clone();
        session.synths = assets.synths.clone();
        session.roots = assets
            .bank
            .samples
            .iter()
            .filter_map(|s| Some((s.instrument.key.clone(), s.instrument.root_midi?)))
            .collect();
        session.library_path = library_path;
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
        match audio::start(bank, assets.scales, assets.synths) {
            Ok((output, controller)) => {
                session.controller = Some(controller);
                session._output = Some(output);
            }
            Err(e) => session.status.push(format!("no sound: {e}")),
        }
        session.load_master();
        session
    }

    fn new(library: Library, kits: KitSet, status: Vec<String>) -> Self {
        Self {
            editor: Editor::new(library),
            scales: Scales::default(),
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
            song_scroll: (0, 0),
            hovered_clip: None,
            roll_page: 0,
            roll_top: None,
            roll_cursor: None,
            roll_drag: None,
            roll_gauge: None,
            roll_sequence: 0,
            roll_hover: None,
            panel_size: (0, 0, 1),
            render: None,
            controller: None,
            _output: None,
            status,
            message: String::new(),
            cursor: None,
            selected: None,
            clipboard: None,
            library_path: None,
            saved_revision: 0,
            played_revision: 0,
            changed_at: None,
            menu: None,
            overlay: None,
            last_dir: None,
            assets_dir: None,
            quit: false,
            scale_list: ScaleList::default(),
            editors: EditorState::default(),
            show_track_editor: true,
            track_levels: Vec::new(),
            focus: None,
            drag: None,
            dropdown: None,
            entry: None,
            last_click: None,
            note_focus: None,
            detached: None,
            kits_dirty: None,
            master: ordrumbox_engine::MasterSettings::default(),
            master_dirty: None,
            output_tab: 0,
            synth: None,
            synth_tab: 0,
            synths_dirty: None,
            started: Instant::now(),
            analyser: std::cell::RefCell::default(),
            spectrum: std::cell::RefCell::default(),
            hover: None,
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

    /// The window split for `size` (a real window size): the main editor
    /// region (top-left) and the output strip (bottom).
    pub(super) fn body(&self, size: (i32, i32, i32)) -> view::Body {
        let (width, height, scale) = size;
        view::Body::new(width, height, scale, self.track_editor_width(width, scale), self.status.len())
    }

    pub fn playing(&self) -> bool {
        self.controller.as_ref().is_some_and(Controller::is_playing)
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
        if self.editor.library().songs.is_empty() {
            self.message = "no song in the library".into();
            return;
        }
        let start = self.song_start();
        self.toggle(Mode::Song(self.editor.selected_song()), start);
    }

    /// Stop, or play `mode` from pass `first`.
    fn toggle(&mut self, mode: Mode, first: u64) {
        let Some(controller) = &mut self.controller else { return };
        if controller.is_playing() {
            controller.stop();
        } else {
            if let Mode::Pattern(p) = mode {
                let rng = &mut self.rng;
                self.editor.auto_loop(p, 0, &self.scales, &mut || rng.next_f64());
            }
            controller.play_from(self.editor.library(), mode, first);
            self.played_revision = self.editor.revision();
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
    /// file in the exports folder.
    pub fn export_wav(&mut self, song: bool) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        let Some(setup) = &self.render else {
            self.message = "nothing to render with (no samples loaded)".into();
            return;
        };
        let library = self.editor.library();
        let started = Instant::now();
        let (rendered, name) = if song {
            let index = self.editor.selected_song();
            let Some(s) = library.songs.get(index) else {
                self.message = "no song in the library".into();
                return;
            };
            (offline::render_song(library, index, setup), format!("song-{}", s.id))
        } else {
            let Some(p) = library.patterns.get(self.pattern_index) else { return };
            // Tracks in auto mode get their new lines loop after loop, on the render's copy.
            let (scales, mut rng) = (&self.scales, Rng::new(setup.seed));
            let mut loop_start = |library: &mut Library, p: usize, number: u64| {
                if let Some(pattern) = library.patterns.get_mut(p).filter(|p| generate::plays_auto(p)) {
                    generate::run_loop(&mut AutoGen::new(scales, &mut || rng.next_f64()), pattern, number);
                }
            };
            (offline::render_pattern(library, self.pattern_index, 1, setup, Some(&mut loop_start)), p.id.clone())
        };
        let Some((left, right)) = rendered else {
            self.message = "nothing to export (no playable clip)".into();
            return;
        };
        let path = dir.join(format!("ordrumbox-{name}.wav"));
        let bytes = wav::encode(&left, &right, setup.sample_rate);
        let result = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, bytes));
        self.message = match result {
            Ok(()) => format!(
                "exported {} ({:.1} s of audio in {:.1} s)",
                path.display(),
                left.len() as f64 / f64::from(setup.sample_rate),
                started.elapsed().as_secs_f64()
            ),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// Ctrl+M: the selected pattern to a MIDI file in the exports folder.
    pub fn export_midi(&mut self) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        let Some(pattern) = self.pattern() else { return };
        let mut rng = Rng::new(1);
        let bytes = midi::export(pattern, 1, &self.roots, &mut rng, &self.scales);
        let path = dir.join(format!("ordrumbox-{}.mid", pattern.id));
        let result = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &bytes));
        self.message = match result {
            Ok(()) => format!("exported {} ({} bytes)", path.display(), bytes.len()),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// A MIDI file dropped on the window (or given on the command line):
    /// new patterns from it, the last one selected.
    pub fn import_midi(&mut self, path: &Path) {
        let name = path.file_name().map_or_else(|| "midi".into(), |n| n.to_string_lossy().into_owned());
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message = format!("not imported: {}: {e}", path.display());
                return;
            }
        };
        match self.editor.import_midi(&name, &bytes, &self.roots) {
            Ok((index, message)) => {
                self.pattern_index = index;
                self.deselect();
                self.edited();
                self.message = message;
            }
            Err(reason) => self.message = format!("{name}: {reason}"),
        }
    }

    pub fn step_pattern(&mut self, delta: isize) {
        let count = self.editor.library().patterns.len();
        if count > 0 {
            self.pattern_index = (self.pattern_index as isize + delta).rem_euclid(count as isize) as usize;
            self.deselect();
            self.message = format!("pattern {}", self.pattern().and_then(|p| p.name.as_deref()).unwrap_or_default());
            // While playing in pattern mode, switch to the new pattern on the
            // current pass instead of waiting for the next loop.
            let playing = self.playing();
            let mode_is_pattern =
                self.controller.as_ref().map(|c| matches!(c.mode(), Mode::Pattern(_))).unwrap_or(false);
            if playing && mode_is_pattern {
                if let Some(controller) = &mut self.controller {
                    controller.select_pattern(self.pattern_index);
                }
                let library = self.editor.library();
                if let Some(controller) = &mut self.controller {
                    controller.refresh(library);
                }
            }
        }
    }

    pub fn step_kit(&mut self, delta: isize) {
        let count = self.kits.kits.len();
        if count == 0 {
            return;
        }
        self.select_kit((self.kit_index as isize + delta).rem_euclid(count as isize) as usize);
        self.message = format!("kit {}", self.kits.kits[self.kit_index].name);
    }

    /// Use kit `index` (its sounds are assigned again).
    fn select_kit(&mut self, index: usize) {
        let Some(kit) = self.kits.kits.get(index) else { return };
        self.kit_index = index;
        if let Some(controller) = &mut self.controller {
            controller.set_kit(&kit.name);
        }
        // A kit change affects every track's sound: rebuild the pass
        // playing now so it is heard at once (not at the next loop).
        let playing = self.playing();
        if playing {
            let library = self.editor.library();
            if let Some(controller) = &mut self.controller {
                controller.refresh(library);
            }
        }
        if let Some(render) = &mut self.render {
            render.kit.clone_from(&kit.name);
        }
        if let Some(sample) = self.kit_editor {
            self.kit_editor = Some(sample.min(kit.instruments.len().saturating_sub(1)));
        }
        self.update_analysis();
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
                    self.synth = None;
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
        self.synth = None;
        self.set_workspace(Workspace::Grid);
    }

    /// Generation on (tracks in auto mode): drums, bass, chords.
    fn generation(&self) -> [bool; 3] {
        let generating = |kind: fn(TrackType) -> bool| {
            self.pattern()
                .is_some_and(|p| p.tracks.iter().any(|t| generate::is_auto_on(t) && kind(detect_track_type(&t.name))))
        };
        [generating(TrackType::is_drum), generating(|k| k == TrackType::Bass), generating(|k| k == TrackType::Piano)]
    }

    /// What the menus show now.
    fn menu_state(&self) -> MenuState {
        let history = self.editor.history();
        let pattern = self.pattern();
        let focus = self.focus_track().and_then(|t| self.track(t)).map(|t| (t.auto, t.mute, t.solo));
        MenuState {
            can_save: self.library_path.is_some(),
            can_render: self.render.is_some(),
            has_pattern: pattern.is_some(),
            undo: history.next_undo().map(str::to_owned),
            redo: history.next_redo().map(str::to_owned),
            has_cursor: self.cursor.is_some(),
            focus_track: focus,
            can_remove_track: focus.is_some() && pattern.is_some_and(|p| p.tracks.len() > 1),
            clipboard_step: matches!(self.clipboard, Some(Clipboard::Step(_))),
            clipboard_track: matches!(self.clipboard, Some(Clipboard::Track(_))),
            has_selection: self.selected.is_some(),
            playing: self.playing(),
            has_songs: !self.editor.library().songs.is_empty(),
            generation: self.generation(),
            kits: self.kits.kits.iter().map(|k| k.name.clone()).collect(),
            kit_index: self.kit_index,
            kit_editor: self.kit_editor.is_some(),
            track_editor: self.show_track_editor,
            grid: self.workspace == Workspace::Grid && self.kit_editor.is_none(),
            song_view: self.song_view_open(),
            piano_roll: self.piano_roll_open(),
            synth: self.synth_open(),
            can_save_song: store::songs_dir().is_some(),
            saved_songs: store::songs_dir().is_some_and(|d| !store::list_songs(&d).is_empty()),
            kit_sample: self.kit_list().is_some_and(|(s, n)| s < n),
        }
    }

    pub fn menus(&self) -> Vec<menu::Menu> {
        menu::menus(&self.menu_state())
    }

    /// The open menu, if any.
    pub fn open_menu(&self) -> Option<menu::Open> {
        self.menu
    }

    pub fn set_menu(&mut self, open: Option<menu::Open>) {
        self.menu = open;
    }

    /// File > Quit was chosen.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Run a menu command (or its shortcut).
    pub fn run(&mut self, command: Command) {
        use Command as C;
        self.menu = None;
        match command {
            C::Save => self.save(),
            C::ImportMidi => self.open_picker(Purpose::ImportMidi),
            C::ExportWav => self.export_wav(false),
            C::ExportSongWav => self.export_wav(true),
            C::ExportMidi => self.export_midi(),
            C::Quit => self.quit = true,
            C::Undo => self.undo(),
            C::Redo => self.redo(),
            C::CopyStep => self.copy(false),
            C::CopyTrack => self.copy(true),
            C::Paste => self.paste(false),
            C::PasteTrack => self.paste(true),
            C::ClearStep => self.delete_step(),
            C::Deselect => self.deselect(),
            C::NewPattern => self.new_pattern(),
            C::DuplicatePattern => self.duplicate_pattern(),
            C::RenamePattern => self.song_text_entry(Id::SongPattern(self.pattern_index)),
            C::PatternDescription => self.song_text_entry(Id::PatternDescription),
            C::PreviousPattern => self.step_pattern(-1),
            C::NextPattern => self.step_pattern(1),
            C::TempoDown => self.step_tempo(-1.0),
            C::TempoUp => self.step_tempo(1.0),
            C::LengthDown => self.change_length(-1),
            C::LengthUp => self.change_length(1),
            C::AddTrack => self.add_track(),
            C::RemoveTrack => self.remove_track(),
            C::NextGrid => self.next_grid(),
            C::LoopBack => self.loop_point_back(),
            C::CompactTracks => self.compact_tracks(),
            C::GeneratePattern => self.generate_pattern(),
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
            C::Kit(i) => {
                self.select_kit(i);
                if let Some(kit) = self.kits.kits.get(i) {
                    self.message = format!("kit {}", kit.name);
                }
            }
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
            C::RenameSong => self.song_text_entry(Id::SongName),
            C::SongDescription => self.song_text_entry(Id::SongDescription),
            C::SaveSong => self.song_button("save"),
            C::LoadSong => self.song_button("load"),
            C::ExportSong => self.song_button("export"),
            C::ImportSong => self.song_button("import"),
            C::NormalizeKit => self.kit_normalize(),
            C::ExportMapping => self.kit_export(),
            C::LoadMapping => self.open_picker(Purpose::LoadMapping),
            C::AddSample => self.open_picker(Purpose::AddSample),
            C::ReplaceSample => self.open_picker(Purpose::ReplaceSample),
            C::ImportFolder => self.open_picker(Purpose::ImportFolder),
            C::Reassign => self.kit_reassign(),
            C::Shortcuts => self.overlay = Some(Overlay::Shortcuts),
            C::About => self.overlay = Some(Overlay::About),
            C::FactoryReset => self.overlay = Some(Overlay::FactoryReset),
        }
    }

    fn open_picker(&mut self, purpose: Purpose) {
        self.overlay = Some(Overlay::Picker(Picker::new(purpose, self.last_dir.as_deref())));
    }

    /// A panel (file picker, key list, about) is over the window.
    pub fn overlay_open(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
    }

    /// A key while a panel is open: the file picker's keys, or any key closes it.
    pub fn overlay_key(&mut self, key: OverlayKey) {
        let is_factory = matches!(self.overlay, Some(Overlay::FactoryReset));
        let Some(Overlay::Picker(picker)) = &mut self.overlay else {
            if is_factory && matches!(key, OverlayKey::Enter) {
                self.factory_reset();
                return;
            }
            if matches!(key, OverlayKey::Escape | OverlayKey::Enter) {
                self.overlay = None;
            }
            return;
        };
        let outcome = match key {
            OverlayKey::Escape => {
                self.overlay = None;
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
        let Some(Overlay::Picker(picker)) = &mut self.overlay else {
            self.overlay = None;
            return;
        };
        let layout = PickerLayout::new(width, height, scale);
        if !layout.contains(x, y) {
            self.overlay = None;
            return;
        }
        if let Some(i) = layout.hit(picker, x, y) {
            let outcome = picker.click(i);
            self.picker_outcome(outcome);
        }
    }

    fn picker_outcome(&mut self, outcome: Outcome) {
        let Outcome::Picked(path) = outcome else { return };
        let Some(Overlay::Picker(picker)) = self.overlay.take() else { return };
        self.last_dir = picker.dir.clone();
        match picker.purpose {
            Purpose::ImportMidi => self.import_midi(&path),
            Purpose::ImportSong => self.import_song(&path),
            Purpose::ImportSynth => self.import_synths(&path),
            Purpose::ReplaceTrackSample => self.replace_track_sample(&path),
            Purpose::ReplaceSample => self.drop_path(&path, true),
            Purpose::AddSample | Purpose::LoadMapping | Purpose::ImportFolder => self.drop_path(&path, false),
        }
    }

    /// Lines of Help > About.
    fn about(&self) -> Vec<String> {
        let shown = |p: Option<PathBuf>| p.map_or_else(|| "-".to_owned(), |p| p.display().to_string());
        vec![
            format!("orDrumbox {} - native port of ordrumbox-v2", env!("CARGO_PKG_VERSION")),
            "GPL-3.0-only - https://www.ordrumbox.com".into(),
            String::new(),
            format!("library   {}", shown(self.library_path.clone())),
            format!("user data {}", shown(store::user_dir())),
            format!("exports   {}", shown(store::exports_dir())),
            format!("assets    {}", shown(self.assets_dir.clone())),
        ]
    }

    /// Lines of Help > Factory reset.
    fn factory_reset_lines(&self) -> Vec<String> {
        let dir = self.library_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(store::user_dir);
        vec![
            "Restore the factory settings?".into(),
            String::new(),
            "The user files are removed (moved out of the way):".into(),
            "  library.json, kits.json, settings.json,".into(),
            "  generated_sounds.json, samples/, songs/".into(),
            String::new(),
            format!("folder: {}", dir.map_or_else(|| "-".to_owned(), |p| p.display().to_string())),
            String::new(),
            "ENTER: restore     ESC or click: cancel".into(),
        ]
    }

    /// Help > Factory reset: move the user's edited files aside and reload
    /// from the assets, as at first start.
    pub fn factory_reset(&mut self) {
        self.overlay = None;
        self.menu = None;
        self.dropdown = None;
        self.entry = None;
        let dir = self.library_path.as_ref().and_then(|p| p.parent().map(Path::to_path_buf)).or_else(store::user_dir);
        let Some(dir) = dir else {
            self.message = "factory reset: no user data folder".into();
            return;
        };
        match store::factory_reset(&dir) {
            Ok(removed) => {
                *self = Self::load();
                self.message = if removed.is_empty() {
                    "factory settings restored (no user files found)".into()
                } else {
                    format!("factory settings restored (removed: {})", removed.join(", "))
                };
            }
            Err(e) => self.message = format!("factory reset failed: {e}"),
        }
    }

    // ─── Kit editor ──────────────────────────────────────────────────────────

    pub fn kit_editor_open(&self) -> bool {
        self.kit_editor.is_some()
    }

    /// The kit editor's selected sample and the kit's sample count, when open.
    pub fn kit_list(&self) -> Option<(usize, usize)> {
        let sample = self.kit_editor?;
        Some((sample, self.kits.kits.get(self.kit_index).map_or(0, |k| k.instruments.len())))
    }

    /// Ctrl+K: the kit editor (the drumkit manager), open or closed.
    pub fn toggle_kit_editor(&mut self) {
        self.kit_editor = match self.kit_editor {
            Some(_) => None,
            None => Some(0),
        };
        self.focus = None;
        self.dropdown = None;
        self.update_analysis();
    }

    /// The analysis of the sample shown, computed when it changes.
    fn update_analysis(&mut self) {
        let url = self.kit_editor.and_then(|s| self.kits.sample(self.kit_index, s)).map(|i| i.url.clone());
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
        self.kits_dirty = None;
        self.track_analysis.borrow_mut().clear();
        let (bank, errors) = self.kits.bank();
        self.roots =
            bank.samples.iter().filter_map(|s| Some((s.instrument.key.clone(), s.instrument.root_midi?))).collect();
        let bank = Arc::new(bank);
        if let Some(controller) = &mut self.controller {
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
        let count = self.kits.kits.get(self.kit_index).map_or(0, |k| k.instruments.len());
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
        let Some(url) = self.kits.sample(self.kit_index, sample).map(|i| i.url.clone()) else { return };
        if let Some(controller) = &mut self.controller
            && let Some(index) = controller.bank().find_url(&url)
        {
            controller.audition_sample(index);
        }
    }

    /// Delete: the selected sample out of the kit.
    pub fn kit_remove(&mut self) {
        let Some(sample) = self.kit_editor else { return };
        if let Some(message) = self.kits.remove(self.kit_index, sample) {
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

    /// R: the sounds of the tracks assigned again (`autoDetectAll`).
    pub fn kit_reassign(&mut self) {
        self.select_kit(self.kit_index);
        self.message = "Auto-detect complete".into();
    }

    /// E: the kit's mapping to a JSON file in the exports folder.
    pub fn kit_export(&mut self) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        self.message = self.kits.export_mapping(self.kit_index, &dir).unwrap_or_else(|e| e);
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
    /// while playing. Before the next pass of a pattern is built, its tracks
    /// in auto mode get their new lines (the JS loop start).
    pub fn update(&mut self) {
        let Some(controller) = &mut self.controller else { return };
        controller.select_pattern(self.pattern_index);
        if self.editor.revision() != self.played_revision {
            controller.refresh(self.editor.library());
        }
        if let (Some(number), Mode::Pattern(p)) = (controller.pending_pass(), controller.mode()) {
            let rng = &mut self.rng;
            self.editor.auto_loop(p, number, &self.scales, &mut || rng.next_f64());
        }
        self.played_revision = self.editor.revision();
        controller.update(self.editor.library());
    }

    fn audition(&mut self, t: usize, note: Option<usize>) {
        if let Some(controller) = &mut self.controller {
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
        self.changed_at = Some(Instant::now());
        if let Some(desc) = self.editor.history().next_undo() {
            self.message = desc.to_owned();
        }
    }

    /// Keep the pattern, cursor and selection inside the (changed) library.
    fn clamp_selection(&mut self) {
        let count = self.editor.library().patterns.len();
        self.pattern_index = self.pattern_index.min(count.saturating_sub(1));
        let tracks = self.pattern().map_or(0, |p| p.tracks.len());
        if let Some(cursor) = &mut self.cursor {
            if tracks == 0 {
                self.cursor = None;
            } else {
                cursor.track = cursor.track.min(tracks - 1);
            }
        }
        let valid = |&(t, n): &(usize, usize)| self.track(t).is_some_and(|t| n < t.notes.len());
        self.selected = self.selected.filter(valid);
    }

    pub fn undo(&mut self) {
        self.message = match self.editor.undo() {
            Some(desc) => format!("undone: {desc}"),
            None => "nothing to undo".into(),
        };
        self.after_history();
    }

    pub fn redo(&mut self) {
        self.message = match self.editor.redo() {
            Some(desc) => format!("redone: {desc}"),
            None => "nothing to redo".into(),
        };
        self.after_history();
    }

    fn after_history(&mut self) {
        self.selected = None;
        self.clamp_selection();
        self.changed_at = Some(Instant::now());
    }

    pub fn click(&mut self, hit: Hit) {
        match hit {
            Hit::Mute(t) => self.toggle_row(t, "mute"),
            Hit::Solo(t) => self.toggle_row(t, "solo"),
            Hit::Name(t) => {
                self.cursor = Some(Cursor { track: t, beat: self.cursor.map_or(0, |c| c.beat), step: 0 });
                self.selected = None;
                self.note_focus = Some(NoteFocus::Track(t));
                self.audition(t, None);
            }
            Hit::Cell { track, beat, step } => {
                self.cursor = Some(Cursor { track, beat, step });
                self.toggle_note(track, beat, step);
            }
        }
    }

    /// The JS grid click / Enter: add a note on an empty step, select the
    /// note of a step, delete the selected note.
    fn toggle_note(&mut self, t: usize, beat: i32, step: i32) {
        let p = self.pattern_index;
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.beat_step == step);
        match at_step {
            Some(n) if self.selected == Some((t, n)) => {
                let pitch = track.notes[n].pitch;
                self.editor.delete_note(p, t, beat, step, pitch);
                self.selected = None;
                self.note_focus = Some(NoteFocus::Step { track: t, beat, step });
                self.edited();
            }
            Some(n) => {
                self.selected = Some((t, n));
                self.audition(t, Some(n));
            }
            None => {
                self.selected = self.editor.add_note(p, t, beat, step, 0.0).map(|n| (t, n));
                self.edited();
                self.audition(t, self.selected.map(|(_, n)| n));
            }
        }
    }

    pub fn enter(&mut self) {
        let Some(c) = self
            .cursor
            .or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| Cursor { track: 0, beat: 0, step: 0 }))
        else {
            return;
        };
        self.cursor = Some(c);
        self.toggle_note(c.track, c.beat, c.step);
    }

    pub fn move_cursor(&mut self, direction: Move) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let tracks = pattern.tracks.len();
        if tracks == 0 {
            return;
        }
        let beats = pattern.beat_count.max(1);
        let Some(mut c) = self.cursor else {
            self.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
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
        self.cursor = Some(c);
        self.note_focus = Some(NoteFocus::Step { track: c.track, beat: c.beat, step: c.step });
        let note = pattern.tracks[c.track].notes.iter().position(|n| n.beat == c.beat && n.beat_step == c.step);
        self.selected = note.map(|n| (c.track, n));
        if let Some(n) = note {
            self.audition(c.track, Some(n));
        }
    }

    /// Shift+arrows on the selected note: ↑↓ velocity, ←→ pitch.
    pub fn nudge(&mut self, direction: Move) {
        let Some((t, n)) = self.selected else {
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
        let count = self.editor.compact_pattern(self.pattern_index);
        if count == 0 {
            self.message = "no track repeats a shorter loop".into();
            return;
        }
        self.selected = None;
        self.clamp_selection();
        self.edited();
        self.message = format!("compacted {count} track{}", if count == 1 { "" } else { "s" });
    }

    pub fn delete_step(&mut self) {
        let Some(c) = self.cursor else { return };
        if self.editor.delete_step(self.pattern_index, c.track, c.beat, c.step) {
            self.edited();
        }
        self.selected = None;
    }

    pub fn deselect(&mut self) {
        self.cursor = None;
        self.selected = None;
    }

    /// The track the cursor is on, else the selected note's.
    fn focus_track(&self) -> Option<usize> {
        self.cursor.map(|c| c.track).or(self.selected.map(|(t, _)| t)).filter(|&t| self.track(t).is_some())
    }

    pub fn toggle_row(&mut self, t: usize, key: &str) {
        if self.track(t).is_some() && self.editor.toggle_track(self.pattern_index, t, key) {
            self.edited();
        }
    }

    pub fn step_tempo(&mut self, delta: f64) {
        let Some(bpm) = self.pattern().map(|p| p.bpm) else { return };
        self.set_tempo(bpm + delta);
    }

    /// The pattern at `bpm` (20–300, whole), a run of changes one undo step.
    pub(super) fn set_tempo(&mut self, bpm: f64) {
        let bpm = bpm.round().clamp(20.0, 300.0);
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
        if let Some(c) = &mut self.cursor {
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

    pub fn loop_point_back(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.decr_loop_point(self.pattern_index, t) {
            self.edited();
        }
    }

    /// A new track `T<n>` (the JS "add track" button).
    pub fn add_track(&mut self) {
        let Some(count) = self.pattern().map(|p| p.tracks.len()) else { return };
        if let Some(t) = self.editor.add_track(self.pattern_index, &format!("T{}", count + 1), 4) {
            self.cursor = Some(Cursor { track: t, beat: 0, step: 0 });
            self.selected = None;
            self.edited();
        }
    }

    /// Remove the focused track, never the last one.
    pub fn remove_track(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.pattern().is_some_and(|p| p.tracks.len() > 1) && self.editor.remove_track(self.pattern_index, t) {
            self.selected = None;
            self.edited();
            self.clamp_selection();
        }
    }

    // ─── Generation ──────────────────────────────────────────────────────────

    /// B: a new pattern, generated (genre, tracks and lines).
    pub fn generate_pattern(&mut self) {
        let rng = &mut self.rng;
        self.pattern_index = self.editor.generate_pattern(&self.scales, &mut || rng.next_f64());
        self.deselect();
        self.edited();
        if let Some(pattern) = self.pattern() {
            let genre = pattern.extra.get("_autoGenGenre").and_then(|g| g.as_str()).unwrap_or_default();
            self.message = format!("generated {} ({genre})", pattern.name.as_deref().unwrap_or_default());
        }
    }

    /// D, S, A: ↻ Drum / Bass / Chords — new lines in auto mode, or auto mode off.
    pub fn toggle_generation(&mut self, part: Option<MelodicKind>) {
        let p = self.pattern_index;
        let rng = &mut self.rng;
        let rnd = &mut || rng.next_f64();
        let on = match part {
            None => self.editor.toggle_drums(p, &self.scales, rnd),
            Some(kind) => self.editor.toggle_melodic(p, kind, &self.scales, rnd),
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
            let genre = self.pattern().and_then(|p| p.extra.get("_autoGenGenre")?.as_str()).unwrap_or_default();
            format!("{name}: generated, new lines every loop ({genre})")
        } else {
            format!("{name}: auto off")
        };
    }

    /// Z: auto mode of the focused track (a new line every loop).
    pub fn toggle_auto(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "auto") {
            self.edited();
        }
    }

    /// X: random notes on the focused track.
    pub fn randomize_track(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        let rng = &mut self.rng;
        if self.editor.randomize_track(self.pattern_index, t, &mut || rng.next_f64()) {
            self.selected = None;
            self.edited();
        }
    }

    pub fn new_pattern(&mut self) {
        self.pattern_index = self.editor.add_pattern(None);
        self.deselect();
        self.edited();
    }

    pub fn duplicate_pattern(&mut self) {
        if let Some(index) = self.editor.duplicate_pattern(self.pattern_index) {
            self.pattern_index = index;
            self.deselect();
            self.edited();
        }
    }

    /// Ctrl+C: the notes of the cursor step (with Shift, or no cursor: the track).
    pub fn copy(&mut self, track: bool) {
        let Some(t) = self.focus_track() else { return };
        let Some(source) = self.track(t).cloned() else { return };
        let name = source.name.clone();
        if track || self.cursor.is_none() {
            self.message = format!("copied track \"{name}\" ({} notes)", source.notes.len());
            self.clipboard = Some(Clipboard::Track(Box::new(source)));
        } else if let Some(c) = self.cursor {
            let notes: Vec<Note> =
                source.notes.iter().filter(|n| n.beat == c.beat && n.beat_step == c.step).cloned().collect();
            self.message = format!("copied {} note(s) — {name} @ {}.{}", notes.len(), c.beat + 1, c.step + 1);
            self.clipboard = Some(Clipboard::Step(notes));
        }
    }

    /// Ctrl+V: the copied step at the cursor, or the copied track after the
    /// focused one (with Shift, only a track).
    pub fn paste(&mut self, track_only: bool) {
        let p = self.pattern_index;
        match self.clipboard.clone() {
            None => self.message = "clipboard is empty".into(),
            Some(Clipboard::Track(track)) => {
                let after = self.focus_track().map_or_else(|| self.pattern().map_or(0, |p| p.tracks.len()), |t| t + 1);
                if let Some(t) = self.editor.paste_track(p, after, &track) {
                    self.cursor = Some(Cursor { track: t, beat: 0, step: 0 });
                    self.selected = None;
                    self.edited();
                }
            }
            Some(Clipboard::Step(_)) if track_only => self.message = "no track copied".into(),
            Some(Clipboard::Step(notes)) => {
                let c = self.cursor.unwrap_or(Cursor { track: 0, beat: 0, step: 0 });
                if self.editor.paste_step_notes(p, c.track, c.beat, c.step, &notes) {
                    self.cursor = Some(c);
                    self.selected = None;
                    self.edited();
                }
            }
        }
    }

    // ─── Saving ──────────────────────────────────────────────────────────────

    pub fn save(&mut self) {
        let Some(path) = &self.library_path else {
            self.message = "not saved: no library file (see the messages below)".into();
            return;
        };
        match store::save(path, self.editor.library()) {
            Ok(()) => {
                self.saved_revision = self.editor.revision();
                self.changed_at = None;
                self.message = format!("saved {}", path.display());
            }
            Err(e) => self.message = format!("not saved: {}: {e}", path.display()),
        }
    }

    pub fn save_if_changed(&mut self) {
        if self.library_path.is_some() && self.editor.revision() != self.saved_revision {
            self.save();
        }
    }

    /// Save once the edits have settled; when to call again, if a save is pending.
    /// Save what is due (the library, kit knob edits); when the next save is.
    pub fn autosave(&mut self) -> Option<Instant> {
        let due = [self.flush_kits(), self.flush_master(), self.flush_synths(), self.autosave_library()];
        due.into_iter().flatten().min()
    }

    fn autosave_library(&mut self) -> Option<Instant> {
        let changed = self.changed_at?;
        if self.library_path.is_none() || self.editor.revision() == self.saved_revision {
            self.changed_at = None;
            return None;
        }
        let due = changed + SAVE_DELAY;
        if Instant::now() >= due {
            self.save();
            return None;
        }
        Some(due)
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    /// The window's size and scale (where the lists opened from the menus go).
    pub fn set_size(&mut self, size: (i32, i32, i32)) {
        self.panel_size = size;
    }

    pub fn draw(&mut self, canvas: &mut Canvas, scale: i32) {
        let layout = self.panel_layout(canvas.width(), canvas.height(), scale);
        let position = self.controller.as_ref().and_then(Controller::position);
        let history = self.editor.history();
        let library = self.editor.library();
        let song_mode = self.controller.as_ref().and_then(|c| match c.mode() {
            Mode::Song(s) => library.songs.get(s),
            Mode::Pattern(_) => None,
        });
        let song = match (song_mode, position) {
            (Some(song), Some(p)) => {
                let total = song.length_measures().max(1.0);
                let measure = (p.pass as f64 + p.frame as f64 / p.frames.max(1) as f64) % total;
                Some(format!("SONG {}  measure {}/{}", song.name, measure.floor() + 1.0, total.ceil()))
            }
            _ => library.songs.get(self.editor.selected_song()).map(|s| format!("song {}", s.name)),
        };
        // Smoothed per-track levels for the VU meter stripe (peak-hold + decay).
        let raw_levels = self.controller.as_ref().map(|c| c.levels()).unwrap_or_default();
        if self.track_levels.len() < raw_levels.len() {
            self.track_levels.resize(raw_levels.len(), 0.0);
        }
        for level in &mut self.track_levels {
            *level *= 0.88;
            if *level < 0.001 {
                *level = 0.0;
            }
        }
        for (i, level) in self.track_levels.iter_mut().enumerate() {
            let r = raw_levels.get(i).copied().unwrap_or(0.0).abs();
            if r > *level {
                *level = r;
            }
        }
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
            genre: pattern.and_then(|p| p.extra.get("_autoGenGenre")?.as_str()),
            pattern_index: self.pattern_index,
            pattern_count: library.patterns.len(),
            kit_view: self.kit_view(layout.details_top),
            right: layout.right,
            levels: self.track_levels.clone(),
            playhead: position.filter(|_| song_mode.is_none()).map(|p| p.frame as f64 / p.frames.max(1) as f64),
            song: song.as_deref(),
            cursor: self.cursor,
            selected: self.selected,
            undo: history.next_undo(),
            redo: history.next_redo(),
            message: &self.message,
            status: &self.status,
        };
        view::draw(canvas, &view, scale);
        self.draw_synth(canvas, scale);
        crate::panel::draw(canvas, &layout.items, self.focus, scale);
        self.draw_tooltip(canvas, scale, layout.right);
        menu::draw(canvas, &self.menus(), self.menu, scale);
        if let Some(dropdown) = &self.dropdown {
            dropdown.draw(canvas, scale);
        }
        if let Some(entry) = &self.entry {
            entry.draw(canvas, scale);
        }
        match &self.overlay {
            Some(Overlay::Picker(p)) => {
                let layout = PickerLayout::new(canvas.width(), canvas.height(), scale);
                picker::draw(canvas, p, &layout);
            }
            Some(Overlay::Shortcuts) => {
                let lines: Vec<String> = SHORTCUTS.iter().map(|l| (*l).to_owned()).collect();
                view::draw_notice(canvas, "Keyboard shortcuts", &lines, scale);
            }
            Some(Overlay::About) => view::draw_notice(canvas, "About orDrumbox", &self.about(), scale),
            Some(Overlay::FactoryReset) => {
                view::draw_notice(canvas, "Factory reset", &self.factory_reset_lines(), scale)
            }
            None => {}
        }
    }

    /// What the kit editor shows; its sample's details start at `details_top`.
    fn kit_view(&self, details_top: i32) -> Option<KitView<'_>> {
        let sample = self.kit_editor?;
        let kit = self.kits.kits.get(self.kit_index)?;
        let samples = kit.instruments.iter().map(|i| (i.display_name.as_str(), i.key.as_str())).collect();
        let selected = kit.instruments.get(sample);
        let analysis =
            self.analysis.as_ref().filter(|(url, _)| selected.is_some_and(|i| i.url == *url)).map(|(_, a)| a);
        let info = analysis.map(|a| {
            let db = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "-".into() };
            let note = a.note.map_or_else(|| "-".to_owned(), |n| n.to_string());
            format!("{note}  {:.0} ms  {} dB peak  {} dB RMS", a.duration_sec * 1000.0, db(a.peak_db), db(a.rms_db))
        });
        let detected = selected.map(|i| {
            let name = if i.display_name.is_empty() { &i.url } else { &i.display_name };
            ordrumbox_core::instruments::catalog()
                .from_file_name(name)
                .map_or_else(|| "No instrument detected".to_owned(), |d| format!("Detected: {}", d.id))
        });
        let decay_marker = match (selected, analysis) {
            (Some(i), Some(a)) if a.duration_sec > 0.0 => Some((i.decay_ms() / 1000.0 / a.duration_sec).min(1.0)),
            _ => None,
        };
        Some(KitView {
            kit: &kit.name,
            samples,
            selected: sample,
            details_top,
            info,
            detected,
            envelope: analysis.map_or(&[][..], |a| &a.envelope),
            decay_marker,
        })
    }
}
