//! The generators against the JS ones (`tests/fixtures/generators.json`,
//! written by `tools/export_gen_fixtures.mjs`): with the same random stream,
//! the same notes, the same track settings and the same number of draws. What
//! departs from them on purpose is checked against `generators_rust.txt`, and
//! by properties.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
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

fn harmony_in(value: Option<&Value>) -> Harmony {
    match value {
        Some(h) if !h.is_null() => {
            Harmony { root: f(h, "root"), scale: h.get("scale").and_then(Value::as_str).map(str::to_owned) }
        }
        _ => Harmony::default(),
    }
}

// ─── Cases ───────────────────────────────────────────────────────────────────

/// The pattern of a track case, its track's index and its generator.
fn track_case(case: &Value) -> (Pattern, usize, InstrumentFamily) {
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
    let name = case.get("name").and_then(Value::as_str).unwrap();
    let (beats, spb) = (f(case, "beatCount") as i32, f(case, "stepsPerBeat") as i32);
    let mut pattern = Pattern { tracks: vec![Track::new(name, beats, spb)], ..Pattern::default() };
    if let Some(bass) = case.get("bass").and_then(Value::as_array) {
        let mut track = Track::new("BASS", beats, 4);
        for (n, pitch) in bass.iter().enumerate() {
            let step_percent = Some(0.0);
            let note = Note { beat: n as i32 % beats, pitch: pitch.as_f64().unwrap(), step_percent, ..Note::default() };
            track.notes.push(note);
        }
        pattern.tracks.insert(0, track);
    }
    let t = pattern.tracks.len() - 1;
    (pattern, t, kind)
}

#[test]
fn every_variant_of_every_generator() {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    let cases = items(&fixtures, "tracks");
    assert!(cases.len() > 500);
    for case in cases.iter().filter(|c| !departs(c)) {
        let name = case.get("name").and_then(Value::as_str).unwrap();
        let variant = case.get("variant").and_then(Value::as_str);
        let (beats, spb) = (f(case, "beatCount") as i32, f(case, "stepsPerBeat") as i32);
        let (mut pattern, t, kind) = track_case(case);
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

// ─── What departs from the JS ────────────────────────────────────────────────

/// Whether a track case departs from the JS on purpose (`README.md`, "Golden
/// fixtures"): the drum lines are standard rhythms here.
fn departs(case: &Value) -> bool {
    matches!(case.get("kind").and_then(Value::as_str).unwrap(), "kick" | "snare" | "hat" | "clap" | "cowbell")
}

/// One line of the snapshot: what a case made, and its draws.
fn snapshot_line(at: &str, value: &Value, draws: u64) -> String {
    format!("{at} draws {draws} {}", value.to_json())
}

/// The cases that depart from the JS, as this code generates them. They start
/// from the fixture's inputs and seeds.
fn rust_snapshot() -> Vec<String> {
    let fixtures = fixtures();
    let scales = Scales::from_json(fixtures.get("scales").unwrap());
    let mut lines = Vec::new();
    for (n, case) in items(&fixtures, "tracks").iter().enumerate().filter(|(_, c)| departs(c)) {
        let (mut pattern, t, kind) = track_case(case);
        let mut stream = Stream::new(f(case, "seed"));
        let mut rnd = || stream.next();
        let harmony = harmony_in(case.get("harmony"));
        let mut g = AutoGenerator::new(&scales, &mut rnd);
        instrument_line(
            &mut g,
            &mut pattern,
            t,
            kind,
            case.get("variant").and_then(Value::as_str),
            f(case, "density"),
            &harmony,
        );
        drop(g);
        lines.push(snapshot_line(&format!("track {n}"), &pattern.to_json(), stream.draws));
    }
    for (n, case) in items(&fixtures, "patterns").iter().enumerate() {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let options = match case.get("options").filter(|o| !o.is_null()) {
            Some(o) => Options {
                genre: o.get("genre").and_then(Value::as_str).map(str::to_owned),
                key_offset: o.get("keyOffset").and_then(Value::as_f64),
                scale: o.get("scale").and_then(Value::as_str).map(str::to_owned),
            },
            None => Options::default(),
        };
        let mut stream = Stream::new(f(case, "seed"));
        let mut rnd = || stream.next();
        generate_pattern(&mut AutoGenerator::new(&scales, &mut rnd), &mut pattern, &options);
        lines.push(snapshot_line(&format!("pattern {n}"), &pattern.to_json(), stream.draws));
    }
    for (n, case) in items(&fixtures, "loops").iter().enumerate() {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for index in 0..f(case, "loopCount") as usize {
            let start = stream.draws;
            let mut rnd = || stream.next();
            run_loop(&mut AutoGenerator::new(&scales, &mut rnd), &mut pattern, index as u64);
            let at = format!("loops {n} loop {index}");
            lines.push(snapshot_line(&at, &pattern.to_json(), stream.draws - start));
        }
    }
    for (n, case) in items(&fixtures, "toggles").iter().enumerate() {
        let mut pattern = pattern_in(case.get("before").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for (i, step) in items(case, "steps").iter().enumerate() {
            let step = step.as_str().unwrap();
            let start = stream.draws;
            let mut rnd = || stream.next();
            let mut g = AutoGenerator::new(&scales, &mut rnd);
            match step {
                "drums" => toggle_drums(&mut g, &mut pattern),
                "bass" => toggle_melodic(&mut g, &mut pattern, MelodicKind::Bass),
                _ => toggle_melodic(&mut g, &mut pattern, MelodicKind::Chords),
            };
            drop(g);
            let at = format!("toggles {n} step {i} {step}");
            lines.push(snapshot_line(&at, &pattern.to_json(), stream.draws - start));
        }
    }
    lines
}

fn snapshot_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generators_rust.txt")
}

/// Writes `generators_rust.txt` from the generators as they are: `cargo test
/// -p ordrumbox-logic --test generators -- --ignored`, when what they make
/// changes on purpose (`README.md`, "Golden fixtures").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    std::fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn what_departs_from_the_js_matches_its_snapshot() {
    let text = std::fs::read_to_string(snapshot_path()).expect("tests/fixtures/generators_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}

// ─── The density the departure is for ────────────────────────────────────────

const GENRES: [&str; 8] = ["techno", "house", "drumandbass", "hiphop", "rock", "funk", "disco", "reggae"];

fn scales() -> Scales {
    Scales::from_json(fixtures().get("scales").unwrap())
}

fn generated(genre: &str, scales: &Scales, stream: &mut Stream) -> Pattern {
    let mut pattern = Pattern { beat_count: 4, ..Pattern::default() };
    let options = Options { genre: Some(genre.to_owned()), ..Options::default() };
    let mut rnd = || stream.next();
    generate_pattern(&mut AutoGenerator::new(scales, &mut rnd), &mut pattern, &options);
    pattern
}

fn drums(pattern: &Pattern) -> impl Iterator<Item = (usize, &Track)> {
    pattern.tracks.iter().enumerate().filter(|(_, t)| instrument_family(&t.name).is_drum())
}

#[test]
fn a_new_pattern_stays_within_its_drum_track_limit_with_kick_and_snare() {
    let scales = scales();
    let mut stream = Stream::new(11.0);
    for genre in GENRES {
        for _ in 0..100 {
            let pattern = generated(genre, &scales, &mut stream);
            let limit = if matches!(genre, "hiphop" | "reggae" | "rock" | "funk") { 5 } else { 6 };
            assert!(drums(&pattern).count() <= limit, "{genre}");
            for name in ["KICK", "SNARE"] {
                assert!(pattern.tracks.iter().any(|t| t.name == name), "{genre}: no {name}");
            }
        }
    }
}

#[test]
fn drums_on_a_full_pattern_leaves_the_tracks_past_the_limit_empty_and_out_of_auto() {
    let scales = scales();
    let names = ["CHH", "OHH", "CLAP", "PERC", "COWBELL", "CRASH", "KICK", "SNARE"];
    for seed in 0..50 {
        let mut tracks: Vec<Track> = names.iter().map(|n| Track::new(n, 4, 4)).collect();
        tracks.iter_mut().for_each(|t| t.notes.push(Note::default()));
        let mut pattern = Pattern { beat_count: 4, tracks, ..Pattern::default() };
        let mut stream = Stream::new(f64::from(seed));
        let mut rnd = || stream.next();
        assert!(toggle_drums(&mut AutoGenerator::new(&scales, &mut rnd), &mut pattern));
        let auto: Vec<&Track> = drums(&pattern).map(|(_, t)| t).filter(|t| t.auto_mode).collect();
        assert!(auto.len() <= 6);
        assert!(auto.iter().any(|t| t.name == "KICK") && auto.iter().any(|t| t.name == "SNARE"));
        assert!(drums(&pattern).all(|(_, t)| t.auto_mode || t.notes.is_empty()));
    }
}

#[test]
fn four_on_the_floor_is_about_one_kick_a_beat() {
    let scales = scales();
    let mut stream = Stream::new(5.0);
    let mut notes = 0;
    for _ in 0..200 {
        let mut pattern = Pattern { tracks: vec![Track::new("KICK", 4, 4)], beat_count: 4, ..Pattern::default() };
        let mut rnd = || stream.next();
        let mut g = AutoGenerator::new(&scales, &mut rnd);
        instrument_line(&mut g, &mut pattern, 0, InstrumentFamily::Kick, Some("fourOnFloor"), 1.0, &Harmony::default());
        drop(g);
        notes += pattern.tracks[0].notes.len();
    }
    // One beat loops: the first step, and rarely another.
    assert!((200..=240).contains(&notes), "{notes} notes over 200 lines");
}

#[test]
fn locked_hats_follow_the_density() {
    let scales = scales();
    let mut stream = Stream::new(3.0);
    let mut count = |density: f64| {
        let mut notes = 0;
        for _ in 0..100 {
            let mut pattern = Pattern { tracks: vec![Track::new("CHH", 4, 4)], beat_count: 4, ..Pattern::default() };
            let mut rnd = || stream.next();
            let mut g = AutoGenerator::new(&scales, &mut rnd);
            let variant = Some("chh16thLocked");
            instrument_line(&mut g, &mut pattern, 0, InstrumentFamily::Hat, variant, density, &Harmony::default());
            drop(g);
            notes += pattern.tracks[0].notes.len();
        }
        notes
    };
    assert_eq!(count(1.0), 400);
    let thin = count(0.4);
    assert!((240..=320).contains(&thin), "{thin} notes over 100 lines");
}

// ─── Standard rhythms ────────────────────────────────────────────────────────

/// The line of `kind`, variant `variant`, as steps of 16 in the measure.
fn line_steps(stream: &mut Stream, name: &str, kind: InstrumentFamily, variant: &str, density: f64) -> Vec<i32> {
    let scales = scales();
    let mut pattern = Pattern { tracks: vec![Track::new(name, 4, 4)], beat_count: 4, ..Pattern::default() };
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &mut rnd);
    instrument_line(&mut g, &mut pattern, 0, kind, Some(variant), density, &Harmony::default());
    drop(g);
    pattern.tracks[0].notes.iter().map(|n| n.beat * 4 + n.step).collect()
}

#[test]
fn the_downbeat_and_backbeat_hold_at_any_density() {
    let mut stream = Stream::new(11.0);
    for density in [0.2, 0.4, 0.7, 1.0] {
        for _ in 0..50 {
            for variant in ["basic", "syncopated", "fourOnFloor"] {
                let kick = line_steps(&mut stream, "KICK", InstrumentFamily::Kick, variant, density);
                assert!(kick.contains(&0), "{variant} at {density}: {kick:?}");
            }
            for variant in ["basic", "syncopated", "ghost"] {
                let snare = line_steps(&mut stream, "SNARE", InstrumentFamily::Snare, variant, density);
                assert!(snare.contains(&4) && snare.contains(&12), "{variant} at {density}: {snare:?}");
            }
            let hats = line_steps(&mut stream, "CHH", InstrumentFamily::Hat, "chhBasic", density);
            assert!(hats.contains(&0) && hats.contains(&2), "{hats:?}");
        }
    }
}

#[test]
fn a_drum_groove_holds_through_its_section() {
    let scales = scales();
    let mut stream = Stream::new(13.0);
    let mut track = Track::new("KICK", 4, 4);
    track.auto_mode = true;
    let mut pattern = Pattern { tracks: vec![track], beat_count: 4, ..Pattern::default() };
    pattern.extra.insert(ordrumbox_logic::generate::GENRE, Value::from("funk"));
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &mut rnd);
    // The song form starts with 4 loops of intro: new, held, held, fill.
    assert!(run_loop(&mut g, &mut pattern, 0));
    let groove = pattern.tracks[0].notes.clone();
    assert!(!run_loop(&mut g, &mut pattern, 1));
    assert!(!run_loop(&mut g, &mut pattern, 2));
    assert_eq!(pattern.tracks[0].notes, groove);
    assert!(run_loop(&mut g, &mut pattern, 3));
}

#[test]
fn bass_and_chords_hold_through_their_section_with_no_variation() {
    let scales = scales();
    let mut stream = Stream::new(17.0);
    let mut tracks = vec![Track::new("BASS", 4, 4), Track::new("PIANO", 4, 4)];
    tracks.iter_mut().for_each(|t| t.auto_mode = true);
    let mut pattern = Pattern { tracks, beat_count: 4, ..Pattern::default() };
    pattern.extra.insert(ordrumbox_logic::generate::GENRE, Value::from("house"));
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &mut rnd);
    // 4 loops of intro, then the chorus: only its first loop is new.
    assert!(run_loop(&mut g, &mut pattern, 0));
    let lines = pattern.tracks.clone();
    assert!(lines.iter().all(|t| !t.notes.is_empty()));
    for index in 1..4 {
        assert!(!run_loop(&mut g, &mut pattern, index), "loop {index}");
    }
    assert_eq!(pattern.tracks[0].notes, lines[0].notes);
    assert_eq!(pattern.tracks[1].notes, lines[1].notes);
    assert!(run_loop(&mut g, &mut pattern, 4));
    assert!(!run_loop(&mut g, &mut pattern, 5));
    for track in &pattern.tracks {
        assert_eq!((track.auto_hit_variation, track.auto_note_variation), (Some(0.0), Some(0.0)));
    }
}
