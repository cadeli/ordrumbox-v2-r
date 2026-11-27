//! The controls added since the first presets: written only once set (a
//! preset without one plays as it always did), shown last in their card,
//! at the value the engine plays with while they are absent; what touching
//! one of them switches on.

use ordrumbox_core::json::{Object, Value};

use super::{Meta, TIME_MIN, meta, number, set, time};
use crate::synth_preset::{
    self, ATTACK_MAX, DECAY_MAX, FILTER_ENV_STAGES, FILTER2_FREQ_DEFAULT, FILTER2_Q_DEFAULT, FILTER2_TYPE_DEFAULT,
    GLIDE_MAX, LFO_DELAY_MAX, NOISE_RATE_RANGE, RELEASE_MAX,
};

/// Fields added since the first presets, shown last in their card and written
/// only once set (absent, the engine's default applies): kept out of
/// [`super::DEFAULTS`] so a hydrated preset keeps the keys it was written
/// with. The groups `filter2` and `voice` are made of them alone: a preset
/// holds them only once one of their controls is set.
pub(super) const EXTRA_FIELDS: [(&str, &str); 26] = [
    ("filter", "slope"),
    ("vco1", "semitone"),
    ("vco2", "semitone"),
    ("vco3", "semitone"),
    ("vco1", "width"),
    ("vco2", "width"),
    ("vco2", "ring"),
    ("vco2", "sync"),
    ("vco3", "width"),
    ("noise", "mode"),
    ("noise", "rate"),
    ("filterEnv", "attack"),
    ("filterEnv", "decay"),
    ("filterEnv", "sustain"),
    ("filterEnv", "release"),
    ("filterEnv", "invert"),
    ("filter2", "type"),
    ("filter2", "freq"),
    ("filter2", "Q"),
    ("lfo", "delay"),
    ("lfo", "retrigger"),
    ("lfo2", "delay"),
    ("lfo2", "retrigger"),
    ("voice", "mode"),
    ("voice", "glide"),
    ("voice", "accent"),
];

/// The controls of [`EXTRA_FIELDS`] that are knobs: the pulse width of each
/// oscillator, how far oscillator 1 ring-modulates oscillator 2, the clock
/// of the console noise, the filter envelope's stages, the second filter's
/// cutoff and resonance, the LFOs' fade-in, the glide and the accent.
pub const EXTRA_METAS: [Meta; 15] = [
    Meta { default: 0.5, label: Some("Width"), ..meta("vco1.width", 0.02, 0.98, 0.01, "") },
    Meta { default: 0.5, label: Some("Width"), ..meta("vco2.width", 0.02, 0.98, 0.01, "") },
    Meta { default: 0.5, label: Some("Width"), ..meta("vco3.width", 0.02, 0.98, 0.01, "") },
    Meta { label: Some("Ring"), ..meta("vco2.ring", 0.0, 1.0, 0.01, "") },
    Meta {
        log: true,
        default: ordrumbox_dsp::synth::patch::NOISE_RATE_DEFAULT,
        label: Some("Rate"),
        ..meta("noise.rate", NOISE_RATE_RANGE.0, NOISE_RATE_RANGE.1, 1.0, "Hz")
    },
    time("filterEnv.attack", ATTACK_MAX),
    time("filterEnv.decay", DECAY_MAX),
    meta("filterEnv.sustain", 0.0, 1.0, 0.01, ""),
    time("filterEnv.release", RELEASE_MAX),
    Meta {
        log: true,
        default: FILTER2_FREQ_DEFAULT,
        ..meta("filter2.freq", ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ, 1.0, "Hz")
    },
    Meta { default: FILTER2_Q_DEFAULT, ..meta("filter2.Q", 0.1, 24.0, 0.1, "") },
    Meta { label: Some("Delay"), ..meta("lfo.delay", 0.0, LFO_DELAY_MAX, 0.01, "s") },
    Meta { label: Some("Delay"), ..meta("lfo2.delay", 0.0, LFO_DELAY_MAX, 0.01, "s") },
    Meta { label: Some("Glide"), ..meta("voice.glide", 0.0, GLIDE_MAX, 0.001, "s") },
    Meta { label: Some("Accent"), ..meta("voice.accent", 0.0, 1.0, 0.01, "") },
];

// The filter envelope's stages start at the shortest time a knob sets, as the envelopes' do.
const _: () = assert!(EXTRA_METAS[5].min == TIME_MIN);

/// The controls of [`EXTRA_FIELDS`] that are switches, off while absent.
pub const SWITCHES: [&str; 3] = ["filterEnv.invert", "lfo.retrigger", "lfo2.retrigger"];

/// The switches named shorter than their key under them, for the LFOs' strips to fit.
pub const SWITCH_LABELS: [(&str, &str); 2] = [("lfo.retrigger", "Retrig"), ("lfo2.retrigger", "Retrig")];

/// The value of an extra control `draft` has not set, if `path` is one.
pub fn extra_default(path: &str) -> Option<f64> {
    EXTRA_METAS.iter().find(|m| m.path == path).map(|m| m.default)
}

/// The value extra control `path` plays with while `draft` does not set it:
/// a stage of the filter envelope follows the amplitude envelope
/// ([`synth_preset::filter_adsr_default`]), any other its default.
pub fn extra_value(draft: &Object, path: &str) -> Option<f64> {
    let Some(stage) = path.strip_prefix("filterEnv.").filter(|stage| FILTER_ENV_STAGES.contains(stage)) else {
        return extra_default(path);
    };
    let envelope = synth_preset::amp_adsr(draft.get("envelope"));
    let adsr = synth_preset::filter_adsr_default(envelope);
    Some(match stage {
        "attack" => adsr.attack,
        "decay" => adsr.decay,
        "sustain" => adsr.sustain,
        _ => adsr.release,
    })
}

/// Whether `path` is an extra field: its group may be absent from a preset.
pub(super) fn is_extra(path: &str) -> bool {
    path.split_once('.').is_some_and(|(group, key)| EXTRA_FIELDS.contains(&(group, key)))
}

/// The groups made of extra fields alone (`filter2`, `voice`): absent from a preset until one is set.
pub(super) const EXTRA_GROUPS: [&str; 2] = ["filter2", "voice"];

/// The second filter a power button turns on: its type, cutoff and resonance.
pub(super) fn filter2_defaults() -> Value {
    let mut group = Object::default();
    group.insert("type", Value::from(FILTER2_TYPE_DEFAULT));
    group.insert("freq", Value::from(FILTER2_FREQ_DEFAULT));
    group.insert("Q", Value::from(FILTER2_Q_DEFAULT));
    Value::Object(group)
}

/// The amount of the filter envelope its stages switch on when it has none.
const IMPLICIT_FILTER_ENV_AMOUNT: f64 = 0.5;

/// What touching extra control `key` of `group` switches on, when it is one
/// that does: a stage of the filter envelope writes all four (the envelope
/// then has its own ADSR, at the values shown) and gives it an amount; a
/// control of the second filter turns it on. The paths it changed; `None`
/// when `group.key` is no such control.
pub(super) fn implicit_enable_extra(draft: &mut Object, group: &str, key: &str) -> Option<Vec<String>> {
    let mut changed = Vec::new();
    if group == "filterEnv" && FILTER_ENV_STAGES.contains(&key) {
        if !FILTER_ENV_STAGES.iter().any(|stage| super::get(draft, &format!("filterEnv.{stage}")).is_some()) {
            for stage in FILTER_ENV_STAGES {
                let path = format!("filterEnv.{stage}");
                let value = extra_value(draft, &path).unwrap_or(0.0);
                set(draft, &path, Value::from(value));
                changed.push(path);
            }
        }
        if number(draft, "filterEnv.filterEnvelopeAmount").unwrap_or(0.0) == 0.0 {
            set(draft, "filterEnv.filterEnvelopeAmount", Value::from(IMPLICIT_FILTER_ENV_AMOUNT));
            changed.push("filterEnv.filterEnvelopeAmount".into());
        }
        draft.insert("bypassFilterEnv", Value::Bool(false));
        return Some(changed);
    }
    if group == "filter2" {
        if draft.get("filter2").is_none() {
            draft.insert("filter2", filter2_defaults());
            changed.extend(["type", "freq", "Q"].map(|k| format!("filter2.{k}")));
        }
        draft.insert("bypassFilter2", Value::Bool(false));
        return Some(changed);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::{defaults, fields, get, hydrate, implicit_enable};
    use super::*;

    #[test]
    fn a_filter_stage_touched_writes_the_four_at_the_values_shown() {
        let mut d = defaults();
        set(&mut d, "envelope.attack", Value::from(0.02));
        assert_eq!(extra_value(&d, "filterEnv.attack"), Some(0.02));
        assert_eq!(extra_value(&d, "filterEnv.sustain"), Some(0.0));
        let changed = implicit_enable(&mut d, "filterEnv.sustain");
        assert_eq!(changed.len(), 5, "{changed:?}");
        assert_eq!(number(&d, "filterEnv.attack"), Some(0.02));
        assert_eq!(number(&d, "filterEnv.filterEnvelopeAmount"), Some(IMPLICIT_FILTER_ENV_AMOUNT));
        assert!(implicit_enable(&mut d, "filterEnv.decay").is_empty(), "already its own");
    }

    #[test]
    fn the_second_filter_and_the_voice_are_absent_until_set() {
        let mut d = defaults();
        hydrate(&mut d);
        assert!(d.get("filter2").is_none() && d.get("voice").is_none());
        assert_eq!(fields(&d, "voice"), ["voice.mode", "voice.glide", "voice.accent"]);
        assert!(set(&mut d, "voice.glide", Value::from(0.1)), "the group is made");
        assert_eq!(get(&d, "voice.glide"), Some(&Value::from(0.1)));
        assert_eq!(implicit_enable(&mut d, "filter2.freq"), ["filter2.type", "filter2.freq", "filter2.Q"]);
        assert_eq!(d.get("bypassFilter2"), Some(&Value::Bool(false)));
    }

    #[test]
    fn a_set_extra_group_shows_each_control_once() {
        let mut d = defaults();
        hydrate(&mut d);
        implicit_enable(&mut d, "filter2.freq");
        assert!(set(&mut d, "voice.glide", Value::from(0.1)));
        assert_eq!(fields(&d, "filter2"), ["filter2.type", "filter2.freq", "filter2.Q"]);
        assert_eq!(fields(&d, "voice"), ["voice.mode", "voice.glide", "voice.accent"]);
    }
}
