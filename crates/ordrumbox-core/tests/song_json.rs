//! The JSON layer on the library files: reading then writing gives the input
//! back, and writing what was read from a write gives the same text.

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

/// Writing is idempotent, compact and pretty: the text of a value read back
/// from its own text is that text, and the value is the same.
#[test]
fn writing_is_idempotent() {
    for name in ["song.json", "edge_library.json"] {
        let value = json::parse(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (form, write) in [("compact", Value::to_json as fn(&Value) -> String), ("pretty", |v| v.to_json_pretty(2))]
        {
            let first = write(&value);
            let reread = json::parse(&first).unwrap_or_else(|e| panic!("{name}, {form}: {e}"));
            assert_eq!(reread, value, "{name}, {form}: the text does not hold the value");
            assert_same_text(&write(&reread), &first, &format!("{name}, {form}"));
        }
    }
}
