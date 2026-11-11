//! What the app ships with (`assets/`), end to end: every sample of every kit
//! is in the format the kits are made in and needs no repair, and every
//! pattern of the demo library, with every kit, plays notes that are well
//! formed and renders audio that sounds and does not clip; so does the demo
//! song.
//!
//! Nothing here is a snapshot: the demo library and the kits change without
//! the code changing.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Rng, Scales, expand_hits};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{Kit, SampleBank, SynthPresets, kits_from_json, wav};
use ordrumbox_probe::fix::{FixSettings, fix};

/// The sample rate of a kit sample.
const KIT_RATE: u32 = 44_100;
/// Passes of each pattern whose notes are checked.
const PASS_COUNT: u32 = 4;

fn assets() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

fn data(name: &str) -> Value {
    let path = assets().join("data").join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    json::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn kits() -> Vec<Kit> {
    let (kits, issues) = kits_from_json(&data("kits.json")).unwrap();
    assert!(issues.is_empty(), "kits.json: {issues:#?}");
    kits
}

fn library() -> Library {
    let (library, report) = Library::from_json(&data("library.json"));
    assert!(report.issues.is_empty(), "library.json: {:#?}", report.issues);
    assert!(report.dropped_clips.is_empty(), "library.json: {:#?}", report.dropped_clips);
    library
}

fn setup(bank: &Arc<SampleBank>, kit: &str) -> RenderSetup {
    RenderSetup {
        bank: bank.clone(),
        scales: Scales::from_json(&data("scales.json")),
        synths: SynthPresets::from_json(&data("synth_presets.json")).unwrap(),
        kit: kit.to_owned(),
        sample_rate: EXPORT_RATE,
        seed: 1,
    }
}

fn bank(kits: &[Kit]) -> Arc<SampleBank> {
    let (bank, errors) = SampleBank::load(kits, |path| std::fs::read(assets().join("kits").join(path)));
    assert!(errors.is_empty(), "{errors:#?}");
    bank.into_shared()
}

/// What is wrong with a render, if anything.
fn fault((left, right): &(Vec<f32>, Vec<f32>)) -> Option<String> {
    let mut peak = 0.0_f32;
    for &sample in left.iter().chain(right) {
        if !sample.is_finite() {
            return Some("a sample that is not a number".to_owned());
        }
        peak = peak.max(sample.abs());
    }
    if peak == 0.0 {
        Some("silent".to_owned())
    } else if peak > 1.0 {
        Some(format!("clips (peak {peak})"))
    } else {
        None
    }
}

/// The format tag, channel count, sample rate and bits of a WAV file's `fmt ` chunk.
fn wav_format(bytes: &[u8]) -> Option<(u16, u16, u32, u16)> {
    let at = bytes.windows(4).position(|w| w == b"fmt ")? + 8;
    let field = bytes.get(at..at + 16)?;
    let u16_at = |i: usize| u16::from_le_bytes([field[i], field[i + 1]]);
    Some((u16_at(0), u16_at(2), u32::from_le_bytes([field[4], field[5], field[6], field[7]]), u16_at(14)))
}

#[test]
fn every_kit_sample_is_mono_16_or_24_bit_pcm_at_44_1_khz_and_needs_no_repair() {
    let mut faults = Vec::new();
    let mut count = 0;
    for kit in kits() {
        for sample in &kit.samples {
            count += 1;
            let bytes = std::fs::read(assets().join("kits").join(&sample.path)).expect(&sample.path);
            match wav_format(&bytes) {
                Some((1, 1, KIT_RATE, 16 | 24)) => {}
                format => {
                    faults.push(format!(
                        "{}: format {format:?}, not mono 16- or 24-bit PCM at {KIT_RATE} Hz",
                        sample.path
                    ));
                }
            }
            let buffer = wav::decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", sample.path));
            // What `ordrumbox-probe fix` would do: a late start, a loud end, DC, a peak off −3 dB.
            for change in fix(&buffer, &FixSettings::default()).changes {
                faults.push(format!("{}: {change}", sample.path));
            }
        }
    }
    assert!(count > 0);
    assert!(faults.is_empty(), "{} faults:\n{}", faults.len(), faults.join("\n"));
}

#[test]
fn every_demo_pattern_plays_well_formed_notes() {
    let library = library();
    let scales = Scales::from_json(&data("scales.json"));
    for pattern in &library.patterns {
        let ticks = u32::try_from(pattern.beat_count).unwrap() * ordrumbox_core::steps::TICK;
        let mut played = 0;
        for pass in 0..PASS_COUNT {
            for seed in [1, 2] {
                for (tick, hits) in expand_hits(pattern, pass, &mut Rng::new(seed), &scales) {
                    assert!(tick < ticks, "{} pass {pass}: a note at tick {tick} of {ticks}", pattern.id);
                    for hit in &hits {
                        let track = pattern.tracks.get(hit.track).expect("the note's track");
                        assert!(hit.source.is_none_or(|s| s < track.notes.len()), "{}: source", pattern.id);
                        assert!((0.0..=1.0).contains(&hit.velocity), "{}: velocity {}", pattern.id, hit.velocity);
                        assert!((-1.0..=1.0).contains(&hit.pan), "{}: pan {}", pattern.id, hit.pan);
                        assert!(hit.pitch.is_finite(), "{}: pitch {}", pattern.id, hit.pitch);
                    }
                    played += hits.len();
                }
            }
        }
        assert!(played > 0, "{} plays nothing", pattern.id);
    }
}

#[test]
fn every_demo_pattern_renders_clean_audio_with_every_kit() {
    let library = library();
    let kits = kits();
    let bank = bank(&kits);
    // One thread a kit: some six hundred renders.
    let faults_of = |kit: &Kit| {
        let setup = setup(&bank, &kit.name);
        let mut faults = Vec::new();
        for (p, pattern) in library.patterns.iter().enumerate() {
            let render = offline::render_pattern(&library, p, 1, &setup, None).expect("a pattern");
            if let Some(fault) = fault(&render) {
                faults.push(format!("{} with kit {}: {fault}", pattern.id, kit.name));
            }
        }
        faults
    };
    let faults: Vec<String> = std::thread::scope(|scope| {
        let threads: Vec<_> = kits.iter().map(|kit| scope.spawn(|| faults_of(kit))).collect();
        threads.into_iter().flat_map(|thread| thread.join().expect("a kit's renders")).collect()
    });
    assert!(faults.is_empty(), "{} faults:\n{}", faults.len(), faults.join("\n"));
}

#[test]
fn the_demo_song_renders_clean_audio() {
    let library = library();
    let kits = kits();
    let bank = bank(&kits);
    assert!(!library.songs.is_empty());
    for (s, song) in library.songs.iter().enumerate() {
        let render = offline::render_song(&library, s, &setup(&bank, &kits[0].name)).expect("a song with clips");
        assert_eq!(fault(&render), None, "song {}", song.id);
    }
}
