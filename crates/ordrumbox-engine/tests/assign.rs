//! Sound assignment and WAV encoding against the JS app (`assign.json`,
//! `wav.json`, from tools/export_assign_fixtures.mjs).

use std::path::Path;

use ordrumbox_core::Track;
use ordrumbox_core::json::{self, Value};
use ordrumbox_dsp::SampleBuffer;
use ordrumbox_engine::assign::{TrackSound, assign_track};
use ordrumbox_engine::kit::BankSample;
use ordrumbox_engine::{SampleBank, SynthPresets, kits_from_json, wav};

fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name);
    json::parse(&std::fs::read_to_string(path).expect(name)).expect("parses")
}

/// Every sample of drumkits.json, in file order (the JS registry order).
fn bank() -> (SampleBank, Vec<String>) {
    let kits = kits_from_json(&fixture("drumkits.json"));
    let samples = kits
        .iter()
        .flat_map(|kit| {
            kit.instruments.iter().map(|instrument| BankSample {
                kit: kit.name.clone(),
                instrument: instrument.clone(),
                buffer: SampleBuffer::mono(vec![0.0], 44_100),
            })
        })
        .collect();
    (SampleBank { samples }, kits.iter().map(|k| k.name.clone()).collect())
}

#[test]
fn auto_assign_picks_what_the_js_picks() {
    let (bank, kits) = bank();
    let cases = fixture("assign.json");
    let cases = cases.as_array().expect("cases");
    assert!(cases.len() > 500);
    let synths = SynthPresets::default();
    for case in cases {
        let name = case.get("name").and_then(Value::as_str).expect("name");
        let kit = &kits[case.get("kit").and_then(Value::as_f64).expect("kit") as usize];
        let track = Track { name: name.to_owned(), ..Track::default() };
        for (label, pick) in [("first", 0usize), ("last", usize::MAX)] {
            let mut choose = |n: usize| pick.min(n - 1);
            let got = match assign_track(&track, &bank, kit, &synths, &mut choose) {
                TrackSound::Sample(i) => bank.samples[i].instrument.url.clone(),
                TrackSound::None => "NOT_DEFINED".to_owned(),
                TrackSound::Synth(key) => panic!("synth {key}"),
            };
            let want = case.get(label).and_then(Value::as_str).expect(label);
            assert_eq!(got, want, "{name:?} in kit {kit}, {label} candidate");
        }
    }
}

#[test]
fn synth_tracks_get_a_preset() {
    let (bank, _) = bank();
    let synths = SynthPresets::from_json(&json::parse(r#"{"A":{},"B":{}}"#).unwrap());
    let synth =
        |key: Option<&str>| Track { use_soft_synth: true, synth_sound_key: key.map(str::to_owned), ..Track::default() };
    let mut last = |n: usize| n - 1;
    assert_eq!(assign_track(&synth(Some("A")), &bank, "real", &synths, &mut last), TrackSound::Synth("A".into()));
    assert_eq!(assign_track(&synth(Some("gone")), &bank, "real", &synths, &mut last), TrackSound::Synth("B".into()));
    assert_eq!(assign_track(&synth(None), &bank, "real", &SynthPresets::default(), &mut last), TrackSound::None);
    let explicit = Track { use_auto_assign_sound: false, sample_id: "matt/RIMSHOT.wav".into(), ..Track::default() };
    let found = assign_track(&explicit, &bank, "real", &synths, &mut last);
    assert_eq!(found, TrackSound::Sample(bank.find_url("matt/RIMSHOT.wav").unwrap()));
}

#[test]
fn wav_bytes_match_buffer_to_wav() {
    let fx = fixture("wav.json");
    let channel = |key: &str| -> Vec<f32> {
        fx.get(key).and_then(Value::as_array).unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect()
    };
    let bytes = wav::encode(&channel("left"), &channel("right"), 44_100);
    let expected = base64(fx.get("bytes").and_then(Value::as_str).unwrap());
    assert_eq!(bytes, expected);
    let decoded = wav::decode(&bytes).expect("decodes");
    assert_eq!((decoded.frames(), decoded.sample_rate), (40, 44_100));
}

/// Standard base64.
fn base64(text: &str) -> Vec<u8> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0u32, 0);
    for c in text.bytes().filter(|&c| c != b'=') {
        acc = (acc << 6) | ALPHABET.iter().position(|&a| a == c).expect("base64") as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    out
}
