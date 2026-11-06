//! Note sub-positions (the piano roll's ghosts) against their Rust snapshot,
//! `sub_positions_rust.txt`: every track of `song.json` that has notes, and
//! hand-made tracks (retriggers at every rate, euclidean fills and rotations,
//! loop points, every arpeggio form). `write_the_snapshot` rewrites it when
//! where the ghosts fall changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json;
use ordrumbox_core::note_positions::{SubKind, sub_positions};
use ordrumbox_core::pattern_engine::SpanResolver;
use ordrumbox_core::{Library, Rng, Track};

/// The seed a `random` arpeggio draws its order from.
const SEED: u64 = 1;

/// Tracks `song.json` does not reach.
const HAND_TRACKS: [&str; 10] = [
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"retriggerCount":4,"retriggerRate":2},{"beat":2,"retriggerRate":10,"retriggerCount":3}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"euclideanPulses":3},{"beat":2,"euclideanPulses":5,"euclideanRotation":2}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":8,"loopLength":10,"notes":[{"euclideanPulses":4,"euclideanRotation":1}]}"#,
    r#"{"stepsPerBeat":3,"beatCount":4,"notes":[{"arp":[0,4,7],"retriggerCount":6,"retriggerRate":4}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"arp":"updown","retriggerCount":9,"retriggerRate":2,"euclideanPulses":2}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"arp":{"intervals":[3,7,10],"mode":"down"},"retriggerCount":5,"retriggerRate":3}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"arp":"0,5,9","retriggerCount":40,"retriggerRate":1}]}"#,
    r#"{"stepsPerBeat":2,"beatCount":3,"notes":[{"beat":1,"step":1,"retriggerCount":3,"retriggerRate":8,"euclideanPulses":7}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"arp":"random","retriggerCount":4,"retriggerRate":2}]}"#,
    r#"{"stepsPerBeat":4,"beatCount":4,"notes":[{"arp":"nonsense","retriggerCount":3,"retriggerRate":2}]}"#,
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// One line a note: `<at> note <n>:` then each sub-position, its step, `r`
/// (retrigger or arpeggio step) or `e` (euclidean fill), and its pitch offset.
fn track_lines(track: &Track, at: &str, lines: &mut Vec<String>) {
    let total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let span = SpanResolver::new(track);
    for (n, note) in track.notes.iter().enumerate() {
        let mut line = format!("{at} note {n}:");
        for sub in sub_positions(note, track, total, &span, &mut Rng::new(SEED)) {
            let kind = match sub.kind {
                SubKind::Retrigger => 'r',
                SubKind::Euclidean => 'e',
            };
            write!(line, " {}{kind}{:+}", sub.step, sub.pitch_offset).unwrap();
        }
        lines.push(line);
    }
}

fn rust_snapshot() -> Vec<String> {
    let text = fs::read_to_string(fixtures().join("song.json")).expect("song.json");
    let (library, _) = Library::from_json(&json::parse(&text).expect("song.json parses"));
    let mut lines = Vec::new();
    for (p, pattern) in library.patterns.iter().enumerate() {
        for (t, track) in pattern.tracks.iter().enumerate().filter(|(_, t)| !t.notes.is_empty()) {
            track_lines(track, &format!("pattern {p} track {t}"), &mut lines);
        }
    }
    for (i, track) in HAND_TRACKS.iter().enumerate() {
        let (library, report) = Library::parse(&format!(r#"{{"patterns":[{{"tracks":[{track}]}}]}}"#)).unwrap();
        assert!(report.issues.is_empty(), "hand track {i}: {:#?}", report.issues);
        track_lines(&library.patterns[0].tracks[0], &format!("hand track {i}"), &mut lines);
    }
    lines
}

fn snapshot_path() -> PathBuf {
    fixtures().join("sub_positions_rust.txt")
}

/// Writes `sub_positions_rust.txt` from the code as it is: `cargo test -p
/// ordrumbox-core --test sub_positions -- --ignored`, when where the ghosts
/// fall changes on purpose (`README.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn sub_positions_match_their_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/sub_positions_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}
