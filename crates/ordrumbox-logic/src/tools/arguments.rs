//! What the tools read: their arguments, checked, and the patterns, tracks and
//! songs they name.

use super::NOT_EDITED;

use crate::editor::Editor;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::{Library, Pattern, SetField, Track};

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
    library
        .pattern_index(reference.trim())
        .or_else(|| library.patterns.iter().position(|p| p.name.as_deref().is_some_and(|n| same_name(n, reference))))
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
