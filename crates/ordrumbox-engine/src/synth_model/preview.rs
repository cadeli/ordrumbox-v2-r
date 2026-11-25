//! What the synth editor plots and animates: the LFO modulation, the scope,
//! the envelope and the filter curve.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::lfo;
use ordrumbox_dsp::synth::ring;

use super::{EXTRA_METAS, FM_DEVIATION, OSC2_SYNCS, WAVES, lfo_scale, number, slope, text};

/// The LFOs playing (not bypassed): LFO1, LFO2.
fn lfos(draft: &Object) -> [Option<&Object>; 2] {
    let lfo = |group: &str, flag: &str| {
        (!draft.get(flag).is_some_and(Value::is_true)).then(|| draft.get(group)?.as_object()).flatten()
    };
    [lfo("lfo", "bypassLfo1"), lfo("lfo2", "bypassLfo2")]
}

/// The modulation an LFO adds to its target at `time` seconds, in the
/// target's units: tempo sync wins over the frequency.
pub fn lfo_mod(lfo: &Object, time: f64, bpm: f64) -> f64 {
    let target = lfo.get("target").and_then(Value::as_str).unwrap_or_default();
    let scale = lfo_scale(target);
    if scale == 0.0 {
        return 0.0;
    }
    let freq = crate::synth_preset::sync_to_hz(lfo.get("sync"), bpm)
        .unwrap_or_else(|| lfo.get("freq").and_then(Value::to_number).unwrap_or(0.0));
    let depth = lfo.get("depth").and_then(Value::to_number).unwrap_or(0.0);
    if freq <= 0.0 || depth <= 0.0 {
        return 0.0;
    }
    let wave = lfo.get("wave").and_then(Value::as_str).unwrap_or("sine");
    let code = WAVES.iter().position(|w| *w == wave).unwrap_or(0) as f64;
    lfo::waveform(time * freq, code) * depth * scale
}

/// The modulation the playing LFOs add to `path` at `time`.
pub fn lfo_mod_on(draft: &Object, path: &str, time: f64, bpm: f64) -> f64 {
    lfos(draft)
        .into_iter()
        .flatten()
        .filter(|l| l.get("target").and_then(Value::as_str) == Some(path))
        .map(|l| lfo_mod(l, time, bpm))
        .sum()
}

/// The controls an LFO is on: its target, when it
/// has depth and is not bypassed.
pub fn lfo_targeted(draft: &Object, path: &str) -> bool {
    lfos(draft).into_iter().flatten().any(|l| {
        l.get("target").and_then(Value::as_str) == Some(path)
            && path != "NOT"
            && l.get("depth").and_then(Value::as_f64).unwrap_or(0.0) > 0.0
    })
}

// ─── What the canvases plot ──────────────────────────────────────────────────

/// A wave's value at phase `p` (0–1) of cycle `cycle`; a square is high for `width` of it.
fn wave_at(wave: &str, p: f64, cycle: f64, width: f64) -> f64 {
    use std::f64::consts::PI;
    match wave {
        "square" => {
            if p < width {
                1.0
            } else {
                -1.0
            }
        }
        "sawtooth" => 2.0 * p - 1.0,
        "triangle" => 4.0 * (p - 0.5).abs() - 1.0,
        "random" => lfo::waveform(cycle + p, 4.0),
        _ => (2.0 * PI * p).sin(),
    }
}

/// Move the three oscillators of the scope on by their `steps` (cycles a
/// point); oscillator 2 restarts with each cycle of oscillator 1 when `is_synced`.
fn advance(phase: &mut [f64; 3], cycle: &mut [f64; 3], steps: [f64; 3], is_synced: bool) {
    let mut osc1_began = false;
    for (i, step) in steps.into_iter().enumerate() {
        if i == 1 && is_synced && osc1_began && steps[0] > 0.0 {
            // As far into its new cycle as it got since oscillator 1 began its own.
            phase[1] = (phase[0] * step / steps[0]).rem_euclid(1.0);
            cycle[1] += 1.0;
            continue;
        }
        phase[i] += step;
        let whole = phase[i].floor();
        cycle[i] += whole;
        phase[i] -= whole;
        osc1_began = i == 0 && whole != 0.0;
    }
}

/// The phase increments `inc` of the three oscillators, frequency modulated
/// by FM algorithm `algo` at depths (`depth1`, `depth2`); `wave` gives each
/// oscillator's value now.
fn fm_increments(algo: i64, inc: [f64; 3], (depth1, depth2): (f64, f64), wave: impl Fn(usize) -> f64) -> [f64; 3] {
    let [mut f1, mut f2, f3] = inc;
    match algo {
        0 => f1 += wave(1) * depth1,
        1 => f1 += wave(2) * depth1,
        2 => {
            f1 += wave(1) * depth1;
            f2 += wave(2) * depth2;
        }
        3 => f1 += (wave(1) + wave(2)) * depth1,
        4 => {
            f1 += wave(1) * depth1;
            f2 += wave(0) * depth2;
        }
        _ => {}
    }
    [f1, f2, f3]
}

/// Points of the scope: four cycles of the three
/// oscillators with their gains, octaves, detunes, FM, the sync and ring
/// modulation of oscillator 2, and sub, the drive's
/// saturation, scaled to the master volume. `time`: when (the LFOs).
pub fn scope(draft: &Object, time: f64, bpm: f64) -> Vec<f64> {
    const SAMPLES: usize = 1024;
    let vco = |n: usize, key: &str, default: f64| number(draft, &format!("vco{n}.{key}")).unwrap_or(default);
    let waves: Vec<String> =
        (1..=3).map(|n| text(draft, &format!("vco{n}.wave")).unwrap_or("sine").to_owned()).collect();
    let modded = |path: &str| lfo_mod_on(draft, path, time, bpm);
    let (width_min, width_max) = (EXTRA_METAS[0].min, EXTRA_METAS[0].max);
    let width: Vec<f64> = (1..=3)
        .map(|n| (vco(n, "width", 0.5) + modded(&format!("vco{n}.width"))).clamp(width_min, width_max))
        .collect();
    let ratio: Vec<f64> = (1..=3)
        .map(|n| {
            let octave = vco(n, "octave", 0.0) + modded(&format!("vco{n}.octave"));
            let detune = vco(n, "detune", 0.0) + modded(&format!("vco{n}.detune"));
            octave.exp2() * (detune / 1200.0).exp2()
        })
        .collect();
    let gain: Vec<f64> = (1..=3)
        .map(|n| (vco(n, "gain", if n == 1 { 1.0 } else { 0.0 }) + modded(&format!("vco{n}.gain"))).clamp(0.0, 1.0))
        .collect();
    let ring_amount = (number(draft, "vco2.ring").unwrap_or(0.0) + modded("vco2.ring")).clamp(0.0, 1.0);
    let is_synced = text(draft, "vco2.sync") == Some(OSC2_SYNCS[1]);
    let fm_amount = number(draft, "fm.amount").unwrap_or(0.0);
    let fm_algo = number(draft, "fm.algo").unwrap_or(0.0);
    let sub_gain = number(draft, "subGain").unwrap_or(0.0);
    let drive = number(draft, "filter.drive").or_else(|| number(draft, "drive")).unwrap_or(0.0);
    let base = 4.0 / SAMPLES as f64;
    let inc: Vec<f64> = ratio.iter().map(|r| base * r).collect();
    let (depth1, depth2) = (fm_amount * FM_DEVIATION * inc[0], fm_amount * FM_DEVIATION * inc[1]);
    let (mut phase, mut cycle) = ([0.0f64; 3], [0.0f64; 3]);
    let mut out = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let wave = |i: usize| wave_at(&waves[i], phase[i], cycle[i], width[i]);
        let increments = if fm_amount > 0.001 {
            fm_increments(fm_algo as i64, [inc[0], inc[1], inc[2]], (depth1, depth2), wave)
        } else {
            [inc[0], inc[1], inc[2]]
        };
        advance(&mut phase, &mut cycle, increments, is_synced);
        let sub = if sub_gain > 0.0 {
            wave_at("sine", (cycle[0].rem_euclid(2.0) + phase[0]) * 0.5, 0.0, 0.5) * sub_gain
        } else {
            0.0
        };
        let [o1, o2, o3] = [0, 1, 2].map(|i| wave_at(&waves[i], phase[i], cycle[i], width[i]));
        let o2 = ring(o2, o1, ring_amount);
        let mut sample = o1 * gain[0] + o2 * gain[1] + o3 * gain[2] + sub;
        if drive > 0.0 {
            let driven = sample * (1.0 + drive * 3.0);
            sample = driven / (1.0 + driven.abs() * 0.5);
        }
        out.push(sample);
    }
    let peak = out.iter().fold(0.0f64, |m, s| m.max(s.abs()));
    let master = number(draft, "masterVolume").unwrap_or(1.0);
    if peak > 0.0 {
        for s in &mut out {
            *s = *s / peak * master;
        }
    }
    out
}

/// The envelope's corners as (time, level), over its plotted length
///.
pub fn envelope(draft: &Object) -> (Vec<(f64, f64)>, f64) {
    let v = |key: &str, default: f64| number(draft, &format!("envelope.{key}")).unwrap_or(default);
    let (a, d, s, r) = (v("attack", 0.0), v("decay", 0.12), v("sustain", 1.0), v("release", 0.05));
    let total = (a + d + 0.3 + r).max(0.5);
    (vec![(0.0, 0.0), (a, 1.0), (a + d, s), (total - r, s), (total, 0.0)], total)
}

/// The filter's response, dB at `points + 1` frequencies from 20 Hz to
/// 20 kHz (log), centred on `fc` (`#drawFilterResponse`'s `drawCurve`); a
/// slope of 24 dB/oct is the product of its two stages.
pub fn filter_response(kind: &str, fc: f64, q: f64, slope_db: f64, points: usize) -> Vec<f64> {
    let magnitude = |f: f64, q: f64| {
        let d = fc * fc - f * f;
        let denom = (d * d + (fc * f / q) * (fc * f / q)).sqrt();
        match kind {
            "highpass" => f * f / denom,
            "bandpass" => fc * f / q / denom,
            _ => fc * fc / denom,
        }
    };
    let mode = match kind {
        "highpass" => slope::FilterMode::Highpass,
        "bandpass" => slope::FilterMode::Bandpass,
        _ => slope::FilterMode::Lowpass,
    };
    let (k1, k2, _) = slope::stages(1.0 / q, mode);
    (0..=points)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / points as f64);
            // The bandpass curve is drawn normalized: both stages peak at 1.
            let mag = if slope::is_steep(slope_db) {
                magnitude(f, 1.0 / k1) * magnitude(f, 1.0 / k2)
            } else {
                magnitude(f, q)
            };
            (20.0 * mag.max(1e-10).log10()).clamp(-30.0, 12.0)
        })
        .collect()
}

/// The filter curve's type, centre, Q with the LFOs at `time` and slope, and
/// the band an LFO on the frequency sweeps.
pub fn filter_now(draft: &Object, time: f64, bpm: f64) -> (String, f64, f64, f64, Option<(f64, f64)>) {
    let kind = text(draft, "filter.type").unwrap_or("lowpass").to_owned();
    let slope_db = number(draft, "filter.slope").unwrap_or(12.0);
    let fc = number(draft, "filter.freq")
        .unwrap_or(400.0)
        .clamp(ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ);
    let q = number(draft, "filter.Q").unwrap_or(1.0).clamp(0.1, 24.0);
    let fc = (fc + lfo_mod_on(draft, "filter.freq", time, bpm))
        .clamp(ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ);
    let q = (q + lfo_mod_on(draft, "filter.Q", time, bpm)).clamp(0.1, 24.0);
    let depth = lfos(draft)
        .into_iter()
        .flatten()
        .filter(|l| l.get("target").and_then(Value::as_str) == Some("filter.freq"))
        .map(|l| l.get("depth").and_then(Value::as_f64).unwrap_or(0.0))
        .fold(0.0f64, f64::max);
    let band = (depth > 0.0).then(|| {
        (
            (fc - depth * 1000.0).max(ordrumbox_dsp::MIN_FILTER_FREQ),
            (fc + depth * 1000.0).min(ordrumbox_dsp::MAX_FILTER_FREQ),
        )
    });
    (kind, fc, q, slope_db, band)
}
