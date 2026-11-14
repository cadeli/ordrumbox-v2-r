//! The MIDI output: the port the user chose (Edit > Preferences) through
//! `midir`. The notes the pattern plays go out on their channel and key (the
//! engine's `take_notes`, as under a plugin host), and, when asked for, the
//! MIDI clock (`take_clock`): pulses, Start, Continue and Stop.
//!
//! The audio callback writes each message with its place in the block to a
//! ring it never waits on; a thread of its own reads the ring every half
//! millisecond, holds each message until its time, and sends it, so that a
//! note lands where the block put it, not where the callback ran. The
//! messages are not delayed by the sound device's latency: a device that
//! plays late by a few milliseconds hears them a few milliseconds early.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use midir::{MidiOutput, MidiOutputConnection};
use ordrumbox_engine::AudioSide;
use ordrumbox_ui::MidiPort;

/// Messages the ring holds; past them the newest are lost.
const RING: usize = 1024;
/// Marks a slot as holding a message.
const FULL: u64 = 1 << 63;
/// How often the thread looks for messages and for those due.
const POLL: Duration = Duration::from_micros(500);

/// A message and when it is due, from the start of the callback that wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Timed {
    micros: u32,
    bytes: [u8; 3],
    len: usize,
}

/// A single-producer, single-consumer queue of timed messages: the audio
/// callback writes, the output thread reads. No lock, no allocation.
#[derive(Debug)]
struct Ring {
    slots: [AtomicU64; RING],
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl Ring {
    const fn new() -> Self {
        Self { slots: [const { AtomicU64::new(0) }; RING], head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    fn push(&self, message: Timed) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) >= RING {
            return false;
        }
        let [status, first, second] = message.bytes;
        let bytes = u64::from(status) | u64::from(first) << 8 | u64::from(second) << 16 | (message.len as u64) << 24;
        self.slots[tail % RING].store(FULL | u64::from(message.micros) << 26 | bytes, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    fn pop(&self) -> Option<Timed> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let word = self.slots[head % RING].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(Timed {
            micros: ((word >> 26) & u64::from(u32::MAX)) as u32,
            bytes: [word as u8, (word >> 8) as u8, (word >> 16) as u8],
            len: ((word >> 24) & 3) as usize,
        })
    }
}

/// What the audio callback feeds and the output thread reads.
static MESSAGES: Ring = Ring::new();
/// A port is open: the notes go to it.
static OPEN: AtomicBool = AtomicBool::new(false);
/// The clock goes to it too.
static CLOCK: AtomicBool = AtomicBool::new(false);

/// Send the clock through the output, or not.
pub fn set_clock(on: bool) {
    CLOCK.store(on, Ordering::Relaxed);
}

/// The names of the MIDI output ports, for Edit > Preferences.
pub fn ports() -> Vec<String> {
    let Ok(output) = MidiOutput::new("orDrumbox ports") else { return Vec::new() };
    output.ports().iter().filter_map(|port| output.port_name(port).ok()).collect()
}

/// An open MIDI output; dropping it closes the port.
pub struct Output {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// The send error that ended the thread (a device unplugged...), until the UI takes it.
    error: Arc<Mutex<Option<String>>>,
    pub name: String,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").field("name", &self.name).finish_non_exhaustive()
    }
}

impl Output {
    /// The error that closed the port since the last call, if any.
    pub fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        OPEN.store(false, Ordering::Relaxed);
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            // A thread that panicked has nothing left to say: its port is closed all the same.
            thread.join().ok();
        }
    }
}

/// Open the port `choice` names. `Ok(None)`: no port to open (none chosen,
/// or the one chosen is not connected).
pub fn open(choice: &MidiPort) -> Result<Option<Output>, String> {
    let output = MidiOutput::new("orDrumbox").map_err(|e| format!("MIDI out: {e}"))?;
    let ports = output.ports();
    let port = match choice {
        MidiPort::Off => return Ok(None),
        MidiPort::First => ports.first(),
        MidiPort::Named(wanted) => ports.iter().find(|p| output.port_name(p).is_ok_and(|name| name == *wanted)),
    };
    let Some(port) = port else { return Ok(None) };
    let name = output.port_name(port).map_err(|e| format!("MIDI out: {e}"))?;
    let connection = output.connect(port, "orDrumbox out").map_err(|e| format!("MIDI out {name}: {e}"))?;
    let (stop, error) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
    let thread = {
        let (stop, error) = (stop.clone(), error.clone());
        std::thread::Builder::new()
            .name("midi-out".into())
            .spawn(move || run(connection, &stop, &error))
            .map_err(|e| format!("MIDI out {name}: {e}"))?
    };
    // What the engine queued while no port was open is not for this one.
    while MESSAGES.pop().is_some() {}
    OPEN.store(true, Ordering::Relaxed);
    Ok(Some(Output { stop, thread: Some(thread), error, name }))
}

/// The thread: messages from the ring, each sent when its time comes.
fn run(mut connection: MidiOutputConnection, stop: &AtomicBool, error: &Mutex<Option<String>>) {
    let mut waiting: Vec<(Instant, Timed)> = Vec::with_capacity(RING);
    while !stop.load(Ordering::Relaxed) {
        let now = Instant::now();
        while let Some(message) = MESSAGES.pop() {
            waiting.push((now + Duration::from_micros(u64::from(message.micros)), message));
        }
        // Stable: two messages due together go out in the order they were written.
        waiting.sort_by_key(|(due, _)| *due);
        let due = waiting.partition_point(|(due, _)| *due <= now);
        for (_, message) in waiting.drain(..due) {
            if let Err(e) = connection.send(&message.bytes[..message.len]) {
                *error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(format!("MIDI out: {e}"));
                OPEN.store(false, Ordering::Relaxed);
                return;
            }
        }
        std::thread::sleep(POLL);
    }
}

/// The MIDI the block `audio` just rendered sent (audio callback): its notes,
/// and the clock when it is sent, `offset` frames after the start of the
/// callback, at `rate` frames a second. What no port is open for is dropped.
pub fn send_block(audio: &mut AudioSide, offset: usize, rate: u32) {
    let (open, clock) = (OPEN.load(Ordering::Relaxed), CLOCK.load(Ordering::Relaxed));
    let micros = |timing: u32| {
        let frames = offset as u64 + u64::from(timing);
        u32::try_from(frames * 1_000_000 / u64::from(rate.max(1))).unwrap_or(u32::MAX)
    };
    audio.take_notes(|note| {
        if !open {
            return;
        }
        // A note on at the lowest velocity the engine sends is 1: 0 would be a note off.
        let (status, velocity) = match note.velocity {
            Some(velocity) => (0x90, (velocity.clamp(0.0, 1.0) * 127.0).round().max(1.0) as u8),
            None => (0x80, 0),
        };
        let bytes = [status | (note.channel & 0x0F), note.key & 0x7F, velocity];
        MESSAGES.push(Timed { micros: micros(note.timing), bytes, len: 3 });
    });
    audio.take_clock(|event| {
        if open && clock {
            let (bytes, len) = event.message.bytes();
            MESSAGES.push(Timed { micros: micros(event.timing), bytes, len });
        }
    });
    audio.set_clock_out(open && clock);
}

#[cfg(test)]
mod tests {
    use ordrumbox_engine::ClockMessage;

    use super::*;

    #[test]
    fn the_ring_gives_messages_back_in_order_with_their_time_and_refuses_past_its_size() {
        let ring = Ring::new();
        assert_eq!(ring.pop(), None);
        let note = Timed { micros: 12_345, bytes: [0x99, 36, 100], len: 3 };
        let pulse = Timed { micros: 0, bytes: [0xF8, 0, 0], len: 1 };
        assert!(ring.push(note) && ring.push(pulse));
        assert_eq!(ring.pop(), Some(note));
        assert_eq!(ring.pop(), Some(pulse));
        for _ in 0..RING {
            assert!(ring.push(note));
        }
        assert!(!ring.push(note), "full");
        assert!(ring.pop().is_some());
        assert!(ring.push(pulse), "room again");
    }

    #[test]
    fn a_timing_message_keeps_its_length_through_the_ring() {
        let ring = Ring::new();
        let (bytes, len) = ClockMessage::Position(300).bytes();
        ring.push(Timed { micros: u32::MAX, bytes, len });
        let back = ring.pop().unwrap();
        assert_eq!((back.micros, back.len), (u32::MAX, 3));
        assert_eq!(ClockMessage::parse(&back.bytes[..back.len]), Some(ClockMessage::Position(300)));
    }
}
