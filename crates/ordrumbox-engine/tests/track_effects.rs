//! The effects of a track's strip, end to end: a pattern whose track settings
//! turn an effect on is rendered as the WAV export does
//! (`offline::render_pattern`), written to a 16-bit WAV file and read back,
//! then the file is measured against the same render without the effect.
//!
//! The sources are synthetic so that what an effect adds is measurable: a
//! noise burst of 50 ms (KICK) and a sine held 0.4 s at one level then 0.4 s
//! at twice that level (SNARE), both below the master bus's threshold, so
//! the master only adds its make-up gain and its gentle filters, the same to
//! every render; and a loud sine (TOM), for the saturation to bend.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::sync::Arc;

use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::kit::{BankSample, KitSample};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets, wav};

const RATE: f64 = EXPORT_RATE as f64;
/// Length of the KICK burst.
const BURST_SECONDS: f64 = 0.05;
/// Length of each level of the SNARE and TOM tones, their frequency, and the
/// SNARE's first level.
const TONE_SECONDS: f64 = 0.4;
const TONE_HZ: f64 = 440.0;
const TONE_LEVEL: f64 = 0.02;
/// Level of the TOM's tone, over its whole length.
const LOUD_LEVEL: f64 = 0.5;
/// The SNARE and the TOM play on beat 2: 1 s into the 2 s pattern at 120 bpm.
const TONE_START: f64 = 1.0;

fn frame(seconds: f64) -> usize {
    (seconds * RATE).round() as usize
}

/// A linear fade in and out of `fade` frames over `frames`, at frame `i`.
fn edge(i: usize, frames: usize, fade: usize) -> f64 {
    (i.min(frames - 1 - i) as f64 / fade as f64).min(1.0)
}

/// White noise from a fixed linear congruential generator, with 1 ms fades
/// so that the burst itself has no click.
fn burst() -> Vec<f32> {
    let frames = frame(BURST_SECONDS);
    let mut state: u32 = 0x1234_5678;
    (0..frames)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = f64::from(state >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0;
            (white * 0.05 * edge(i, frames, frame(0.001))) as f32
        })
        .collect()
}

/// A sine of `TONE_HZ`, `2 × TONE_SECONDS` long, at `first` for the first half
/// and `second` for the second.
fn tone(first: f64, second: f64) -> Vec<f32> {
    let frames = 2 * frame(TONE_SECONDS);
    (0..frames)
        .map(|i| {
            let level = if i < frames / 2 { first } else { second };
            let phase = std::f64::consts::TAU * TONE_HZ * i as f64 / RATE;
            (phase.sin() * level * edge(i, frames, frame(0.002))) as f32
        })
        .collect()
}

fn setup() -> RenderSetup {
    let sample = |key: &str, buffer: Vec<f32>| BankSample {
        kit: "t".into(),
        sample: KitSample {
            key: key.into(),
            display_name: key.to_lowercase(),
            path: format!("t/{key}.wav"),
            // To -60 dB in 100 s: 0.24 dB over a level of the tone.
            decay: Some(100_000.0),
            ..Default::default()
        },
        buffer: SampleBuffer::mono(buffer, EXPORT_RATE).into(),
    };
    RenderSetup {
        bank: Arc::new(SampleBank {
            samples: vec![
                sample("KICK", burst()),
                sample("SNARE", tone(TONE_LEVEL, 2.0 * TONE_LEVEL)),
                sample("TOM", tone(LOUD_LEVEL, LOUD_LEVEL)),
            ],
        }),
        scales: Scales::default(),
        synths: SynthPresets::default(),
        kit: "t".into(),
        sample_rate: EXPORT_RATE,
        seed: 1,
    }
}

/// The stereo channels of a WAV file.
struct Wav {
    left: Vec<f32>,
    right: Vec<f32>,
}

#[derive(Debug, Clone, Copy)]
enum Side {
    Left,
    Right,
}

impl Wav {
    fn channel(&self, side: Side) -> &[f32] {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }
}

/// A track of the pattern: its instrument, the beat of its one note, and its
/// settings (JSON members, each after a comma).
type TrackSpec<'a> = (&'a str, u32, &'a str);

/// One 4-beat pattern at 120 bpm of `tracks`, rendered once, written to a WAV
/// file and read back.
fn render(tracks: &[TrackSpec<'_>]) -> Wav {
    let tracks: Vec<String> = tracks
        .iter()
        .map(|(name, beat, settings)| {
            format!(r#"{{"name":"{name}","stepsPerBeat":4,"notes":[{{"beat":{beat},"beatStep":0}}]{settings}}}"#)
        })
        .collect();
    let text = format!(
        r#"{{"bpm":120,"patterns":[{{"id":"fx","name":"Fx","bpm":120,"beatCount":4,"tracks":[{}]}}]}}"#,
        tracks.join(",")
    );
    let (library, report) = Library::parse(&text).expect("parses");
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    let (left, right) = offline::render_pattern(&library, 0, 1, &setup(), None).expect("renders");
    assert_eq!(left.len(), 2 * EXPORT_RATE as usize, "4 beats at 120 bpm");
    let file = wav::decode(&wav::encode(&left, &right, EXPORT_RATE)).expect("reads back");
    assert_eq!(file.sample_rate, EXPORT_RATE);
    let right = file.right.expect("a stereo file");
    assert!(file.left.iter().chain(&right).all(|s| s.abs() < 1.0), "no clipping");
    Wav { left: file.left, right }
}

const MUTED: &str = r#","mute":true"#;

/// The KICK alone on beat 0, with `settings`.
fn kick(settings: &str) -> Wav {
    render(&[("KICK", 0, settings)])
}

/// The SNARE alone on beat 2, with `settings`.
fn snare(settings: &str) -> Wav {
    render(&[("SNARE", 2, settings)])
}

/// The TOM alone on beat 2, with `settings`.
fn tom(settings: &str) -> Wav {
    render(&[("TOM", 2, settings)])
}

/// Whether two files hold the same samples.
fn same(a: &Wav, b: &Wav) -> bool {
    a.left == b.left && a.right == b.right
}

fn energy(signal: &[f32]) -> f64 {
    signal.iter().map(|&s| f64::from(s).powi(2)).sum()
}

fn rms(signal: &[f32]) -> f64 {
    (energy(signal) / signal.len().max(1) as f64).sqrt()
}

fn db(ratio: f64) -> f64 {
    20.0 * ratio.log10()
}

/// The signal between two times, seconds.
fn span(signal: &[f32], from: f64, to: f64) -> &[f32] {
    &signal[frame(from)..frame(to)]
}

/// Energy of the first difference over energy: about 2 for white noise,
/// near 0 for a signal without highs.
fn brightness(signal: &[f32]) -> f64 {
    let diff: f64 = signal.windows(2).map(|w| f64::from(w[1] - w[0]).powi(2)).sum();
    diff / energy(signal)
}

/// Amplitude of the `hz` component of `signal` (a Goertzel filter).
fn amplitude_at(signal: &[f32], hz: f64) -> f64 {
    let coefficient = 2.0 * (std::f64::consts::TAU * hz / RATE).cos();
    let (mut s1, mut s2) = (0.0, 0.0);
    for &x in signal {
        let s = f64::from(x) + coefficient * s1 - s2;
        s2 = s1;
        s1 = s;
    }
    (s1 * s1 + s2 * s2 - coefficient * s1 * s2).sqrt() * 2.0 / signal.len() as f64
}

/// Normalized correlation of two signals of the same length.
fn correlation(a: &[f32], b: &[f32]) -> f64 {
    let dot: f64 = a.iter().zip(b).map(|(&x, &y)| f64::from(x) * f64::from(y)).sum();
    dot / (energy(a) * energy(b)).sqrt()
}

#[test]
fn a_dry_track_is_its_source_and_silence() {
    let dry = kick("");
    let sound = energy(span(&dry.left, 0.0, BURST_SECONDS + 0.01));
    assert!(sound > 1.0, "the burst is in the file: energy {sound}");
    // Nothing after the burst, beyond the master's filters ringing out.
    let after = energy(span(&dry.left, 0.1, 2.0));
    assert!(after < sound * 1e-6, "energy after the burst {after}");
    assert_eq!(dry.left, dry.right, "centred");
}

#[test]
fn the_reverb_adds_a_tail_and_its_led_turns_it_off() {
    let dry = kick("");
    let wet = kick(r#","reverbType":"hall","reverbMix":0.5"#);
    let sound = energy(span(&dry.left, 0.0, 0.1));
    let tail = energy(span(&wet.left, 0.2, 1.0));
    assert!(tail > sound * 0.01, "a tail of {tail} after a sound of {sound}");
    assert_ne!(wet.left, wet.right, "the hall is wide");
    let off = kick(r#","reverbType":"hall","reverbMix":0.5,"reverbOn":false"#);
    assert!(same(&off, &dry), "its LED off: the file without the reverb");
}

#[test]
fn a_slap_delay_repeats_the_burst_once_after_its_time() {
    let dry = kick("");
    // Half a beat at 120 bpm: 0.25 s.
    let wet = kick(r#","delayType":"slap","delayBeats":0.5,"delayMix":0.5"#);
    let echo = span(&wet.left, 0.25, 0.25 + BURST_SECONDS + 0.01);
    let source = span(&dry.left, 0.0, BURST_SECONDS + 0.01);
    let shape = correlation(echo, source);
    assert!(shape > 0.9, "the echo is the burst: correlation {shape}");
    let level = db((energy(echo) / energy(source)).sqrt());
    assert!(level > -12.0 && level < 0.0, "echo at {level:.1} dB");
    // Between the burst and the echo, and after the echo: silence (a slap has one echo).
    for (from, to) in [(0.1, 0.24), (0.35, 2.0)] {
        let rest = energy(span(&wet.left, from, to));
        assert!(rest < energy(source) * 1e-4, "energy {rest} from {from} s to {to} s");
    }
}

#[test]
fn the_tape_delay_repeats_with_feedback() {
    let wet = kick(r#","delayType":"tape","delayBeats":0.5,"delayMix":0.5,"delayFeedback":0.5"#);
    let echoes: Vec<f64> =
        (1..=3).map(|n| energy(span(&wet.left, 0.25 * f64::from(n), 0.25 * f64::from(n) + 0.08))).collect();
    assert!(echoes.iter().all(|&e| e > 0.0), "{echoes:?}");
    assert!(echoes.windows(2).all(|w| w[1] < w[0]), "each echo quieter: {echoes:?}");
}

#[test]
fn the_filters_take_the_highs_or_the_lows_off() {
    let dry = brightness(span(&kick("").left, 0.0, 0.1));
    let low = brightness(span(&kick(r#","filterType":"lowpass","filterCutoff":500"#).left, 0.0, 0.1));
    let high = brightness(span(&kick(r#","filterType":"highpass","filterCutoff":5000"#).left, 0.0, 0.1));
    assert!(dry > 1.2, "white noise under the master's 18.5 kHz cut: brightness {dry}");
    assert!(low < dry * 0.05, "lowpass at 500 Hz: brightness {low} from {dry}");
    assert!(high > dry * 1.2, "highpass at 5 kHz: brightness {high} from {dry}");
}

#[test]
fn the_compressor_divides_a_level_step_by_its_ratio() {
    // The last 0.2 s of each level of the tone: the detector has settled.
    let step = |wav: &Wav| {
        let first = rms(span(&wav.left, TONE_START + 0.2, TONE_START + TONE_SECONDS));
        let second = rms(span(&wav.left, TONE_START + TONE_SECONDS + 0.2, TONE_START + 2.0 * TONE_SECONDS));
        db(second / first)
    };
    let dry = step(&snare(""));
    // Twice the level (6.02 dB), less the sample's decay over 0.4 s (0.24 dB).
    assert!((dry - 5.78).abs() < 0.1, "the source doubles its level: {dry:.2} dB");
    for ratio in [2.0, 4.0] {
        let compressed = step(&snare(&format!(r#","compMix":1,"compThresholdDb":-60,"compRatio":{ratio}"#)));
        assert!((compressed - dry / ratio).abs() < 0.2, "ratio {ratio}: a step of {compressed:.2} dB");
    }
    // In parallel, half dry and half compressed: in between.
    let half = step(&snare(r#","compMix":0.5,"compThresholdDb":-60,"compRatio":4"#));
    assert!(half > dry / 4.0 + 0.5 && half < dry - 0.5, "half mix: a step of {half:.2} dB");
}

#[test]
fn a_compressor_above_the_signal_or_at_no_mix_changes_nothing() {
    let dry = snare("");
    for settings in [r#","compMix":1,"compThresholdDb":0,"compRatio":20"#, r#","compMix":0,"compThresholdDb":-60"#] {
        assert!(same(&snare(settings), &dry), "{settings}");
    }
}

#[test]
fn saturation_adds_odd_harmonics_and_its_led_turns_it_off() {
    let third = |wav: &Wav| {
        let tone = span(&wav.left, TONE_START + 0.1, TONE_START + 2.0 * TONE_SECONDS - 0.1);
        amplitude_at(tone, 3.0 * TONE_HZ) / amplitude_at(tone, TONE_HZ)
    };
    let dry = tom("");
    let clean = third(&dry);
    let hard = third(&tom(r#","saturationType":"hard","saturationMix":1"#));
    let soft = third(&tom(r#","saturationType":"soft","saturationMix":1"#));
    assert!(clean < 0.01, "a sine through the master: third harmonic at {clean}");
    assert!(hard > 0.1, "hard saturation: third harmonic at {hard}");
    assert!(soft > 0.03 && soft < hard, "soft saturation: third harmonic at {soft}");
    let off = tom(r#","saturationType":"hard","saturationMix":1,"saturationOn":false"#);
    assert!(same(&off, &dry), "its LED off: the file without the saturation");
}

#[test]
fn a_hard_pan_leaves_the_other_side_silent() {
    let left = kick(r#","pan":-1"#);
    assert!(energy(&left.left) > 1.0);
    assert!(energy(&left.right) < energy(&left.left) * 1e-6, "right {}", energy(&left.right));
}

#[test]
fn the_effects_of_one_track_stay_on_its_strip() {
    let everything = r#","reverbType":"hall","reverbMix":0.8,"delayType":"tape","delayMix":0.8,
        "filterType":"lowpass","filterCutoff":300,"compMix":1,"compThresholdDb":-60,"pan":1"#;
    let both = |kick: &str| render(&[("KICK", 0, kick), ("SNARE", 2, "")]);
    // The KICK muted with its effects on: the SNARE as it is alone.
    let muted = format!("{everything}{MUTED}");
    assert!(same(&both(&muted), &both(MUTED)));
    // Both playing: the strips add up through the master, linear at these
    // levels, so what the effects change in the mix is what they change in
    // the KICK alone, to the 16-bit rounding of the four files.
    let (mix_dry, mix_wet) = (both(""), both(everything));
    let (alone_dry, alone_wet) = (kick(""), kick(everything));
    let difference = |a: &[f32], b: &[f32]| -> Vec<f32> { a.iter().zip(b).map(|(x, y)| x - y).collect() };
    for side in [Side::Left, Side::Right] {
        let in_mix = difference(mix_wet.channel(side), mix_dry.channel(side));
        let alone = difference(alone_wet.channel(side), alone_dry.channel(side));
        let residual = energy(&difference(&in_mix, &alone));
        assert!(residual < energy(&alone) * 1e-4, "{side:?}: residual {residual} of {}", energy(&alone));
    }
}
