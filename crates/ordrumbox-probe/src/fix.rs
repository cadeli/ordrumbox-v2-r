//! Repairs what `kit` flags in a sample: a late start trimmed, a loud end
//! faded out, DC removed, the peak set to the kit's level. A high-pass filter
//! and a shorter length are applied only when asked. Each repair is listed.

use ordrumbox_dsp::SampleBuffer;

use crate::kit::{DC_LIMIT, END_LEVEL_LIMIT_DB, LEAD_IN_LIMIT_MS};
use crate::measure::{END_WINDOW_MS, LEAD_IN_LEVEL, amplitude_db, rms};

/// The peak level of the kits' samples.
pub const DEFAULT_PEAK_DB: f64 = -3.0;
/// Long enough to sound like the sample's own decay, short enough for a short sample.
pub const DEFAULT_FADE_OUT_MS: f64 = 130.0;
/// What a trimmed sample keeps before its start, faded in.
const PRE_ROLL_MS: f64 = 1.0;
/// A peak this close to the target is left as it is.
const PEAK_TOLERANCE_DB: f64 = 0.05;
/// A fade-out takes at most this share of what is left of the sample.
const MAX_FADE_OUT_SHARE: f64 = 0.5;
const BUTTERWORTH_Q: f64 = std::f64::consts::FRAC_1_SQRT_2;

/// What `fix` does beyond the flagged problems, and how.
#[derive(Debug, Clone, PartialEq)]
pub struct FixSettings {
    /// Peak level to set, dB; `None` keeps the sample's level.
    pub peak_db: Option<f64>,
    /// Length of a fade-out, ms.
    pub fade_out_ms: f64,
    /// Cut-off of a high-pass filter (12 dB/octave), Hz.
    pub highpass_hz: Option<f64>,
    /// Longest the sample may last, ms; a longer one is cut and faded out.
    pub length_ms: Option<f64>,
}

impl Default for FixSettings {
    fn default() -> Self {
        Self { peak_db: Some(DEFAULT_PEAK_DB), fade_out_ms: DEFAULT_FADE_OUT_MS, highpass_hz: None, length_ms: None }
    }
}

/// A repaired sample and what was done to it, empty when nothing was.
#[derive(Debug, Clone, PartialEq)]
pub struct Fixed {
    pub buffer: SampleBuffer,
    pub changes: Vec<String>,
}

fn frames_of(ms: f64, rate: f64) -> usize {
    (ms * rate / 1000.0).round() as usize
}

/// The channels' mean, as `measure` hears the sample.
fn mono(channels: &[Vec<f64>]) -> Vec<f64> {
    let count = channels.len() as f64;
    (0..channels[0].len()).map(|i| channels.iter().map(|c| c[i]).sum::<f64>() / count).collect()
}

fn peak(samples: &[f64]) -> f64 {
    samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

/// A 2nd-order Butterworth high-pass filter, run over `samples`.
fn highpass(samples: &mut [f64], cutoff_hz: f64, rate: f64) {
    let w0 = std::f64::consts::TAU * cutoff_hz / rate;
    let alpha = w0.sin() / (2.0 * BUTTERWORTH_Q);
    let cos = w0.cos();
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = ((1.0 + cos) / 2.0 / a0, -(1.0 + cos) / a0, (1.0 + cos) / 2.0 / a0);
    let (a1, a2) = (-2.0 * cos / a0, (1.0 - alpha) / a0);
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    for sample in samples {
        let x = *sample;
        let y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2;
        (x2, x1, y2, y1) = (x1, x, y1, y);
        *sample = y;
    }
}

/// Raised cosine from 0 to 1 over `length` frames, at `frame`.
fn rise(frame: usize, length: usize) -> f64 {
    0.5 - 0.5 * (std::f64::consts::PI * frame as f64 / length as f64).cos()
}

/// Repair `buffer` as `settings` say.
pub fn fix(buffer: &SampleBuffer, settings: &FixSettings) -> Fixed {
    let rate = f64::from(buffer.sample_rate);
    let wide = |channel: &[f32]| channel.iter().map(|&s| f64::from(s)).collect::<Vec<f64>>();
    let mut channels = vec![wide(&buffer.left)];
    if let Some(right) = &buffer.right {
        let frames = buffer.left.len().min(right.len());
        channels[0].truncate(frames);
        channels.push(wide(&right[..frames]));
    }
    let mut changes = Vec::new();
    if channels[0].is_empty() || peak(&mono(&channels)) == 0.0 {
        return Fixed { buffer: buffer.clone(), changes };
    }

    if let Some(cutoff_hz) = settings.highpass_hz {
        for channel in &mut channels {
            highpass(channel, cutoff_hz, rate);
        }
        changes.push(format!("high-passed at {cutoff_hz:.0} Hz"));
    }

    // The gain of the fades applied, frame by frame: DC is taken out along it,
    // so that a faded start or end stays at zero.
    let mut shape = vec![1.0; channels[0].len()];
    let samples = mono(&channels);
    let mono_peak = peak(&samples);
    let onset = samples.iter().position(|s| s.abs() >= LEAD_IN_LEVEL * mono_peak).unwrap_or(0);
    let lead_in_ms = onset as f64 * 1000.0 / rate;
    if lead_in_ms > LEAD_IN_LIMIT_MS {
        let pre_roll = frames_of(PRE_ROLL_MS, rate).min(onset);
        let start = onset - pre_roll;
        for track in channels.iter_mut().chain(std::iter::once(&mut shape)) {
            track.drain(..start);
            for (frame, sample) in track[..pre_roll].iter_mut().enumerate() {
                *sample *= rise(frame, pre_roll);
            }
        }
        changes.push(format!("started {lead_in_ms:.0} ms late: trimmed to {PRE_ROLL_MS:.0} ms before its start"));
    }

    let mut cut = false;
    if let Some(length_ms) = settings.length_ms {
        let length = frames_of(length_ms, rate).max(1);
        if shape.len() > length {
            channels.iter_mut().chain(std::iter::once(&mut shape)).for_each(|track| track.truncate(length));
            changes.push(format!("cut to {length_ms:.0} ms"));
            cut = true;
        }
    }

    let samples = mono(&channels);
    let frames = samples.len();
    let window = frames_of(END_WINDOW_MS, rate).clamp(1, frames);
    let end_level_db = amplitude_db(rms(&samples[frames - window..]) / peak(&samples));
    if cut || end_level_db > END_LEVEL_LIMIT_DB {
        let fade = frames_of(settings.fade_out_ms, rate).min((frames as f64 * MAX_FADE_OUT_SHARE) as usize).max(1);
        for track in channels.iter_mut().chain(std::iter::once(&mut shape)) {
            for (frame, sample) in track[frames - fade..].iter_mut().enumerate() {
                *sample *= rise(fade - frame, fade).powi(2);
            }
        }
        let fade_ms = fade as f64 * 1000.0 / rate;
        if cut {
            changes.push(format!("faded out over its last {fade_ms:.0} ms"));
        } else {
            changes.push(format!("still loud at its end ({end_level_db:.0} dB): faded out over {fade_ms:.0} ms"));
        }
    }

    let mean = |track: &[f64]| track.iter().sum::<f64>() / track.len() as f64;
    let dc = mean(&mono(&channels));
    let shape_mean = mean(&shape);
    if dc.abs() > DC_LIMIT && shape_mean > 0.0 {
        for channel in &mut channels {
            let offset = mean(channel) / shape_mean;
            channel.iter_mut().zip(&shape).for_each(|(s, gain)| *s -= offset * gain);
        }
        changes.push(format!("DC {dc:.3} removed"));
    }

    if let Some(target_db) = settings.peak_db {
        let level = channels.iter().map(|channel| peak(channel)).fold(0.0, f64::max);
        let level_db = amplitude_db(level);
        if (level_db - target_db).abs() > PEAK_TOLERANCE_DB {
            let gain = 10f64.powf(target_db / 20.0) / level;
            channels.iter_mut().flatten().for_each(|s| *s *= gain);
            changes.push(format!("peak {level_db:.1} dB set to {target_db:.1} dB"));
        }
    }

    let narrow = |channel: &[f64]| channel.iter().map(|&s| s as f32).collect::<Vec<f32>>();
    let fixed = SampleBuffer {
        left: narrow(&channels[0]),
        right: channels.get(1).map(|channel| narrow(channel)),
        sample_rate: buffer.sample_rate,
    };
    if changes.is_empty() {
        return Fixed { buffer: buffer.clone(), changes };
    }
    Fixed { buffer: fixed, changes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::measure::measure;

    const RATE: u32 = 44_100;

    /// A decaying 200 Hz tone of `ms`, at `level`, after `silence_ms` of silence.
    fn hit(silence_ms: f64, ms: f64, level: f64, decay_per_s: f64) -> Vec<f32> {
        let rate = f64::from(RATE);
        let silence = frames_of(silence_ms, rate);
        let tone = (0..frames_of(ms, rate)).map(|i| {
            let t = i as f64 / rate;
            (level * (-decay_per_s * t).exp() * (std::f64::consts::TAU * 200.0 * t).sin()) as f32
        });
        std::iter::repeat_n(0.0, silence).chain(tone).collect()
    }

    fn fixed(samples: Vec<f32>, settings: &FixSettings) -> (Fixed, crate::measure::Measures) {
        let result = fix(&SampleBuffer::mono(samples, RATE), settings);
        let m = measure(&result.buffer);
        (result, m)
    }

    #[test]
    fn a_clean_sample_is_left_as_it_is() {
        let samples = hit(0.0, 300.0, 10f64.powf(DEFAULT_PEAK_DB / 20.0), 40.0);
        let peak = samples.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let samples: Vec<f32> = samples.iter().map(|s| s * 10f32.powf(-3.0 / 20.0) / peak).collect();
        let (result, _) = fixed(samples.clone(), &FixSettings::default());
        assert_eq!(result.changes, Vec::<String>::new());
        assert_eq!(result.buffer.left, samples);
    }

    #[test]
    fn a_late_start_is_trimmed() {
        let (result, m) = fixed(hit(20.0, 300.0, 0.7, 40.0), &FixSettings { peak_db: None, ..FixSettings::default() });
        assert_eq!(result.changes, ["started 20 ms late: trimmed to 1 ms before its start"]);
        assert!(m.lead_in_ms <= PRE_ROLL_MS + 0.1, "{}", m.lead_in_ms);
        assert!((m.duration_ms - 301.0).abs() < 0.1, "{}", m.duration_ms);
    }

    #[test]
    fn a_loud_end_is_faded_out() {
        let (result, m) = fixed(hit(0.0, 300.0, 0.7, 2.0), &FixSettings { peak_db: None, ..FixSettings::default() });
        assert_eq!(result.changes.len(), 1);
        assert!(result.changes[0].starts_with("still loud at its end"), "{:?}", result.changes);
        assert!(m.end_level_db.unwrap() < END_LEVEL_LIMIT_DB - 20.0, "{:?}", m.end_level_db);
    }

    #[test]
    fn the_peak_is_set_and_dc_removed() {
        let samples: Vec<f32> = hit(0.0, 300.0, 0.2, 40.0).iter().map(|s| s + 0.05).collect();
        let (result, m) = fixed(samples, &FixSettings { fade_out_ms: 50.0, ..FixSettings::default() });
        assert!(result.changes.iter().any(|change| change.starts_with("DC 0.0")), "{:?}", result.changes);
        assert!(result.changes.last().unwrap().ends_with("set to -3.0 dB"), "{:?}", result.changes);
        assert!((amplitude_db(m.peak) - DEFAULT_PEAK_DB).abs() < 0.01, "{}", m.peak);
        assert!(m.dc.abs() < DC_LIMIT);
    }

    #[test]
    fn a_cut_fades_out_and_a_high_pass_takes_the_rumble() {
        let rate = f64::from(RATE);
        let rumble: Vec<f32> =
            (0..RATE as usize).map(|i| (0.5 * (std::f64::consts::TAU * 30.0 * i as f64 / rate).sin()) as f32).collect();
        let settings =
            FixSettings { peak_db: None, highpass_hz: Some(200.0), length_ms: Some(250.0), ..FixSettings::default() };
        let (result, m) = fixed(rumble, &settings);
        assert_eq!(result.changes, ["high-passed at 200 Hz", "cut to 250 ms", "faded out over its last 125 ms"]);
        assert!((m.duration_ms - 250.0).abs() < 0.1);
        assert!(m.peak < 0.5 * 0.1, "{}", m.peak);
    }

    #[test]
    fn stereo_stays_stereo_and_silence_stays_silent() {
        let left = hit(0.0, 100.0, 0.3, 40.0);
        let stereo = SampleBuffer { left: left.clone(), right: Some(left), sample_rate: RATE };
        let result = fix(&stereo, &FixSettings::default());
        assert!(result.buffer.right.is_some());
        let silent = SampleBuffer::mono(vec![0.0; 100], RATE);
        assert_eq!(fix(&silent, &FixSettings::default()), Fixed { buffer: silent, changes: Vec::new() });
    }
}
