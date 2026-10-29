//! The key names of the JS app and of older saves (`prob_pitch`, `velocity`
//! for the track volume, `infos`…), which loading still reads.

use crate::json::{Object, Value};
use crate::note::{Note, upgrade_arp};
use crate::track::Track;

/// A library, a pattern or a list of either with every legacy key renamed
/// to its current name, values and key order kept: the keys a load then a
/// save would write. Compares a JS export with what this app saves.
pub fn upgrade_keys(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(upgrade_keys).collect()),
        Value::Object(object) if object.contains_key("patterns") || object.contains_key("tracks") => {
            let mut upgraded = Object::new();
            for (key, value) in object.iter() {
                let value = match key {
                    "patterns" => upgrade_keys(value),
                    "tracks" => map_array(value, upgrade_track),
                    _ => value.clone(),
                };
                upgraded.insert(if key == "infos" { "info" } else { key }, value);
            }
            Value::Object(upgraded)
        }
        other => other.clone(),
    }
}

fn map_array(value: &Value, f: impl Fn(&Value) -> Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(f).collect()),
        other => other.clone(),
    }
}

fn rename_keys(object: &Object, canonical: impl Fn(&str) -> Option<&'static str>) -> Object {
    let mut renamed = Object::new();
    for (key, value) in object.iter() {
        renamed.insert(canonical(key).unwrap_or(key), value.clone());
    }
    renamed
}

fn upgrade_track(track: &Value) -> Value {
    let Some(object) = track.as_object() else { return track.clone() };
    let mut upgraded = rename_keys(object, Track::canonical_key);
    upgraded.remove("fxSelected"); // an editor tab, not a track setting
    let mut arp_column = None;
    if let Some(header) = upgraded.get_mut("noteKeys").and_then(Value::as_array_mut) {
        for key in header.iter_mut() {
            if let Some(name) = key.as_str().and_then(Note::canonical_key) {
                *key = Value::from(name);
            }
        }
        arp_column = header.iter().position(|key| key.as_str() == Some("arp"));
    }
    if let Some(notes) = upgraded.get_mut("notes").and_then(Value::as_array_mut) {
        for note in notes.iter_mut() {
            match note {
                Value::Object(object) => {
                    *object = rename_keys(object, Note::canonical_key);
                    if let Some(arp) = object.get_mut("arp") {
                        upgrade_arp(arp);
                    }
                }
                Value::Array(values) => {
                    if let Some(arp) = arp_column.and_then(|i| values.get_mut(i)) {
                        upgrade_arp(arp);
                    }
                }
                _ => {}
            }
        }
    }
    Value::Object(upgraded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    #[test]
    fn renames_track_note_and_library_keys_in_place() {
        let old = json::parse(
            r#"{"infos":{"name":"l"},"patterns":[{"name":"p","tracks":[{"name":"KICK","velocity":0.5,"prob_pitch":10,
                "noteKeys":["velocity","beat","prob"],"notes":[[0.4,1,0.5]]}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            upgrade_keys(&old).to_json(),
            r#"{"info":{"name":"l"},"patterns":[{"name":"p","tracks":[{"name":"KICK","volume":0.5,"autoPitchChance":10,"noteKeys":["velocity","beat","probability"],"notes":[[0.4,1,0.5]]}]}]}"#
        );
    }
}
