//! The demos a new version brings: the patterns and songs of the starter library that the user's
//! library has not met yet, added to it. A demo the user has seen is never added again, so one they
//! deleted stays deleted, and one they changed is never replaced.

use std::collections::HashSet;

use ordrumbox_core::Library;

/// The ids of the starter patterns and songs the user's library has been offered.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KnownDemos {
    pub pattern_ids: Vec<String>,
    pub song_ids: Vec<String>,
}

impl KnownDemos {
    /// Every pattern and song of `starter`.
    #[must_use]
    pub fn of(starter: &Library) -> Self {
        Self {
            pattern_ids: starter.patterns.iter().map(|p| p.id.clone()).collect(),
            song_ids: starter.songs.iter().map(|s| s.id.clone()).collect(),
        }
    }
}

/// What [`add_new_demos`] added, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AddedDemos {
    pub pattern_names: Vec<String>,
    pub song_names: Vec<String>,
}

impl AddedDemos {
    /// Nothing was added.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pattern_names.is_empty() && self.song_names.is_empty()
    }
}

/// Add to `library` the patterns and songs of `starter` that are not in `known`, and not already in
/// `library` under the same id (the user's wins). A new song brings the patterns it plays that
/// `library` lacks, known or not.
pub fn add_new_demos(library: &mut Library, starter: &Library, known: &KnownDemos) -> AddedDemos {
    let known_patterns: HashSet<&str> = known.pattern_ids.iter().map(String::as_str).collect();
    let known_songs: HashSet<&str> = known.song_ids.iter().map(String::as_str).collect();
    let mut added = AddedDemos::default();

    for song in &starter.songs {
        if known_songs.contains(song.id.as_str()) || library.song_index(&song.id).is_some() {
            continue;
        }
        let brought = library.add_song_from(song, starter);
        added.pattern_names.extend(brought.iter().map(|p| p.name.clone().unwrap_or_default()));
        added.song_names.push(song.name.clone());
    }
    for pattern in &starter.patterns {
        if known_patterns.contains(pattern.id.as_str()) || library.pattern(&pattern.id).is_some() {
            continue;
        }
        added.pattern_names.push(pattern.name.clone().unwrap_or_default());
        library.patterns.push(pattern.clone());
    }
    added
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(text: &str) -> Library {
        Library::parse(text).expect("a library").0
    }

    const STARTER: &str = r#"{"patterns":[{"id":"a","name":"A"},{"id":"b","name":"B"},{"id":"c","name":"C"}],
        "songs":[{"id":"s","name":"S","clips":[{"patternId":"c","startMeasure":0,"measureCount":1}]}]}"#;

    #[test]
    fn a_new_pattern_is_added_and_a_known_one_is_not() {
        let starter = library(STARTER);
        let mut mine = library(r#"{"patterns":[{"id":"a","name":"A"}]}"#);
        let known = KnownDemos { pattern_ids: vec!["a".into(), "b".into()], song_ids: vec!["s".into()] };
        let added = add_new_demos(&mut mine, &starter, &known);
        assert_eq!(added.pattern_names, ["C"]);
        let ids: Vec<_> = mine.patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"], "b was deleted by the user and stays so");
    }

    #[test]
    fn a_new_song_brings_the_patterns_it_plays() {
        let starter = library(STARTER);
        let mut mine = library(r#"{"patterns":[{"id":"a","name":"A"}]}"#);
        let known = KnownDemos { pattern_ids: vec!["a".into(), "b".into(), "c".into()], song_ids: vec![] };
        let added = add_new_demos(&mut mine, &starter, &known);
        assert_eq!(added.song_names, ["S"]);
        assert_eq!(added.pattern_names, ["C"], "c is known, but the song needs it");
        assert!(mine.songs[0].clips.len() == 1 && mine.pattern("c").is_some());
    }

    #[test]
    fn the_users_pattern_of_the_same_id_wins() {
        let starter = library(STARTER);
        let mut mine = library(r#"{"patterns":[{"id":"c","name":"My C"}]}"#);
        let added = add_new_demos(&mut mine, &starter, &KnownDemos::default());
        assert_eq!(mine.pattern("c").and_then(|p| p.name.as_deref()), Some("My C"));
        assert_eq!(added.pattern_names, ["A", "B"]);
    }

    #[test]
    fn nothing_new_changes_nothing() {
        let starter = library(STARTER);
        let mut mine = library(r#"{"patterns":[]}"#);
        let before = mine.clone();
        assert!(add_new_demos(&mut mine, &starter, &KnownDemos::of(&starter)).is_empty());
        assert_eq!(mine, before);
    }
}
