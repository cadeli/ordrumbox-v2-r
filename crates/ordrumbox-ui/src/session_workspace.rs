//! Which view is shown under the header: the grid, the piano roll, the song
//! panel or the synth editor, and the playback mode that follows it.

use super::Session;
use super::session_types::Workspace;
use ordrumbox_engine::Mode;

impl Session {
    /// V: show/hide the VU meters on the track stripes.
    pub fn toggle_vus(&mut self) {
        self.chrome.show_vus = !self.chrome.show_vus;
        self.message = if self.chrome.show_vus { "VU meters on".into() } else { "VU meters off".into() };
    }

    /// A view button (`view::VIEWS`): show that view, unless it is shown.
    /// The last button (KIT) toggles the kit editor over the main editor.
    pub(super) fn show_view(&mut self, i: u8) {
        if i == 4 {
            self.toggle_kit_editor();
            return;
        }
        let target = [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth];
        let Some(&target) = target.get(usize::from(i)) else { return };
        if self.workspace == target && !self.kit_editor.is_open() {
            return;
        }
        self.kit_editor.sample = None;
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
        if song == (self.workspace == Workspace::Song) && !self.kit_editor.is_open() {
            return;
        }
        self.show_view(if song { 2 } else { 0 });
    }

    /// Show the grid (View > Grid, F5).
    pub(super) fn show_grid(&mut self) {
        self.synth_editor.close();
        self.set_workspace(Workspace::Grid);
    }

    /// Show `workspace`. While playing, a switch to or from the song panel
    /// switches the playback to the view's mode, from its start
    ///; the grid and the piano roll share one mode.
    pub(super) fn set_workspace(&mut self, workspace: Workspace) {
        let was_song = self.workspace == Workspace::Song;
        self.workspace = workspace;
        self.kit_editor.sample = None;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.song_panel.hovered_clip = None;
        self.song_panel.drag = None;
        self.song_panel.bar_drag = None;
        self.roll.release();
        if self.playing() && was_song != (workspace == Workspace::Song) {
            let mode = self.play_mode();
            self.playback.play(&self.editor, mode);
        }
    }

    /// What Space plays: the song in the song panel, else the pattern.
    fn play_mode(&self) -> Mode {
        match self.workspace {
            Workspace::Song => Mode::Song(self.editor.selected_song()),
            _ => Mode::Pattern(self.pattern_index),
        }
    }
}
