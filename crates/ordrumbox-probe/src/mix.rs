//! Measurements of a whole mix, where a note's attack or pitch mean nothing:
//! loudness (ITU-R BS.1770, EBU R128), true peak, and each hit found in it.

use std::f64::consts::PI;

use ordrumbox_dsp::SampleBuffer;

use crate::fft::power_spectrum;
use crate::measure::{BANDS, amplitude_db, band_levels, median};

const MOMENTARY_SECONDS: f64 = 0.4;
const SHORT_TERM_SECONDS: f64 = 3.0;
const BLOCK_HOP_SECONDS: f64 = 0.1;
const ABSOLUTE_GATE_LUFS: f64 = -70.0;
const INTEGRATED_RELATIVE_GATE_LU: f64 = 10.0;
const RANGE_RELATIVE_GATE_LU: f64 = 20.0;
const RANGE_LOW_PERCENTILE: f64 = 0.10;
const RANGE_HIGH_PERCENTILE: f64 = 0.95;
/// BS.1770's offset, so that the K-weighting reads 0 dB at 997 Hz.
const LOUDNESS_OFFSET: f64 = -0.691;
/// Each sample gets three points between it and the next.
const OVERSAMPLING: usize = 4;
/// Samples each side of an interpolated point.
const TRUE_PEAK_REACH: usize = 6;
const FLUX_FRAME: usize = 1024;
const FLUX_HOP: usize = 256;
/// Compression before the flux, so a quiet hat can stand beside a kick.
const FLUX_COMPRESSION: f64 = 100.0;
const FLUX_MEDIAN_SECONDS: f64 = 0.25;
/// Share of the largest flux a frame must rise above its neighbourhood.
const FLUX_THRESHOLD: f64 = 0.04;
const MIN_HIT_GAP_SECONDS: f64 = 0.03;
const ONSET_WINDOW_SECONDS: f64 = 0.001;
const ONSET_RISE_SHARE: f64 = 0.3;
const HIT_LENGTH_SECONDS: f64 = 0.1;
/// Deviation from the grid past which a hit counts as off it.
const OFF_GRID_MS: f64 = 10.0;

/// One hit of a mix.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub ms: f64,
    pub peak: f64,
    /// Band holding most of the energy of its first 100 ms.
    pub band: &'static str,
}

/// The hits of one band.
#[derive(Debug, Clone, PartialEq)]
pub struct BandHits {
    pub band: &'static str,
    pub count: usize,
    /// Loudest over quietest hit, dB.
    pub level_spread_db: f64,
}

/// How hits sit on a step grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridFit {
    /// Median deviation from the grid: the samples' lead-in, not an error.
    pub offset_ms: f64,
    /// Largest deviation from that median.
    pub max_deviation_ms: f64,
    /// Hits over 10 ms from it: swing plays them there on purpose.
    pub off_grid_count: usize,
}

/// The measurements of a mix.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MixMeasures {
    /// `None` when under 400 ms or silent.
    pub integrated_lufs: Option<f64>,
    pub max_momentary_lufs: Option<f64>,
    /// `None` under 3 s.
    pub max_short_term_lufs: Option<f64>,
    pub loudness_range_lu: Option<f64>,
    pub true_peak: f64,
    /// True peak over integrated loudness, dB.
    pub peak_to_loudness_db: Option<f64>,
    pub hits: Vec<Hit>,
    pub band_hits: Vec<BandHits>,
    pub grid: Option<GridFit>,
}

/// A second-order filter, direct form.
fn biquad(input: &[f64], b: [f64; 3], a: [f64; 2]) -> Vec<f64> {
    let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
    input
        .iter()
        .map(|&x| {
            let y = b[0] * x + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
            (x2, x1, y2, y1) = (x1, x, y1, y);
            y
        })
        .collect()
}

/// BS.1770's K-weighting at any rate: the two stages' analog prototypes (their
/// constants fit the 48 kHz coefficients of the standard) through the bilinear
/// transform.
fn k_weighted(channel: &[f64], rate: f64) -> Vec<f64> {
    let k = (PI * 1_681.974_450_955_533 / rate).tan();
    let q = 0.707_175_236_955_419_6;
    let vh = 10f64.powf(3.999_843_853_973_347 / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let shelf = biquad(
        channel,
        [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
    );
    let k = (PI * 38.135_470_876_024_44 / rate).tan();
    let q = 0.500_327_037_323_877_3;
    let a0 = 1.0 + k / q + k * k;
    biquad(&shelf, [1.0, -2.0, 1.0], [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0])
}

/// Mean energy over all channels of each block of `seconds`, every 100 ms.
fn block_energies(weighted: &[Vec<f64>], rate: f64, seconds: f64) -> Vec<f64> {
    let window = (seconds * rate) as usize;
    let hop = ((BLOCK_HOP_SECONDS * rate) as usize).max(1);
    let frames = weighted.first().map_or(0, Vec::len);
    if window == 0 || frames < window {
        return Vec::new();
    }
    let mut totals = vec![0.0; (frames - window) / hop + 1];
    for channel in weighted {
        let mut cumulative = Vec::with_capacity(frames + 1);
        cumulative.push(0.0);
        for s in channel {
            cumulative.push(cumulative[cumulative.len() - 1] + s * s);
        }
        for (block, total) in totals.iter_mut().enumerate() {
            let start = block * hop;
            *total += (cumulative[start + window] - cumulative[start]).max(0.0) / window as f64;
        }
    }
    totals
}

fn lufs(energy: f64) -> f64 {
    if energy > 0.0 { LOUDNESS_OFFSET + 10.0 * energy.log10() } else { f64::NEG_INFINITY }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// Blocks above the absolute gate and within `relative_lu` of their mean.
fn gated(blocks: &[f64], relative_lu: f64) -> Vec<f64> {
    let loud: Vec<f64> = blocks.iter().copied().filter(|&e| lufs(e) > ABSOLUTE_GATE_LUFS).collect();
    let Some(average) = mean(&loud) else { return Vec::new() };
    let gate = lufs(average) - relative_lu;
    loud.into_iter().filter(|&e| lufs(e) > gate).collect()
}

fn loudest(blocks: &[f64]) -> Option<f64> {
    blocks.iter().copied().fold(None, |best: Option<f64>, e| Some(best.map_or(e, |b| b.max(e)))).map(lufs)
}

/// EBU Tech 3342's loudness range: the 10th to 95th percentile of the gated
/// short-term loudness.
fn loudness_range(blocks: &[f64]) -> Option<f64> {
    let mut levels: Vec<f64> = gated(blocks, RANGE_RELATIVE_GATE_LU).into_iter().map(lufs).collect();
    if levels.is_empty() {
        return None;
    }
    levels.sort_by(f64::total_cmp);
    let at = |share: f64| levels[((levels.len() - 1) as f64 * share).round() as usize];
    Some(at(RANGE_HIGH_PERCENTILE) - at(RANGE_LOW_PERCENTILE))
}

/// Highest absolute value of `channel` and of the points between its samples.
fn true_peak(channel: &[f64]) -> f64 {
    let reach = TRUE_PEAK_REACH as f64;
    let taps = 2 * TRUE_PEAK_REACH;
    let phases: Vec<Vec<f64>> = (1..OVERSAMPLING)
        .map(|phase| {
            let fraction = phase as f64 / OVERSAMPLING as f64;
            let raw: Vec<f64> = (0..taps)
                .map(|tap| {
                    let t = tap as f64 + 1.0 - reach - fraction;
                    let sinc = (PI * t).sin() / (PI * t);
                    sinc * (0.5 + 0.5 * (PI * t / reach).cos())
                })
                .collect();
            let sum: f64 = raw.iter().sum();
            raw.iter().map(|h| h / sum).collect()
        })
        .collect();
    let mut peak = channel.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    for window in channel.windows(taps) {
        for phase in &phases {
            let value: f64 = phase.iter().zip(window).map(|(h, s)| h * s).sum();
            peak = peak.max(value.abs());
        }
    }
    peak
}

/// Where the energy starts to rise near `center`: finer than a flux frame. The
/// first rise of a good share of the largest one, since a sound's strongest
/// rise can come well after its start.
fn refine_onset(samples: &[f64], center: usize, rate: f64) -> usize {
    let window = ((ONSET_WINDOW_SECONDS * rate) as usize).max(1);
    let from = center.saturating_sub(FLUX_FRAME).max(window);
    let to = (center + FLUX_FRAME / 2).min(samples.len().saturating_sub(window));
    let energy = |range: &[f64]| range.iter().map(|s| s * s).sum::<f64>();
    let rises: Vec<(usize, f64)> = (from..to)
        .step_by((window / 4).max(1))
        .map(|position| {
            (position, energy(&samples[position..position + window]) - energy(&samples[position - window..position]))
        })
        .collect();
    let largest = rises.iter().fold(0.0f64, |m, (_, rise)| m.max(*rise));
    let first = rises.iter().find(|(_, rise)| largest > 0.0 && *rise >= largest * ONSET_RISE_SHARE);
    first.map_or(center.min(samples.len().saturating_sub(1)), |(position, _)| *position)
}

/// The hits of `samples`: peaks of the spectral flux over its local median.
fn find_hits(samples: &[f64], rate: f64) -> Vec<Hit> {
    if samples.len() < FLUX_FRAME {
        return Vec::new();
    }
    let mut previous = vec![0.0; FLUX_FRAME / 2 + 1];
    let mut flux = Vec::new();
    for start in (0..=samples.len() - FLUX_FRAME).step_by(FLUX_HOP) {
        let compressed: Vec<f64> = power_spectrum(&samples[start..start + FLUX_FRAME], FLUX_FRAME)
            .iter()
            .map(|p| (1.0 + FLUX_COMPRESSION * p.sqrt()).ln())
            .collect();
        flux.push(compressed.iter().zip(&previous).map(|(now, before)| (now - before).max(0.0)).sum::<f64>());
        previous = compressed;
    }
    let largest = flux.iter().copied().fold(0.0, f64::max);
    if largest <= 0.0 {
        return Vec::new();
    }
    let reach = ((FLUX_MEDIAN_SECONDS * rate) as usize / FLUX_HOP).max(1);
    let gap = (((MIN_HIT_GAP_SECONDS * rate) as usize) / FLUX_HOP).max(1);
    let mut onsets: Vec<usize> = Vec::new();
    for frame in 0..flux.len() {
        let near = &flux[frame.saturating_sub(gap)..(frame + gap + 1).min(flux.len())];
        let is_top = near.iter().all(|&f| f <= flux[frame]);
        let local = &flux[frame.saturating_sub(reach)..(frame + reach + 1).min(flux.len())];
        if !is_top || flux[frame] < median(local).unwrap_or(0.0) + largest * FLUX_THRESHOLD {
            continue;
        }
        let onset = refine_onset(samples, frame * FLUX_HOP + FLUX_FRAME / 2, rate);
        if onsets.last().is_none_or(|last| onset as f64 - *last as f64 >= MIN_HIT_GAP_SECONDS * rate) {
            onsets.push(onset);
        }
    }
    let longest = (HIT_LENGTH_SECONDS * rate) as usize;
    let size = longest.next_power_of_two();
    let bin_hz = rate / size as f64;
    onsets
        .iter()
        .enumerate()
        .map(|(i, &onset)| {
            let next = onsets.get(i + 1).copied().unwrap_or(samples.len());
            let segment = &samples[onset..(onset + longest).min(next).max(onset + 1).min(samples.len())];
            let levels = band_levels(&power_spectrum(segment, size), bin_hz);
            let band =
                levels.iter().max_by(|a, b| a.share.total_cmp(&b.share)).map_or(BANDS[0].name, |level| level.name);
            let peak = segment.iter().fold(0.0f64, |m, s| m.max(s.abs()));
            Hit { ms: onset as f64 * 1000.0 / rate, peak, band }
        })
        .collect()
}

fn band_statistics(hits: &[Hit]) -> Vec<BandHits> {
    BANDS
        .iter()
        .filter_map(|band| {
            let peaks: Vec<f64> = hits.iter().filter(|hit| hit.band == band.name).map(|hit| hit.peak).collect();
            let high = peaks.iter().copied().fold(0.0, f64::max);
            let low = peaks.iter().copied().fold(f64::INFINITY, f64::min);
            (!peaks.is_empty()).then(|| BandHits {
                band: band.name,
                count: peaks.len(),
                level_spread_db: if low > 0.0 { amplitude_db(high / low) } else { 0.0 },
            })
        })
        .collect()
}

/// How `hits` sit on a grid of steps `step_ms` long.
fn grid_fit(hits: &[Hit], step_ms: f64) -> Option<GridFit> {
    let deviation = |ms: f64| ms - (ms / step_ms).round() * step_ms;
    let deviations: Vec<f64> = hits.iter().map(|hit| deviation(hit.ms)).collect();
    let offset = median(&deviations)?;
    let from_offset = deviations.iter().map(|d| (d - offset).abs());
    Some(GridFit {
        offset_ms: offset,
        max_deviation_ms: from_offset.clone().fold(0.0, f64::max),
        off_grid_count: from_offset.filter(|d| *d > OFF_GRID_MS).count(),
    })
}

/// Measure the mix in `buffer`; `step_ms` is the grid its hits are checked on.
pub fn measure_mix(buffer: &SampleBuffer, step_ms: Option<f64>) -> MixMeasures {
    let rate = f64::from(buffer.sample_rate);
    let wide = |channel: &[f32]| channel.iter().map(|&s| f64::from(s)).collect::<Vec<f64>>();
    let mut channels = vec![wide(&buffer.left)];
    if let Some(right) = &buffer.right {
        channels.push(wide(right));
    }
    let weighted: Vec<Vec<f64>> = channels.iter().map(|channel| k_weighted(channel, rate)).collect();
    let momentary = block_energies(&weighted, rate, MOMENTARY_SECONDS);
    let short_term = block_energies(&weighted, rate, SHORT_TERM_SECONDS);
    let integrated = mean(&gated(&momentary, INTEGRATED_RELATIVE_GATE_LU)).map(lufs);
    let peak = channels.iter().map(|channel| true_peak(channel)).fold(0.0, f64::max);

    let frames = channels[0].len();
    let count = channels.len() as f64;
    let mono: Vec<f64> = (0..frames).map(|i| channels.iter().map(|c| c[i]).sum::<f64>() / count).collect();
    let hits = find_hits(&mono, rate);
    MixMeasures {
        integrated_lufs: integrated,
        max_momentary_lufs: loudest(&momentary).filter(|l| l.is_finite()),
        max_short_term_lufs: loudest(&short_term).filter(|l| l.is_finite()),
        loudness_range_lu: loudness_range(&short_term),
        true_peak: peak,
        peak_to_loudness_db: integrated.map(|l| amplitude_db(peak) - l),
        band_hits: band_statistics(&hits),
        grid: step_ms.and_then(|step| grid_fit(&hits, step)),
        hits,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: u32 = 48_000;

    fn stereo(seconds: f64, rate: u32, f: impl Fn(f64) -> f64) -> SampleBuffer {
        let frames = (seconds * f64::from(rate)) as usize;
        let left: Vec<f32> = (0..frames).map(|i| f(i as f64 / f64::from(rate)) as f32).collect();
        SampleBuffer { right: Some(left.clone()), left, sample_rate: rate }
    }

    /// EBU Tech 3341: a 1 kHz sine at -23 dBFS on both channels reads -23 LUFS.
    #[test]
    fn the_reference_tone_reads_its_loudness_at_either_rate() {
        let amplitude = 10f64.powf(-23.0 / 20.0);
        for rate in [48_000, 44_100] {
            let m = measure_mix(&stereo(5.0, rate, |t| amplitude * (TAU * 1000.0 * t).sin()), None);
            assert!((m.integrated_lufs.unwrap() + 23.0).abs() < 0.1, "{rate}: {:?}", m.integrated_lufs);
            assert!((m.max_short_term_lufs.unwrap() + 23.0).abs() < 0.1);
            assert!(m.loudness_range_lu.unwrap() < 0.1);
        }
    }

    #[test]
    fn silence_is_gated_out_and_a_short_sound_has_no_loudness() {
        let amplitude = 10f64.powf(-23.0 / 20.0);
        let m = measure_mix(
            &stereo(10.0, RATE, |t| if t < 5.0 { amplitude * (TAU * 1000.0 * t).sin() } else { 0.0 }),
            None,
        );
        assert!((m.integrated_lufs.unwrap() + 23.0).abs() < 0.2, "{:?}", m.integrated_lufs);
        assert_eq!(measure_mix(&stereo(0.2, RATE, |_| 0.5), None).integrated_lufs, None);
        assert_eq!(measure_mix(&stereo(1.0, RATE, |_| 0.0), None).integrated_lufs, None);
    }

    #[test]
    fn the_range_reads_two_levels_apart() {
        // EBU Tech 3342: 20 s at -20 and 20 s at -30 LUFS read a range of 10 LU.
        let level = |lufs: f64| 10f64.powf(lufs / 20.0);
        let m = measure_mix(
            &stereo(40.0, RATE, |t| level(if t < 20.0 { -20.0 } else { -30.0 }) * (TAU * 1000.0 * t).sin()),
            None,
        );
        assert!((m.loudness_range_lu.unwrap() - 10.0).abs() < 1.0, "{:?}", m.loudness_range_lu);
    }

    #[test]
    fn the_true_peak_sees_between_the_samples() {
        // A quarter of the rate, 45° off: every sample reads 0.707.
        let samples: Vec<f64> = (0..4800).map(|i| (TAU * i as f64 / 4.0 + TAU / 8.0).sin()).collect();
        assert!(samples.iter().all(|s| s.abs() < 0.71));
        assert!((true_peak(&samples) - 1.0).abs() < 0.02, "{}", true_peak(&samples));
    }

    /// Bursts of a tone at `times` seconds, 40 ms long, decaying.
    fn bursts(times: &[f64], hz: f64, gains: &[f64]) -> impl Fn(f64) -> f64 {
        let (times, gains) = (times.to_vec(), gains.to_vec());
        move |t| {
            times
                .iter()
                .zip(&gains)
                .filter(|(start, _)| t >= **start && t < **start + 0.04)
                .map(|(start, gain)| gain * (-(t - start) / 0.01).exp() * (TAU * hz * (t - start)).sin())
                .sum()
        }
    }

    #[test]
    fn hits_are_found_and_timed() {
        let times = [0.1, 0.35, 0.6, 0.85, 1.1, 1.35];
        let m = measure_mix(&stereo(1.6, RATE, bursts(&times, 5000.0, &[0.8, 0.2, 0.8, 0.2, 0.8, 0.2])), Some(125.0));
        assert_eq!(m.hits.len(), times.len(), "{:?}", m.hits);
        for (hit, time) in m.hits.iter().zip(times) {
            assert!((hit.ms - time * 1000.0).abs() < 3.0, "{hit:?} at {time}");
            assert_eq!(hit.band, "highMid");
        }
        assert_eq!(m.band_hits.len(), 1);
        assert!((m.band_hits[0].level_spread_db - 12.04).abs() < 1.0, "{:?}", m.band_hits);
        let grid = m.grid.unwrap();
        assert_eq!(grid.off_grid_count, 0);
        assert!(grid.max_deviation_ms < 3.0, "{grid:?}");
    }

    #[test]
    fn a_quiet_hit_stands_beside_loud_ones() {
        // 14 dB under the others, as a ghost note beside a backbeat.
        let times = [0.1, 0.35, 0.6, 0.85];
        let m = measure_mix(&stereo(1.1, RATE, bursts(&times, 3000.0, &[0.8, 0.8, 0.16, 0.8])), None);
        assert_eq!(m.hits.len(), times.len(), "{:?}", m.hits);
    }

    #[test]
    fn a_late_hit_leaves_the_grid_and_a_steady_tone_has_no_hits() {
        let times = [0.1, 0.35, 0.6, 0.875, 1.1];
        let m = measure_mix(&stereo(1.3, RATE, bursts(&times, 5000.0, &[0.8; 5])), Some(125.0));
        assert_eq!(m.grid.unwrap().off_grid_count, 1, "{:?}", m.grid);
        let steady = measure_mix(&stereo(1.0, RATE, |t| 0.5 * (TAU * 440.0 * t).sin()), None);
        assert!(steady.hits.len() <= 1, "{:?}", steady.hits);
    }
}
