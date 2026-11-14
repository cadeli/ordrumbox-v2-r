//! Tests of tools.

use super::*;
use ordrumbox_core::json;

/// What the tools work on, owned by the test.
struct Bench {
    editor: Editor,
    scales: Scales,
    scale_list: ScaleList,
    styles: Styles,
    learned_styles: (Styles, Option<u64>),
    rng: Rng,
}

fn bench() -> Bench {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":2}]}]},
            {"name":"jazz"}]}"#,
    )
    .unwrap();
    Bench {
        editor: Editor::new(library),
        scales: Scales::default(),
        scale_list: ScaleList::default(),
        styles: Styles::default(),
        learned_styles: (Styles::default(), None),
        rng: Rng::new(7),
    }
}

/// The note editor's ranges of the settings the tests set.
fn note_range(key: &str) -> Option<(f64, f64)> {
    match key {
        "velocity" | "probability" => Some((0.0, 1.0)),
        "lengthSteps" => Some((0.25, 1024.0)),
        _ => None,
    }
}

fn call(b: &mut Bench, name: &str, args: &str) -> Result<Value, String> {
    let mut tools = Tools {
        editor: &mut b.editor,
        scales: &b.scales,
        scale_list: &b.scale_list,
        styles: &b.styles,
        learned_styles: &mut b.learned_styles,
        rng: &mut b.rng,
        synth_presets: &["acid"],
        note_range,
    };
    tools.call(name, &json::parse(args).unwrap()).unwrap_or_else(|| panic!("{name} is not a library tool"))
}

fn get<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(value, |v, key| v.get(key).unwrap_or_else(|| panic!("no {key} in {v}")))
}

fn undo_len(b: &Bench) -> usize {
    b.editor.history().undo_len()
}

#[test]
fn legato_mono_and_note_length_go_through_the_tools() {
    let mut b = bench();
    // Legato and mono, of the track; the length of every note it has.
    let updated = call(
        &mut b,
        "updateTrack",
        r#"{"patternName":"rock","trackName":"KICK","updates":{"legato":true,"mono":true},"noteUpdates":{"lengthSteps":4}}"#,
    )
    .unwrap();
    assert_eq!(get(&updated, &["settings"]).to_json(), r#"{"legato":true,"mono":true}"#);
    assert_eq!(get(&updated, &["notesUpdated"]).as_f64(), Some(2.0));
    // A note added with its length.
    call(
        &mut b,
        "addNotesToPattern",
        r#"{"patternName":"rock","notes":[{"trackName":"SNARE","step":0,"lengthSteps":2.5}]}"#,
    )
    .unwrap();
    let loaded = call(&mut b, "loadPattern", r#"{"patternName":"rock"}"#).unwrap();
    let tracks = get(&loaded, &["tracks"]).as_array().unwrap();
    let kick = &tracks[0];
    assert_eq!(get(kick, &["legato"]).as_bool(), Some(true));
    assert_eq!(get(kick, &["mono"]).as_bool(), Some(true));
    let lengths: Vec<f64> =
        get(kick, &["notes"]).as_array().unwrap().iter().map(|n| get(n, &["lengthSteps"]).as_f64().unwrap()).collect();
    assert_eq!(lengths, [4.0, 4.0]);
    let snare = &tracks[1];
    let lengths: Vec<f64> =
        get(snare, &["notes"]).as_array().unwrap().iter().map(|n| get(n, &["lengthSteps"]).as_f64().unwrap()).collect();
    assert_eq!(lengths, [2.5]);
    // A length past the note editor's range is refused.
    let refused = call(
        &mut b,
        "addNotesToPattern",
        r#"{"patternName":"rock","notes":[{"trackName":"SNARE","step":4,"lengthSteps":2048}]}"#,
    );
    assert_eq!(refused.unwrap_err(), "notes[0].lengthSteps: 2048 is outside 0.25 to 1024");
}

#[test]
fn notes_are_added_in_one_undo_step() {
    let mut b = bench();
    let before = b.editor.library().clone();
    let answer = call(
        &mut b,
        "addNotesToPattern",
        r#"{"patternName":"ROCK","notes":[{"trackName":"kick","step":4,"velocity":0.5},
            {"trackName":"snare","step":20,"probability":0.5},{"trackName":"KICK","step":8}]}"#,
    )
    .unwrap();
    assert_eq!(answer.to_json(), r#"{"created":2,"updated":1,"tracksAdded":["SNARE"],"beatCount":8}"#);
    let pattern = &b.editor.library().patterns[0];
    assert_eq!(pattern.tracks[1].name, "SNARE");
    // KICK has no loop point; SNARE, added on 4 beats, looped over them
    // and loops over the 8 now.
    assert_eq!(pattern.tracks.iter().map(|t| t.loop_length).collect::<Vec<_>>(), [None, Some(32)]);
    assert_eq!((pattern.tracks[0].notes[2].beat, pattern.tracks[0].notes[2].velocity), (1, 0.5));
    assert_eq!((pattern.tracks[1].notes[0].beat, pattern.tracks[1].notes[0].probability), (5, 0.5));
    assert_eq!(undo_len(&b), 1);
    assert_eq!(b.editor.history().next_undo(), Some("Add 3 notes to \"rock\""));
    b.editor.undo();
    assert_eq!(b.editor.library(), &before);
}

#[test]
fn wrong_arguments_change_nothing() {
    let mut b = bench();
    let refused = [
        ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":0,"velocity":2}]}"#),
        ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":0,"cycle":"x"}]}"#),
        ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":64}]}"#),
        ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":1.5}]}"#),
        ("addNotesToPattern", r#"{"patternName":"pop","notes":[]}"#),
        ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"loudness":1}}"#),
        ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"name":"X"}}"#),
        ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"mute":1}}"#),
        ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"synthPreset":"B1"}}"#),
        ("setPatternBeatCount", r#"{"patternName":"rock","beatCount":20}"#),
        ("createNewPattern", r#"{"patternName":" Jazz "}"#),
        ("createArrangement", r#"{"name":"song","clips":[{"patternName":"pop"}]}"#),
        ("addPatternToArrangement", r#"{"patternName":"rock"}"#),
    ];
    for (tool, args) in refused {
        assert!(call(&mut b, tool, args).is_err(), "{tool} {args}");
    }
    assert_eq!(undo_len(&b), 0);
    let error = call(&mut b, "addNotesToPattern", refused[0].1).unwrap_err();
    assert_eq!(error, "notes[0].velocity: 2 is outside 0 to 1");
}

#[test]
fn a_track_is_updated_or_added() {
    let mut b = bench();
    let args = r#"{"patternName":"rock","trackName":"kick","updates":{"volume":3,"mute":true,"velocity":0.5},
        "noteUpdates":{"probability":0.25}}"#;
    let answer = call(&mut b, "updateTrack", args).unwrap();
    // `velocity`, the web version's key, is `volume`: the later one wins.
    assert_eq!(
        answer.to_json(),
        r#"{"action":"updated","trackName":"KICK","settings":{"volume":0.5,"mute":true},"notesUpdated":2}"#
    );
    assert!(b.editor.library().patterns[0].tracks[0].notes.iter().all(|n| n.probability == 0.25));
    let answer = call(&mut b, "updateTrack", r#"{"patternName":"rock","trackName":"chh","updates":{"pan":-4}}"#);
    assert_eq!(get(&answer.unwrap(), &["settings", "pan"]).as_f64(), Some(-1.0));
    assert_eq!(b.editor.library().patterns[0].tracks[1].name, "CHH");
    let preset = r#"{"patternName":"rock","trackName":"KICK","updates":{"useSynth":true,"synthPreset":"acid"}}"#;
    call(&mut b, "updateTrack", preset).unwrap();
    assert_eq!(undo_len(&b), 3);
}

#[test]
fn patterns_are_listed_and_set() {
    let mut b = bench();
    let created = call(&mut b, "createNewPattern", r#"{"patternName":"house"}"#).unwrap();
    assert_eq!(get(&created, &["index"]).as_f64(), Some(2.0));
    let bpm = call(&mut b, "setBpm", r#"{"bpm":128.4}"#).unwrap();
    assert_eq!(bpm.to_json(), r#"{"bpm":128,"note":"128.4 asked, 128 used (whole, 20 to 300)"}"#);
    call(&mut b, "setPatternTags", r#"{"patternName":"rock","tags":["dance"," "]}"#).unwrap();
    call(&mut b, "setPatternDescription", r#"{"patternName":"rock","description":"four on the floor"}"#).unwrap();
    let listed = call(&mut b, "listPatterns", "{}").unwrap();
    let rock = &get(&listed, &["patterns"]).as_array().unwrap()[0];
    assert_eq!(
        rock.to_json(),
        r#"{"index":0,"id":"rock","name":"rock","beatCount":4,"trackCount":1,"tags":["dance"]}"#
    );
    let loaded = call(&mut b, "loadPattern", r#"{"patternName":"rock"}"#).unwrap();
    assert_eq!(get(&loaded, &["description"]).as_str(), Some("four on the floor"));
    let track = &get(&loaded, &["tracks"]).as_array().unwrap()[0];
    assert_eq!(get(track, &["notes"]).to_json(), r#"[{"step":0},{"step":8,"beat":2}]"#);
    assert_eq!(get(track, &["volume"]).as_f64(), Some(1.0));
}

#[test]
fn patterns_are_renamed_and_copied() {
    let mut b = bench();
    assert!(call(&mut b, "renamePattern", r#"{"patternName":"rock","newName":"JAZZ"}"#).is_err());
    let renamed = call(&mut b, "renamePattern", r#"{"patternName":"rock","newName":" Rock "}"#).unwrap();
    assert_eq!((get(&renamed, &["id"]).as_str(), get(&renamed, &["name"]).as_str()), (Some("rock"), Some("Rock")));
    let copy = call(&mut b, "duplicatePattern", r#"{"patternName":"rock","newName":"rock 2"}"#).unwrap();
    assert_eq!((get(&copy, &["index"]).as_f64(), get(&copy, &["trackCount"]).as_f64()), (Some(2.0), Some(1.0)));
    assert_eq!(get(&copy, &["name"]).as_str(), Some("rock 2"));
    assert_eq!(undo_len(&b), 2);
}

#[test]
fn notes_and_tracks_are_deleted() {
    let mut b = bench();
    call(&mut b, "addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"SNARE","step":4}]}"#).unwrap();
    let args = r#"{"patternName":"rock","notes":[{"trackName":"kick","step":8},{"trackName":"KICK","step":8},
        {"trackName":"KICK","step":1}]}"#;
    assert_eq!(call(&mut b, "deleteNotes", args).unwrap().to_json(), r#"{"deleted":1}"#);
    assert_eq!(b.editor.library().patterns[0].tracks[0].notes.len(), 1);
    assert!(call(&mut b, "deleteNotes", args).is_err(), "nothing left at those steps");
    let unknown = r#"{"patternName":"rock","notes":[{"trackName":"CHH","step":0}]}"#;
    assert!(call(&mut b, "deleteNotes", unknown).is_err());
    let removed = call(&mut b, "removeTrack", r#"{"patternName":"rock","trackName":"kick"}"#).unwrap();
    assert_eq!(removed.to_json(), r#"{"removed":"KICK","tracks":["SNARE"]}"#);
    assert!(call(&mut b, "removeTrack", r#"{"patternName":"rock","trackName":"SNARE"}"#).is_err());
    assert_eq!(undo_len(&b), 3);
}

#[test]
fn tracks_are_copied_renamed_and_compacted() {
    let mut b = bench();
    let args = r#"{"patternName":"rock","trackName":"kick","toPatternName":"jazz"}"#;
    let copied = call(&mut b, "copyTrack", args).unwrap();
    assert_eq!(copied.to_json(), r#"{"patternName":"jazz","trackName":"KICK","noteCount":2}"#);
    assert!(call(&mut b, "copyTrack", args).is_err(), "jazz has a KICK now");
    let renamed = call(&mut b, "copyTrack", r#"{"patternName":"rock","trackName":"KICK","newName":"kick 2"}"#);
    assert_eq!(get(&renamed.unwrap(), &["trackName"]).as_str(), Some("KICK 2"));
    assert_eq!(b.editor.library().patterns[0].tracks.len(), 2);
    call(&mut b, "removeTrack", r#"{"patternName":"rock","trackName":"kick 2"}"#).unwrap();
    call(&mut b, "addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"SNARE","step":4}]}"#).unwrap();
    assert!(call(&mut b, "renameTrack", r#"{"patternName":"rock","trackName":"kick","newName":"snare"}"#).is_err());
    let renamed = call(&mut b, "renameTrack", r#"{"patternName":"rock","trackName":"kick","newName":"bd"}"#);
    assert_eq!(renamed.unwrap().to_json(), r#"{"trackName":"BD","tracks":["BD","SNARE"]}"#);
    // BD repeats its note of beat 0 on beat 2; SNARE's one note would
    // only move its loop point, so it is left alone.
    let compacted = call(&mut b, "compactPattern", r#"{"patternName":"rock"}"#).unwrap();
    assert_eq!(get(&compacted, &["tracksCompacted"]).as_f64(), Some(1.0));
    assert_eq!(undo_len(&b), 6);
}

#[test]
fn arrangements_are_made_and_edited() {
    let mut b = bench();
    let args = r#"{"name":"Song","bpm":500,"clips":[{"patternName":"rock"},{"patternName":"jazz","startMeasure":1,"measureCount":2}]}"#;
    let made = call(&mut b, "createArrangement", args).unwrap();
    assert_eq!(get(&made, &["bpm"]).as_f64(), Some(300.0));
    assert_eq!(get(&made, &["note"]).as_str(), Some("bpm 500 asked, 300 used (20 to 300)"));
    assert_eq!(get(&made, &["lengthMeasures"]).as_f64(), Some(3.0));
    assert_eq!(undo_len(&b), 1);
    let added =
        call(&mut b, "addPatternToArrangement", r#"{"patternName":"ROCK","startMeasure":3,"arrangement":"song"}"#);
    assert_eq!(get(&added.unwrap(), &["clip"]).to_json(), r#"{"patternId":"rock","startMeasure":3,"measureCount":1}"#);
    let removed = call(&mut b, "removePatternFromArrangement", r#"{"patternName":"rock","arrangement":0}"#).unwrap();
    assert_eq!(get(&removed, &["removed"]).as_f64(), Some(2.0));
    assert!(call(&mut b, "removePatternFromArrangement", r#"{"startMeasure":9}"#).is_err());
    let listed = call(&mut b, "listArrangements", "{}").unwrap();
    let clips = get(&get(&listed, &["arrangements"]).as_array().unwrap()[0], &["clips"]).to_json();
    assert_eq!(clips, r#"[{"patternId":"jazz","patternName":"jazz","startMeasure":1,"measureCount":2}]"#);
    let updated = call(&mut b, "updateArrangement", r#"{"bpm":90,"loopMeasures":2}"#).unwrap();
    assert_eq!(get(&updated, &["bpm"]).as_f64(), Some(90.0));
    assert_eq!(b.editor.library().songs[0].loop_measures, Some(2.0));
    assert!(call(&mut b, "updateArrangement", "{}").is_err());
}

#[test]
fn clips_are_moved_and_repeated() {
    let mut b = bench();
    let args =
        r#"{"name":"Song","clips":[{"patternName":"rock","measureCount":2},{"patternName":"jazz","startMeasure":0}]}"#;
    call(&mut b, "createArrangement", args).unwrap();
    assert!(call(&mut b, "moveClip", r#"{"startMeasure":0,"toMeasure":4}"#).is_err(), "two clips start there");
    let moved = call(&mut b, "moveClip", r#"{"startMeasure":0,"toMeasure":4,"patternName":"jazz"}"#).unwrap();
    let clips = get(&moved, &["clips"]).as_array().unwrap();
    assert!(
        clips
            .iter()
            .any(|c| c.to_json() == r#"{"patternId":"jazz","patternName":"jazz","startMeasure":4,"measureCount":1}"#)
    );
    assert!(call(&mut b, "moveClip", r#"{"startMeasure":2,"toMeasure":4}"#).is_err());
    let repeated = call(&mut b, "repeatClip", r#"{"startMeasure":0}"#).unwrap();
    assert_eq!(get(&repeated, &["clip"]).to_json(), r#"{"patternId":"rock","startMeasure":2,"measureCount":2}"#);
    assert!(call(&mut b, "repeatClip", r#"{"startMeasure":1}"#).is_err());
    call(&mut b, "createArrangement", r#"{"name":"Other"}"#).unwrap();
    assert!(call(&mut b, "renameArrangement", r#"{"arrangement":"other","newName":"song"}"#).is_err());
    let renamed = call(&mut b, "renameArrangement", r#"{"arrangement":0,"newName":"Verse"}"#).unwrap();
    assert_eq!((get(&renamed, &["name"]).as_str(), get(&renamed, &["id"]).as_str()), (Some("Verse"), Some("song")));
    assert_eq!(undo_len(&b), 5);
}

#[test]
fn patterns_are_generated() {
    let mut b = bench();
    assert!(call(&mut b, "generatePattern", r#"{"style":"no such style"}"#).is_err());
    let made = call(&mut b, "generatePattern", "{}").unwrap();
    assert_eq!(get(&made, &["index"]).as_f64(), Some(2.0));
    assert!(get(&made, &["trackCount"]).as_f64().is_some_and(|n| n > 0.0));
    assert_eq!(undo_len(&b), 1);
    let bass = r#"{"patternName":"rock","part":"bass"}"#;
    assert_eq!(get(&call(&mut b, "toggleGeneration", bass).unwrap(), &["auto"]), &Value::Bool(true));
    assert_eq!(get(&call(&mut b, "toggleGeneration", bass).unwrap(), &["auto"]), &Value::Bool(false));
    assert!(call(&mut b, "toggleGeneration", r#"{"patternName":"rock","part":"lead"}"#).is_err());
    let set = call(&mut b, "setPatternMacro", r#"{"patternName":"rock","macro":"energy","value":80}"#).unwrap();
    assert_eq!(set.to_json(), r#"{"macro":"energy","value":80}"#);
    assert!(call(&mut b, "setPatternMacro", r#"{"patternName":"rock","macro":"energy","value":120}"#).is_err());
    let random = call(&mut b, "randomizeTrack", r#"{"patternName":"rock","trackName":"KICK"}"#).unwrap();
    assert_eq!(get(&random, &["trackName"]).as_str(), Some("KICK"));
    call(&mut b, "updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"composeLock":true}}"#).unwrap();
    assert!(call(&mut b, "regenerateTrack", r#"{"patternName":"rock","trackName":"KICK"}"#).is_err(), "locked");
    let auto = call(&mut b, "togglePatternAuto", r#"{"patternName":"rock"}"#);
    assert!(get(&auto.unwrap(), &["auto"]).as_bool().is_some());
    assert!(get(&call(&mut b, "listStyles", "{}").unwrap(), &["styles"]).as_array().is_some());
    assert!(get(&call(&mut b, "listScales", "{}").unwrap(), &["scales"]).as_array().is_some());
}

#[test]
fn other_tools_are_left_to_the_app() {
    let mut b = bench();
    let mut tools = Tools {
        editor: &mut b.editor,
        scales: &b.scales,
        scale_list: &b.scale_list,
        styles: &b.styles,
        learned_styles: &mut b.learned_styles,
        rng: &mut b.rng,
        synth_presets: &[],
        note_range,
    };
    for name in ["play", "selectPattern", "removePattern", "getSynthPreset", "setColorScheme"] {
        assert!(tools.call(name, &Value::Object(Object::new())).is_none(), "{name}");
    }
}
