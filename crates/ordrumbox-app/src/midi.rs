//! The MIDI input: the port the user chose (Edit > Preferences) through
//! `midir`. A note on plays the pattern's tracks (the engine's key map, as
//! under a plugin host) and, when recording, is written to the pattern. The
//! notes cross two ways: a ring the audio callback empties without locking
//! or allocating, and a channel the UI thread reads between frames. A
//! control change goes to the UI thread (the knobs, `Session::midi_control`);
//! the clock goes to `midi_sync` when it is followed.

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use midir::{MidiInput, MidiInputConnection};
use ordrumbox_engine::ClockMessage;
use ordrumbox_ui::{MidiPort, Waker};

/// A note on: its channel, key and velocity (0–1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOn {
    pub channel: u8,
    pub key: u8,
    pub velocity: f32,
}

/// What the MIDI thread tells the UI thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    Note(NoteOn),
    /// A control change: the value (0–127) of a controller on a channel.
    Control {
        channel: u8,
        controller: u8,
        value: u8,
    },
    /// The machine whose clock is followed started (`true`) or stopped.
    Clock(bool),
}

/// Notes the ring holds; past them the oldest wait and the newest are lost.
const RING: usize = 256;
/// Marks a slot as holding a note (a note on key 0, channel 0, velocity 0 is not one).
const FULL: u32 = 1 << 31;

/// A single-producer, single-consumer queue of notes on: the MIDI thread
/// writes, the audio callback reads. No lock, no allocation.
#[derive(Debug)]
pub struct Ring {
    slots: [AtomicU32; RING],
    /// Next slot to read (the consumer's).
    head: AtomicUsize,
    /// Next slot to write (the producer's).
    tail: AtomicUsize,
}

impl Ring {
    const fn new() -> Self {
        Self { slots: [const { AtomicU32::new(0) }; RING], head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    /// Queue `note`; false when the ring is full.
    pub fn push(&self, note: NoteOn) -> bool {
        let tail = self.tail.load(Ordering::Relaxed);
        if tail.wrapping_sub(self.head.load(Ordering::Acquire)) >= RING {
            return false;
        }
        // 0–127 and 0–15 fit their fields; the velocity is rounded to 7 bits.
        let velocity = (note.velocity.clamp(0.0, 1.0) * 127.0).round() as u32;
        let word = FULL | u32::from(note.channel & 15) << 16 | u32::from(note.key & 127) << 8 | velocity;
        self.slots[tail % RING].store(word, Ordering::Relaxed);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
        true
    }

    /// The oldest note queued.
    pub fn pop(&self) -> Option<NoteOn> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let word = self.slots[head % RING].load(Ordering::Relaxed);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(NoteOn {
            channel: ((word >> 16) & 15) as u8,
            key: ((word >> 8) & 127) as u8,
            velocity: (word & 127) as f32 / 127.0,
        })
    }
}

/// What the MIDI thread feeds and the audio callback reads.
pub static NOTES: Ring = Ring::new();

/// The note on a MIDI message is, if it is one (a note on at velocity 0 is a note off).
pub fn note_on(message: &[u8]) -> Option<NoteOn> {
    let [status, key, velocity, ..] = *message else { return None };
    (status & 0xF0 == 0x90 && velocity > 0 && key < 128).then(|| NoteOn {
        channel: status & 0x0F,
        key,
        velocity: f32::from(velocity.min(127)) / 127.0,
    })
}

/// The control change a MIDI message is, if it is one (the controllers from
/// 120 up are channel modes, not knobs).
pub fn control(message: &[u8]) -> Option<Event> {
    let [status, controller, value, ..] = *message else { return None };
    (status & 0xF0 == 0xB0 && controller < 120).then(|| Event::Control {
        channel: status & 0x0F,
        controller,
        value: value.min(127),
    })
}

/// The names of the MIDI input ports, for Edit > Preferences.
pub fn ports() -> Vec<String> {
    let Ok(input) = MidiInput::new("orDrumbox ports") else { return Vec::new() };
    input.ports().iter().filter_map(|port| input.port_name(port).ok()).collect()
}

/// An open MIDI input; dropping it closes the port.
pub struct Input {
    _connection: MidiInputConnection<()>,
    pub name: String,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input").field("name", &self.name).finish_non_exhaustive()
    }
}

/// Open the port `choice` names, its notes going to [`NOTES`] and to `ui`,
/// which `waker` then wakes, with its control changes and the starts and
/// stops of the clock followed. `Ok(None)`: no port to open (none chosen,
/// none connected, or the port chosen is not).
pub fn open(choice: &MidiPort, ui: Sender<Event>, waker: Waker) -> Result<Option<Input>, String> {
    let input = MidiInput::new("orDrumbox").map_err(|e| format!("MIDI: {e}"))?;
    let ports = input.ports();
    let port = match choice {
        MidiPort::Off => return Ok(None),
        MidiPort::First => ports.first(),
        MidiPort::Named(wanted) => ports.iter().find(|p| input.port_name(p).is_ok_and(|name| name == *wanted)),
    };
    let Some(port) = port else { return Ok(None) };
    let name = input.port_name(port).map_err(|e| format!("MIDI: {e}"))?;
    let connection = input
        .connect(
            port,
            "orDrumbox in",
            move |_, message, ()| {
                let event = match ClockMessage::parse(message) {
                    Some(clock) => crate::midi_sync::input(clock).map(Event::Clock),
                    None => note_on(message).map(|note| {
                        NOTES.push(note);
                        Event::Note(note)
                    }),
                }
                .or_else(|| control(message));
                // The UI thread is gone when the app quits: nobody to tell.
                if let Some(event) = event
                    && ui.send(event).is_ok()
                {
                    waker.wake();
                }
            },
            (),
        )
        .map_err(|e| format!("MIDI {name}: {e}"))?;
    Ok(Some(Input { _connection: connection, name }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_note_on_with_a_velocity_is_a_note() {
        assert_eq!(note_on(&[0x99, 36, 127]), Some(NoteOn { channel: 9, key: 36, velocity: 1.0 }));
        assert_eq!(note_on(&[0x90, 60, 0]), None, "a note off in disguise");
        assert_eq!(note_on(&[0x80, 60, 64]), None);
        assert_eq!(note_on(&[0xB0, 1, 64]), None);
        assert_eq!(note_on(&[0x90, 60]), None);
    }

    #[test]
    fn a_control_change_is_a_knob_unless_it_is_a_channel_mode() {
        assert_eq!(control(&[0xB2, 21, 99]), Some(Event::Control { channel: 2, controller: 21, value: 99 }));
        assert_eq!(control(&[0xB0, 120, 0]), None, "all sound off");
        assert_eq!(control(&[0x90, 21, 99]), None);
        assert_eq!(control(&[0xB0, 7]), None);
    }

    #[test]
    fn the_ring_gives_notes_back_in_order_and_refuses_past_its_size() {
        let ring = Ring::new();
        assert_eq!(ring.pop(), None);
        for n in 0..RING {
            assert!(ring.push(NoteOn { channel: 9, key: (n % 128) as u8, velocity: 0.5 }));
        }
        assert!(!ring.push(NoteOn { channel: 0, key: 1, velocity: 1.0 }), "full");
        let first = ring.pop().unwrap();
        assert_eq!((first.channel, first.key), (9, 0));
        assert!((first.velocity - 0.5).abs() < 0.01);
        assert!(ring.push(NoteOn { channel: 0, key: 1, velocity: 1.0 }), "room again");
        assert_eq!((1..RING).filter_map(|_| ring.pop()).count(), RING - 1);
        assert_eq!(ring.pop().map(|n| n.key), Some(1));
    }
}
