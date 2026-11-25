//! A library file (`assets/data/song.json`): `{info, songs, patterns}`, the
//! patterns being shared by every song of the file, and the synth presets
//! saved with it (`synthPresets`).

use std::collections::HashSet;

use crate::ids::{slugify, unique_id};
use crate::json::{self, Object, ParseError, Value};
use crate::pattern::Pattern;
use crate::record::Issue;
use crate::song::{DEFAULT_BPM, DroppedClip, Song, bpm_from_json, songs_from_json};

#[derive(Debug, Clone, PartialEq)]
pub struct Library {
    /// Free-form header (`name`, `description`, `date`…), kept as written.
    pub info: Object,
    /// The session's tempo: every pattern plays at it, a song at its own.
    pub bpm: f64,
    /// The name of the kit the tracks draw their samples from; `None` for a
    /// file saved before the kit was kept (the kit in use stays).
    pub kit: Option<String>,
    pub songs: Vec<Song>,
    pub patterns: Vec<Pattern>,
    /// The synth presets saved with the library, by key, over the assets'
    /// ones: `null` for one of the assets' deleted. `None` for a file saved
    /// before they were.
    pub synths: Option<Object>,
}

impl Default for Library {
    fn default() -> Self {
        Self { info: Object::new(), bpm: DEFAULT_BPM, kit: None, songs: Vec::new(), patterns: Vec::new(), synths: None }
    }
}

/// The key of the synth presets in a library file.
const SYNTHS_KEY: &str = "synthPresets";
/// The key of the kit in a library or project file.
const KIT_KEY: &str = "kit";

/// The kit a library or project file names, if any: a value that is not a
/// name is reported and left out.
fn kit_from_json(value: &Value, issues: &mut Vec<Issue>) -> Option<String> {
    let raw = value.get(KIT_KEY)?;
    let name = raw.as_str().filter(|name| !name.is_empty());
    if name.is_none() {
        issues.push(Issue::new(KIT_KEY, format!("not a kit name ({}), the kit in use kept", raw.to_json())));
    }
    name.map(str::to_owned)
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
        // `infos` in files from the JS app and older saves.
        let info = ["info", "infos"]
            .into_iter()
            .find_map(|key| value.get(key).and_then(Value::as_object))
            .cloned()
            .unwrap_or_default();

        let raw_patterns = value.get("patterns").and_then(Value::as_array).unwrap_or_default();
        // A file older than the session tempo has one per pattern: the first plays on.
        let bpm = match (value.get("bpm"), raw_patterns.first().and_then(|p| p.get("bpm"))) {
            (Some(raw), _) => bpm_from_json(raw, "bpm", &mut report.issues),
            (None, Some(raw)) => bpm_from_json(raw, "patterns[0].bpm", &mut report.issues),
            (None, None) => DEFAULT_BPM,
        };
        let mut patterns: Vec<Pattern> = raw_patterns
            .iter()
            .enumerate()
            .filter_map(|(i, raw)| Pattern::from_json(raw, &format!("patterns[{i}]"), &mut report.issues))
            .collect();
        assign_pattern_ids(&mut patterns);

        let known_ids = patterns.iter().map(|p| p.id.clone()).collect();
        let (songs, dropped) =
            songs_from_json(value.get("songs").unwrap_or(&Value::Null), &known_ids, &mut report.issues);
        report.dropped_clips = dropped;
        let synths = value.get(SYNTHS_KEY).map(|raw| synths_from_json(raw, &mut report.issues));
        let kit = kit_from_json(value, &mut report.issues);
        (Self { info, bpm, kit, songs, patterns, synths }, report)
    }

    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("info", Value::Object(self.info.clone()));
        object.insert("bpm", Value::from(self.bpm));
        if let Some(kit) = &self.kit {
            object.insert(KIT_KEY, Value::from(kit.as_str()));
        }
        object.insert("songs", Value::Array(self.songs.iter().map(Song::to_json).collect()));
        object.insert("patterns", Value::Array(self.patterns.iter().map(Pattern::to_json).collect()));
        if let Some(synths) = &self.synths {
            object.insert(SYNTHS_KEY, Value::Object(synths.clone()));
        }
        Value::Object(object)
    }

    /// The file text, indented like the app writes it (2 spaces, final newline).
    pub fn to_file_text(&self) -> String {
        self.to_json().to_json_pretty(2) + "\n"
    }

    pub fn pattern(&self, id: &str) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    /// Add `song`, with the patterns of `source` it plays that this library
    /// lacks (those it holds under the same id are kept); the patterns added.
    pub fn add_song_from(&mut self, song: &Song, source: &Self) -> &[Pattern] {
        let before = self.patterns.len();
        for clip in &song.clips {
            if self.pattern(&clip.pattern_id).is_none()
                && let Some(pattern) = source.pattern(&clip.pattern_id)
            {
                self.patterns.push(pattern.clone());
            }
        }
        self.songs.push(song.clone());
        &self.patterns[before..]
    }

    /// A project file (`SongService.buildSongData`, the record the song panel
    /// saves and exports as `.odbox`): the header's description and date,
    /// the session tempo, the kit, the patterns, the arrangements, what is selected, and `stamp` (key
    /// and time in ms: `savedAt` or `exportedAt`).
    pub fn project_record(
        &self,
        name: &str,
        selected_pattern: usize,
        selected_song: usize,
        stamp: (&str, f64),
    ) -> Value {
        let info = |key: &str| self.info.get(key).cloned().unwrap_or_else(|| Value::from(""));
        let mut object = Object::new();
        object.insert("name", Value::from(name));
        object.insert("description", info("description"));
        object.insert("date", info("date"));
        object.insert("bpm", Value::from(self.bpm));
        if let Some(kit) = &self.kit {
            object.insert(KIT_KEY, Value::from(kit.as_str()));
        }
        object.insert("patterns", Value::Array(self.patterns.iter().map(Pattern::to_json).collect()));
        object.insert("selectedPattern", Value::from(selected_pattern as f64));
        object.insert("songs", Value::Array(self.songs.iter().map(Song::to_json).collect()));
        object.insert("selectedSong", Value::from(selected_song as f64));
        object.insert(stamp.0, Value::from(stamp.1));
        Value::Object(object)
    }

    /// Read a project file (`parseImportedFile`, then `applyToAppState`):
    /// `None` without a `patterns` array. Its name, description and date
    /// replace those of `info`, the header kept otherwise.
    pub fn from_project_record(value: &Value, info: &Object) -> Option<ProjectRecord> {
        value.get("patterns").and_then(Value::as_array)?;
        let (mut library, report) = Self::from_json(value);
        let text = |key: &str| value.get(key).and_then(Value::to_text);
        let name = text("name");
        library.info = info.clone();
        for (key, v) in [("description", text("description")), ("date", text("date"))] {
            library.info.insert(key, Value::from(v.unwrap_or_default().as_str()));
        }
        // `selectedPatternIdx`, `selectedSongIdx`: the JS keys, older saves.
        let index = |keys: [&str; 2]| {
            let v = keys.iter().find_map(|key| value.get(key)).and_then(Value::to_number).unwrap_or(0.0);
            if v > 0.0 { v as usize } else { 0 }
        };
        Some(ProjectRecord {
            name,
            library,
            report,
            selected_pattern: index(["selectedPattern", "selectedPatternIdx"]),
            selected_song: index(["selectedSong", "selectedSongIdx"]),
        })
    }
}

/// A project file read back ([`Library::from_project_record`]).
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectRecord {
    /// Its name, when it has one (else the file's).
    pub name: Option<String>,
    pub library: Library,
    pub report: LoadReport,
    pub selected_pattern: usize,
    pub selected_song: usize,
}

/// The synth presets of a library file: an object of presets (objects), or
/// `null` for one deleted; anything else is left out and reported.
fn synths_from_json(raw: &Value, issues: &mut Vec<Issue>) -> Object {
    let Some(object) = raw.as_object() else {
        issues.push(Issue::new(SYNTHS_KEY, "not an object: no synth preset read"));
        return Object::new();
    };
    let mut synths = Object::new();
    for (key, preset) in object.iter() {
        if matches!(preset, Value::Object(_) | Value::Null) {
            synths.insert(key, preset.clone());
        } else {
            issues.push(Issue::new(format!("{SYNTHS_KEY}.{key}"), "not a synth preset: left out"));
        }
    }
    synths
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
    fn project_records_round_trip() {
        let (library, _) = Library::parse(
            r#"{"infos":{"name":"lib","date":"2026","url":"u"},"bpm":97,"songs":[{"name":"s","clips":[{"pattern":"b"}]}],
                "patterns":[{"name":"a"},{"name":"b"}]}"#,
        )
        .unwrap();
        let record = library.project_record("My song", 1, 0, ("savedAt", 5.0));
        let keys: Vec<_> = record.as_object().unwrap().iter().map(|(k, _)| k.to_owned()).collect();
        assert_eq!(
            keys,
            ["name", "description", "date", "bpm", "patterns", "selectedPattern", "songs", "selectedSong", "savedAt"]
        );
        let read = Library::from_project_record(&record, &library.info).unwrap();
        assert_eq!(read.name.as_deref(), Some("My song"));
        assert_eq!((read.selected_pattern, read.selected_song), (1, 0));
        assert_eq!(read.library.bpm, 97.0);
        assert_eq!(read.library.songs, library.songs);
        assert_eq!(read.library.patterns, library.patterns);
        assert_eq!(read.library.info.get("url").and_then(Value::as_str), Some("u"));
        assert_eq!(read.library.synths, None, "a project file holds no synth presets");
        assert_eq!(read.library.info.get("description").and_then(Value::as_str), Some(""));
        assert!(Library::from_project_record(&json::parse(r#"{"songs":[]}"#).unwrap(), &Object::new()).is_none());
    }

    #[test]
    fn synth_presets_are_kept_and_a_wrong_one_is_left_out() {
        let text = r#"{"synthPresets":{"a":{"vco1":{"gain":0.5}},"gone":null,"bad":3},"patterns":[]}"#;
        let (library, report) = Library::parse(text).unwrap();
        let synths = library.synths.as_ref().unwrap();
        assert_eq!(synths.iter().map(|(k, _)| k).collect::<Vec<_>>(), ["a", "gone"]);
        assert_eq!(report.issues, [Issue::new("synthPresets.bad", "not a synth preset: left out")]);
        let again = Library::parse(&library.to_file_text()).unwrap().0;
        assert_eq!(again, library);
        assert_eq!(Library::parse("{}").unwrap().0.synths, None, "a file from before");
        let (library, report) = Library::parse(r#"{"synthPresets":[]}"#).unwrap();
        assert_eq!(library.synths, Some(Object::new()));
        assert_eq!(report.issues.len(), 1);
    }

    #[test]
    fn the_kit_is_kept_in_a_library_and_a_project() {
        let (library, report) = Library::parse(r#"{"kit":"human","patterns":[]}"#).unwrap();
        assert_eq!((library.kit.as_deref(), report.issues.len()), (Some("human"), 0));
        assert_eq!(library.to_json().get("kit").cloned(), Some(Value::from("human")));
        let record = library.project_record("p", 0, 0, ("savedAt", 0.0));
        let read = Library::from_project_record(&record, &Object::new()).unwrap();
        assert_eq!(read.library.kit.as_deref(), Some("human"));
        let (none, _) = Library::parse(r#"{"patterns":[]}"#).unwrap();
        assert_eq!((none.kit.as_deref(), none.to_json().get("kit").cloned()), (None, None), "an older file: no kit");
        let (wrong, report) = Library::parse(r#"{"kit":3,"patterns":[]}"#).unwrap();
        assert_eq!(wrong.kit, None);
        assert_eq!(report.issues, [Issue::new("kit", "not a kit name (3), the kit in use kept")]);
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

    #[test]
    fn a_song_brings_the_patterns_it_plays_that_are_missing() {
        let (source, _) = Library::parse(
            r#"{"patterns":[{"id":"a","name":"A"},{"id":"b","name":"B"},{"id":"c"}],
            "songs":[{"id":"s","clips":[{"patternId":"a","startMeasure":0,"measureCount":1},
                {"patternId":"b","startMeasure":1,"measureCount":1},{"patternId":"a","startMeasure":2,"measureCount":1}]}]}"#,
        )
        .unwrap();
        let (mut mine, _) = Library::parse(r#"{"patterns":[{"id":"b","name":"My B"}]}"#).unwrap();
        let added: Vec<&str> = mine.add_song_from(&source.songs[0], &source).iter().map(|p| p.id.as_str()).collect();
        assert_eq!(added, ["a"], "b is kept as the user's, a added once, c not played");
        assert_eq!(mine.pattern("b").and_then(|p| p.name.as_deref()), Some("My B"));
        assert_eq!(mine.songs.len(), 1);
    }

    #[test]
    fn the_session_tempo_is_the_files_else_the_first_patterns() {
        let bpm = |text: &str| {
            let (library, report) = Library::parse(text).unwrap();
            (library.bpm, report.issues)
        };
        assert_eq!(bpm(r#"{"bpm":90,"patterns":[{"bpm":140}]}"#), (90.0, vec![]));
        assert_eq!(bpm(r#"{"patterns":[{"bpm":140},{"bpm":70}]}"#), (140.0, vec![]));
        assert_eq!(bpm(r#"{"patterns":[{"name":"p"}]}"#), (DEFAULT_BPM, vec![]));
        assert_eq!(bpm(r#"{"bpm":400}"#), (DEFAULT_BPM, vec![Issue::new("bpm", "400 outside 20–300, 120 used")]));
        let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","bpm":140}]}"#).unwrap();
        assert_eq!(
            library.to_json().to_json(),
            r#"{"info":{},"bpm":140,"songs":[],"patterns":[{"id":"p","name":"p"}]}"#
        );
    }
}
