//! The Playwright scenarios of ordrumbox-v2 that only need the engine
//! (`e2e/live-vs-export`, `song-wav-export`, `kit-change`, `cold-start`,
//! `song-playback`): playback driven like an audio callback, and the offline
//! renders, on the demo library (song.json) with a real sample (kick.wav).
//!
//! The JS tests judge the audio of a headless browser with tolerances; here
//! the renders are deterministic, so most comparisons are exact.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::path::Path;
use std::sync::Arc;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_engine::kit::{BankSample, KitSample};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{Controller, Mode, SampleBank, SynthPresets, TrackSound, wav};

const KEYS: [&str; 8] = ["KICK", "SNARE", "TOM", "CLAP", "COWBELL", "CHH", "OHH", "CRASH"];

fn fixture(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).expect(path)
}

fn library() -> Library {
    Library::from_json(&json::parse(&fixture("../ordrumbox-core/tests/fixtures/song.json")).expect("parses")).0
}

/// Kits `a` and `b`, each with every drum key of the demo: kick.wav in `a`,
/// the same at half level in `b` (so the two kits sound different).
fn bank() -> Arc<SampleBank> {
    let bytes = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/kick.wav")).expect("kick.wav");
    let buffer = wav::decode(&bytes).expect("decodes");
    let mut quieter = buffer.clone();
    quieter.left.iter_mut().for_each(|s| *s *= 0.5);
    let mut samples = Vec::new();
    for (kit, buffer) in [("a", &buffer), ("b", &quieter)] {
        for key in KEYS {
            samples.push(BankSample {
                kit: kit.into(),
                sample: KitSample {
                    key: key.into(),
                    display_name: key.to_lowercase(),
                    path: format!("{kit}/{key}.wav"),
                    decay: Some(5000.0),
                    ..Default::default()
                },
                buffer: buffer.clone().into(),
            });
        }
    }
    Arc::new(SampleBank { samples })
}

/// The first preset of synth_patches.json, as `BASS0`.
fn synths() -> SynthPresets {
    let cases = json::parse(&fixture("tests/fixtures/synth_patches.json")).expect("parses");
    let preset = cases.as_array().and_then(|c| c.first()).and_then(|c| c.get("preset")).cloned().expect("a preset");
    let mut presets = Object::new();
    presets.insert("BASS0", preset);
    SynthPresets::from_json(&Value::Object(presets))
}

fn setup(seed: u64) -> RenderSetup {
    RenderSetup {
        bank: bank(),
        scales: Scales::default(),
        synths: synths(),
        kit: "a".into(),
        sample_rate: EXPORT_RATE,
        seed,
    }
}

fn rms(signal: &[f32]) -> f64 {
    (signal.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>() / signal.len().max(1) as f64).sqrt()
}

fn peak(signal: &[f32]) -> f32 {
    signal.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// What playback gives: a controller and its audio side, updated before each
/// callback of `chunk` frames, as the UI thread and the audio thread do.
fn play_live(library: &Library, mode: Mode, frames: usize, chunk: usize, setup: &RenderSetup) -> Vec<f32> {
    let (mut controller, mut audio) = Controller::with_rng(
        setup.sample_rate as f32,
        setup.bank.clone(),
        setup.scales.clone(),
        setup.synths.clone(),
        Rng::new(setup.seed),
    );
    controller.set_kit(&setup.kit);
    controller.play(library, mode);
    let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for from in (0..frames).step_by(chunk) {
        let to = (from + chunk).min(frames);
        controller.update(library);
        audio.render(&mut left[from..to], &mut right[from..to]);
    }
    left
}

/// A library of the demo patterns and one song made of `clips`
/// (pattern id, start measure, measures).
fn with_song(bpm: f64, loop_measures: u32, clips: &[(&str, u32, f64)]) -> Library {
    let mut library = library();
    let clips: Vec<String> = clips
        .iter()
        .map(|(p, start, count)| format!(r#"{{"pattern":"{p}","startMeasure":{start},"measureCount":{count}}}"#))
        .collect();
    let song = format!(
        r#"{{"id":"e2e","name":"E2E Song","bpm":{bpm},"loopMeasureCount":{loop_measures},"clips":[{}]}}"#,
        clips.join(",")
    );
    let known = library.patterns.iter().map(|p| p.id.clone()).collect();
    library.songs =
        vec![ordrumbox_core::Song::from_json(&json::parse(&song).unwrap(), &known, "song", &mut Vec::new()).unwrap().0];
    library
}

// ─── live-vs-export.spec.js ─────────────────────────────────────────────────

/// RMS of each block of 1024 frames.
fn envelope(signal: &[f32]) -> Vec<f64> {
    signal.chunks(1024).map(rms).collect()
}

/// Pearson correlation of two envelopes.
fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    let (mean_a, mean_b) = (a.iter().sum::<f64>() / n, b.iter().sum::<f64>() / n);
    let (mut cov, mut var_a, mut var_b) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        cov += (x - mean_a) * (y - mean_b);
        var_a += (x - mean_a).powi(2);
        var_b += (y - mean_b).powi(2);
    }
    cov / (var_a * var_b).sqrt()
}

#[test]
fn the_wav_export_sounds_like_playback() {
    let library = library();
    let setup = setup(3);
    let p = library.patterns.iter().position(|p| p.id == "rock").expect("rock");
    let (export, _) = offline::render_pattern(&library, p, 1, &setup, None).expect("renders");
    // rock: 8 beats at 120 bpm = 4 s.
    assert_eq!(export.len(), 4 * EXPORT_RATE as usize);
    assert!(rms(&export) > 0.001 && peak(&export) > 0.001, "export rms {}", rms(&export));

    // Playback in callbacks of whole quanta (128 frames), same seed: the same samples.
    for chunk in [128, 1024] {
        let live = play_live(&library, Mode::Pattern(p), export.len(), chunk, &setup);
        assert!(live == export, "callbacks of {chunk} frames: playback differs from the export");
    }

    // Any other callback size cuts the quanta elsewhere (parameters are read
    // once per quantum): the JS test's tolerances.
    let live = play_live(&library, Mode::Pattern(p), export.len(), 300, &setup);
    let relative = |a: f64, b: f64| (a - b).abs() / a.max(b);
    assert!(relative(rms(&live), rms(&export)) < 0.05);
    assert!(relative(f64::from(peak(&live)), f64::from(peak(&export))) < 0.05);
    let mean_diff = live.iter().zip(&export).map(|(a, b)| f64::from((a - b).abs())).sum::<f64>() / live.len() as f64;
    assert!(mean_diff < 0.005, "mean sample difference {mean_diff}");
    let correlation = correlation(&envelope(&live), &envelope(&export));
    assert!(correlation > 0.95, "envelope correlation {correlation}");
}

#[test]
fn a_synth_pattern_exports_audible_audio_every_time() {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"id":"synth","name":"SynthExportTest","bpm":120,"beatCount":2,"tracks":[
            {"name":"BASS","useSoftSynth":true,"synthSoundKey":"BASS0","stepsPerBeat":4,
             "notes":[{"beat":0,"beatStep":0},{"beat":1,"beatStep":0}]}]}]}"#,
    )
    .unwrap();
    for seed in [1, 2] {
        let (left, _) = offline::render_pattern(&library, 0, 1, &setup(seed), None).expect("renders");
        // 2 beats at 120 bpm = 1 s.
        assert_eq!(left.len(), EXPORT_RATE as usize);
        assert!(rms(&left) > 0.001, "render {seed}: rms {}", rms(&left));
    }
}

// ─── song-wav-export.spec.js ────────────────────────────────────────────────

#[test]
fn a_song_renders_at_its_length_with_audio_in_it() {
    let library = with_song(120.0, 2, &[("rock", 0, 2.0), ("slrock", 0, 2.0)]);
    let (left, right) = offline::render_song(&library, 0, &setup(1)).expect("renders");
    // 2 measures of 4/4 at 120 bpm = 4 s, a valid 16-bit stereo file.
    assert_eq!(left.len(), 4 * EXPORT_RATE as usize);
    let bytes = wav::encode(&left, &right, EXPORT_RATE);
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([bytes[22], bytes[23]]), 2);
    assert_eq!(u32::from_le_bytes(bytes[24..28].try_into().unwrap()), EXPORT_RATE);
    assert_eq!(u16::from_le_bytes([bytes[34], bytes[35]]), 16);
    assert!(rms(&left) > 0.001);
    assert!(peak(&left) > 0.05);
}

#[test]
fn a_song_length_follows_the_song_tempo_not_a_pattern_one() {
    let library = with_song(240.0, 2, &[("slrock", 0, 2.0)]);
    let (left, _) = offline::render_song(&library, 0, &setup(1)).expect("renders");
    // slrock plays at 115 bpm on its own; the song at 240: 2 measures = 2 s.
    assert_eq!(left.len(), 2 * EXPORT_RATE as usize);
    assert!(rms(&left) > 0.001);
}

#[test]
fn a_second_clip_on_the_same_measure_changes_the_render() {
    let single = with_song(120.0, 2, &[("rock", 0, 2.0)]);
    let layered = with_song(120.0, 2, &[("rock", 0, 2.0), ("slrock", 0, 2.0)]);
    let (a, _) = offline::render_song(&single, 0, &setup(1)).expect("renders");
    let (b, _) = offline::render_song(&layered, 0, &setup(1)).expect("renders");
    assert!(rms(&a) > 0.001);
    let differing = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    let relative = (a.iter().zip(&b).map(|(x, y)| f64::from(x - y).powi(2)).sum::<f64>()
        / a.iter().map(|&x| f64::from(x).powi(2)).sum::<f64>())
    .sqrt();
    assert!(differing * 2 > a.len(), "{differing} of {} samples differ", a.len());
    assert!(relative > 0.05, "relative difference {relative}");
}

#[test]
fn a_song_without_a_playable_clip_is_refused() {
    let library = with_song(120.0, 0, &[]);
    assert!(offline::render_song(&library, 0, &setup(1)).is_none());
}

#[test]
fn the_song_export_follows_the_arrangement_loop() {
    // The demo arrangement is 66 measures; its loop shortened to 2.
    let mut library = library();
    library.songs[0].loop_measures = Some(2.0);
    let (left, _) = offline::render_song(&library, 0, &setup(1)).expect("renders");
    assert_eq!(left.len(), 4 * EXPORT_RATE as usize);
}

// ─── kit-change.spec.js, cold-start.spec.js ─────────────────────────────────

/// The sample of `sound`, as (kit, key).
fn sample_of(bank: &SampleBank, sound: &TrackSound) -> Option<(String, String)> {
    match sound {
        TrackSound::Sample(i) => bank.get(*i).map(|s| (s.kit.clone(), s.sample.key.clone())),
        _ => None,
    }
}

#[test]
fn a_kit_change_while_playing_reassigns_every_auto_track() {
    let library = library();
    let p = library.patterns.iter().position(|p| p.id == "slrock").expect("slrock");
    let pattern = &library.patterns[p];
    let bank = bank();
    let (mut controller, mut audio) =
        Controller::with_rng(8000.0, bank.clone(), Scales::default(), SynthPresets::default(), Rng::new(5));
    controller.set_kit("a");
    // Cold start: every track has a sound before anything played.
    for t in 0..pattern.tracks.len() {
        assert_eq!(sample_of(&bank, &controller.track_sound(&library, p, t)).map(|s| s.0).as_deref(), Some("a"));
    }
    controller.play(&library, Mode::Pattern(p));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut run = |controller: &mut Controller, callbacks: usize| {
        let mut energy = 0.0;
        for _ in 0..callbacks {
            controller.update(&library);
            audio.render(&mut l, &mut r);
            energy += l.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
        }
        energy
    };
    // A beat at 115 bpm is about 4200 frames: the first pass sounds.
    assert!(run(&mut controller, 40) > 0.0);
    let before = controller.position().expect("playing");

    controller.set_kit("b");
    // Two passes later (4 beats each, about 66 callbacks), still playing, on.
    assert!(run(&mut controller, 140) > 0.0);
    assert!(controller.is_playing());
    assert!(controller.position().expect("playing").pass > before.pass);
    for (t, track) in pattern.tracks.iter().enumerate() {
        assert!(track.auto_assign);
        let (kit, key) = sample_of(&bank, &controller.track_sound(&library, p, t)).expect("a sample");
        assert_eq!(kit, "b", "track {} plays {key} of kit {kit}", track.name);
    }
}

// ─── song-playback.spec.js ──────────────────────────────────────────────────

#[test]
fn starting_a_song_makes_every_pattern_it_plays_audible() {
    let library = library();
    let bank = bank();
    let (mut controller, mut audio) =
        Controller::with_rng(8000.0, bank.clone(), Scales::default(), SynthPresets::default(), Rng::new(5));
    controller.set_kit("a");
    controller.play(&library, Mode::Song(0));
    // A measure at 120 bpm = 16000 frames: two measures.
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut energy = 0.0;
    for _ in 0..32_000 / 256 {
        controller.update(&library);
        audio.render(&mut l, &mut r);
        energy += l.iter().map(|&s| f64::from(s).powi(2)).sum::<f64>();
    }
    assert!(energy > 0.0);
    assert_eq!(controller.mode(), Mode::Song(0));
    assert!(controller.position().expect("playing").pass >= 1, "the arrangement advances");
    // Every pattern of the arrangement, not only the selected one, has its sounds.
    for clip in &library.songs[0].clips {
        let p = library.patterns.iter().position(|p| p.id == clip.pattern_id).expect("known pattern");
        for (t, track) in library.patterns[p].tracks.iter().enumerate() {
            let sound = controller.track_sound(&library, p, t);
            assert_ne!(sound, TrackSound::None, "{} track {} is silent", clip.pattern_id, track.name);
        }
    }
}
