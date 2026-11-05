//! Dragging a note edits it (`ui/components/note_drag.js`): up and down
//! moves its velocity (200 px for 0 to 1), sideways transposes it (12 px a
//! semitone, inside the piano roll's range), and the axis is claimed on the
//! first real movement (3 px), so a sloppy diagonal changes one parameter
//! only. A press that claims no axis is a click.

use ordrumbox_core::Note;
use ordrumbox_core::js::round;

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
                    Axis::Pitch => round(if note.pitch.is_finite() { note.pitch } else { 0.0 }),
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
            Axis::Pitch => (self.start + round(dx / PITCH_PX_PER_SEMITONE))
                .clamp(MIDI_MIN - MIDDLE_C - self.track_pitch, MIDI_MAX - MIDDLE_C - self.track_pitch),
            Axis::Velocity => round((self.start + dy * VELOCITY_PER_PX).clamp(0.0, 1.0) * 100.0) / 100.0,
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
    use ordrumbox_core::json::{self, Value};

    /// The JS `NoteDrag` and `midiName` (tools/export_piano_fixtures.mjs).
    #[test]
    fn golden_drags_and_names() {
        let fixture = json::parse(include_str!("../tests/fixtures/piano_roll.json")).unwrap();
        let drags = fixture.get("drags").and_then(Value::as_array).unwrap();
        for case in drags {
            let mut note = Note::default();
            for (key, value) in case.get("start").and_then(Value::as_object).unwrap().iter() {
                let _ = note.set_field(key, value);
            }
            let track_pitch = case.get("trackPitch").and_then(Value::as_f64).unwrap();
            let mut drag = NoteDrag::new(0, 0, track_pitch);
            let mut calls = Vec::new();
            for m in case.get("moves").and_then(Value::as_array).unwrap() {
                let m = m.as_array().unwrap();
                let (x, y) = (m[0].as_f64().unwrap() as i32, m[1].as_f64().unwrap() as i32);
                for event in drag.moved(x, y, &note) {
                    calls.push(match event {
                        DragEvent::Start(axis, v) => {
                            format!(r#"{{"start":"{}","value":{}}}"#, axis.key(), Value::from(v).to_json())
                        }
                        DragEvent::Value(axis, v, dir) => {
                            // The surface writes the value.
                            match axis {
                                Axis::Pitch => note.pitch = v,
                                Axis::Velocity => note.velocity = v,
                            }
                            format!(r#"{{"axis":"{}","value":{},"dir":{dir}}}"#, axis.key(), Value::from(v).to_json())
                        }
                    });
                }
            }
            let expected: Vec<String> =
                case.get("calls").and_then(Value::as_array).unwrap().iter().map(Value::to_json).collect();
            assert_eq!(calls, expected, "{}", case.to_json());
            let axis = case.get("axis").and_then(Value::as_str);
            assert_eq!(drag.axis().map(Axis::key), axis);
        }
        let names = fixture.get("midiNames").and_then(Value::as_array).unwrap();
        for (midi, name) in names.iter().enumerate() {
            assert_eq!(midi_name(midi as i32), name.as_str().unwrap());
        }
    }
}
