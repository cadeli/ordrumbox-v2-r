//! What the two sides share: the inbox the controller posts into, the scope and
//! the published position.

use crate::engine::{HostMix, HostTransport, Position};
use crate::kit::SampleBank;
use crate::midi::KeyMap;
use crate::pass::Pass;
use ordrumbox_dsp::MasterSettings;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

#[derive(Debug)]
pub(super) enum Command {
    Play(Arc<Pass>),
    /// A rebuild of the playing pass.
    Replace(Arc<Pass>),
    Stop,
}

#[derive(Debug, Default)]
pub(super) struct Inbox {
    pub(super) command: Option<Command>,
    pub(super) next: Option<Arc<Pass>>,
    pub(super) bank: Option<Arc<SampleBank>>,
    pub(super) master: Option<MasterSettings>,
    /// A live mix on top of the tracks' (a MIDI controller's knobs).
    pub(super) mix: Option<HostMix>,
    pub(super) preview: Option<Arc<Pass>>,
    pub(super) keymap: Option<Arc<KeyMap>>,
    /// Under a stopped plugin host: the pass it would start in, for the
    /// engine to start on the block the host starts ([`Controller::update`]).
    pub(super) cue: Option<Arc<Pass>>,
}

/// Samples the scope keeps (the JS analyser's `fftSize`).
pub const SCOPE_SIZE: usize = 4096;

/// The last samples played, mono, as a ring (the analyser's input).
#[derive(Debug)]
pub(super) struct Scope {
    pub(super) samples: Vec<f32>,
    /// Where the next sample goes.
    pub(super) at: usize,
}

impl Default for Scope {
    fn default() -> Self {
        Self { samples: vec![0.0; SCOPE_SIZE], at: 0 }
    }
}

/// What the engine publishes after each callback, read as one snapshot: a
/// seqlock, odd `seq` while the audio side writes. The audio side never waits;
/// the UI reads again when a write was under way.
#[derive(Debug, Default)]
pub(super) struct Published {
    pub(super) seq: AtomicU64,
    pub(super) playing: AtomicBool,
    pub(super) pass: AtomicU64,
    pub(super) frame: AtomicU64,
    pub(super) frames: AtomicU64,
    /// Pass index the engine needs, plus one (0: none).
    pub(super) wants: AtomicU64,
    /// Whether the last callback followed a plugin host's transport, and that transport.
    pub(super) synced: AtomicBool,
    pub(super) host_playing: AtomicBool,
    pub(super) host_bpm: AtomicU64,
    pub(super) host_beat: AtomicU64,
    /// The host was in a pass the engine does not play.
    pub(super) lost: AtomicBool,
}

/// One callback's published state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Snapshot {
    /// `None` when stopped.
    pub(super) position: Option<Position>,
    /// Pass index the engine needs, plus one (0: none).
    pub(super) wants: u64,
    /// The host's transport, when the callback followed one.
    pub(super) host: Option<HostTransport>,
    /// The host was in a pass the engine does not play.
    pub(super) lost: bool,
}

impl Published {
    /// Audio side, the only writer.
    pub(super) fn write(&self, snapshot: Snapshot) {
        let Snapshot { position, wants, host, lost } = snapshot;
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.playing.store(position.is_some(), Ordering::Relaxed);
        if let Some(Position { pass, frame, frames }) = position {
            self.pass.store(pass, Ordering::Relaxed);
            self.frame.store(frame, Ordering::Relaxed);
            self.frames.store(frames, Ordering::Relaxed);
        }
        self.wants.store(wants, Ordering::Relaxed);
        self.synced.store(host.is_some(), Ordering::Relaxed);
        if let Some(HostTransport { playing, bpm, beat }) = host {
            self.host_playing.store(playing, Ordering::Relaxed);
            self.host_bpm.store(bpm.to_bits(), Ordering::Relaxed);
            self.host_beat.store(beat.to_bits(), Ordering::Relaxed);
        }
        self.lost.store(lost, Ordering::Relaxed);
        self.seq.store(seq + 2, Ordering::Release);
    }

    /// What one callback published.
    pub(super) fn read(&self) -> Snapshot {
        loop {
            let before = self.seq.load(Ordering::Acquire);
            if before.is_multiple_of(2) {
                let playing = self.playing.load(Ordering::Relaxed);
                let position = Position {
                    pass: self.pass.load(Ordering::Relaxed),
                    frame: self.frame.load(Ordering::Relaxed),
                    frames: self.frames.load(Ordering::Relaxed),
                };
                let wants = self.wants.load(Ordering::Relaxed);
                let host = self.synced.load(Ordering::Relaxed).then(|| HostTransport {
                    playing: self.host_playing.load(Ordering::Relaxed),
                    bpm: f64::from_bits(self.host_bpm.load(Ordering::Relaxed)),
                    beat: f64::from_bits(self.host_beat.load(Ordering::Relaxed)),
                });
                let lost = self.lost.load(Ordering::Relaxed);
                fence(Ordering::Acquire);
                if self.seq.load(Ordering::Relaxed) == before {
                    return Snapshot { position: playing.then_some(position), wants, host, lost };
                }
            }
            std::hint::spin_loop();
        }
    }
}

/// State shared by both sides.
#[derive(Debug, Default)]
pub(super) struct Link {
    pub(super) inbox: Mutex<Inbox>,
    /// Copied from the audio side's own ring when it can take the lock.
    pub(super) scope: Mutex<Scope>,
    pub(super) levels: Mutex<Vec<f32>>,
    pub(super) published: Published,
}

/// The lock, whether or not a thread panicked holding it: the values it
/// guards are plain data, valid at any time.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The lock if it is free (the audio side never waits), poisoned or not.
pub(super) fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// Drop the references only `list` still holds; the newest is kept when
/// `keep_last` (the next pass follows it, the engine plays the last keymap).
pub(super) fn release<T>(list: &mut VecDeque<Arc<T>>, keep_last: bool) {
    let last = list.len().saturating_sub(1);
    let mut i = 0;
    list.retain(|pass| {
        let keep = (keep_last && i == last) || Arc::strong_count(pass) > 1;
        i += 1;
        keep
    });
}
