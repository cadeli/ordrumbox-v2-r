//! The presets shipped with the app, rendered by the real voice and measured.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use ordrumbox_core::json;
use ordrumbox_engine::SynthPresets;
use ordrumbox_probe::measure::measure;
use ordrumbox_probe::render::{NoteSettings, render_note};
use ordrumbox_probe::report::{report_to_json, sound_to_json};

fn presets() -> SynthPresets {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/data/synth_presets.json");
    SynthPresets::from_json(&json::parse(&std::fs::read_to_string(path).unwrap()).unwrap())
}

#[test]
fn every_preset_sounds_and_ends() {
    let presets = presets();
    assert!(!presets.is_empty());
    for key in presets.keys() {
        let buffer = render_note(presets.get(key).unwrap(), &NoteSettings::default()).unwrap();
        let m = measure(&buffer);
        assert!(m.peak > 0.0 && m.peak.is_finite(), "{key}: {}", m.peak);
        assert!(m.attack_ms.is_some() && m.end_ms.is_some() && m.centroid_hz.is_some(), "{key}");
        let report = report_to_json(json::Object::new(), vec![sound_to_json(key, &m, None)]);
        json::parse(&report.to_json()).unwrap();
    }
}

#[test]
fn the_gate_sets_the_length_and_the_note_the_pitch() {
    let preset =
        json::parse(r#"{"vco1":{"gain":1,"wave":"sine"},"bypassFilter":true,"envelope":{"sustain":1,"release":0.01}}"#)
            .unwrap();
    let measured = |settings: NoteSettings| measure(&render_note(&preset, &settings).unwrap());

    // One step at 120 BPM and 4 steps per beat, as the app plays it.
    let step = measured(NoteSettings::default());
    assert!((step.end_ms.unwrap() - 125.0).abs() < 15.0, "{:?}", step.end_ms);
    let long = measured(NoteSettings { gate_seconds: Some(1.0), ..NoteSettings::default() });
    assert!((long.end_ms.unwrap() - 1000.0).abs() < 15.0, "{:?}", long.end_ms);

    let c3 = long.fundamental_hz.unwrap();
    assert!((c3 - 130.81).abs() < 1.0, "{c3}");
    let c4 = measured(NoteSettings { ratio: 2.0, gate_seconds: Some(1.0), ..NoteSettings::default() });
    assert!((c4.fundamental_hz.unwrap() - 261.63).abs() < 2.0);
}
