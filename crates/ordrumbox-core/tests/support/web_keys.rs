//! Test support: the golden fixtures, written by the web version, under this
//! app's key names (`prob_pitch` → `autoPitchChance`, `velocity` → `volume`
//! for a track, `infos` → `info`…), to compare them with what it saves.
//! Shared by the tests of `ordrumbox-core` and `ordrumbox-logic` (`#[path]`).

use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::note::upgrade_arp;
use ordrumbox_core::{Note, Pattern, Track};

/// A library, a pattern or a list of either with every web-version key
/// renamed to its current name and the dropped keys left out, values and key
/// order kept: the keys a load then a save would write.
pub fn current_keys(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(current_keys).collect()),
        Value::Object(object) if object.contains_key("patterns") => {
            let mut upgraded = Object::new();
            for (key, value) in object.iter() {
                let (key, value) = match key {
                    "patterns" => (key, map_array(value, upgrade_pattern)),
                    "songs" => (key, map_array(value, upgrade_song)),
                    "infos" => ("info", value.clone()),
                    "selectedPatternIdx" => ("selectedPattern", value.clone()),
                    "selectedSongIdx" => ("selectedSong", value.clone()),
                    _ => (key, value.clone()),
                };
                upgraded.insert(key, value);
            }
            Value::Object(upgraded)
        }
        Value::Object(_) => upgrade_pattern(value),
        other => other.clone(),
    }
}

fn upgrade_pattern(pattern: &Value) -> Value {
    let Some(object) = pattern.as_object() else { return pattern.clone() };
    let mut upgraded = Object::new();
    for (key, value) in object.iter() {
        match key {
            "application" | "url" | "_revision" => {}
            "tracks" => _ = upgraded.insert(key, map_array(value, upgrade_track)),
            _ => _ = upgraded.insert(Pattern::canonical_key(key), value.clone()),
        }
    }
    Value::Object(upgraded)
}

fn upgrade_song(song: &Value) -> Value {
    let Some(object) = song.as_object() else { return song.clone() };
    let mut upgraded = Object::new();
    for (key, value) in object.iter() {
        match key {
            "loopMeasureCount" => _ = upgraded.insert("loopMeasures", value.clone()),
            "clips" => {
                _ = upgraded.insert(
                    key,
                    map_array(value, |clip| match clip.as_object() {
                        Some(clip) => Value::Object(rename_keys(clip, |key| (key == "pattern").then_some("patternId"))),
                        None => clip.clone(),
                    }),
                )
            }
            _ => _ = upgraded.insert(key, value.clone()),
        }
    }
    Value::Object(upgraded)
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
    use ordrumbox_core::json;

    #[test]
    fn renames_track_note_and_library_keys_in_place() {
        let old = json::parse(
            r#"{"infos":{"name":"l"},"patterns":[{"name":"p","tracks":[{"name":"KICK","velocity":0.5,"prob_pitch":10,
                "noteKeys":["velocity","beat","prob"],"notes":[[0.4,1,0.5]]}]}]}"#,
        )
        .unwrap();
        assert_eq!(
            current_keys(&old).to_json(),
            r#"{"info":{"name":"l"},"patterns":[{"name":"p","tracks":[{"name":"KICK","volume":0.5,"autoPitchChance":10,"noteKeys":["velocity","beat","probability"],"notes":[[0.4,1,0.5]]}]}]}"#
        );
    }
}
