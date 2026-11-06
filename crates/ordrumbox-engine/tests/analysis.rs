//! The kit editor's sample analysis. Sines of known frequency and amplitude
//! give the fundamental, peak, RMS, envelope and note that theory gives; the
//! edge cases (silence, very short files, noise, decays, a DC offset, stereo,
//! every WAV format, the kick.wav fixture) are checked against their Rust
//! snapshot `analysis_rust.txt`, which `write_the_snapshot` rewrites when the
//! analysis changes on purpose.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::f64::consts::{PI, TAU};
use std::fs;
use std::path::{Path, PathBuf};

use ordrumbox_core::Rng;
use ordrumbox_engine::analysis::{Analysis, analyze, hz_to_note};
use ordrumbox_engine::{SampleBuffer, wav};

/// `frames` samples of a sine of `hz` at `rate`, of peak `amp`, decaying by
/// `decay` per second (e^-decay·t).
fn tone(hz: f64, rate: u32, frames: usize, amp: f64, decay: f64) -> Vec<f64> {
    let rate = f64::from(rate);
    (0..frames).map(|i| i as f64 / rate).map(|t| amp * (TAU * hz * t).sin() * (-decay * t).exp()).collect()
}

fn as_f32(samples: &[f64]) -> Vec<f32> {
    samples.iter().map(|&s| s as f32).collect()
}

fn to_db(linear: f64) -> f64 {
    20.0 * linear.log10()
}

// ─── Sines: what theory gives ────────────────────────────────────────────────

/// A sine, its sample rate and length, and the note it is.
struct Sine {
    hz: f64,
    rate: u32,
    frames: usize,
    amp: f64,
    note: &'static str,
}

const SINES: [Sine; 7] = [
    Sine { hz: 220.0, rate: 44_100, frames: 9_000, amp: 0.5, note: "A3" },
    Sine { hz: 440.0, rate: 48_000, frames: 6_000, amp: 0.8, note: "A4" },
    Sine { hz: 65.41, rate: 44_100, frames: 20_000, amp: 0.9, note: "C2" },
    Sine { hz: 110.0, rate: 22_050, frames: 8_000, amp: 0.3, note: "A2" },
    Sine { hz: 1046.5, rate: 44_100, frames: 5_000, amp: 0.25, note: "C6" },
    Sine { hz: 1760.0, rate: 44_100, frames: 8_000, amp: 1.0, note: "A6" },
    Sine { hz: 392.0, rate: 96_000, frames: 12_000, amp: 0.01, note: "G4" },
];

/// Checks the analysis of a sine of `hz` at `rate` and peak `amp`.
fn check_sine(name: &str, a: &Analysis, sine: &Sine) {
    let rate = f64::from(sine.rate);
    assert_eq!(a.duration_sec, sine.frames as f64 / rate, "{name}: duration");
    // The samples miss the crest by at most half a sample's phase.
    let lowest_peak = sine.amp * (PI * sine.hz / rate).cos();
    let rounding = 1e-6 * sine.amp;
    assert!((lowest_peak - rounding..=sine.amp + rounding).contains(&a.peak_linear), "{name}: peak {}", a.peak_linear);
    assert!((a.peak_db - to_db(a.peak_linear)).abs() <= 5e-4, "{name}: peak dB {}", a.peak_db);
    // A sine's RMS is its peak over √2; a part period at the end moves it a
    // little, by less than 1/(2N·sin(ωT)) of the power.
    let expected_rms_db = to_db(sine.amp / 2f64.sqrt());
    assert!((a.rms_db - expected_rms_db).abs() < 0.05, "{name}: RMS {} dB, expected {expected_rms_db}", a.rms_db);
    assert!((a.rms_linear - sine.amp / 2f64.sqrt()).abs() < 0.006 * sine.amp, "{name}: RMS {}", a.rms_linear);
    // The fundamental is a whole lag of the autocorrelation: within one sample
    // of the period.
    let fundamental = a.fundamental_hz.unwrap_or_else(|| panic!("{name}: no fundamental"));
    assert!((rate / fundamental - rate / sine.hz).abs() < 1.0, "{name}: fundamental {fundamental}");
    assert!(a.pitch_confidence > 0.95, "{name}: confidence {}", a.pitch_confidence);
    let note = a.note.unwrap_or_else(|| panic!("{name}: no note"));
    assert_eq!(format!("{}{}", note.name, note.octave), sine.note, "{name}: note {note}");
    assert_eq!(Some(note), hz_to_note(fundamental), "{name}: the note of the fundamental");
    // The envelope: 128 slices, each at most the peak; the loudest is the
    // peak; a slice of a whole period or more holds a crest.
    assert_eq!(a.envelope.len(), 128, "{name}: envelope points");
    let rounded_peak = (a.peak_linear * 1e6).round() / 1e6;
    assert!(a.envelope.iter().all(|&e| e <= rounded_peak), "{name}: envelope above the peak");
    assert_eq!(a.envelope.iter().copied().fold(0.0, f64::max), rounded_peak, "{name}: envelope peak");
    if sine.frames / 128 >= (rate / sine.hz).ceil() as usize {
        assert!(a.envelope.iter().all(|&e| e >= lowest_peak - 1e-6), "{name}: envelope {:?}", a.envelope);
    }
}

#[test]
fn sines_give_their_frequency_peak_and_rms() {
    for sine in &SINES {
        let samples = as_f32(&tone(sine.hz, sine.rate, sine.frames, sine.amp, 0.0));
        let name = format!("{} Hz at {}", sine.hz, sine.rate);
        check_sine(&name, &analyze(&SampleBuffer::mono(samples, sine.rate)), sine);
    }
}

#[test]
fn a_sine_read_from_a_16_bit_file_gives_the_same_analysis() {
    let sine = &SINES[0];
    let samples = as_f32(&tone(sine.hz, sine.rate, sine.frames, sine.amp, 0.0));
    let decoded = wav::decode(&wav::encode(&samples, &samples, sine.rate)).unwrap();
    let a = analyze(&decoded);
    check_sine("16-bit", &a, sine);
    let exact = analyze(&SampleBuffer::mono(samples, sine.rate));
    assert_eq!((a.fundamental_hz, a.note), (exact.fundamental_hz, exact.note));
    assert!((a.peak_linear - exact.peak_linear).abs() <= 1.0 / 32_768.0);
}

#[test]
fn levels_follow_the_amplitude() {
    let (hz, rate, frames) = (330.0, 44_100, 8_000);
    let base = analyze(&SampleBuffer::mono(as_f32(&tone(hz, rate, frames, 0.8, 0.0)), rate));
    for gain in [0.5, 0.1, 0.01] {
        let a = analyze(&SampleBuffer::mono(as_f32(&tone(hz, rate, frames, 0.8 * gain, 0.0)), rate));
        let db = to_db(gain);
        assert!((a.peak_db - base.peak_db - db).abs() < 2e-3, "peak at {gain}: {}", a.peak_db);
        assert!((a.rms_db - base.rms_db - db).abs() < 2e-3, "RMS at {gain}: {}", a.rms_db);
        assert_eq!(a.fundamental_hz, base.fundamental_hz, "fundamental at {gain}");
    }
}

#[test]
fn a_dc_offset_moves_the_peak_not_the_pitch() {
    let (hz, rate, amp, dc) = (100.0, 44_100, 0.3, 0.4);
    // 15 whole periods: the DC and the sine do not mix in the mean square.
    let samples: Vec<f64> = tone(hz, rate, 6_615, amp, 0.0).iter().map(|s| s + dc).collect();
    let a = analyze(&SampleBuffer::mono(as_f32(&samples), rate));
    assert!((a.peak_linear - (amp + dc)).abs() < 1e-4, "peak {}", a.peak_linear);
    // RMS² = DC² + amp²/2.
    let rms = (dc * dc + amp * amp / 2.0).sqrt();
    assert!((a.rms_linear - rms).abs() < 2e-3, "RMS {}", a.rms_linear);
    let fundamental = a.fundamental_hz.unwrap();
    assert!((f64::from(rate) / fundamental - f64::from(rate) / hz).abs() < 1.0, "fundamental {fundamental}");
}

#[test]
fn stereo_is_analysed_as_its_mono_mix() {
    let rate = 44_100;
    let left = as_f32(&tone(330.0, rate, 7_000, 0.6, 0.0));
    let right = as_f32(&tone(331.0, rate, 7_000, 0.4, 3.0));
    let mix: Vec<f32> = left.iter().zip(&right).map(|(l, r)| (l + r) * 0.5).collect();
    let stereo = SampleBuffer { left, right: Some(right), sample_rate: rate };
    assert_eq!(analyze(&stereo), analyze(&SampleBuffer::mono(mix, rate)));
}

#[test]
fn silence_has_no_level_and_no_pitch() {
    for frames in [0, 3, 3_000] {
        let a = analyze(&SampleBuffer::mono(vec![0.0; frames], 44_100));
        assert_eq!(a.envelope, vec![0.0; frames.min(128)], "{frames}");
        assert_eq!((a.peak_linear, a.rms_linear), (0.0, 0.0), "{frames}");
        assert_eq!((a.peak_db, a.rms_db), (f64::NEG_INFINITY, f64::NEG_INFINITY), "{frames}");
        assert_eq!((a.fundamental_hz, a.pitch_confidence, a.note), (None, 0.0, None), "{frames}");
    }
}

// ─── Edge cases: the snapshot ────────────────────────────────────────────────

/// A WAV file of `channels` (-1..1) as `format` 1 (PCM) or 3 (float), `bits` deep.
fn wav_file(channels: &[Vec<f64>], rate: u32, format: u16, bits: u16) -> Vec<u8> {
    let mut data = Vec::new();
    for i in 0..channels[0].len() {
        for channel in channels {
            let s = channel[i].clamp(-1.0, 1.0);
            match (format, bits) {
                (3, _) => data.extend((s as f32).to_le_bytes()),
                (_, 8) => data.push(((s * 127.0).round() + 128.0) as u8),
                (_, 16) => data.extend(((s * 32_767.0).round() as i16).to_le_bytes()),
                (_, 24) => data.extend(&((s * 8_388_607.0).round() as i32).to_le_bytes()[..3]),
                _ => data.extend(((s * 2_147_483_647.0).round() as i32).to_le_bytes()),
            }
        }
    }
    let block = channels.len() as u16 * bits / 8;
    let mut out = Vec::new();
    out.extend(b"RIFF");
    out.extend((36 + data.len() as u32).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(format.to_le_bytes());
    out.extend((channels.len() as u16).to_le_bytes());
    out.extend(rate.to_le_bytes());
    out.extend((rate * u32::from(block)).to_le_bytes());
    out.extend(block.to_le_bytes());
    out.extend(bits.to_le_bytes());
    out.extend(b"data");
    out.extend((data.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

fn noise(rng: &mut Rng, frames: usize, amp: f64) -> Vec<f64> {
    (0..frames).map(|_| (rng.next_f64() * 2.0 - 1.0) * amp).collect()
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures")
}

/// The edge cases, as WAV files: the quantization keeps the last bit of the
/// platform's sine out of the snapshot.
fn edge_cases() -> Vec<(&'static str, Vec<u8>)> {
    let mut rng = Rng::new(3);
    let noisy: Vec<f64> = tone(196.0, 44_100, 9_000, 0.5, 0.0).iter().map(|v| v + noise(&mut rng, 1, 0.2)[0]).collect();
    let dc: Vec<f64> = tone(100.0, 44_100, 6_000, 0.3, 0.0).iter().map(|v| v + 0.4).collect();
    vec![
        ("c2-decay-24bit", wav_file(&[tone(65.41, 44_100, 20_000, 0.9, 6.0)], 44_100, 1, 24)),
        ("high-1500-32bit", wav_file(&[tone(1500.0, 44_100, 5_000, 0.3, 0.0)], 44_100, 1, 32)),
        ("a440-float-48k", wav_file(&[tone(440.0, 48_000, 6_000, 0.8, 0.0)], 48_000, 3, 32)),
        (
            "stereo-detuned",
            wav_file(&[tone(330.0, 44_100, 7_000, 0.6, 0.0), tone(331.0, 44_100, 7_000, 0.4, 3.0)], 44_100, 1, 16),
        ),
        ("noise-8bit", wav_file(&[noise(&mut rng, 5_000, 0.7)], 22_050, 1, 8)),
        ("noisy-tone", wav_file(&[noisy], 44_100, 1, 16)),
        ("silence", wav_file(&[vec![0.0; 3_000]], 44_100, 1, 16)),
        ("empty", wav_file(&[Vec::new()], 44_100, 1, 16)),
        ("very-short", wav_file(&[vec![0.1, -0.5, 0.25, 0.9, -0.2]], 44_100, 1, 16)),
        ("shorter-than-envelope", wav_file(&[tone(1000.0, 8_000, 100, 0.5, 0.0)], 8_000, 3, 32)),
        ("dc-offset", wav_file(&[dc], 44_100, 1, 16)),
        ("kick.wav", fs::read(fixtures().join("kick.wav")).expect("kick.wav")),
    ]
}

fn rust_snapshot() -> Vec<String> {
    let mut lines = Vec::new();
    for (name, bytes) in edge_cases() {
        let a = analyze(&wav::decode(&bytes).unwrap());
        let fundamental = a.fundamental_hz.map_or("none".to_owned(), |f| f.to_string());
        let note = a.note.map_or("none".to_owned(), |n| n.to_string());
        lines.push(format!(
            "{name}: duration={} peak={} peakDb={} rms={} rmsDb={} fundamental={fundamental} confidence={} note={note}",
            a.duration_sec, a.peak_linear, a.peak_db, a.rms_linear, a.rms_db, a.pitch_confidence
        ));
        let envelope: Vec<String> = a.envelope.iter().map(f64::to_string).collect();
        lines.push(format!("{name} envelope: {}", envelope.join(" ")));
    }
    lines
}

fn snapshot_path() -> PathBuf {
    fixtures().join("analysis_rust.txt")
}

/// Writes `analysis_rust.txt` from the code as it is: `cargo test -p
/// ordrumbox-engine --test analysis -- --ignored`, when the analysis changes
/// on purpose (`README.md`, "Golden fixtures").
#[test]
#[ignore = "writes the snapshot"]
fn write_the_snapshot() {
    fs::write(snapshot_path(), rust_snapshot().join("\n") + "\n").expect("written");
}

#[test]
fn edge_cases_match_their_snapshot() {
    let text = fs::read_to_string(snapshot_path()).expect("tests/fixtures/analysis_rust.txt");
    let expected: Vec<&str> = text.lines().collect();
    let actual = rust_snapshot();
    assert_eq!(actual.len(), expected.len(), "snapshot lines");
    for (actual, expected) in actual.iter().zip(expected) {
        assert_eq!(actual, expected);
    }
}
