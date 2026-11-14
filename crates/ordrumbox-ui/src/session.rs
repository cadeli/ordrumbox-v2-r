//! Everything the window drives: the library being edited, the playback, the
//! generators, MIDI files, the grid cursor and selection, the clipboard, saving. Edits go through the
//! [`Editor`] (each one an undo step); the playing pass is rebuilt after each.
//!
//! This file holds the session's state and nothing else; its loading is `session_open.rs`, undo and saving
//! `session_history.rs`, its drawing `session_draw.rs`, its toasts `session_toast.rs`; the rest is in
//! `session_*.rs`, one `impl Session` each:
//! `session_transport.rs` (play, the pattern, kit and tempo), `session_workspace.rs` (the view shown),
//! `session_edit.rs` (the grid's edits), `session_generate.rs`, `session_commands.rs` (the
//! menus), `session_overlay.rs`, `session_kit.rs`, `session_files.rs`
//! (exports, imports, dropped files), and `session_panel*.rs` (the editors'
//! widgets).
//!
//! The state is held in groups, each declared with the code that owns it:
//! `Chrome`, `Selection`, `Saving`, `Playback`, `Transport`
//! (`session_transport.rs`), `Generators` (`session_generators.rs`), `Synths`
//! (`session_synth.rs`), `Host` (`session_host.rs`), `Program`
//! (`session_commands.rs`), `KitEditor` (`session_kit.rs`) and `Flash`
//! (`session_toast.rs`).
//!
//! The song panel (F3) and the piano roll (F4) take the grid's place:
//! `session_song*.rs`, `session_roll.rs`.
//!
//! The grid follows the JS pattern panel (`pointer_section.js`,
//! `keyboard_section.js`, `clipboard_section.js`): a click on an empty step
//! adds a note, on a note selects it, on the selected note deletes it.

use std::path::PathBuf;
use std::sync::Arc;

use ordrumbox_core::{Pattern, Scales, Track};
use ordrumbox_engine::SynthPresets;
use ordrumbox_engine::analysis::Analysis;
use ordrumbox_engine::kit::Kit;
use ordrumbox_engine::offline::RenderSetup;
use ordrumbox_logic::Editor;

use crate::action_log::ActionLog;
use crate::editors::EditorState;
use crate::kits::KitSet;
use crate::mcp_link;
use crate::menu::{self};
use session_chrome::Chrome;
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
#[path = "session_roll_length.rs"]
mod session_roll_length;
#[path = "session_roll_zone.rs"]
mod session_roll_zone;
#[path = "session_tempo.rs"]
mod session_tempo;
#[path = "session_toast.rs"]
mod session_toast;
#[path = "session_workspace.rs"]
mod session_workspace;
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
    /// The kit editor (`session_kit.rs`).
    kit_editor: session_kit::KitEditor,
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
    /// The message as last shown, and the toasts on screen (`session_toast.rs`).
    flash: session_toast::Flash,
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

    /// The kit used.
    fn kit(&self) -> Option<&Kit> {
        self.kits.kits().get(self.kit_index)
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
}
