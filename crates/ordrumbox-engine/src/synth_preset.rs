//! Synth presets (`assets/data/generated_sounds.json`) and the patch a note
//! plays with (`WorkletSynthVoice#sendUpdate()`).
//!
//! Presets are free-form JSON, read leniently: a number or a numeric string,
//! else the default; a flag on only when `true`; an unknown enum name 0.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_dsp::SynthPatch;
use ordrumbox_dsp::lfo::Wave;
use ordrumbox_dsp::slope::FilterMode;
pub use ordrumbox_dsp::synth::FM_DEVIATION;
use ordrumbox_dsp::synth::patch::{Adsr, LfoTarget, ModEnvTarget, SynthBypass, SynthLfo};

/// `C3_FREQ`, Hz: the pitch of ratio 1.
const C3_FREQ: f64 = 130.812_782_650_299_3;
/// `MIN_NOTE_RATIO`.
const MIN_NOTE_RATIO: f64 = 0.0001;

/// The oscillator and LFO waves, by code.
pub const WAVES: [&str; 5] = ordrumbox_dsp::lfo::WAVE_NAMES;
/// The filter types, by code (a strip's and a synth voice's filter mode).
pub const FILTER_TYPES: [&str; 4] = ["lowpass", "highpass", "bandpass", "notch"];
const MOD_ENV_TARGETS: [&str; 5] = ["off", "filter", "pitch", "fm", "shape"];
/// The LFO targets by their name in a preset (two names for the filter envelope amount).
const LFO_TARGETS: [(&str, LfoTarget); 35] = [
    ("NOT", LfoTarget::Off),
    ("FLT", LfoTarget::Filter),
    ("VCO1", LfoTarget::Osc1),
    ("VCO2", LfoTarget::Osc2),
    ("VCO3", LfoTarget::Osc3),
    ("masterVolume", LfoTarget::Volume),
    ("vco1.gain", LfoTarget::Osc1Gain),
    ("vco1.detune", LfoTarget::Osc1Detune),
    ("vco1.octave", LfoTarget::Osc1Octave),
    ("vco2.gain", LfoTarget::Osc2Gain),
    ("vco2.detune", LfoTarget::Osc2Detune),
    ("vco2.octave", LfoTarget::Osc2Octave),
    ("vco3.gain", LfoTarget::Osc3Gain),
    ("vco3.detune", LfoTarget::Osc3Detune),
    ("vco3.octave", LfoTarget::Osc3Octave),
    ("filter.freq", LfoTarget::FilterFreq),
    ("filter.filterEnvelopeAmount", LfoTarget::FilterEnvAmount),
    ("filterEnv.filterEnvelopeAmount", LfoTarget::FilterEnvAmount),
    ("filter.Q", LfoTarget::FilterQ),
    ("noise.mix", LfoTarget::NoiseMix),
    ("subGain", LfoTarget::SubGain),
    ("pitchPunch", LfoTarget::PitchPunch),
    ("filter.drive", LfoTarget::Drive),
    ("noise.filterFreq", LfoTarget::NoiseFilterFreq),
    ("noise.filterQ", LfoTarget::NoiseFilterQ),
    ("fm.amount", LfoTarget::FmAmount),
    ("fm.algo", LfoTarget::FmAlgorithm),
    ("envelope.attack", LfoTarget::EnvAttack),
    ("envelope.decay", LfoTarget::EnvDecay),
    ("envelope.sustain", LfoTarget::EnvSustain),
    ("envelope.release", LfoTarget::EnvRelease),
    ("modEnvelope.attack", LfoTarget::ModEnvAttack),
    ("modEnvelope.decay", LfoTarget::ModEnvDecay),
    ("modEnvelope.sustain", LfoTarget::ModEnvSustain),
    ("modEnvelope.release", LfoTarget::ModEnvRelease),
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
    /// Read a presets file; anything but an object is an error.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        value.as_object().cloned().map(Self).ok_or_else(|| "not an object of presets".to_owned())
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

    /// Set preset `key` (a new key goes last).
    pub fn set(&mut self, key: &str, preset: Value) {
        self.0.insert(key, preset);
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Add the presets of `other` this one does not hold, leaving its own as they are.
    pub fn add_missing(&mut self, other: &Self) {
        for key in other.keys() {
            if self.get(key).is_none()
                && let Some(preset) = other.get(key)
            {
                self.set(key, preset.clone());
            }
        }
    }

    /// The presets as `generated_sounds.json` holds them.
    pub fn to_json(&self) -> Value {
        Value::Object(self.0.clone())
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

/// The number `value` holds (a number or a numeric string), `fallback` without one.
fn finite(value: Option<&Value>, fallback: f64) -> f64 {
    value.and_then(Value::to_number).unwrap_or(fallback)
}

/// A level of a preset (a gain, a mix, the master volume), inside 0–1 as
/// its control is: a file's level outside would play louder than full scale.
fn level(value: Option<&Value>, fallback: f64) -> f64 {
    clamp(finite(value, fallback), 0.0, 1.0)
}

/// `x` inside `min`–`max`; `min > max` gives `max` instead of a panic.
fn clamp(x: f64, min: f64, max: f64) -> f64 {
    x.max(min).min(max)
}

/// The choice of `choices` at the place `value` has in `names`; an unknown
/// name is the first choice.
fn choice<T: Copy + Default>(value: Option<&Value>, names: &[&str], choices: &[T]) -> T {
    let name = value.and_then(Value::as_str);
    names.iter().zip(choices).find(|(n, _)| Some(**n) == name).map_or_else(T::default, |(_, choice)| *choice)
}

fn lfo_target(value: Option<&Value>) -> LfoTarget {
    let name = value.and_then(Value::as_str);
    LFO_TARGETS.iter().find(|(n, _)| Some(*n) == name).map_or(LfoTarget::Off, |&(_, target)| target)
}

/// `syncToHz()`: `None` when off, unknown, or without a tempo.
pub fn sync_to_hz(sync: Option<&Value>, bpm: f64) -> Option<f64> {
    let sync = sync?.as_str()?;
    if sync == "off" || bpm.is_nan() || bpm <= 0.0 {
        return None;
    }
    let base = SYNC_NOTE_HZ.iter().find(|(label, _)| *label == sync)?.1;
    Some(base * (bpm / 60.0))
}

/// Property `key` of `value` (`value?.[key]`; `None` for non-objects).
fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|v| v.as_object()).and_then(|o| o.get(key))
}

/// `a` unless missing or null, else `b`.
fn or_else<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    a.filter(|v| !v.is_null()).or(b)
}

/// The `update` message `WorkletSynthVoice` sends for `note` with `preset`.
pub fn synth_patch(preset: &Value, note: SynthNote) -> SynthPatch {
    let gs = Some(preset);
    let at = |key: &str| get(gs, key);
    let note_ratio = finite(Some(&Value::Number(note.ratio)), 1.0).max(MIN_NOTE_RATIO);

    let vcos = [at("vco1"), at("vco2"), at("vco3")];
    let gains = vcos.map(|vco| level(get(vco, "gain"), 0.0));
    let total_gain = gains[0] + gains[1] + gains[2];
    // The voice mixes oscillators and noise by `noise.mix`: both count, or a
    // sound mostly made of noise would be raised by its quiet oscillators.
    let noise_mix =
        if at("bypassNoise").is_some_and(Value::is_true) { 0.0 } else { level(get(at("noise"), "mix"), 0.0) };
    let mix_gain = total_gain * (1.0 - noise_mix) + noise_mix;
    let mix_norm = if mix_gain > 0.001 { 1.0 / mix_gain } else { 1.0 };
    let note_velo = note.velocity * mix_norm;
    let accent = if note_velo > 0.5 { 1.0 + 0.5 * 0.5 } else { 1.0 };
    let master_volume = level(at("masterVolume"), 0.8);
    let peak = note_velo * master_volume * accent * 1.0;

    let osc_freq = vcos.map(|vco| {
        if !vco.is_some_and(|v| v.as_object().is_some()) {
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
            wave: choice(get(l, "wave"), &WAVES, &Wave::ALL),
            freq: sync_to_hz(get(l, "sync"), note.bpm).unwrap_or_else(|| finite(get(l, "freq"), 0.0)),
            depth: finite(get(l, "depth"), 0.0),
        }
    };
    let mod_env = at("modEnvelope");
    let mod_target = get(mod_env, "target");
    let fm = at("fm");
    let flag = |key: &str| at(key).is_some_and(Value::is_true);

    SynthPatch {
        osc_freq,
        osc_gain: gains,
        osc_detune: vcos.map(|vco| finite(get(vco, "detune"), 0.0)),
        osc_wave: vcos.map(|vco| choice(get(vco, "wave"), &WAVES, &Wave::ALL)),
        noise_mix: level(get(noise, "mix"), 0.0),
        noise_filter_type: choice(get(noise, "filterType"), &FILTER_TYPES, &FilterMode::ALL),
        noise_filter_freq: finite(get(noise, "filterFreq"), 1000.0),
        noise_filter_q: finite(get(noise, "filterQ"), 0.7),
        filter_type: choice(get(filter, "type"), &FILTER_TYPES, &FilterMode::ALL),
        filter_freq: finite(get(filter, "freq"), 1000.0),
        filter_q: finite(get(filter, "Q"), 0.7),
        filter_slope: finite(get(filter, "slope"), 12.0),
        drive: finite(get(filter, "drive"), 0.0),
        pitch_punch: finite(at("pitchPunch"), 0.0),
        sub_gain: level(at("subGain"), 0.0),
        envelope: Adsr {
            attack: clamp(finite(get(env, "attack"), 0.01), 0.003, 0.5),
            decay: finite(get(env, "decay"), 0.1).min(1.0),
            sustain: finite(get(env, "sustain"), 0.7),
            release: clamp(finite(get(env, "release"), 0.1), 0.008, 0.5),
        },
        volume: 1.0,
        pan: finite(Some(&Value::Number(note.pan)), 0.0),
        velocity: peak,
        lfo1: lfo("lfo"),
        lfo2: lfo("lfo2"),
        filter_env_amount: clamp(finite(get(or_else(at("filterEnv"), filter), "filterEnvelopeAmount"), 0.0), 0.0, 1.0),
        fm_amount: clamp(finite(get(fm, "amount"), 0.0), 0.0, 1.0),
        fm_algorithm: clamp(finite(get(fm, "algo"), 0.0).round(), 0.0, 4.0),
        mod_envelope: Adsr {
            attack: clamp(finite(get(mod_env, "attack"), 0.01), 0.003, 0.5),
            decay: finite(get(mod_env, "decay"), 0.1).min(1.0),
            sustain: finite(get(mod_env, "sustain"), 0.0),
            release: clamp(finite(get(mod_env, "release"), 0.1), 0.008, 0.5),
        },
        mod_env_target: choice(mod_target, &MOD_ENV_TARGETS, &ModEnvTarget::ALL),
        mod_env_depth: if mod_target.and_then(Value::as_str).is_some_and(|t| !t.is_empty() && t != "off") {
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
    fn a_level_outside_its_control_is_brought_inside() {
        let patch = synth_patch(
            &json::parse(
                r#"{"masterVolume":1e308,"subGain":-3,"vco1":{"gain":2},"vco2":{"gain":-1e308},"noise":{"mix":7}}"#,
            )
            .unwrap(),
            SynthNote { ratio: 1.0, velocity: 0.4, pan: 0.0, bpm: 120.0 },
        );
        assert_eq!((patch.osc_gain, patch.noise_mix, patch.sub_gain), ([1.0, 0.0, 0.0], 1.0, 0.0));
        // All noise at full mix: the note's velocity at a master volume of 1.
        assert_eq!(patch.velocity, 0.4);
    }

    #[test]
    fn missing_sections_fall_back() {
        let patch = synth_patch(
            &json::parse(r#"{"vco1":{"gain":0.5,"wave":"square","octave":-1}}"#).unwrap(),
            SynthNote { ratio: 2.0, velocity: 0.8, pan: 0.0, bpm: 120.0 },
        );
        assert_eq!(patch.osc_freq, [C3_FREQ, 0.0, 0.0]);
        assert_eq!(patch.osc_wave, [Wave::Square, Wave::Sine, Wave::Sine]);
        // 0.8 / 0.5 = 1.6, accented (×1.25), master volume 0.8.
        assert!((patch.velocity - 1.6 * 0.8 * 1.25).abs() < 1e-12);
        assert_eq!(patch.envelope, Adsr { attack: 0.01, decay: 0.1, sustain: 0.7, release: 0.1 });
    }

    #[test]
    fn add_missing_keeps_what_is_there() {
        let presets = |text: &str| SynthPresets::from_json(&json::parse(text).unwrap()).unwrap();
        let mut saved = presets(r#"{"A":{"masterVolume":0.1}}"#);
        saved.add_missing(&presets(r#"{"A":{"masterVolume":0.9},"B":{"masterVolume":0.5}}"#));
        assert_eq!(saved, presets(r#"{"A":{"masterVolume":0.1},"B":{"masterVolume":0.5}}"#));
    }

    #[test]
    fn the_level_counts_the_noise_in_the_mix() {
        let velocity = |preset: &str| {
            synth_patch(&json::parse(preset).unwrap(), SynthNote { ratio: 1.0, velocity: 0.8, pan: 0.0, bpm: 120.0 })
                .velocity
        };
        // Oscillators at 0.4 for 20% of the mix, noise for 80%: 0.88 in all.
        let noisy = velocity(r#"{"masterVolume":1,"vco1":{"gain":0.4},"noise":{"mix":0.8}}"#);
        assert!((noisy - 0.8 / 0.88 * 1.25).abs() < 1e-12, "{noisy}");
        // Bypassed, the noise is not in the mix.
        let bypassed = velocity(r#"{"masterVolume":1,"vco1":{"gain":0.4},"noise":{"mix":0.8},"bypassNoise":true}"#);
        assert!((bypassed - 0.8 / 0.4 * 1.25).abs() < 1e-12, "{bypassed}");
    }

    #[test]
    fn lfo_sync_and_targets() {
        let preset =
            json::parse(r#"{"lfo":{"target":"filter.Q","wave":"random","freq":3,"sync":"1/8"},"lfo2":{"target":"x"}}"#)
                .unwrap();
        let patch = synth_patch(&preset, SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 90.0 });
        assert_eq!((patch.lfo1.target, patch.lfo1.wave, patch.lfo1.freq), (LfoTarget::FilterQ, Wave::Random, 3.0));
        assert_eq!(patch.lfo2.target, LfoTarget::Off);
        assert_eq!(note_seconds(120.0, 4), 0.125);
    }
}
