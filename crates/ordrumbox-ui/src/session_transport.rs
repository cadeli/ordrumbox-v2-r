//! What plays and what is shown: play and stop, the pattern, kit and song
//! stepped through or picked at random, the tempo, and the choices a plugin
//! host makes.

use super::Session;
use crate::menu;
use ordrumbox_core::automation::STEPS_PER_MEASURE;

/// What a loop command says without a loop.
const NO_LOOP: &str = "no loop: a drag on the song's ruler sets one";
use ordrumbox_engine::Mode;

/// Where and how the session plays, beside the engine's own state.
#[derive(Debug, Default)]
pub(super) struct Transport {
    /// The measure the song plays from.
    pub(super) song_cursor: u32,
    /// The steps a song plays in a loop: its index, first step, end (excluded).
    pub(super) song_loop: Option<(usize, u32, u32)>,
    /// The loop plays; off, its ends stay on the ruler, for Ctrl+L to turn it on again.
    pub(super) is_loop_on: bool,
    /// A click on every beat.
    pub(super) metronome: bool,
    /// The row keys put notes on the step the pattern plays (`session_record.rs`).
    pub(super) recording: bool,
    /// Recorded notes keep how early or late they were played, as their shift.
    pub(super) record_timing: bool,
    /// When tempo was tapped, oldest first (`session_tempo.rs`).
    pub(super) taps: Vec<std::time::Instant>,
}

impl Session {
    pub fn playing(&self) -> bool {
        self.playback.is_playing()
    }

    // ─── Transport ───────────────────────────────────────────────────────────

    /// Space: play or stop the selected pattern (in song mode: the song).
    pub fn toggle_play(&mut self) {
        if self.is_song_mode() {
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
        if self.is_song_mode() {
            let first = self.song_loop_region().map_or(0, |(first, _)| first / STEPS_PER_MEASURE);
            self.set_song_cursor(first);
            self.message = format!("back to measure {}", first + 1);
        } else if self.playback.is_playing() {
            self.playback.play_from(&self.editor, Mode::Pattern(self.pattern_index), 0);
            self.message = "back to the first beat".into();
        } else {
            self.message = "stopped at the first beat".into();
        }
    }

    /// The steps the selected song plays in a loop, `first..end`; `None`
    /// without a loop or with it off.
    pub(super) fn song_loop_region(&self) -> Option<(u32, u32)> {
        self.song_loop_marks().filter(|_| self.transport.is_loop_on)
    }

    /// The selected song's loop, `first..end`, on or off.
    pub(super) fn song_loop_marks(&self) -> Option<(u32, u32)> {
        let (song, first, end) = self.transport.song_loop?;
        (song == self.editor.selected_song() && first < end).then_some((first, end))
    }

    /// Shift+click on the ruler: the song loops from the measure the cursor
    /// is on to measure `measure`, both included.
    pub(super) fn loop_to(&mut self, measure: u32) {
        let from = self.playhead_measure();
        let (first, last) = (from.min(measure), from.max(measure));
        self.set_song_loop(first * STEPS_PER_MEASURE, (last + 1) * STEPS_PER_MEASURE);
    }

    /// The selected song loops over steps `first..end`, the loop on, said
    /// as measures and steps (`2` for a measure's start, `2.5` for its
    /// fifth step).
    pub(super) fn set_song_loop(&mut self, first: u32, end: u32) {
        self.transport.is_loop_on = true;
        self.move_song_loop(first, end);
    }

    /// The selected song's loop moved to steps `first..end`, on or off as it was.
    pub(super) fn move_song_loop(&mut self, first: u32, end: u32) {
        self.transport.song_loop = Some((self.editor.selected_song(), first, end));
        let at = |step: u32| match step % STEPS_PER_MEASURE {
            0 => (step / STEPS_PER_MEASURE + 1).to_string(),
            in_measure => format!("{}.{}", step / STEPS_PER_MEASURE + 1, in_measure + 1),
        };
        let state = if self.transport.is_loop_on { "Ctrl+L turns it off" } else { "off, Ctrl+L turns it on" };
        self.message = format!("loop: {} to {}, {state}", at(first), at(end));
    }

    /// Ctrl+L: the song's loop on or off, its ends kept.
    pub fn toggle_loop(&mut self) {
        if self.song_loop_marks().is_none() {
            self.message = NO_LOOP.into();
            return;
        }
        self.transport.is_loop_on = !self.transport.is_loop_on;
        self.message = (if self.transport.is_loop_on { "loop on" } else { "loop off" }).into();
    }

    /// Play > Clear the loop: the song plays on to its end, the loop's ends removed.
    pub fn clear_loop(&mut self) {
        if self.transport.song_loop.take().is_some() {
            self.message = "loop cleared".into();
        } else {
            self.message = NO_LOOP.into();
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
        let count = self.kits.kits().len();
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

    /// Pattern `index` (from 0) chosen by a MIDI note: as [`Self::host_select_pattern`],
    /// and a pattern playing is left at once for the new one, from its first
    /// beat (under a plugin host, from the host's place in the pass).
    pub fn select_pattern_now(&mut self, index: usize) -> bool {
        let chosen = self.host_select_pattern(index);
        if chosen {
            self.playback.restart_pattern(&self.editor, index);
        }
        chosen
    }

    /// Kit `index` (from 0) used, as a plugin host's parameter asks. False,
    /// with a message, when there is no such kit.
    pub fn host_select_kit(&mut self, index: usize) -> bool {
        let count = self.kits.kits().len();
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
        self.kits.kits().iter().map(|k| k.name.as_str())
    }

    /// Use kit `index` and say so.
    pub(super) fn use_kit(&mut self, index: usize) {
        self.select_kit(index);
        if let Some(kit) = self.kits.kits().get(index) {
            self.message = format!("kit {}", kit.name);
        }
    }

    /// Use kit `index` (its sounds are assigned again).
    pub(super) fn select_kit(&mut self, index: usize) {
        let Some(kit) = self.kits.kits().get(index) else { return };
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
        let count = self.kits.kits().len();
        if count == 0 {
            return;
        }
        let index = self.generators.rng.below(count as u64) as usize;
        self.use_kit(index);
    }

    pub fn step_tempo(&mut self, delta: f64) {
        self.set_tempo(self.tempo() + delta);
    }

    /// The song the header's tempo is: the selected one in song mode
    /// (what Space plays then), else none (the library's tempo).
    pub(super) fn tempo_song(&self) -> Option<usize> {
        (self.is_song_mode() && self.song().is_some()).then(|| self.editor.selected_song())
    }

    /// The tempo the header shows and turns: the selected song's in song
    /// mode, else the library's, which every pattern plays at.
    pub fn tempo(&self) -> f64 {
        self.tempo_song()
            .and_then(|s| self.editor.library().songs.get(s))
            .map_or(self.editor.library().bpm, |song| song.bpm)
    }
}

impl Session {
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
}
