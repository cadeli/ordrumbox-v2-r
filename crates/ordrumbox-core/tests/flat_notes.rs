//! The pattern engine against the JS app: `flat_notes/<id>.json` holds what
//! `recomputeFlatNotes()` gives for passes 0–3 of every pattern of song.json
//! and engine_cases.json, loaded the way the app boots (tools/export_fixtures.mjs).
//!
//! A pass marked deterministic gave the same notes for three different random
//! streams: Rust must give exactly those notes, in the same order. The other
//! passes depend on chance; they are checked by properties.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Hits, Library, Pattern, Rng, Scales, arp, expand_hits};

/// Ticks a beat in the JS app, which wrote the fixtures; a fixture tick is
/// scaled by `TICK / JS_TICK` to compare.
const JS_TICK: u32 = 32;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn read_json(path: &Path) -> Value {
    let text = fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// Patterns of song.json and engine_cases.json, loaded like the app does.
fn patterns() -> Vec<Pattern> {
    ["song.json", "engine_cases.json"]
        .iter()
        .flat_map(|name| {
            let (library, report) = Library::from_json(&read_json(&fixtures().join(name)));
            assert!(report.issues.is_empty(), "{name}: {:#?}", report.issues);
            library.patterns
        })
        .collect()
}

/// `(pattern id, pass, deterministic, ticks)` of every fixture pass.
fn fixture_passes() -> Vec<(String, u32, bool, Value)> {
    let mut entries: Vec<_> = fs::read_dir(fixtures().join("flat_notes")).expect("flat_notes").collect();
    entries.sort_by_key(|e| e.as_ref().map(|e| e.path()).ok());
    let mut passes = Vec::new();
    for entry in entries {
        let fixture = read_json(&entry.expect("entry").path());
        let id = fixture.get("id").and_then(Value::as_str).expect("id").to_owned();
        for pass in fixture.get("passes").and_then(Value::as_array).expect("passes") {
            let number = pass.get("loop").and_then(Value::as_f64).expect("loop") as u32;
            let deterministic = pass.get("deterministic").and_then(Value::as_bool).expect("deterministic");
            passes.push((id.clone(), number, deterministic, pass.get("ticks").cloned().expect("ticks")));
        }
    }
    passes
}

/// Passes where the engine leaves the JS on purpose (`README.md`, "Golden
/// fixtures"): pattern, passes, the ticks the fixture holds, and the ticks
/// Rust gives instead. Both patterns play a 3-a-beat grid, which the JS
/// rounded to 32 ticks a beat; at 96 a beat its steps and their repeats fall
/// on whole ticks.
type Departure = (&'static str, &'static [u32], &'static [u32], &'static [u32]);

const DEPARTURES: &[Departure] = &[
    (
        "case-grid3",
        &[0, 1, 2, 3],
        &[11, 32, 53, 54, 55, 56, 64, 71, 78, 85, 92],
        &[32, 96, 160, 164, 168, 172, 192, 212, 232, 252, 272, 276],
    ),
    (
        "shuffle",
        &[0],
        &[0, 11, 16, 27, 32, 43, 48, 53, 59, 64, 75, 80, 91, 96, 101, 107, 112, 117, 123],
        &[0, 32, 48, 80, 96, 128, 144, 160, 176, 192, 224, 240, 272, 288, 304, 320, 336, 352, 368],
    ),
    (
        "shuffle",
        &[1, 2, 3],
        &[0, 11, 16, 27, 32, 43, 48, 59, 64, 75, 80, 91, 96, 107, 112, 123],
        &[0, 32, 48, 80, 96, 128, 144, 176, 192, 224, 240, 272, 288, 320, 336, 368],
    ),
];

/// The fixture's ticks, as the JS wrote them (`JS_TICK` a beat).
fn tick_list(js: &Value) -> Vec<u32> {
    js.as_array().expect("ticks").iter().map(|t| t.as_array().unwrap()[0].as_f64().unwrap() as u32).collect()
}

/// The fixture's ticks at `TICK` a beat.
fn scaled_tick_list(js: &Value) -> Vec<u32> {
    tick_list(js).into_iter().map(|t| t * (TICK / JS_TICK)).collect()
}

fn compute(pattern: &Pattern, pass: u32, seed: u64) -> Hits {
    expand_hits(pattern, pass, &mut Rng::new(seed), &Scales::default())
}

/// First difference between the Rust notes and the JS ones, if any.
fn diff(pattern: &Pattern, rust: &Hits, js: &Value) -> Option<String> {
    let rust_ticks: Vec<u32> = rust.keys().copied().collect();
    let js_ticks = scaled_tick_list(js);
    let js = js.as_array().expect("ticks");
    if rust_ticks != js_ticks {
        return Some(format!("ticks {rust_ticks:?}\n   JS {js_ticks:?}"));
    }
    for (entry, (tick, notes)) in js.iter().zip(rust) {
        let expected = entry.as_array().unwrap()[1].as_array().unwrap();
        if expected.len() != notes.len() {
            return Some(format!("tick {tick}: {} notes, JS {}", notes.len(), expected.len()));
        }
        for (i, (note, want)) in notes.iter().zip(expected).enumerate() {
            let field = |key: &str| want.get(key).cloned().unwrap_or(Value::Null);
            let at = format!("tick {tick} note {i}");
            if Value::from(note.track as u32) != field("track") {
                return Some(format!("{at}: track {}, JS {}", note.track, field("track")));
            }
            if Value::Number(note.pitch) != field("pitch") || Value::Number(note.velocity) != field("velocity") {
                return Some(format!(
                    "{at}: pitch/velocity {}/{}, JS {}/{}",
                    note.pitch,
                    note.velocity,
                    field("pitch"),
                    field("velocity")
                ));
            }
            // JS hands out the written note object itself, except for arpeggio
            // notes, which are copies (index null in the fixture).
            let source = note.source.expect("no variation in a deterministic pass");
            let has_arp = arp::sequence(&pattern.tracks[note.track].notes[source].arp, &mut Rng::new(0)).is_some();
            let expected_source = field("note").as_f64().map(|n| n as usize);
            if (has_arp && expected_source.is_some()) || (!has_arp && expected_source != Some(source)) {
                return Some(format!("{at}: source {source} (arp {has_arp}), JS {}", field("note")));
            }
        }
    }
    None
}

#[test]
fn deterministic_passes_match_the_js_engine() {
    let patterns = patterns();
    let mut checked = 0;
    let mut failures = Vec::new();
    for (id, pass, deterministic, ticks) in fixture_passes() {
        if !deterministic {
            continue;
        }
        let pattern = patterns.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("no pattern {id}"));
        let mut rng = Rng::new(7);
        let before = rng.clone().next_u64();
        let rust = expand_hits(pattern, pass, &mut rng, &Scales::default());
        if let Some(&(_, _, js, own)) = DEPARTURES.iter().find(|(d, passes, ..)| *d == id && passes.contains(&pass)) {
            assert_eq!(tick_list(&ticks), js, "departure {id}: the fixture changed");
            assert_eq!(rust.keys().copied().collect::<Vec<_>>(), own, "departure {id} pass {pass}");
        } else if let Some(difference) = diff(pattern, &rust, &ticks) {
            failures.push(format!("{id} pass {pass}: {difference}"));
        }
        if rng.next_u64() != before {
            failures.push(format!("{id} pass {pass}: drew randomness for a pass that has none"));
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{} of {checked} passes differ:\n{}", failures.len(), failures.join("\n"));
    assert_eq!(checked, 184);
}

#[test]
fn random_passes_are_reproducible_and_well_formed() {
    let patterns = patterns();
    let random: Vec<_> = fixture_passes().into_iter().filter(|(_, _, deterministic, _)| !deterministic).collect();
    assert_eq!(random.len(), 8);
    for (id, pass, _, _) in random {
        let pattern = patterns.iter().find(|p| p.id == id).expect("pattern");
        let runs: Vec<Hits> = (0..20).map(|seed| compute(pattern, pass, seed)).collect();
        assert_eq!(compute(pattern, pass, 3), runs[3], "{id}: same seed, same pass");
        assert!(runs.iter().any(|run| *run != runs[0]), "{id} pass {pass}: no seed changes anything");
        for run in &runs {
            for note in run.values().flatten() {
                let track = &pattern.tracks[note.track];
                assert!(note.source.is_none_or(|s| s < track.notes.len()), "{id}: source out of range");
                assert!((0.0..=1.0).contains(&note.velocity), "{id}: velocity {}", note.velocity);
            }
        }
    }
}

#[test]
fn variation_respects_its_budget() {
    // A full `variation` amount buys 16 units: at most 16 notes change.
    let patterns = patterns();
    let pattern = patterns.iter().find(|p| p.id == "case-variation").expect("pattern");
    let mut only_chh = pattern.clone();
    only_chh.tracks.truncate(1);
    only_chh.tracks[0].beat_count = 8; // one loop over the 8-beat pattern: one budget
    let mut plain = only_chh.clone();
    plain.tracks[0].hit_variation = 0.0;
    let reference = compute(&plain, 0, 0);
    let reference_count: usize = reference.values().map(Vec::len).sum();
    for seed in 0..50 {
        let varied = compute(&only_chh, 0, seed);
        let count: usize = varied.values().map(Vec::len).sum();
        let added = varied.values().flatten().filter(|n| n.source.is_none()).count();
        // Each add or silence costs 3 units, so at most 5 of them.
        assert!(added <= 5, "seed {seed}: {added} notes added");
        assert!(count + 5 >= reference_count && count <= reference_count + 5, "seed {seed}: {count} notes");
    }
}
