//! Dragging a note edits it (`ui/components/note_drag.js`): up and down
//! moves its velocity (200 px for 0 to 1), sideways transposes it (12 px a
//! semitone, inside the piano roll's range), and the axis is claimed on the
//! first real movement (3 px), so a sloppy diagonal changes one parameter
//! only. A press that claims no axis is a click.

use ordrumbox_core::Note;
use ordrumbox_core::num::round2;

/// Movement before the gesture claims an axis, px.
const AXIS_LOCK_PX: f64 = 3.0;
/// Velocity per pixel dragged up.
const VELOCITY_PER_PX: f64 = 0.005;
/// Pixels per semitone dragged right.
const PITCH_PX_PER_SEMITONE: f64 = 12.0;
/// The piano roll's range and middle C (`piano_roll_constants.js`).
pub const MIDI_MIN: f64 = 12.0;
pub const MIDI_MAX: f64 = 108.0;
pub const MIDDLE_C: f64 = 60.0;
/// A note's velocity when it has none (`NOTE_DEFAULTS`).
const DEFAULT_VELOCITY: f64 = 0.8;

/// What a drag edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Velocity,
    Pitch,
}

impl Axis {
    /// The note field it edits.
    pub fn key(self) -> &'static str {
        match self {
            Self::Velocity => "velocity",
            Self::Pitch => "pitch",
        }
    }
}

/// What a movement did.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragEvent {
    /// The axis was claimed: the value it starts from.
    Start(Axis, f64),
    /// A new value for the note, and its direction from the start.
    Value(Axis, f64, i32),
}

/// A note being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteDrag {
    /// Where the press was.
    x: i32,
    y: i32,
    track_pitch: f64,
    axis: Option<Axis>,
    start: f64,
}

impl NoteDrag {
    /// A press at `(x, y)` on a note of a track transposed by `track_pitch`.
    pub fn new(x: i32, y: i32, track_pitch: f64) -> Self {
        Self { x, y, track_pitch, axis: None, start: 0.0 }
    }

    /// The axis claimed, `None` while the pointer has not really moved: a click.
    pub fn axis(&self) -> Option<Axis> {
        self.axis
    }

    /// The pointer at `(x, y)` over `note` as it is now; what to do.
    pub fn moved(&mut self, x: i32, y: i32, note: &Note) -> Vec<DragEvent> {
        let dx = f64::from(x - self.x);
        let dy = f64::from(self.y - y); // dragging up is positive
        let mut events = Vec::new();
        let axis = match self.axis {
            Some(axis) => axis,
            None => {
                if dx.abs() < AXIS_LOCK_PX && dy.abs() < AXIS_LOCK_PX {
                    return events;
                }
                let axis = if dx.abs() >= dy.abs() { Axis::Pitch } else { Axis::Velocity };
                self.start = match axis {
                    Axis::Pitch => {
                        if note.pitch.is_finite() {
                            note.pitch.round()
                        } else {
                            0.0
                        }
                    }
                    Axis::Velocity => {
                        if note.velocity.is_finite() {
                            note.velocity
                        } else {
                            DEFAULT_VELOCITY
                        }
                    }
                };
                self.axis = Some(axis);
                events.push(DragEvent::Start(axis, self.start));
                axis
            }
        };
        let value = match axis {
            Axis::Pitch => (self.start + (dx / PITCH_PX_PER_SEMITONE).round())
                .clamp(MIDI_MIN - MIDDLE_C - self.track_pitch, MIDI_MAX - MIDDLE_C - self.track_pitch),
            Axis::Velocity => round2((self.start + dy * VELOCITY_PER_PX).clamp(0.0, 1.0)),
        };
        let current = match axis {
            Axis::Pitch => note.pitch,
            Axis::Velocity => note.velocity,
        };
        // `Object.is`: -0 is not 0.
        if current.to_bits() != value.to_bits() {
            events.push(DragEvent::Value(axis, value, if value > self.start { 1 } else { -1 }));
        }
        events
    }
}

/// The name of MIDI note `midi`, C4 = 60.
pub fn midi_name(midi: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Axis::{Pitch, Velocity};
    use DragEvent::{Start, Value};

    /// A note `(velocity, pitch)` on a track transposed by `track_pitch`,
    /// dragged through `moves` from (0, 0): the events, and the axis claimed.
    struct Case {
        note: (f64, f64),
        track_pitch: f64,
        moves: &'static [(i32, i32)],
        events: &'static [DragEvent],
        axis: Option<Axis>,
    }

    const CASES: &[Case] = &[
        // Up and down: the velocity, by hundredths, inside 0–1.
        Case {
            note: (0.8, 0.0),
            track_pitch: 0.0,
            moves: &[(1, -1), (2, -2), (0, -3), (0, -10), (0, -100), (5, 40), (0, 300)],
            events: &[
                Start(Velocity, 0.8),
                Value(Velocity, 0.82, 1),
                Value(Velocity, 0.85, 1),
                Value(Velocity, 1.0, 1),
                Value(Velocity, 0.6, -1),
                Value(Velocity, 0.0, -1),
            ],
            axis: Some(Velocity),
        },
        // Sideways: the pitch, inside the piano roll's range.
        Case {
            note: (0.5, 3.0),
            track_pitch: 0.0,
            moves: &[(3, 1), (11, 0), (12, -2), (30, 9), (-50, 0), (-900, 0), (900, 0)],
            events: &[
                Start(Pitch, 3.0),
                Value(Pitch, 4.0, 1),
                Value(Pitch, 6.0, 1),
                Value(Pitch, -1.0, -1),
                Value(Pitch, -48.0, -1),
                Value(Pitch, 48.0, 1),
            ],
            axis: Some(Pitch),
        },
        // The track's transposition narrows the range.
        Case {
            note: (0.8, 0.0),
            track_pitch: 40.0,
            moves: &[(4, 4), (200, 0)],
            events: &[Start(Pitch, 0.0), Value(Pitch, 8.0, 1)],
            axis: Some(Pitch),
        },
        // A fractional pitch starts from its rounding.
        Case {
            note: (0.8, 2.6),
            track_pitch: 0.0,
            moves: &[(-3, 3), (-13, 0)],
            events: &[Start(Pitch, 3.0), Value(Pitch, 3.0, -1), Value(Pitch, 2.0, -1)],
            axis: Some(Pitch),
        },
        Case {
            note: (0.333, 0.0),
            track_pitch: 0.0,
            moves: &[(2, 3), (2, 5), (2, 4)],
            events: &[Start(Velocity, 0.333), Value(Velocity, 0.32, -1), Value(Velocity, 0.31, -1)],
            axis: Some(Velocity),
        },
        // Less than 3 px, or no movement: a click.
        Case { note: (0.8, 0.0), track_pitch: 0.0, moves: &[(1, 1), (2, -2)], events: &[], axis: None },
        Case { note: (0.8, 0.0), track_pitch: 0.0, moves: &[], events: &[], axis: None },
    ];

    #[test]
    fn a_drag_gives_its_events() {
        for case in CASES {
            let mut note = Note { velocity: case.note.0, pitch: case.note.1, ..Note::default() };
            let mut drag = NoteDrag::new(0, 0, case.track_pitch);
            let mut events = Vec::new();
            for &(x, y) in case.moves {
                for event in drag.moved(x, y, &note) {
                    // The surface writes the value.
                    if let Value(axis, v, _) = event {
                        match axis {
                            Pitch => note.pitch = v,
                            Velocity => note.velocity = v,
                        }
                    }
                    events.push(event);
                }
            }
            assert_eq!(events, case.events, "{:?} on {} by {:?}", case.note, case.track_pitch, case.moves);
            assert_eq!(drag.axis(), case.axis);
        }
    }

    #[test]
    fn midi_names() {
        let names = [
            (-2, "A#-2"),
            (0, "C-1"),
            (1, "C#-1"),
            (11, "B-1"),
            (12, "C0"),
            (21, "A0"),
            (59, "B3"),
            (60, "C4"),
            (61, "C#4"),
            (69, "A4"),
            (108, "C8"),
            (127, "G9"),
        ];
        for (midi, name) in names {
            assert_eq!(midi_name(midi), name);
        }
    }
}
