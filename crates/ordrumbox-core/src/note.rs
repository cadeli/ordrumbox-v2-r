//! A note of a track (`src/core/note_schema.js`) and its compact file encoding.
//!
//! In a file, the notes of a track are arrays of values named by a `noteKeys`
//! header (`noteKeys: ["velocity", "beat"]`, `notes: [[0.4], [0.9, 1]]`);
//! trailing values at their default are left out.

use crate::json::{Object, Value};
use crate::record::{Issue, SetField, json_record, round2};

json_record! {
    /// One note. The field order is the compact-format key order
    /// (`NOTE_KEY_ORDER`): a `noteKeys` header lists its keys in this order.
    pub struct Note {
        /// Playback volume, 0–1.
        velocity: f64 = 0.8 => "velocity",
        /// Beat index within the track, 0-based (a measure is 4 beats).
        beat: i32 = 0 => "beat",
        /// Step within the beat, `0..stepsPerBeat`.
        beat_step: i32 = 0 => "beatStep",
        /// Transposition in semitones (may be fractional).
        pitch: f64 = 0.0 => "pitch",
        /// Stereo position, -1 (left) to 1 (right).
        pan: f64 = 0.0 => "pan",
        /// Fires once every `every` passes of the pattern (not every N steps).
        every: i32 = 1 => "every",
        /// Trigger probability, 0–1.
        prob: f64 = 1.0 => "prob",
        /// Ghost/retrigger spacing code: `< 8` is `rate / 8` step, `>= 8` is
        /// `rate - 7` steps. 1 is the tightest, not "normal".
        rate: f64 = 1.0 => "rate",
        /// Hits: the note plus its repeats (with an arpeggio, the arp notes).
        retrigger_count: i32 = 1 => "retriggerCount",
        /// Arpeggio as written in the file: interval array, `"0,4,7"`, a mode
        /// keyword, or `{intervals, mode}`. `null` = none. Interpreted by the
        /// pattern engine, kept verbatim here.
        arp: Value = Value::Null => "arp",
        /// Probability of each arpeggio/retrigger hit, 0–1.
        arp_trigger_probability: f64 = 1.0 => "arpTriggerProbability",
        /// Euclidean pulses over the span to the next note, base note included (0 = off).
        euclidean_fill: i32 = 0 => "euclideanFill",
        /// Rotation of the euclidean pattern, in steps.
        euclidean_rotation: i32 = 0 => "euclideanRotation",
        /// Phase in the `every` cycle: fires on the pass where `(pass + pos) % every == 0`.
        pos: i32 = 0 => "pos",
        /// Arpeggio scale picked in the note editor.
        arp_scale: Option<String> = None => "_arpScale",
        /// Arpeggio direction picked in the note editor.
        arp_type: Option<String> = None => "_arpType",
    }
    other_fields {
        /// Position in the beat, percent (`stepPercent`): never saved, kept
        /// while editing so a change of steps per beat and back restores the
        /// step. `None`: derived from `beat_step`.
        step_percent: Option<f64> = None,
    }
}

/// Keys written rounded to 2 decimals (`NOTE_ROUND_2D`).
const ROUND_2D: [&str; 4] = ["velocity", "pan", "prob", "rate"];

impl Note {
    /// A note from its object form (`{"beat": 1, "velocity": 0.5}`). Unknown
    /// keys (`stepPercent`, recomputed; anything else) are ignored: the compact
    /// format the app writes cannot hold them.
    pub(crate) fn from_object(object: &Object, at: &str, issues: &mut Vec<Issue>) -> Self {
        let mut note = Self::default();
        for (key, value) in object.iter() {
            note.set_or_report(key, value, at, issues);
        }
        note
    }

    fn set_or_report(&mut self, key: &str, value: &Value, at: &str, issues: &mut Vec<Issue>) {
        if self.set_field(key, value) == SetField::WrongType {
            issues.push(Issue::new(format!("{at}.{key}"), format!("wrong type ({}), default kept", value.to_json())));
        }
    }

    /// `stepPercent` at `steps_per_beat` steps per beat.
    pub fn step_percent(&self, steps_per_beat: i32) -> f64 {
        self.step_percent.filter(|p| p.is_finite()).unwrap_or_else(|| {
            crate::js::round(
                f64::from(self.beat_step) * 100.0 / f64::from(if steps_per_beat == 0 { 4 } else { steps_per_beat }),
            )
        })
    }

    /// Fold a `beatStep` past the end of its beat into the following beats:
    /// beat 1 step 6 at 4 steps per beat is beat 2 step 2 (`normalizeNoteGridPosition`).
    pub(crate) fn wrap_grid_position(&mut self, steps_per_beat: i32) {
        if steps_per_beat > 0 && self.beat_step >= steps_per_beat {
            self.beat += self.beat_step / steps_per_beat;
            self.beat_step %= steps_per_beat;
        }
    }
}

/// Decode a track's notes: compact arrays named by `note_keys`, or objects.
///
/// Header rule: distinct known keys, **in any order**. The JS loader also
/// requires `NOTE_KEY_ORDER` order and otherwise keeps the raw arrays, which
/// its exporter then writes back as garbage (`[[null]]`). The positional decode
/// is unambiguous as long as each key is known and appears once.
pub(crate) fn decode_notes(notes: &[Value], note_keys: Option<&Value>, at: &str, issues: &mut Vec<Issue>) -> Vec<Note> {
    let header = note_keys.map(|keys| parse_header(keys, at, issues));
    let mut decoded = Vec::with_capacity(notes.len());
    for (i, item) in notes.iter().enumerate() {
        let note_at = format!("{at}.notes[{i}]");
        match (item, &header) {
            (Value::Object(object), _) => decoded.push(Note::from_object(object, &note_at, issues)),
            (Value::Array(values), Some(Some(keys))) => {
                if values.len() > keys.len() {
                    issues.push(Issue::new(&note_at, "more values than noteKeys, extra values ignored"));
                }
                let mut note = Note::default();
                for (key, value) in keys.iter().zip(values) {
                    note.set_or_report(key, value, &note_at, issues);
                }
                decoded.push(note);
            }
            (Value::Array(_), Some(None)) => {} // header already reported
            (Value::Array(_), None) => {
                issues.push(Issue::new(note_at, "compact note without a noteKeys header, skipped"))
            }
            _ => issues.push(Issue::new(note_at, "note is neither an object nor an array, skipped")),
        }
    }
    decoded
}

/// The header keys, or `None` (reported) when they cannot name note fields.
fn parse_header(keys: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Vec<String>> {
    let at = format!("{at}.noteKeys");
    let Some(items) = keys.as_array() else {
        issues.push(Issue::new(at, "noteKeys is not an array, compact notes skipped"));
        return None;
    };
    let mut header: Vec<String> = Vec::with_capacity(items.len());
    for item in items {
        match item.as_str() {
            Some(key) if !Note::KEYS.contains(&key) => {
                issues.push(Issue::new(at, format!("unknown note key {key:?}, compact notes skipped")));
                return None;
            }
            Some(key) if header.iter().any(|k| k == key) => {
                issues.push(Issue::new(at, format!("duplicate note key {key:?}, compact notes skipped")));
                return None;
            }
            Some(key) => header.push(key.to_owned()),
            None => {
                issues.push(Issue::new(at, "noteKeys holds a non-string, compact notes skipped"));
                return None;
            }
        }
    }
    Some(header)
}

/// Encode notes in the compact format (`Exporter.encodeNotes`): the header
/// lists, in key order, every key some note holds at a non-default value; each
/// note stops at its last non-default value. `velocity`, `pan`, `prob` and
/// `rate` are rounded to 2 decimals.
pub(crate) fn encode_notes(notes: &[Note]) -> (Vec<Value>, Vec<Value>) {
    let fields: Vec<_> = notes.iter().map(Note::table_fields).collect();
    let used: Vec<usize> = (0..Note::KEYS.len()).filter(|&k| fields.iter().any(|note| !note[k].2)).collect();

    let header = used.iter().map(|&k| Value::from(Note::KEYS[k])).collect();
    let encoded = fields
        .iter()
        .map(|note| {
            let last = used.iter().rposition(|&k| !note[k].2).map_or(0, |i| i + 1);
            let values = used[..last]
                .iter()
                .map(|&k| {
                    let (key, value, _) = &note[k];
                    match value {
                        Value::Number(n) if ROUND_2D.contains(key) => Value::Number(round2(*n)),
                        _ => value.clone(),
                    }
                })
                .collect();
            Value::Array(values)
        })
        .collect();
    (header, encoded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn decode(notes: &str, keys: Option<&str>) -> (Vec<Note>, Vec<Issue>) {
        let notes = json::parse(notes).unwrap();
        let keys = keys.map(|k| json::parse(k).unwrap());
        let mut issues = Vec::new();
        let decoded = decode_notes(notes.as_array().unwrap(), keys.as_ref(), "t", &mut issues);
        (decoded, issues)
    }

    fn encode(notes: &[Note]) -> String {
        let (header, encoded) = encode_notes(notes);
        format!("{} {}", Value::Array(header), Value::Array(encoded))
    }

    #[test]
    fn key_order_is_the_js_note_key_order() {
        assert_eq!(
            Note::KEYS,
            [
                "velocity",
                "beat",
                "beatStep",
                "pitch",
                "pan",
                "every",
                "prob",
                "rate",
                "retriggerCount",
                "arp",
                "arpTriggerProbability",
                "euclideanFill",
                "euclideanRotation",
                "pos",
                "_arpScale",
                "_arpType",
            ]
        );
    }

    #[test]
    fn decodes_compact_notes() {
        let (notes, issues) =
            decode("[[0.4], [0.9, 1], [0.35, 2, 2, -3]]", Some(r#"["velocity","beat","beatStep","pitch"]"#));
        assert!(issues.is_empty());
        assert_eq!(notes[0], Note { velocity: 0.4, ..Note::default() });
        assert_eq!(notes[2], Note { velocity: 0.35, beat: 2, beat_step: 2, pitch: -3.0, ..Note::default() });
    }

    #[test]
    fn accepts_a_reordered_header() {
        let (notes, issues) = decode("[[1, 0.5]]", Some(r#"["beat","velocity"]"#));
        assert!(issues.is_empty());
        assert_eq!(notes[0], Note { beat: 1, velocity: 0.5, ..Note::default() });
    }

    #[test]
    fn rejects_unknown_or_duplicate_keys() {
        let (notes, issues) = decode("[[1]]", Some(r#"["steppc"]"#));
        assert!(notes.is_empty());
        assert_eq!(issues[0].message, "unknown note key \"steppc\", compact notes skipped");
        let (notes, _) = decode("[[1, 2]]", Some(r#"["beat","beat"]"#));
        assert!(notes.is_empty());
    }

    #[test]
    fn reports_wrong_types_and_keeps_defaults() {
        let (notes, issues) = decode(r#"[[2.5, "x"], {"velocity": null, "pos": 1}]"#, Some(r#"["beat","pitch"]"#));
        assert_eq!(notes[0], Note::default());
        assert_eq!(issues.len(), 2);
        assert_eq!(issues[0].at, "t.notes[0].beat");
        assert_eq!(notes[1], Note { pos: 1, ..Note::default() });
    }

    #[test]
    fn encodes_like_the_js_exporter() {
        let notes = [
            Note { velocity: 0.81234, beat: 1, beat_step: 1, ..Note::default() },
            Note { beat: 3, beat_step: 1, pitch: -2.5, prob: 0.3333, arp: Value::from("0,4,7"), ..Note::default() },
            Note::default(),
        ];
        assert_eq!(
            encode(&notes),
            r#"["velocity","beat","beatStep","pitch","prob","arp"] [[0.81,1,1],[0.8,3,1,-2.5,0.33,"0,4,7"],[]]"#
        );
    }

    #[test]
    fn all_default_notes_keep_their_slot() {
        assert_eq!(encode(&[Note::default(), Note::default()]), "[] [[],[]]");
    }

    #[test]
    fn grid_wrap_adds_whole_beats() {
        let mut note = Note { beat: 1, beat_step: 6, ..Note::default() };
        note.wrap_grid_position(4);
        assert_eq!((note.beat, note.beat_step), (2, 2));
        note.wrap_grid_position(0);
        assert_eq!((note.beat, note.beat_step), (2, 2));
    }
}
