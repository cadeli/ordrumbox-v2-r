//! The arrangement commands against the JS SongCommands and HistoryManager
//! (`song_edits.json`, from tools/export_song_fixtures.mjs): the same edits
//! on `song.json`'s arrangements, then undo all, then redo all; after each,
//! the undo/redo stack sizes, the newest step's description, what the
//! command returned, the selected arrangement and every arrangement must
//! match. Also `songMeasureAtTick()`.

use std::collections::HashSet;
use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Song, SongClip};
use ordrumbox_logic::Editor;

fn read(path: &Path) -> Value {
    json::parse(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).expect("parses")
}

fn f64_of(o: &Value, key: &str) -> Option<f64> {
    o.get(key).and_then(Value::as_f64)
}

fn index(o: &Value, key: &str) -> Option<usize> {
    f64_of(o, key).map(|v| v as usize)
}

fn start(o: &Value) -> u32 {
    f64_of(o, "start").expect("start") as u32
}

fn reference(o: &Value) -> &str {
    o.get("ref").and_then(Value::as_str).expect("ref")
}

fn clip_json(clip: &SongClip) -> Value {
    json::parse(&format!(
        r#"{{"pattern":{},"startMeasure":{},"measureCount":{}}}"#,
        Value::from(clip.pattern.as_str()).to_json(),
        clip.start_measure,
        Value::from(clip.measure_count).to_json()
    ))
    .expect("clip")
}

/// The same operation as `run()` in the export script; what it returned.
fn run(editor: &mut Editor, o: &Value) -> Value {
    let song = index(o, "songIdx");
    let clip = |r: Result<SongClip, String>| r.ok().as_ref().map_or(Value::Null, clip_json);
    match o.get("op").and_then(Value::as_str).expect("op") {
        "addPatternAtMeasure" => {
            clip(editor.add_pattern_at_measure(reference(o), start(o), f64_of(o, "measureCount"), song))
        }
        "repeatPatternAtMeasure" => clip(editor.repeat_pattern_at_measure(start(o), song)),
        "removeSongClips" => {
            let indices: Vec<usize> = o
                .get("indices")
                .and_then(Value::as_array)
                .expect("indices")
                .iter()
                .filter_map(Value::as_f64)
                .filter(|&i| i >= 0.0)
                .map(|i| i as usize)
                .collect();
            Value::Bool(editor.remove_song_clips(song, &indices))
        }
        "removePatternAtMeasure" => {
            Value::from(editor.remove_pattern_at_measure(start(o), song).map_or(0, |c| c.len()) as f64)
        }
        "removePatternClips" => {
            Value::from(editor.remove_pattern_clips(reference(o), song).map_or(0, |c| c.len()) as f64)
        }
        "setSelectedSongIdx" => Value::from(editor.set_selected_song(song.expect("songIdx")) as f64),
        "addSong" => {
            let i = editor.add_song(o.get("name").and_then(Value::as_str).expect("name"));
            Value::from(editor.library().songs[i].id.as_str())
        }
        "removeSong" => Value::Bool(editor.remove_song(song)),
        "undo" => {
            editor.undo();
            Value::Null
        }
        "redo" => {
            editor.redo();
            Value::Null
        }
        op => panic!("unknown op {op}"),
    }
}

/// An arrangement as the fixture writes it.
fn song_json(song: &Song) -> Value {
    let clips: Vec<String> = song.clips.iter().map(|c| clip_json(c).to_json()).collect();
    json::parse(&format!(
        r#"{{"id":{},"name":{},"description":{},"bpm":{},"loopMeasureCount":{},"length":{},"clips":[{}]}}"#,
        Value::from(song.id.as_str()).to_json(),
        Value::from(song.name.as_str()).to_json(),
        Value::from(song.description.as_str()).to_json(),
        Value::from(song.bpm).to_json(),
        song.loop_measure_count.map_or(Value::Null, Value::from).to_json(),
        Value::from(song.length_measures()).to_json(),
        clips.join(",")
    ))
    .expect("song")
}

#[test]
fn arrangement_edits_match_the_js_commander() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = read(&root.join("tests/fixtures/song_edits.json"));
    let library_text =
        std::fs::read_to_string(root.join("../ordrumbox-core/tests/fixtures/song.json")).expect("song.json");
    let mut editor = Editor::new(Library::parse(&library_text).expect("library").0);
    editor.set_clock(Some(0.0));

    let steps = fixture.get("steps").and_then(Value::as_array).expect("steps");
    for (i, step) in steps.iter().enumerate() {
        let op = step.get("after").expect("after");
        let at = format!("step {i} ({})", op.to_json());
        let result = run(&mut editor, op);
        let history = editor.history();
        assert_eq!(history.undo_len(), index(step, "undo").unwrap(), "{at}: undo steps");
        assert_eq!(history.redo_len(), index(step, "redo").unwrap(), "{at}: redo steps");
        if let Some(desc) = step.get("desc").and_then(Value::as_str) {
            assert_eq!(history.next_undo(), Some(desc), "{at}: description");
        }
        if let Some(expected) = step.get("result").filter(|r| !r.is_null()) {
            assert_eq!(result.to_json(), expected.to_json(), "{at}: result");
        }
        assert_eq!(editor.selected_song(), index(step, "selected").unwrap(), "{at}: selected arrangement");
        let songs: Vec<String> = editor.library().songs.iter().map(|s| song_json(s).to_json()).collect();
        let expected: Vec<String> =
            step.get("songs").and_then(Value::as_array).expect("songs").iter().map(Value::to_json).collect();
        assert_eq!(songs, expected, "{at}: arrangements");
    }
}

#[test]
fn measure_at_tick_matches_song_measure_at_tick() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = read(&root.join("tests/fixtures/song_edits.json"));
    for case in fixture.get("measures").and_then(Value::as_array).expect("measures") {
        let raw = json::parse(&format!(r#"{{"clips":{}}}"#, case.get("clips").expect("clips").to_json())).unwrap();
        let ids: HashSet<String> = ["rock", "basrock", "smrock", "rock8a", "rock8b", "jazz", "funk"]
            .into_iter()
            .map(str::to_owned)
            .chain(
                case.get("clips")
                    .and_then(Value::as_array)
                    .unwrap()
                    .iter()
                    .filter_map(|c| c.get("pattern").and_then(Value::as_str).map(str::to_owned)),
            )
            .collect();
        let mut song = Song::from_json(&raw, &ids).expect("song").0;
        song.loop_measure_count = f64_of(case, "loopMeasureCount");
        for at in case.get("at").and_then(Value::as_array).expect("at") {
            let tick = f64_of(at, "tick").unwrap_or(0.0);
            let expected = f64_of(at, "measure").expect("measure");
            assert_eq!(song.measure_at_tick(tick, 32.0), expected, "tick {tick} on {}", case.to_json());
        }
    }
}
