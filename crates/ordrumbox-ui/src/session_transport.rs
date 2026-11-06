//! What plays and what is shown: play and stop, the pattern, kit and song
//! stepped through or picked at random, the tempo, and the choices a plugin
//! host makes.

use super::*;

impl Session {
    pub fn playing(&self) -> bool {
        self.playback.is_playing()
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
    pub(super) fn toggle(&mut self, mode: Mode, first: u64) {
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

    pub fn step_pattern(&mut self, delta: isize) {
        let count = self.editor.library().patterns.len();
        if count > 0 {
            self.show_pattern((self.pattern_index as isize + delta).rem_euclid(count as isize) as usize);
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
    pub(super) fn use_kit(&mut self, index: usize) {
        self.select_kit(index);
        if let Some(kit) = self.kits.kits.get(index) {
            self.message = format!("kit {}", kit.name);
        }
    }

    /// Use kit `index` (its sounds are assigned again).
    pub(super) fn select_kit(&mut self, index: usize) {
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

    pub fn step_tempo(&mut self, delta: f64) {
        self.set_tempo(self.tempo() + delta);
    }

    /// The song the header's tempo is: the selected one in the song panel
    /// (what Space plays there), else none (the library's tempo).
    pub(super) fn tempo_song(&self) -> Option<usize> {
        (self.workspace == Workspace::Song && self.song().is_some()).then(|| self.editor.selected_song())
    }

    /// The tempo the header shows and turns: the selected song's in the
    /// song panel, else the library's, which every pattern plays at.
    pub fn tempo(&self) -> f64 {
        self.tempo_song()
            .and_then(|s| self.editor.library().songs.get(s))
            .map_or(self.editor.library().bpm, |song| song.bpm)
    }
}
