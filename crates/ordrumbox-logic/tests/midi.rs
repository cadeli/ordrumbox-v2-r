//! MIDI files against `tests/fixtures/midi_rust.txt`: the parser's events of
//! the files in `tests/fixtures/midi/`, the exporter's bytes of the patterns
//! in `tests/fixtures/midi_exports.json`, and the patterns an import creates.
//! `cargo test -p ordrumbox-logic --test midi -- --ignored` writes the
//! snapshot, only when one of them changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::{Path, PathBuf};

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, Pattern, Rng, Scales, expand_hits, lfo};
use ordrumbox_logic::midi::{self, C3_MIDI_NOTE, PPQN, smf};

mod strict_smf;

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// The sample root notes and the patterns to export, with their loop count.
fn exports() -> Value {
    let text = std::fs::read_to_string(fixture_path("midi_exports.json")).expect("tests/fixtures/midi_exports.json");
    json::parse(&text).expect("fixture parses")
}

/// The MIDI files of `tests/fixtures/midi/`, by name.
fn files() -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> = std::fs::read_dir(fixture_path("midi"))
        .expect("tests/fixtures/midi/")
        .map(|entry| {
            let path = entry.expect("entry").path();
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            (name, std::fs::read(&path).expect("read"))
        })
        .filter(|(name, _)| name.ends_with(".mid"))
        .collect();
    files.sort();
    files
}

fn file(name: &str) -> Vec<u8> {
    std::fs::read(fixture_path("midi").join(name)).expect("a fixture file")
}

fn items<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value.get(key).and_then(Value::as_array).unwrap_or_else(|| panic!("no {key}"))
}

fn num(n: impl Into<f64>) -> Value {
    Value::Number(n.into())
}

fn roots(exports: &Value) -> Vec<(String, f64)> {
    items(exports, "sounds")
        .iter()
        .filter_map(|s| Some((s.get("key")?.as_str()?.to_owned(), s.get("rootMidi")?.as_f64()?)))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// An event, its unset fields left out.
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

/// What the parser reads of a file: header, events, names, note-ons, programs.
fn parsed(bytes: &[u8]) -> Value {
    let midi = smf::parse(bytes);
    let mut header = Object::new();
    header.insert("format", num(midi.header.format));
    header.insert("trackCount", num(midi.header.track_count));
    header.insert("division", num(midi.header.division));
    let tracks = midi.tracks.iter().map(|events| Value::Array(events.iter().map(event_out).collect())).collect();
    let names = midi.track_names.iter().map(|n| Value::from(n.as_str())).collect();
    let notes = midi
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
    let programs =
        midi.program_changes().iter().map(|(c, p)| Value::Array(vec![num(*c), p.map_or(Value::Null, num)])).collect();
    let mut o = Object::new();
    o.insert("header", Value::Object(header));
    o.insert("tracks", Value::Array(tracks));
    o.insert("trackNames", Value::Array(names));
    o.insert("notes", Value::Array(notes));
    o.insert("programs", Value::Array(programs));
    Value::Object(o)
}

/// What an import makes of a file: its patterns and message, or its warning.
fn imported(name: &str, bytes: &[u8], roots: &[(String, f64)]) -> Value {
    let mut o = Object::new();
    match midi::import(name, bytes, roots) {
        Err(warning) => {
            o.insert("warning", Value::from(warning.as_str()));
        }
        Ok(imported) => {
            o.insert("message", Value::from(imported.message.as_str()));
            o.insert("patterns", Value::Array(imported.patterns.iter().map(|p| p.to_json()).collect()));
        }
    }
    Value::Object(o)
}

/// The parser, the importer and the exporter as this code runs them, one line each.
fn rust_snapshot() -> Vec<String> {
    let exports = exports();
    let roots = roots(&exports);
    let files = files();
    let mut lines = Vec::new();
    for (name, bytes) in &files {
        lines.push(format!("parse {name} {}", parsed(bytes).to_json()));
    }
    for (name, bytes) in &files {
        lines.push(format!("import {name} {}", imported(name, bytes, &roots).to_json()));
    }
    for case in items(&exports, "exports") {
        let mut wrapper = Object::new();
        wrapper.insert("patterns", Value::Array(vec![case.get("source").unwrap().clone()]));
        let (library, _) = Library::from_json(&Value::Object(wrapper));
        let pattern = &library.patterns[0];
        let loops = case.get("loops").and_then(Value::as_f64).unwrap() as u32;
        let bytes = midi::export(pattern, library.bpm, loops, &roots, &mut Rng::new(1), &Scales::default());
        lines.push(format!("export {} x{loops} {}", pattern.id, hex(&bytes)));
    }
    lines
}

/// Writes `midi_rust.txt` from the code as it is: `cargo test -p
/// ordrumbox-logic --test midi -- --ignored`, when the parser, the import or
/// the export changes on purpose (`README.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    std::fs::write(fixture_path("midi_rust.txt"), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn parser_import_and_export_match_the_snapshot() {
    let text = std::fs::read_to_string(fixture_path("midi_rust.txt")).expect("tests/fixtures/midi_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert!(actual.len() > 120, "{} lines", actual.len());
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}

#[test]
fn the_notes_of_one_tick_are_written_offs_first_then_by_key() {
    let exports = exports();
    let roots = roots(&exports);
    for case in items(&exports, "exports") {
        let mut wrapper = Object::new();
        wrapper.insert("patterns", Value::Array(vec![case.get("source").unwrap().clone()]));
        let (library, _) = Library::from_json(&Value::Object(wrapper));
        let pattern = &library.patterns[0];
        let bytes = midi::export(pattern, library.bpm, 3, &roots, &mut Rng::new(1), &Scales::default());
        for events in smf::parse(&bytes).tracks {
            // (tick, on, key) of each note event, in file order.
            let notes: Vec<(u64, bool, u8)> = events
                .iter()
                .filter_map(|e| match e.kind {
                    smf::EventKind::Note { status, note: Some(note), .. } => {
                        Some((e.tick as u64, status & 0xf0 == 0x90, note))
                    }
                    _ => None,
                })
                .collect();
            assert!(notes.is_sorted(), "{}: {notes:?}", pattern.id);
        }
    }
}

/// The patterns of `midi_exports.json`, with their loop count.
/// Each export case: its pattern, its tempo (the source's, which the library
/// takes) and its loops.
fn export_cases(exports: &Value) -> Vec<(Pattern, f64, u32)> {
    items(exports, "exports")
        .iter()
        .map(|case| {
            let mut wrapper = Object::new();
            wrapper.insert("patterns", Value::Array(vec![case.get("source").unwrap().clone()]));
            let (mut library, _) = Library::from_json(&Value::Object(wrapper));
            let loops = case.get("loops").and_then(Value::as_f64).unwrap() as u32;
            (library.patterns.remove(0), library.bpm, loops)
        })
        .collect()
}

/// One event of a track chunk: its absolute tick and its bytes, status first
/// (a meta event: 0xff, its type, then its data, without the length).
type RawEvent = (u64, Vec<u8>);

/// The track chunks of an export, once `strict_smf` accepts the file, and
/// what the exporter writes past the spec: format 1 at 96 PPQN, a first track
/// of meta events only (the tempo map) signed by orDrumbox, then instrument
/// tracks of a name at tick 0 and channel messages, no key struck while it
/// sounds on its channel.
fn strict_tracks(bytes: &[u8]) -> Result<Vec<Vec<RawEvent>>, String> {
    let file = strict_smf::validate(bytes)?;
    if (file.format, file.division) != (1, PPQN as u16) {
        return Err(format!("format {}, division {}", file.format, file.division));
    }
    let signature = strict_smf::Kind::Meta { meta_type: 0x01, data: midi::SIGNATURE.as_bytes().to_vec() };
    if !file.tracks[0].iter().any(|e| e.tick == 0 && e.kind == signature) {
        return Err("no orDrumbox signature at tick 0 of the tempo track".into());
    }
    let mut tracks = Vec::new();
    for (index, events) in file.tracks.into_iter().enumerate() {
        let raw: Vec<RawEvent> = events
            .into_iter()
            .map(|e| match e.kind {
                strict_smf::Kind::Channel(message) if index > 0 => Ok((e.tick, message)),
                strict_smf::Kind::Meta { meta_type, data } => Ok((e.tick, [vec![0xff, meta_type], data].concat())),
                other => Err(format!("track {index}: {other:x?}")),
            })
            .collect::<Result<_, _>>()?;
        if index > 0 && !matches!(raw.first(), Some((0, e)) if e[..2] == [0xff, 0x03]) {
            return Err(format!("track {index} does not start with its name"));
        }
        let mut sounding = [[false; 128]; 16];
        for (tick, e) in raw.iter().filter(|(_, e)| e[0] & 0xe0 == 0x80) {
            let key = &mut sounding[usize::from(e[0] & 0x0f)][usize::from(e[1])];
            if e[0] & 0xf0 == 0x90 && e[2] > 0 && *key {
                return Err(format!("track {index}: key {} struck again while it sounds, at tick {tick}", e[1]));
            }
            *key = e[0] & 0xf0 == 0x90 && e[2] > 0;
        }
        tracks.push(raw);
    }
    Ok(tracks)
}

/// Where the notes of track name `name` go, by the rules of `README.md`
/// ("Files"): a drum on channel 10 at its GM key; a melodic instrument on
/// the next channel at the sample's root note, else its mapping's key, else
/// C3; an unknown name a bass drum. Channel (0-based), base note.
fn target(name: &str, roots: &[(String, f64)], next_melodic: &mut u8) -> (u8, f64) {
    let upper = name.trim().to_uppercase();
    match catalog().by_name(&upper).filter(|i| !i.midi.is_empty()) {
        Some(instrument) if instrument.drum => (9, instrument.midi[0].key.map_or(C3_MIDI_NOTE, |k| k as f64)),
        Some(instrument) => {
            let channel = *next_melodic;
            *next_melodic += 1;
            let root = roots.iter().find(|(key, _)| upper.contains(&key.to_uppercase())).map(|r| r.1);
            (channel, root.or(instrument.midi[0].key.map(|k| k as f64)).unwrap_or(C3_MIDI_NOTE))
        }
        None => (9, 36.0),
    }
}

/// A note as a file holds it: tick, key, velocity.
type FileNote = (u64, u8, u8);

/// The notes each instrument track of the file must hold, straight from the
/// pattern engine: by track name, in the order the names first come, the
/// flat notes of every playing track of that name, pass after pass, on that
/// name's channel; the notes of one key at one tick as one, at the loudest
/// velocity.
fn expected_tracks(pattern: &Pattern, loops: u32, roots: &[(String, f64)]) -> Vec<(String, u8, Vec<FileNote>)> {
    let any_solo = pattern.tracks.iter().any(|t| t.solo);
    let plays: Vec<bool> = pattern.tracks.iter().map(|t| if any_solo { t.solo } else { !t.mute }).collect();
    let mut next_melodic = 0;
    let mut by_name: Vec<(String, u8, f64, Vec<FileNote>)> = Vec::new();
    for track in pattern.tracks.iter().zip(&plays).filter(|(_, plays)| **plays).map(|(t, _)| t) {
        let (channel, base) = target(&track.name, roots, &mut next_melodic);
        match by_name.iter_mut().find(|(n, ..)| *n == track.name) {
            // A repeated melodic name takes a channel each time and keeps the last.
            Some(entry) => (entry.1, entry.2) = (channel, base),
            None => by_name.push((track.name.clone(), channel, base, Vec::new())),
        }
    }
    let mut rng = Rng::new(1);
    let midi_ratio = u64::from(PPQN) / u64::from(TICK);
    for pass in 0..loops {
        let offset = u64::from(pass) * u64::from(TICK) * pattern.beat_count as u64 * midi_ratio;
        for (&tick, hits) in &expand_hits(pattern, pass, &mut rng, &Scales::default()) {
            for hit in hits.iter().filter(|h| plays[h.track]) {
                let track = &pattern.tracks[hit.track];
                let entry = by_name.iter_mut().find(|(n, ..)| *n == track.name).unwrap();
                let mut velocity = hit.velocity;
                if let Some(l) = &track.volume_lfo {
                    velocity = (velocity * lfo::value(l, f64::from(tick))).clamp(0.0, 1.0);
                }
                let mut pitch = hit.pitch + track.transpose;
                if let Some(l) = &track.transpose_lfo {
                    pitch += lfo::value(l, f64::from(tick));
                }
                let key = (entry.2 + pitch).clamp(0.0, 127.0) as u8;
                // A silent note is not written: a note-on at velocity 0 would be a note-off.
                let velocity = (velocity * 127.0).round() as u8;
                if velocity > 0 {
                    entry.3.push((u64::from(tick) * midi_ratio + offset, key, velocity));
                }
            }
        }
    }
    by_name
        .into_iter()
        .filter(|e| !e.3.is_empty())
        .map(|(name, channel, _, notes)| {
            let mut loudest: std::collections::BTreeMap<(u64, u8), u8> = std::collections::BTreeMap::new();
            for (tick, key, velocity) in notes {
                let v = loudest.entry((tick, key)).or_default();
                *v = (*v).max(velocity);
            }
            (name, channel, loudest.into_iter().map(|((tick, key), velocity)| (tick, key, velocity)).collect())
        })
        .collect()
}

#[test]
fn every_export_is_a_strict_file_of_the_notes_the_engine_plays() {
    let exports = exports();
    let roots = roots(&exports);
    let cases = export_cases(&exports);
    assert!(cases.len() > 80);
    let mut failures = Vec::new();
    for (pattern, bpm, loops) in &cases {
        let at = format!("{} × {loops}", pattern.id);
        let bytes = midi::export(pattern, *bpm, *loops, &roots, &mut Rng::new(1), &Scales::default());
        let tracks = match strict_tracks(&bytes) {
            Ok(tracks) => tracks,
            Err(e) => {
                failures.push(format!("{at}: {e}"));
                continue;
            }
        };
        let expected = expected_tracks(pattern, *loops, &roots);
        let names: Vec<String> = tracks[1..]
            .iter()
            .map(|events| match events.first() {
                Some((0, e)) if e[..2] == [0xff, 0x03] => String::from_utf8(e[2..].to_vec()).unwrap_or_default(),
                _ => String::new(),
            })
            .collect();
        let want: Vec<&String> = expected.iter().map(|e| &e.0).collect();
        if names.iter().collect::<Vec<_>>() != want {
            failures.push(format!("{at}: tracks {names:?}, want {want:?}"));
            continue;
        }
        for (events, (name, channel, want)) in tracks[1..].iter().zip(&expected) {
            let mut ons: Vec<FileNote> = Vec::new();
            let mut offs: Vec<(u64, u8)> = Vec::new();
            for (tick, e) in events.iter().filter(|(_, e)| e[0] != 0xff) {
                if e[0] & 0x0f != *channel {
                    failures.push(format!("{at} {name}: {e:x?} on channel {}, want {channel}", e[0] & 0x0f));
                }
                match e[0] & 0xf0 {
                    0x90 if e[2] > 0 => ons.push((*tick, e[1], e[2])),
                    0x90 | 0x80 => offs.push((*tick, e[1])),
                    _ => {}
                }
            }
            let mut want = want.clone();
            want.sort_unstable();
            ons.sort_unstable();
            if ons != want {
                let first = ons.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(ons.len().min(want.len()));
                failures.push(format!(
                    "{at} {name}: {} notes, want {}; first difference at {first}: {:?} vs {:?}",
                    ons.len(),
                    want.len(),
                    ons.get(first),
                    want.get(first)
                ));
            }
            // A note lasts a 16th, or ends where its key is struck again.
            let mut ends: Vec<(u64, u8)> = want
                .iter()
                .map(|&(tick, key, _)| {
                    let next = want.iter().filter(|n| n.1 == key && n.0 > tick).map(|n| n.0).min();
                    (next.map_or(tick + 24, |next| next.min(tick + 24)), key)
                })
                .collect();
            ends.sort_unstable();
            offs.sort_unstable();
            if offs != ends {
                failures.push(format!("{at} {name}: {} note-offs, want {}", offs.len(), ends.len()));
            }
        }
    }
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn track_names_are_written_in_utf8_and_read_in_utf8_or_latin1() {
    let (library, _) =
        Library::parse(r#"{"patterns":[{"tracks":[{"name":"Caisse claire é ☃","notes":[{}]}]}]}"#).unwrap();
    let bytes = midi::export(&library.patterns[0], library.bpm, 1, &[], &mut Rng::new(1), &Scales::default());
    assert_eq!(smf::parse(&bytes).track_names[1], "Caisse claire é ☃");
    // An older file's Latin-1 name: 0xe9 alone is not UTF-8.
    let mut latin1 = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk\0\0\0\x0b\0\xff\x03\x03\x63\x61\xe9\0\xff\x2f\0".to_vec();
    assert_eq!(smf::parse(&latin1).track_names, ["caé"]);
    latin1[28] = b'f';
    assert_eq!(smf::parse(&latin1).track_names, ["caf"]);
}

/// The import message says so when notes past beat 512 are left out.
const CUT_NOTE: &str = ", notes past beat 512 left out";

/// A type 0 file of `division` holding a kick on beat 0 and one at `last_tick`.
fn two_kicks(division: u16, last_tick: u32) -> Vec<u8> {
    let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01".to_vec();
    bytes.extend(division.to_be_bytes());
    let mut events = vec![0x00, 0x99, 36, 100];
    events.extend(smf::encode_vlq(last_tick));
    events.extend([0x99, 36, 100, 0x00, 0xff, 0x2f, 0x00]);
    bytes.extend(b"MTrk");
    bytes.extend(u32::try_from(events.len()).unwrap().to_be_bytes());
    bytes.extend(events);
    bytes
}

#[test]
fn a_file_not_timed_in_ticks_per_beat_is_refused() {
    for division in [0u16, 0xe728, 0xe250] {
        let refused = midi::import("smpte.mid", &two_kicks(division, 96), &[]).unwrap_err();
        assert!(refused.starts_with("MIDI timing not supported"), "{division:#x}: {refused}");
    }
    assert!(midi::import("ppqn.mid", &two_kicks(96, 96), &[]).is_ok());
}

#[test]
fn notes_past_the_last_pattern_are_announced() {
    const PPQN: u32 = 96;
    let last_beat = |imported: &midi::Imported| {
        let kicks = imported.patterns.iter().enumerate().flat_map(|(p, pattern)| {
            pattern.tracks[0].notes.iter().map(move |n| p as i32 * pattern.beat_count + n.beat)
        });
        kicks.max().unwrap()
    };
    let fits = midi::import("fits.mid", &two_kicks(PPQN as u16, 511 * PPQN), &[]).unwrap();
    assert_eq!(fits.patterns.len(), 16);
    assert_eq!(last_beat(&fits), 511);
    assert!(!fits.message.ends_with(CUT_NOTE), "{}", fits.message);

    let cut = midi::import("cut.mid", &two_kicks(PPQN as u16, 512 * PPQN), &[]).unwrap();
    assert_eq!(cut.patterns.len(), 16);
    assert_eq!(last_beat(&cut), 0, "the kick on beat 512 has no pattern");
    assert_eq!(cut.message, format!("MIDI imported: 1 track(s) into 16 patterns{CUT_NOTE}"));
}

#[test]
fn a_damaged_file_is_imported_or_refused_never_a_panic() {
    // A drum channel, melodic programs and running status: most of the parser's paths.
    let bytes = file("band-format1.mid");
    let mut damaged: Vec<Vec<u8>> = (0..bytes.len()).map(|n| bytes[..n].to_vec()).collect();
    for at in 0..bytes.len() {
        for value in [0x00, 0x7f, 0x80, 0xff] {
            let mut copy = bytes.clone();
            copy[at] = value;
            damaged.push(copy);
        }
    }
    let mut rng = Rng::new(7);
    for _ in 0..2000 {
        let mut copy = bytes.clone();
        for _ in 0..4 {
            let at = (rng.next_u64() % copy.len() as u64) as usize;
            copy[at] = rng.next_u64() as u8;
        }
        damaged.push(copy);
    }
    for (case, file) in damaged.iter().enumerate() {
        let outcome = std::panic::catch_unwind(|| midi::import("damaged.mid", file, &[]));
        assert!(outcome.is_ok(), "case {case} panics: {file:?}");
    }
}
