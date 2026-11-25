//! What the synth editor edits (`synth_editor/synth_preset_model.js`,
//! `synth_editor_constants.js` and the logic of `synth_editor.js`,
//! `groups_section.js` and `waveform_section.js`): a preset as JSON, filled
//! with the group defaults; its cards, their bypassed state and what turning
//! one on or off does; the groups a control quietly switches on; the LFO
//! modulation; and what the scope, the envelope and the filter curve plot.

use ordrumbox_core::json::{self, Object, Value};

use crate::slope;
use crate::synth_preset::FM_DEVIATION;
use crate::synth_preset::tags::TAG_KEYS;
use ordrumbox_dsp::synth::patch::LfoTarget;
pub use ordrumbox_dsp::synth::patch::MATRIX_SIZE;

pub use crate::synth_preset::{FILTER_TYPES, MATRIX_SOURCES, NOISE_MODES, OSC2_SYNCS, WAVES};
use crate::synth_preset::{PITCH_ENV_AMOUNT_RANGE, PITCH_ENV_DECAY_DEFAULT, PITCH_ENV_DECAY_RANGE};

mod choices;
pub use choices::*;
mod preview;
pub use preview::*;
/// A group's defaults (`SYNTH_GROUP_DEFAULTS`), in order.
const DEFAULTS: &str = r#"{"masterVolume":0.8,"subGain":0,"pitchPunch":0,
    "vco1":{"gain":1,"octave":0,"detune":0,"wave":"sine"},
    "vco2":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "vco3":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "filter":{"type":"lowpass","freq":400,"Q":1,"drive":0},
    "filterEnv":{"filterEnvelopeAmount":0},
    "fm":{"amount":0,"algo":0},
    "unison":{"count":1,"detune":0,"spread":0},
    "lfo":{"target":"NOT","wave":"sine","freq":0,"depth":0,"sync":"off"},
    "lfo2":{"target":"NOT","wave":"sine","freq":0,"depth":0,"sync":"off"},
    "noise":{"mix":0,"filterType":"highpass","filterFreq":1000,"filterQ":1},
    "envelope":{"attack":0,"decay":0.12,"sustain":1,"release":0.05},
    "modEnvelope":{"attack":0,"decay":0.12,"sustain":0,"release":0.1,"target":"off"},
    "pitchEnv":{"amount":0,"decay":0.05}}"#;

/// Fields the JS editor lacks, shown last in their card and written only once
/// set (absent, the engine's default applies): kept out of [`DEFAULTS`] so a
/// hydrated preset stays the JS one.
const EXTRA_FIELDS: [(&str, &str); 8] = [
    ("filter", "slope"),
    ("vco1", "width"),
    ("vco2", "width"),
    ("vco2", "ring"),
    ("vco2", "sync"),
    ("vco3", "width"),
    ("noise", "mode"),
    ("noise", "rate"),
];

#[expect(clippy::expect_used, reason = "a constant, read by the tests")]
pub fn defaults() -> Object {
    let mut defaults =
        json::parse(DEFAULTS).ok().and_then(|v| v.as_object().cloned()).expect("DEFAULTS is a JSON object");
    let mut matrix = Object::default();
    for n in 1..=MATRIX_SIZE {
        matrix.insert(format!("source{n}"), Value::from("off"));
        matrix.insert(format!("target{n}"), Value::from("NOT"));
        matrix.insert(format!("amount{n}"), Value::from(0.0));
    }
    defaults.insert("matrix", Value::Object(matrix));
    defaults
}

/// A numeric control (`SYNTH_PARAM_META`): its path, range, step, unit, label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meta {
    pub path: &'static str,
    pub min: f64,
    pub max: f64,
    /// What a reset puts it back to, and what a preset that lacks it plays with.
    pub default: f64,
    pub step: f64,
    pub unit: &'static str,
    pub label: Option<&'static str>,
    pub log: bool,
}

const fn meta(path: &'static str, min: f64, max: f64, step: f64, unit: &'static str) -> Meta {
    Meta { path, min, max, default: min, step, unit, label: None, log: false }
}

/// Every numeric control, in the JS order (the LFO targets follow it).
pub const METAS: [Meta; 35] = [
    meta("masterVolume", 0.0, 1.0, 0.01, ""),
    meta("subGain", 0.0, 1.0, 0.01, ""),
    meta("pitchPunch", 0.0, 1.0, 0.01, ""),
    meta("vco1.gain", 0.0, 1.0, 0.01, ""),
    meta("vco1.octave", -4.0, 4.0, 1.0, "oct"),
    meta("vco1.detune", -100.0, 100.0, 1.0, "ct"),
    meta("vco2.gain", 0.0, 1.0, 0.01, ""),
    meta("vco2.octave", -4.0, 4.0, 1.0, "oct"),
    meta("vco2.detune", -100.0, 100.0, 1.0, "ct"),
    meta("vco3.gain", 0.0, 1.0, 0.01, ""),
    meta("vco3.octave", -4.0, 4.0, 1.0, "oct"),
    meta("vco3.detune", -100.0, 100.0, 1.0, "ct"),
    Meta {
        log: true,
        ..meta("filter.freq", ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ, 1.0, "Hz")
    },
    meta("filter.Q", 0.1, 24.0, 0.1, ""),
    meta("filter.drive", 0.0, 1.0, 0.01, ""),
    Meta { label: Some("Env"), ..meta("filterEnv.filterEnvelopeAmount", 0.0, 1.0, 0.01, "") },
    Meta { log: true, ..meta("lfo.freq", 0.01, 5.0, 0.01, "Hz") },
    meta("lfo.depth", 0.0, 1.0, 0.01, ""),
    Meta { log: true, ..meta("lfo2.freq", 0.01, 5.0, 0.01, "Hz") },
    meta("lfo2.depth", 0.0, 1.0, 0.01, ""),
    meta("noise.mix", 0.0, 1.0, 0.01, ""),
    Meta {
        log: true,
        ..meta("noise.filterFreq", ordrumbox_dsp::MIN_FILTER_FREQ, ordrumbox_dsp::MAX_FILTER_FREQ, 1.0, "Hz")
    },
    meta("noise.filterQ", 0.1, 24.0, 0.1, ""),
    Meta { label: Some("FM"), ..meta("fm.amount", 0.0, 1.0, 0.01, "") },
    Meta { label: Some("Algo"), ..meta("fm.algo", 0.0, 4.0, 1.0, "") },
    Meta { label: Some("Layers"), ..meta("unison.count", 1.0, ordrumbox_dsp::synth::MAX_UNISON as f64, 1.0, "") },
    meta("unison.detune", 0.0, 100.0, 1.0, "ct"),
    meta("unison.spread", 0.0, 1.0, 0.01, ""),
    meta("envelope.attack", 0.0, 0.5, 0.001, "s"),
    meta("envelope.decay", 0.0, 1.0, 0.001, "s"),
    meta("envelope.sustain", 0.0, 1.0, 0.01, ""),
    meta("envelope.release", 0.0, 0.5, 0.001, "s"),
    meta("modEnvelope.attack", 0.0, 0.5, 0.001, "s"),
    meta("modEnvelope.decay", 0.0, 1.0, 0.001, "s"),
    meta("modEnvelope.sustain", 0.0, 1.0, 0.01, ""),
];

/// `modEnvelope.release` (`METAS` is full at 32).
const MOD_RELEASE: Meta = meta("modEnvelope.release", 0.0, 0.5, 0.001, "s");

/// The amount of each line of the matrix: either way, a fraction of what a
/// full swing is worth on the target.
pub const MATRIX_AMOUNTS: [Meta; MATRIX_SIZE] = [
    meta("matrix.amount1", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount2", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount3", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount4", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount5", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount6", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount7", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount8", -1.0, 1.0, 0.01, ""),
];

/// The pitch envelope's controls: its offset and its decay.
pub const PITCH_ENV_METAS: [Meta; 2] = [
    Meta {
        label: Some("Amount"),
        ..meta("pitchEnv.amount", PITCH_ENV_AMOUNT_RANGE.0, PITCH_ENV_AMOUNT_RANGE.1, 1.0, "st")
    },
    Meta {
        log: true,
        default: PITCH_ENV_DECAY_DEFAULT,
        label: Some("Decay"),
        ..meta("pitchEnv.decay", PITCH_ENV_DECAY_RANGE.0, PITCH_ENV_DECAY_RANGE.1, 0.001, "s")
    },
];

/// The controls of [`EXTRA_FIELDS`] that are knobs: the pulse width of each
/// oscillator, how far oscillator 1 ring-modulates oscillator 2, and the
/// clock of the console noise.
pub const EXTRA_METAS: [Meta; 5] = [
    Meta { default: 0.5, label: Some("Width"), ..meta("vco1.width", 0.02, 0.98, 0.01, "") },
    Meta { default: 0.5, label: Some("Width"), ..meta("vco2.width", 0.02, 0.98, 0.01, "") },
    Meta { default: 0.5, label: Some("Width"), ..meta("vco3.width", 0.02, 0.98, 0.01, "") },
    Meta { label: Some("Ring"), ..meta("vco2.ring", 0.0, 1.0, 0.01, "") },
    Meta {
        log: true,
        default: ordrumbox_dsp::synth::patch::NOISE_RATE_DEFAULT,
        label: Some("Rate"),
        ..meta(
            "noise.rate",
            crate::synth_preset::NOISE_RATE_RANGE.0,
            crate::synth_preset::NOISE_RATE_RANGE.1,
            1.0,
            "Hz",
        )
    },
];

pub fn metas() -> impl Iterator<Item = &'static Meta> {
    METAS.iter().chain(std::iter::once(&MOD_RELEASE)).chain(&MATRIX_AMOUNTS).chain(&EXTRA_METAS).chain(&PITCH_ENV_METAS)
}

/// The value of an extra control `draft` has not set, if `path` is one.
pub fn extra_default(path: &str) -> Option<f64> {
    EXTRA_METAS.iter().find(|m| m.path == path).map(|m| m.default)
}

pub fn find_meta(path: &str) -> Option<&'static Meta> {
    metas().find(|m| m.path == path)
}

/// What an LFO or a line of the matrix can modulate (`SYNTH_LFO_TARGETS`): off, then every control but the
/// LFOs', the unison's and the matrix's.
pub fn lfo_targets() -> Vec<&'static str> {
    std::iter::once("NOT")
        .chain(
            metas()
                .map(|m| m.path)
                .filter(|p| !["lfo.", "lfo2.", "unison.", "matrix."].iter().any(|group| p.starts_with(group))),
        )
        .collect()
}

/// What one unit of an LFO's wave is worth on a target, in the draft's
/// units: the synth's own swing, with an octave target in octaves (the
/// synth moves it in cents).
#[must_use]
pub fn lfo_scale(target: &str) -> f64 {
    let target = crate::synth_preset::lfo_target_named(target);
    let swing = ordrumbox_dsp::synth::lfo_swing(target);
    match target {
        LfoTarget::Osc1Octave | LfoTarget::Osc2Octave | LfoTarget::Osc3Octave => swing / 1200.0,
        _ => swing,
    }
}

/// The cards' bypass flags (`CARD_BYPASS_FLAGS`).
pub const BYPASS_FLAGS: [(&str, &str); 8] = [
    ("noise", "bypassNoise"),
    ("filter", "bypassFilter"),
    ("filterEnv", "bypassFilterEnv"),
    ("envelope", "bypassEnv"),
    ("lfo", "bypassLfo1"),
    ("lfo2", "bypassLfo2"),
    ("fm", "bypassFm"),
    ("modEnvelope", "bypassModEnv"),
];

/// The cards in order (`SYNTH_GROUP_ORDER`), and the master card's controls.
pub const GROUP_ORDER: [&str; 16] = [
    "scope",
    "vco1",
    "vco2",
    "vco3",
    "unison",
    "fm",
    "filter",
    "filterEnv",
    "lfo",
    "lfo2",
    "matrix",
    "noise",
    "master",
    "envelope",
    "pitchEnv",
    "modEnvelope",
];
pub const MASTER_KEYS: [&str; 3] = ["masterVolume", "subGain", "pitchPunch"];

/// The tabs and which cards each shows (`TAB_DEFS`, `GROUP_TAB`).
pub const TABS: [&str; 4] = ["OSC", "FLT", "MOD", "ENV"];

pub fn group_tab(group: &str) -> usize {
    match group {
        "filter" | "filterEnv" | "modEnvelope" => 1,
        "lfo" | "lfo2" | "matrix" | "noise" => 2,
        "envelope" | "pitchEnv" | "master" => 3,
        _ => 0,
    }
}

/// A card's title (`SYNTH_GROUP_LABELS`).
pub fn group_label(group: &str) -> String {
    match group {
        "scope" => "Scope".into(),
        "master" => "Master".into(),
        "filter" => "Filter".into(),
        "filterEnv" => "Filter Env".into(),
        "fm" => "FM".into(),
        "unison" => "Unison".into(),
        "lfo" => "LFO1".into(),
        "matrix" => "Matrix".into(),
        "envelope" => "Envelope".into(),
        "modEnvelope" => "Mod Env".into(),
        "pitchEnv" => "Pitch Env".into(),
        g if g.len() > 3 && g[..3].eq_ignore_ascii_case("vco") && g[3..].chars().all(|c| c.is_ascii_digit()) => {
            g.to_uppercase()
        }
        g => g.to_owned(),
    }
}

/// The value at a dotted `path` of `draft`.
pub fn get<'a>(draft: &'a Object, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut value = draft.get(parts.next()?)?;
    for part in parts {
        value = value.as_object()?.get(part)?;
    }
    Some(value)
}

pub fn number(draft: &Object, path: &str) -> Option<f64> {
    get(draft, path).and_then(Value::as_f64)
}

fn text<'a>(draft: &'a Object, path: &str) -> Option<&'a str> {
    get(draft, path).and_then(Value::as_str)
}

/// Set the value at a dotted `path` (its group must exist); whether it was set.
pub fn set(draft: &mut Object, path: &str, value: Value) -> bool {
    match path.split_once('.') {
        None => {
            draft.insert(path, value);
            true
        }
        Some((group, key)) => match draft.get_mut(group) {
            Some(Value::Object(object)) => {
                object.insert(key, value);
                true
            }
            _ => false,
        },
    }
}

/// Fill what `draft` lacks with the group defaults (`hydrate`).
pub fn hydrate(draft: &mut Object) {
    for (key, default) in defaults().iter() {
        let Value::Object(children) = default else {
            if draft.get(key).is_none() {
                draft.insert(key, default.clone());
            }
            continue;
        };
        let Some(Value::Object(group)) = draft.get_mut(key) else {
            draft.insert(key, default.clone());
            continue;
        };
        for (child, value) in children.iter() {
            if group.get(child).is_none() {
                group.insert(child, value.clone());
            }
        }
    }
}

/// Which cards show bypassed: an oscillator at gain
/// 0, a card whose bypass flag is set.
pub fn bypassed(draft: &Object, group: &str) -> bool {
    if matches!(group, "vco1" | "vco2" | "vco3") {
        return number(draft, &format!("{group}.gain")) == Some(0.0);
    }
    BYPASS_FLAGS.iter().find(|(g, _)| *g == group).is_some_and(|(_, flag)| draft.get(flag).is_some_and(Value::is_true))
}

/// Whether a card has a bypass state (the oscillators and the flagged cards).
pub fn has_power(group: &str) -> bool {
    matches!(group, "vco1" | "vco2" | "vco3") || BYPASS_FLAGS.iter().any(|(g, _)| *g == group)
}

/// The cards of `draft`, in order: the known ones,
/// then any other group it has (not a bypass flag nor a tag), by name.
pub fn groups(draft: &Object) -> Vec<String> {
    let mut groups: Vec<String> = GROUP_ORDER.iter().map(|g| (*g).to_owned()).collect();
    for (key, _) in draft.iter() {
        let is_card = !MASTER_KEYS.contains(&key) && !TAG_KEYS.contains(&key) && !key.starts_with("bypass");
        if is_card && !groups.iter().any(|g| g == key) {
            groups.push(key.to_owned());
        }
    }
    let rank = |g: &str| GROUP_ORDER.iter().position(|o| *o == g);
    groups.sort_by(|a, b| match (rank(a), rank(b)) {
        (Some(x), Some(y)) => x.cmp(&y),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.cmp(b),
    });
    groups
}

/// A card's fields: the master keys, a group's own
/// keys known to its defaults, or the value itself.
pub fn fields(draft: &Object, group: &str) -> Vec<String> {
    if group == "master" {
        return MASTER_KEYS.iter().map(|k| (*k).to_owned()).collect();
    }
    let defaults = defaults();
    match draft.get(group) {
        Some(Value::Object(object)) => object
            .iter()
            .filter(|(key, _)| match defaults.get(group) {
                Some(Value::Object(known)) => known.get(key).is_some(),
                _ => true,
            })
            .map(|(key, _)| format!("{group}.{key}"))
            .chain(EXTRA_FIELDS.iter().filter(|(g, _)| *g == group).map(|(g, key)| format!("{g}.{key}")))
            .collect(),
        _ => vec![group.to_owned()],
    }
}

/// A card turned on or off by its power button: an
/// oscillator keeps its gain aside and goes to 0 (and gets it back), a
/// flagged card sets its flag (the envelope, turned on, is reset).
pub fn toggle_card(draft: &mut Object, group: &str) {
    let off = !bypassed(draft, group);
    if matches!(group, "vco1" | "vco2" | "vco3") {
        let Some(Value::Object(vco)) = draft.get_mut(group) else { return };
        if off {
            let gain = vco.get("gain").and_then(Value::as_f64).unwrap_or(0.0);
            vco.insert("_savedGain", Value::from(gain));
            vco.insert("gain", Value::from(0.0));
        } else {
            let saved = vco.remove("_savedGain").and_then(|v| v.as_f64());
            vco.insert("gain", Value::from(saved.unwrap_or(if group == "vco1" { 1.0 } else { 0.0 })));
        }
    } else if let Some((_, flag)) = BYPASS_FLAGS.iter().find(|(g, _)| *g == group) {
        draft.insert(*flag, Value::Bool(off));
        if group == "envelope" && !off {
            draft.insert("_resetEnv", Value::Bool(true));
        }
    }
}

/// A group off by default switched on when one of its controls is touched
///: an oscillator at gain 0 to 0.5, FM to 0.3, the noise
/// to 0.15, an LFO onto the filter (depth 0.5 unless that is the depth
/// being set), the mod envelope onto the filter. The paths it changed.
pub fn implicit_enable(draft: &mut Object, path: &str) -> Vec<String> {
    let Some((group, key)) = path.split_once('.') else { return Vec::new() };
    let mut changed = Vec::new();
    let gain = |d: &Object, g: &str| number(d, &format!("{g}.gain")).unwrap_or(0.0);
    if matches!(group, "vco2" | "vco3")
        && matches!(key, "octave" | "detune" | "wave" | "width" | "ring" | "sync")
        && gain(draft, group) == 0.0
    {
        if let Some(Value::Object(vco)) = draft.get_mut(group) {
            vco.insert("gain", Value::from(0.5));
            vco.remove("_savedGain");
        }
        changed.push(format!("{group}.gain"));
    } else if group == "fm" && key == "algo" && number(draft, "fm.amount").unwrap_or(0.0) == 0.0 {
        set(draft, "fm.amount", Value::from(0.3));
        draft.insert("bypassFm", Value::Bool(false));
        changed.push("fm.amount".into());
    } else if group == "noise"
        && (key.starts_with("filter") || matches!(key, "mode" | "rate"))
        && number(draft, "noise.mix").unwrap_or(0.0) == 0.0
    {
        set(draft, "noise.mix", Value::from(0.15));
        draft.insert("bypassNoise", Value::Bool(false));
        changed.push("noise.mix".into());
    } else if matches!(group, "lfo" | "lfo2")
        && matches!(key, "wave" | "freq" | "depth" | "sync")
        && text(draft, &format!("{group}.target")) == Some("NOT")
    {
        set(draft, &format!("{group}.target"), Value::from("filter.freq"));
        draft.insert(if group == "lfo" { "bypassLfo1" } else { "bypassLfo2" }, Value::Bool(false));
        if key != "depth" && number(draft, &format!("{group}.depth")).unwrap_or(0.0) == 0.0 {
            set(draft, &format!("{group}.depth"), Value::from(0.5));
            changed.push(format!("{group}.depth"));
        }
        changed.push(format!("{group}.target"));
    } else if group == "matrix"
        && let Some(n) = key.strip_prefix("source")
        && text(draft, &format!("matrix.target{n}")) == Some("NOT")
    {
        // A source touched with nowhere to go: onto the filter, at half (an `off` source adds nothing).
        set(draft, &format!("matrix.target{n}"), Value::from("filter.freq"));
        changed.push(format!("matrix.target{n}"));
        if number(draft, &format!("matrix.amount{n}")).unwrap_or(0.0) == 0.0 {
            set(draft, &format!("matrix.amount{n}"), Value::from(0.5));
            changed.push(format!("matrix.amount{n}"));
        }
    } else if group == "modEnvelope"
        && matches!(key, "attack" | "decay" | "sustain" | "release")
        && text(draft, "modEnvelope.target") == Some("off")
    {
        set(draft, "modEnvelope.target", Value::from("filter"));
        draft.insert("bypassModEnv", Value::Bool(false));
        changed.push("modEnvelope.target".into());
    }
    changed
}

/// The module pills under the cards: label, active.
pub fn modules(draft: &Object) -> Vec<(&'static str, bool)> {
    let n = |path: &str| number(draft, path).unwrap_or(0.0);
    let flag = |key: &str| draft.get(key).is_some_and(Value::is_true);
    let gain = |v: usize| number(draft, &format!("vco{v}.gain")).unwrap_or(if v == 1 { 1.0 } else { 0.0 });
    let target = |path: &str, off: &str| text(draft, path).unwrap_or(off) != off;
    let env_amount = number(draft, "filterEnv.filterEnvelopeAmount")
        .or_else(|| number(draft, "filter.filterEnvelopeAmount"))
        .unwrap_or(0.0);
    vec![
        ("VCO1", gain(1) > 0.01),
        ("VCO2", gain(2) > 0.01),
        ("VCO3", gain(3) > 0.01),
        ("Filter", !flag("bypassFilter")),
        ("Envelope", !flag("bypassEnv")),
        ("LFO1", !flag("bypassLfo1") && target("lfo.target", "NOT") && n("lfo.depth") > 0.0),
        ("LFO2", !flag("bypassLfo2") && target("lfo2.target", "NOT") && n("lfo2.depth") > 0.0),
        ("FM", !flag("bypassFm") && n("fm.amount") > 0.001),
        ("Mod", !flag("bypassModEnv") && target("modEnvelope.target", "off")),
        ("Noise", !flag("bypassNoise") && n("noise.mix") > 0.001),
        ("Filter Env", !flag("bypassFilterEnv") && env_amount > 0.001),
        ("Pitch Env", n("pitchEnv.amount") != 0.0),
    ]
}

/// The name of the first free `new_preset`, `new_preset_1`….
pub fn new_preset_name(keys: &[&str]) -> String {
    let mut name = "new_preset".to_owned();
    let mut n = 1;
    while keys.contains(&name.as_str()) {
        name = format!("new_preset_{n}");
        n += 1;
    }
    name
}

/// The preset to show after deleting `deleted` from `keys` (sorted, before
/// the deletion: `deletePreset`): the next one, else the previous.
pub fn after_delete<'a>(keys: &[&'a str], deleted: &str) -> Option<&'a str> {
    let index = keys.iter().position(|k| *k == deleted)?;
    let next = if index + 1 < keys.len() { index } else { index.checked_sub(1)? };
    if keys[next] == deleted { keys.get((index + 1) % keys.len()).copied() } else { Some(keys[next]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_enable_and_cards() {
        let mut d = defaults();
        assert_eq!(implicit_enable(&mut d, "vco2.wave"), ["vco2.gain"]);
        assert_eq!(number(&d, "vco2.gain"), Some(0.5));
        assert!(implicit_enable(&mut d, "vco2.wave").is_empty());
        assert_eq!(implicit_enable(&mut d, "lfo.freq"), ["lfo.depth", "lfo.target"]);
        assert_eq!(text(&d, "lfo.target"), Some("filter.freq"));
        assert_eq!(implicit_enable(&mut d, "lfo2.depth"), ["lfo2.target"]);
        assert_eq!(number(&d, "lfo2.depth"), Some(0.0));
        assert_eq!(implicit_enable(&mut d, "fm.algo"), ["fm.amount"]);
        assert_eq!(implicit_enable(&mut d, "noise.filterQ"), ["noise.mix"]);
        assert_eq!(implicit_enable(&mut d, "modEnvelope.decay"), ["modEnvelope.target"]);
        assert!(implicit_enable(&mut d, "vco1.wave").is_empty());
        // Power: an oscillator keeps its gain aside.
        toggle_card(&mut d, "vco2");
        assert!(bypassed(&d, "vco2"));
        toggle_card(&mut d, "vco2");
        assert_eq!(number(&d, "vco2.gain"), Some(0.5));
        toggle_card(&mut d, "envelope");
        assert!(bypassed(&d, "envelope"));
        toggle_card(&mut d, "envelope");
        assert_eq!(d.get("_resetEnv"), Some(&Value::Bool(true)));
    }

    #[test]
    fn the_scope_shows_the_sync_and_the_ring() {
        let mut d = defaults();
        set(&mut d, "vco2.gain", Value::from(0.5));
        set(&mut d, "vco2.detune", Value::from(50.0));
        let plain = scope(&d, 0.0, 120.0);
        set(&mut d, "vco2.sync", Value::from("vco1"));
        let synced = scope(&d, 0.0, 120.0);
        assert_ne!(synced, plain);
        set(&mut d, "vco2.ring", Value::from(1.0));
        assert_ne!(scope(&d, 0.0, 120.0), synced);
    }

    #[test]
    fn preset_names() {
        assert_eq!(new_preset_name(&["a"]), "new_preset");
        assert_eq!(new_preset_name(&["new_preset", "new_preset_1"]), "new_preset_2");
        let keys = ["a", "b", "c"];
        assert_eq!(after_delete(&keys, "b"), Some("c"));
        assert_eq!(after_delete(&keys, "c"), Some("b"));
        assert_eq!(after_delete(&keys, "a"), Some("b"));
    }

    #[test]
    fn plots() {
        let mut d = defaults();
        let wave = scope(&d, 0.0, 120.0);
        assert_eq!(wave.len(), 1024);
        assert!((wave.iter().fold(0.0f64, |m, s| m.max(s.abs())) - 0.8).abs() < 1e-9);
        set(&mut d, "fm.amount", Value::from(0.5));
        set(&mut d, "vco2.gain", Value::from(0.5));
        assert_ne!(scope(&d, 0.0, 120.0), wave);
        let (points, total) = envelope(&d);
        assert_eq!((points.len(), total), (5, 0.5));
        let low = filter_response("lowpass", 1000.0, 1.0, 12.0, 200);
        assert!(low[0] > -1.0 && low[200] == -30.0);
        let high = filter_response("highpass", 1000.0, 1.0, 12.0, 200);
        assert!(high[0] == -30.0 && high[200] > -1.0);
        // The point of frequency `f`: 200 points span 3 decades from 20 Hz.
        let at = |f: f64| (200.0 * (f / 20.0f64).log10() / 3.0).round() as usize;
        let steep = filter_response("lowpass", 1000.0, std::f64::consts::FRAC_1_SQRT_2, 24.0, 200);
        assert!((steep[at(1000.0)] + 3.0).abs() < 0.5 && steep[at(2000.0)] < -23.0, "{}", steep[at(2000.0)]);
        assert_eq!(
            modules(&defaults()).iter().filter(|(_, on)| *on).map(|(l, _)| *l).collect::<Vec<_>>(),
            ["VCO1", "Filter", "Envelope"]
        );
    }
}
