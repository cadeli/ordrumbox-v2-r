//! A pattern (`src/model/pattern_schema.js`): tempo, length, tags and tracks,
//! loaded like `fixPattern()` and saved like `Exporter.export()`.

use crate::json::{Object, Value};
use crate::record::Issue;
use crate::track::Track;

/// Keys read and dropped: `_revision` is runtime state; `application` and
/// `url`, the web app's name and address, were written in every pattern.
const DROPPED_KEYS: [&str; 3] = ["_revision", "application", "url"];

/// The generator's keys under their JS names (older saves).
const LEGACY_KEYS: [(&str, &str); 3] =
    [("_autoGenGenre", "autoGenre"), ("_autoGenKeyOffset", "autoKeyOffset"), ("_autoGenScale", "autoScale")];

/// The current name of a pattern key.
pub(crate) fn canonical_key(key: &str) -> &str {
    LEGACY_KEYS.iter().find(|(old, _)| *old == key).map_or(key, |(_, new)| new)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pattern {
    /// Stable id: song clips reference it, a rename never changes it. Empty
    /// until the library assigns one (see `Library`).
    pub id: String,
    pub name: Option<String>,
    /// Beats in the pattern (a measure is 4); 1–16.
    pub beat_count: i32,
    pub bpm: f64,
    pub description: String,
    pub tags: Vec<String>,
    pub tracks: Vec<Track>,
    /// Other keys (`autoGenre`, `autoKeyOffset`, `autoScale`…), kept and written back.
    pub extra: Object,
}

impl Default for Pattern {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: None,
            beat_count: 4,
            bpm: 120.0,
            description: String::new(),
            tags: Vec::new(),
            tracks: Vec::new(),
            extra: Object::new(),
        }
    }
}

impl Pattern {
    /// Load one pattern; the id is left empty when the file has none (the
    /// library assigns ids across all its patterns).
    pub(crate) fn from_json(value: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let Some(object) = value.as_object() else {
            issues.push(Issue::new(at, "pattern is not an object, skipped"));
            return None;
        };
        let mut pattern = Self::default();
        let mut wrong_type = |key: &str, value: &Value| {
            issues.push(Issue::new(format!("{at}.{key}"), format!("wrong type ({value}), default kept")));
        };
        for (key, value) in object.iter() {
            match key {
                // A blank or non-string id is replaced by one made from the name.
                "id" => pattern.id = value.as_str().map(str::trim).unwrap_or_default().to_owned(),
                "name" => match value.as_str() {
                    Some(name) => pattern.name = Some(name.to_owned()),
                    None => wrong_type(key, value),
                },
                "beatCount" => {
                    match value.as_f64().filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= f64::from(i32::MAX)) {
                        Some(n) => pattern.beat_count = n as i32,
                        None => wrong_type(key, value),
                    }
                }
                "bpm" => match value.as_f64().filter(|n| n.is_finite()) {
                    Some(bpm) => pattern.bpm = bpm,
                    None => wrong_type(key, value),
                },
                "description" => match value.as_str() {
                    Some(text) => pattern.description = text.to_owned(),
                    None => wrong_type(key, value),
                },
                "tags" => match tags_from_json(value) {
                    Some(tags) => pattern.tags = tags,
                    None => wrong_type(key, value),
                },
                "tracks" => {}
                key if DROPPED_KEYS.contains(&key) => {}
                _ => {
                    pattern.extra.insert(canonical_key(key), value.clone());
                }
            }
        }
        match object.get("tracks") {
            Some(Value::Array(tracks)) => {
                pattern.tracks = tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, track)| Track::from_json(track, &format!("{at}.tracks[{i}]"), issues))
                    .collect();
            }
            Some(Value::Null) | None => {}
            Some(_) => issues.push(Issue::new(format!("{at}.tracks"), "tracks is not an array, skipped")),
        }
        Some(pattern)
    }

    /// Save the non-default fields, the other keys, and the tracks last.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        if !self.id.is_empty() {
            object.insert("id", Value::from(self.id.as_str()));
        }
        if let Some(name) = &self.name {
            object.insert("name", Value::from(name.as_str()));
        }
        let defaults = Self::default();
        if self.beat_count != defaults.beat_count {
            object.insert("beatCount", Value::from(self.beat_count));
        }
        if self.bpm != defaults.bpm {
            object.insert("bpm", Value::from(self.bpm));
        }
        if !self.description.is_empty() {
            object.insert("description", Value::from(self.description.as_str()));
        }
        if !self.tags.is_empty() {
            object.insert("tags", Value::Array(self.tags.iter().map(|t| Value::from(t.as_str())).collect()));
        }
        for (key, value) in self.extra.iter() {
            object.insert(key, value.clone());
        }
        if !self.tracks.is_empty() {
            object.insert("tracks", Value::Array(self.tracks.iter().map(Track::to_json).collect()));
        }
        Value::Object(object)
    }
}

/// Tags as an array of strings, or the `{"0": "rock", "1": "70s"}` object an
/// older save wrote by mistake (39 of the 42 demo patterns): written back as
/// an array, which the JS app reads as well.
fn tags_from_json(value: &Value) -> Option<Vec<String>> {
    let items: Vec<&Value> = match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(object) => object.iter().map(|(_, item)| item).collect(),
        _ => return None,
    };
    items.into_iter().map(|item| item.as_str().map(str::to_owned)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn load(text: &str) -> (Pattern, Vec<Issue>) {
        let mut issues = Vec::new();
        let pattern = Pattern::from_json(&json::parse(text).unwrap(), "p", &mut issues).unwrap();
        (pattern, issues)
    }

    #[test]
    fn saves_only_what_differs_from_the_defaults() {
        let (pattern, issues) = load(r#"{"id":" rock ","name":"Rock","beatCount":4,"bpm":120,"_revision":3}"#);
        assert!(issues.is_empty());
        assert_eq!(pattern.to_json().to_json(), r#"{"id":"rock","name":"Rock"}"#);
    }

    #[test]
    fn drops_the_web_header_and_renames_the_generator_keys() {
        let (pattern, _) = load(r#"{"application":"other","url":"u","_autoGenGenre":"house","x":1,"bpm":97.5}"#);
        assert_eq!(pattern.to_json().to_json(), r#"{"bpm":97.5,"autoGenre":"house","x":1}"#);
    }

    #[test]
    fn reads_legacy_object_tags() {
        assert_eq!(load(r#"{"tags":{"0":"a","1":"b"}}"#).0.tags, ["a", "b"]);
        assert_eq!(load(r#"{"tags":["c"]}"#).0.tags, ["c"]);
        let (pattern, issues) = load(r#"{"tags":[1]}"#);
        assert!(pattern.tags.is_empty());
        assert_eq!(issues[0].at, "p.tags");
    }
}
