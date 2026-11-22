//! The controller following a plugin host's transport (or a MIDI clock):
//! its tempo, its start and stop, the pass it jumps to, and the cue it is
//! given while stopped.

use ordrumbox_core::Library;
use ordrumbox_core::song::clamp_bpm;

use super::link::{Command, Snapshot, lock};
use super::{Controller, CueKey, Mode, TEMPO_EPSILON};
use crate::engine::{HostTransport, host_pass};
use crate::pass::{SONG_PASS_TICKS, pattern_pass_ticks};

impl Controller {
    /// The plugin host's transport seen by the last callback, if it followed one.
    pub fn host(&self) -> Option<HostTransport> {
        self.link.published.read().host
    }

    /// Ticks in one pass of what plays.
    fn pass_ticks(&self, library: &Library) -> Option<u32> {
        match self.mode {
            Mode::Pattern(p) => library.patterns.get(p).map(pattern_pass_ticks),
            Mode::Song(s) => library.songs.get(s).map(|_| SONG_PASS_TICKS),
        }
    }

    /// Follow the plugin host's transport: its tempo for every pass, start
    /// and stop with it, and the pass it is in when it jumps (a loop, a
    /// click on its timeline). Stopped, the host is given a cue: the pass it
    /// would start in, which the engine starts on its own.
    pub(super) fn follow_host(&mut self, library: &Library, host: HostTransport, snapshot: Snapshot) {
        let bpm = clamp_bpm(host.bpm);
        let new_tempo = self.tempo.is_none_or(|tempo| (tempo - bpm).abs() > TEMPO_EPSILON);
        if new_tempo {
            self.tempo = Some(bpm);
        }
        if !host.rolling() {
            if self.playing {
                self.stop();
                // The next play assigns the sounds afresh.
                self.assigned.clear();
                self.cue = None;
            }
            self.post_cue(library, host.beat);
            return;
        }
        // A Play still waiting is the host's pass already.
        if matches!(lock(&self.link.inbox).command, Some(Command::Play(_))) {
            return;
        }
        let cued = self.cue.is_some_and(|cue| snapshot.position.is_some_and(|at| at.pass == cue.number));
        if !self.playing && !snapshot.lost && cued {
            // The engine started from the cue: it plays on, at the tempo now.
            self.playing = true;
            self.cue = None;
            if new_tempo {
                self.refresh(library);
            }
        } else if !self.playing || snapshot.lost {
            let Some((number, _)) = self.pass_ticks(library).and_then(|ticks| host_pass(host.beat, ticks)) else {
                return;
            };
            if !self.playing {
                self.assigned.clear();
            }
            self.jump(library, number);
        } else if new_tempo {
            self.refresh(library);
        }
    }

    /// The transport followed a clock outside (a MIDI clock) and does no
    /// more: the library's tempo plays again, and the cue is dropped. The
    /// blocks rendered while it was followed are not followed any more
    /// ([`Self::follow_clock_again`] follows them again).
    pub fn leave_host(&mut self, library: &Library) {
        self.host_left = true;
        self.release_host(library);
    }

    /// The transport follows a clock outside again ([`AudioSide::render_synced`]).
    pub fn follow_clock_again(&mut self) {
        self.host_left = false;
    }

    pub(super) fn release_host(&mut self, library: &Library) {
        self.tempo = None;
        lock(&self.link.inbox).cue = None;
        self.refresh(library);
    }

    /// Under a stopped host at `beat`: the pass it would start in posted as
    /// the cue, unless the one posted is still there and built from the same.
    fn post_cue(&mut self, library: &Library, beat: f64) {
        let Some((number, _)) = self.pass_ticks(library).and_then(|ticks| host_pass(beat, ticks)) else { return };
        let key = CueKey { number, mode: self.mode, tempo: self.tempo, swing: self.swing };
        // The engine took a cue when the host started and stopped before this thread saw it.
        if self.cue == Some(key) && lock(&self.link.inbox).cue.is_some() {
            return;
        }
        self.song_notes.clear();
        self.pass_patterns.clear();
        let Some(pass) = self.build(library, number) else { return };
        lock(&self.link.inbox).cue = Some(pass);
        self.cue = Some(key);
    }
}
