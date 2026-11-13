//! The tempo: the one that plays, the one the header sets, and tap tempo,
//! where it follows the time between taps.

use super::Session;
use ordrumbox_engine::Mode;
use std::time::{Duration, Instant};

/// Taps older than this start a new count.
const TAP_RESET: Duration = Duration::from_secs(2);
/// Taps the average is made of, at most.
const TAP_WINDOW: usize = 8;

impl Session {
    /// T: a tap. From the second tap on, the tempo is the one the gaps between
    /// the latest taps say (one edit, like the tempo knob), kept inside the
    /// tempo bounds with the value used shown.
    pub fn tap_tempo(&mut self) {
        self.tap_tempo_at(Instant::now());
    }

    pub(super) fn tap_tempo_at(&mut self, now: Instant) {
        if self.host.hosted {
            self.message = "the tempo is the host's".into();
            return;
        }
        if self.transport.taps.last().is_some_and(|&last| now.duration_since(last) > TAP_RESET) {
            self.transport.taps.clear();
        }
        self.transport.taps.push(now);
        if self.transport.taps.len() > TAP_WINDOW {
            self.transport.taps.remove(0);
        }
        let (Some(&first), Some(&last)) = (self.transport.taps.first(), self.transport.taps.last()) else { return };
        let gaps = self.transport.taps.len() - 1;
        if gaps == 0 {
            self.message = "tap tempo: tap again".into();
            return;
        }
        let beat_seconds = last.duration_since(first).as_secs_f64() / gaps as f64;
        if beat_seconds <= 0.0 {
            return;
        }
        let asked = (60.0 / beat_seconds).round();
        self.set_tempo(asked);
        let used = self.tempo();
        self.message = if (used - asked).abs() < 0.5 {
            format!("tap tempo: {} BPM", ordrumbox_core::num::text(used))
        } else {
            format!("tap tempo: {asked} BPM asked, {} BPM is the limit", ordrumbox_core::num::text(used))
        };
    }

    /// F9: the click on every beat, on or off.
    pub fn toggle_metronome(&mut self) {
        self.transport.metronome = !self.transport.metronome;
        self.message = if self.transport.metronome { "metronome on" } else { "metronome off" }.into();
    }
}

impl Session {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_taps_set_the_tempo_and_a_pause_starts_over() {
        let mut s = Session::for_tests(ordrumbox_core::Library::default(), false);
        let start = Instant::now();
        let beat = Duration::from_millis(500);
        s.tap_tempo_at(start);
        assert_eq!(s.message(), "tap tempo: tap again");
        s.tap_tempo_at(start + beat);
        s.tap_tempo_at(start + beat * 2);
        assert!((s.tempo() - 120.0).abs() < 0.5, "{}", s.tempo());
        // A long pause: this tap is the first of a new count and leaves the tempo alone.
        s.tap_tempo_at(start + beat * 2 + TAP_RESET * 2);
        assert!((s.tempo() - 120.0).abs() < 0.5);
        assert_eq!(s.message(), "tap tempo: tap again");
    }
}
