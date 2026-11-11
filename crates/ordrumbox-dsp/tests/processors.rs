//! The processors (strip, master bus, synth voice) on the cases of
//! `render_cases`, quantum by quantum, against Rust snapshots of their output.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

mod render_cases;

use std::path::{Path, PathBuf};

use ordrumbox_dsp::{MasterBus, MasterSettings, Strip, StripSettings, SynthVoice};
use render_cases::{MASTER_CASES, MasterCase, STRIP_CASES, StripCase, SynthCase, synth_cases};

const QUANTUM: usize = 128;
/// Largest difference allowed with a snapshot (math functions of the
/// platforms may differ in their last bit).
const TOLERANCE: f32 = 2e-5;
/// Every frame below this one is kept in the snapshots, then one in
/// `SNAPSHOT_STRIDE`.
const FULL_FRAMES: usize = 1024;
const SNAPSHOT_STRIDE: usize = 4;

/// The snapshots: the synth voice's cases, then the strip's and master bus's.
const SYNTH_SNAPSHOT: &str = "synth_renders.bin";
const EFFECT_SNAPSHOT: &str = "effect_renders.bin";

/// Bursts of decaying noise with a click every 3000 frames, from integer
/// arithmetic only.
fn burst(frames: usize, seed: u32, gain: f64) -> Vec<f32> {
    let mut s = seed;
    let mut env = 0.0_f64;
    (0..frames)
        .map(|i| {
            s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            if i % 3000 == 0 {
                env = 1.0;
            }
            let noise = f64::from(s >> 8) / 16_777_216.0 * 2.0 - 1.0;
            let mut x = noise * env * 0.6;
            if i % 3000 == 0 {
                x += 0.9;
            }
            env *= 0.9993;
            (x * gain) as f32
        })
        .collect()
}

fn bursts(frames: usize, (seed_l, seed_r, gain): (u32, u32, f64)) -> (Vec<f32>, Vec<f32>) {
    (burst(frames, seed_l, gain), burst(frames, seed_r, gain))
}

fn render_strip(case: &StripCase) -> (Vec<f32>, Vec<f32>) {
    let frames = case.frames;
    let (in_l, in_r) = bursts(frames, case.input);
    let (mut out_l, mut out_r) = (vec![0.0; frames], vec![0.0; frames]);
    let mut strip = Strip::new(case.sample_rate);
    let mut settings = StripSettings::default();
    for (q, from) in (0..frames).step_by(QUANTUM).enumerate() {
        for (_, change) in case.changes.iter().filter(|(at, _)| *at == q) {
            change(&mut settings);
        }
        let to = (from + QUANTUM).min(frames);
        strip.process(&settings, &in_l[from..to], &in_r[from..to], &mut out_l[from..to], &mut out_r[from..to]);
    }
    (out_l, out_r)
}

fn render_master(case: &MasterCase) -> (Vec<f32>, Vec<f32>) {
    let frames = case.frames;
    let (mut l, mut r) = bursts(frames, case.input);
    let mut bus = MasterBus::new(case.sample_rate);
    let mut settings = MasterSettings::default();
    (case.settings)(&mut settings);
    for from in (0..frames).step_by(QUANTUM) {
        let to = (from + QUANTUM).min(frames);
        bus.process(&settings, &mut l[from..to], &mut r[from..to]);
    }
    (l, r)
}

/// One note, started and released, rendered quantum by quantum.
fn render_synth(case: &SynthCase) -> (Vec<f32>, Vec<f32>) {
    let frames = case.frames;
    let mut voice = SynthVoice::default();
    voice.start(&case.patch, 0, case.start_time, 1);
    voice.release_at(case.release_time);
    let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
    for from in (0..frames).step_by(QUANTUM) {
        let to = (from + QUANTUM).min(frames);
        voice.render(case.sample_rate, from as u64, &mut l[from..to], &mut r[from..to]);
    }
    (l, r)
}

/// The frames a snapshot keeps of a render, left then right.
fn thin((l, r): (Vec<f32>, Vec<f32>)) -> Vec<f32> {
    let kept = |(i, _): &(usize, f32)| *i < FULL_FRAMES || i % SNAPSHOT_STRIDE == 0;
    let side = |s: Vec<f32>| s.into_iter().enumerate().filter(kept).map(|(_, x)| x);
    side(l).chain(side(r)).collect()
}

/// Every case of a snapshot, rendered by the processors as they are.
fn renders(file: &str) -> Vec<(&'static str, Vec<f32>)> {
    if file == SYNTH_SNAPSHOT {
        return synth_cases().iter().map(|case| (case.name, thin(render_synth(case)))).collect();
    }
    let strips = STRIP_CASES.iter().map(|case| (case.name, thin(render_strip(case))));
    strips.chain(MASTER_CASES.iter().map(|case| (case.name, thin(render_master(case))))).collect()
}

fn snapshot_path(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(file)
}

/// Writes the snapshots from the processors as they are: `cargo test -p
/// ordrumbox-dsp --test processors -- --ignored`, when their sound changes on
/// purpose (`DEVELOPMENT.md`, "Snapshots").
#[test]
#[ignore = "writes the snapshots"]
fn write_the_snapshots() {
    for file in [SYNTH_SNAPSHOT, EFFECT_SNAPSHOT] {
        let bytes: Vec<u8> = renders(file).iter().flat_map(|(_, s)| s).flat_map(|x| x.to_le_bytes()).collect();
        std::fs::write(snapshot_path(file), bytes).expect("written");
    }
}

#[test]
fn the_synth_voice_matches_its_snapshot() {
    check_snapshot(SYNTH_SNAPSHOT);
}

#[test]
fn the_strip_and_master_bus_match_their_snapshot() {
    check_snapshot(EFFECT_SNAPSHOT);
}

fn check_snapshot(file: &str) {
    let bytes = std::fs::read(snapshot_path(file)).expect(file);
    let stored: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect();
    let cases = renders(file);
    assert_eq!(stored.len(), cases.iter().map(|(_, s)| s.len()).sum::<usize>(), "{file}: frames stored");
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
    assert!(failures.is_empty(), "{file}:\n{}", failures.join("\n"));
}
