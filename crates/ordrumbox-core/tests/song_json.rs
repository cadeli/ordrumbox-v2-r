//! The JSON layer against files produced by the JS app (tools/export_fixtures.mjs).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Value};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn read(name: &str) -> String {
    fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Byte-for-byte comparison that reports where the texts part, instead of
/// dumping two 200 KB strings. A CRLF checkout shows up as `"\r\n"` vs `"\n"`.
#[track_caller]
fn assert_same_text(written: &str, expected: &str, what: &str) {
    if written == expected {
        return;
    }
    let at =
        written.bytes().zip(expected.bytes()).position(|(a, b)| a != b).unwrap_or(written.len().min(expected.len()));
    let excerpt = |s: &str| {
        let start = char_boundary(s, at.saturating_sub(30));
        let end = char_boundary(s, at + 30);
        format!("{:?}", &s[start..end])
    };
    let line = expected[..char_boundary(expected, at)].matches('\n').count() + 1;
    let crlf_hint = if expected.contains("\r\n") {
        "\n(the expected file has CRLF line endings: check .gitattributes)"
    } else {
        ""
    };
    panic!(
        "{what}: texts differ at byte {at} (line {line}); lengths {} written vs {} expected\n  written:  {}\n  expected: {}{crlf_hint}",
        written.len(),
        expected.len(),
        excerpt(written),
        excerpt(expected),
    );
}

/// The char boundary at or before byte `i` (clamped to the text length).
fn char_boundary(s: &str, i: usize) -> usize {
    (0..=i.min(s.len())).rev().find(|&b| s.is_char_boundary(b)).unwrap_or(0)
}

#[test]
fn song_json_pretty_round_trip_is_byte_identical() {
    // song.json is written by the app as JSON.stringify(lib, null, 2) + "\n".
    let raw = read("song.json");
    let value = json::parse(&raw).expect("song.json parses");
    assert_same_text(&(value.to_json_pretty(2) + "\n"), &raw, "song.json");
}

#[test]
fn song_json_compact_matches_json_stringify() {
    let value = json::parse(&read("song.json")).expect("song.json parses");
    assert_same_text(&value.to_json(), &read("song.min.json"), "song.min.json");
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
        assert_same_text(&(value.to_json() + "\n"), &raw, &path.display().to_string());
        count += 1;
    }
    assert!(count > 0, "no fixture in {}", dir.display());
}
