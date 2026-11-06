//! The generators on the inputs of `tests/fixtures/generator_cases.json`,
//! drawing from a seeded stream: every melodic variant on several track
//! setups, densities and harmonies, random tracks, whole patterns, auto mode
//! loops and the ↻ buttons, checked against `generators_rust.txt`; the drum
//! lines come from the styles (`tests/compose.rs`). Properties follow.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Note, Pattern, Scales, Styles, Track};
use ordrumbox_logic::generate::{
    AutoGenerator, Harmony, MelodicKind, Options, generate_pattern, instrument_line, random_track, recompose,
    restyle_pattern, run_loop, toggle_auto, toggle_drums, toggle_melodic,
};

/// mulberry32: a seeded stream for the generators.
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

fn cases() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/generator_cases.json"))
        .expect("tests/fixtures/generator_cases.json");
    json::parse(&text).expect("generator_cases.json parses")
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or_else(|| panic!("no {key}"))
}

fn f(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("no number {key} in {value}"))
}

fn data(file: &str) -> Value {
    let path = format!("{}/../../assets/data/{file}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
    json::parse(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn scales() -> Scales {
    Scales::from_json(&data("scales.json"))
}

fn styles() -> Styles {
    let (styles, issues) = Styles::from_json(&data("styles.json"));
    assert_eq!(issues, [], "styles.json");
    styles
}

/// A pattern of the cases, loaded as a library loads it, with no id.
fn pattern_in(value: &Value) -> Pattern {
    let mut library = Object::new();
    library.insert("patterns", Value::Array(vec![value.clone()]));
    let (library, report) = Library::from_json(&Value::Object(library));
    assert_eq!(report.issues, [], "{value}");
    let mut pattern = library.patterns.into_iter().next().expect("one pattern");
    pattern.id.clear();
    pattern
}

/// A track as a pattern saves it.
fn track_json(track: &Track) -> Value {
    let pattern = Pattern { tracks: vec![track.clone()], ..Pattern::default() };
    pattern.to_json().get("tracks").and_then(Value::as_array).unwrap()[0].clone()
}

fn harmony_in(value: Option<&Value>) -> Harmony {
    match value {
        Some(h) if !h.is_null() => {
            Harmony { root: f(h, "root"), scale: h.get("scale").and_then(Value::as_str).map(str::to_owned) }
        }
        _ => Harmony::default(),
    }
}

// ─── Snapshot ────────────────────────────────────────────────────────────────

/// The pattern of a melodic track case, its track's index and its generator.
fn track_case(case: &Value) -> (Pattern, usize, InstrumentFamily) {
    let kind = match case.get("kind").and_then(Value::as_str).unwrap() {
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

/// One line of the snapshot: what a case made, and its draws.
fn snapshot_line(at: &str, value: &Value, draws: u64) -> String {
    format!("{at} draws {draws} {}", value.to_json())
}

/// What the generators make of every case.
fn snapshot() -> Vec<String> {
    let cases = cases();
    let (scales, styles) = (scales(), styles());
    let mut lines = Vec::new();
    for (n, case) in items(&cases, "tracks").iter().enumerate() {
        let (mut pattern, t, kind) = track_case(case);
        let variant = case.get("variant").and_then(Value::as_str);
        let mut stream = Stream::new(f(case, "seed"));
        let mut rnd = || stream.next();
        let mut g = AutoGenerator::new(&scales, &styles, &mut rnd);
        instrument_line(&mut g, &mut pattern, t, kind, variant, f(case, "density"), &harmony_in(case.get("harmony")));
        drop(g);
        lines.push(snapshot_line(&format!("track {n}"), &track_json(&pattern.tracks[t]), stream.draws));
    }
    for (n, case) in items(&cases, "random").iter().enumerate() {
        let mut track = Track::new("T1", f(case, "beatCount") as i32, f(case, "stepsPerBeat") as i32);
        let mut stream = Stream::new(f(case, "seed"));
        random_track(&mut track, &mut || stream.next());
        lines.push(snapshot_line(&format!("random {n}"), &track_json(&track), stream.draws));
    }
    for (n, case) in items(&cases, "patterns").iter().enumerate() {
        let mut pattern = pattern_in(case.get("pattern").unwrap());
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
        generate_pattern(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, &options);
        lines.push(snapshot_line(&format!("pattern {n}"), &pattern.to_json(), stream.draws));
    }
    for (n, case) in items(&cases, "loops").iter().enumerate() {
        let mut pattern = pattern_in(case.get("pattern").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for index in 0..f(case, "loopCount") as usize {
            let start = stream.draws;
            let mut rnd = || stream.next();
            run_loop(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, index as u64);
            let at = format!("loops {n} loop {index}");
            lines.push(snapshot_line(&at, &pattern.to_json(), stream.draws - start));
        }
    }
    for (n, case) in items(&cases, "toggles").iter().enumerate() {
        let mut pattern = pattern_in(case.get("pattern").unwrap());
        let mut stream = Stream::new(f(case, "seed"));
        for (i, step) in items(case, "steps").iter().enumerate() {
            let step = step.as_str().unwrap();
            let start = stream.draws;
            let mut rnd = || stream.next();
            let mut g = AutoGenerator::new(&scales, &styles, &mut rnd);
            match step {
                "drums" => toggle_drums(&mut g, &mut pattern, &Options::default()),
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
/// changes on purpose (`README.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    std::fs::write(snapshot_path(), snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn the_generators_match_their_snapshot() {
    let text = std::fs::read_to_string(snapshot_path()).expect("tests/fixtures/generators_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}

// ─── Patterns in a style ─────────────────────────────────────────────────────

fn generated(genre: &str, scales: &Scales, styles: &Styles, stream: &mut Stream) -> Pattern {
    let mut pattern = Pattern { beat_count: 4, ..Pattern::default() };
    let options = Options { genre: Some(genre.to_owned()), ..Options::default() };
    let mut rnd = || stream.next();
    generate_pattern(&mut AutoGenerator::new(scales, styles, &mut rnd), &mut pattern, &options);
    pattern
}

#[test]
fn a_new_pattern_has_its_style_tracks_meter_and_tempo() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(11.0);
    for style in &styles.list {
        for _ in 0..30 {
            let pattern = generated(&style.name, &scales, &styles, &mut stream);
            assert_eq!(pattern.beat_count, style.beat_count, "{}", style.name);
            assert!((style.tempo.0..=style.tempo.1).contains(&pattern.bpm), "{}: {}", style.name, pattern.bpm);
            for track in &pattern.tracks {
                assert!(style.tracks.contains(&track.name), "{}: {}", style.name, track.name);
                if instrument_family(&track.name).is_drum() {
                    assert_eq!(track.steps_per_beat, style.steps_per_beat, "{}: {}", style.name, track.name);
                    assert_eq!(track.swing_amount, style.swing_amount);
                }
            }
            // The parts every pattern of the style has.
            for (name, part) in &style.parts {
                if part.chance >= 1.0 && part.hits_per_measure > 0.0 {
                    let track = pattern.tracks.iter().find(|t| t.name == *name).unwrap();
                    assert!(!track.notes.is_empty(), "{}: {name}", style.name);
                }
            }
        }
    }
}

#[test]
fn drums_on_a_full_pattern_play_the_style_parts_and_leave_the_others_empty_out_of_auto() {
    let (scales, styles) = (scales(), styles());
    let names = ["CHH", "OHH", "CLAP", "PERC", "COWBELL", "CRASH", "KICK", "SNARE"];
    for seed in 0..20 {
        let mut tracks: Vec<Track> = names.iter().map(|n| Track::new(n, 4, 4)).collect();
        tracks.iter_mut().for_each(|t| t.notes.push(Note::default()));
        let mut pattern = Pattern { beat_count: 4, tracks, ..Pattern::default() };
        let mut stream = Stream::new(f64::from(seed));
        let mut rnd = || stream.next();
        let options = Options { genre: Some("reggae".into()), ..Options::default() };
        assert!(toggle_drums(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, &options));
        let reggae = styles.get("reggae").unwrap();
        for track in pattern.tracks.iter().filter(|t| instrument_family(&t.name).is_drum()) {
            let plays = reggae.part_for(&track.name).is_some();
            assert_eq!(track.auto_mode, plays, "{}", track.name);
            if !plays {
                assert!(track.notes.is_empty(), "{}", track.name);
            }
        }
        let kick = pattern.tracks.iter().find(|t| t.name == "KICK").unwrap();
        assert_eq!(kick.notes.iter().map(|n| (n.beat, n.step)).collect::<Vec<_>>(), [(2, 0)], "one drop");
    }
}

#[test]
fn a_drum_line_varies_through_its_section_and_holds_with_no_variation() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(13.0);
    let mut track = Track::new("KICK", 4, 4);
    track.auto_mode = true;
    let mut pattern = Pattern { tracks: vec![track], beat_count: 4, ..Pattern::default() };
    pattern.extra.insert(ordrumbox_logic::generate::GENRE, Value::from("funk"));
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &styles, &mut rnd);
    assert!(run_loop(&mut g, &mut pattern, 0));
    let groove = pattern.tracks[0].notes.clone();
    assert!(run_loop(&mut g, &mut pattern, 1));
    assert_ne!(pattern.tracks[0].notes, groove);
    assert_eq!(pattern.tracks[0].notes.len(), groove.len());
    // No variation: the line holds until the next section.
    pattern.tracks[0].compose_variation = Some(0.0);
    let held = pattern.tracks[0].notes.clone();
    assert!(!run_loop(&mut g, &mut pattern, 2));
    assert!(!run_loop(&mut g, &mut pattern, 3));
    assert_eq!(pattern.tracks[0].notes, held);
    assert!(run_loop(&mut g, &mut pattern, 4));
}

#[test]
fn bass_and_chords_hold_through_their_section_with_no_variation() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(17.0);
    let mut tracks = vec![Track::new("BASS", 4, 4), Track::new("PIANO", 4, 4)];
    tracks.iter_mut().for_each(|t| t.auto_mode = true);
    let mut pattern = Pattern { tracks, beat_count: 4, ..Pattern::default() };
    pattern.extra.insert(ordrumbox_logic::generate::GENRE, Value::from("house"));
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &styles, &mut rnd);
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

// ─── ⟳, the macros, the lock, AUTO ───────────────────────────────────────────

#[test]
fn a_pattern_made_again_in_a_style_takes_its_tracks_and_keeps_the_locked_one() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(23.0);
    let mut pattern = generated("rock", &scales, &styles, &mut stream);
    let t = pattern.tracks.iter().position(|t| t.name == "SNARE").unwrap();
    pattern.tracks[t].compose_lock = true;
    pattern.tracks[t].volume = 0.3;
    let locked = pattern.tracks[t].clone();
    let kick_volume = 0.6;
    pattern.tracks[0].volume = kick_volume;
    let mut rnd = || stream.next();
    let options = Options { genre: Some("salsa".into()), ..Options::default() };
    restyle_pattern(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, &options);
    let salsa = styles.get("salsa").unwrap();
    assert_eq!(pattern.beat_count, salsa.beat_count);
    assert!(pattern.tracks.iter().any(|t| t.name == "CLAVES" && !t.notes.is_empty()));
    assert!(pattern.tracks.iter().all(|t| t.name != "CHH"), "a track the style does not name goes");
    assert_eq!(pattern.tracks.iter().find(|t| t.name == "SNARE"), Some(&locked));
    // A track the style names keeps its sound settings.
    assert_eq!(pattern.tracks.iter().find(|t| t.name == "KICK").unwrap().volume, kick_volume);
}

#[test]
fn the_macros_move_every_line_and_the_same_values_make_the_same_lines() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(29.0);
    let mut pattern = Pattern { beat_count: 4, ..Pattern::default() };
    let mut rnd = || stream.next();
    let options = Options { genre: Some("funk".into()), ..Options::default() };
    restyle_pattern(&mut AutoGenerator::new(&scales, &styles, &mut rnd), &mut pattern, &options);
    let hits = |p: &Pattern| {
        p.tracks.iter().filter(|t| instrument_family(&t.name).is_drum()).map(|t| t.notes.len()).sum::<usize>()
    };
    let before = pattern.clone();
    let mut set = |pattern: &mut Pattern, energy: f64| {
        pattern.extra.insert(ordrumbox_core::style::ENERGY, Value::Number(energy));
        let mut rnd = || stream.next();
        recompose(&mut AutoGenerator::new(&scales, &styles, &mut rnd), pattern);
    };
    set(&mut pattern, 100.0);
    assert!(hits(&pattern) > hits(&before));
    set(&mut pattern, 0.0);
    assert!(hits(&pattern) < hits(&before));
    set(&mut pattern, 50.0);
    pattern.extra.remove(ordrumbox_core::style::ENERGY);
    let mut expected = before.clone();
    expected.extra.remove(ordrumbox_core::style::ENERGY);
    assert_eq!(pattern.tracks, expected.tracks, "back to neutral: the lines of the seed");
}

#[test]
fn a_locked_track_holds_through_auto_mode_and_auto_leaves_it() {
    let (scales, styles) = (scales(), styles());
    let mut stream = Stream::new(31.0);
    let mut pattern = generated("funk", &scales, &styles, &mut stream);
    pattern.tracks[0].compose_lock = true;
    assert!(toggle_auto(&mut pattern));
    assert!(!pattern.tracks[0].auto_mode && pattern.tracks[1..].iter().all(|t| t.auto_mode));
    let kick = pattern.tracks[0].clone();
    pattern.tracks[0].auto_mode = true;
    let mut rnd = || stream.next();
    let mut g = AutoGenerator::new(&scales, &styles, &mut rnd);
    for index in 0..6 {
        run_loop(&mut g, &mut pattern, index);
    }
    assert_eq!(pattern.tracks[0].notes, kick.notes);
    assert!(!toggle_auto(&mut pattern));
    assert!(pattern.tracks[1..].iter().all(|t| !t.auto_mode));
}
