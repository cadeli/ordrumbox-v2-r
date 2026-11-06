//! The chord progressions of the keyboard menu's "Add sequence" against their
//! Rust snapshot, `composition_rust.txt`: every progression, indices past
//! both ends, several tonics and pattern lengths. `write_the_snapshot`
//! rewrites it when a progression changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use ordrumbox_logic::composition::{PROGRESSIONS, progression, progression_notes};

const TONICS: [f64; 3] = [0.0, -5.0, 7.0];
const BEAT_COUNTS: [i32; 7] = [0, 1, 3, 4, 7, 8, 16];

/// One line a case: `<index> <id> <name> tonic <t> beats <b>:` then each
/// note, `beat:pitch`.
fn rust_snapshot() -> Vec<String> {
    let len = PROGRESSIONS.len() as i64;
    let mut lines = Vec::new();
    for index in -2..len + 2 {
        let s = progression(index);
        for tonic in TONICS {
            for beat_count in BEAT_COUNTS {
                let mut line = format!("{index} {} {} tonic {tonic} beats {beat_count}:", s.id, s.name);
                for (beat, pitch) in progression_notes(s, tonic, beat_count) {
                    write!(line, " {beat}:{pitch}").unwrap();
                }
                lines.push(line);
            }
        }
    }
    lines
}

fn snapshot_path() -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/composition_rust.txt")
}

/// Writes `composition_rust.txt` from the progressions as they are: `cargo
/// test -p ordrumbox-logic --test composition -- --ignored`, when a
/// progression changes on purpose (`README.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn progressions_match_their_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/composition_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}
