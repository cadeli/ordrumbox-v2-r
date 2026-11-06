//! What the synth editor makes of a preset, against its Rust snapshot
//! `synth_editor_rust.txt`: every preset of the web version
//! (`generated_sounds.json`) and hand-made ones, filled with the group
//! defaults, with their cards in order and those bypassed; the LFO targets and
//! their scales; and an LFO's modulation for every target, wave and sync at
//! several times. `write_the_snapshot` rewrites it when one of these changes
//! on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_engine::synth_model::{WAVES, bypassed, groups, has_power, hydrate, lfo_mod, lfo_scale, lfo_targets};

/// Presets the web version does not have: empty, partial and broken, bypassed.
const HAND_PRESETS: [(&str, &str); 3] = [
    ("empty", "{}"),
    ("partial", r#"{"vco2":{"gain":0.4},"filter":"broken","bypassLfo1":true,"bypassFm":1,"extra":{"a":1},"zeta":3}"#),
    ("bypassed", r#"{"vco1":{"gain":0},"bypassNoise":true,"bypassFilter":true,"bypassEnv":false,"bypassModEnv":true}"#),
];

/// The times (seconds) an LFO's modulation is read at, at `BPM`.
const TIMES: [f64; 5] = [0.0, 0.1, 0.37, 1.5, 7.77];
const BPM: f64 = 120.0;
/// Frequency, depth and sync of each LFO case: free, tempo-synced, and two
/// that do nothing.
const LFO_SETTINGS: [(f64, f64, &str); 4] =
    [(1.3, 0.5, "off"), (0.25, 1.0, "1/8T"), (0.0, 0.5, "off"), (2.0, 0.0, "off")];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

fn object(text: &str) -> Object {
    json::parse(text).expect("parses").as_object().cloned().expect("an object")
}

/// A modulation as written: 9 decimals, so that the last bit of a sine,
/// which the platform's libm computes, does not show; no negative zero.
fn modulation(value: f64) -> f64 {
    (value * 1e9).round() / 1e9 + 0.0
}

fn rust_snapshot() -> Vec<String> {
    let text = fs::read_to_string(fixtures().join("generated_sounds.json")).expect("generated_sounds.json");
    let mut presets: Vec<(String, Object)> = object(&text)
        .iter()
        .map(|(key, preset)| (key.to_owned(), preset.as_object().cloned().expect("an object")))
        .collect();
    presets.extend(HAND_PRESETS.iter().map(|(key, text)| ((*key).to_owned(), object(text))));
    let mut lines = Vec::new();
    for (key, mut draft) in presets {
        hydrate(&mut draft);
        lines.push(format!("preset {key} draft: {}", Value::Object(draft.clone()).to_json()));
        let mut line = format!("preset {key} cards:");
        for group in groups(&draft) {
            let off = if has_power(&group) && bypassed(&draft, &group) { " (off)" } else { "" };
            write!(line, " {group}{off}").unwrap();
        }
        lines.push(line);
    }
    let targets = lfo_targets();
    let mut line = "lfo targets:".to_owned();
    for target in &targets {
        write!(line, " {target}={}", lfo_scale(target)).unwrap();
    }
    lines.push(line);
    for target in targets.iter().copied().chain(["unknown"]) {
        for wave in WAVES.iter().copied().chain(["bogus"]) {
            for (freq, depth, sync) in LFO_SETTINGS {
                let lfo =
                    format!(r#"{{"target":"{target}","wave":"{wave}","freq":{freq},"depth":{depth},"sync":"{sync}"}}"#);
                let mut line = format!("lfo {lfo}:");
                for time in TIMES {
                    write!(line, " {}", modulation(lfo_mod(&object(&lfo), time, BPM))).unwrap();
                }
                lines.push(line);
            }
        }
    }
    lines
}

fn snapshot_path() -> PathBuf {
    fixtures().join("synth_editor_rust.txt")
}

/// Writes `synth_editor_rust.txt` from the code as it is: `cargo test -p
/// ordrumbox-engine --test synth_editor -- --ignored`, when the editor's
/// model changes on purpose (`README.md`, "Golden fixtures").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn synth_editor_matches_its_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/synth_editor_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}
