//! Sample, kit and synth preset files that are cut, mistyped or hold values
//! of another type or out of range: each is refused or read, never a panic,
//! and a preset that was read plays samples that are numbers, however large
//! the number it holds.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::panic::catch_unwind;
use std::path::Path;
use std::sync::Arc;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{SampleBank, SynthPresets, kits_from_json, wav};

/// The bytes put in place of each byte of a WAV file's first chunks.
const HOSTILE_BYTES: [u8; 4] = [0x00, 0x01, 0x7f, 0xff];
/// The start of kick.wav damaged byte by byte: its `RIFF`, `fmt ` and `data` headers.
const HEADER_BYTES: usize = 64;
/// Random damages of the whole file.
const RANDOM_CASES: usize = 2000;

fn fixture(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// The values put in place of each value of a file: other types, and numbers
/// out of the range of most settings.
fn hostile_values() -> Vec<Value> {
    let mut nested = Object::new();
    nested.insert("0", Value::from("a"));
    let numbers = [-1.0, 0.0, 0.5, 300.0, 1e9, -1e9, 1e308, -1e308].map(Value::from);
    let others = [
        Value::Null,
        Value::Bool(true),
        Value::from(""),
        Value::from("NaN"),
        Value::Array(Vec::new()),
        Value::Object(Object::new()),
        Value::Object(nested),
    ];
    numbers.into_iter().chain(others).collect()
}

/// The path to every value of `value`, itself first: keys and indices from the root.
fn paths(value: &Value, at: &mut Vec<String>, found: &mut Vec<Vec<String>>) {
    found.push(at.clone());
    let children: Vec<(String, &Value)> = match value {
        Value::Object(object) => object.iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        Value::Array(items) => items.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        _ => Vec::new(),
    };
    for (key, child) in children {
        at.push(key);
        paths(child, at, found);
        at.pop();
    }
}

/// `value` with the value at `path` replaced.
fn replaced(value: &Value, path: &[String], with: &Value) -> Value {
    let Some((key, rest)) = path.split_first() else { return with.clone() };
    match value {
        Value::Object(object) => {
            let mut copy = Object::new();
            for (k, v) in object.iter() {
                copy.insert(k, if k == key { replaced(v, rest, with) } else { v.clone() });
            }
            Value::Object(copy)
        }
        Value::Array(items) => {
            let index: usize = key.parse().expect("an index");
            let item = |(i, v): (usize, &Value)| if i == index { replaced(v, rest, with) } else { v.clone() };
            Value::Array(items.iter().enumerate().map(item).collect())
        }
        _ => unreachable!("a path goes through objects and arrays"),
    }
}

/// Every damage of `value`: each of its values replaced by each hostile one,
/// with where and by what.
fn damages(value: &Value) -> Vec<(String, Value)> {
    let mut found = Vec::new();
    paths(value, &mut Vec::new(), &mut found);
    let hostile = hostile_values();
    found
        .iter()
        .flat_map(|path| {
            hostile
                .iter()
                .map(move |with| (format!("{} replaced by {with}", path.join(".")), replaced(value, path, with)))
        })
        .collect()
}

#[test]
fn a_damaged_wav_file_is_decoded_or_refused_never_a_panic() {
    let bytes = fixture("kick.wav");
    let mut damaged: Vec<Vec<u8>> = (0..HEADER_BYTES).map(|n| bytes[..n].to_vec()).collect();
    // Cut in its samples, on and off a frame.
    damaged.extend((HEADER_BYTES..bytes.len()).step_by(997).map(|n| bytes[..n].to_vec()));
    for at in 0..HEADER_BYTES {
        for value in HOSTILE_BYTES {
            let mut copy = bytes.clone();
            copy[at] = value;
            damaged.push(copy);
        }
    }
    let mut rng = Rng::new(7);
    for _ in 0..RANDOM_CASES {
        let mut copy = bytes.clone();
        for _ in 0..4 {
            let at = (rng.next_u64() % HEADER_BYTES as u64) as usize;
            copy[at] = rng.next_u64() as u8;
        }
        damaged.push(copy);
    }
    let (mut decoded, mut refused) = (0, 0);
    for (case, file) in damaged.iter().enumerate() {
        match catch_unwind(|| wav::decode(file).map(|buffer| buffer.frames())) {
            Ok(Ok(frames)) => {
                assert!(frames <= file.len(), "case {case}: {frames} frames out of {} bytes", file.len());
                decoded += 1;
            }
            Ok(Err(_)) => refused += 1,
            Err(_) => panic!("case {case} panics: {:?}", &file[..file.len().min(HEADER_BYTES)]),
        }
    }
    assert!(decoded > 100 && refused > 100, "{decoded} decoded, {refused} refused");
}

#[test]
fn damaged_kits_are_read_or_refused_never_a_panic() {
    let kits = json::parse(
        r#"[{"name":"a","info":"i","samples":[
               {"displayName":"KICK","key":"KICK","path":"a/kick.wav","peakDb":-3,"rootMidi":31,"decay":5000},
               {"key":"SNARE","path":"a/snare.wav","gain":0.5}]},
            {"name":"web","instruments":[{"key":"CHH","url":"b/chh.wav","display_name":"hat","tune":2}]}]"#,
    )
    .unwrap();
    let silence = wav::encode(&[0.0; 8], &[0.0; 8], 8000);
    for (what, damaged) in damages(&kits) {
        let outcome = catch_unwind(|| {
            if let Ok((kits, _)) = kits_from_json(&damaged) {
                // What was read loads its samples and is saved.
                let (bank, errors) = SampleBank::load(&kits, |_| Ok(silence.clone()));
                assert!(errors.is_empty(), "{errors:?}");
                assert_eq!(bank.samples.len(), kits.iter().map(|kit| kit.samples.len()).sum::<usize>());
                ordrumbox_engine::kits_to_json(&kits).to_json();
            }
        });
        assert!(outcome.is_ok(), "kits: {what} panics");
    }
}

#[test]
fn a_damaged_synth_preset_plays_numbers_or_is_refused_never_a_panic() {
    let presets = json::parse(&String::from_utf8(fixture("generated_sounds.json")).unwrap()).unwrap();
    let (library, _) = Library::parse(
        r#"{"patterns":[{"bpm":120,"beatCount":1,"tracks":[
            {"name":"BASS","useSynth":true,"synthPreset":"P","notes":[{},{"step":2,"pitch":7}]}]}]}"#,
    )
    .unwrap();
    let mut failures = Vec::new();
    // A bass, a drum, a bell.
    for key in ["BASS0", "SN", "BELL1"] {
        let preset = presets.get(key).unwrap_or_else(|| panic!("no preset {key}"));
        for (what, damaged) in damages(preset) {
            let mut file = Object::new();
            file.insert("P", damaged);
            let outcome = catch_unwind(|| {
                let Ok(synths) = SynthPresets::from_json(&Value::Object(file)) else { return true };
                let setup = RenderSetup {
                    bank: Arc::new(SampleBank::default()),
                    scales: Scales::default(),
                    synths,
                    kit: String::new(),
                    sample_rate: 8000,
                    seed: 1,
                };
                let (left, right) = offline::render_pattern(&library, 0, 1, &setup, None).expect("a pattern");
                left.iter().chain(&right).all(|sample| sample.is_finite())
            });
            match outcome {
                Ok(true) => {}
                Ok(false) => failures.push(format!("{key}: {what} plays a sample that is not a number")),
                Err(_) => failures.push(format!("{key}: {what} panics")),
            }
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}
