//! Track and preset settings against what the JS app sends to its audio
//! processors (fixtures from `tools/export_dsp_fixtures.mjs`).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::Path;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, lfo};
use ordrumbox_dsp::{StripSettings, SynthPatch};
use ordrumbox_engine::strip_settings::strip_settings;
use ordrumbox_engine::synth_preset::{SynthNote, synth_patch};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    json::parse(&std::fs::read_to_string(path).expect(name)).expect("parses")
}

fn num(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("{key}"))
}

fn strip_fields(s: &StripSettings) -> [(&'static str, f32); 18] {
    [
        ("cutoff", s.cutoff),
        ("q", s.q),
        ("filterMode", s.filter_mode),
        ("satType", s.saturation_type),
        ("satDrive", s.saturation_drive),
        ("satOut", s.saturation_output),
        ("satMix", s.saturation_mix),
        ("revRoom", s.reverb_room),
        ("revDamp", s.reverb_damping),
        ("revWidth", s.reverb_width),
        ("revMix", s.reverb_mix),
        ("dlyTimeL", s.delay_time_l),
        ("dlyTimeR", s.delay_time_r),
        ("dlyFb", s.delay_feedback),
        ("dlyMix", s.delay_mix),
        ("dlyMode", s.delay_mode),
        ("volume", s.volume),
        ("pan", s.pan),
    ]
}

#[test]
fn strip_settings_match_applytracktostrip() {
    let cases = fixture("strip_settings.json");
    let cases = cases.as_array().expect("array");
    assert!(cases.len() > 100);
    for case in cases {
        let track_json = case.get("track").expect("track");
        let library = format!(r#"{{"patterns":[{{"name":"p","tracks":[{}]}}]}}"#, track_json.to_json());
        let (library, _) = Library::parse(&library).expect("library");
        let track = &library.patterns[0].tracks[0];
        let settings = strip_settings(track, num(case, "bpm"));
        let expected = case.get("settings").expect("settings");
        for (key, got) in strip_fields(&settings) {
            let want = num(expected, key) as f32;
            assert_eq!(got, want, "{key} of {}", track_json.to_json());
        }
    }
}

#[test]
fn lfo_values_match_computelfovalue() {
    let fx = fixture("lfo_values.json");
    let ticks: Vec<f64> =
        fx.get("ticks").and_then(Value::as_array).expect("ticks").iter().filter_map(Value::as_f64).collect();
    let lfos = fx.get("lfos").and_then(Value::as_array).expect("lfos");
    assert!(lfos.len() > 30);
    for case in lfos {
        let object = case.get("lfo").and_then(Value::as_object).expect("lfo");
        let values = case.get("values").and_then(Value::as_array).expect("values");
        for (tick, want) in ticks.iter().zip(values) {
            let want = want.as_f64().expect("number");
            assert_eq!(lfo::value(object, *tick), want, "{} at tick {tick}", case.get("lfo").unwrap().to_json());
        }
    }
}

/// A patch as the `update` message's fields (booleans as 0/1).
fn patch_fields(p: &SynthPatch) -> Vec<(String, f64)> {
    let mut fields = Vec::new();
    let mut add = |key: &str, v: f64| fields.push((key.to_owned(), v));
    for i in 0..3 {
        add(&format!("osc{}Freq", i + 1), p.osc_freq[i]);
        add(&format!("osc{}Gain", i + 1), p.osc_gain[i]);
        add(&format!("osc{}Detune", i + 1), p.osc_detune[i]);
        add(&format!("osc{}Wave", i + 1), p.osc_wave[i]);
    }
    add("noiseMix", p.noise_mix);
    add("noiseFilterType", p.noise_filter_type);
    add("noiseFilterFreq", p.noise_filter_freq);
    add("noiseFilterQ", p.noise_filter_q);
    add("filterType", p.filter_type);
    add("filterFreq", p.filter_freq);
    add("filterQ", p.filter_q);
    add("drive", p.drive);
    add("pitchPunch", p.pitch_punch);
    add("subGain", p.sub_gain);
    add("attack", p.envelope.attack);
    add("decay", p.envelope.decay);
    add("sustain", p.envelope.sustain);
    add("release", p.envelope.release);
    add("master", p.volume);
    add("pan", p.pan);
    add("velocity", p.velocity);
    for (i, l) in [(1, p.lfo1), (2, p.lfo2)] {
        add(&format!("lfo{i}Target"), l.target);
        add(&format!("lfo{i}Wave"), l.wave);
        add(&format!("lfo{i}Freq"), l.freq);
        add(&format!("lfo{i}Depth"), l.depth);
    }
    add("filterEnvAmt", p.filter_env_amount);
    add("fmAmount", p.fm_amount);
    add("fmAlgo", p.fm_algorithm);
    add("modEnvAttack", p.mod_envelope.attack);
    add("modEnvDecay", p.mod_envelope.decay);
    add("modEnvSustain", p.mod_envelope.sustain);
    add("modEnvRelease", p.mod_envelope.release);
    add("modEnvTarget", p.mod_env_target);
    add("modEnvDepth", p.mod_env_depth);
    let b = p.bypass;
    for (key, on) in [
        ("bypassNoise", b.noise),
        ("bypassFilter", b.filter),
        ("bypassEnv", b.env),
        ("bypassLfo1", b.lfo1),
        ("bypassLfo2", b.lfo2),
        ("bypassFm", b.fm),
        ("bypassModEnv", b.mod_env),
        ("bypassFilterEnv", b.filter_env),
    ] {
        add(key, f64::from(u8::from(on)));
    }
    fields
}

#[test]
fn synth_patches_match_worklet_synth_voice() {
    let cases = fixture("synth_patches.json");
    let cases = cases.as_array().expect("array");
    assert!(cases.len() >= 16);
    for case in cases {
        let name = case.get("name").and_then(Value::as_str).expect("name");
        let note = case.get("note").expect("note");
        let patch = synth_patch(
            case.get("preset").expect("preset"),
            SynthNote {
                ratio: num(note, "ratio"),
                velocity: num(note, "velocity"),
                pan: num(note, "pan"),
                bpm: num(note, "bpm"),
            },
        );
        let update = case.get("update").expect("update");
        let fields = patch_fields(&patch);
        let expected_keys =
            update.as_object().expect("object").iter().filter(|(k, _)| *k != "type" && *k != "fmAlgo").count();
        assert_eq!(fields.len(), expected_keys + 1, "{name}: field count");
        for (key, got) in fields {
            let want = match update.get(&key) {
                Some(Value::Bool(b)) => f64::from(u8::from(*b)),
                Some(v) => v.as_f64().unwrap_or_else(|| panic!("{name}.{key}")),
                None => panic!("{name}: no {key}"),
            };
            assert_eq!(got, want, "{name}.{key}");
        }
    }
}
