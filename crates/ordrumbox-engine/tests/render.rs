//! The demo library through the whole chain: a real kit sample (kick.wav,
//! 24-bit mono 44.1 kHz from ordrumbox-v2), passes at 48 kHz, offline render.

use std::path::Path;
use std::sync::Arc;

use ordrumbox_core::json;
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Library, Rng, Scales, recompute_flat_notes};
use ordrumbox_engine::assign::{TrackSound, assign_pattern};
use ordrumbox_engine::kit::{BankSample, Instrument};
use ordrumbox_engine::pass::{PassContext, build_pass, frames_per_tick};
use ordrumbox_engine::{Engine, SampleBank, SynthPresets, wav};

const RATE: f64 = 48_000.0;

fn library() -> Library {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ordrumbox-core/tests/fixtures/song.json");
    let text = std::fs::read_to_string(path).expect("song.json");
    Library::from_json(&json::parse(&text).expect("parses")).0
}

/// Every drum key of the demo kits played by the same kick sample.
fn bank() -> SampleBank {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kick.wav")).expect("kick.wav");
    let buffer = wav::decode(&bytes).expect("decodes");
    assert_eq!((buffer.sample_rate, buffer.frames(), buffer.is_stereo()), (44_100, 9_802, false));
    let samples = ["KICK", "SNARE", "TOM", "CLAP", "COWBELL", "CHH", "OHH", "CRASH"]
        .iter()
        .map(|key| BankSample {
            kit: "test".into(),
            instrument: Instrument {
                key: (*key).into(),
                display_name: key.to_lowercase(),
                url: format!("{key}.wav"),
                decay_ms: 5000.0,
            },
            buffer: buffer.clone(),
        })
        .collect();
    SampleBank { samples }
}

#[test]
fn every_note_becomes_an_event_on_its_frame() {
    let library = library();
    let bank = bank();
    let scales = Scales::default();
    for pattern in &library.patterns {
        let synths = SynthPresets::default();
        let sounds = assign_pattern(pattern, &bank, "test", &synths, &mut |_| 0);
        let context = PassContext {
            bank: &bank,
            sounds: &sounds,
            scales: &scales,
            synths: &synths,
            sample_rate: RATE,
            first_tick: 0,
        };
        let pass = build_pass(pattern, 0, &mut Rng::new(1), context);
        let flat = recompute_flat_notes(pattern, 0, &mut Rng::new(1), &scales);
        let per_tick = frames_per_tick(pattern.bpm, RATE);

        let any_solo = pattern.tracks.iter().any(|t| t.solo);
        let audible = |i: usize| if any_solo { pattern.tracks[i].solo } else { !pattern.tracks[i].mute };
        let expected: Vec<u64> = flat
            .iter()
            .flat_map(|(&tick, notes)| {
                notes.iter().filter(|n| matches!(sounds[n.track], TrackSound::Sample(_)) && audible(n.track)).map(
                    move |n| {
                        let swing = n.swing_beats(&pattern.tracks[n.track]) * f64::from(TICK);
                        (((f64::from(tick) + swing) * per_tick).round() as u64).min(pass.frames - 1)
                    },
                )
            })
            .collect();
        let mut expected = expected;
        expected.sort_unstable();
        let frames: Vec<u64> = pass.events.iter().map(|e| e.frame).collect();
        assert_eq!(frames, expected, "{}", pattern.id);
        assert_eq!(pass.frames, (f64::from(TICK) * f64::from(pattern.beat_count) * per_tick).round() as u64);
    }
}

#[test]
fn rock_renders_clean_audio_starting_on_frame_zero() {
    let library = library();
    let bank = Arc::new(bank());
    let rock = library.pattern("rock").expect("rock");
    let (scales, synths) = (Scales::default(), SynthPresets::default());
    let sounds = assign_pattern(rock, &bank, "test", &synths, &mut |_| 0);
    let context = PassContext {
        bank: &bank,
        sounds: &sounds,
        scales: &scales,
        synths: &synths,
        sample_rate: RATE,
        first_tick: 0,
    };
    let pass = Arc::new(build_pass(rock, 0, &mut Rng::new(1), context));
    let frames = pass.frames as usize;

    let mut engine = Engine::new(RATE as f32, bank.clone());
    engine.play(pass.clone());
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    engine.render(&mut left, &mut right);

    assert!(left.iter().chain(&right).all(|s| s.is_finite() && s.abs() <= 1.0));
    // The kick on beat 1 starts at frame 0: sound appears where the sample's
    // own leading silence ends, resampled from 44.1 to 48 kHz.
    let lead = bank.samples[0].buffer.left.iter().position(|s| s.abs() > 1e-6).expect("sample has sound");
    let expected = lead as f64 * RATE / 44_100.0;
    let first = left.iter().position(|s| s.abs() > 1e-6).expect("some sound");
    assert!((first as f64 - expected).abs() <= 2.0, "sound from frame {first}, expected about {expected}");
    let energy: f64 = left.iter().map(|&s| f64::from(s).powi(2)).sum();
    assert!(energy > 1.0, "energy {energy}");
    // At the very end of the pass; the next one starts on the next frame rendered.
    assert_eq!(engine.position().map(|p| (p.pass, p.frame)), Some((0, pass.frames)));
}
