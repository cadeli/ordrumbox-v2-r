//! The generators against the JS ones (`tests/fixtures/generators.json`,
//! written by `tools/export_gen_fixtures.mjs`): with the same random stream,
//! the same notes, the same track settings and the same number of draws.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::drum_taxonomy::InstrumentFamily;
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Note, Pattern, Scales, Track};
use ordrumbox_logic::generate::{
    AutoGenerator, Harmony, MelodicKind, Options, generate_pattern, instrument_line, random_track, run_loop,
    toggle_drums, toggle_melodic,
};

/// mulberry32, the stream the fixture script gives `Math.random`.
struct Stream {
    state: u32,
    draws: u64,
}

impl Stream {
    fn new(seed: f64) -> Self {
        Self { state: seed as u32, draws: 0 }
    }

    fn next(&mut self) -> f64 {
        self.draws += 1;
        self.state = self.state.wrapping_add(0x6d2b_79f5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }
}

fn fixtures() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/generators.json"))
        .expect("tests/fixtures/generators.json (run tools/export_gen_fixtures.mjs)");
    json::parse(&text).expect("fixture parses")
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or_else(|| panic!("no {key}"))
}

fn f(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("no number {key} in {value}"))
}

// ─── The JS shapes ───────────────────────────────────────────────────────────

/// `noteOut` of the script: `[velocity, beat, beatStep, pitch, {other non-default keys, stepPercent}]`.
fn note_out(note: &Note) -> Value {
    let defaults = Note::default();
    let mut extra = Object::new();
    for key in [
        "pan",
        "every",
        "prob",
        "rate",
        "retriggerCount",
        "arp",
        "arpTriggerProbability",
        "euclideanFill",
        "euclideanRotation",
        "pos",
    ] {
        let mut value = note.field(key).unwrap_or(Value::Null);
        // The script writes an arpeggio's direction under its JS key, `mode`.
        if let Value::Object(arp) = &mut value
            && let Some(direction) = arp.remove("direction")
        {
            arp.insert("mode", direction);
        }
        if Some(&value) != defaults.field(key).as_ref() {
            extra.insert(key, value);
        }
    }
    if let Some(percent) = note.step_percent {
        extra.insert("stepPercent", Value::Number(percent));
    }
    let mut out =
        vec![Value::Number(note.velocity), Value::from(note.beat), Value::from(note.step), Value::Number(note.pitch)];
    if !extra.is_empty() {
        out.push(Value::Object(extra));
    }
    Value::Array(out)
}

fn note_in(value: &Value) -> Note {
    let v = value.as_array().expect("note array");
    let mut note = Note {
        velocity: v[0].as_f64().unwrap(),
        beat: v[1].as_f64().unwrap() as i32,
        step: v[2].as_f64().unwrap() as i32,
        pitch: v[3].as_f64().unwrap(),
        ..Note::default()
    };
    if let Some(extra) = v.get(4).and_then(Value::as_object) {
        for (key, value) in extra.iter() {
            if key == "stepPercent" {
                note.step_percent = value.as_f64();
            } else {
                note.set_field(key, value);
            }
        }
    }
    note
}

/// A track as the script wrote it (`trackOut`).
fn track_in(value: &Value) -> Track {
    let mut track = Track::default();
    for (key, value) in value.as_object().unwrap().iter() {
        match key {
            "notes" => track.notes = value.as_array().unwrap().iter().map(note_in).collect(),
            "_toolbarAuto" => _ = track.extra.insert(key, value.clone()),
            _ => _ = track.set_field(key, value),
        }
    }
    track
}

fn pattern_in(value: &Value) -> Pattern {
    let mut pattern = Pattern::default();
    for (key, value) in value.as_object().unwrap().iter() {
        match key {
            "tracks" => pattern.tracks = value.as_array().unwrap().iter().map(track_in).collect(),
            "beatCount" => pattern.beat_count = value.as_f64().unwrap() as i32,
            _ => _ = pattern.extra.insert(current_pattern_key(key), value.clone()),
        }
    }
    pattern
}

/// The generator's pattern keys: the script writes their JS names.
const PATTERN_KEYS: [(&str, &str); 3] =
    [("_autoGenGenre", "autoGenre"), ("_autoGenKeyOffset", "autoKeyOffset"), ("_autoGenScale", "autoScale")];

fn current_pattern_key(key: &str) -> &str {
    PATTERN_KEYS.iter().find(|(js, _)| *js == key).map_or(key, |(_, current)| current)
}

/// Compare a track with the script's `trackOut`: every key it wrote, and the
/// optional keys it did not write must be unset here.
fn check_track(track: &Track, expected: &Value, at: &str) {
    let object = expected.as_object().unwrap();
    for (key, value) in object.iter() {
        let actual = match key {
            "notes" => Value::Array(track.notes.iter().map(note_out).collect()),
            "_toolbarAuto" => track.extra.get(key).cloned().unwrap_or(Value::Null),
            _ => track.field(key).unwrap_or(Value::Null),
        };
        if actual != *value {
            if key == "notes" {
                let (a, e) = (actual.as_array().unwrap(), value.as_array().unwrap());
                let first = a.iter().zip(e).position(|(a, e)| a != e).unwrap_or(a.len().min(e.len()));
                panic!(
                    "{at}: notes differ at {first} ({} vs {} notes)\n  rust: {}\n  js:   {}",
                    a.len(),
                    e.len(),
                    a.get(first).map_or("-".into(), Value::to_json),
                    e.get(first).map_or("-".into(), Value::to_json)
                );
            }
            panic!("{at}: {key} is {actual}, JS {value}");
        }
    }
    if !object.contains_key("_toolbarAuto") {
        assert_eq!(track.extra.get("_toolbarAuto"), None, "{at}: _toolbarAuto set, JS left it unset");
    }
    for key in ["auto_variation", "auto_variation2"] {
        if !object.contains_key(key) {
            assert_eq!(track.field(key), Some(Value::Null), "{at}: {key} set, JS left it unset");
        }
    }
}

fn check_pattern(pattern: &Pattern, expected: &Value, at: &str) {
    for key in ["autoGen", "_autoGenGenre", "_autoGenKeyOffset", "_autoGenScale"] {
        assert_eq!(pattern.extra.get(current_pattern_key(key)), expected.get(key), "{at}: {key}");
    }
    let tracks = items(expected, "tracks");
    let names: Vec<&str> = pattern.tracks.iter().map(|t| t.name.as_str()).collect();
    let expected_names: Vec<&str> = tracks.iter().map(|t| t.get("name").and_then(Value::as_str).unwrap()).collect();
    assert_eq!(names, expected_names, "{at}: tracks");
    for (track, expected) in pattern.tracks.iter().zip(tracks) {
        check_track(track, expected, &format!("{at} {}", track.name));
    }
}

fn harmony_in(value: Option<&Value>) -> Harmony {
    match value {
        Some(h) if !h.is_null() => {
            Harmony { root: f(h, "root"), scale: h.get("scale").and_then(Value::as_str).map(str::to_owned) }
        }
        _ => Harmony::default(),
    }
}

// ─── Cases ───────────────────────────────────────────────────────────────────

#[test]
fn every_variant_of_every_generator() {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    let cases = items(&fixtures, "tracks");
    assert!(cases.len() > 500);
    for case in cases {
        let kind = match case.get("kind").and_then(Value::as_str).unwrap() {
            "kick" => InstrumentFamily::Kick,
            "snare" => InstrumentFamily::Snare,
            "hat" => InstrumentFamily::Hat,
            "clap" => InstrumentFamily::Clap,
            "cowbell" => InstrumentFamily::Cowbell,
            "perc" => InstrumentFamily::Perc,
            "melody" => InstrumentFamily::Piano,
            "bass" => InstrumentFamily::Bass,
            other => panic!("unknown kind {other}"),
        };
        let variant = case.get("variant").and_then(Value::as_str);
        let name = case.get("name").and_then(Value::as_str).unwrap();
        let (beats, spb) = (f(case, "beatCount") as i32, f(case, "stepsPerBeat") as i32);
        let mut pattern = Pattern { tracks: vec![Track::new(name, beats, spb)], ..Pattern::default() };
        if let Some(bass) = case.get("bass").and_then(Value::as_array) {
            let mut track = Track::new("BASS", beats, 4);
            for (n, pitch) in bass.iter().enumerate() {
                let step_percent = Some(0.0);
                let note =
                    Note { beat: n as i32 % beats, pitch: pitch.as_f64().unwrap(), step_percent, ..Note::default() };
                track.notes.push(note);
            }
            pattern.tracks.insert(0, track);
        }
        let t = pattern.tracks.len() - 1;
        let at = format!(
            "{} {:?} {name} {beats}x{spb} d{} seed {}",
            case.get("kind").unwrap(),
            variant,
            f(case, "density"),
            f(case, "seed")
        );
        let mut stream = Stream::new(f(case, "seed"));
        let mut rnd = || stream.next();
        let mut g = AutoGenerator::new(&scales, &mut rnd);
        instrument_line(&mut g, &mut pattern, t, kind, variant, f(case, "density"), &harmony_in(case.get("harmony")));
        drop(g);
        check_track(&pattern.tracks[t], case.get("track").unwrap(), &at);
        assert_eq!(stream.draws as f64, f(case, "draws"), "{at}: draws");
    }
}

#[test]
fn random_tracks() {
    let fixtures = fixtures();
    for case in items(&fixtures, "random") {
        let mut track = Track::new("T1", f(case, "beatCount") as i32, f(case, "stepsPerBeat") as i32);
        let mut stream = Stream::new(f(case, "seed"));
        random_track(&mut track, &mut || stream.next());
        let at = format!("random seed {}", f(case, "seed"));
        check_track(&track, case.get("track").unwrap(), &at);
        assert_eq!(stream.draws as f64, f(case, "draws"), "{at}: draws");
    }
}

#[test]
fn whole_patterns() {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    for case in items(&fixtures, "patterns") {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let options = match case.get("options").filter(|o| !o.is_null()) {
            Some(o) => Options {
                genre: o.get("genre").and_then(Value::as_str).map(str::to_owned),
                key_offset: o.get("keyOffset").and_then(Value::as_f64),
                scale: o.get("scale").and_then(Value::as_str).map(str::to_owned),
            },
            None => Options::default(),
        };
        let at = format!("pattern seed {}", f(case, "seed"));
        let mut stream = Stream::new(f(case, "seed"));
        let mut rnd = || stream.next();
        generate_pattern(&mut AutoGenerator::new(&scales, &mut rnd), &mut pattern, &options);
        check_pattern(&pattern, case.get("after").unwrap(), &at);
        assert_eq!(stream.draws as f64, f(case, "draws"), "{at}: draws");
    }
}

#[test]
fn loop_after_loop() {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    for case in items(&fixtures, "loops") {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for (index, expected) in items(case, "after").iter().enumerate() {
            let at = format!("loops seed {} loop {index}", f(case, "seed"));
            let start = stream.draws;
            let mut rnd = || stream.next();
            run_loop(&mut AutoGenerator::new(&scales, &mut rnd), &mut pattern, index as u64);
            check_pattern(&pattern, expected.get("pattern").unwrap(), &at);
            assert_eq!((stream.draws - start) as f64, f(expected, "draws"), "{at}: draws");
        }
    }
}

#[test]
fn toolbar_toggles() {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    for case in items(&fixtures, "toggles") {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for (i, expected) in items(case, "after").iter().enumerate() {
            let step = expected.get("step").and_then(Value::as_str).unwrap();
            let at = format!("toggles seed {} step {i} {step}", f(case, "seed"));
            let start = stream.draws;
            let mut rnd = || stream.next();
            let mut g = AutoGenerator::new(&scales, &mut rnd);
            match step {
                "drums" => toggle_drums(&mut g, &mut pattern),
                "bass" => toggle_melodic(&mut g, &mut pattern, MelodicKind::Bass),
                _ => toggle_melodic(&mut g, &mut pattern, MelodicKind::Chords),
            };
            drop(g);
            check_pattern(&pattern, expected.get("pattern").unwrap(), &at);
            assert_eq!((stream.draws - start) as f64, f(expected, "draws"), "{at}: draws");
        }
    }
}
