//! The model against the JS app: a library loaded then saved by Rust must hold
//! what `fixPatterns` + `fixSongs` + `Exporter.export` produce in JS
//! (`library.export.json`, `edge_library.export.json`, from tools/export_fixtures.mjs).
//!
//! Compared as JSON values with key order ignored: the JS exporter follows the
//! accidental key order of each object, Rust writes a fixed order.

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{DroppedClip, Library, LoadReport, ids};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn read_json(name: &str) -> Value {
    let text = fs::read_to_string(fixtures().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
    json::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn load(name: &str) -> (Library, LoadReport) {
    Library::from_json(&read_json(name))
}

/// First difference between two values, key order ignored.
fn diff(rust: &Value, js: &Value, at: &str) -> Option<String> {
    match (rust, js) {
        (Value::Object(r), Value::Object(j)) => {
            let missing =
                j.iter().find(|(k, _)| !r.contains_key(k)).map(|(k, v)| format!("{at}.{k}: missing, JS has {v}"));
            let extra = r.iter().find(|(k, _)| !j.contains_key(k)).map(|(k, v)| format!("{at}.{k}: {v}, JS has none"));
            missing
                .or(extra)
                .or_else(|| r.iter().find_map(|(k, v)| j.get(k).and_then(|jv| diff(v, jv, &format!("{at}.{k}")))))
        }
        (Value::Array(r), Value::Array(j)) if r.len() == j.len() => {
            r.iter().zip(j).enumerate().find_map(|(i, (rv, jv))| diff(rv, jv, &format!("{at}[{i}]")))
        }
        _ if rust == js => None,
        _ => Some(format!("{at}: {rust} vs JS {js}")),
    }
}

/// The JS output, minus the behaviours the port fixes on purpose:
/// - `tags` saved as `{"0": "a"}` (a past bug) are written back as an array;
/// - a pattern id with spaces is trimmed (JS trims it for the song lookup only,
///   so clips can never match it).
fn js_reference(name: &str) -> (Value, Vec<DroppedClip>) {
    let mut reference = read_json(name);
    let object = reference.as_object_mut().expect("object");
    let dropped = object
        .remove("dropped")
        .and_then(|d| d.as_array().map(<[Value]>::to_vec))
        .unwrap_or_default()
        .iter()
        .map(|d| DroppedClip {
            pattern: d.get("pattern").and_then(Value::as_str).unwrap_or_default().to_owned(),
            reason: match d.get("reason").and_then(Value::as_str) {
                Some("unknown pattern id") => "unknown pattern id",
                Some("clip has no pattern") => "clip has no pattern",
                Some("clip is not an object") => "clip is not an object",
                other => panic!("unexpected reason {other:?}"),
            },
        })
        .collect();
    for pattern in object.get_mut("patterns").and_then(Value::as_array_mut).expect("patterns") {
        let pattern = pattern.as_object_mut().expect("pattern");
        if let Some(Value::Object(tags)) = pattern.get("tags") {
            let list = tags.iter().map(|(_, tag)| tag.clone()).collect();
            pattern.insert("tags", Value::Array(list));
        }
        if let Some(id) = pattern.get("id").and_then(Value::as_str).map(str::trim).map(str::to_owned) {
            pattern.insert("id", Value::from(id));
        }
    }
    (reference, dropped)
}

/// Remove the tracks named `name` from every pattern.
fn without_track(mut library: Value, name: &str) -> Value {
    let patterns = library.as_object_mut().and_then(|o| o.get_mut("patterns")).and_then(Value::as_array_mut);
    for pattern in patterns.expect("patterns") {
        if let Some(tracks) = pattern.as_object_mut().and_then(|p| p.get_mut("tracks")).and_then(Value::as_array_mut) {
            tracks.retain(|t| t.get("name").and_then(Value::as_str) != Some(name));
        }
    }
    library
}

#[test]
fn song_json_saves_what_the_js_exporter_saves() {
    let (library, report) = load("song.json");
    let (reference, js_dropped) = js_reference("library.export.json");
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
    assert_eq!(report.dropped_clips, js_dropped);
    assert_eq!(library.patterns.len(), 42);
    if let Some(difference) = diff(&library.to_json(), &reference, "") {
        panic!("{difference}");
    }
}

#[test]
fn edge_cases_save_what_the_js_exporter_saves() {
    let (library, report) = load("edge_library.json");
    let (reference, js_dropped) = js_reference("edge_library.export.json");
    assert_eq!(report.dropped_clips, js_dropped);
    // Track ODD has a reordered noteKeys header and badly typed values: JS
    // keeps them raw and writes garbage back; it is checked on its own below.
    let rust = without_track(library.to_json(), "ODD");
    let reference = without_track(reference, "ODD");
    if let Some(difference) = diff(&rust, &reference, "") {
        panic!("{difference}");
    }
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
            "patterns[0].tracks[4].pan: not a number (\"left\"), pan taken from the track name",
            "patterns[0].tracks[4].mono: wrong type (\"yes\"), default kept",
            "patterns[0].tracks[4].notes[0]: more values than noteKeys, extra values ignored",
        ]
    );
    assert_eq!(library.patterns[3].id, "spaced");
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
fn slugify_matches_js() {
    let cases = read_json("slugify.json");
    for case in cases.as_array().expect("array") {
        let [input, expected] = case.as_array().expect("pair") else { panic!("pair expected") };
        let (input, expected) = (input.as_str().unwrap(), expected.as_str().unwrap());
        assert_eq!(ids::slugify(input), expected, "slugify({input:?})");
    }
}

#[test]
fn infos_are_kept_as_written() {
    let (library, _) = load("song.json");
    let infos: &Object = &library.infos;
    assert_eq!(infos.get("name").and_then(Value::as_str), Some("orDrumbox Demo lib"));
}
