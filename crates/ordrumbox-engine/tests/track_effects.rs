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
use ordrumbox_engine::{SampleBank, SampleBuffer, SynthPresets};

mod signals;
use signals::{RATE, Wav, amplitude_at, brightness, db, edge, energy, frame, rms, span, through_wav};
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
            key: Some(key.into()),
            display_name: key.to_lowercase(),
            path: format!("t/{key}.wav"),
            // To -60 dB in 100 s: 0.24 dB over a level of the tone.
            decay_ms: Some(100_000.0),
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
        returns: Default::default(),
    }
}

#[derive(Debug, Clone, Copy)]
enum Side {
    Left,
    Right,
}

/// The channel `side` of `wav`.
fn channel(wav: &Wav, side: Side) -> &[f32] {
    match side {
        Side::Left => &wav.left,
        Side::Right => &wav.right,
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
    render_library(&text)
}

/// The library `text`'s first pattern, rendered once, written to a WAV file
/// and read back.
fn render_library(text: &str) -> Wav {
    let (library, report) = Library::parse(text).expect("parses");
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    let (left, right) = offline::render_pattern(&library, 0, 1, &setup(), None).expect("renders");
    assert_eq!(left.len(), 2 * EXPORT_RATE as usize, "4 beats at 120 bpm");
    through_wav(&left, &right)
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

/// The KICK with `settings`, played by two notes, on beats 0 and 1, `first`
/// and `second` given as JSON members of the notes (their locks).
fn locked_kick(settings: &str, first: &str, second: &str) -> Wav {
    let text = format!(
        r#"{{"bpm":120,"patterns":[{{"id":"fx","name":"Fx","bpm":120,"beatCount":4,"tracks":[
            {{"name":"KICK","stepsPerBeat":4,"notes":[{{"beat":0{first}}},{{"beat":1{second}}}]{settings}}}]}}]}}"#
    );
    render_library(&text)
}

#[test]
fn a_filter_lock_closes_the_filter_for_its_note_alone() {
    // The track has no filter (allpass): the lock turns a lowpass on, and the
    // next note, unlocked, is the open strip again.
    let wav = locked_kick("", r#","filterLock":0.2"#, "");
    let open = locked_kick("", "", "");
    let (locked, plain) = (brightness(span(&wav.left, 0.0, 0.1)), brightness(span(&open.left, 0.0, 0.1)));
    assert!(locked < plain * 0.1, "the first note is dull: brightness {locked} from {plain}");
    let (next, plain) = (brightness(span(&wav.left, 0.5, 0.6)), brightness(span(&open.left, 0.5, 0.6)));
    assert!((next / plain - 1.0).abs() < 0.2, "the second note is as bright as ever: {next} from {plain}");
}

#[test]
fn a_reverb_lock_gives_a_tail_to_its_note_alone() {
    // The track has no reverb (none): the lock turns a room on, for the first note.
    let wav = locked_kick("", r#","reverbLock":0.8"#, "");
    let dry = locked_kick("", "", "");
    let tail = |w: &Wav, from| energy(span(&w.left, from, from + 0.2));
    let sound = energy(span(&dry.left, 0.0, 0.1));
    assert!(tail(&wav, 0.06) > sound * 0.05, "a tail after the locked note: {} of {sound}", tail(&wav, 0.06));
    assert!(tail(&dry, 0.06) < sound * 1e-4, "none without the lock");
    // The second note (beat 1, at 0.5 s) is dry again: its own tail is the burst's alone.
    assert!(tail(&wav, 0.56) < sound * 1e-4 + tail(&dry, 0.56), "none after the unlocked note");
}

#[test]
fn a_delay_lock_repeats_its_note_alone() {
    let settings = r#","delayType":"slap","delayBeats":0.5,"delayMix":0.0"#;
    let wav = locked_kick(settings, r#","delayLock":0.8"#, "");
    let plain = locked_kick(settings, "", "");
    let echo = |w: &Wav, from| energy(span(&w.left, from, from + 0.05));
    assert!(echo(&wav, 0.25) > echo(&plain, 0.25) * 10.0 + 1e-9, "the locked note echoes");
    assert!(echo(&wav, 0.75) < 1e-6 + echo(&plain, 0.75) * 2.0, "the unlocked one does not");
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
        let in_mix = difference(channel(&mix_wet, side), channel(&mix_dry, side));
        let alone = difference(channel(&alone_wet, side), channel(&alone_dry, side));
        let residual = energy(&difference(&in_mix, &alone));
        assert!(residual < energy(&alone) * 1e-4, "{side:?}: residual {residual} of {}", energy(&alone));
    }
}
