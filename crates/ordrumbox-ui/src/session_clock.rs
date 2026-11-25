//! What the session follows from outside: the MIDI clock of another
//! machine, which plays the transport as a host's would, and the tracks the
//! MIDI controls mute.

use super::Session;

/// What the session knows of the clock it follows and of the MIDI controls.
#[derive(Debug, Default)]
pub(super) struct Followed {
    /// The tracks the MIDI controls mute, bit `t` for track `t`.
    pub(super) muted: u64,
    /// The clock's tempo last said to be outside the tempo bounds
    /// (`report_clock_tempo`), so the message is given once.
    tempo_outside: Option<f64>,
}

impl Session {
    /// Say once when the followed clock's tempo is outside the tempo bounds:
    /// the transport plays at the nearest bound instead.
    pub(super) fn report_clock_tempo(&mut self) {
        use ordrumbox_core::num;
        use ordrumbox_core::song::{MAX_BPM, MIN_BPM, clamp_bpm};
        let outside = self.playback.host().map(|host| host.bpm).filter(|bpm| !(MIN_BPM..=MAX_BPM).contains(bpm));
        if outside == self.followed.tempo_outside {
            return;
        }
        self.followed.tempo_outside = outside;
        if let Some(bpm) = outside {
            let (clock, used) = (num::text(bpm), num::text(clamp_bpm(bpm)));
            self.message = format!("clock tempo {clock} outside {MIN_BPM}–{MAX_BPM}: plays at {used}");
        }
    }

    /// The tracks the MIDI controls mute, bit `t` for track `t`, shown on
    /// their mute boxes; true when they changed.
    pub(super) fn set_live_muted(&mut self, muted: u64) -> bool {
        std::mem::replace(&mut self.followed.muted, muted) != muted
    }
}

impl Session {
    /// The program follows the MIDI clock of its input, or does not any more:
    /// the machine whose clock it is starts, stops and sets the tempo of the
    /// transport (`Ui::idle` keeps the engine fed while it does).
    pub fn set_clock_follow(&mut self, on: bool) {
        let offers = self.playback.offers_mut();
        if std::mem::replace(&mut offers.clock_follow, on) == on {
            return;
        }
        offers.clock_running = offers.clock_running && on;
        self.playback.set_clock_follow(&self.editor, on);
    }

    /// The machine whose clock is followed started or stopped.
    pub fn set_clock_running(&mut self, running: bool) {
        self.playback.offers_mut().clock_running = running;
    }

    /// The transport follows a MIDI clock.
    pub fn follows_clock(&self) -> bool {
        self.playback.offers().clock_follow
    }

    /// The transport follows a MIDI clock, and the machine plays.
    pub fn clock_running(&self) -> bool {
        self.playback.offers().clock_running
    }
}
