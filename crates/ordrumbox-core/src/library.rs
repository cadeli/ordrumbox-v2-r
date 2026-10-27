//! A library file (`assets/data/song.json`): `{infos, songs, patterns}`, the
//! patterns being shared by every song of the file.

use std::collections::HashSet;

use crate::ids::{slugify, unique_id};
use crate::json::{self, Object, ParseError, Value};
use crate::pattern::Pattern;
use crate::record::Issue;
use crate::song::{DroppedClip, Song, songs_from_json};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Library {
    /// Free-form header (`name`, `description`, `date`…), kept as written.
    pub infos: Object,
    pub songs: Vec<Song>,
    pub patterns: Vec<Pattern>,
}

/// What a load had to repair or leave out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadReport {
    /// Song clips whose pattern reference is unusable.
    pub dropped_clips: Vec<DroppedClip>,
    /// Values replaced by their default, items skipped.
    pub issues: Vec<Issue>,
}

impl Library {
    /// Parse and load a library file.
    pub fn parse(text: &str) -> Result<(Self, LoadReport), ParseError> {
        Ok(Self::from_json(&json::parse(text)?))
    }

    /// Load as the app does (`fixPatterns` then `fixSongs`): every pattern gets
    /// a unique id (a missing one is made from its name), then songs are
    /// checked against those ids.
    pub fn from_json(value: &Value) -> (Self, LoadReport) {
        let mut report = LoadReport::default();
        let infos = value.get("infos").and_then(Value::as_object).cloned().unwrap_or_default();

        let raw_patterns = value.get("patterns").and_then(Value::as_array).unwrap_or_default();
        let mut patterns: Vec<Pattern> = raw_patterns
            .iter()
            .enumerate()
            .filter_map(|(i, raw)| Pattern::from_json(raw, &format!("patterns[{i}]"), &mut report.issues))
            .collect();
        assign_pattern_ids(&mut patterns);

        let known_ids = patterns.iter().map(|p| p.id.clone()).collect();
        let (songs, dropped) = songs_from_json(value.get("songs").unwrap_or(&Value::Null), &known_ids);
        report.dropped_clips = dropped;
        (Self { infos, songs, patterns }, report)
    }

    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("infos", Value::Object(self.infos.clone()));
        object.insert("songs", Value::Array(self.songs.iter().map(Song::to_json).collect()));
        object.insert("patterns", Value::Array(self.patterns.iter().map(Pattern::to_json).collect()));
        Value::Object(object)
    }

    /// The file text, indented like the app writes it (2 spaces, final newline).
    pub fn to_file_text(&self) -> String {
        self.to_json().to_json_pretty(2) + "\n"
    }

    pub fn pattern(&self, id: &str) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }
}

/// Give every pattern an id, in file order, as `ensurePatternId()`: an existing
/// id is kept, a missing one is the slug of the name made unique against the
/// ids met so far.
fn assign_pattern_ids(patterns: &mut [Pattern]) {
    let mut taken = HashSet::new();
    for pattern in patterns {
        if pattern.id.is_empty() {
            pattern.id = unique_id(&slugify(pattern.name.as_deref().unwrap_or_default()), &mut taken);
        } else {
            taken.insert(pattern.id.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assigns_missing_ids_from_names() {
        let (library, report) =
            Library::parse(r#"{"patterns":[{"name":"Rock"},{"name":"Rock"},{"id":"x"},{},{"name":"***"}]}"#).unwrap();
        let ids: Vec<_> = library.patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["rock", "rock-2", "x", "pattern", "pattern-2"]);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn songs_see_the_assigned_ids() {
        let (library, report) = Library::parse(
            r#"{"songs":[{"clips":[{"pattern":"rock"},{"pattern":"jazz"}]}],"patterns":[{"name":"Rock"}]}"#,
        )
        .unwrap();
        assert_eq!(library.songs[0].clips.len(), 1);
        assert_eq!(report.dropped_clips, [DroppedClip { pattern: "jazz".into(), reason: "unknown pattern id" }]);
        assert!(library.pattern("rock").is_some());
    }
}
