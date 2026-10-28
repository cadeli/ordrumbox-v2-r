//! The chord progressions against the JS `getSequence()` and
//! `buildSequenceNotes()` (`composition.json`, from
//! tools/export_piano_fixtures.mjs): every progression, indices past both
//! ends, several tonics and pattern lengths.

use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_logic::composition::{sequence, sequence_notes};

#[test]
fn sequences_match_build_sequence_notes() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/composition.json");
    let fixture = json::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    let num = |o: &Value, key: &str| o.get(key).and_then(Value::as_f64).unwrap();
    for case in fixture.get("sequences").and_then(Value::as_array).unwrap() {
        let s = sequence(num(case, "index") as i64);
        assert_eq!(s.id, case.get("id").and_then(Value::as_str).unwrap());
        assert_eq!(s.name, case.get("name").and_then(Value::as_str).unwrap());
        let got = sequence_notes(s, num(case, "tonic"), num(case, "beatCount") as i32);
        let expected: Vec<(i32, f64)> = case
            .get("notes")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|n| {
                assert_eq!(num(n, "beatStep"), 0.0);
                (num(n, "beat") as i32, num(n, "pitch"))
            })
            .collect();
        assert_eq!(got, expected, "{}", case.to_json());
    }
}
