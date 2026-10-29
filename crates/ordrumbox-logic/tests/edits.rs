//! The editing commands against the JS Commander and HistoryManager
//! (`edits.json`, from tools/export_edit_fixtures.mjs): the same edits on
//! `song.json`, then undo all, then redo all; after each, the undo/redo stack
//! sizes, the newest step's description, the pattern ids and the saved
//! patterns must match.

use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Note};

#[path = "../../ordrumbox-core/tests/support/web_keys.rs"]
mod web_keys;
use ordrumbox_logic::Editor;

fn read(path: &Path) -> Value {
    json::parse(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).expect("parses")
}

fn usize_of(o: &Value, key: &str) -> usize {
    o.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}")) as usize
}

fn i32_of(o: &Value, key: &str) -> i32 {
    o.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}")) as i32
}

fn f64_of(o: &Value, key: &str) -> f64 {
    o.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}"))
}

fn str_of<'a>(o: &'a Value, key: &str) -> &'a str {
    o.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("{key}"))
}

fn updates(o: &Value) -> Vec<(String, Value)> {
    o.get("updates")
        .and_then(Value::as_object)
        .expect("updates")
        .iter()
        .map(|(k, v)| (k.to_owned(), v.clone()))
        .collect()
}

fn coalesce(o: &Value) -> bool {
    o.get("coalesce").and_then(Value::as_bool).unwrap_or(false)
}

/// The same operation as `run()` in the export script.
fn run(editor: &mut Editor, o: &Value) {
    let (p, t) = (|| usize_of(o, "p"), || usize_of(o, "t"));
    let op = str_of(o, "op");
    match op {
        "addNote" => {
            editor.add_note(p(), t(), i32_of(o, "beat"), i32_of(o, "beatStep"), f64_of(o, "pitch"));
        }
        "deleteNote" => {
            editor.delete_note(p(), t(), i32_of(o, "beat"), i32_of(o, "beatStep"), f64_of(o, "pitch"));
        }
        "updateTrack" => {
            let u = updates(o);
            let u: Vec<(&str, Value)> = u.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            editor.update_track(p(), t(), &u, None, coalesce(o));
        }
        "setStepsPerBeat" => {
            editor.set_steps_per_beat(p(), t(), f64_of(o, "value"), false);
        }
        "incrStepsPerBeat" => {
            editor.incr_steps_per_beat(p(), t());
        }
        "decrLoopPoint" => {
            editor.shorten_loop(p(), t());
        }
        "updateNote" => {
            let u = updates(o);
            let u: Vec<(&str, Value)> = u.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            editor.update_note(p(), t(), usize_of(o, "n"), &u, None, coalesce(o));
        }
        "pasteStepNotes" => {
            let from = o.get("from").expect("from");
            let source: Note =
                editor.library().patterns[p()].tracks[usize_of(from, "t")].notes[usize_of(from, "n")].clone();
            editor.paste_step_notes(p(), t(), i32_of(o, "beat"), i32_of(o, "beatStep"), &[source]);
        }
        "pasteTrack" => {
            let source = editor.library().patterns[p()].tracks[usize_of(o, "from")].clone();
            editor.paste_track(p(), usize_of(o, "index"), &source);
        }
        "removeTrack" => {
            editor.remove_track(p(), t());
        }
        "addTrack" => {
            editor.add_track(p(), str_of(o, "name"), 4);
        }
        "changeTrackName" => {
            editor.change_track_name(p(), t(), str_of(o, "name"));
        }
        "changeTrackSound" => {
            editor.change_track_sound(p(), t(), str_of(o, "sampleId"));
        }
        "cleanTrack" => {
            editor.clean_track(p(), t());
        }
        "setPatternBpm" => {
            editor.set_pattern_bpm(p(), f64_of(o, "bpm"), false);
        }
        "setPatternBeatCount" => {
            editor.set_pattern_beat_count(p(), f64_of(o, "beats"));
        }
        "setPatternDescription" => {
            editor.set_pattern_description(p(), str_of(o, "description"));
        }
        "renamePattern" => {
            editor.rename_pattern(p(), str_of(o, "name"));
        }
        "addPattern" => {
            editor.add_pattern(o.get("name").and_then(Value::as_str));
        }
        "removePattern" => {
            editor.remove_pattern(p());
        }
        "undo" => {
            editor.undo().expect("something to undo");
        }
        "redo" => {
            editor.redo().expect("something to redo");
        }
        other => panic!("unknown op {other}"),
    }
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

/// `tags` saved as `{"0": "a"}` by JS (a past bug) are an array in Rust,
/// and the web-version key names have their current ones.
fn js_pattern(pattern: Value) -> Value {
    let mut pattern = web_keys::current_keys(&pattern);
    if let Some(object) = pattern.as_object_mut()
        && let Some(Value::Object(tags)) = object.get("tags")
    {
        let list = tags.iter().map(|(_, tag)| tag.clone()).collect();
        object.insert("tags", Value::Array(list));
    }
    pattern
}

#[test]
fn edits_undo_and_redo_like_the_js_commander() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (library, _) = Library::from_json(&read(&dir.join("../ordrumbox-core/tests/fixtures/song.json")));
    let fixture = read(&dir.join("tests/fixtures/edits.json"));
    let mut editor = Editor::new(library);
    editor.set_clock(Some(0.0)); // the JS edits run within one coalescing window

    let steps = fixture.get("steps").and_then(Value::as_array).expect("steps");
    assert!(steps.len() > 70);
    for (i, step) in steps.iter().enumerate() {
        let op = step.get("after").expect("after");
        run(&mut editor, op);
        let at = format!("step {i} ({})", op.to_json());
        let history = editor.history();
        assert_eq!(history.undo_len(), usize_of(step, "undo"), "{at}: undo steps");
        assert_eq!(history.redo_len(), usize_of(step, "redo"), "{at}: redo steps");
        if let Some(desc) = step.get("desc").and_then(Value::as_str) {
            // The JS "Loop point back on X" is "Shorten loop on X" here.
            let desc = desc.replace("Loop point back on ", "Shorten loop on ");
            assert_eq!(history.next_undo(), Some(desc.as_str()), "{at}: description");
        }
        let ids: Vec<&str> = editor.library().patterns.iter().map(|p| p.id.as_str()).collect();
        let js_ids: Vec<&str> =
            step.get("ids").and_then(Value::as_array).expect("ids").iter().filter_map(Value::as_str).collect();
        assert_eq!(ids, js_ids, "{at}: pattern ids");
        let js_patterns = step.get("patterns").and_then(Value::as_array).expect("patterns");
        let indices = std::iter::once(0).chain(42..editor.library().patterns.len());
        for (index, js) in indices.zip(js_patterns) {
            let rust = editor.library().patterns[index].to_json();
            if let Some(difference) = diff(&rust, &js_pattern(js.clone()), &format!("patterns[{index}]")) {
                panic!("{at}: {difference}");
            }
        }
    }
}

/// Compact Tracks against the JS `compactTrack` on every track of every
/// pattern (`compact.json`): the steps recorded, then each track's loop
/// point and notes (beat, step, pitch, velocity).
#[test]
fn compact_tracks_like_the_js_commander() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = read(&dir.join("tests/fixtures/compact.json"));
    let song = read(&dir.join("../ordrumbox-core/tests/fixtures/song.json"));
    let hand = json::parse(&format!(r#"{{"patterns":{}}}"#, fixture.get("hand").expect("hand").to_json())).unwrap();
    let libraries = [Library::from_json(&song).0, Library::from_json(&hand).0];
    let cases = fixture.get("patterns").and_then(Value::as_array).expect("patterns");
    let mut compacted = 0;
    for case in cases {
        let source = usize::from(case.get("source").and_then(Value::as_str) == Some("hand"));
        // A fresh library each time (the history keeps 50 steps).
        let editor = &mut Editor::new(libraries[source].clone());
        let p = usize_of(case, "p");
        let at = format!("pattern {p} ({})", ["song", "hand"][source]);
        let changed = editor.compact_pattern(p);
        let descs = case.get("descs").and_then(Value::as_array).expect("descs");
        assert_eq!(changed, descs.len(), "{at}: tracks compacted ({})", Value::Array(descs.to_vec()).to_json());
        assert_eq!(editor.history().undo_len(), descs.len(), "{at}: steps");
        if let Some(desc) = descs.last().and_then(Value::as_str) {
            assert_eq!(editor.history().next_undo(), Some(desc), "{at}");
        }
        compacted += changed;
        let tracks = case.get("tracks").and_then(Value::as_array).expect("tracks");
        for (track, js) in editor.library().patterns[p].tracks.iter().zip(tracks) {
            let name = js.get("name").and_then(Value::as_str).unwrap();
            assert_eq!(track.name, name, "{at}");
            let loop_at = js.get("loopAtStep").and_then(Value::as_f64).map(|l| l as i32);
            assert_eq!(track.loop_length, loop_at, "{at} {name}: loop point");
            let notes: Vec<String> = track
                .notes
                .iter()
                .map(|n| {
                    format!(
                        "[{},{},{},{}]",
                        n.beat,
                        n.step,
                        Value::from(n.pitch).to_json(),
                        Value::from(n.velocity).to_json()
                    )
                })
                .collect();
            let js_notes: Vec<String> =
                js.get("notes").and_then(Value::as_array).unwrap().iter().map(Value::to_json).collect();
            assert_eq!(notes, js_notes, "{at} {name}: notes");
        }
    }
    assert!(compacted > 10, "{compacted}");
}
