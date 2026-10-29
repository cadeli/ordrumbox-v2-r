//! Catalog lookups and song clip resolution against the JS app
//! (`instruments.json`, `song_sources.json`, from tools/export_assign_fixtures.mjs).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::collections::HashSet;
use std::path::Path;

use ordrumbox_core::instruments::{NOT_FOUND, catalog};
use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Song, song};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    json::parse(&std::fs::read_to_string(path).expect(name)).expect("parses")
}

fn id(found: Option<&ordrumbox_core::instruments::Instrument>) -> &str {
    found.map_or(NOT_FOUND, |i| i.id.as_str())
}

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("{key}"))
}

fn num(v: &Value, key: &str) -> i64 {
    v.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}")) as i64
}

#[test]
fn lookups_match_the_js_instruments_manager() {
    let fx = fixture("instruments.json");
    let c = catalog();
    let lookups = fx.get("lookups").and_then(Value::as_array).expect("lookups");
    assert!(lookups.len() > 50);
    for l in lookups {
        let name = str_of(l, "name");
        assert_eq!(id(c.by_id(name)), str_of(l, "byId"), "findById({name:?})");
        assert_eq!(c.by_id(name).is_some_and(|i| i.drum), l.get("byIdDrum").and_then(Value::as_bool) == Some(true));
        assert_eq!(id(c.by_name(name)), str_of(l, "byName"), "findByName({name:?})");
        assert_eq!(id(c.from_file_name(name)), str_of(l, "fromFileName"), "findInstrumentFromFileName({name:?})");
    }
    for m in fx.get("midi").and_then(Value::as_array).expect("midi") {
        let (channel, key) = (num(m, "channel"), num(m, "key"));
        assert_eq!(id(c.from_midi(channel, key)), str_of(m, "id"), "MIDI {channel}/{key}");
    }
    for p in fx.get("programs").and_then(Value::as_array).expect("programs") {
        let program = num(p, "program");
        assert_eq!(id(c.from_midi_program(program)), str_of(p, "id"), "program {program}");
    }
}

#[test]
fn song_sources_match_resolve_song_sources() {
    let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/song.json")).unwrap();
    let (library, _) = Library::parse(&text).unwrap();
    let known: HashSet<String> = library.patterns.iter().map(|p| p.id.clone()).collect();
    let cases = fixture("song_sources.json");
    let cases = cases.as_array().expect("cases");
    assert_eq!(cases.len(), 2);
    for case in cases {
        let (songs, _) =
            song::songs_from_json(&Value::Array(vec![case.get("song").unwrap().clone()]), &known, &mut Vec::new());
        let song: &Song = &songs[0];
        for t in case.get("ticks").and_then(Value::as_array).expect("ticks") {
            let tick = num(t, "tick") as u64;
            let got: Vec<(String, usize, u64, u64, u64)> = song
                .active_clips_at(&library.patterns, tick, 32)
                .iter()
                .map(|s| (library.patterns[s.pattern].id.clone(), s.clip, s.loop_index, s.local_tick, s.pattern_ticks))
                .collect();
            let want: Vec<(String, usize, u64, u64, u64)> = t
                .get("sources")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|s| {
                    let n = |k: &str| num(s, k) as u64;
                    (
                        str_of(s, "pattern").to_owned(),
                        n("clipIndex") as usize,
                        n("loop"),
                        n("localStep"),
                        n("patternTicks"),
                    )
                })
                .collect();
            assert_eq!(got, want, "{} at tick {tick}", song.id);
        }
    }
}
