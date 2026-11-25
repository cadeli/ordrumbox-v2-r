//! What the tools answer: the library's patterns, tracks, notes and songs as JSON.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::song::SongClip;
use ordrumbox_core::{Library, Note, Pattern, Track};

/// `{key: value, ...}`.
pub fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(key, value);
    }
    Value::Object(object)
}

pub fn count(n: usize) -> Value {
    Value::from(n as f64)
}

pub fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|s| Value::from(s.as_str())).collect())
}

/// A note as a tool shows it: its absolute step, and the fields away from
/// their default.
pub(super) fn note_json(note: &Note, steps_per_beat: i32) -> Value {
    let defaults = Note::default();
    let mut object = Object::new();
    object.insert("step", Value::from(note.beat * steps_per_beat + note.step));
    for &key in Note::KEYS {
        let value = note.field(key).unwrap_or_default();
        if value != defaults.field(key).unwrap_or_default() {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

/// A track as a tool shows it: every setting, its notes.
pub(super) fn track_json(track: &Track) -> Value {
    let mut object = Object::new();
    for &key in Track::KEYS {
        object.insert(key, track.field(key).unwrap_or_default());
    }
    let notes = track.notes.iter().map(|n| note_json(n, track.steps_per_beat)).collect();
    object.insert("notes", Value::Array(notes));
    Value::Object(object)
}

/// Pattern `p` as a list shows it.
pub fn pattern_summary(index: usize, pattern: &Pattern) -> Value {
    object([
        ("index", count(index)),
        ("id", Value::from(pattern.id.as_str())),
        ("name", pattern.name.as_deref().map_or(Value::Null, Value::from)),
        ("beatCount", Value::from(pattern.beat_count)),
        ("trackCount", count(pattern.tracks.len())),
        ("tags", strings(&pattern.tags)),
    ])
}

/// Song `s` as a tool shows it: its settings and its clips.
pub fn song_json(library: &Library, s: usize) -> Value {
    let song = &library.songs[s];
    let clips = song.clips.iter().map(|clip| {
        let name = library.pattern(&clip.pattern_id).and_then(|p| p.name.as_deref());
        object([
            ("patternId", Value::from(clip.pattern_id.as_str())),
            ("patternName", name.map_or(Value::Null, Value::from)),
            ("startMeasure", Value::from(clip.start_measure)),
            ("measureCount", Value::from(clip.length_measures)),
        ])
    });
    object([
        ("index", count(s)),
        ("id", Value::from(song.id.as_str())),
        ("name", Value::from(song.name.as_str())),
        ("description", Value::from(song.description.as_str())),
        ("bpm", Value::from(song.bpm)),
        ("lengthMeasures", Value::from(song.length_measures())),
        ("clips", Value::Array(clips.collect())),
    ])
}

/// A clip as a tool shows it.
pub(super) fn clip_json(clip: &SongClip) -> Value {
    object([
        ("patternId", Value::from(clip.pattern_id.as_str())),
        ("startMeasure", Value::from(clip.start_measure)),
        ("measureCount", Value::from(clip.length_measures)),
    ])
}

pub(super) fn instruments() -> Value {
    let instruments = catalog().instruments.iter().map(|i| {
        object([
            ("id", Value::from(i.id.as_str())),
            ("drum", Value::Bool(i.drum)),
            ("pan", Value::from(i.pan.as_str())),
        ])
    });
    let instruments: Vec<Value> = instruments.collect();
    object([("count", count(instruments.len())), ("instruments", Value::Array(instruments))])
}
