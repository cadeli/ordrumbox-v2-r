//! Tests of the MCP tools run on the session.

use super::*;
use crate::kits::KitSet;
use crate::session::NOTHING_TO_RENDER;
use crate::session::session_confirm::Pending;
use crate::session::session_playback::Playback;
use crate::test_support::scratch_dir;
use ordrumbox_core::json;
use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_engine::kit::{BankSample, Kit, KitSample};
use ordrumbox_engine::{Controller, SampleBank, SampleBuffer};
use ordrumbox_engine::{SynthPresets, wav};
use std::sync::Arc;

fn session() -> Session {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":2}]}]},
            {"name":"jazz"}]}"#,
    )
    .unwrap();
    Session::new(library, KitSet::empty(), Vec::new())
}

fn call(session: &mut Session, name: &str, args: &str) -> Result<Value, String> {
    session.mcp_call(name, &json::parse(args).unwrap())
}

fn get<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter().fold(value, |v, key| v.get(key).unwrap_or_else(|| panic!("no {key} in {v}")))
}

#[test]
fn library_tools_run_on_the_session() {
    let mut s = session();
    let before = s.editor.library().clone();
    let args = r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":4,"velocity":2}]}"#;
    // The note editor's ranges are the session's.
    assert_eq!(call(&mut s, "addNotesToPattern", args), Err("notes[0].velocity: 2 is outside 0 to 1".into()));
    let args = r#"{"patternName":"rock","notes":[{"trackName":"snare","step":4},{"trackName":"KICK","step":8}]}"#;
    call(&mut s, "addNotesToPattern", args).unwrap();
    assert_eq!(s.message, "Add 2 notes to \"rock\"");
    let undone = call(&mut s, "undo", "{}").unwrap();
    assert_eq!(get(&undone, &["message"]).as_str(), Some("undone: Add 2 notes to \"rock\""));
    assert_eq!(s.editor.library(), &before);
    call(&mut s, "redo", "{}").unwrap();
    assert_eq!(s.editor.library().patterns[0].tracks.len(), 2);
    assert_eq!(call(&mut s, "setColorScheme", r#"{"scheme":2}"#), Err("Unknown tool: setColorScheme".into()));
}

#[test]
fn export_file_names_stay_in_their_folder() {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"id":"/../../x","name":"a"}],"songs":[{"id":"..\\..\\y","name":"b","clips":[]}]}"#,
    )
    .unwrap();
    let s = Session::new(library, KitSet::empty(), Vec::new());
    assert_eq!(s.export_file_name_of(Export::PatternWav, 0, 0).as_deref(), Some("ordrumbox-_______x.wav"));
    assert_eq!(s.export_file_name_of(Export::PatternMidi, 0, 0).as_deref(), Some("ordrumbox-_______x.mid"));
    assert_eq!(s.export_file_name_of(Export::SongWav, 0, 0).as_deref(), Some("ordrumbox-song-______y.wav"));
}

#[test]
fn patterns_are_shown_and_removed() {
    let mut s = session();
    call(&mut s, "createNewPattern", r#"{"patternName":"house"}"#).unwrap();
    let shown = call(&mut s, "selectPattern", r#"{"patternName":"house"}"#).unwrap();
    assert_eq!((s.pattern_index, get(&shown, &["name"]).as_str()), (2, Some("house")));
    call(&mut s, "selectPattern", r#"{"index":1}"#).unwrap();
    assert_eq!(s.pattern_index, 1);
    assert!(call(&mut s, "selectPattern", r#"{"index":3}"#).is_err());
    assert_eq!(get(&call(&mut s, "listPatterns", "{}").unwrap(), &["shown"]).as_f64(), Some(1.0));
    call(&mut s, "createArrangement", r#"{"name":"song","clips":[{"patternName":"jazz"},{"patternName":"rock"}]}"#)
        .unwrap();
    call(&mut s, "selectPattern", r#"{"patternName":"house"}"#).unwrap();
    let removed = call(&mut s, "removePattern", r#"{"patternName":"jazz"}"#).unwrap();
    assert_eq!(removed.to_json(), r#"{"removed":"jazz","clipsRemoved":1}"#);
    // The pattern shown stays the same one.
    assert_eq!(s.pattern().and_then(|p| p.name.as_deref()), Some("house"));
    assert_eq!(s.editor.library().songs[0].clips.len(), 1);
    call(&mut s, "removePattern", r#"{"patternName":"rock"}"#).unwrap();
    assert!(call(&mut s, "removePattern", r#"{"patternName":"house"}"#).is_err(), "the last pattern stays");
}

#[test]
fn arrangements_are_selected_and_removed() {
    let mut s = session();
    call(&mut s, "createArrangement", r#"{"name":"A","clips":[{"patternName":"rock","measureCount":2}]}"#).unwrap();
    call(&mut s, "createArrangement", r#"{"name":"B"}"#).unwrap();
    assert_eq!(s.editor.selected_song(), 1);
    assert!(call(&mut s, "selectArrangement", "{}").is_err());
    let selected = call(&mut s, "selectArrangement", r#"{"arrangement":"a"}"#).unwrap();
    assert_eq!((s.editor.selected_song(), get(&selected, &["name"]).as_str()), (0, Some("A")));
    let info = call(&mut s, "setSongInfo", r#"{"name":" Live set ","description":"two parts"}"#).unwrap();
    assert_eq!(info.to_json(), r#"{"name":"Live set","description":"two parts"}"#);
    assert!(call(&mut s, "setSongInfo", r#"{"name":""}"#).is_err());
    let removed = call(&mut s, "removeArrangement", r#"{"arrangement":"B"}"#).unwrap();
    assert_eq!(removed.to_json(), r#"{"removed":"B","arrangements":1}"#);
}

#[test]
fn the_master_is_read_and_set() {
    let mut s = session();
    let master = call(&mut s, "setMaster", r#"{"settings":{"volume":0.5,"compBypass":true}}"#).unwrap();
    assert_eq!((get(&master, &["volume"]).as_f64(), get(&master, &["compBypass"])), (Some(0.5), &Value::Bool(true)));
    for refused in
        [r#"{"settings":{"volume":3}}"#, r#"{"settings":{"loudness":1}}"#, r#"{"settings":{"compBypass":1}}"#]
    {
        assert!(call(&mut s, "setMaster", refused).is_err(), "{refused}");
    }
    assert_eq!(call(&mut s, "getMaster", "{}").unwrap(), master);
    assert_eq!(s.editor.history().undo_len(), 0);
}

#[test]
fn lists_exports_and_imports() {
    let mut s = session();
    assert_eq!(call(&mut s, "listSynthPresets", "{}").unwrap().to_json(), r#"{"count":0,"presets":[]}"#);
    let preset = r#"{"patternName":"rock","trackName":"KICK","updates":{"useSynth":true,"synthPreset":"B1"}}"#;
    assert!(call(&mut s, "updateTrack", preset).is_err(), "no such preset");
    assert!(get(&call(&mut s, "listScales", "{}").unwrap(), &["scales"]).as_array().is_some());
    assert!(get(&call(&mut s, "listStyles", "{}").unwrap(), &["styles"]).as_array().is_some());
    assert_eq!(call(&mut s, "exportPatternWav", r#"{"patternName":"rock"}"#), Err(NOTHING_TO_RENDER.into()));
    let path = std::env::temp_dir().join(format!("ordrumbox_mcp_{}.mid", std::process::id()));
    s.write_midi(0, &path).unwrap();
    let args = object([("filePath", Value::from(path.display().to_string()))]);
    let imported = s.mcp_call("importMidi", &args).unwrap();
    std::fs::remove_file(&path).unwrap();
    let patterns = get(&imported, &["patterns"]).as_array().unwrap();
    assert_eq!(get(&patterns[0], &["index"]).as_f64(), Some(2.0));
    assert_eq!(s.pattern_index, s.editor.library().patterns.len() - 1);
    assert!(call(&mut s, "importMidi", r#"{"filePath":"/no/such/file.mid"}"#).is_err());
}

#[test]
fn synth_presets_are_designed() {
    let mut s = session();
    let args = r#"{"preset":"acid","settings":{"vco1.wave":"sawtooth","filter.freq":800,"filter.slope":24,
        "lfo.depth":0.3,"bypassFilter":false}}"#;
    let made = call(&mut s, "createSynthPreset", args).unwrap();
    assert_eq!(get(&made, &["settings", "vco1", "wave"]).as_str(), Some("sawtooth"));
    assert_eq!(get(&made, &["settings", "filter", "slope"]).as_f64(), Some(24.0));
    // Setting the LFO's depth aims it at the filter, as the editor does.
    assert_eq!(get(&made, &["settings", "lfo", "target"]).as_str(), Some("filter.freq"));
    assert!(call(&mut s, "createSynthPreset", r#"{"preset":"ACID"}"#).is_err(), "the key is taken");
    let refused = [
        r#"{"preset":"acid","settings":{"filter.freq":5}}"#,
        r#"{"preset":"acid","settings":{"vco1.octave":1.5}}"#,
        r#"{"preset":"acid","settings":{"vco1.wave":"saw"}}"#,
        r#"{"preset":"acid","settings":{"filter.slope":"24"}}"#,
        r#"{"preset":"acid","settings":{"vco9.gain":1}}"#,
        r#"{"preset":"acid","settings":{}}"#,
        r#"{"preset":"bass","settings":{"vco1.gain":1}}"#,
    ];
    for args in refused {
        assert!(call(&mut s, "updateSynthPreset", args).is_err(), "{args}");
    }
    let copy =
        call(&mut s, "createSynthPreset", r#"{"preset":"acid 2","from":"Acid","settings":{"vco2.wave":"square"}}"#);
    let copy = copy.unwrap();
    assert_eq!(get(&copy, &["settings", "filter", "freq"]).as_f64(), Some(800.0));
    assert_eq!(get(&copy, &["settings", "vco2", "gain"]).as_f64(), Some(0.5), "vco2 switched on");
    call(&mut s, "updateSynthPreset", r#"{"preset":"acid","settings":{"envelope.decay":0.2}}"#).unwrap();
    let read = call(&mut s, "getSynthPreset", r#"{"preset":"acid"}"#).unwrap();
    assert_eq!(get(&read, &["settings", "envelope", "decay"]).as_f64(), Some(0.2));
    let preset = r#"{"patternName":"rock","trackName":"KICK","updates":{"useSynth":true,"synthPreset":"acid"}}"#;
    call(&mut s, "updateTrack", preset).unwrap();
    let removed = call(&mut s, "removeSynthPreset", r#"{"preset":"acid"}"#).unwrap();
    assert_eq!(removed.to_json(), r#"{"removed":"acid","tracksUsing":["rock/KICK"]}"#);
    assert!(call(&mut s, "removeSynthPreset", r#"{"preset":"acid 2"}"#).is_err(), "the last preset stays");
    assert_eq!(s.synths.keys(), ["acid 2"]);
}
#[test]
fn songs_kits_and_playback_without_files_or_sound() {
    let mut s = session();
    assert_eq!(call(&mut s, "getPlaybackState", "{}").unwrap().to_json(), r#"{"playing":false}"#);
    assert_eq!(call(&mut s, "auditionTrack", r#"{"patternName":"rock","trackName":"KICK"}"#), Err(NO_AUDIO.into()));
    // A session of the tests has no user data folder and no kit.
    assert!(call(&mut s, "saveSong", "{}").is_err());
    assert!(call(&mut s, "listSavedSongs", "{}").is_err());
    assert!(call(&mut s, "loadSong", "{}").is_err());
    assert!(call(&mut s, "loadSong", r#"{"filePath":"/no/such/song.odbox"}"#).is_err());
    assert!(call(&mut s, "normalizeKit", "{}").is_err());
    assert!(call(&mut s, "exportKitMapping", r#"{"kit":"808"}"#).is_err());
    assert!(call(&mut s, "importKitFolder", r#"{"folderPath":"/no/such/folder"}"#).is_err());
    assert!(call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"random"}"#).is_err());
    assert!(call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"synth"}"#).is_err());
    let kit = call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"kit"}"#).unwrap();
    assert_eq!(get(&kit, &["tracks"]).to_json(), r#"[{"trackName":"KICK","sound":"kit"}]"#);
    assert_eq!(s.editor.history().undo_len(), 0);
}

#[test]
fn a_song_file_is_loaded_by_path() {
    let mut s = session();
    let path = std::env::temp_dir().join(format!("ordrumbox_mcp_{}.odbox", std::process::id()));
    store::save_song(&path, &s.song_record("savedAt")).unwrap();
    call(&mut s, "removePattern", r#"{"patternName":"jazz"}"#).unwrap();
    let args = object([("filePath", Value::from(path.display().to_string()))]);
    let loaded = s.mcp_call("loadSong", &args).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(get(&loaded, &["patterns"]).as_f64(), Some(2.0));
    assert_eq!(s.editor.library().patterns.len(), 2);
}

const RATE: f32 = 8000.0;

fn write_wav(path: &Path, level: f32) {
    let buffer = SampleBuffer::mono((0..400).map(|i| level * (i as f32 * 0.3).sin()).collect(), RATE as u32);
    std::fs::write(path, wav::encode_float(&buffer)).unwrap();
}

/// The session of `session()` with kit `a` (a KICK at `k.wav`) and an
/// empty kit `b`, kept in `dir`.
fn session_with_kits(dir: &Path) -> Session {
    let sample =
        KitSample { key: "KICK".into(), display_name: "kick".into(), path: "k.wav".into(), ..Default::default() };
    let kits = vec![
        Kit { name: "a".into(), samples: vec![sample.clone()], ..Kit::default() },
        Kit { name: "b".into(), ..Kit::default() },
    ];
    let buffer = Arc::new(SampleBuffer::mono(vec![0.25, -0.5, 0.1], RATE as u32));
    let bank = SampleBank { samples: vec![BankSample { kit: "a".into(), sample, buffer }] };
    let kits = KitSet::new(kits, &bank, None, Some(dir.join("kits.json")), Some(dir.join("samples")));
    let mut s = session();
    s.kits = kits;
    s.kits_changed("kits loaded".into());
    s
}

#[test]
fn kits_are_listed_chosen_and_changed() {
    let dir = scratch_dir("kits");
    let mut s = session_with_kits(&dir);
    let listed = call(&mut s, "listKitSamples", "{}").unwrap();
    assert_eq!(get(&listed, &["kit"]).as_str(), Some("a"));
    assert_eq!(get(&listed, &["kits"]).as_array().map(<[Value]>::len), Some(2));
    assert_eq!(call(&mut s, "selectKit", r#"{"kit":"B"}"#).unwrap().to_json(), r#"{"kit":"b"}"#);
    assert_eq!(s.kit_index, 1);
    assert!(call(&mut s, "selectKit", r#"{"kit":"909"}"#).is_err());

    let normalized = call(&mut s, "normalizeKit", r#"{"kit":"a"}"#).unwrap();
    assert_eq!(normalized.to_json(), r#"{"kit":"a","normalized":1}"#);
    let analyzed = call(&mut s, "analyzeSamples", r#"{"samples":["k.wav","none.wav"]}"#).unwrap();
    let results = get(&analyzed, &["results"]).as_array().unwrap();
    assert!(get(&results[0], &["peakDb"]).as_f64().is_some());
    assert!(results[1].get("error").is_some());
    assert!(call(&mut s, "analyzeSamples", r#"{"samples":[1]}"#).is_err());

    let snare = dir.join("SnareDrum.wav");
    write_wav(&snare, 0.5);
    let args = object([("kit", Value::from("a")), ("filePath", Value::from(snare.display().to_string()))]);
    let added = s.mcp_call("addSampleToKit", &args).unwrap();
    assert_eq!(get(&added, &["key"]).as_str(), Some("SNARE"));
    assert_eq!(s.kits.kits[0].samples.len(), 2);

    let mut args = object([
        ("kit", Value::from("a")),
        ("samplePath", Value::from("k.wav")),
        ("filePath", Value::from(snare.display().to_string())),
    ]);
    s.mcp_call("replaceKitSample", &args).unwrap();
    assert_eq!(s.kits.kits[0].samples[0].display_name, "SnareDrum.wav");
    if let Some(fields) = args.as_object_mut() {
        fields.insert("samplePath", Value::from("none.wav"));
    }
    assert!(s.mcp_call("replaceKitSample", &args).is_err());

    let folder = dir.join("tr8");
    std::fs::create_dir_all(&folder).unwrap();
    write_wav(&folder.join("Kick.wav"), 0.5);
    let args = object([("folderPath", Value::from(folder.display().to_string()))]);
    let imported = s.mcp_call("importKitFolder", &args).unwrap();
    assert_eq!(get(&imported, &["kit"]).as_str(), Some("tr8"));
    assert_eq!(get(&imported, &["samples"]).as_array().map(<[Value]>::len), Some(1));

    let mapping = dir.join("mapping.json");
    s.kits.export_mapping(0, &mapping).unwrap();
    s.kits.kits[0].samples.clear();
    let args = object([("filePath", Value::from(mapping.display().to_string()))]);
    let loaded = s.mcp_call("loadKitMapping", &args).unwrap();
    assert_eq!((get(&loaded, &["kit"]).as_str(), s.kits.kits[0].samples.len()), (Some("a"), 2));

    let random = call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"random"}"#).unwrap();
    let sound = get(&random, &["tracks"]).as_array().map(|t| get(&t[0], &["sound"]).clone());
    assert!(sound.as_ref().and_then(Value::as_str).is_some_and(|path| path != "kit"), "{sound:?}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn playback_is_started_followed_and_stopped() {
    let mut s = session();
    let sample = KitSample { key: "KICK".into(), path: "k".into(), decay: Some(100.0), ..Default::default() };
    let buffer = SampleBuffer::mono(vec![0.5; 400], RATE as u32).into();
    let bank = Arc::new(SampleBank { samples: vec![BankSample { kit: "x".into(), sample, buffer }] });
    let (controller, mut audio) =
        Controller::with_rng(RATE, bank, Scales::default(), SynthPresets::default(), Rng::new(1));
    s.playback = Playback::without_output(controller);
    let mut render = |s: &mut Session| {
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        for _ in 0..8 {
            s.update();
            audio.render(&mut l, &mut r);
        }
    };

    assert_eq!(call(&mut s, "play", "{}").unwrap().to_json(), r#"{"playing":"rock"}"#);
    render(&mut s);
    let state = call(&mut s, "getPlaybackState", "{}").unwrap();
    assert_eq!(get(&state, &["patternName"]).as_str(), Some("rock"));
    assert!(get(&state, &["beat"]).as_f64().is_some(), "{state}");
    let auditioned = call(&mut s, "auditionTrack", r#"{"patternName":"rock","trackName":"kick"}"#).unwrap();
    assert_eq!(auditioned.to_json(), r#"{"trackName":"KICK"}"#);

    assert_eq!(call(&mut s, "play", r#"{"song":true}"#), Err("No arrangement to play: createArrangement".into()));
    call(&mut s, "createArrangement", r#"{"name":"A","clips":[{"patternName":"rock"}]}"#).unwrap();
    assert_eq!(call(&mut s, "play", r#"{"song":true}"#).unwrap().to_json(), r#"{"playing":"A"}"#);
    render(&mut s);
    let state = call(&mut s, "getPlaybackState", "{}").unwrap();
    assert_eq!(get(&state, &["arrangement"]).as_str(), Some("A"));
    assert!(get(&state, &["measure"]).as_f64().is_some(), "{state}");
    // An arrangement removed while it plays stops it.
    call(&mut s, "removeArrangement", r#"{"arrangement":"A"}"#).unwrap();
    assert_eq!(call(&mut s, "getPlaybackState", "{}").unwrap().to_json(), r#"{"playing":false}"#);

    call(&mut s, "play", "{}").unwrap();
    assert_eq!(call(&mut s, "stop", "{}").unwrap().to_json(), r#"{"playing":false}"#);
    assert_eq!(s.message, "stopped");
}

#[test]
fn songs_are_saved_listed_and_loaded_by_name() {
    let dir = scratch_dir("songs");
    let mut s = session();
    s.saving.songs_dir = Some(dir.clone());
    call(&mut s, "setSongInfo", r#"{"name":"Live set"}"#).unwrap();
    let saved = call(&mut s, "saveSong", "{}").unwrap();
    assert!(get(&saved, &["filePath"]).as_str().is_some_and(|p| p.starts_with(&*dir.to_string_lossy())));
    let listed = call(&mut s, "listSavedSongs", "{}").unwrap();
    let names = get(&listed, &["songs"]).as_array().unwrap();
    assert_eq!(names.len(), 1);
    let name = names[0].as_str().unwrap().to_owned();

    call(&mut s, "removePattern", r#"{"patternName":"jazz"}"#).unwrap();
    let args = object([("name", Value::from(name.to_uppercase()))]);
    let loaded = s.mcp_call("loadSong", &args).unwrap();
    assert_eq!(get(&loaded, &["patterns"]).as_f64(), Some(2.0));
    assert!(call(&mut s, "loadSong", r#"{"name":"none"}"#).is_err());
    assert!(call(&mut s, "loadSong", r#"{"name":"a","filePath":"b"}"#).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn arguments_and_states_that_refuse_a_call() {
    let mut s = session();
    s.ask(Pending::DemoLibrary);
    assert!(call(&mut s, "listPatterns", "{}").unwrap_err().contains("confirmation"));
    s.chrome.overlay = None;

    // No pattern named: the one shown.
    assert_eq!(get(&call(&mut s, "selectPattern", "{}").unwrap(), &["name"]).as_str(), Some("rock"));
    assert!(call(&mut s, "setSongInfo", "{}").is_err());
    assert!(call(&mut s, "createSynthPreset", r#"{"preset":" "}"#).is_err());
    assert!(call(&mut s, "createSynthPreset", r#"{"preset":"a","settings":3}"#).is_err());
    assert!(call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"pads"}"#).is_err());

    // The preset the synth editor shows follows the tools.
    call(&mut s, "createSynthPreset", r#"{"preset":"a"}"#).unwrap();
    call(&mut s, "createSynthPreset", r#"{"preset":"b"}"#).unwrap();
    s.load_synth("a");
    call(&mut s, "updateSynthPreset", r#"{"preset":"a","settings":{"filter.freq":900}}"#).unwrap();
    call(&mut s, "removeSynthPreset", r#"{"preset":"a"}"#).unwrap();
    assert!(s.synth_editor.edits("b"));

    let synth = call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"synth"}"#).unwrap();
    assert_eq!(get(&synth, &["tracks"]).to_json(), r#"[{"trackName":"KICK","sound":"synth:b"}]"#);
}
