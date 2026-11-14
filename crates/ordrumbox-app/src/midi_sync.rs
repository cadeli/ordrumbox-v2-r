//! The MIDI clock followed: the MIDI thread feeds a [`ClockFollower`] with the
//! pulses, starts and stops of the input; the audio callback asks it where
//! the transport is and renders to it ([`AudioSide::render_synced`]), as under
//! a plugin host. Shared through statics like the notes ([`crate::midi::NOTES`]):
//! the audio callback has no way to be handed them. The callback only
//! `try_lock`s: a pulse being written is a block that keeps the last transport.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::Instant;

use ordrumbox_engine::{ClockFollower, ClockMessage, HostTransport};

/// The transport follows the clock of the input.
static FOLLOW: AtomicBool = AtomicBool::new(false);
static FOLLOWER: Mutex<ClockFollower> = Mutex::new(ClockFollower::new());
static EPOCH: OnceLock<Instant> = OnceLock::new();

/// The tempo until two pulses have given one.
const FALLBACK_BPM: f64 = 120.0;

/// Seconds on the clock both threads time the pulses by (started on its first call).
pub fn seconds() -> f64 {
    EPOCH.get_or_init(Instant::now).elapsed().as_secs_f64()
}

/// Follow the clock of the input, or do not any more.
pub fn set_following(on: bool) {
    FOLLOW.store(on, Ordering::Relaxed);
    if !on {
        *lock() = ClockFollower::new();
    }
}

pub fn following() -> bool {
    FOLLOW.load(Ordering::Relaxed)
}

fn lock() -> MutexGuard<'static, ClockFollower> {
    FOLLOWER.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A timing message came from the input (MIDI thread). `Some(true)` for a
/// start, `Some(false)` for a stop: the UI is told; none when the clock is not followed.
pub fn input(message: ClockMessage) -> Option<bool> {
    if !following() {
        return None;
    }
    lock().message(message, seconds());
    match message {
        ClockMessage::Start | ClockMessage::Continue => Some(true),
        ClockMessage::Stop => Some(false),
        ClockMessage::Pulse | ClockMessage::Position(_) => None,
    }
}

/// The transport `ahead` seconds from now (audio callback); `None` while
/// the MIDI thread holds the follower.
pub fn transport(ahead: f64) -> Option<HostTransport> {
    let mut follower = match FOLLOWER.try_lock() {
        Ok(follower) => follower,
        Err(TryLockError::WouldBlock) => return None,
        Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
    };
    Some(follower.transport(seconds() + ahead, FALLBACK_BPM))
}
