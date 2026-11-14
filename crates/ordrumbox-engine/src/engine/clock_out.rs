//! The MIDI clock an engine sends: Start, Continue or Stop with the
//! transport, and 24 pulses a beat while a pass plays. Off unless asked for
//! (a plugin host has its own clock). Every message is timed in engine clock
//! frames; [`ClockSender::take`] gives them to the audio callback block by block.

use ordrumbox_core::steps::TICK;

use crate::midi::{BlockClock, ClockEvent, ClockMessage};
use crate::pass::Pass;

/// Messages one block can hold; more are not sent.
pub const CLOCK_OUT: usize = 512;

/// Pulses in a beat.
const PULSES_PER_BEAT: u32 = 24;
const _: () = assert!(TICK.is_multiple_of(PULSES_PER_BEAT), "a pulse falls on a tick");
/// Ticks between two pulses.
const PULSE_TICKS: u32 = TICK / PULSES_PER_BEAT;
/// Ticks in a song position unit (a sixteenth note: 6 pulses).
const POSITION_TICKS: u64 = PULSE_TICKS as u64 * 6;

/// The clock being sent, and the next pulse of the pass playing.
#[derive(Debug)]
pub(super) struct ClockSender {
    enabled: bool,
    /// The next pulse of the pass playing, counted from its first frame.
    next_pulse: u64,
    queue: Vec<ClockEvent>,
}

impl ClockSender {
    pub(super) fn new() -> Self {
        Self { enabled: false, next_pulse: 0, queue: Vec::with_capacity(CLOCK_OUT) }
    }

    /// Send the clock, or stop sending it (what is waiting is dropped).
    pub(super) fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.queue.clear();
        }
    }

    fn push(&mut self, clock: u64, message: ClockMessage) {
        if self.enabled && self.queue.len() < self.queue.capacity() {
            self.queue.push(ClockEvent { clock, message });
        }
    }

    /// A pass starts the transport at `clock`: Start from the very beginning,
    /// else the song position and Continue.
    pub(super) fn started(&mut self, clock: u64, pass: &Pass) {
        self.next_pulse = 0;
        if pass.first_tick == 0 {
            self.push(clock, ClockMessage::Start);
        } else {
            let position = u16::try_from(pass.first_tick / POSITION_TICKS).unwrap_or(u16::MAX);
            self.push(clock, ClockMessage::Position(position));
            self.push(clock, ClockMessage::Continue);
        }
    }

    pub(super) fn stopped(&mut self, clock: u64) {
        self.push(clock, ClockMessage::Stop);
    }

    /// A pass starts at its first frame.
    pub(super) fn restart(&mut self) {
        self.next_pulse = 0;
    }

    /// Playback goes on from `frame` of `pass`: the pulses before it are not sent.
    pub(super) fn seek(&mut self, pass: &Pass, frame: u64) {
        let pulses = pulse_count(pass);
        self.next_pulse = if pass.frames == 0 {
            0
        } else {
            u64::try_from((u128::from(frame) * u128::from(pulses)).div_ceil(u128::from(pass.frames))).unwrap_or(pulses)
        };
    }

    /// Send the pulses due at `frame` of `pass` (clock frame `now`); the
    /// frame of the next one, if the pass has one left.
    pub(super) fn due(&mut self, pass: &Pass, frame: u64, now: u64) -> Option<u64> {
        if !self.enabled {
            return None;
        }
        loop {
            let at = pulse_frame(pass, self.next_pulse)?;
            if at > frame {
                return Some(at);
            }
            self.push(now, ClockMessage::Pulse);
            self.next_pulse += 1;
        }
    }

    /// Give the messages queued since the last call to `send`, in the order
    /// they were queued, timed from clock frame `block_start` up to `block_frames` frames.
    pub(super) fn take(&mut self, block_start: u64, block_frames: usize, mut send: impl FnMut(BlockClock)) {
        let last = u32::try_from(block_frames.saturating_sub(1)).unwrap_or(u32::MAX);
        for event in self.queue.drain(..) {
            let timing = u32::try_from(event.clock.saturating_sub(block_start)).unwrap_or(u32::MAX).min(last);
            send(BlockClock { timing, message: event.message });
        }
    }
}

/// Pulses in `pass`.
fn pulse_count(pass: &Pass) -> u64 {
    u64::from(pass.ticks / PULSE_TICKS)
}

/// The frame pulse `index` of `pass` falls on; `None` past the last one.
fn pulse_frame(pass: &Pass, index: u64) -> Option<u64> {
    let pulses = pulse_count(pass);
    if index >= pulses {
        return None;
    }
    let frame = (u128::from(index) * u128::from(pass.frames)).div_ceil(u128::from(pulses));
    u64::try_from(frame).ok().filter(|&frame| frame < pass.frames)
}
