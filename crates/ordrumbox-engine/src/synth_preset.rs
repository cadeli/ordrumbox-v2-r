//! Synth presets (`assets/data/generated_sounds.json`) and the patch a note
//! plays with (`WorkletSynthVoice#sendUpdate()`).
//!
//! Presets are free-form JSON in the JS app; they are kept as JSON here too
//! and read with the same lenient coercions (`toFiniteNumber`, `!!`, enum
//! maps that fall back to 0).

use ordrumbox_core::js::{round, to_number, truthy};
use ordrumbox_core::json::{Object, Value};
use ordrumbox_dsp::SynthPatch;
use ordrumbox_dsp::synth::{Adsr, SynthBypass, SynthLfo};

/// `C3_FREQ`, Hz: the pitch of ratio 1.
const C3_FREQ: f64 = 130.812_782_650_299_3;
/// `MIN_NOTE_RATIO`.
const MIN_NOTE_RATIO: f64 = 0.0001;

const WAVES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];
const FILTERS: [&str; 4] = ["lowpass", "highpass", "bandpass", "notch"];
const MOD_ENV_TARGETS: [&str; 5] = ["off", "filter", "pitch", "fm", "shape"];
/// `LFO_TARGET_TO_INT`, by code (two names share 16).
const LFO_TARGETS: [(&str, u8); 35] = [
    ("NOT", 0),
    ("FLT", 1),
    ("VCO1", 2),
    ("VCO2", 3),
    ("VCO3", 4),
    ("masterVolume", 5),
    ("vco1.gain", 6),
    ("vco1.detune", 7),
    ("vco1.octave", 8),
    ("vco2.gain", 9),
    ("vco2.detune", 10),
    ("vco2.octave", 11),
    ("vco3.gain", 12),
    ("vco3.detune", 13),
    ("vco3.octave", 14),
    ("filter.freq", 15),
    ("filter.filterEnvelopeAmount", 16),
    ("filterEnv.filterEnvelopeAmount", 16),
    ("filter.Q", 17),
    ("noise.mix", 18),
    ("subGain", 19),
    ("pitchPunch", 20),
    ("filter.drive", 21),
    ("noise.filterFreq", 22),
    ("noise.filterQ", 23),
    ("fm.amount", 24),
    ("fm.algo", 25),
    ("envelope.attack", 26),
    ("envelope.decay", 27),
    ("envelope.sustain", 28),
    ("envelope.release", 29),
    ("modEnvelope.attack", 30),
    ("modEnvelope.decay", 31),
    ("modEnvelope.sustain", 32),
    ("modEnvelope.release", 33),
];
/// `SYNC_NOTE_HZ`: LFO cycles per beat-second for each sync label.
const SYNC_NOTE_HZ: [(&str, f64); 7] = [
    ("1/1", 0.25),
    ("1/2", 0.5),
    ("1/4", 1.0),
    ("1/8", 2.0),
    ("1/16", 4.0),
    ("1/8T", (2.0 * 2.0) / 3.0),
    ("1/16T", (4.0 * 2.0) / 3.0),
];

/// Every preset by key (`generated_sounds.json` is an object of presets).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SynthPresets(Object);

impl SynthPresets {
    /// Read generated_sounds.json; anything but an object gives no preset.
    pub fn from_json(value: &Value) -> Self {
        Self(value.as_object().cloned().unwrap_or_default())
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Preset keys, in file order.
    pub fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(k, _)| k).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// The note a patch is built for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthNote {
    /// Pitch ratio (1 = C3).
    pub ratio: f64,
    pub velocity: f64,
    pub pan: f64,
    pub bpm: f64,
}

/// `toFiniteNumber(value, fallback)`; a missing value is `undefined`.
fn finite(value: Option<&Value>, fallback: f64) -> f64 {
    let n = value.map_or(f64::NAN, to_number);
    if n.is_finite() { n } else { fallback }
}

/// `Math.min(max, Math.max(min, x))`.
fn clamp(x: f64, min: f64, max: f64) -> f64 {
    x.max(min).min(max)
}

/// `mapEnum()`: index of a known name, else 0.
fn enum_code(value: Option<&Value>, names: &[&str]) -> f64 {
    value.and_then(Value::as_str).and_then(|v| names.iter().position(|n| *n == v)).map_or(0.0, |i| i as f64)
}

fn lfo_target(value: Option<&Value>) -> f64 {
    let name = value.and_then(Value::as_str);
    LFO_TARGETS.iter().find(|(n, _)| Some(*n) == name).map_or(0.0, |&(_, code)| f64::from(code))
}

/// `syncToHz()`: `None` when off, unknown, or without a tempo.
fn sync_to_hz(sync: Option<&Value>, bpm: f64) -> Option<f64> {
    let sync = sync.filter(|v| truthy(v))?;
    if sync.as_str() == Some("off") || bpm.is_nan() || bpm <= 0.0 {
        return None;
    }
    let base = SYNC_NOTE_HZ.iter().find(|(label, _)| Some(*label) == sync.as_str())?.1;
    Some(base * (bpm / 60.0))
}

/// Property `key` of `value` (`value?.[key]`; `None` for non-objects).
fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|v| v.as_object()).and_then(|o| o.get(key))
}

/// `a ?? b`: `a` unless missing or null.
fn or_else<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    a.filter(|v| !v.is_null()).or(b)
}

/// The `update` message `WorkletSynthVoice` sends for `note` with `preset`.
pub fn synth_patch(preset: &Value, note: SynthNote) -> SynthPatch {
    let gs = Some(preset);
    let at = |key: &str| get(gs, key);
    let note_ratio = finite(Some(&Value::Number(note.ratio)), 1.0).max(MIN_NOTE_RATIO);

    let vcos = [at("vco1"), at("vco2"), at("vco3")];
    let gains = vcos.map(|vco| finite(get(vco, "gain"), 0.0));
    let total_gain = gains[0] + gains[1] + gains[2];
    let vco_norm = if total_gain > 0.001 { 1.0 / total_gain } else { 1.0 };
    let note_velo = note.velocity * vco_norm;
    let accent = if note_velo > 0.5 { 1.0 + 0.5 * 0.5 } else { 1.0 };
    let master_volume = finite(at("masterVolume"), 0.8);
    let peak = note_velo * master_volume * accent * 1.0;

    let osc_freq = vcos.map(|vco| {
        if !vco.is_some_and(truthy) {
            return 0.0;
        }
        let octave = clamp(finite(get(vco, "octave"), 0.0), -4.0, 4.0);
        C3_FREQ * note_ratio.max(MIN_NOTE_RATIO) * 2.0_f64.powf(octave + 0.0)
    });

    let noise = at("noise");
    let filter = at("filter");
    let env = at("envelope");
    let lfo = |key: &str| {
        let l = at(key);
        SynthLfo {
            target: lfo_target(get(l, "target")),
            wave: enum_code(get(l, "wave"), &WAVES),
            freq: sync_to_hz(get(l, "sync"), note.bpm).unwrap_or_else(|| finite(get(l, "freq"), 0.0)),
            depth: finite(get(l, "depth"), 0.0),
        }
    };
    let mod_env = at("modEnvelope");
    let mod_target = get(mod_env, "target");
    let fm = at("fm");
    let flag = |key: &str| at(key).is_some_and(truthy);

    SynthPatch {
        osc_freq,
        osc_gain: gains,
        osc_detune: vcos.map(|vco| finite(get(vco, "detune"), 0.0)),
        osc_wave: vcos.map(|vco| enum_code(get(vco, "wave"), &WAVES)),
        noise_mix: finite(get(noise, "mix"), 0.0),
        noise_filter_type: enum_code(get(noise, "filterType"), &FILTERS),
        noise_filter_freq: finite(get(noise, "filterFreq"), 1000.0),
        noise_filter_q: finite(get(noise, "filterQ"), 0.7),
        filter_type: enum_code(get(filter, "type"), &FILTERS),
        filter_freq: finite(get(filter, "freq"), 1000.0),
        filter_q: finite(get(filter, "Q"), 0.7),
        drive: finite(get(filter, "drive"), 0.0),
        pitch_punch: finite(at("pitchPunch"), 0.0),
        sub_gain: finite(at("subGain"), 0.0),
        envelope: Adsr {
            attack: clamp(finite(get(env, "attack"), 0.01), 0.003, 0.5),
            decay: finite(get(env, "decay"), 0.1).min(1.0),
            sustain: finite(get(env, "sustain"), 0.7),
            release: clamp(finite(get(env, "release"), 0.1), 0.008, 0.5),
        },
        master: 1.0,
        pan: finite(Some(&Value::Number(note.pan)), 0.0),
        velocity: peak,
        lfo1: lfo("lfo"),
        lfo2: lfo("lfo2"),
        filter_env_amount: clamp(finite(get(or_else(at("filterEnv"), filter), "filterEnvelopeAmount"), 0.0), 0.0, 1.0),
        fm_amount: clamp(finite(get(fm, "amount"), 0.0), 0.0, 1.0),
        fm_algo: clamp(round(finite(get(fm, "algo"), 0.0)), 0.0, 4.0),
        mod_envelope: Adsr {
            attack: clamp(finite(get(mod_env, "attack"), 0.01), 0.003, 0.5),
            decay: finite(get(mod_env, "decay"), 0.1).min(1.0),
            sustain: finite(get(mod_env, "sustain"), 0.0),
            release: clamp(finite(get(mod_env, "release"), 0.1), 0.008, 0.5),
        },
        mod_env_target: enum_code(mod_target, &MOD_ENV_TARGETS),
        mod_env_depth: if mod_target.is_some_and(truthy) && mod_target.and_then(Value::as_str) != Some("off") {
            1.0
        } else {
            0.0
        },
        bypass: SynthBypass {
            noise: flag("bypassNoise"),
            filter: flag("bypassFilter"),
            env: flag("bypassEnv"),
            lfo1: flag("bypassLfo1"),
            lfo2: flag("bypassLfo2"),
            fm: flag("bypassFm"),
            mod_env: flag("bypassModEnv"),
            filter_env: flag("bypassFilterEnv"),
        },
    }
}

/// Note length of a synth note: one grid step (`60 / bpm / stepsPerBeat`).
pub fn note_seconds(bpm: f64, steps_per_beat: i32) -> f64 {
    60.0 / bpm / f64::from(steps_per_beat.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn missing_sections_fall_back() {
        let patch = synth_patch(
            &json::parse(r#"{"vco1":{"gain":0.5,"wave":"square","octave":-1}}"#).unwrap(),
            SynthNote { ratio: 2.0, velocity: 0.8, pan: 0.0, bpm: 120.0 },
        );
        assert_eq!(patch.osc_freq, [C3_FREQ, 0.0, 0.0]);
        assert_eq!(patch.osc_wave, [3.0, 0.0, 0.0]);
        // 0.8 / 0.5 = 1.6, accented (×1.25), master volume 0.8.
        assert!((patch.velocity - 1.6 * 0.8 * 1.25).abs() < 1e-12);
        assert_eq!(patch.envelope, Adsr { attack: 0.01, decay: 0.1, sustain: 0.7, release: 0.1 });
    }

    #[test]
    fn lfo_sync_and_targets() {
        let preset =
            json::parse(r#"{"lfo":{"target":"filter.Q","wave":"random","freq":3,"sync":"1/8"},"lfo2":{"target":"x"}}"#)
                .unwrap();
        let patch = synth_patch(&preset, SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 90.0 });
        assert_eq!((patch.lfo1.target, patch.lfo1.wave, patch.lfo1.freq), (17.0, 4.0, 3.0));
        assert_eq!(patch.lfo2.target, 0.0);
        assert_eq!(note_seconds(120.0, 4), 0.125);
    }
}
