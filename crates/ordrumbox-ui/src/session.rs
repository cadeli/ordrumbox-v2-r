//! Everything the window drives: the library being edited, the playback, the
//! generators, MIDI files, the grid cursor and selection, the clipboard, saving. Edits go through the
//! [`Editor`] (each one an undo step); the playing pass is rebuilt after each.
//!
//! This file holds the session's state; its loading is `session_open.rs`, undo and saving `session_history.rs`,
//! its drawing `session_draw.rs`, its toasts `session_toast.rs`; the rest is in `session_*.rs`, one `impl Session` each:
//! `session_transport.rs` (play, the pattern, kit and tempo), `session_edit.rs`
//! (the grid's edits), `session_generate.rs`, `session_commands.rs` (the
//! menus), `session_overlay.rs`, `session_kit.rs`, `session_files.rs`
//! (exports, imports, dropped files), and `session_panel*.rs` (the editors'
//! widgets).
//!
//! The state is held in groups, each declared with the code that owns it:
//! `Chrome`, `Selection`, `Saving`, `Playback`, `Transport`
//! (`session_transport.rs`), `Generators` (`session_generators.rs`), `Synths`
//! (`session_synth.rs`), `Host` (`session_host.rs`) and `Program`
//! (`session_commands.rs`).
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

use ordrumbox_core::{Library, Pattern, Rng, Scales, Track};
use ordrumbox_engine::analysis::Analysis;
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{Mode, SynthPresets, wav};
use ordrumbox_logic::generate::{self, AutoGenerator};
use ordrumbox_logic::midi;
use ordrumbox_logic::{Editor, compose};

use crate::action_log::ActionLog;
use crate::editors::EditorState;
use crate::kits::KitSet;
use crate::mcp_link;
use crate::menu::{self};
use crate::panel::Rect;
use crate::panel::{Dropdown, Entry, Id};
use crate::picker::{Export, Picker, Purpose};
use crate::store;
use crate::view::{self};
use session_chrome::Chrome;
use session_confirm::Pending;
use session_generators::Generators;
use session_playback::Playback;
use session_saving::Saving;
use session_selection::Selection;

#[path = "session_chrome.rs"]
mod session_chrome;
#[path = "session_color.rs"]
mod session_color;
#[path = "session_commands.rs"]
mod session_commands;
#[path = "session_confirm.rs"]
mod session_confirm;
#[path = "session_draw.rs"]
mod session_draw;
#[path = "session_edit.rs"]
mod session_edit;
#[path = "session_files.rs"]
mod session_files;
#[path = "session_generate.rs"]
mod session_generate;
#[path = "session_generators.rs"]
mod session_generators;
#[path = "session_grid.rs"]
mod session_grid;
#[path = "session_history.rs"]
mod session_history;
#[path = "session_host.rs"]
mod session_host;
#[path = "session_labels.rs"]
mod session_labels;
#[path = "session_lane.rs"]
mod session_lane;
#[path = "session_open.rs"]
mod session_open;
#[path = "session_prefs.rs"]
mod session_prefs;
#[path = "session_record.rs"]
mod session_record;
#[path = "session_roll_zone.rs"]
mod session_roll_zone;
#[path = "session_tempo.rs"]
mod session_tempo;
#[path = "session_toast.rs"]
mod session_toast;
#[path = "session_zone.rs"]
mod session_zone;
#[path = "session_zoom.rs"]
mod session_zoom;

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
#[path = "session_types.rs"]
mod session_types;
#[path = "session_update.rs"]
mod session_update;
#[path = "session_user_data.rs"]
mod session_user_data;
pub use session_open::LibrarySource;
pub use session_panel::PanelKey;
pub use session_types::{Move, OverlayKey, Workspace};

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

pub struct Session {
    /// What the window shows around the library (`session_chrome.rs`).
    chrome: Chrome,
    /// The cursor, the selected note, the clipboard (`session_selection.rs`).
    selection: Selection,
    /// Where the files go and when (`session_saving.rs`).
    saving: Saving,
    editor: Editor,
    /// What the generators read and draw from (`session_generators.rs`).
    generators: Generators,
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
    /// The synth presets: those played, the assets' (`session_synth.rs`).
    synths: session_synth::Synths,
    pattern_index: usize,
    /// What is shown under the header.
    workspace: Workspace,
    /// Where and how the session plays (`session_transport.rs`).
    transport: session_transport::Transport,
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
    /// What the session asks of the program that runs it (`session_commands.rs`).
    program: session_commands::Program,
    /// Tabs and selection of the track and note editors.
    editors: EditorState,
    /// The plugin host the session follows, if any (`session_host.rs`).
    host: session_host::Host,
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
    /// A control's value copied from its right-click menu, to paste on
    /// another whose range holds it.
    copied_value: Option<f64>,
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
    /// Edit > Preferences: the settings being chosen, kept when OK is chosen.
    Preferences(session_prefs::Prefs),
    /// Help > Factory reset, until the user says so: `yes` is the button
    /// ENTER would run, `false` (the default) keeps.
    FactoryReset {
        yes: bool,
    },
    /// What was done; the first row shown.
    History {
        scroll: usize,
    },
    /// The file an export would replace, until the user says so: `yes` is
    /// the button ENTER would run, `false` (the default) keeps the file.
    ConfirmOverwrite {
        export: Export,
        path: PathBuf,
        yes: bool,
    },
    /// What removes or replaces the user's work, until the user says so:
    /// `yes` is the button ENTER would run, `false` (the default) keeps.
    Confirm {
        pending: Pending,
        yes: bool,
    },
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
    /// The program lists the sound devices with `devices`, for File > Audio output.
    pub fn offer_devices(&mut self, devices: crate::audio_output::OutputDevices) {
        self.playback.offers_mut().devices = Some(devices);
    }

    /// The program lists the MIDI inputs with `ports`, for Edit > Preferences.
    pub fn offer_midi_inputs(&mut self, ports: crate::audio_output::OutputDevices) {
        self.playback.offers_mut().midi_ports = Some(ports);
    }

    /// The program has a MIDI input open (or has not): its notes are
    /// recorded when recording, and play the pattern's tracks.
    pub fn set_midi_open(&mut self, open: bool) {
        self.playback.offers_mut().midi_open = open;
    }

    /// The MIDI input chosen in Edit > Preferences changed since the last
    /// call: the program opens the one now chosen.
    pub fn take_midi_change(&mut self) -> bool {
        std::mem::take(&mut self.program.midi_changed)
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

    /// A mode button: the song panel, or the grid in its place (what is
    /// shown is left as it is when it already is the mode).
    pub(super) fn show_mode(&mut self, song: bool) {
        if song == (self.workspace == Workspace::Song) && self.kit_editor.is_none() {
            return;
        }
        self.show_view(if song { 2 } else { 0 });
    }

    /// Show the grid (View > Grid, F5).
    pub(super) fn show_grid(&mut self) {
        self.synth_editor.close();
        self.set_workspace(Workspace::Grid);
    }

    /// The session is a plugin's: the host plays, saves and has the keys it
    /// leaves out of the menus.
    pub fn hosted(&self) -> bool {
        self.host.hosted
    }

    /// A session of `library` with no kit, a plugin's when `hosted`.
    #[cfg(test)]
    pub(crate) fn for_tests(library: Library, hosted: bool) -> Self {
        let mut session = Self::new(library, KitSet::empty(), Vec::new());
        session.host.hosted = hosted;
        session
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

    /// Keep the engine fed and the playing pass current; call every frame
    /// while playing. The next pass of a pattern is built with the new lines
    /// of its tracks in auto mode (the JS loop start); they replace the library's
    /// when that pass starts.
    pub fn update(&mut self) {
        // What the library's tags teach, learned again after an edit (the header and menus read it).
        self.generators.learn(&self.editor);
        self.refresh_synths();
        let region = self.song_loop_region().map(|(first, end)| (u64::from(first), u64::from(end)));
        self.playback.set_song_loop(&self.editor, region);
        self.playback.set_metronome(&self.editor, self.transport.metronome);
        let midi_open = self.playback.offers().midi_open;
        self.playback.set_live_midi(midi_open);
        self.playback
            .update(&mut self.editor, self.pattern_index, |editor, p, pass| self.generators.auto_loop(editor, p, pass));
        self.report_host_tempo();
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
}
