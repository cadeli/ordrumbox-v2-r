//! Which view is shown under the header (the grid, the piano roll, the song
//! panel, the synth editor, the mixer) and, apart from it, the play mode the
//! mode switch chose.

use super::Session;
use super::session_types::{PlayMode, Workspace};
use crate::view;
use ordrumbox_core::ActiveClip;
use ordrumbox_core::steps::TICK;
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
        if i == view::KIT_VIEW {
            self.toggle_kit_editor();
            return;
        }
        let target = [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth, Workspace::Mixer];
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
                Workspace::Mixer => self.toggle_mixer(),
            }
        }
    }

    /// A mode button: the pattern or the song mode, and the view of that
    /// mode: the song panel, or the grid in its place (another view is left
    /// as it is for the pattern mode).
    pub(super) fn show_mode(&mut self, mode: PlayMode) {
        self.set_mode(mode);
        let song = mode == PlayMode::Song;
        if song != (self.workspace == Workspace::Song) || self.kit_editor.is_open() {
            self.show_view(if song { 2 } else { 0 });
        }
    }

    /// The play mode is `mode`. From the song to the pattern, the pattern
    /// plays on at the song's tempo. While playing, a change of mode plays
    /// the new one from its start.
    pub(super) fn set_mode(&mut self, mode: PlayMode) {
        if self.mode == mode {
            return;
        }
        let song_tempo = self.tempo_song().map(|_| self.tempo());
        self.mode = mode;
        if let (PlayMode::Pattern, Some(bpm)) = (mode, song_tempo) {
            self.set_tempo(bpm);
        }
        if self.playing() {
            let mode = self.play_mode();
            self.playback.play(&self.editor, mode);
        }
    }

    /// The song mode is chosen (the mode switch), whatever the view.
    pub(super) fn is_song_mode(&self) -> bool {
        self.mode == PlayMode::Song
    }

    /// Show the grid (View > Grid, F5).
    pub(super) fn show_grid(&mut self) {
        self.synth_editor.close();
        self.set_workspace(Workspace::Grid);
    }

    /// Show `workspace`. What plays goes on: the view does not change the mode.
    pub(super) fn set_workspace(&mut self, workspace: Workspace) {
        self.workspace = workspace;
        self.kit_editor.sample = None;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
        self.song_panel.hovered_clip = None;
        self.song_panel.hovered_lane = None;
        self.song_panel.drag = None;
        self.song_panel.bar_drag = None;
        self.roll.release();
    }

    /// What Space plays: the selected song in song mode, else the pattern.
    fn play_mode(&self) -> Mode {
        self.mode_of(self.mode)
    }

    /// What `mode` plays: the selected song, or the selected pattern.
    pub(super) fn mode_of(&self, mode: PlayMode) -> Mode {
        match mode {
            PlayMode::Song => Mode::Song(self.editor.selected_song()),
            PlayMode::Pattern => Mode::Pattern(self.pattern_index),
        }
    }

    /// The clip the song plays now, if a song plays: the first of the clips
    /// sounding at the playhead (`None` when stopped or between clips).
    pub(super) fn song_clip_playing(&self) -> Option<ActiveClip> {
        let Some(Mode::Song(s)) = self.playback.mode().filter(|_| self.playing()) else {
            return None;
        };
        let library = self.editor.library();
        let tick = self.playback.tick()?;
        library.songs.get(s)?.active_clips_at(&library.patterns, tick, TICK).into_iter().next()
    }

    /// While a song plays, the grid and the piano roll show the pattern it
    /// plays now; called every frame (`Session::update`).
    pub(super) fn follow_song_pattern(&mut self) {
        if !matches!(self.workspace, Workspace::Grid | Workspace::PianoRoll) {
            return;
        }
        let Some(clip) = self.song_clip_playing().filter(|clip| clip.pattern != self.pattern_index) else { return };
        self.pattern_index = clip.pattern;
        self.deselect();
        self.chrome.beat_window.rewind();
    }
}
