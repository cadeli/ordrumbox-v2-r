//! Helpers of the tools: reading the arguments, looking things up, shaping the answers
//! and the edits shared by several tools.

use super::NOT_EDITED;
use crate::editor::{Editor, MAX_BEATS};
use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::song::SongClip;
use ordrumbox_core::{Library, Note, Pattern, SetField, Track};

// ─── Arguments ───────────────────────────────────────────────────────────────

/// Argument `key`, which must be a string.
pub fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    optional_text(args, key)?.ok_or_else(|| format!("{key} is required"))
}

pub fn optional_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("{key}: not a string ({other})")),
    }
}

pub fn optional_number(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.is_finite() => Ok(Some(*n)),
        Some(other) => Err(format!("{key}: not a number ({other})")),
    }
}

/// A whole number from 0 (a step, a measure, an index).
pub fn optional_whole(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match optional_number(args, key)? {
        None => Ok(None),
        Some(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => Ok(Some(n as u32)),
        Some(n) => Err(format!("{key}: {n} is not a whole number from 0")),
    }
}

pub fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(format!("{key}: not true or false ({other})")),
    }
}

pub fn list<'a>(args: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match args.get(key) {
        Some(Value::Array(items)) => Ok(items),
        None | Some(Value::Null) => Err(format!("{key} is required")),
        Some(other) => Err(format!("{key}: not an array ({other})")),
    }
}

/// Two names the same once trimmed, in any case.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_uppercase() == b.trim().to_uppercase()
}

/// The track settings of `fields`, by their current key, checked: known
/// and of the right type (numbers are brought inside their range by the edit).
pub(super) fn track_fields(fields: &Object, at: &str) -> Result<Vec<(&'static str, Value)>, String> {
    let mut checked = Vec::new();
    for (key, value) in fields.iter() {
        let canonical = Track::canonical_key(key).filter(|k| Track::is_setting(k) && !NOT_EDITED.contains(k));
        let Some(canonical) = canonical else { return Err(format!("{at}.{key}: not a track setting")) };
        if Track::default().set_field(key, value) == SetField::WrongType {
            return Err(format!("{at}.{key}: wrong type ({value})"));
        }
        checked.push((canonical, value.clone()));
    }
    Ok(checked)
}

/// An arrangement's description, tempo and loop length, each when given.
pub(super) type SongSettings<'a> = (Option<&'a str>, Option<f64>, Option<f64>);

/// The arrangement settings of `args`.
pub(super) fn song_settings(args: &Value) -> Result<SongSettings<'_>, String> {
    let loop_measures = optional_number(args, "loopMeasures")?;
    if loop_measures.is_some_and(|l| l < 0.0) {
        return Err("loopMeasures: below 0".into());
    }
    Ok((optional_text(args, "description")?, optional_number(args, "bpm")?, loop_measures))
}

// ─── Lookups ─────────────────────────────────────────────────────────────────

/// A pattern by id, else by name (trimmed, any case).
pub fn find_pattern(library: &Library, reference: &str) -> Result<usize, String> {
    let patterns = &library.patterns;
    let by_id = patterns.iter().position(|p| p.id == reference.trim());
    by_id
        .or_else(|| patterns.iter().position(|p| p.name.as_deref().is_some_and(|n| same_name(n, reference))))
        .ok_or_else(|| format!("No pattern named \"{}\" (listPatterns gives them)", reference.trim()))
}

/// The pattern and track of arguments `patternName` and `trackName`.
pub fn find_track(library: &Library, args: &Value) -> Result<(usize, usize), String> {
    let p = find_pattern(library, text(args, "patternName")?)?;
    let name = text(args, "trackName")?;
    let t = track_index(&library.patterns[p], name)
        .ok_or_else(|| format!("The pattern has no track \"{}\"", name.trim()))?;
    Ok((p, t))
}

/// Argument `arrangement`, by name (any case) or index, else the selected one.
pub fn find_song(editor: &Editor, args: &Value) -> Result<usize, String> {
    let songs = &editor.library().songs;
    let found = match args.get("arrangement") {
        None | Some(Value::Null) => {
            Some(editor.selected_song()).filter(|&s| s < songs.len()).ok_or("No arrangement: createArrangement")
        }
        Some(Value::String(name)) => songs.iter().position(|s| same_name(&s.name, name)).ok_or("No such arrangement"),
        Some(Value::Number(n)) if n.fract() == 0.0 && *n >= 0.0 && (*n as usize) < songs.len() => Ok(*n as usize),
        Some(_) => Err("No such arrangement"),
    };
    found.map_err(|e| format!("{e} (listArrangements gives them)"))
}

/// The track of `pattern` named `name` (trimmed, any case).
pub(super) fn track_index(pattern: &Pattern, name: &str) -> Option<usize> {
    pattern.tracks.iter().position(|t| same_name(&t.name, name))
}

/// A pattern's name, its id when it has none.
pub fn pattern_name(pattern: &Pattern) -> String {
    pattern.name.clone().unwrap_or_else(|| pattern.id.clone())
}

// ─── Answers ─────────────────────────────────────────────────────────────────

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

/// Arrangement `s` as a tool shows it: its settings and its clips.
pub fn song_json(library: &Library, s: usize) -> Value {
    let song = &library.songs[s];
    let clips = song.clips.iter().map(|clip| {
        let name = library.pattern(&clip.pattern_id).and_then(|p| p.name.as_deref());
        object([
            ("patternId", Value::from(clip.pattern_id.as_str())),
            ("patternName", name.map_or(Value::Null, Value::from)),
            ("startMeasure", Value::from(clip.start_measure)),
            ("measureCount", Value::from(clip.measure_count)),
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
        ("measureCount", Value::from(clip.measure_count)),
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

// ─── Edits ───────────────────────────────────────────────────────────────────

/// Absolute step `step` of a track as its beat and its step in the beat.
pub(super) fn beat_and_step(step: u32, steps_per_beat: i32) -> (i32, i32) {
    let steps_per_beat = steps_per_beat.max(1);
    (step as i32 / steps_per_beat, step as i32 % steps_per_beat)
}

/// Pattern `p` at least `beats` long: the next multiple of 4 beats (at most
/// 16). A track that looped over its whole length still does.
pub(super) fn grow_pattern(editor: &mut Editor, p: usize, beats: i32) {
    let pattern = &editor.library().patterns[p];
    if beats <= pattern.beat_count {
        return;
    }
    let whole: Vec<usize> = (0..pattern.tracks.len())
        .filter(|&t| {
            let track = &pattern.tracks[t];
            track.loop_length == Some(track.beat_count * track.steps_per_beat)
        })
        .collect();
    let beats = ((beats + 3) / 4 * 4).min(MAX_BEATS);
    editor.set_pattern_beat_count(p, f64::from(beats));
    for t in whole {
        let length = beats * editor.library().patterns[p].tracks[t].steps_per_beat;
        editor.update_track(p, t, &[("loopLength", Value::from(length))], None, false);
    }
}
