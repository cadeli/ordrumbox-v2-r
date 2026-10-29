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
    pub info: Object,
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
        (Self { info: infos, songs, patterns }, report)
    }

    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("infos", Value::Object(self.info.clone()));
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

    /// A song file (`SongService.buildSongData`, the record the song panel
    /// saves and exports as `.odbox`): the header's description and date,
    /// the patterns, the arrangements, what is selected, and `stamp` (key
    /// and time in ms: `savedAt` or `exportedAt`).
    pub fn song_record(&self, name: &str, selected_pattern: usize, selected_song: usize, stamp: (&str, f64)) -> Value {
        let info = |key: &str| self.info.get(key).cloned().unwrap_or_else(|| Value::from(""));
        let mut object = Object::new();
        object.insert("name", Value::from(name));
        object.insert("description", info("description"));
        object.insert("date", info("date"));
        object.insert("patterns", Value::Array(self.patterns.iter().map(Pattern::to_json).collect()));
        object.insert("selectedPatternIdx", Value::from(selected_pattern as f64));
        object.insert("songs", Value::Array(self.songs.iter().map(Song::to_json).collect()));
        object.insert("selectedSongIdx", Value::from(selected_song as f64));
        object.insert(stamp.0, Value::from(stamp.1));
        Value::Object(object)
    }

    /// Read a song file (`parseImportedFile`, then `applyToAppState`):
    /// `None` without a `patterns` array. Its name, description and date
    /// replace those of `info`, the header kept otherwise.
    pub fn from_song_record(value: &Value, info: &Object) -> Option<SongRecord> {
        value.get("patterns").and_then(Value::as_array)?;
        let (mut library, report) = Self::from_json(value);
        let text = |key: &str| value.get(key).filter(|v| !v.is_null()).map(crate::js::to_string);
        let name = text("name");
        library.info = info.clone();
        for (key, v) in [("description", text("description")), ("date", text("date"))] {
            library.info.insert(key, Value::from(v.unwrap_or_default().as_str()));
        }
        let index = |key: &str| {
            let v = value.get(key).map_or(0.0, crate::js::to_number);
            if v > 0.0 { v as usize } else { 0 }
        };
        Some(SongRecord {
            name,
            library,
            report,
            selected_pattern: index("selectedPatternIdx"),
            selected_song: index("selectedSongIdx"),
        })
    }
}

/// A song file read back ([`Library::from_song_record`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SongRecord {
    /// Its name, when it has one (else the file's).
    pub name: Option<String>,
    pub library: Library,
    pub report: LoadReport,
    pub selected_pattern: usize,
    pub selected_song: usize,
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
    fn song_records_round_trip() {
        let (library, _) = Library::parse(
            r#"{"infos":{"name":"lib","date":"2026","url":"u"},"songs":[{"name":"s","clips":[{"pattern":"b"}]}],
                "patterns":[{"name":"a"},{"name":"b"}]}"#,
        )
        .unwrap();
        let record = library.song_record("My song", 1, 0, ("savedAt", 5.0));
        let keys: Vec<_> = record.as_object().unwrap().iter().map(|(k, _)| k.to_owned()).collect();
        assert_eq!(
            keys,
            ["name", "description", "date", "patterns", "selectedPatternIdx", "songs", "selectedSongIdx", "savedAt"]
        );
        let read = Library::from_song_record(&record, &library.info).unwrap();
        assert_eq!(read.name.as_deref(), Some("My song"));
        assert_eq!((read.selected_pattern, read.selected_song), (1, 0));
        assert_eq!(read.library.songs, library.songs);
        assert_eq!(read.library.patterns, library.patterns);
        assert_eq!(read.library.info.get("url").and_then(Value::as_str), Some("u"));
        assert_eq!(read.library.info.get("description").and_then(Value::as_str), Some(""));
        assert!(Library::from_song_record(&json::parse(r#"{"songs":[]}"#).unwrap(), &Object::new()).is_none());
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
