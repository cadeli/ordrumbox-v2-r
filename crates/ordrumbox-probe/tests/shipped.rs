//! What the app ships with (`assets/`), end to end: every sample of every kit
//! is in the format the kits are made in and needs no repair, and every
//! demo pattern (the starter library's and those of `demos/`), with every
//! kit, plays notes that are well formed and renders audio that sounds and
//! does not clip; so does every demo song.
//!
//! Nothing here is a snapshot: the demos and the kits change without
//! the code changing.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Rng, Scales, expand_hits};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{Kit, SampleBank, SynthPresets, kits_from_json, wav};
use ordrumbox_probe::fix::{FixSettings, fix};
use ordrumbox_probe::kit_files::read_kits;

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

/// The kits as the app reads them: its kit folders, as `kits.json` describes them.
fn kits() -> Vec<Kit> {
    let (described, issues) = kits_from_json(&data("kits.json")).unwrap();
    assert!(issues.is_empty(), "kits.json: {issues:#?}");
    let (kits, issues) = read_kits(&assets().join("kits"), &described).unwrap();
    assert!(issues.is_empty(), "kits/: {issues:#?}");
    kits
}

/// Library file `name` of `data/`, read without a value repaired.
fn library_file(name: &str) -> Library {
    let (library, report) = Library::from_json(&data(name));
    assert!(report.issues.is_empty(), "{name}: {:#?}", report.issues);
    assert!(report.dropped_clips.is_empty(), "{name}: {:#?}", report.dropped_clips);
    library
}

/// Every demo: the starter library's patterns and songs and those of each
/// style of `demos/` (the starter's again among them), in one library.
fn library() -> Library {
    let mut library = library_file("library.json");
    let mut names: Vec<String> = std::fs::read_dir(assets().join("data").join("demos"))
        .unwrap()
        .map(|entry| format!("demos/{}", entry.unwrap().file_name().to_string_lossy()))
        .collect();
    names.sort();
    assert!(!names.is_empty());
    for name in names {
        let style = library_file(&name);
        let new_patterns: Vec<_> = style.patterns.into_iter().filter(|p| library.pattern(&p.id).is_none()).collect();
        let new_songs: Vec<_> = style.songs.into_iter().filter(|s| library.song_index(&s.id).is_none()).collect();
        library.patterns.extend(new_patterns);
        library.songs.extend(new_songs);
    }
    library
}

/// The shipped scales, every one read.
fn scales() -> Scales {
    let (scales, issues) = Scales::from_json(&data("scales.json"));
    assert!(issues.is_empty(), "{issues:#?}");
    scales
}

fn setup(bank: &Arc<SampleBank>, kit: &str) -> RenderSetup {
    RenderSetup {
        bank: bank.clone(),
        scales: scales(),
        synths: SynthPresets::from_json(&data("synth_presets.json")).unwrap(),
        kit: kit.to_owned(),
        sample_rate: EXPORT_RATE,
        seed: 1,
        returns: Default::default(),
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
    let scales = scales();
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
fn every_drum_track_of_the_demos_follows_the_kit() {
    // Changing the kit while a demo plays changes its drums: no fixed sample, no synth preset.
    // A melodic track (a bass, a pad) keeps its synth preset, which no drum kit replaces; so
    // does a sound no kit holds (a dub siren), which the kit could only give a stand-in for.
    let catalog = ordrumbox_core::instruments::catalog();
    let group = |name: &str| {
        catalog.by_id(name).or_else(|| catalog.by_name(name)).map(|i| i.variant_of.clone().unwrap_or(i.id.clone()))
    };
    let kits = kits();
    let in_a_kit = |name: &str| {
        let wanted = group(name);
        kits.iter().flat_map(|kit| &kit.samples).any(|s| s.key.as_deref().is_some_and(|key| group(key) == wanted))
    };
    let fixed: Vec<String> = library()
        .patterns
        .iter()
        .flat_map(|p| p.tracks.iter().map(move |t| (p, t)))
        .filter(|(_, t)| t.use_synth || !t.auto_assign)
        .filter(|(_, t)| !catalog.by_id(&t.name).or_else(|| catalog.by_name(&t.name)).is_some_and(|i| i.melodic))
        .filter(|(_, t)| in_a_kit(&t.name))
        .map(|(p, t)| format!("{} {}", p.id, t.name))
        .collect();
    assert!(fixed.is_empty(), "{} drum tracks with a fixed sound:\n{}", fixed.len(), fixed.join("\n"));
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
fn every_demo_song_renders_clean_audio() {
    let library = library();
    let kits = kits();
    let bank = bank(&kits);
    assert!(!library.songs.is_empty());
    for (s, song) in library.songs.iter().enumerate() {
        let render = offline::render_song(&library, s, &setup(&bank, &kits[0].name)).expect("a song with clips");
        assert_eq!(fault(&render), None, "song {}", song.id);
    }
}
