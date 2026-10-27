//! What the kit editor shows about a sample (`audio/analyzer.js`,
//! `audio/sample_analyzer.js`, `core/hz_to_note.js`): its envelope, length,
//! peak and RMS levels, and the note it sounds (autocorrelation over its
//! loudest frame), computed as the JS computes them.

use ordrumbox_core::js;
use ordrumbox_dsp::SampleBuffer;

/// Points of the envelope (`envelopePoints`).
const ENVELOPE_POINTS: usize = 128;
/// Samples of the frame the pitch is estimated on (`pitchFrameSize`).
const PITCH_FRAME: usize = 4096;
const MIN_FUNDAMENTAL_HZ: f64 = 40.0;
const MAX_FUNDAMENTAL_HZ: f64 = 2000.0;

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// The nearest note of a frequency (`hzToNote`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteInfo {
    pub name: &'static str,
    pub octave: i32,
    pub cents: i32,
    pub midi: i32,
}

impl std::fmt::Display for NoteInfo {
    /// `formatNote`: "A4 +3ct".
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let sign = if self.cents > 0 { "+" } else { "" };
        write!(f, "{}{} {sign}{}ct", self.name, self.octave, self.cents)
    }
}

/// `hzToNote`.
pub fn hz_to_note(hz: f64) -> Option<NoteInfo> {
    if !hz.is_finite() || hz <= 0.0 {
        return None;
    }
    let midi_float = 69.0 + 12.0 * (hz / 440.0).log2();
    let midi = js::round(midi_float);
    let cents = js::round((midi_float - midi) * 100.0) as i32;
    let midi = midi as i32;
    Some(NoteInfo { name: NOTE_NAMES[midi.rem_euclid(12) as usize], octave: midi.div_euclid(12) - 1, cents, midi })
}

/// A sample's analysis (`analyzeSample`).
#[derive(Debug, Clone, PartialEq)]
pub struct Analysis {
    /// Peak of each of 128 slices, 6 decimals.
    pub envelope: Vec<f64>,
    pub duration_sec: f64,
    pub peak_linear: f64,
    /// dB, 3 decimals; -∞ for silence.
    pub peak_db: f64,
    pub rms_linear: f64,
    pub rms_db: f64,
    pub fundamental_hz: Option<f64>,
    pub pitch_confidence: f64,
    pub note: Option<NoteInfo>,
}

/// `mixToMono`, in `f32` like the `Float32Array` it fills.
fn mono(buffer: &SampleBuffer) -> Vec<f32> {
    match &buffer.right {
        None => buffer.left.clone(),
        Some(right) => {
            let length = buffer.left.len().min(right.len());
            (0..length).map(|i| (buffer.left[i] + right[i]) * 0.5).collect()
        }
    }
}

/// `linearToDb`.
fn to_db(value: f64) -> f64 {
    if !value.is_finite() || value <= 0.0 { f64::NEG_INFINITY } else { js::to_fixed(20.0 * value.log10(), 3) }
}

/// `nextPowerOfTwo`.
fn next_power_of_two(value: usize) -> usize {
    let mut power = 1;
    while power < value {
        power <<= 1;
    }
    power
}

/// `selectAnalysisFrame`: the loudest `target`-sample frame (zero-padded
/// when the sample is shorter).
fn analysis_frame(samples: &[f32], target: usize) -> Vec<f32> {
    let size = next_power_of_two(target.min(samples.len())).max(32);
    if samples.len() <= size {
        let mut frame = vec![0.0; size];
        frame[..samples.len()].copy_from_slice(samples);
        return frame;
    }
    let hop = (size / 4).max(1);
    let (mut best_offset, mut best_energy) = (0, -1.0);
    let mut offset = 0;
    while offset + size <= samples.len() {
        let energy: f64 = samples[offset..offset + size].iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        if energy > best_energy {
            best_energy = energy;
            best_offset = offset;
        }
        offset += hop;
    }
    samples[best_offset..best_offset + size].to_vec()
}

/// `estimateFundamental`: the lag of the best normalized autocorrelation
/// (the first strong peak near it), as a frequency, with its confidence.
fn fundamental(frame: &[f32], sample_rate: f64) -> (Option<f64>, f64) {
    let mean = frame.iter().fold(0.0, |sum, &v| sum + f64::from(v)) / frame.len() as f64;
    let normalized: Vec<f64> = frame.iter().map(|&v| f64::from((f64::from(v) - mean) as f32)).collect();
    let max_lag = (normalized.len() - 1).min((sample_rate / MIN_FUNDAMENTAL_HZ).floor() as usize);
    let min_lag = ((sample_rate / MAX_FUNDAMENTAL_HZ).floor() as usize).max(1);
    let energy = normalized.iter().fold(0.0, |sum, v| sum + v * v);
    if energy == 0.0 || max_lag <= min_lag {
        return (None, 0.0);
    }
    let (mut best_lag, mut best) = (0usize, 0.0f64);
    let mut correlations = vec![0.0f64; max_lag + 1];
    for lag in min_lag..=max_lag {
        let (mut num, mut energy_a, mut energy_b) = (0.0, 0.0, 0.0);
        for i in 0..normalized.len() - lag {
            let (a, b) = (normalized[i], normalized[i + lag]);
            num += a * b;
            energy_a += a * a;
            energy_b += b * b;
        }
        let denominator = (energy_a * energy_b).sqrt();
        let correlation = if denominator > 0.0 { num / denominator } else { 0.0 };
        correlations[lag] = correlation;
        if correlation > best {
            best = correlation;
            best_lag = lag;
        }
    }
    if best_lag == 0 || best < 0.1 {
        return (None, js::to_fixed(best.max(0.0), 4));
    }
    let threshold = (best * 0.9).max(0.6);
    for lag in min_lag + 1..max_lag {
        let c = correlations[lag];
        if c >= threshold && c >= correlations[lag - 1] && c >= correlations[lag + 1] {
            best_lag = lag;
            best = c;
            break;
        }
    }
    (Some(js::to_fixed(sample_rate / best_lag as f64, 3)), js::to_fixed(best.clamp(0.0, 1.0), 4))
}

/// Analyse `buffer` (`analyzeAudioBuffer`).
pub fn analyze(buffer: &SampleBuffer) -> Analysis {
    let samples = mono(buffer);
    let rate = f64::from(buffer.sample_rate);
    if samples.is_empty() {
        return Analysis {
            envelope: Vec::new(),
            duration_sec: 0.0,
            peak_linear: 0.0,
            peak_db: f64::NEG_INFINITY,
            rms_linear: 0.0,
            rms_db: f64::NEG_INFINITY,
            fundamental_hz: None,
            pitch_confidence: 0.0,
            note: None,
        };
    }
    let len = samples.len();
    let points = ENVELOPE_POINTS.min(len);
    let envelope = (0..points)
        .map(|p| {
            let (start, end) = (p * len / points, ((p + 1) * len / points).min(len));
            let peak = samples[start..end].iter().fold(0.0f32, |m, s| m.max(s.abs()));
            js::to_fixed(f64::from(peak), 6)
        })
        .collect();
    let peak_linear = samples.iter().fold(0.0f64, |m, &s| m.max(f64::from(s.abs())));
    let squares = samples.iter().fold(0.0, |sum, &s| sum + f64::from(s) * f64::from(s));
    let rms_linear = (squares / len as f64).sqrt();
    let (fundamental_hz, pitch_confidence) = fundamental(&analysis_frame(&samples, PITCH_FRAME), rate);
    Analysis {
        envelope,
        duration_sec: len as f64 / rate,
        peak_linear,
        peak_db: to_db(peak_linear),
        rms_linear,
        rms_db: to_db(rms_linear),
        fundamental_hz,
        pitch_confidence,
        note: fundamental_hz.and_then(hz_to_note),
    }
}

/// `normalizeAll` for one sample: scaled to a 0 dB peak; `None` when silent.
pub fn normalized(buffer: &SampleBuffer) -> Option<SampleBuffer> {
    let analysis = analyze(buffer);
    if analysis.peak_linear <= 0.0 {
        return None;
    }
    let gain = 10f64.powf(-analysis.peak_db / 20.0);
    let scale = |channel: &[f32]| channel.iter().map(|&s| (f64::from(s) * gain) as f32).collect::<Vec<f32>>();
    Some(SampleBuffer {
        left: scale(&buffer.left),
        right: buffer.right.as_deref().map(scale),
        sample_rate: buffer.sample_rate,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes() {
        assert_eq!(hz_to_note(440.0).unwrap().to_string(), "A4 0ct");
        assert_eq!(hz_to_note(261.63).unwrap().to_string(), "C4 0ct");
        assert_eq!(hz_to_note(450.0).unwrap().to_string(), "A4 +39ct");
        assert_eq!(hz_to_note(0.0), None);
    }

    #[test]
    fn sine_is_found() {
        let rate = 44_100;
        let samples: Vec<f32> = (0..8000)
            .map(|i| (std::f64::consts::TAU * 220.0 * f64::from(i) / f64::from(rate)).sin() as f32 * 0.5)
            .collect();
        let analysis = analyze(&SampleBuffer::mono(samples, rate));
        assert_eq!(analysis.note.unwrap().name, "A");
        assert!((analysis.peak_db + 6.02).abs() < 0.01);
        assert_eq!(analysis.envelope.len(), 128);
        let louder = normalized(&SampleBuffer::mono(vec![0.25, -0.5], 8000)).unwrap();
        // The gain is computed from the peak in dB to 3 decimals, like the JS.
        assert!((louder.left[1] + 1.0).abs() < 1e-4 && (louder.left[0] - 0.5).abs() < 1e-4);
    }
}
