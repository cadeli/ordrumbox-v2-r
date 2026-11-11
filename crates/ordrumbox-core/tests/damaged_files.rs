//! Library files that are cut, mistyped or hold values of another type or out
//! of range (`edge_library.json` and `engine_cases.json`, damaged in every
//! place): a file is refused or loaded with its issues, never a panic; what
//! was loaded plays, is saved, and its save loads again, however large the
//! number it holds (a track of a billion beats).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::fs;
use std::panic::catch_unwind;
use std::path::Path;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Rng, Scales, expand_hits};

/// The characters put in place of each character of a file.
const HOSTILE_CHARS: [char; 8] = ['"', '{', ']', ',', ':', '-', '9', 'e'];
/// Passes played of each pattern of a damaged library.
const PASS_COUNT: u32 = 2;

fn fixture(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
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
        Value::Array(vec![Value::Null]),
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
            Value::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(i, v)| if i == index { replaced(v, rest, with) } else { v.clone() })
                    .collect(),
            )
        }
        _ => unreachable!("a path goes through objects and arrays"),
    }
}

/// Everything the app does with a library it loaded: play it, save it, load its save.
fn use_library(library: &Library) {
    let scales = Scales::default();
    for pattern in &library.patterns {
        for pass in 0..PASS_COUNT {
            for hit in expand_hits(pattern, pass, &mut Rng::new(1), &scales).values().flatten() {
                assert!(hit.track < pattern.tracks.len(), "a note of no track");
                assert!(hit.velocity.is_finite() && hit.pitch.is_finite() && hit.pan.is_finite(), "{hit:?}");
            }
        }
    }
    let saved = library.to_file_text();
    let (reloaded, _) = Library::parse(&saved).expect("a save parses");
    assert_eq!(reloaded.to_file_text(), saved, "a second save changes the file");
}

#[test]
fn a_value_of_another_type_or_out_of_range_is_repaired_or_reported_never_a_panic() {
    let hostile = hostile_values();
    let mut cases = 0;
    let mut failures = Vec::new();
    for name in ["edge_library.json", "engine_cases.json"] {
        let value = json::parse(&fixture(name)).expect("parses");
        let mut found = Vec::new();
        paths(&value, &mut Vec::new(), &mut found);
        for path in &found {
            for with in &hostile {
                let damaged = replaced(&value, path, with);
                if catch_unwind(|| use_library(&Library::from_json(&damaged).0)).is_err() {
                    failures.push(format!("{name}: {} replaced by {with}", path.join(".")));
                }
                cases += 1;
            }
        }
    }
    assert!(cases > 5000, "{cases} cases");
    assert!(failures.is_empty(), "{} panics:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn a_cut_or_mistyped_file_is_refused_or_loaded_never_a_panic() {
    let text = fixture("edge_library.json");
    let cuts = text.char_indices().map(|(at, _)| text[..at].to_owned());
    let typos = text.char_indices().flat_map(|(at, c)| {
        let (before, after) = (&text[..at], &text[at + c.len_utf8()..]);
        HOSTILE_CHARS.iter().map(move |typo| format!("{before}{typo}{after}"))
    });
    let (mut loaded, mut refused) = (0, 0);
    for damaged in cuts.chain(typos) {
        let outcome = catch_unwind(|| Library::parse(&damaged).map(|(library, _)| use_library(&library)).is_ok());
        match outcome {
            Ok(true) => loaded += 1,
            Ok(false) => refused += 1,
            Err(_) => panic!("panics on {damaged}"),
        }
    }
    assert!(loaded > 100 && refused > 100, "{loaded} loaded, {refused} refused");
}
