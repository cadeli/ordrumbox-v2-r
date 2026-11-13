//! What plays and what is shown: play and stop, the pattern, kit and song
//! stepped through or picked at random, the tempo, and the choices a plugin
//! host makes.

use super::Session;
use super::session_types::Workspace;
use crate::menu;
use ordrumbox_engine::Mode;

/// Where and how the session plays, beside the engine's own state.
#[derive(Debug, Default)]
pub(super) struct Transport {
    /// The measure the song plays from.
    pub(super) song_cursor: u32,
    /// The measures a song plays in a loop: its index, first measure, end (excluded).
    pub(super) song_loop: Option<(usize, u32, u32)>,
    /// A click on every beat.
    pub(super) metronome: bool,
    /// The row keys put notes on the step the pattern plays (`session_record.rs`).
    pub(super) recording: bool,
    /// When tempo was tapped, oldest first (`session_tempo.rs`).
    pub(super) taps: Vec<std::time::Instant>,
}

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
        if self.host.hosted {
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
            // A song stops where it is: Space goes on from this measure, Home goes back.
            if matches!(mode, Mode::Song(_)) && self.song_playing() {
                self.transport.song_cursor = self.playhead_measure();
                self.message =
                    format!("paused at measure {}, Space goes on, Home rewinds", self.transport.song_cursor + 1);
            }
            self.playback.stop();
        } else {
            if let Mode::Pattern(p) = mode {
                self.generators.auto_loop(&mut self.editor, p, 0);
            }
            self.playback.play_from(&self.editor, mode, first);
        }
    }

    /// Home: back to the start. A song goes to its first measure, or its
    /// loop's, and goes on from there if it plays; a pattern plays from its
    /// first beat again if it plays.
    pub fn rewind(&mut self) {
        if self.workspace == Workspace::Song {
            let first = self.song_loop_region().map_or(0, |(first, _)| first);
            self.set_song_cursor(first);
            self.message = format!("back to measure {}", first + 1);
        } else if self.playback.is_playing() {
            self.playback.play_from(&self.editor, Mode::Pattern(self.pattern_index), 0);
            self.message = "back to the first beat".into();
        } else {
            self.message = "stopped at the first beat".into();
        }
    }

    /// The measures the selected song plays in a loop, `first..end`.
    pub(super) fn song_loop_region(&self) -> Option<(u32, u32)> {
        let (song, first, end) = self.transport.song_loop?;
        (song == self.editor.selected_song() && first < end).then_some((first, end))
    }

    /// Shift+click on the ruler: the song loops from the measure the cursor
    /// is on to measure `measure`, both included.
    pub(super) fn loop_to(&mut self, measure: u32) {
        let from = self.playhead_measure();
        let (first, last) = (from.min(measure), from.max(measure));
        self.transport.song_loop = Some((self.editor.selected_song(), first, last + 1));
        self.message = if first == last {
            format!("loop: measure {}, Play > Clear the loop ends it", first + 1)
        } else {
            format!("loop: measures {}-{}, Play > Clear the loop ends it", first + 1, last + 1)
        };
    }

    /// Play > Clear the loop: the song plays on to its end.
    pub fn clear_loop(&mut self) {
        if self.transport.song_loop.take().is_some() {
            self.message = "loop cleared".into();
        } else {
            self.message = "no loop: Shift+click the song's ruler sets one".into();
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

    /// A plugin host's swing on every track: where the second step of each
    /// pair starts (`ordrumbox_core::track::STRAIGHT_SWING`: none). The pass
    /// playing is rebuilt when it changes.
    pub fn set_host_swing(&mut self, swing: f64) {
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_host_swing(self.editor.library(), swing);
        }
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
        if let Some(sample) = self.kit_editor.sample {
            self.kit_editor.sample = Some(sample.min(kit.samples.len().saturating_sub(1)));
        }
        self.update_analysis();
    }

    /// F: a random pattern.
    pub fn select_random_pattern(&mut self) {
        let count = self.editor.library().patterns.len();
        if count == 0 {
            return;
        }
        let index = self.generators.rng.below(count as u64) as usize;
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
        let index = self.generators.rng.below(count as u64) as usize;
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
