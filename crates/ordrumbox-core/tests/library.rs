//! A library loaded then saved, against its Rust snapshot: `song.json` (the
//! demo library of the web version) and `edge_library.json` (hand-written:
//! repairs, dropped clips, badly typed values). The snapshots,
//! `library_rust.json` and `edge_library_rust.json`, are what the code saves
//! of them; `write_the_snapshots` rewrites them when a load or a save changes
//! on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{DroppedClip, Library, LoadReport};

/// Each input and the snapshot of its save.
const SNAPSHOTS: [(&str, &str); 2] =
    [("song.json", "library_rust.json"), ("edge_library.json", "edge_library_rust.json")];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn read(name: &str) -> String {
    fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn read_json(name: &str) -> Value {
    json::parse(&read(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load(name: &str) -> (Library, LoadReport) {
    Library::from_json(&read_json(name))
}

/// First difference between two values, key order included.
fn diff(actual: &Value, expected: &Value, at: &str) -> Option<String> {
    match (actual, expected) {
        (Value::Object(a), Value::Object(e)) => {
            let keys = |o: &Object| o.iter().map(|(k, _)| k.to_owned()).collect::<Vec<_>>();
            if keys(a) != keys(e) {
                return Some(format!("{at}: keys {:?} vs {:?}", keys(a), keys(e)));
            }
            a.iter().find_map(|(k, v)| e.get(k).and_then(|ev| diff(v, ev, &format!("{at}.{k}"))))
        }
        (Value::Array(a), Value::Array(e)) if a.len() == e.len() => {
            a.iter().zip(e).enumerate().find_map(|(i, (av, ev))| diff(av, ev, &format!("{at}[{i}]")))
        }
        _ if actual == expected => None,
        _ => Some(format!("{at}: {actual} vs snapshot {expected}")),
    }
}

/// Writes the snapshots from the code as it is: `cargo test -p ordrumbox-core
/// --test library -- --ignored`, when a load or a save changes on purpose
/// (`DEVELOPMENT.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshots"]
fn write_the_snapshots() {
    for (input, snapshot) in SNAPSHOTS {
        fs::write(fixtures().join(snapshot), load(input).0.to_file_text()).expect("written");
    }
}

#[test]
fn a_loaded_library_saves_its_snapshot() {
    for (input, snapshot) in SNAPSHOTS {
        let saved = load(input).0.to_file_text();
        let expected = read(snapshot);
        if saved != expected {
            let difference = diff(&json::parse(&saved).expect("saved text parses"), &read_json(snapshot), "");
            panic!("{input} against {snapshot}: {}", difference.as_deref().unwrap_or("same values, other text"));
        }
    }
}

#[test]
fn song_json_loads_without_issue() {
    let (library, report) = load("song.json");
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
    assert!(report.dropped_clips.is_empty(), "{:#?}", report.dropped_clips);
    assert_eq!(library.patterns.len(), 42);
}

#[test]
fn edge_cases_drop_the_clips_they_cannot_play() {
    let (_, report) = load("edge_library.json");
    let dropped = |pattern: &str, reason| DroppedClip { pattern: pattern.to_owned(), reason };
    assert_eq!(
        report.dropped_clips,
        [
            dropped("ghost", "unknown pattern id"),
            dropped("#3", "clip has no pattern"),
            dropped("#4", "clip is not an object"),
            dropped("#5", "clip is not an object"),
        ]
    );
}

#[test]
fn edge_cases_repair_and_report() {
    let (library, report) = load("edge_library.json");
    let odd = library.patterns[0].tracks.iter().find(|t| t.name == "ODD").expect("ODD track");
    assert_eq!((odd.notes[0].beat, odd.notes[0].velocity), (1, 2.0));
    assert!(!odd.mono);
    let messages: Vec<String> = report.issues.iter().map(|i| format!("{}: {}", i.at, i.message)).collect();
    assert_eq!(
        messages,
        [
            "patterns[0].tracks[0].notes[0].stepPercent: not a percent from 0 to 100 (999), derived from the step",
            "patterns[0].tracks[4].pan: not a number (\"left\"), pan taken from the track name",
            "patterns[0].tracks[4].mono: wrong type (\"yes\"), default kept",
            "patterns[0].tracks[4].notes[0]: more values than noteKeys, extra values ignored",
            "songs[1].bpm: 10 outside 20–300, 120 used",
            "songs[2].bpm: 301 outside 20–300, 120 used",
            "songs[3]: \"clips\" must be an array, skipped",
            "songs[4]: Each song must be a JSON object, skipped",
        ]
    );
    // An id with spaces is trimmed, so that clips can name it; tags saved as
    // an object (`{"0": "a"}`, a past bug) are read as a list.
    assert_eq!(library.patterns[3].id, "spaced");
    assert_eq!(library.patterns[1].tags, ["legacy", "object"]);
}

/// Saving rounds some values to 2 decimals (as the JS exporter does), so the
/// first save may change the model; from then on save/load is stable.
#[test]
fn saving_is_stable() {
    for name in ["song.json", "edge_library.json"] {
        let (library, _) = load(name);
        let first = library.to_file_text();
        let (reloaded, report) = Library::parse(&first).expect("saved text parses");
        assert!(report.issues.is_empty(), "{name}: {:#?}", report.issues);
        assert!(report.dropped_clips.is_empty(), "{name}: {:#?}", report.dropped_clips);
        let second = reloaded.to_file_text();
        assert_eq!(second, first, "{name}");
        assert_eq!(Library::parse(&second).expect("parses").0, reloaded, "{name}");
    }
}

#[test]
fn infos_are_kept_as_written() {
    let (library, _) = load("song.json");
    let infos: &Object = &library.info;
    assert_eq!(infos.get("name").and_then(Value::as_str), Some("orDrumbox Demo lib"));
}

#[test]
fn a_saved_library_loads_back_to_the_same_file() {
    // song.json has the web version's key names; the save writes the current
    // ones. The file, not the model, is compared: a save rounds some values
    // (0.792 -> 0.79).
    for name in ["song.json", "edge_library.json"] {
        let (library, _) = load(name);
        let saved = library.to_json();
        let (reloaded, report) = Library::from_json(&saved);
        assert!(report.issues.is_empty(), "{name}: {:#?}", report.issues);
        if let Some(difference) = diff(&reloaded.to_json(), &saved, name) {
            panic!("{difference}");
        }
    }
}
