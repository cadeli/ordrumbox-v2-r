//! The measurements of one sound.

use ordrumbox_dsp::SampleBuffer;
use ordrumbox_engine::analysis::hz_to_note;

use crate::fft::power_spectrum;

const SPECTRUM_SIZE: usize = 8192;
const SPECTRUM_HOP: usize = 2048;
/// Below this a bin is DC or the leak of DC, not part of the sound's spectrum.
const MIN_SPECTRUM_HZ: f64 = 20.0;
const ROLLOFF_ENERGY: f64 = 0.85;
const PEAK_COUNT: usize = 8;
/// Bins two peaks stay apart: the width of a Hann window's main lobe.
const PEAK_SPACING_BINS: usize = 4;
const PEAK_FLOOR_DB: f64 = -60.0;
const HARMONIC_COUNT: usize = 8;
/// Lowest level reported relative to the strongest bin.
const FLOOR_DB: f64 = -120.0;
/// A periodicity below this is not a pitch.
const MIN_PITCH_CONFIDENCE: f64 = 0.5;
/// YIN's threshold: the first dip under it is the period, which keeps a
/// strong octave from winning over the true fundamental.
const PITCH_DIP: f64 = 0.15;
const MIN_PITCH_HZ: f64 = 25.0;
const MAX_PITCH_HZ: f64 = 4000.0;
/// Largest share of a peak's level a frequency between two bins can lose
/// under a Hann window, dB.
const MAX_BIN_LOSS_DB: f64 = 1.5;
const ENVELOPE_WINDOW_MS: f64 = 10.0;
const ENVELOPE_HOP_MS: f64 = 1.0;
const TRACK_POINTS: usize = 64;
const PITCH_TRACK_POINTS: usize = 32;
/// Long enough for two periods of [`MIN_PITCH_HZ`] at 48 kHz, so a track does
/// not see pitch moves much shorter than 85 ms.
const PITCH_FRAME: usize = 4096;
/// Over two periods of a 65 Hz note at 48 kHz: shorter, the centroid follows
/// each period of a low note instead of the sound's brightness.
const CENTROID_FRAME: usize = 2048;
/// Frames quieter than this relative to the loudest have no meaningful
/// pitch or brightness.
const TRACK_FLOOR: f64 = 0.01;
pub const SPECTROGRAM_COLUMNS: usize = 160;
pub const SPECTROGRAM_BANDS: usize = 48;
pub const SPECTROGRAM_MIN_HZ: f64 = 30.0;
const SPECTROGRAM_FRAME: usize = 2048;

/// Width of a Hann window's main lobe in bins: the bins a sine's power spreads
/// over, summed, read this many times its power.
const HANN_ENERGY_BINS: f64 = 1.5;
/// Last part of a sound compared to its peak.
const END_WINDOW_MS: f64 = 5.0;
/// Share of the peak where a sound counts as started.
const LEAD_IN_LEVEL: f64 = 0.01;

/// A frequency band.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub name: &'static str,
    pub low_hz: f64,
    pub high_hz: f64,
}

/// The bands energy is reported in.
pub const BANDS: [Band; 6] = [
    Band { name: "sub", low_hz: 20.0, high_hz: 60.0 },
    Band { name: "low", low_hz: 60.0, high_hz: 250.0 },
    Band { name: "lowMid", low_hz: 250.0, high_hz: 500.0 },
    Band { name: "mid", low_hz: 500.0, high_hz: 2000.0 },
    Band { name: "highMid", low_hz: 2000.0, high_hz: 6000.0 },
    Band { name: "high", low_hz: 6000.0, high_hz: f64::INFINITY },
];

/// A band's energy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandLevel {
    pub name: &'static str,
    /// Of the energy of all bands, 0 to 1.
    pub share: f64,
    /// A full-scale sine in the band reads 0 dB.
    pub db: f64,
}

/// The energy of each of [`BANDS`] in a power spectrum.
pub fn band_levels(power: &[f64], bin_hz: f64) -> Vec<BandLevel> {
    let energies: Vec<f64> = BANDS
        .iter()
        .map(|band| {
            let in_band = |i: &usize| (band.low_hz..band.high_hz).contains(&(*i as f64 * bin_hz));
            (0..power.len()).filter(in_band).map(|i| power[i]).sum()
        })
        .collect();
    let total: f64 = energies.iter().sum();
    BANDS
        .iter()
        .zip(energies)
        .map(|(band, energy)| BandLevel {
            name: band.name,
            share: if total > 0.0 { energy / total } else { 0.0 },
            db: amplitude_db((energy / HANN_ENERGY_BINS).sqrt()),
        })
        .collect()
}

/// The middle value; `None` when empty.
pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted.get(sorted.len() / 2).copied()
}

/// A frequency with its level, dB relative to the strongest bin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpectralPeak {
    pub hz: f64,
    pub db: f64,
}

/// A value at a time, milliseconds from the start of the sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimePoint {
    pub ms: f64,
    pub value: f64,
}

/// Everything measured on a sound. Levels are of the stereo signal, the rest
/// of its mono sum.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Measures {
    pub sample_rate: u32,
    pub duration_ms: f64,
    /// Highest absolute sample, either channel.
    pub peak: f64,
    pub rms_db: f64,
    pub dc: f64,
    /// Peak over RMS of the mono sum, dB.
    pub crest_db: f64,
    /// Frames at or over full scale.
    pub clipped_frames: usize,
    /// Left RMS over right RMS, dB.
    pub balance_db: f64,
    /// From 10% to 90% of the peak, on the samples themselves: a low note
    /// adds up to a quarter of its period.
    pub attack_ms: Option<f64>,
    /// Where the 10 ms RMS envelope is highest.
    pub envelope_peak_ms: Option<f64>,
    /// From the envelope's peak to its last point within 20 dB of it.
    pub decay_20db_ms: Option<f64>,
    /// The envelope's last point within 60 dB of its peak.
    pub end_ms: Option<f64>,
    pub centroid_hz: Option<f64>,
    /// Frequency under which 85% of the energy lies.
    pub rolloff_hz: Option<f64>,
    /// Geometric over arithmetic mean of the power: near 0 for tones, higher
    /// for noise.
    pub flatness: Option<f64>,
    pub peaks: Vec<SpectralPeak>,
    /// Period of the loudest frame, 25 to 4000 Hz (YIN).
    pub fundamental_hz: Option<f64>,
    pub note: Option<String>,
    pub pitch_confidence: f64,
    /// Level of each multiple of the fundamental, dB relative to the first.
    pub harmonics_db: Vec<f64>,
    /// Level an octave under the fundamental, dB relative to the fundamental.
    pub sub_octave_db: Option<f64>,
    pub pitch_track: Vec<TimePoint>,
    /// Highest over lowest tracked pitch, cents.
    pub pitch_range_cents: Option<f64>,
    pub centroid_track: Vec<TimePoint>,
    /// Highest over lowest tracked centroid, octaves.
    pub brightness_range_octaves: Option<f64>,
    /// RMS, dB, every millisecond.
    pub envelope_db: Vec<TimePoint>,
    /// Bins from 0 Hz, each `spectrum_bin_hz` wide, dB relative to the
    /// strongest, power averaged over the sound.
    pub spectrum_db: Vec<f64>,
    pub spectrum_bin_hz: f64,
    pub bands: Vec<BandLevel>,
    /// Before the sound reaches 1% of its peak.
    pub lead_in_ms: f64,
    /// Last 5 ms over the peak, dB: near 0 for a sound that is cut off.
    pub end_level_db: Option<f64>,
    /// Columns of [`SPECTROGRAM_BANDS`] log-spaced bands, dB relative to the
    /// strongest cell.
    pub spectrogram_db: Vec<Vec<f64>>,
}

/// Amplitude to dB; -∞ for silence.
pub fn amplitude_db(value: f64) -> f64 {
    if value > 0.0 { 20.0 * value.log10() } else { f64::NEG_INFINITY }
}

fn power_db(power: f64, reference: f64) -> f64 {
    if power > 0.0 && reference > 0.0 { (10.0 * (power / reference).log10()).max(FLOOR_DB) } else { FLOOR_DB }
}

fn rms(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f64>() / samples.len() as f64).sqrt()
}

/// Power per bin over the whole sound, frame powers averaged.
fn average_spectrum(samples: &[f64]) -> Vec<f64> {
    let mut total = vec![0.0; SPECTRUM_SIZE / 2 + 1];
    let last = samples.len().saturating_sub(SPECTRUM_SIZE);
    let mut frame_count = 0.0;
    for start in (0..=last).step_by(SPECTRUM_HOP) {
        let end = (start + SPECTRUM_SIZE).min(samples.len());
        for (sum, power) in total.iter_mut().zip(power_spectrum(&samples[start..end], SPECTRUM_SIZE)) {
            *sum += power;
        }
        frame_count += 1.0;
    }
    total.iter_mut().for_each(|p| *p /= frame_count);
    total
}

/// Energy-weighted mean frequency of the bins from `first_bin` on. Weighing
/// by amplitude would let thousands of quiet bins outweigh the sound.
fn centroid(power: &[f64], bin_hz: f64, first_bin: usize) -> Option<f64> {
    let (mut weighted, mut sum) = (0.0, 0.0);
    for (i, p) in power.iter().enumerate().skip(first_bin) {
        weighted += p * i as f64 * bin_hz;
        sum += p;
    }
    (sum > 0.0).then(|| weighted / sum)
}

/// The strongest local maxima, strongest first, their frequency refined from
/// their neighbours.
fn spectral_peaks(db: &[f64], bin_hz: f64, first_bin: usize) -> Vec<SpectralPeak> {
    let mut candidates: Vec<(usize, f64)> = (first_bin.max(1)..db.len().saturating_sub(1))
        .filter(|&i| db[i] >= PEAK_FLOOR_DB && db[i] > db[i - 1] && db[i] >= db[i + 1])
        .map(|i| (i, db[i]))
        .collect();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut chosen: Vec<(usize, f64)> = Vec::new();
    for candidate in candidates {
        if chosen.len() == PEAK_COUNT {
            break;
        }
        if chosen.iter().all(|c| c.0.abs_diff(candidate.0) >= PEAK_SPACING_BINS) {
            chosen.push(candidate);
        }
    }
    chosen
        .into_iter()
        .map(|(i, level)| {
            let (a, b, c) = (db[i - 1], db[i], db[i + 1]);
            let bend = a - 2.0 * b + c;
            let offset = if bend != 0.0 { 0.5 * (a - c) / bend } else { 0.0 };
            SpectralPeak { hz: (i as f64 + offset) * bin_hz, db: level }
        })
        .collect()
}

/// Level around `hz`, dB relative to the strongest bin; `None` above Nyquist.
fn level_at(db: &[f64], bin_hz: f64, hz: f64) -> Option<f64> {
    let bin = (hz / bin_hz).round() as usize;
    if bin + 1 >= db.len() {
        return None;
    }
    let low = bin.saturating_sub(1);
    let strongest = (low..=bin + 1).fold(low, |best, i| if db[i] > db[best] { i } else { best });
    if strongest == 0 || strongest + 1 >= db.len() {
        return Some(db[strongest]);
    }
    // A frequency between two bins reads low: the top of the parabola through
    // the three bins around it is its level.
    let (a, b, c) = (db[strongest - 1], db[strongest], db[strongest + 1]);
    let bend = a - 2.0 * b + c;
    let gain = if bend < 0.0 { -(a - c) * (a - c) / (8.0 * bend) } else { 0.0 };
    Some(b + gain.min(MAX_BIN_LOSS_DB))
}

/// YIN over `frame`: the frequency of its period and how periodic it is, 0 to
/// 1; the frequency is `None` under [`MIN_PITCH_CONFIDENCE`].
fn detect_pitch(frame: &[f64], rate: f64) -> (Option<f64>, f64) {
    let max_lag = ((rate / MIN_PITCH_HZ) as usize).min(frame.len() / 2);
    let min_lag = ((rate / MAX_PITCH_HZ) as usize).max(2);
    if max_lag <= min_lag + 1 {
        return (None, 0.0);
    }
    let compared = frame.len() - max_lag;
    let mut normalized = vec![1.0; max_lag + 1];
    let mut running = 0.0;
    for lag in 1..=max_lag {
        let difference: f64 = (0..compared).map(|j| (frame[j] - frame[j + lag]).powi(2)).sum();
        running += difference;
        normalized[lag] = if running > 0.0 { difference * lag as f64 / running } else { 1.0 };
    }
    let mut best = min_lag;
    for lag in min_lag..max_lag {
        if normalized[lag] < PITCH_DIP {
            best = lag;
            while best < max_lag - 1 && normalized[best + 1] < normalized[best] {
                best += 1;
            }
            break;
        }
        if normalized[lag] < normalized[best] {
            best = lag;
        }
    }
    let confidence = (1.0 - normalized[best]).clamp(0.0, 1.0);
    if confidence < MIN_PITCH_CONFIDENCE {
        return (None, confidence);
    }
    let (a, b, c) = (normalized[best - 1], normalized[best], normalized[best + 1]);
    let bend = a - 2.0 * b + c;
    let offset = if bend > 0.0 { (0.5 * (a - c) / bend).clamp(-1.0, 1.0) } else { 0.0 };
    (Some(rate / (best as f64 + offset)), confidence)
}

/// The loudest [`PITCH_FRAME`] samples of the sound, or all of it.
fn loudest_frame(samples: &[f64]) -> &[f64] {
    if samples.len() <= PITCH_FRAME {
        return samples;
    }
    let energy = |start: usize| samples[start..start + PITCH_FRAME].iter().map(|s| s * s).sum::<f64>();
    let starts = (0..=samples.len() - PITCH_FRAME).step_by(PITCH_FRAME / 8);
    let best = starts.map(|start| (start, energy(start))).max_by(|a, b| a.1.total_cmp(&b.1));
    let start = best.map_or(0, |(start, _)| start);
    &samples[start..start + PITCH_FRAME]
}

/// Start of each frame of `frame` samples, at most `count`, evenly over the sound.
fn frame_starts(length: usize, frame: usize, count: usize) -> Vec<usize> {
    if length < frame {
        return Vec::new();
    }
    let hop = ((length - frame) / count.max(1)).max(frame / 8).max(1);
    (0..=length - frame).step_by(hop).collect()
}

/// Pitch and centroid over time, on the frames loud enough to have them.
fn tracks(samples: &[f64], rate: u32) -> (Vec<TimePoint>, Vec<TimePoint>) {
    let rate_f = f64::from(rate);
    let loudest = samples.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    let time_of = |start: usize, frame: usize| (start as f64 + frame as f64 / 2.0) * 1000.0 / rate_f;
    let loud = |frame: &[f64]| frame.iter().fold(0.0f64, |m, s| m.max(s.abs())) >= loudest * TRACK_FLOOR;

    let mut pitch = Vec::new();
    for start in frame_starts(samples.len(), PITCH_FRAME, PITCH_TRACK_POINTS) {
        let frame = &samples[start..start + PITCH_FRAME];
        if !loud(frame) {
            continue;
        }
        if let (Some(hz), _) = detect_pitch(frame, rate_f) {
            pitch.push(TimePoint { ms: time_of(start, PITCH_FRAME), value: hz });
        }
    }
    let bin_hz = rate_f / CENTROID_FRAME as f64;
    let first_bin = (MIN_SPECTRUM_HZ / bin_hz).ceil() as usize;
    let mut brightness = Vec::new();
    for start in frame_starts(samples.len(), CENTROID_FRAME, TRACK_POINTS) {
        let frame = &samples[start..start + CENTROID_FRAME];
        if !loud(frame) {
            continue;
        }
        if let Some(hz) = centroid(&power_spectrum(frame, CENTROID_FRAME), bin_hz, first_bin) {
            brightness.push(TimePoint { ms: time_of(start, CENTROID_FRAME), value: hz });
        }
    }
    (pitch, brightness)
}

/// `f(highest / lowest)` of a track's values; `None` under two points.
fn track_range(track: &[TimePoint], f: impl Fn(f64) -> f64) -> Option<f64> {
    if track.len() < 2 {
        return None;
    }
    let low = track.iter().fold(f64::INFINITY, |m, p| m.min(p.value));
    let high = track.iter().fold(0.0f64, |m, p| m.max(p.value));
    (low > 0.0).then(|| f(high / low))
}

/// Frequency of the lower edge of a spectrogram band.
pub fn band_hz(band: usize, nyquist: f64) -> f64 {
    SPECTROGRAM_MIN_HZ * (nyquist / SPECTROGRAM_MIN_HZ).powf(band as f64 / SPECTROGRAM_BANDS as f64)
}

fn spectrogram(samples: &[f64], rate: f64) -> Vec<Vec<f64>> {
    let bin_hz = rate / SPECTROGRAM_FRAME as f64;
    let nyquist = rate / 2.0;
    let hop = (samples.len() / SPECTROGRAM_COLUMNS).max(1);
    let powers: Vec<Vec<f64>> = (0..SPECTROGRAM_COLUMNS.min(samples.len()))
        .map(|column| {
            let start = column * hop;
            let end = (start + SPECTROGRAM_FRAME).min(samples.len());
            let power = power_spectrum(&samples[start..end], SPECTROGRAM_FRAME);
            (0..SPECTROGRAM_BANDS)
                .map(|band| {
                    let low = (band_hz(band, nyquist) / bin_hz).round() as usize;
                    let high = ((band_hz(band + 1, nyquist) / bin_hz).round() as usize).clamp(low, power.len() - 1);
                    power[low.min(high)..=high].iter().copied().fold(0.0, f64::max)
                })
                .collect()
        })
        .collect();
    let strongest = powers.iter().flatten().copied().fold(0.0, f64::max);
    powers.iter().map(|column| column.iter().map(|&p| power_db(p, strongest)).collect()).collect()
}

/// Measure `buffer`.
pub fn measure(buffer: &SampleBuffer) -> Measures {
    let rate = f64::from(buffer.sample_rate);
    let left = &buffer.left;
    let right = buffer.right.as_deref().unwrap_or(left);
    let frames = left.len().min(right.len());
    let samples: Vec<f64> = (0..frames).map(|i| (f64::from(left[i]) + f64::from(right[i])) * 0.5).collect();
    let mut result =
        Measures { sample_rate: buffer.sample_rate, duration_ms: frames as f64 * 1000.0 / rate, ..Measures::default() };
    let wide = |channel: &[f32]| channel[..frames].iter().map(|&s| f64::from(s)).collect::<Vec<f64>>();
    let (left_wide, right_wide) = (wide(left), wide(right));
    result.peak = left_wide.iter().chain(&right_wide).fold(0.0, |m: f64, s| m.max(s.abs()));
    result.clipped_frames = (0..frames).filter(|&i| left_wide[i].abs() >= 1.0 || right_wide[i].abs() >= 1.0).count();
    let total_rms = rms(&samples);
    result.rms_db = amplitude_db(total_rms);
    if frames == 0 || result.peak == 0.0 {
        result.balance_db = 0.0;
        return result;
    }
    result.dc = samples.iter().sum::<f64>() / frames as f64;
    let mono_peak = samples.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    result.crest_db = if total_rms > 0.0 { amplitude_db(mono_peak / total_rms) } else { 0.0 };
    let (left_rms, right_rms) = (rms(&left_wide), rms(&right_wide));
    result.balance_db = if left_rms > 0.0 && right_rms > 0.0 { amplitude_db(left_rms / right_rms) } else { 0.0 };

    if mono_peak > 0.0 {
        let first_at = |level: f64| samples.iter().position(|s| s.abs() >= level * mono_peak);
        if let (Some(low), Some(high)) = (first_at(0.1), first_at(0.9)) {
            result.attack_ms = Some((high - low) as f64 * 1000.0 / rate);
        }
    }
    measure_envelope(&samples, rate, &mut result);
    if mono_peak > 0.0 {
        let started = samples.iter().position(|s| s.abs() >= LEAD_IN_LEVEL * mono_peak).unwrap_or(0);
        result.lead_in_ms = started as f64 * 1000.0 / rate;
        let window = ((END_WINDOW_MS * rate / 1000.0) as usize).clamp(1, frames);
        result.end_level_db = Some(amplitude_db(rms(&samples[frames - window..]) / mono_peak));
    }

    let power = average_spectrum(&samples);
    let bin_hz = rate / SPECTRUM_SIZE as f64;
    result.spectrum_bin_hz = bin_hz;
    let first_bin = (MIN_SPECTRUM_HZ / bin_hz).ceil() as usize;
    let strongest = power[first_bin..].iter().copied().fold(0.0, f64::max);
    let db: Vec<f64> = power.iter().map(|&p| power_db(p, strongest)).collect();
    result.centroid_hz = centroid(&power, bin_hz, first_bin);
    let energy: f64 = power[first_bin..].iter().sum();
    let mut running = 0.0;
    result.rolloff_hz = power.iter().enumerate().skip(first_bin).find_map(|(i, p)| {
        running += p;
        (energy > 0.0 && running >= energy * ROLLOFF_ENERGY).then_some(i as f64 * bin_hz)
    });
    let count = (power.len() - first_bin) as f64;
    if energy > 0.0 {
        let log_mean = power[first_bin..].iter().map(|p| p.max(1e-30).ln()).sum::<f64>() / count;
        result.flatness = Some(log_mean.exp() / (energy / count));
    }
    result.peaks = spectral_peaks(&db, bin_hz, first_bin);

    (result.fundamental_hz, result.pitch_confidence) = detect_pitch(loudest_frame(&samples), rate);
    if let Some(f0) = result.fundamental_hz {
        result.note = hz_to_note(f0).map(|note| note.to_string());
        if let Some(reference) = level_at(&db, bin_hz, f0) {
            result.harmonics_db = (1..=HARMONIC_COUNT)
                .map_while(|k| level_at(&db, bin_hz, f0 * k as f64))
                .map(|level| level - reference)
                .collect();
            if f0 / 2.0 >= MIN_SPECTRUM_HZ {
                result.sub_octave_db = level_at(&db, bin_hz, f0 / 2.0).map(|level| level - reference);
            }
        }
    }
    result.spectrum_db = db;
    result.bands = band_levels(&power, bin_hz);

    (result.pitch_track, result.centroid_track) = tracks(&samples, buffer.sample_rate);
    result.pitch_range_cents = track_range(&result.pitch_track, |ratio| 1200.0 * ratio.log2());
    result.brightness_range_octaves = track_range(&result.centroid_track, f64::log2);
    result.spectrogram_db = spectrogram(&samples, rate);
    result
}

/// The RMS envelope and the times read from it.
fn measure_envelope(samples: &[f64], rate: f64, result: &mut Measures) {
    let window = ((ENVELOPE_WINDOW_MS * rate / 1000.0) as usize).clamp(1, samples.len());
    let hop = ((ENVELOPE_HOP_MS * rate / 1000.0) as usize).max(1);
    let mut cumulative = Vec::with_capacity(samples.len() + 1);
    cumulative.push(0.0);
    for s in samples {
        cumulative.push(cumulative[cumulative.len() - 1] + s * s);
    }
    let levels: Vec<TimePoint> = (0..=samples.len() - window)
        .step_by(hop)
        .map(|start| {
            let energy = (cumulative[start + window] - cumulative[start]).max(0.0);
            TimePoint {
                ms: (start as f64 + window as f64 / 2.0) * 1000.0 / rate,
                value: (energy / window as f64).sqrt(),
            }
        })
        .collect();
    let Some(top) = levels.iter().copied().max_by(|a, b| a.value.total_cmp(&b.value)) else { return };
    if top.value <= 0.0 {
        return;
    }
    let last_within = |ratio: f64| levels.iter().rev().find(|p| p.value >= top.value * ratio).map(|p| p.ms);
    result.envelope_peak_ms = Some(top.ms);
    result.decay_20db_ms = last_within(0.1).map(|ms| ms - top.ms);
    result.end_ms = last_within(0.001);
    result.envelope_db = levels.iter().map(|p| TimePoint { ms: p.ms, value: amplitude_db(p.value) }).collect();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: u32 = 48_000;

    fn signal(seconds: f64, f: impl Fn(f64) -> f64) -> SampleBuffer {
        let frames = (seconds * f64::from(RATE)) as usize;
        SampleBuffer::mono((0..frames).map(|i| f(i as f64 / f64::from(RATE)) as f32).collect(), RATE)
    }

    #[test]
    fn a_sine_has_one_harmonic_and_the_right_levels() {
        let m = measure(&signal(0.5, |t| 0.5 * (TAU * 440.0 * t).sin()));
        assert!((m.peak - 0.5).abs() < 1e-3);
        assert!((m.rms_db - amplitude_db(0.5 / 2f64.sqrt())).abs() < 0.01);
        assert!((m.crest_db - 3.01).abs() < 0.05);
        assert!(m.dc.abs() < 1e-3);
        assert_eq!(m.clipped_frames, 0);
        assert!((m.fundamental_hz.unwrap() - 440.0).abs() < 2.0);
        assert!(m.note.as_deref().unwrap().starts_with("A4"));
        assert!((m.peaks[0].hz - 440.0).abs() < 1.0);
        assert!((m.centroid_hz.unwrap() - 440.0).abs() < 25.0, "{:?}", m.centroid_hz);
        assert_eq!(m.harmonics_db[0], 0.0);
        assert!(m.harmonics_db[1] < -60.0);
        assert!(m.sub_octave_db.unwrap() < -60.0);
    }

    #[test]
    fn a_low_dull_tone_is_found_and_not_its_octave() {
        let low = measure(&signal(1.0, |t| (TAU * 32.7 * t).sin() + 0.1 * (TAU * 65.4 * t).sin()));
        assert!((low.fundamental_hz.unwrap() - 32.7).abs() < 0.5, "{:?}", low.fundamental_hz);
        assert!(low.pitch_range_cents.unwrap() < 20.0, "{:?}", low.pitch_range_cents);
        let strong_octave = measure(&signal(1.0, |t| 0.3 * (TAU * 65.4 * t).sin() + (TAU * 130.8 * t).sin()));
        assert!((strong_octave.fundamental_hz.unwrap() - 65.4).abs() < 0.5, "{:?}", strong_octave.fundamental_hz);
    }

    #[test]
    fn bands_read_their_share_and_level() {
        let m = measure(&signal(0.5, |t| 0.5 * (TAU * 1000.0 * t).sin() + 0.25 * (TAU * 100.0 * t).sin()));
        let band = |name: &str| *m.bands.iter().find(|b| b.name == name).unwrap();
        assert!((band("mid").db + 6.02).abs() < 0.3, "{:?}", m.bands);
        assert!((band("low").db + 12.04).abs() < 0.3, "{:?}", m.bands);
        assert!((band("mid").share - 0.8).abs() < 0.01);
        assert!(band("high").share < 1e-6);
    }

    #[test]
    fn the_start_and_the_end_are_read() {
        let late =
            measure(&signal(
                0.2,
                |t| if t < 0.02 { 0.0 } else { (-(t - 0.02) / 0.01).exp() * (TAU * 1000.0 * t).sin() },
            ));
        assert!((late.lead_in_ms - 20.0).abs() < 0.5, "{}", late.lead_in_ms);
        assert!(late.end_level_db.unwrap() < -60.0);
        let cut = measure(&signal(0.2, |t| (TAU * 1000.0 * t).sin()));
        assert!(cut.end_level_db.unwrap() > -6.0, "{:?}", cut.end_level_db);
    }

    #[test]
    fn no_level_rises_above_what_a_bin_can_lose() {
        // A bin next to the floor would otherwise bend the parabola far up.
        let db = [-120.0, -120.0, -100.0, -120.0];
        assert!(level_at(&db, 1.0, 2.0).unwrap() <= -100.0 + MAX_BIN_LOSS_DB);
    }

    #[test]
    fn harmonics_read_their_levels() {
        let m = measure(&signal(0.5, |t| 0.5 * (TAU * 200.0 * t).sin() + 0.25 * (TAU * 400.0 * t).sin()));
        assert!((m.harmonics_db[1] + 6.02).abs() < 0.3, "{:?}", m.harmonics_db);
    }

    #[test]
    fn a_rectified_sine_shows_dc_and_nothing_an_octave_under() {
        let m = measure(&signal(0.5, |t| (TAU * 50.0 * t).sin().abs()));
        assert!((m.dc - 2.0 / std::f64::consts::PI).abs() < 0.01);
        assert!((m.fundamental_hz.unwrap() - 100.0).abs() < 1.0);
        assert!(m.sub_octave_db.unwrap() < -60.0);
    }

    #[test]
    fn attack_and_decay_are_timed() {
        let attack = measure(&signal(0.2, |t| (t / 0.02).min(1.0) * (TAU * 1000.0 * t).sin()));
        assert!((attack.attack_ms.unwrap() - 16.0).abs() < 1.5, "{:?}", attack.attack_ms);
        // 20 dB every 100 ms.
        let decay = measure(&signal(0.6, |t| 10f64.powf(-t / 0.1) * (TAU * 1000.0 * t).sin()));
        assert!((decay.decay_20db_ms.unwrap() - 100.0).abs() < 3.0, "{:?}", decay.decay_20db_ms);
        assert!((decay.end_ms.unwrap() - 300.0).abs() < 8.0, "{:?}", decay.end_ms);
    }

    #[test]
    fn a_sweep_moves_pitch_and_brightness() {
        // 200 to 400 Hz over the second: the phase is the integral of the frequency.
        let m = measure(&signal(1.0, |t| (TAU * 200.0 * (2f64.powf(t) - 1.0) / std::f64::consts::LN_2).sin()));
        assert!((m.pitch_range_cents.unwrap() - 1200.0).abs() < 150.0, "{:?}", m.pitch_range_cents);
        assert!((m.brightness_range_octaves.unwrap() - 1.0).abs() < 0.4, "{:?}", m.brightness_range_octaves);
        let steady = measure(&signal(1.0, |t| (TAU * 300.0 * t).sin()));
        assert!(steady.pitch_range_cents.unwrap() < 20.0);
    }

    #[test]
    fn noise_is_flatter_than_a_tone() {
        let mut state = 12_345u32;
        let noise: Vec<f32> = (0..24_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (f64::from(state) / f64::from(u32::MAX) * 2.0 - 1.0) as f32
            })
            .collect();
        let noisy = measure(&SampleBuffer::mono(noise, RATE));
        let tone = measure(&signal(0.5, |t| (TAU * 440.0 * t).sin()));
        assert!(noisy.flatness.unwrap() > 0.3 && tone.flatness.unwrap() < 0.01);
        assert!(noisy.fundamental_hz.is_none());
    }

    #[test]
    fn clipping_balance_and_silence() {
        let hot = measure(&SampleBuffer {
            left: vec![1.2, 0.5, 1.0, 0.0],
            right: Some(vec![0.6, 0.25, 0.5, 0.0]),
            sample_rate: RATE,
        });
        assert_eq!(hot.clipped_frames, 2);
        assert!((hot.balance_db - 6.02).abs() < 0.01);
        let silent = measure(&SampleBuffer::mono(vec![0.0; 1000], RATE));
        assert_eq!(silent.peak, 0.0);
        assert_eq!(silent.rms_db, f64::NEG_INFINITY);
        assert_eq!(silent.attack_ms, None);
        assert_eq!(measure(&SampleBuffer::mono(Vec::new(), RATE)).duration_ms, 0.0);
    }
}
