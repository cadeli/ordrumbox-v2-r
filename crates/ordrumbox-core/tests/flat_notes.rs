//! The pattern engine against its Rust snapshot, `flat_notes_rust.txt`:
//! passes 0–3 of every pattern of song.json and engine_cases.json
//! (hand-written: paths the demo library does not reach), one line a pass,
//! with seed 7. `write_the_snapshot` rewrites it when the engine changes on
//! purpose.
//!
//! A pass is `fixed` when it draws no randomness: every seed gives its
//! notes. The others (`drawn`) depend on chance; besides their seed-7 line,
//! they are checked by properties.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Hits, Library, Pattern, Rng, Scales, expand_hits};

/// Passes of each pattern in the snapshot.
const PASS_COUNT: u32 = 4;
/// The seed of the snapshot.
const SEED: u64 = 7;

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

fn compute(pattern: &Pattern, pass: u32, seed: u64) -> Hits {
    expand_hits(pattern, pass, &mut Rng::new(seed), &Scales::default())
}

/// Whether a pass draws from the random stream.
fn draws(pattern: &Pattern, pass: u32) -> bool {
    let mut rng = Rng::new(SEED);
    let before = rng.clone().next_u64();
    expand_hits(pattern, pass, &mut rng, &Scales::default());
    rng.next_u64() != before
}

/// One pass: `<id> pass <n> fixed|drawn:` then each tick and its notes,
/// `track/source beat.step pitch velocity pan` (`-` for a note the
/// variation layer added).
fn pass_line(pattern: &Pattern, pass: u32) -> String {
    let kind = if draws(pattern, pass) { "drawn" } else { "fixed" };
    let mut line = format!("{} pass {pass} {kind}:", pattern.id);
    for (tick, hits) in compute(pattern, pass, SEED) {
        write!(line, " {tick} [").unwrap();
        for (i, hit) in hits.iter().enumerate() {
            let source = hit.source.map_or("-".to_owned(), |s| s.to_string());
            let separator = if i == 0 { "" } else { ", " };
            write!(
                line,
                "{separator}{}/{source} {}.{} {} {} {}",
                hit.track, hit.beat, hit.step, hit.pitch, hit.velocity, hit.pan
            )
            .unwrap();
        }
        line.push(']');
    }
    line
}

fn rust_snapshot() -> Vec<String> {
    patterns().iter().flat_map(|p| (0..PASS_COUNT).map(move |pass| pass_line(p, pass))).collect()
}

fn snapshot_path() -> PathBuf {
    fixtures().join("flat_notes_rust.txt")
}

/// Writes `flat_notes_rust.txt` from the engine as it is: `cargo test -p
/// ordrumbox-core --test flat_notes -- --ignored`, when what a pattern plays
/// changes on purpose (`README.md`, "Golden fixtures").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn every_pass_matches_its_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/flat_notes_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
    assert_eq!(actual.iter().filter(|line| line.contains(" fixed:")).count(), 184);
}

#[test]
fn a_fixed_pass_is_the_same_for_every_seed() {
    for pattern in &patterns() {
        for pass in (0..PASS_COUNT).filter(|&pass| !draws(pattern, pass)) {
            let reference = compute(pattern, pass, SEED);
            for seed in [0, 1, u64::MAX] {
                assert_eq!(compute(pattern, pass, seed), reference, "{} pass {pass} seed {seed}", pattern.id);
            }
        }
    }
}

#[test]
fn random_passes_are_reproducible_and_well_formed() {
    let patterns = patterns();
    let random: Vec<(&Pattern, u32)> = patterns
        .iter()
        .flat_map(|p| (0..PASS_COUNT).map(move |pass| (p, pass)))
        .filter(|&(p, pass)| draws(p, pass))
        .collect();
    assert_eq!(random.len(), 8);
    for (pattern, pass) in random {
        let id = &pattern.id;
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
