//! MIDI files against the JS app (`tests/fixtures/midi.json`, written by
//! `tools/export_midi_fixtures.mjs`): the parser's events, the exporter's
//! bytes, the patterns an import creates, and the sort order the exporter
//! relies on.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Rng, Scales, js};
use ordrumbox_logic::midi::{self, smf};

fn fixtures() -> Value {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/midi.json"))
        .expect("tests/fixtures/midi.json (run tools/export_midi_fixtures.mjs)");
    json::parse(&text).expect("fixture parses")
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or_else(|| panic!("no {key}"))
}

fn bytes(value: &Value) -> Vec<u8> {
    value.as_array().expect("bytes").iter().map(|b| b.as_f64().expect("byte") as u8).collect()
}

fn num(n: impl Into<f64>) -> Value {
    Value::Number(n.into())
}

fn roots(fixtures: &Value) -> Vec<(String, f64)> {
    items(fixtures, "sounds")
        .iter()
        .filter_map(|s| Some((s.get("key")?.as_str()?.to_owned(), s.get("rootMidi")?.as_f64()?)))
        .collect()
}

#[test]
fn sort_is_v8s() {
    let fixtures = fixtures();
    for (i, case) in items(&fixtures, "sort").iter().enumerate() {
        let mut list: Vec<(f64, bool, usize)> = items(case, "items")
            .iter()
            .enumerate()
            .map(|(k, item)| {
                let item = item.as_array().unwrap();
                (item[0].as_f64().unwrap(), item[1].as_bool().unwrap(), k)
            })
            .collect();
        js::sort_by(&mut list, |a, b| {
            let d = a.0 - b.0;
            if d != 0.0 {
                d
            } else if a.1 {
                1.0
            } else {
                -1.0
            }
        });
        let order: Vec<f64> = list.iter().map(|e| e.2 as f64).collect();
        let expected: Vec<f64> = items(case, "order").iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(order, expected, "sort case {i} ({} items)", order.len());
    }
}

#[test]
fn sort_follows_every_step_of_timsort() {
    let fixtures = fixtures();
    let chaos = |a: i64, b: i64| (((a * 7919 + b * 104_729 + ((a * b) % 13)) % 5) - 2) as f64;
    for case in items(&fixtures, "chaosSorts") {
        let n = case.get("n").and_then(Value::as_f64).unwrap() as i64;
        let mut list: Vec<i64> = (0..n).collect();
        js::sort_by(&mut list, |a, b| chaos(*a, *b));
        let expected: Vec<i64> = items(case, "order").iter().map(|v| v.as_f64().unwrap() as i64).collect();
        assert_eq!(list, expected, "chaos n = {n}");
    }
    for case in items(&fixtures, "blockSorts") {
        let values: Vec<f64> = items(case, "values").iter().map(|v| v.as_f64().unwrap()).collect();
        let flags: Option<Vec<bool>> =
            case.get("flags").and_then(Value::as_array).map(|f| f.iter().map(|v| v.as_bool().unwrap()).collect());
        let mut list: Vec<usize> = (0..values.len()).collect();
        js::sort_by(&mut list, |a, b| {
            let d = values[*a] - values[*b];
            match &flags {
                Some(flags) if d == 0.0 => {
                    if flags[*a] {
                        -1.0
                    } else {
                        1.0
                    }
                }
                _ => d,
            }
        });
        let expected: Vec<usize> = items(case, "order").iter().map(|v| v.as_f64().unwrap() as usize).collect();
        assert_eq!(list, expected, "blocks of {}", values.len());
    }
}

/// An event as `parseMidi` writes it (undefined fields left out).
fn event_out(event: &smf::Event) -> Value {
    let mut o = Object::new();
    o.insert("absTick", num(event.tick as f64));
    match &event.kind {
        smf::EventKind::Meta { meta_type, data } => {
            o.insert("type", Value::from("meta"));
            if let Some(t) = meta_type {
                o.insert("metaType", num(*t));
            }
            o.insert("data", Value::Array(data.iter().map(|&b| num(b)).collect()));
        }
        smf::EventKind::Program { status, channel, program } => {
            o.insert("type", Value::from("midi"));
            o.insert("status", num(*status));
            o.insert("channel", num(*channel));
            if let Some(p) = program {
                o.insert("program", num(*p));
            }
        }
        smf::EventKind::Note { status, channel, note, velocity } => {
            o.insert("type", Value::from("midi"));
            o.insert("status", num(*status));
            o.insert("channel", num(*channel));
            if let Some(n) = note {
                o.insert("note", num(*n));
            }
            o.insert("velocity", num(*velocity));
        }
    }
    Value::Object(o)
}

#[test]
fn parser() {
    let fixtures = fixtures();
    for file in items(&fixtures, "files") {
        let name = file.get("name").and_then(Value::as_str).unwrap();
        let expected = file.get("parsed").unwrap();
        let midi = smf::parse(&bytes(file.get("bytes").unwrap()));
        let header = expected.get("header").unwrap();
        assert_eq!(
            [midi.header.format, midi.header.track_count, midi.header.division].map(f64::from),
            ["format", "trackCount", "division"].map(|k| header.get(k).unwrap().as_f64().unwrap()),
            "{name}: header"
        );
        let tracks = items(expected, "tracks");
        assert_eq!(midi.tracks.len(), tracks.len(), "{name}: tracks");
        for (t, (events, expected)) in midi.tracks.iter().zip(tracks).enumerate() {
            let expected = expected.as_array().unwrap();
            assert_eq!(events.len(), expected.len(), "{name}: track {t} events");
            for (k, (event, expected)) in events.iter().zip(expected).enumerate() {
                assert_eq!(event_out(event), *expected, "{name}: track {t} event {k}");
            }
        }
        let names: Vec<Value> = midi.track_names.iter().map(|n| Value::from(n.as_str())).collect();
        assert_eq!(names, items(expected, "trackNames"), "{name}: names");
        let notes: Vec<Value> = midi
            .note_ons()
            .iter()
            .map(|n| {
                let mut o = Object::new();
                o.insert("absTick", num(n.tick as f64));
                o.insert("channel", num(n.channel));
                if let Some(note) = n.note {
                    o.insert("note", num(note));
                }
                o.insert("velocity", num(n.velocity));
                o.insert("trackIdx", num(n.track as f64));
                Value::Object(o)
            })
            .collect();
        assert_eq!(notes, items(expected, "notes"), "{name}: notes");
        let programs: Vec<Value> = midi
            .program_changes()
            .iter()
            .map(|(c, p)| Value::Array(vec![num(*c), p.map_or(Value::Null, num)]))
            .collect();
        assert_eq!(programs, items(expected, "programs"), "{name}: programs");
    }
}

#[test]
fn exporter() {
    let fixtures = fixtures();
    let roots = roots(&fixtures);
    let cases = items(&fixtures, "exports");
    assert!(cases.len() > 80);
    for case in cases {
        let mut wrapper = Object::new();
        wrapper.insert("patterns", Value::Array(vec![case.get("source").unwrap().clone()]));
        let (library, _) = Library::from_json(&Value::Object(wrapper));
        let pattern = &library.patterns[0];
        let loops = case.get("loops").and_then(Value::as_f64).unwrap() as u32;
        let rust = midi::export(pattern, loops, &roots, &mut Rng::new(1), &Scales::default());
        let js = bytes(case.get("bytes").unwrap());
        if rust != js {
            let at = rust.iter().zip(&js).position(|(a, b)| a != b).unwrap_or(rust.len().min(js.len()));
            let window = |b: &[u8]| format!("{:?}", &b[at.saturating_sub(8)..(at + 8).min(b.len())]);
            panic!(
                "{} × {loops}: bytes differ at {at} ({} vs {} bytes)\n  rust: {}\n  js:   {}",
                pattern.id,
                rust.len(),
                js.len(),
                window(&rust),
                window(&js)
            );
        }
    }
}

#[test]
fn importer() {
    let fixtures = fixtures();
    let roots = roots(&fixtures);
    let files = items(&fixtures, "files");
    for case in items(&fixtures, "imports") {
        let name = case.get("name").and_then(Value::as_str).unwrap();
        let file = files.iter().find(|f| f.get("name").and_then(Value::as_str) == Some(name)).unwrap();
        let result = case.get("result").unwrap();
        match midi::import(name, &bytes(file.get("bytes").unwrap()), &roots) {
            Err(warning) => {
                assert_eq!(Some(warning.as_str()), result.get("warning").and_then(Value::as_str), "{name}");
                assert!(items(case, "patterns").is_empty());
            }
            Ok(imported) => {
                assert_eq!(Some(imported.message.as_str()), result.get("message").and_then(Value::as_str), "{name}");
                let patterns = items(case, "patterns");
                assert_eq!(imported.patterns.len(), patterns.len(), "{name}: patterns");
                for (pattern, expected) in imported.patterns.iter().zip(patterns) {
                    let at = format!("{name} {}", pattern.name.as_deref().unwrap_or_default());
                    assert_eq!(pattern.name.as_deref(), expected.get("name").and_then(Value::as_str), "{at}");
                    assert_eq!(num(pattern.bpm), *expected.get("bpm").unwrap(), "{at}: bpm");
                    assert_eq!(num(pattern.beat_count), *expected.get("beatCount").unwrap(), "{at}: beats");
                    let tracks = items(expected, "tracks");
                    assert_eq!(pattern.tracks.len(), tracks.len(), "{at}: tracks");
                    for (track, expected) in pattern.tracks.iter().zip(tracks) {
                        let at = format!("{at} {}", track.name);
                        assert_eq!(Some(track.name.as_str()), expected.get("name").and_then(Value::as_str), "{at}");
                        for (key, value) in [
                            ("beatCount", num(track.beat_count)),
                            ("stepsPerBeat", num(track.steps_per_beat)),
                            ("loopAtStep", track.loop_length.map_or(Value::Null, num)),
                        ] {
                            assert_eq!(value, *expected.get(key).unwrap(), "{at}: {key}");
                        }
                        let notes: Vec<Value> = track
                            .notes
                            .iter()
                            .map(|n| {
                                let pitch = if n.pitch.is_nan() { Value::Null } else { num(n.pitch) };
                                let percent = n.step_percent.map_or(Value::Null, num);
                                Value::Array(vec![num(n.velocity), num(n.beat), num(n.step), pitch, percent])
                            })
                            .collect();
                        assert_eq!(notes, items(expected, "notes"), "{at}: notes");
                    }
                }
            }
        }
    }
}
