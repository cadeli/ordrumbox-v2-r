//! What the synth editor edits: a preset as JSON, filled
//! with the group defaults; its cards, their bypassed state and what turning
//! one on or off does; the groups a control quietly switches on; the LFO
//! modulation; and what the scope, the envelope and the filter curve plot.

use ordrumbox_core::json::{self, Object, Value};

use crate::slope;
use crate::synth_preset::tags::TAG_KEYS;
use ordrumbox_dsp::synth::FM_DEVIATION;
use ordrumbox_dsp::synth::patch::LfoTarget;
use ordrumbox_dsp::synth::patch::MATRIX_SIZE;

use crate::synth_preset::{ATTACK_MAX, DECAY_MAX, LFO_FREQ_MAX, RELEASE_MAX};
use crate::synth_preset::{FILTER_TYPES, MATRIX_SOURCES, NOISE_MODES, OSC_BYPASS_FLAGS, OSC2_SYNCS, WAVES};
use crate::synth_preset::{PITCH_ENV_AMOUNT_RANGE, PITCH_ENV_DECAY_DEFAULT, PITCH_ENV_DECAY_RANGE};

mod choices;
pub use choices::*;
mod matrix_lines;
pub use matrix_lines::*;
mod preview;
pub use preview::*;
mod power;
pub use power::*;
mod extras;
use extras::{EXTRA_FIELDS, EXTRA_GROUPS, implicit_enable_extra, is_extra};
pub use extras::{EXTRA_METAS, SWITCH_LABELS, SWITCHES, extra_default, extra_value};
/// A group's defaults (`SYNTH_GROUP_DEFAULTS`), in order.
const DEFAULTS: &str = r#"{"masterVolume":0.8,"subGain":0,
    "vco1":{"gain":1,"octave":0,"detune":0,"wave":"sine"},
    "vco2":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "vco3":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "filter":{"type":"lowpass","freq":400,"Q":1,"drive":0},
    "filterEnv":{"filterEnvelopeAmount":0},
    "fm":{"amount":0,"algo":0},
    "unison":{"count":1,"detune":0,"spread":0},
    "lfo":{"wave":"sine","freq":0,"sync":"off"},
    "lfo2":{"wave":"sine","freq":0,"sync":"off"},
    "noise":{"mix":0,"filterType":"highpass","filterFreq":1000,"filterQ":1},
    "envelope":{"attack":0,"decay":0.12,"sustain":1,"release":0.05},
    "modEnvelope":{"attack":0,"decay":0.12,"sustain":0,"release":0.1},
    "pitchEnv":{"amount":0,"decay":0.05}}"#;

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

/// Every numeric control, in the editor's order (the LFO targets follow it).
pub const METAS: [Meta; 32] = [
    meta("masterVolume", 0.0, 1.0, 0.01, ""),
    Meta { label: Some("Sub"), ..meta("subGain", 0.0, 1.0, 0.01, "") },
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
    Meta { log: true, ..meta("lfo.freq", LFO_FREQ_MIN, LFO_FREQ_MAX, 0.01, "Hz") },
    Meta { log: true, ..meta("lfo2.freq", LFO_FREQ_MIN, LFO_FREQ_MAX, 0.01, "Hz") },
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
    time("envelope.attack", ATTACK_MAX),
    time("envelope.decay", DECAY_MAX),
    meta("envelope.sustain", 0.0, 1.0, 0.01, ""),
    time("envelope.release", RELEASE_MAX),
    time("modEnvelope.attack", ATTACK_MAX),
    time("modEnvelope.decay", DECAY_MAX),
    meta("modEnvelope.sustain", 0.0, 1.0, 0.01, ""),
];

/// `modEnvelope.release`.
const MOD_RELEASE: Meta = time("modEnvelope.release", RELEASE_MAX);

/// The slowest LFO a knob sets, Hz (0, which a preset may hold, is still).
const LFO_FREQ_MIN: f64 = 0.01;
/// The shortest time of an envelope a knob sets, seconds: its knob spans
/// milliseconds to seconds, the shortest stage as fine as the longest.
pub(crate) const TIME_MIN: f64 = 0.001;

/// An envelope's time at `path`, up to `max` seconds, on a logarithmic knob.
const fn time(path: &'static str, max: f64) -> Meta {
    Meta { log: true, ..meta(path, TIME_MIN, max, 0.001, "s") }
}

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
    meta("matrix.amount9", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount10", -1.0, 1.0, 0.01, ""),
    meta("matrix.amount11", -1.0, 1.0, 0.01, ""),
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

pub fn metas() -> impl Iterator<Item = &'static Meta> {
    METAS.iter().chain(std::iter::once(&MOD_RELEASE)).chain(&MATRIX_AMOUNTS).chain(&EXTRA_METAS).chain(&PITCH_ENV_METAS)
}

pub fn find_meta(path: &str) -> Option<&'static Meta> {
    metas().find(|m| m.path == path)
}

/// The name under the control of field `path`: its label, a switch's short
/// name ([`SWITCH_LABELS`]), else its key.
#[must_use]
pub fn field_label(path: &str) -> &str {
    let switch = || SWITCH_LABELS.iter().find(|(p, _)| *p == path).map(|(_, label)| *label);
    let key = path.rsplit('.').next().unwrap_or(path);
    find_meta(path).and_then(|m| m.label).or_else(switch).unwrap_or(key)
}

/// What a line of the matrix can modulate (`SYNTH_LFO_TARGETS`): off, every control that is
/// a target (not the LFOs', the unison's, the matrix's, the voice's nor the filter envelope's
/// stages), each with the others of its card (an oscillator's width and ring with its gain),
/// then the effects on several ([`crate::synth_preset::EFFECT_TARGETS`]).
pub fn lfo_targets() -> Vec<&'static str> {
    let mut controls: Vec<&'static str> =
        metas().map(|m| m.path).filter(|p| crate::synth_preset::lfo_target_named(p) != LfoTarget::Off).collect();
    let group = |path: &str| path.split_once('.').map_or(path, |(group, _)| group).to_owned();
    let groups: Vec<String> = controls.iter().map(|p| group(p)).collect();
    // A stable sort: the cards in the order they first come, their controls in theirs.
    controls.sort_by_key(|p| groups.iter().position(|g| *g == group(p)));
    std::iter::once("NOT").chain(controls).chain(crate::synth_preset::EFFECT_TARGETS).collect()
}

/// What one unit of a matrix source is worth on a target, in the draft's
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

/// The cards in order (`SYNTH_GROUP_ORDER`), and the master card's controls.
pub const GROUP_ORDER: [&str; 18] = [
    "scope",
    "vco1",
    "vco2",
    "vco3",
    "voice",
    "unison",
    "fm",
    "noise",
    "filter",
    "filterEnv",
    "filter2",
    "envelope",
    "pitchEnv",
    "lfo",
    "lfo2",
    "modEnvelope",
    "matrix",
    "master",
];
pub const MASTER_KEYS: [&str; 1] = ["masterVolume"];

/// The keys of a preset that hold one number (each shown in a card: the
/// master's, or one of [`BORROWED_FIELDS`]).
const NUMBER_KEYS: [&str; 2] = ["masterVolume", "subGain"];

/// The fields shown last in another card than their group's: the card, the
/// field. The sub-oscillator, an octave under VCO1, in VCO1's strip; the
/// filter envelope's amount in the filter's card, its stages in a card of
/// their own.
pub const BORROWED_FIELDS: [(&str, &str); 2] = [("vco1", "subGain"), ("filter", "filterEnv.filterEnvelopeAmount")];

/// The fields of [`BORROWED_FIELDS`] that card `group` shows.
pub fn borrowed_fields(group: &str) -> impl Iterator<Item = &'static str> + '_ {
    BORROWED_FIELDS.iter().filter(move |(card, _)| *card == group).map(|(_, path)| *path)
}

/// The card that shows field `path`, when it is not its group's.
#[must_use]
pub fn borrowing_card(path: &str) -> Option<&'static str> {
    BORROWED_FIELDS.iter().find(|(_, field)| *field == path).map(|(card, _)| *card)
}

/// A card's title (`SYNTH_GROUP_LABELS`).
pub fn group_label(group: &str) -> String {
    match group {
        "scope" => "Scope".into(),
        "master" => "Master".into(),
        "filter" => "Filter".into(),
        "filterEnv" => "Filter Env".into(),
        "filter2" => "Filter 2".into(),
        "voice" => "Voice".into(),
        "fm" => "FM".into(),
        "unison" => "Unison".into(),
        "lfo" => "LFO1".into(),
        "lfo2" => "LFO2".into(),
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

/// Whether oscillator 2 is synced to oscillator 1 (`vco2.sync`).
#[must_use]
pub fn is_osc2_synced(draft: &Object) -> bool {
    text(draft, "vco2.sync") == Some(OSC2_SYNCS[1])
}

/// The `vco2.sync` value that syncs oscillator 2 (`on`) or frees it.
#[must_use]
pub fn osc2_sync_value(on: bool) -> Value {
    Value::from(if on { OSC2_SYNCS[1] } else { OSC2_SYNCS[0] })
}

/// Set the value at a dotted `path` (its group must exist, unless `path`
/// is an extra field: its group is then made); whether it was set.
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
            None if is_extra(path) => {
                let mut object = Object::default();
                object.insert(key, value);
                draft.insert(group, Value::Object(object));
                true
            }
            _ => false,
        },
    }
}

/// Fill what `draft` lacks with the group defaults (`hydrate`), once its older
/// LFO settings are brought up to date ([`crate::synth_preset::legacy_modulation`]).
pub fn hydrate(draft: &mut Object) {
    crate::synth_preset::legacy_modulation::upgrade(draft);
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

/// The cards of `draft`, in order: the known ones,
/// then any other group it has (an object of fields, not a tag), by name. A
/// key of another kind (a bypass flag, a text such as a stray `profile`) is
/// kept in the preset but makes no card.
pub fn groups(draft: &Object) -> Vec<String> {
    let mut groups: Vec<String> = GROUP_ORDER.iter().map(|g| (*g).to_owned()).collect();
    for (key, value) in draft.iter() {
        let is_card = matches!(value, Value::Object(_)) && !NUMBER_KEYS.contains(&key) && !TAG_KEYS.contains(&key);
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
/// keys known to its defaults but those another card shows (then its extra
/// fields, then those it borrows), or the value itself.
pub fn fields(draft: &Object, group: &str) -> Vec<String> {
    if group == "master" {
        return MASTER_KEYS.iter().map(|k| (*k).to_owned()).collect();
    }
    let extras = || EXTRA_FIELDS.iter().filter(move |(g, _)| *g == group).map(|(g, key)| format!("{g}.{key}"));
    let defaults = defaults();
    match draft.get(group) {
        Some(Value::Object(object)) => object
            .iter()
            .filter(|(key, _)| match defaults.get(group) {
                Some(Value::Object(known)) => known.get(key).is_some(),
                // A group of extra fields alone (`filter2`, `voice`): they come last, from `extras`.
                _ => !EXTRA_FIELDS.contains(&(group, *key)),
            })
            .map(|(key, _)| format!("{group}.{key}"))
            .filter(|path| borrowing_card(path).is_none())
            .chain(extras())
            .chain(borrowed_fields(group).map(str::to_owned))
            .collect(),
        None if EXTRA_GROUPS.contains(&group) => extras().collect(),
        _ => vec![group.to_owned()],
    }
}

/// A group off by default switched on when one of its controls is touched: an oscillator at gain 0 to 0.5, FM to 0.3, the noise
/// to 0.15, an LFO onto the filter (a free line of the matrix at 0.5, when
/// no line reads it), the mod envelope opening the filter (a free line at
/// 1). The paths it changed.
pub fn implicit_enable(draft: &mut Object, path: &str) -> Vec<String> {
    let Some((group, key)) = path.split_once('.') else { return Vec::new() };
    if let Some(changed) = implicit_enable_extra(draft, group, key) {
        return changed;
    }
    let mut changed = Vec::new();
    let gain = |d: &Object, g: &str| number(d, &format!("{g}.gain")).unwrap_or(0.0);
    if matches!(group, "vco2" | "vco3")
        && matches!(key, "octave" | "detune" | "wave" | "width" | "ring" | "sync")
        && gain(draft, group) == 0.0
        && bypassed(draft, group)
    {
        if let Some(Value::Object(vco)) = draft.get_mut(group) {
            vco.insert("gain", Value::from(IMPLICIT_GAIN));
            vco.remove(SAVED_GAIN);
        }
        if let Some((_, flag)) = BYPASS_FLAGS.iter().find(|(g, _)| *g == group) {
            draft.insert(*flag, Value::Bool(false));
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
    } else if let Some(lfo) = LFOS.iter().find(|lfo| lfo.group == group)
        && matches!(key, "wave" | "freq" | "sync" | "delay" | "retrigger")
        && !is_routed(draft, lfo.source)
    {
        // An LFO touched with nowhere to go: onto the filter, at half, on the first free line.
        draft.insert(lfo.bypass_flag, Value::Bool(false));
        changed.extend(add_route(draft, lfo.source, "filter.freq", 0.5));
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
        && !is_routed(draft, MOD_ENV_SOURCE)
    {
        // Onto the filter, all the way, on the first free line.
        draft.insert("bypassModEnv", Value::Bool(false));
        changed.extend(add_route(draft, MOD_ENV_SOURCE, "filter.open", 1.0));
    }
    changed
}

/// An LFO of the editor: its group, the matrix source it is, its bypass flag.
#[derive(Debug)]
pub struct Lfo {
    pub group: &'static str,
    pub source: &'static str,
    pub bypass_flag: &'static str,
}

/// The two LFOs.
pub const LFOS: [Lfo; 2] = [
    Lfo { group: "lfo", source: "lfo1", bypass_flag: "bypassLfo1" },
    Lfo { group: "lfo2", source: "lfo2", bypass_flag: "bypassLfo2" },
];

/// The lines of the matrix of `draft` that read `source` and move something:
/// target and amount.
pub fn routes_of<'a>(draft: &'a Object, source: &'a str) -> impl Iterator<Item = (&'a str, f64)> + 'a {
    (1..=MATRIX_SIZE).filter_map(move |n| {
        let target = text(draft, &format!("matrix.target{n}")).filter(|t| *t != "NOT")?;
        let amount = number(draft, &format!("matrix.amount{n}")).filter(|a| *a != 0.0)?;
        (text(draft, &format!("matrix.source{n}")) == Some(source)).then_some((target, amount))
    })
}

/// Whether a line of the matrix of `draft` sends `source` somewhere.
#[must_use]
pub fn is_routed(draft: &Object, source: &str) -> bool {
    routes_of(draft, source).next().is_some()
}

/// The matrix source the modulation envelope is.
const MOD_ENV_SOURCE: &str = "modEnv";

/// Set the first line of the matrix of `draft` that reads nothing to
/// `source` → `target` at `amount`; the paths it changed (none when every
/// line is in use).
fn add_route(draft: &mut Object, source: &str, target: &str, amount: f64) -> Vec<String> {
    let Some(n) = (1..=MATRIX_SIZE).find(|n| text(draft, &format!("matrix.source{n}")) == Some("off")) else {
        return Vec::new();
    };
    set(draft, &format!("matrix.source{n}"), Value::from(source));
    set(draft, &format!("matrix.target{n}"), Value::from(target));
    set(draft, &format!("matrix.amount{n}"), Value::from(amount));
    ["source", "target", "amount"].map(|name| format!("matrix.{name}{n}")).into()
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
/// the deletion): the next one, else the previous.
pub fn after_delete<'a>(keys: &[&'a str], deleted: &str) -> Option<&'a str> {
    let index = keys.iter().position(|k| *k == deleted)?;
    let next = if index + 1 < keys.len() { index } else { index.checked_sub(1)? };
    if keys[next] == deleted { keys.get((index + 1) % keys.len()).copied() } else { Some(keys[next]) }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_wave_shape_is_two_cycles_of_it() {
        let square = wave_shape("square", 9);
        assert_eq!(square, [1.0, 1.0, -1.0, -1.0, 1.0, 1.0, -1.0, -1.0, -1.0]);
        let saw = wave_shape("sawtooth", 5);
        assert_eq!(saw, [-1.0, 0.0, -1.0, 0.0, 1.0]);
        assert!(wave_shape("random", 48).iter().all(|v| (-1.0..=1.0).contains(v)));
    }

    use super::*;

    #[test]
    fn implicit_enable_and_cards() {
        let mut d = defaults();
        assert_eq!(implicit_enable(&mut d, "vco2.wave"), ["vco2.gain"]);
        assert_eq!(number(&d, "vco2.gain"), Some(0.5));
        assert!(implicit_enable(&mut d, "vco2.wave").is_empty());
        // An LFO touched with nowhere to go is routed to the filter, on the first free line.
        assert_eq!(implicit_enable(&mut d, "lfo.freq"), ["matrix.source1", "matrix.target1", "matrix.amount1"]);
        assert_eq!(text(&d, "matrix.source1"), Some("lfo1"));
        assert_eq!(text(&d, "matrix.target1"), Some("filter.freq"));
        assert!(implicit_enable(&mut d, "lfo.wave").is_empty());
        assert_eq!(implicit_enable(&mut d, "lfo2.sync"), ["matrix.source2", "matrix.target2", "matrix.amount2"]);
        assert_eq!(implicit_enable(&mut d, "fm.algo"), ["fm.amount"]);
        assert_eq!(implicit_enable(&mut d, "noise.filterQ"), ["noise.mix"]);
        assert_eq!(
            implicit_enable(&mut d, "modEnvelope.decay"),
            ["matrix.source3", "matrix.target3", "matrix.amount3"]
        );
        assert_eq!(text(&d, "matrix.target3"), Some("filter.open"));
        assert!(implicit_enable(&mut d, "vco1.wave").is_empty());
        // Power: an oscillator keeps its gain, under its flag.
        toggle_card(&mut d, "vco2");
        assert!(bypassed(&d, "vco2"));
        assert_eq!((number(&d, "vco2.gain"), d.get("bypassVco2")), (Some(0.5), Some(&Value::Bool(true))));
        toggle_card(&mut d, "vco2");
        assert!(!bypassed(&d, "vco2"));
        // Silent, an oscillator is off unless it modulates; turned on, it gets a gain.
        assert!(bypassed(&d, "vco3"));
        set(&mut d, "fm.algo", Value::from(1.0));
        assert!(!bypassed(&d, "vco3"), "VCO3 modulates VCO1");
        set(&mut d, "fm.algo", Value::from(0.0));
        toggle_card(&mut d, "vco3");
        assert_eq!(number(&d, "vco3.gain"), Some(IMPLICIT_GAIN));
        // An LFO at 0 Hz is off; turned on, it moves.
        assert!(bypassed(&d, "lfo2"));
        set(&mut d, "lfo2.sync", Value::from("1/4"));
        assert!(!bypassed(&d, "lfo2"), "synced, it moves");
        set(&mut d, "lfo2.sync", Value::from("off"));
        toggle_card(&mut d, "lfo2");
        assert_eq!((bypassed(&d, "lfo2"), number(&d, "lfo2.freq")), (false, Some(LFO_ON_HZ)));
        toggle_card(&mut d, "envelope");
        assert!(bypassed(&d, "envelope"));
        toggle_card(&mut d, "envelope");
        assert_eq!(d.get("_resetEnv"), Some(&Value::Bool(true)));
    }

    #[test]
    fn the_sub_and_the_filter_env_amount_show_in_other_cards() {
        let d = defaults();
        let groups = groups(&d);
        assert!(!groups.iter().any(|g| g == "subGain"), "{groups:?}");
        let mut stray = d.clone();
        stray.insert("profile", Value::from("bass"));
        stray.insert("extra", Value::Object(Object::new()));
        let stray_groups = super::groups(&stray);
        assert!(!stray_groups.iter().any(|g| g == "profile"), "a text makes no card");
        assert_eq!(stray_groups.last().map(String::as_str), Some("extra"), "an object of fields does");
        // The filter envelope's card has its stages; its amount stays in the filter's.
        let filter_env = fields(&d, "filterEnv");
        assert_eq!(filter_env.first().map(String::as_str), Some("filterEnv.attack"), "{filter_env:?}");
        assert!(!filter_env.iter().any(|f| f == "filterEnv.filterEnvelopeAmount"));
        assert_eq!(fields(&d, "master"), ["masterVolume"]);
        assert_eq!(fields(&d, "vco1").last().map(String::as_str), Some("subGain"));
        assert_eq!(fields(&d, "filter").last().map(String::as_str), Some("filterEnv.filterEnvelopeAmount"));
        assert_eq!(borrowing_card("subGain"), Some("vco1"));
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
        let (points, total) = envelope(&d, "envelope");
        assert_eq!((points.len(), total), (5, 0.5));
        assert_eq!(points[2], (0.12, 1.0), "the envelope's decay to its sustain");
        let (mod_env, _) = envelope(&d, "modEnvelope");
        assert_eq!(mod_env[2], (0.12, 0.0), "the mod envelope's sustain");
        let (filter_env, _) = envelope(&d, "filterEnv");
        let attack = extra_value(&d, "filterEnv.attack").expect("a stage");
        assert_eq!(filter_env[1], (attack, 1.0), "unset, a stage of the filter envelope plays as its default");
        let low = filter_response("lowpass", 1000.0, 1.0, 12.0, 200);
        assert!(low[0] > -1.0 && low[200] == -30.0);
        let high = filter_response("highpass", 1000.0, 1.0, 12.0, 200);
        assert!(high[0] == -30.0 && high[200] > -1.0);
        // The point of frequency `f`: 200 points span 3 decades from 20 Hz.
        let at = |f: f64| (200.0 * (f / 20.0f64).log10() / 3.0).round() as usize;
        let steep = filter_response("lowpass", 1000.0, std::f64::consts::FRAC_1_SQRT_2, 24.0, 200);
        assert!((steep[at(1000.0)] + 3.0).abs() < 0.5 && steep[at(2000.0)] < -23.0, "{}", steep[at(2000.0)]);
    }
}
