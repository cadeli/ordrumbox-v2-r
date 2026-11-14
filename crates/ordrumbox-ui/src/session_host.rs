//! The plugin host a session follows: whether there is one, and what of its
//! transport and mutes the window shows. And, in the app, the MIDI clock of
//! another machine, followed as a host's transport is.

use super::Session;
use crate::view;

/// What the session knows of its plugin host.
#[derive(Debug, Default)]
pub(super) struct Host {
    /// The library lives in a plugin host's project, not in a file.
    pub(super) hosted: bool,
    /// The tracks the host mutes, bit `t` for track `t`.
    pub(super) muted: u64,
    /// The host's tempo last said to be outside the tempo bounds
    /// (`report_host_tempo`), so the message is given once.
    tempo_outside: Option<f64>,
    /// The host's transport as the header shows it (`watch_host`).
    pub(super) shown: Option<view::HostState>,
}

impl Session {
    /// Say once when the host's tempo is outside the tempo bounds: a plugin
    /// plays at the nearest bound instead, the tempo its header shows.
    pub(super) fn report_host_tempo(&mut self) {
        use ordrumbox_core::num;
        use ordrumbox_core::song::{MAX_BPM, MIN_BPM, clamp_bpm};
        let outside = self.playback.host().map(|host| host.bpm).filter(|bpm| !(MIN_BPM..=MAX_BPM).contains(bpm));
        if outside == self.host.tempo_outside {
            return;
        }
        self.host.tempo_outside = outside;
        if let Some(bpm) = outside {
            let (host, used) = (num::text(bpm), num::text(clamp_bpm(bpm)));
            self.message = format!("host tempo {host} outside {MIN_BPM}–{MAX_BPM}: plays at {used}");
        }
    }

    /// The tracks a plugin host mutes, bit `t` for track `t`, shown on their
    /// mute boxes; true when they changed.
    pub fn set_host_muted(&mut self, muted: u64) -> bool {
        std::mem::replace(&mut self.host.muted, muted) != muted
    }

    /// Take what the plugin host's transport does now, for the header:
    /// whether it plays, and the tempo played, to a tenth. True when what
    /// the header shows changed.
    pub fn watch_host(&mut self) -> bool {
        use ordrumbox_core::{num, song::clamp_bpm};
        let shown = self
            .playback
            .host()
            .map(|host| view::HostState { playing: host.playing, bpm: num::round_to(clamp_bpm(host.bpm), 1) });
        std::mem::replace(&mut self.host.shown, shown) != shown
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

    /// The session is a plugin's: the host plays, saves and has the keys it
    /// leaves out of the menus.
    pub fn hosted(&self) -> bool {
        self.host.hosted
    }
}
