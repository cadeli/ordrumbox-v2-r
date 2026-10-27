//! The JSON layer against files produced by the JS app (tools/export_fixtures.mjs).

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Value};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn read(name: &str) -> String {
    fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn song_json_pretty_round_trip_is_byte_identical() {
    // song.json is written by the app as JSON.stringify(lib, null, 2) + "\n".
    let raw = read("song.json");
    let value = json::parse(&raw).expect("song.json parses");
    assert_eq!(value.to_json_pretty(2) + "\n", raw);
}

#[test]
fn song_json_compact_matches_json_stringify() {
    let value = json::parse(&read("song.json")).expect("song.json parses");
    assert_eq!(value.to_json(), read("song.min.json"));
}

#[test]
fn song_json_has_the_expected_shape() {
    let value = json::parse(&read("song.json")).expect("song.json parses");
    let patterns = value.get("patterns").and_then(Value::as_array).expect("patterns array");
    assert!(!patterns.is_empty());
    for pattern in patterns {
        assert!(pattern.get("id").and_then(Value::as_str).is_some_and(|id| !id.is_empty()));
    }
    assert!(value.get("songs").and_then(Value::as_array).is_some());
}

#[test]
fn flat_note_fixtures_round_trip() {
    let dir = fixtures().join("flat_notes");
    let mut count = 0;
    for entry in fs::read_dir(&dir).expect("flat_notes dir") {
        let path = entry.expect("dir entry").path();
        let raw = fs::read_to_string(&path).expect("fixture readable");
        let value = json::parse(&raw).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(value.to_json() + "\n", raw, "{}", path.display());
        count += 1;
    }
    assert!(count > 0, "no fixture in {}", dir.display());
}
