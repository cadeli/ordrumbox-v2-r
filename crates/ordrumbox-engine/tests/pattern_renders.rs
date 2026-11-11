//! The whole chain against a Rust snapshot of its output,
//! `pattern_renders.bin`: hand-written patterns and a song rendered as the
//! WAV export does (`offline`), from the notes to the master bus. The
//! processors have their own snapshots (`ordrumbox-dsp`); this one holds what
//! is between them: the sample voice (resampled, pitched, stereo), each
//! note's level, pan and frame, swing, repeats, the tracks' mix, a synth
//! beside the samples, the clips of a song. `write_the_snapshot` rewrites it
//! when what a pattern sounds like changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::kit::{BankSample, KitSample};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets, wav};

/// Largest difference allowed with the snapshot (math functions of the
/// platforms may differ in their last bit).
const TOLERANCE: f32 = 2e-5;
/// Every frame below this one is kept in the snapshot, then one in
/// `SNAPSHOT_STRIDE`.
const FULL_FRAMES: usize = 1024;
const SNAPSHOT_STRIDE: usize = 8;
const SNAPSHOT: &str = "pattern_renders.bin";
/// The seed of the renders.
const SEED: u64 = 3;

/// What a case renders of its library.
#[derive(Debug, Clone, Copy)]
enum What {
    /// Pattern 0, this many times.
    Pattern(u32),
    /// Song 0.
    Song,
}

/// A library, what is rendered of it and at which rate.
#[derive(Debug)]
struct Case {
    name: &'static str,
    sample_rate: u32,
    what: What,
    library: &'static str,
}

/// Levels, pans and pitches of sampled notes, on tracks with their own.
const SAMPLES: &str = r#"{"patterns":[{"id":"a","bpm":120,"beatCount":2,"tracks":[
    {"name":"KICK","notes":[{"velocity":1},{"beat":1,"velocity":0.5,"pitch":-5}]},
    {"name":"SNARE","pan":-0.6,"volume":0.7,"notes":[{"step":2,"pan":0.9},{"beat":1,"pitch":7}]},
    {"name":"CHH","transpose":3,"notes":[{},{"step":1,"velocity":0.3},{"step":2},{"beat":1,"step":3,"pan":-1}]},
    {"name":"OHH","pan":0.4,"notes":[{"beat":1,"step":2}]}]}]}"#;

const CASES: &[Case] = &[
    Case { name: "samples_44k", sample_rate: 44_100, what: What::Pattern(1), library: SAMPLES },
    // The bank's 44.1 kHz and 22.05 kHz samples, read at another rate.
    Case { name: "samples_48k", sample_rate: 48_000, what: What::Pattern(1), library: SAMPLES },
    // Swung steps, repeats that fade, an euclidean fill, a note every second
    // pass, a mono track cutting its own tail, a muted track: two passes.
    Case {
        name: "swing_and_repeats",
        sample_rate: 44_100,
        what: What::Pattern(2),
        library: r#"{"patterns":[{"id":"a","bpm":140,"beatCount":2,"tracks":[
            {"name":"CHH","swingAmount":0.6,"swingResolution":2,
             "notes":[{},{"step":1},{"step":2},{"step":3},{"beat":1},{"beat":1,"step":1}]},
            {"name":"SNARE","notes":[{"beat":1,"retriggerCount":4,"retriggerRate":2,"retriggerFade":1}]},
            {"name":"KICK","mono":true,"notes":[{"euclideanPulses":3},{"beat":1,"step":2,"cycle":2}]},
            {"name":"OHH","mute":true,"notes":[{}]}]}]}"#,
    },
    // A synth track beside the samples, effects on the strips.
    Case {
        name: "synth_and_effects",
        sample_rate: 44_100,
        what: What::Pattern(1),
        library: r#"{"patterns":[{"id":"a","bpm":120,"beatCount":2,"tracks":[
            {"name":"BASS","useSynth":true,"synthPreset":"BASS0","notes":[{"pitch":0},{"beat":1,"pitch":7,"velocity":0.5}]},
            {"name":"KICK","reverbType":"room","reverbMix":0.4,"notes":[{},{"beat":1}]},
            {"name":"SNARE","delayMix":0.5,"delayBeats":0.5,"filterType":"lowpass","filterCutoff":2000,
             "notes":[{"step":2}]}]}]}"#,
    },
    // Two clips on the same measure, then one alone, at the song's tempo.
    Case {
        name: "song",
        sample_rate: 44_100,
        what: What::Song,
        library: r#"{"songs":[{"id":"s","bpm":180,"clips":[
                {"patternId":"a","startMeasure":0,"measureCount":2},
                {"patternId":"b","startMeasure":0,"measureCount":1}]}],
            "patterns":[
            {"id":"a","bpm":90,"beatCount":4,"tracks":[{"name":"KICK","notes":[{},{"beat":2}]}]},
            {"id":"b","bpm":100,"beatCount":4,"tracks":[{"name":"CHH","notes":[{"beat":1},{"beat":3,"step":2}]}]}]}"#,
    },
];

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// Decaying noise, from integer arithmetic only: the same on every platform.
fn noise(frames: usize, seed: u32, decay: f64) -> Vec<f32> {
    let mut s = seed;
    let mut level = 0.5_f64;
    (0..frames)
        .map(|_| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let x = (f64::from(s >> 8) / 16_777_216.0 * 2.0 - 1.0) * level;
            level *= decay;
            x as f32
        })
        .collect()
}

/// One kit: a recorded kick (24-bit, 44.1 kHz), a mono snare, a hi-hat
/// recorded at 22.05 kHz and a stereo open hi-hat.
fn bank() -> Arc<SampleBank> {
    let kick = wav::decode(&std::fs::read(fixture("kick.wav")).expect("kick.wav")).expect("decodes");
    let stereo =
        SampleBuffer { left: noise(6000, 5, 0.9992), right: Some(noise(6000, 6, 0.9990)), sample_rate: 44_100 };
    let buffers = [
        ("KICK", kick),
        ("SNARE", SampleBuffer::mono(noise(8000, 2, 0.9994), 44_100)),
        ("CHH", SampleBuffer::mono(noise(1500, 3, 0.996), 22_050)),
        ("OHH", stereo),
    ];
    let samples = buffers
        .into_iter()
        .map(|(key, buffer)| BankSample {
            kit: "kit".into(),
            sample: KitSample {
                key: key.into(),
                display_name: key.to_lowercase(),
                path: format!("kit/{key}.wav"),
                ..Default::default()
            },
            buffer: buffer.into(),
        })
        .collect();
    Arc::new(SampleBank { samples })
}

/// The `BASS0` preset of `generated_sounds.json`.
fn synths() -> SynthPresets {
    let text = std::fs::read_to_string(fixture("generated_sounds.json")).expect("generated_sounds.json");
    let preset = json::parse(&text).expect("parses").get("BASS0").cloned().expect("a preset");
    let mut presets = Object::new();
    presets.insert("BASS0", preset);
    SynthPresets::from_json(&Value::Object(presets)).unwrap()
}

fn render(case: &Case) -> (Vec<f32>, Vec<f32>) {
    let (library, report) = Library::parse(case.library).unwrap_or_else(|e| panic!("{}: {e}", case.name));
    assert!(report.issues.is_empty(), "{}: {:#?}", case.name, report.issues);
    assert!(report.dropped_clips.is_empty(), "{}: {:#?}", case.name, report.dropped_clips);
    let setup = RenderSetup {
        bank: bank(),
        scales: Scales::default(),
        synths: synths(),
        kit: "kit".into(),
        sample_rate: case.sample_rate,
        seed: SEED,
    };
    let rendered = match case.what {
        What::Pattern(loops) => offline::render_pattern(&library, 0, loops, &setup, None),
        What::Song => offline::render_song(&library, 0, &setup),
    };
    rendered.unwrap_or_else(|| panic!("{}: nothing to render", case.name))
}

/// The frames the snapshot keeps of a render, left then right.
fn thin((l, r): (Vec<f32>, Vec<f32>)) -> Vec<f32> {
    let kept = |(i, _): &(usize, f32)| *i < FULL_FRAMES || i % SNAPSHOT_STRIDE == 0;
    let side = |s: Vec<f32>| s.into_iter().enumerate().filter(kept).map(|(_, x)| x);
    side(l).chain(side(r)).collect()
}

fn renders() -> Vec<(&'static str, Vec<f32>)> {
    CASES.iter().map(|case| (case.name, thin(render(case)))).collect()
}

/// Writes the snapshot from the engine as it is: `cargo test -p
/// ordrumbox-engine --test pattern_renders -- --ignored`, when what a pattern
/// sounds like changes on purpose (`DEVELOPMENT.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    let bytes: Vec<u8> = renders().iter().flat_map(|(_, s)| s).flat_map(|x| x.to_le_bytes()).collect();
    std::fs::write(fixture(SNAPSHOT), bytes).expect("written");
}

#[test]
fn the_renders_match_their_snapshot() {
    let bytes = std::fs::read(fixture(SNAPSHOT)).expect(SNAPSHOT);
    let stored: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let cases = renders();
    assert_eq!(stored.len(), cases.iter().map(|(_, s)| s.len()).sum::<usize>(), "frames stored");
    let mut rest = stored.as_slice();
    let mut failures = Vec::new();
    for (name, samples) in &cases {
        let (expected, after) = rest.split_at(samples.len());
        rest = after;
        let worst = samples.iter().zip(expected).map(|(got, want)| (got - want).abs()).fold(0.0_f32, f32::max);
        if worst.is_nan() || worst > TOLERANCE || samples.iter().any(|x| x.is_nan()) {
            failures.push(format!("{name}: max error {worst:.1e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A snapshot of silence would hold nothing: every case sounds on both
/// sides, and each differs from the others.
#[test]
fn every_case_sounds_and_none_repeats_another() {
    let rendered: Vec<_> = CASES.iter().map(|case| (case.name, render(case))).collect();
    for (name, (left, right)) in &rendered {
        for side in [left, right] {
            let peak = side.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.01 && peak <= 1.0, "{name}: peak {peak}");
        }
        assert!(left != right, "{name}: nothing panned");
    }
    for (i, (name, render)) in rendered.iter().enumerate() {
        for (other, other_render) in &rendered[i + 1..] {
            assert!(render != other_render, "{name} and {other} render the same");
        }
    }
}
