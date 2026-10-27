//! The kit editor's sample analysis against the JS Analyzer
//! (`tests/fixtures/analysis.json`, written by `tools/export_kit_fixtures.mjs`).

use ordrumbox_core::json::{self, Value};
use ordrumbox_engine::analysis::analyze;
use ordrumbox_engine::wav;

#[test]
fn analysis_matches_the_js() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/analysis.json"))
        .expect("tests/fixtures/analysis.json (run tools/export_kit_fixtures.mjs)");
    let fixtures = json::parse(&text).unwrap();
    let cases = fixtures.get("cases").and_then(Value::as_array).unwrap();
    assert!(cases.len() >= 12);
    for case in cases {
        let name = case.get("name").and_then(Value::as_str).unwrap();
        let bytes: Vec<u8> =
            case.get("bytes").and_then(Value::as_array).unwrap().iter().map(|b| b.as_f64().unwrap() as u8).collect();
        let a = analyze(&wav::decode(&bytes).unwrap());
        let num = |key: &str| case.get(key).and_then(Value::as_f64);
        let envelope: Vec<f64> =
            case.get("envelope").and_then(Value::as_array).unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        assert_eq!(a.envelope, envelope, "{name}: envelope");
        assert_eq!(Some(a.duration_sec), num("durationSec"), "{name}: duration");
        assert_eq!(Some(a.peak_linear), num("peakLinear"), "{name}: peak");
        assert_eq!(Some(a.peak_db).filter(|v| v.is_finite()), num("peakDb"), "{name}: peak dB");
        assert_eq!(Some(a.rms_db).filter(|v| v.is_finite()), num("rmsDb"), "{name}: RMS dB");
        assert_eq!(a.fundamental_hz, num("fundamentalHz"), "{name}: fundamental");
        assert_eq!(Some(a.pitch_confidence), num("pitchConfidence"), "{name}: confidence");
        let note = a.note.map(|n| n.to_string());
        assert_eq!(note.as_deref(), case.get("note").and_then(Value::as_str), "{name}: note");
    }
}
