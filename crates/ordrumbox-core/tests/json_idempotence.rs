//! Saving a library is idempotent, setting by setting: a library holding
//! every note, track, pattern, song and header field at a value other than
//! its default is saved, read back and saved again; the second save writes
//! the first one's text, reading it reports nothing, and every value is the
//! one written. The first save may normalize (web-version keys, an old
//! arpeggio `mode`); the saves after it never change the file.

use std::path::Path;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Note, Track};

/// A non-default value of every note field, in `Note::KEYS` order.
const NOTE_VALUES: [(&str, &str); 16] = [
    ("velocity", "0.55"),
    ("beat", "5"),
    ("step", "2"),
    ("pitch", "7.5"),
    ("pan", "-0.25"),
    ("cycle", "3"),
    ("probability", "0.75"),
    ("retriggerRate", "9"),
    ("retriggerCount", "4"),
    ("arp", r#"{"intervals":[0,4,7],"direction":"up"}"#),
    ("retriggerProbability", "0.6"),
    ("euclideanPulses", "5"),
    ("euclideanRotation", "2"),
    ("cycleOffset", "1"),
    ("arpScale", r#""minor""#),
    ("arpDirection", r#""down""#),
];

const LFO: &str = r#"{"freq":2,"min":0.2,"max":0.9,"phase":0.25,"type":"triangle"}"#;

/// A non-default value of every track field, in `Track::KEYS` order.
const TRACK_VALUES: [(&str, &str); 55] = [
    ("name", r#""SNARE""#),
    ("autoAssign", "false"),
    ("samplePath", r#""real/snare.wav""#),
    ("beatCount", "8"),
    ("stepsPerBeat", "3"),
    ("loopLength", "20"),
    ("swingResolution", "2"),
    ("swingAmount", "0.3"),
    ("volume", "0.7"),
    ("volumeLfo", LFO),
    ("transpose", "-5"),
    ("transposeLfo", LFO),
    ("pan", "0.35"),
    ("panLfo", LFO),
    ("solo", "true"),
    ("mute", "true"),
    ("autoMode", "true"),
    ("useSynth", "true"),
    ("mono", "true"),
    ("hitVariation", "40"),
    ("noteVariation", "60"),
    ("probability", "0.9"),
    ("autoPitchChance", "11"),
    ("autoVelocityChance", "12"),
    ("autoSilenceChance", "13"),
    ("autoFillChance", "14"),
    ("autoGhostChance", "15"),
    ("autoRetriggerChance", "16"),
    ("autoEuclideanChance", "17"),
    ("autoNoteProbabilityChance", "18"),
    ("autoNoteProbabilityRange", "70"),
    ("autoArpChance", "19"),
    ("pitchRange", "5"),
    ("pitchScaleLock", "true"),
    ("autoVariant", r#""funk""#),
    ("autoDensity", "0.25"),
    ("filterType", r#""lowpass""#),
    ("filterCutoffLfo", LFO),
    ("filterCutoff", "1200"),
    ("filterQLfo", LFO),
    ("filterQ", "2.5"),
    ("reverbType", r#""hall""#),
    ("reverbMix", "0.4"),
    ("delayType", r#""pingpong""#),
    ("delayBeats", "0.75"),
    ("delayMix", "0.2"),
    ("saturationType", r#""hard""#),
    ("saturationMix", "0.15"),
    ("saturationOn", "false"),
    ("reverbOn", "false"),
    ("delayOn", "false"),
    ("synthPreset", r#""BASS0""#),
    ("autoHitVariation", "35"),
    ("autoNoteVariation", "45"),
    ("scaleName", r#""minor""#),
];

/// Web-version note keys and the key each is saved under.
const NOTE_WEB_KEYS: [(&str, &str); 10] = [
    ("beatStep", "step"),
    ("every", "cycle"),
    ("prob", "probability"),
    ("rate", "retriggerRate"),
    ("arpTriggerProbability", "retriggerProbability"),
    ("euclideanFill", "euclideanPulses"),
    ("everyOffset", "cycleOffset"),
    ("pos", "cycleOffset"),
    ("_arpScale", "arpScale"),
    ("_arpType", "arpDirection"),
];

/// Web-version track keys and the key each is saved under.
const TRACK_WEB_KEYS: [(&str, &str); 45] = [
    ("useAutoAssignSound", "autoAssign"),
    ("sampleId", "samplePath"),
    ("loopAtStep", "loopLength"),
    ("velocity", "volume"),
    ("velocityLfo", "volumeLfo"),
    ("pitch", "transpose"),
    ("pitchLfo", "transposeLfo"),
    ("auto", "autoMode"),
    ("useSoftSynth", "useSynth"),
    ("variation", "hitVariation"),
    ("variation2", "noteVariation"),
    ("probPitch", "autoPitchChance"),
    ("prob_pitch", "autoPitchChance"),
    ("probVelocity", "autoVelocityChance"),
    ("prob_velocity", "autoVelocityChance"),
    ("probSilence", "autoSilenceChance"),
    ("prob_silence", "autoSilenceChance"),
    ("probFill", "autoFillChance"),
    ("prob_fill", "autoFillChance"),
    ("probGhost", "autoGhostChance"),
    ("prob_ghost", "autoGhostChance"),
    ("probRetrigger", "autoRetriggerChance"),
    ("prob_retrig", "autoRetriggerChance"),
    ("probEuclidean", "autoEuclideanChance"),
    ("prob_euclid", "autoEuclideanChance"),
    ("probNote", "autoNoteProbabilityChance"),
    ("prob_note", "autoNoteProbabilityChance"),
    ("probNoteDepth", "autoNoteProbabilityRange"),
    ("prob_note_depth", "autoNoteProbabilityRange"),
    ("probArp", "autoArpChance"),
    ("prob_arp", "autoArpChance"),
    ("pitch_range", "pitchRange"),
    ("pitch_scale_lock", "pitchScaleLock"),
    ("auto_variant", "autoVariant"),
    ("auto_density", "autoDensity"),
    ("filterFreqLfo", "filterCutoffLfo"),
    ("filterFreq", "filterCutoff"),
    ("reverbAmount", "reverbMix"),
    ("delayTime", "delayBeats"),
    ("delayDepth", "delayMix"),
    ("saturationAmount", "saturationMix"),
    ("sat", "saturationOn"),
    ("synthSoundKey", "synthPreset"),
    ("auto_variation", "autoHitVariation"),
    ("auto_variation2", "autoNoteVariation"),
];

fn parse(text: &str) -> Value {
    json::parse(text).unwrap_or_else(|e| panic!("{text}: {e}"))
}

fn object(pairs: &[(&str, Value)]) -> Value {
    let mut object = Object::new();
    for (key, value) in pairs {
        object.insert(*key, value.clone());
    }
    Value::Object(object)
}

/// `pairs` with their values parsed.
fn parsed<'a>(pairs: &[(&'a str, &str)]) -> Vec<(&'a str, Value)> {
    pairs.iter().map(|&(key, value)| (key, parse(value))).collect()
}

/// A library of `patterns`.
fn library_of(patterns: Vec<Value>) -> Value {
    object(&[("patterns", Value::Array(patterns))])
}

/// A pattern holding `tracks`.
fn pattern(name: &str, tracks: Vec<Value>) -> Value {
    object(&[("name", Value::from(name)), ("tracks", Value::Array(tracks))])
}

/// A track holding `notes` (objects).
fn track(name: &str, notes: Vec<Value>) -> Value {
    object(&[("name", Value::from(name)), ("notes", Value::Array(notes))])
}

/// Load `value`, save it, load and save again: the second save is the first
/// one's text, reading it reports nothing and gives the same library. The
/// library read back from the first save.
fn saved_twice(value: &Value) -> Library {
    let (library, _) = Library::from_json(value);
    let first = library.to_file_text();
    let (reloaded, report) = Library::parse(&first).expect("the saved text parses");
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
    assert!(report.dropped_clips.is_empty(), "{:#?}", report.dropped_clips);
    assert_eq!(reloaded.to_file_text(), first, "a second save changes the file");
    assert_eq!(reloaded, library, "the file does not hold the whole library");
    reloaded
}

fn assert_note_fields(note: &Note, values: &[(&str, Value)], at: &str) {
    for (key, value) in values {
        assert_eq!(note.field(key).as_ref(), Some(value), "{at}: note {key}");
    }
}

fn assert_track_fields(track: &Track, values: &[(&str, Value)], at: &str) {
    for (key, value) in values {
        assert_eq!(track.field(key).as_ref(), Some(value), "{at}: track {key}");
    }
}

// ─── Notes ──────────────────────────────────────────────────────────────────

#[test]
fn the_test_tables_name_every_field() {
    let note_keys: Vec<&str> = NOTE_VALUES.iter().map(|(key, _)| *key).collect();
    assert_eq!(note_keys, Note::KEYS, "a note field has no test value");
    let track_keys: Vec<&str> = TRACK_VALUES.iter().map(|(key, _)| *key).collect();
    assert_eq!(track_keys, Track::KEYS, "a track field has no test value");
    for (key, value) in parsed(&NOTE_VALUES) {
        assert_ne!(Note::default().field(key), Some(value), "note {key}: the test value is the default");
    }
    for (key, value) in parsed(&TRACK_VALUES) {
        assert_ne!(Track::default().field(key), Some(value), "track {key}: the test value is the default");
    }
}

#[test]
fn every_note_field_survives_saving_together_and_alone() {
    let values = parsed(&NOTE_VALUES);
    // One note with every field set, then one note per field: the compact
    // format leaves trailing defaults out and lists only the keys used.
    let mut notes = vec![object(&values)];
    notes.extend(values.iter().map(|(key, value)| object(&[(key, value.clone())])));
    let library = saved_twice(&library_of(vec![pattern("notes", vec![track("ALL", notes)])]));
    let notes = &library.patterns[0].tracks[0].notes;
    assert_note_fields(&notes[0], &values, "every field");
    for (i, (key, value)) in values.iter().enumerate() {
        let note = &notes[i + 1];
        assert_note_fields(note, &[(key, value.clone())], key);
        let mut alone = Note::default();
        alone.set_field(key, value);
        assert_eq!(*note, alone, "{key}: the other fields stay at their default");
    }
}

#[test]
fn every_arpeggio_form_survives_saving() {
    let forms = ["[0,3,7]", r#""0,4,7""#, r#""updown""#, r#"{"intervals":[0,7],"direction":"random"}"#, "null"];
    let notes = forms.iter().map(|arp| object(&[("arp", parse(arp)), ("retriggerCount", Value::from(3))])).collect();
    let library = saved_twice(&library_of(vec![pattern("arps", vec![track("ARP", notes)])]));
    for (note, arp) in library.patterns[0].tracks[0].notes.iter().zip(forms) {
        assert_eq!(note.arp, parse(arp));
    }
    // An old save's `mode` is read as `direction`, and saved so.
    let old = object(&[("arp", parse(r#"{"intervals":[0,5],"mode":"down"}"#))]);
    let library = saved_twice(&library_of(vec![pattern("old", vec![track("ARP", vec![old])])]));
    assert_eq!(library.patterns[0].tracks[0].notes[0].arp, parse(r#"{"intervals":[0,5],"direction":"down"}"#));
}

#[test]
fn web_note_keys_are_saved_under_their_current_name() {
    let values = parsed(&NOTE_VALUES);
    for (web, current) in NOTE_WEB_KEYS {
        assert_eq!(Note::canonical_key(web), Some(current), "{web}");
        let value = values.iter().find(|(key, _)| *key == current).map(|(_, v)| v.clone()).unwrap();
        let written =
            |key: &str| library_of(vec![pattern("p", vec![track("T", vec![object(&[(key, value.clone())])])])]);
        let from_web = saved_twice(&written(web));
        assert_eq!(from_web, saved_twice(&written(current)), "{web}");
        assert!(!from_web.to_file_text().contains(&format!("\"{web}\"")), "{web} written back");
    }
}

// ─── Tracks ─────────────────────────────────────────────────────────────────

#[test]
fn every_track_field_survives_saving_together_and_alone() {
    let values = parsed(&TRACK_VALUES);
    // Notes on the track's own grid (3 steps a beat, 8 beats).
    let notes = Value::Array(vec![object(&parsed(&NOTE_VALUES)), parse(r#"{"beat":7,"step":1}"#)]);
    let mut all = values.clone();
    all.push(("notes", notes));
    all.push(("someFutureKey", parse(r#"{"kept":[1,2]}"#)));
    // One pattern per field: a track of that field alone.
    let mut patterns = vec![pattern("all", vec![object(&all)])];
    patterns.extend(values.iter().map(|(key, value)| pattern(key, vec![object(&[(key, value.clone())])])));
    let library = saved_twice(&library_of(patterns));

    let track = &library.patterns[0].tracks[0];
    assert_track_fields(track, &values, "every field");
    assert_eq!(track.notes.len(), 2);
    assert_note_fields(&track.notes[0], &parsed(&NOTE_VALUES), "a note of the track");
    assert_eq!(track.extra.get("someFutureKey"), Some(&parse(r#"{"kept":[1,2]}"#)), "an unknown key is kept");
    for (i, (key, value)) in values.iter().enumerate() {
        assert_track_fields(&library.patterns[i + 1].tracks[0], &[(key, value.clone())], key);
    }
}

#[test]
fn a_pan_is_kept_against_the_track_name() {
    // A pan that differs from the one the name implies is saved, even at 0:
    // a renamed track is not re-panned on load.
    let tracks = ["OHH", "KICK", "T"]
        .iter()
        .flat_map(|name| [0.0, 0.35].map(|pan| object(&[("name", Value::from(*name)), ("pan", Value::from(pan))])))
        .collect();
    let library = saved_twice(&library_of(vec![pattern("pans", tracks)]));
    let pans: Vec<f64> = library.patterns[0].tracks.iter().map(|t| t.pan).collect();
    assert_eq!(pans, [0.0, 0.35, 0.0, 0.35, 0.0, 0.35]);
}

#[test]
fn web_track_keys_are_saved_under_their_current_name() {
    let values = parsed(&TRACK_VALUES);
    for (web, current) in TRACK_WEB_KEYS {
        assert_eq!(Track::canonical_key(web), Some(current), "{web}");
        let value = values.iter().find(|(key, _)| *key == current).map(|(_, v)| v.clone()).unwrap();
        let written = |key: &str| library_of(vec![pattern("p", vec![object(&[(key, value.clone())])])]);
        let from_web = saved_twice(&written(web));
        assert_eq!(from_web, saved_twice(&written(current)), "{web}");
        assert!(!from_web.to_file_text().contains(&format!("\"{web}\"")), "{web} written back");
    }
}

// ─── Patterns, songs, the library ───────────────────────────────────────────

#[test]
fn every_pattern_field_survives_saving() {
    let full = parse(
        r#"{"id":"groove-1","name":"Groove","beatCount":12,"bpm":133.5,"description":"a groove",
            "tags":["funk","70s"],"autoGenre":"rock","autoKeyOffset":3,"autoScale":"dorian","custom":{"x":1},
            "tracks":[{"name":"KICK"}]}"#,
    );
    let library = saved_twice(&library_of(vec![full]));
    let p = &library.patterns[0];
    assert_eq!((p.id.as_str(), p.name.as_deref(), p.beat_count, p.bpm), ("groove-1", Some("Groove"), 12, 133.5));
    assert_eq!((p.description.as_str(), p.tags.as_slice()), ("a groove", &["funk".to_owned(), "70s".to_owned()][..]));
    for (key, value) in [("autoGenre", r#""rock""#), ("autoKeyOffset", "3"), ("autoScale", r#""dorian""#)] {
        assert_eq!(p.extra.get(key), Some(&parse(value)), "{key}");
    }
    assert_eq!(p.extra.get("custom"), Some(&parse(r#"{"x":1}"#)));

    // A web-version pattern: object tags, generator keys under their old
    // names, the web header. The first save normalizes it.
    let web = parse(
        r#"{"name":"Groove","tags":{"0":"funk","1":"70s"},"_autoGenGenre":"rock","_autoGenKeyOffset":3,
            "_autoGenScale":"dorian","_revision":7,"application":"orDrumbox","url":"https://x"}"#,
    );
    let current =
        parse(r#"{"name":"Groove","tags":["funk","70s"],"autoGenre":"rock","autoKeyOffset":3,"autoScale":"dorian"}"#);
    assert_eq!(saved_twice(&library_of(vec![web])), saved_twice(&library_of(vec![current])));
}

#[test]
fn every_song_field_survives_saving() {
    let value = parse(
        r#"{"info":{"name":"lib","description":"d","date":"2026-10-29","url":"u","author":{"n":1}},
            "songs":[
              {"id":"intro","name":"Intro","description":"the intro","bpm":98,"loopMeasures":6,
               "clips":[{"patternId":"a","startMeasure":0,"measureCount":0.75},
                        {"patternId":"b","startMeasure":2,"measureCount":2},
                        {"patternId":"a","startMeasure":2,"measureCount":1}]},
              {"id":"empty","name":"","description":"","bpm":120,"clips":[]}],
            "patterns":[{"id":"a","beatCount":3},{"id":"b"}]}"#,
    );
    let library = saved_twice(&value);
    assert_eq!(
        library.info,
        *parse(r#"{"name":"lib","description":"d","date":"2026-10-29","url":"u","author":{"n":1}}"#)
            .as_object()
            .unwrap()
    );
    let song = &library.songs[0];
    assert_eq!((song.id.as_str(), song.name.as_str(), song.description.as_str()), ("intro", "Intro", "the intro"));
    assert_eq!((song.bpm, song.loop_measures), (98.0, Some(6.0)));
    let clips: Vec<(&str, u32, f64)> =
        song.clips.iter().map(|c| (c.pattern_id.as_str(), c.start_measure, c.measure_count)).collect();
    assert_eq!(clips, [("a", 0, 0.75), ("b", 2, 2.0), ("a", 2, 1.0)]);
    assert_eq!(library.songs[1].loop_measures, None);

    // The web keys: `infos`, a clip's `pattern`, `loopMeasureCount`.
    let web = parse(
        r#"{"infos":{"name":"lib"},"songs":[{"id":"s","name":"S","bpm":98,"loopMeasureCount":6,
            "clips":[{"pattern":"a","startMeasure":1,"measureCount":2}]}],"patterns":[{"id":"a"}]}"#,
    );
    let current = parse(
        r#"{"info":{"name":"lib"},"songs":[{"id":"s","name":"S","bpm":98,"loopMeasures":6,
            "clips":[{"patternId":"a","startMeasure":1,"measureCount":2}]}],"patterns":[{"id":"a"}]}"#,
    );
    assert_eq!(saved_twice(&web), saved_twice(&current));
}

#[test]
fn a_song_file_is_saved_as_it_was_read() {
    // A `.odbox` song file: the library, what is selected, when it was saved.
    let library = saved_twice(&parse(
        r#"{"info":{"description":"d","date":"2026"},"songs":[{"id":"s","name":"S","clips":[{"patternId":"b"}]}],
            "patterns":[{"id":"a"},{"id":"b","tracks":[{"name":"KICK","notes":[{"beat":1,"velocity":0.5}]}]}]}"#,
    ));
    let first = library.song_record("My song", 1, 0, ("savedAt", 1_761_000_000_000.0));
    let text = first.to_json_pretty(2);
    let read = Library::from_song_record(&parse(&text), &library.info).expect("a song file");
    assert!(read.report.issues.is_empty(), "{:#?}", read.report.issues);
    assert_eq!((read.name.as_deref(), read.selected_pattern, read.selected_song), (Some("My song"), 1, 0));
    let second = read.library.song_record(
        "My song",
        read.selected_pattern,
        read.selected_song,
        ("savedAt", 1_761_000_000_000.0),
    );
    assert_eq!(second.to_json_pretty(2), text);
}

#[test]
fn the_demo_library_is_saved_as_it_was_read() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data/library.json");
    let text = std::fs::read_to_string(&path).expect("assets/data/library.json");
    saved_twice(&parse(&text));
}
