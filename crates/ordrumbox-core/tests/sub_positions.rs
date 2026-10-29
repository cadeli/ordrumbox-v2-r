//! Note sub-positions against the JS `getNoteSubPositions()`
//! (`sub_positions.json`, from tools/export_piano_fixtures.mjs): every track
//! of `song.json` and hand-made notes (retriggers, rates, euclidean fills,
//! loop points, every arpeggio form). A random arpeggio's pitches are drawn,
//! so only its steps are compared.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::note_positions::{SubKind, sub_positions};
use ordrumbox_core::pattern_engine::SpanResolver;
use ordrumbox_core::{Library, Rng, Track};

fn read(path: &Path) -> Value {
    json::parse(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).expect("parses")
}

/// Compare the positions of every note of `track` to `expected` (one array per note).
fn check(track: &Track, expected: &Value, at: &str) {
    let total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let span = SpanResolver::new(track);
    let expected = expected.as_array().expect("positions");
    assert_eq!(track.notes.len(), expected.len(), "{at}: notes");
    for (n, (note, expected)) in track.notes.iter().zip(expected).enumerate() {
        let got = sub_positions(note, track, total, &span, &mut Rng::new(1));
        let expected = expected.as_array().expect("note positions");
        assert_eq!(
            got.len(),
            expected.len(),
            "{at} note {n}: {got:?} vs {}",
            Value::Array(expected.to_vec()).to_json()
        );
        for (g, e) in got.iter().zip(expected) {
            let kind = match g.kind {
                SubKind::Retrigger => "retrigger",
                SubKind::Euclidean => "euclidean",
            };
            assert_eq!(g.step as f64, e.get("pos").and_then(Value::as_f64).unwrap(), "{at} note {n}: pos");
            assert_eq!(kind, e.get("type").and_then(Value::as_str).unwrap(), "{at} note {n}: type");
            if let Some(offset) = e.get("pitchOffset").and_then(Value::as_f64) {
                assert_eq!(g.pitch_offset, offset, "{at} note {n}: pitch offset");
            }
        }
    }
}

#[test]
fn sub_positions_match_get_note_sub_positions() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = read(&root.join("tests/fixtures/sub_positions.json"));
    let (library, _) = Library::from_json(&read(&root.join("tests/fixtures/song.json")));
    let cases = fixture.get("library").and_then(Value::as_array).unwrap();
    assert!(cases.len() > 100);
    for case in cases {
        let p = case.get("p").and_then(Value::as_f64).unwrap() as usize;
        let t = case.get("t").and_then(Value::as_f64).unwrap() as usize;
        check(&library.patterns[p].tracks[t], case.get("positions").unwrap(), &format!("pattern {p} track {t}"));
    }
    for (i, case) in fixture.get("hand").and_then(Value::as_array).unwrap().iter().enumerate() {
        let raw = format!(r#"{{"patterns":[{{"tracks":[{}]}}]}}"#, case.get("track").unwrap().to_json());
        let (library, _) = Library::parse(&raw).unwrap();
        check(&library.patterns[0].tracks[0], case.get("positions").unwrap(), &format!("hand track {i}"));
    }
}
