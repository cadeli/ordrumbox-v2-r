//! Tap tempo: the tempo follows the time between taps.

use super::Session;
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
        if self.hosted {
            self.message = "the tempo is the host's".into();
            return;
        }
        if self.taps.last().is_some_and(|&last| now.duration_since(last) > TAP_RESET) {
            self.taps.clear();
        }
        self.taps.push(now);
        if self.taps.len() > TAP_WINDOW {
            self.taps.remove(0);
        }
        let (Some(&first), Some(&last)) = (self.taps.first(), self.taps.last()) else { return };
        let gaps = self.taps.len() - 1;
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
        self.metronome = !self.metronome;
        self.message = if self.metronome { "metronome on" } else { "metronome off" }.into();
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
