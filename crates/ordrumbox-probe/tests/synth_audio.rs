//! The synth voice judged by what it sounds like: notes rendered with the real
//! voice, then measured (spectrum, pitch, level over time) and compared with
//! what the settings say they must be. No snapshot: a snapshot only says that
//! nothing changed, these say the sound is right.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::f64::consts::PI;
use std::path::Path;

use std::sync::Arc;

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Library, Scales};
use ordrumbox_dsp::{SampleBuffer, SynthVoice};
use ordrumbox_engine::analysis::analyze;
use ordrumbox_engine::offline::{RenderSetup, render_pattern};
use ordrumbox_engine::synth_preset::{SynthNote, synth_patch};
use ordrumbox_engine::{SampleBank, SynthPresets, wav};
use ordrumbox_probe::fft::power_spectrum;
use ordrumbox_probe::render::{NoteSettings, render_note, repeat_difference_db};

const RATE: f64 = 48_000.0;
/// The pitch of ratio 1, Hz.
const C3: f64 = 130.812_782_650_299_3;
/// Frames of the spectra: 0.34 s, 2.9 Hz a bin.
const WINDOW: usize = 16_384;
/// Where the steady part of a held note starts, frames.
const STEADY_FROM: usize = 4_800;
/// A note held long enough for a [`WINDOW`] of steady sound.
const GATE: f64 = 0.6;

// ─── Sounds ──────────────────────────────────────────────────────────────────

const OPEN: &str = r#""bypassFilter":true"#;
/// An envelope that holds its level from the attack to the release.
const FLAT: &str = r#","envelope":{"attack":0.005,"decay":0.1,"sustain":1,"release":0.05}"#;

/// One oscillator of `wave` at full gain, through `filter` (a JSON member:
/// [`OPEN`] or a `"filter"` group), with `extra` members (each starting with a comma).
fn tone(wave: &str, filter: &str, extra: &str) -> Value {
    let text = format!(
        r#"{{"masterVolume":1,"vco1":{{"gain":1,"wave":"{wave}"}},"vco2":{{"gain":0}},"vco3":{{"gain":0}},{filter}{extra}}}"#
    );
    json::parse(&text).unwrap_or_else(|e| panic!("{e}: {text}"))
}

fn filter(kind: &str, freq: f64, q: f64, slope: u32) -> String {
    format!(r#""bypassFilter":false,"filter":{{"type":"{kind}","freq":{freq},"Q":{q},"slope":{slope}}}"#)
}

fn at(ratio: f64, velocity: f64, gate: f64) -> NoteSettings {
    NoteSettings { ratio, velocity, gate_seconds: Some(gate), ..NoteSettings::default() }
}

fn render(preset: &Value, settings: &NoteSettings) -> SampleBuffer {
    render_note(preset, settings).expect("the note ends")
}

fn left(preset: &Value, ratio: f64) -> Vec<f64> {
    render(preset, &at(ratio, 0.8, GATE)).left.iter().map(|&s| f64::from(s)).collect()
}

// ─── Measures ────────────────────────────────────────────────────────────────

fn db(power: f64) -> f64 {
    10.0 * power.max(1e-30).log10()
}

fn spectrum(samples: &[f64]) -> Vec<f64> {
    power_spectrum(&samples[STEADY_FROM..STEADY_FROM + WINDOW], WINDOW)
}

/// The level, dB, of the strongest bin within two of `hz`.
fn level(power: &[f64], hz: f64) -> f64 {
    let bin = (hz * WINDOW as f64 / RATE).round() as usize;
    db(power[bin.saturating_sub(2)..=(bin + 2).min(power.len() - 1)].iter().fold(0.0, |m, &p| m.max(p)))
}

/// Level of harmonic `k` of a note at `ratio`.
fn harmonic(power: &[f64], ratio: f64, k: usize) -> f64 {
    level(power, C3 * ratio * k as f64)
}

fn rms(samples: &[f64]) -> f64 {
    (samples.iter().map(|s| s * s).sum::<f64>() / samples.len().max(1) as f64).sqrt()
}

/// The level of `samples` in windows of `seconds`, each at its start.
fn rms_series(samples: &[f64], seconds: f64) -> Vec<f64> {
    samples.chunks_exact((seconds * RATE) as usize).map(rms).collect()
}

fn correlation(a: &[f64], b: &[f64]) -> f64 {
    let (mean_a, mean_b) = (a.iter().sum::<f64>() / a.len() as f64, b.iter().sum::<f64>() / b.len() as f64);
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        ab += (x - mean_a) * (y - mean_b);
        aa += (x - mean_a).powi(2);
        bb += (y - mean_b).powi(2);
    }
    ab / (aa * bb).sqrt()
}

/// How many times `series` crosses its own mean.
fn mean_crossings(series: &[f64]) -> usize {
    let mean = series.iter().sum::<f64>() / series.len() as f64;
    series.windows(2).filter(|w| (w[0] - mean) * (w[1] - mean) < 0.0).count()
}

/// The frequency of each cycle of a tone, Hz, and when it starts: from the
/// upward zero crossings, placed between two samples.
fn cycles(samples: &[f64]) -> Vec<(f64, f64)> {
    let crossings: Vec<f64> = samples
        .windows(2)
        .enumerate()
        .filter(|(_, w)| w[0] < 0.0 && w[1] >= 0.0)
        .map(|(i, w)| (i as f64 + w[0] / (w[0] - w[1])) / RATE)
        .collect();
    crossings.windows(2).map(|w| (w[0], 1.0 / (w[1] - w[0]))).collect()
}

// ─── Every preset ────────────────────────────────────────────────────────────

fn factory_presets() -> SynthPresets {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/data/synth_presets.json");
    let text = std::fs::read_to_string(path).expect("synth_presets.json");
    SynthPresets::from_json(&json::parse(&text).expect("parses")).expect("an object")
}

#[test]
fn every_factory_preset_plays_a_sane_note_at_every_pitch() {
    let presets = factory_presets();
    assert!(presets.len() > 20);
    for key in presets.keys() {
        let preset = presets.get(key).unwrap();
        for ratio in [0.5, 1.0, 4.0] {
            let note = render(preset, &NoteSettings { ratio, ..NoteSettings::default() });
            let left = &note.left;
            let what = format!("{key} at ratio {ratio}");
            assert!(left.iter().all(|s| s.is_finite()), "{what}: not a number");
            let peak = left.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            assert!(peak > 1e-3, "{what}: silent ({peak})");
            assert!(peak < 4.0, "{what}: peak {peak}");
            assert!(left[left.len() - 8..].iter().all(|s| s.abs() < 1e-3), "{what}: does not end in silence");
        }
    }
}

#[test]
fn a_note_is_the_same_every_time_it_is_rendered() {
    let presets = factory_presets();
    for key in presets.keys() {
        let preset = presets.get(key).unwrap();
        let settings = NoteSettings::default();
        assert_eq!(render(preset, &settings), render(preset, &settings), "{key}");
    }
}

/// `preset` rendered in blocks of `block` frames, for `frames`.
fn in_blocks(preset: &Value, block: usize, frames: usize) -> Vec<f32> {
    let note = SynthNote { ratio: 1.0, velocity: 0.8, pan: 0.0, bpm: 120.0 };
    let mut voice = SynthVoice::default();
    voice.start(&synth_patch(preset, note), 0, 0.0, 1);
    voice.release_at(0.125);
    let (mut out, mut right) = (vec![0.0; frames], vec![0.0; frames]);
    for from in (0..frames).step_by(block) {
        let to = (from + block).min(frames);
        voice.render(RATE as f32, from as u64, &mut out[from..to], &mut right[from..to]);
    }
    out
}

#[test]
fn the_size_of_the_blocks_does_not_change_the_sound() {
    let presets = factory_presets();
    for key in presets.keys() {
        let preset = presets.get(key).unwrap();
        let whole = in_blocks(preset, 24_000, 24_000);
        for block in [1, 61, 480] {
            assert_eq!(in_blocks(preset, block, 24_000), whole, "{key} in blocks of {block}");
        }
    }
}

// ─── Pitch ───────────────────────────────────────────────────────────────────

fn assert_pitch(preset: &Value, ratio: f64, expected_hz: f64, what: &str) {
    let note = render(preset, &at(ratio, 0.8, GATE));
    let hz = analyze(&note).fundamental_hz.unwrap_or_else(|| panic!("{what}: no pitch"));
    let cents = 1200.0 * (hz / expected_hz).log2();
    assert!(cents.abs() < 6.0, "{what}: {hz} Hz instead of {expected_hz} Hz ({cents:.1} cents)");
}

#[test]
fn a_note_sounds_at_its_pitch() {
    for wave in ["sine", "sawtooth", "square", "triangle"] {
        for ratio in [0.5, 1.0, 2.0, 4.0] {
            assert_pitch(&tone(wave, OPEN, FLAT), ratio, C3 * ratio, &format!("{wave} at ratio {ratio}"));
        }
    }
}

#[test]
fn octave_and_detune_move_the_pitch() {
    let moved = |vco: &str| {
        json::parse(&format!(r#"{{"masterVolume":1,"vco1":{{"gain":1,"wave":"sine",{vco}}},{OPEN}{FLAT}}}"#)).unwrap()
    };
    assert_pitch(&moved(r#""octave":1"#), 1.0, C3 * 2.0, "an octave up");
    assert_pitch(&moved(r#""octave":-1"#), 1.0, C3 / 2.0, "an octave down");
    assert_pitch(&moved(r#""detune":100"#), 1.0, C3 * 2.0_f64.powf(1.0 / 12.0), "a semitone up");
    assert_pitch(&moved(r#""detune":-50"#), 1.0, C3 * 2.0_f64.powf(-0.5 / 12.0), "a quarter tone down");
}

// ─── Waves ───────────────────────────────────────────────────────────────────

/// Level of harmonic `k` against the fundamental's, dB.
fn relative(power: &[f64], k: usize) -> f64 {
    harmonic(power, 1.0, k) - harmonic(power, 1.0, 1)
}

#[test]
fn each_wave_has_its_harmonics() {
    let spectrum_of = |wave| spectrum(&left(&tone(wave, OPEN, FLAT), 1.0));
    let saw = spectrum_of("sawtooth");
    for k in 2..=6 {
        let expected = -20.0 * (k as f64).log10();
        assert!((relative(&saw, k) - expected).abs() < 3.0, "saw harmonic {k}: {} dB", relative(&saw, k));
    }
    let square = spectrum_of("square");
    for k in [2, 4, 6] {
        assert!(relative(&square, k) < -35.0, "square harmonic {k}: {} dB", relative(&square, k));
    }
    assert!((relative(&square, 3) + 9.54).abs() < 3.0, "square harmonic 3: {} dB", relative(&square, 3));
    let triangle = spectrum_of("triangle");
    assert!((relative(&triangle, 3) + 19.08).abs() < 3.0, "triangle harmonic 3: {} dB", relative(&triangle, 3));
    assert!(relative(&triangle, 2) < -35.0, "triangle harmonic 2: {} dB", relative(&triangle, 2));
    let sine = spectrum_of("sine");
    for k in [2, 3, 5] {
        assert!(relative(&sine, k) < -50.0, "sine harmonic {k}: {} dB", relative(&sine, k));
    }
}

/// Share of the energy of `samples`, dB, that is not within a few bins of a
/// harmonic of `fundamental` Hz: what a band-limited wave does not have.
fn off_harmonic_db(samples: &[f64], fundamental: f64) -> f64 {
    let power = spectrum(samples);
    let bin_hz = RATE / WINDOW as f64;
    let near_a_harmonic = |bin: usize| {
        (1..=((RATE / 2.0 / fundamental) as u32 + 1))
            .any(|k| (bin as f64 * bin_hz - fundamental * f64::from(k)).abs() < 12.0 * bin_hz)
    };
    let (mut between, mut total) = (0.0, 0.0);
    for (bin, &p) in power.iter().enumerate().skip((50.0 / bin_hz) as usize) {
        total += p;
        if !near_a_harmonic(bin) {
            between += p;
        }
    }
    db(between / total)
}

#[test]
fn a_saw_aliases_far_less_than_a_naive_one() {
    // PolyBLEP is no oversampling: near the sample rate's half it still
    // aliases a little. It has to stay well under a saw that is not corrected.
    for ratio in [8.0, 32.0] {
        let fundamental = C3 * ratio;
        let naive: Vec<f64> =
            (0..STEADY_FROM + WINDOW).map(|i| 2.0 * (fundamental * i as f64 / RATE).fract() - 1.0).collect();
        let voice = left(&tone("sawtooth", OPEN, FLAT), ratio);
        let (naive_db, voice_db) = (off_harmonic_db(&naive, fundamental), off_harmonic_db(&voice, fundamental));
        assert!(
            voice_db < naive_db - 10.0,
            "ratio {ratio}: {voice_db:.1} dB off the harmonics, a naive saw {naive_db:.1}"
        );
    }
}

// ─── Filter ──────────────────────────────────────────────────────────────────

/// The gain of a lowpass of `poles` (2 or 4, Butterworth at Q = 1/√2) at
/// `hz`, dB, with the bilinear transform's warping of the frequency.
fn lowpass_db(hz: f64, cutoff: f64, poles: i32) -> f64 {
    let r = (PI * hz / RATE).tan() / (PI * cutoff / RATE).tan();
    -10.0 * (1.0 + r.powi(2 * poles)).log10()
}

const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// What a saw at ratio 1 loses at harmonic `k` through `filter_member`, dB.
fn loss_at(filter_member: &str, k: usize) -> f64 {
    let open = spectrum(&left(&tone("sawtooth", OPEN, FLAT), 1.0));
    let filtered = spectrum(&left(&tone("sawtooth", filter_member, FLAT), 1.0));
    harmonic(&filtered, 1.0, k) - harmonic(&open, 1.0, k)
}

#[test]
fn a_lowpass_falls_at_its_slope() {
    for (slope, poles) in [(12, 2), (24, 4)] {
        let member = filter("lowpass", 1000.0, BUTTERWORTH_Q, slope);
        for k in [4, 15, 31] {
            let hz = C3 * k as f64;
            let expected = lowpass_db(hz, 1000.0, poles);
            let loss = loss_at(&member, k);
            assert!(
                (loss - expected).abs() < 2.5,
                "{slope} dB/oct at {hz:.0} Hz: {loss:.1} dB, expected {expected:.1}"
            );
        }
    }
}

#[test]
fn a_highpass_cuts_below_its_cutoff() {
    let member = filter("highpass", 1000.0, BUTTERWORTH_Q, 12);
    assert!(loss_at(&member, 1) < -30.0, "a fundamental under it: {} dB", loss_at(&member, 1));
    assert!(loss_at(&member, 31).abs() < 1.5, "a harmonic well over it: {} dB", loss_at(&member, 31));
}

#[test]
fn a_notch_removes_its_frequency_and_leaves_the_rest() {
    // The tenth harmonic of ratio 1.
    let member = filter("notch", C3 * 10.0, 5.0, 12);
    assert!(loss_at(&member, 10) < -20.0, "at the notch: {} dB", loss_at(&member, 10));
    assert!(loss_at(&member, 3).abs() < 2.0, "away from it: {} dB", loss_at(&member, 3));
    assert!(loss_at(&member, 30).abs() < 2.0, "away from it: {} dB", loss_at(&member, 30));
}

#[test]
fn a_bandpass_lets_its_band_through() {
    let member = filter("bandpass", C3 * 10.0, 5.0, 12);
    let (band, below, above) = (loss_at(&member, 10), loss_at(&member, 2), loss_at(&member, 40));
    assert!(band > below + 15.0 && band > above + 15.0, "{below} dB, {band} dB, {above} dB");
}

#[test]
fn resonance_raises_the_cutoff() {
    let member = filter("lowpass", C3 * 10.0, 10.0, 12);
    // At the cutoff a second-order lowpass reads Q: 20 dB at Q = 10.
    assert!(loss_at(&member, 10) > 14.0, "{} dB", loss_at(&member, 10));
}

// ─── Envelope ────────────────────────────────────────────────────────────────

#[test]
fn the_envelope_attacks_decays_sustains_and_releases() {
    let envelope = r#","envelope":{"attack":0.2,"decay":0.2,"sustain":0.5,"release":0.3}"#;
    let note = render(&tone("sine", OPEN, envelope), &at(4.0, 0.8, 1.0));
    let samples: Vec<f64> = note.left.iter().map(|&s| f64::from(s)).collect();
    let series = rms_series(&samples, 0.01);
    let (peak_index, peak) = series.iter().enumerate().fold((0, 0.0), |m, (i, &r)| if r > m.1 { (i, r) } else { m });
    let peak_seconds = peak_index as f64 * 0.01;
    assert!((peak_seconds - 0.2).abs() < 0.03, "peaks at {peak_seconds} s, not at the end of the attack");
    let sustained = series[70] / peak;
    assert!((sustained - 0.5).abs() < 0.05, "sustains at {sustained} of the peak");
    // Half-way through the release a cubic fall is at an eighth.
    let released = series[115] / series[99];
    assert!((released - 0.125).abs() < 0.05, "{released} of the sustain half-way through the release");
    let seconds = samples.len() as f64 / RATE;
    assert!((seconds - 1.3).abs() < 0.01, "the note lasts {seconds} s, not its gate and release");
}

// ─── LFO ─────────────────────────────────────────────────────────────────────

#[test]
fn an_lfo_on_the_pitch_is_a_vibrato_at_its_rate_and_depth() {
    let lfo = format!(r#"{FLAT},"lfo":{{"target":"VCO1","wave":"sine","freq":5,"depth":0.1}}"#);
    let note = render(&tone("sine", OPEN, &lfo), &at(4.0, 0.8, 1.0));
    let samples: Vec<f64> = note.left.iter().map(|&s| f64::from(s)).collect();
    let steady: Vec<f64> =
        cycles(&samples).into_iter().filter(|(start, _)| (0.05..1.0).contains(start)).map(|(_, hz)| hz).collect();
    let (low, high) = steady.iter().fold((f64::MAX, 0.0f64), |(l, h), &hz| (l.min(hz), h.max(hz)));
    // A depth of 0.1 swings 100 cents each way.
    let swing = 1200.0 * (high / low).log2();
    assert!((swing - 200.0).abs() < 20.0, "swings {swing:.0} cents, not 200");
    // Two crossings of the mean a cycle, 5 cycles a second, 0.95 s.
    let crossings = mean_crossings(&steady);
    assert!((8..=11).contains(&crossings), "{crossings} crossings: not 5 Hz");
}

// ─── Unison ──────────────────────────────────────────────────────────────────

/// How many lines the spectrum has within 150 cents of the fundamental of
/// ratio `ratio`, each within 20 dB of the strongest.
fn lines_near_the_pitch(power: &[f64], ratio: f64) -> usize {
    let bin_hz = RATE / WINDOW as f64;
    let from = (C3 * ratio * 2.0_f64.powf(-150.0 / 1200.0) / bin_hz) as usize;
    let to = (C3 * ratio * 2.0_f64.powf(150.0 / 1200.0) / bin_hz) as usize + 1;
    let top = power[from..to].iter().fold(0.0f64, |m, &p| m.max(p));
    (from + 3..to - 3)
        .filter(|&i| power[i] > top / 100.0 && (i - 3..=i + 3).all(|j| j == i || power[j] < power[i]))
        .count()
}

#[test]
fn unison_layers_are_lines_apart_and_the_spread_is_stereo() {
    let preset = |unison: &str| tone("sawtooth", OPEN, &format!("{FLAT},\"unison\":{unison}"));
    let single = lines_near_the_pitch(&spectrum(&left(&preset(r#"{"count":1,"detune":100,"spread":0}"#), 4.0)), 4.0);
    let five = lines_near_the_pitch(&spectrum(&left(&preset(r#"{"count":5,"detune":100,"spread":0}"#), 4.0)), 4.0);
    assert_eq!(single, 1);
    assert!(five >= 4, "{five} lines for 5 layers");

    let centred = render(&preset(r#"{"count":5,"detune":30,"spread":0}"#), &at(1.0, 0.8, GATE));
    assert_eq!(Some(&centred.left), centred.right.as_ref(), "no spread: the same on both sides");
    let wide = render(&preset(r#"{"count":5,"detune":30,"spread":1}"#), &at(1.0, 0.8, GATE));
    let to_f64 = |side: &[f32]| side.iter().map(|&s| f64::from(s)).collect::<Vec<_>>();
    let (l, r) = (to_f64(&wide.left), to_f64(wide.right.as_ref().unwrap()));
    assert!(correlation(&l, &r) < 0.95, "the sides are alike: {}", correlation(&l, &r));
    assert!((db(rms(&l).powi(2)) - db(rms(&r).powi(2))).abs() < 3.0, "the sides differ in level");
}

// ─── Modulation matrix ───────────────────────────────────────────────────────

fn with_matrix(wave: &str, filter_member: &str, extra: &str, line: &str) -> Value {
    tone(wave, filter_member, &format!("{extra},\"matrix\":{{{line}}}"))
}

fn line(source: &str, target: &str, amount: f64) -> String {
    format!(r#""source1":"{source}","target1":"{target}","amount1":{amount}"#)
}

/// What a lowpass of 2 poles gains, dB, when its cutoff goes from `from` to `to`, at `hz`.
fn opening_db(hz: f64, from: f64, to: f64) -> f64 {
    lowpass_db(hz, to, 2) - lowpass_db(hz, from, 2)
}

/// What `matrixed` has over `plain` at harmonic `k` of a note, dB.
fn gain_over(matrixed: &Value, plain: &Value, ratio: f64, velocity: f64, k: usize) -> f64 {
    let level_of = |preset: &Value| {
        let note = render(preset, &at(ratio, velocity, GATE));
        harmonic(&spectrum(&note.left.iter().map(|&s| f64::from(s)).collect::<Vec<_>>()), ratio, k)
    };
    level_of(matrixed) - level_of(plain)
}

#[test]
fn velocity_can_open_the_filter() {
    let low = filter("lowpass", 500.0, BUTTERWORTH_Q, 12);
    let plain = tone("sawtooth", &low, FLAT);
    // A velocity of 1 adds 1000 Hz of cutoff, 0.2 adds 200.
    let matrixed = with_matrix("sawtooth", &low, FLAT, &line("velocity", "filter.freq", 1.0));
    let hz = C3 * 15.0;
    for (velocity, cutoff) in [(1.0, 1500.0), (0.2, 700.0)] {
        let gain = gain_over(&matrixed, &plain, 1.0, velocity, 15);
        let expected = opening_db(hz, 500.0, cutoff);
        assert!(
            (gain - expected).abs() < 2.5,
            "velocity {velocity}: {gain:.1} dB at {hz:.0} Hz, expected {expected:.1}"
        );
    }
}

#[test]
fn the_key_can_open_the_filter() {
    let low = filter("lowpass", 500.0, BUTTERWORTH_Q, 12);
    let plain = tone("sawtooth", &low, FLAT);
    let matrixed = with_matrix("sawtooth", &low, FLAT, &line("key", "filter.freq", 1.0));
    // An octave up is half of the source: 500 Hz more of cutoff.
    let hz = C3 * 2.0 * 8.0;
    let gain = gain_over(&matrixed, &plain, 2.0, 0.8, 8);
    let expected = opening_db(hz, 500.0, 1000.0);
    assert!((gain - expected).abs() < 2.5, "{gain:.1} dB at {hz:.0} Hz, expected {expected:.1}");
    // An octave down closes it by as much: 500 Hz less, to the floor.
    assert!(gain_over(&matrixed, &plain, 0.5, 0.8, 16) < -3.0);
}

#[test]
fn an_lfo_through_the_matrix_swings_the_volume_by_its_amount() {
    let lfo = format!(r#"{FLAT},"lfo":{{"target":"NOT","wave":"sine","freq":4,"depth":0}}"#);
    let series_of = |amount: f64| {
        let note = render(&with_matrix("sine", OPEN, &lfo, &line("lfo1", "masterVolume", amount)), &at(4.0, 0.8, 1.0));
        let samples: Vec<f64> = note.left.iter().map(|&s| f64::from(s)).collect();
        rms_series(&samples[2_400..], 0.02)[..45].to_vec()
    };
    let down = series_of(-0.5);
    let (low, high) = down.iter().fold((f64::MAX, 0.0f64), |(l, h), &r| (l.min(r), h.max(r)));
    // The volume of 1 swings between 0.5 and 1.5.
    assert!((high / low - 3.0).abs() < 0.6, "swings {} times", high / low);
    // 4 cycles a second over 0.9 s.
    let crossings = mean_crossings(&down);
    assert!((6..=9).contains(&crossings), "{crossings} crossings: not 4 Hz");
    // A positive amount is the same swing the other way round.
    assert!(correlation(&down, &series_of(0.5)) < -0.95);
    let shallow = series_of(-0.25);
    let (low, high) = shallow.iter().fold((f64::MAX, 0.0f64), |(l, h), &r| (l.min(r), h.max(r)));
    assert!((high / low - 5.0 / 3.0).abs() < 0.3, "a quarter swings {} times", high / low);
}

#[test]
fn the_envelopes_through_the_matrix_close_the_volume_while_they_hold() {
    let sustained = |source: &str, extra: &str| {
        let note = render(&with_matrix("sine", OPEN, extra, &line(source, "masterVolume", -1.0)), &at(4.0, 0.8, 1.0));
        rms(&note.left[12_000..36_000].iter().map(|&s| f64::from(s)).collect::<Vec<_>>())
    };
    let open = rms(&left(&tone("sine", OPEN, FLAT), 4.0)[12_000..24_000]);
    // The envelope holds at 1: the volume of 1 falls to 0.
    assert!(sustained("env", FLAT) < open * 0.01, "{}", sustained("env", FLAT));
    let mod_env = format!(r#"{FLAT},"modEnvelope":{{"attack":0,"decay":0,"sustain":1,"release":0.1,"target":"off"}}"#);
    assert!(sustained("modEnv", &mod_env) < open * 0.01, "{}", sustained("modEnv", &mod_env));
}

#[test]
fn the_random_source_is_drawn_again_for_each_note() {
    let settings = at(1.0, 0.8, 0.125);
    let plain = tone("sine", OPEN, FLAT);
    let first = render(&plain, &settings);
    assert_eq!(repeat_difference_db(&plain, &settings, &first).unwrap(), None);
    let random = with_matrix("sine", OPEN, FLAT, &line("random", "vco1.detune", 0.5));
    let first = render(&random, &settings);
    let difference = repeat_difference_db(&random, &settings, &first).unwrap();
    assert!(difference.is_some_and(|d| d > -30.0), "two notes differ by {difference:?} dB");
}

// ─── The whole chain ─────────────────────────────────────────────────────────

/// A pattern with a synth track, rendered as an export renders it (the
/// engine, the strip, the master bus), written as a WAV file, read back and
/// measured.
#[test]
fn an_exported_pattern_plays_its_notes_at_their_pitch_and_time() {
    let mut presets = Object::new();
    // The matrix is on, with nothing to read: the whole group is part of the file.
    let matrix = format!(r#"{FLAT},"matrix":{{{}}}"#, line("lfo1", "filter.freq", 0.0));
    presets.insert("TONE", tone("sine", OPEN, &matrix));
    let synths = SynthPresets::from_json(&Value::Object(presets)).unwrap();
    let setup = RenderSetup {
        bank: Arc::new(SampleBank { samples: Vec::new() }),
        scales: Scales::default(),
        synths,
        kit: "none".into(),
        sample_rate: 44_100,
        seed: 1,
        returns: Default::default(),
    };
    // A note a beat (0.5 s) long on each of the first three beats, none on the fourth.
    let text = r#"{"patterns":[{"id":"p","name":"p","bpm":120,"beatCount":4,"tracks":[
        {"name":"LEAD","useSynth":true,"synthPreset":"TONE","notes":[
            {"beat":0,"pitch":0,"lengthSteps":4},
            {"beat":1,"pitch":12,"lengthSteps":4},
            {"beat":2,"pitch":7,"lengthSteps":4}]}]}]}"#;
    let (library, report) = Library::parse(text).unwrap();
    assert!(report.issues.is_empty(), "{:#?}", report.issues);
    let (left, right) = render_pattern(&library, 0, 1, &setup, None).unwrap();
    assert_eq!(left.len(), 88_200, "4 beats at 120 bpm");

    let file = wav::encode(&left, &right, 44_100);
    let back = wav::decode(&file).unwrap();
    assert_eq!((back.sample_rate, back.frames()), (44_100, left.len()));
    let worst = left.iter().zip(&back.left).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
    assert!(worst < 1.0 / 16_000.0, "16 bits lose {worst}");

    let beat = |n: usize| {
        let from = (n as f64 * 0.5 + 0.1) * 44_100.0;
        let segment = back.left[from as usize..(from + 0.3 * 44_100.0) as usize].to_vec();
        SampleBuffer::mono(segment, 44_100)
    };
    let mut levels = Vec::new();
    for (n, semitones) in [(0, 0.0), (1, 12.0), (2, 7.0)] {
        let note = analyze(&beat(n));
        let expected = C3 * 2.0_f64.powf(semitones / 12.0);
        let hz = note.fundamental_hz.unwrap_or_else(|| panic!("beat {n}: no pitch"));
        assert!((1200.0 * (hz / expected).log2()).abs() < 10.0, "beat {n}: {hz} Hz, expected {expected:.1}");
        levels.push(note.rms_db);
    }
    assert!(levels.iter().all(|l| (l - levels[0]).abs() < 3.0), "the notes differ in level: {levels:?}");
    // The fourth beat has no note: the last one has been released for 0.1 s.
    let silence = analyze(&beat(3));
    assert!(silence.rms_db < levels[0] - 40.0, "{} dB after the notes, {} during", silence.rms_db, levels[0]);
}

// ─── The presets that use the matrix ─────────────────────────────────────────

const MATRIX_PRESETS: [&str; 4] = ["BASS_TOUCH", "BASS_WOBBLE", "SYNTHPAD_DRIFT", "PIANO_KEYBELL"];

/// A factory preset, its matrix emptied when `without_matrix`.
fn preset_of(key: &str, without_matrix: bool) -> Value {
    let mut preset = factory_presets().get(key).unwrap().clone();
    if without_matrix {
        preset.as_object_mut().unwrap().remove("matrix");
    }
    preset
}

/// The share of a steady note's energy above 800 Hz.
fn brightness(preset: &Value, velocity: f64) -> f64 {
    let note = render(preset, &at(0.5, velocity, GATE));
    let samples: Vec<f64> = note.left.iter().map(|&s| f64::from(s)).collect();
    let power = spectrum(&samples);
    let from = (800.0 * WINDOW as f64 / RATE) as usize;
    power[from..].iter().sum::<f64>() / power.iter().sum::<f64>()
}

#[test]
fn the_matrix_presets_sound_unlike_themselves_without_the_matrix() {
    for key in MATRIX_PRESETS {
        let with = render(&preset_of(key, false), &at(1.0, 0.8, GATE));
        let without = render(&preset_of(key, true), &at(1.0, 0.8, GATE));
        let (a, b): (Vec<f64>, Vec<f64>) =
            (with.left.iter().map(|&s| f64::from(s)).collect(), without.left.iter().map(|&s| f64::from(s)).collect());
        let alike = correlation(&a, &b);
        assert!(alike < 0.995, "{key}: the matrix changes nothing (correlation {alike})");
    }
}

#[test]
fn a_touch_sensitive_bass_is_brighter_when_hit_harder() {
    let preset = preset_of("BASS_TOUCH", false);
    let (soft, hard) = (brightness(&preset, 0.3), brightness(&preset, 1.0));
    assert!(hard > soft * 1.5, "soft {soft:.4}, hard {hard:.4}");
    // Without the matrix the velocity only sets the level.
    let flat = preset_of("BASS_TOUCH", true);
    let (soft, hard) = (brightness(&flat, 0.3), brightness(&flat, 1.0));
    assert!((hard / soft - 1.0).abs() < 0.2, "soft {soft:.4}, hard {hard:.4}");
}

#[test]
fn a_drifting_pad_is_not_the_same_on_every_note() {
    let preset = preset_of("SYNTHPAD_DRIFT", false);
    let note = |serial: u64| {
        let mut voice = SynthVoice::default();
        let note = SynthNote { ratio: 1.0, velocity: 0.8, pan: 0.0, bpm: 120.0 };
        voice.start(&synth_patch(&preset, note), 0, 0.0, serial);
        let (mut left, mut right) = (vec![0.0f32; 24_000], vec![0.0f32; 24_000]);
        voice.render(RATE as f32, 0, &mut left, &mut right);
        left.iter().map(|&s| f64::from(s)).collect::<Vec<f64>>()
    };
    assert!(correlation(&note(1), &note(2)) < 0.999, "two notes are alike");
    assert_eq!(note(1), note(1), "a note is not the same twice");
}
