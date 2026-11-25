//! A song's automation lanes, end to end: a song whose lanes are read from
//! its JSON is rendered as the WAV export does (`offline::render_song`),
//! written to a 16-bit WAV file and read back, then its second measure,
//! where each lane starts, is measured against its first, where it sets
//! nothing.
//!
//! The song plays one 4-beat pattern at 120 bpm twice: a measure is 2 s, and
//! the lanes start on step 16, at 2 s. The sources are synthetic, below the
//! master bus's threshold: a noise burst of 50 ms (KICK), a 440 Hz sine of
//! 1.5 s (SNARE), a noise of 1.5 s (HAT), and a loud sine for the
//! saturation to bend (TOM).

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::sync::Arc;

use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::kit::{BankSample, KitSample};
use ordrumbox_engine::offline::{self, EXPORT_RATE, RenderSetup};
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets};

mod signals;
use signals::{RATE, Wav, amplitude_at, brightness, db, edge, energy, frame, rms, span, through_wav};

/// Where the second measure, and every lane, starts: seconds, and steps.
const MEASURE_SECONDS: f64 = 2.0;
const LANE_STEP: u32 = 16;
const BURST_SECONDS: f64 = 0.05;
const TONE_SECONDS: f64 = 1.5;
const TONE_HZ: f64 = 440.0;
const QUIET: f64 = 0.05;
const LOUD: f64 = 0.5;

/// Noise from a fixed linear congruential generator, `seconds` long at
/// `level`, with 1 ms fades.
fn noise(seconds: f64, level: f64) -> Vec<f32> {
    let frames = frame(seconds);
    let mut state: u32 = 0x1234_5678;
    (0..frames)
        .map(|i| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let white = f64::from(state >> 8) / f64::from(1u32 << 24) * 2.0 - 1.0;
            (white * level * edge(i, frames, frame(0.001))) as f32
        })
        .collect()
}

/// A sine of `TONE_HZ`, `TONE_SECONDS` long at `level`, with 2 ms fades.
fn tone(level: f64) -> Vec<f32> {
    let frames = frame(TONE_SECONDS);
    (0..frames)
        .map(|i| {
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
            // To -60 dB in 100 s: flat over a measure.
            decay_ms: Some(100_000.0),
            ..Default::default()
        },
        buffer: SampleBuffer::mono(buffer, EXPORT_RATE).into(),
    };
    RenderSetup {
        bank: Arc::new(SampleBank {
            samples: vec![
                sample("KICK", noise(BURST_SECONDS, QUIET)),
                sample("SNARE", tone(QUIET)),
                sample("HAT", noise(TONE_SECONDS, QUIET)),
                sample("TOM", tone(LOUD)),
            ],
        }),
        scales: Scales::default(),
        synths: SynthPresets::default(),
        kit: "t".into(),
        sample_rate: EXPORT_RATE,
        seed: 1,
        returns: Default::default(),
    }
}

/// A track of the pattern: its instrument, its notes and its settings (JSON
/// members, each after a comma).
type TrackSpec<'a> = (&'a str, &'a str, &'a str);

/// One note on beat 0.
const ON_THE_ONE: &str = r#"[{"beat":0,"beatStep":0}]"#;

/// A lane of `instrument` for `target`, at `value` from [`LANE_STEP`] on.
fn lane(instrument: &str, target: &str, value: f64) -> String {
    format!(r#"{{"instrument":"{instrument}","target":"{target}","points":[{{"step":{LANE_STEP},"value":{value}}}]}}"#)
}

/// The song of `tracks`, two measures of one pattern, with `lanes`,
/// rendered, written to a WAV file and read back.
fn render(tracks: &[TrackSpec<'_>], lanes: &[String]) -> Wav {
    let tracks: Vec<String> = tracks
        .iter()
        .map(|(name, notes, settings)| format!(r#"{{"name":"{name}","stepsPerBeat":4,"notes":{notes}{settings}}}"#))
        .collect();
    let text = format!(
        r#"{{"bpm":120,"patterns":[{{"id":"a","name":"A","bpm":120,"beatCount":4,"tracks":[{}]}}],
            "songs":[{{"id":"s","name":"S","bpm":120,"clips":[{{"pattern":"a","startMeasure":0,"measureCount":2}}],
            "automations":[{}]}}]}}"#,
        tracks.join(","),
        lanes.join(",")
    );
    let (library, report) = Library::parse(&text).expect("parses");
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    assert_eq!(library.songs[0].automations.len(), lanes.len(), "every lane read");
    let (left, right) = offline::render_song(&library, 0, &setup()).expect("renders");
    assert_eq!(left.len(), frame(2.0 * MEASURE_SECONDS), "two measures at 120 bpm");
    through_wav(&left, &right)
}

/// `signal` from `from` to `to` seconds into the first measure, and the same
/// span of the second.
fn measures(signal: &[f32], from: f64, to: f64) -> (&[f32], &[f32]) {
    (span(signal, from, to), span(signal, MEASURE_SECONDS + from, MEASURE_SECONDS + to))
}

/// The seconds into `signal`, from `from`, of its first sample over `level`.
fn onset(signal: &[f32], from: f64, level: f32) -> f64 {
    let start = frame(from);
    let at = signal[start..].iter().position(|s| s.abs() > level).expect("a sound");
    (start + at) as f64 / RATE
}

#[test]
fn lanes_of_an_instrument_the_song_lacks_change_nothing() {
    let tracks = [("SNARE", ON_THE_ONE, ""), ("KICK", ON_THE_ONE, "")];
    let plain = render(&tracks, &[]);
    let targets =
        ["mute", "volume", "pan", "cutoff", "q", "echoSend", "saturation", "transpose", "probability", "swing"];
    let lanes: Vec<String> = targets.iter().map(|target| lane("RIDE", target, 0.0)).collect();
    assert_eq!(render(&tracks, &lanes), plain, "the same file, sample for sample");
}

#[test]
fn a_mute_lane_silences_its_instrument_and_can_unmute_it() {
    let muted = render(&[("SNARE", ON_THE_ONE, "")], &[lane("SNARE", "mute", 1.0)]);
    let (first, second) = measures(&muted.left, 0.05, 1.45);
    assert!(energy(first) > 1.0, "heard before the lane: {}", energy(first));
    assert!(energy(second) < energy(first) * 1e-6, "silent from its step: {}", energy(second));

    let unmuted = render(&[("SNARE", ON_THE_ONE, r#","mute":true"#)], &[lane("SNARE", "mute", 0.0)]);
    let (first, second) = measures(&unmuted.left, 0.05, 1.45);
    assert!(energy(first) < 1e-6 && energy(second) > 1.0, "muted in its pattern, the lane switches it on");
}

#[test]
fn a_solo_lane_silences_the_other_instruments() {
    // The KICK plays at 1.625 s, once the SNARE (1.5 s long) is over.
    let kick_late = r#"[{"beat":3,"beatStep":1}]"#;
    let solo = render(&[("SNARE", ON_THE_ONE, ""), ("KICK", kick_late, "")], &[lane("SNARE", "solo", 1.0)]);
    let (first, second) = measures(&solo.left, 1.625, 1.625 + BURST_SECONDS);
    let (snare_first, snare_second) = measures(&solo.left, 0.05, 0.9);
    assert!(energy(second) < energy(first) * 1e-3, "the KICK silenced: {} of {}", energy(second), energy(first));
    let change = db(rms(snare_second) / rms(snare_first));
    assert!(change.abs() < 0.1, "the SNARE soloed plays on: {change:.2} dB");
}

#[test]
fn a_volume_lane_sets_the_level_in_decibels() {
    // A level of 0.5 is -15 dB (`volume_db`).
    let quieter = render(&[("SNARE", ON_THE_ONE, "")], &[lane("SNARE", "volume", 0.5)]);
    let (first, second) = measures(&quieter.left, 0.05, 1.45);
    let change = db(rms(second) / rms(first));
    assert!((change + 15.0).abs() < 0.2, "-15 dB: {change:.2} dB");
}

#[test]
fn a_pan_lane_moves_the_sound_to_one_side() {
    // A SNARE's default pan is off centre: centred here.
    let left = render(&[("SNARE", ON_THE_ONE, r#","pan":0"#)], &[lane("SNARE", "pan", 0.0)]);
    let (first_left, second_left) = measures(&left.left, 0.05, 1.45);
    let (first_right, second_right) = measures(&left.right, 0.05, 1.45);
    assert!(db(rms(first_left) / rms(first_right)).abs() < 0.01, "centred before the lane");
    assert!(energy(second_right) < energy(second_left) * 1e-6, "all to the left: right {}", energy(second_right));
}

#[test]
fn a_cutoff_lane_filters_a_track_whose_filter_is_off() {
    // 0.25 of the cutoff's travel: about 112 Hz, on a noise.
    let filtered = render(&[("HAT", ON_THE_ONE, "")], &[lane("HAT", "cutoff", 0.25)]);
    let (first, second) = measures(&filtered.left, 0.05, 1.45);
    let (open, closed) = (brightness(first), brightness(second));
    assert!(open > 1.0, "white noise before the lane: {open}");
    assert!(closed < open * 0.01, "the highs taken off: {closed} of {open}");
}

#[test]
fn a_transpose_lane_moves_the_pitch_by_whole_semitones() {
    // 0.75 of -24 to +24: an octave up, the sine at 880 Hz, played twice as fast.
    let up = render(&[("SNARE", ON_THE_ONE, "")], &[lane("SNARE", "transpose", 0.75)]);
    let (first, second) = measures(&up.left, 0.05, 0.7);
    assert!(amplitude_at(first, TONE_HZ) > 10.0 * amplitude_at(first, 2.0 * TONE_HZ), "440 Hz before the lane");
    assert!(amplitude_at(second, 2.0 * TONE_HZ) > 10.0 * amplitude_at(second, TONE_HZ), "880 Hz from its step");
    let (_, after) = measures(&up.left, 0.8, 1.45);
    assert!(energy(after) < energy(second) * 1e-4, "half as long: over by 0.75 s");
}

#[test]
fn a_probability_lane_decides_whether_the_notes_play() {
    let every_beat = r#"[{"beat":0},{"beat":1},{"beat":2},{"beat":3}]"#;
    let never = render(&[("KICK", every_beat, "")], &[lane("KICK", "probability", 0.0)]);
    let (first, second) = measures(&never.left, 0.0, MEASURE_SECONDS);
    assert!(energy(first) > 1.0 && energy(second) < energy(first) * 1e-6, "no note at a probability of 0");

    // The lane replaces the track's probability: a track that never plays plays every note.
    let always = render(&[("KICK", every_beat, r#","probability":0"#)], &[lane("KICK", "probability", 1.0)]);
    let (first, second) = measures(&always.left, 0.0, MEASURE_SECONDS);
    assert!(energy(first) < 1e-6, "the track's probability of 0 before the lane");
    for beat in 0..4 {
        let at = MEASURE_SECONDS + f64::from(beat) * 0.5;
        assert!(energy(span(&always.left, at, at + BURST_SECONDS)) > 0.1, "beat {beat} plays");
    }
    assert!((energy(second) - energy(measures(&never.left, 0.0, MEASURE_SECONDS).0)).abs() < 1e-3);
}

#[test]
fn a_swing_lane_delays_the_second_step_of_a_pair() {
    // A note on the second sixteenth (0.125 s), on a straight track; 0.25 beat
    // of swing is 0.125 s at 120 bpm.
    let off_beat = r#"[{"beat":0,"beatStep":1}]"#;
    let swung = render(&[("KICK", off_beat, "")], &[lane("KICK", "swing", 0.25)]);
    let level = (QUIET / 10.0) as f32;
    let straight = onset(&swung.left, 0.0, level);
    let late = onset(&swung.left, MEASURE_SECONDS, level) - MEASURE_SECONDS;
    assert!((straight - 0.125).abs() < 0.002, "on its step before the lane: {straight:.4} s");
    assert!((late - 0.25).abs() < 0.002, "0.125 s late from its step: {late:.4} s");
}

#[test]
fn an_echo_send_lane_feeds_the_shared_echo() {
    // The echo repeats 0.75 beat later: 0.375 s.
    let sent = render(&[("KICK", ON_THE_ONE, "")], &[lane("KICK", "echoSend", 1.0)]);
    let (burst, _) = measures(&sent.left, 0.0, BURST_SECONDS);
    let (dry, echo) = measures(&sent.left, 0.3, 0.6);
    assert!(energy(dry) < energy(burst) * 1e-6, "no echo before the lane: {}", energy(dry));
    assert!(energy(echo) > energy(burst) * 0.01, "an echo from its step: {} of {}", energy(echo), energy(burst));
}

#[test]
fn a_saturation_lane_adds_odd_harmonics() {
    let bent = render(&[("TOM", ON_THE_ONE, "")], &[lane("TOM", "saturation", 1.0)]);
    let (first, second) = measures(&bent.left, 0.05, 1.45);
    let third = |signal: &[f32]| amplitude_at(signal, 3.0 * TONE_HZ) / amplitude_at(signal, TONE_HZ);
    assert!(third(second) > 10.0 * third(first), "third harmonic: {} from {}", third(second), third(first));
}
