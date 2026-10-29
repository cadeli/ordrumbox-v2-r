//! What the synth editor edits (`synth_editor/synth_preset_model.js`,
//! `synth_editor_constants.js` and the logic of `synth_editor.js`,
//! `groups_section.js` and `waveform_section.js`): a preset as JSON, filled
//! with the group defaults; its cards, their bypassed state and what turning
//! one on or off does; the groups a control quietly switches on; the LFO
//! modulation; and what the scope, the envelope and the filter curve plot.

use ordrumbox_core::js;
use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::lfo;

use crate::control::Spec;

/// The oscillator waves (`WAVE_TYPES`) and filter types (`FILTER_TYPES`).
pub const WAVES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];
pub const FILTER_TYPES: [&str; 3] = ["lowpass", "highpass", "bandpass"];
/// The mod envelope's targets and the LFO syncs (value, label).
pub const MOD_ENV_TARGETS: [(&str, &str); 5] =
    [("off", "Off"), ("filter", "Filter"), ("pitch", "Pitch"), ("fm", "FM"), ("shape", "Shape")];
pub const LFO_SYNCS: [(&str, &str); 8] = [
    ("off", "free"),
    ("1/1", "1/1"),
    ("1/2", "1/2"),
    ("1/4", "1/4"),
    ("1/8", "1/8"),
    ("1/16", "1/16"),
    ("1/8T", "1/8T"),
    ("1/16T", "1/16T"),
];

/// A group's defaults (`SYNTH_GROUP_DEFAULTS`), in order.
const DEFAULTS: &str = r#"{"masterVolume":0.8,"subGain":0,"pitchPunch":0,
    "vco1":{"gain":1,"octave":0,"detune":0,"wave":"sine"},
    "vco2":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "vco3":{"gain":0,"octave":0,"detune":0,"wave":"sine"},
    "filter":{"type":"lowpass","freq":400,"Q":1,"drive":0},
    "filterEnv":{"filterEnvelopeAmount":0},
    "fm":{"amount":0,"algo":0},
    "lfo":{"target":"NOT","wave":"sine","freq":0,"depth":0,"sync":"off"},
    "lfo2":{"target":"NOT","wave":"sine","freq":0,"depth":0,"sync":"off"},
    "noise":{"mix":0,"filterType":"highpass","filterFreq":1000,"filterQ":1},
    "envelope":{"attack":0,"decay":0.12,"sustain":1,"release":0.05},
    "modEnvelope":{"attack":0,"decay":0.12,"sustain":0,"release":0.1,"target":"off"}}"#;

pub fn defaults() -> Object {
    json::parse(DEFAULTS).ok().and_then(|v| v.as_object().cloned()).unwrap_or_default()
}

/// A numeric control (`SYNTH_PARAM_META`): its path, range, step, unit, label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Meta {
    pub path: &'static str,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub unit: &'static str,
    pub label: Option<&'static str>,
    pub log: bool,
}

const fn meta(path: &'static str, min: f64, max: f64, step: f64, unit: &'static str) -> Meta {
    Meta { path, min, max, step, unit, label: None, log: false }
}

/// Every numeric control, in the JS order (the LFO targets follow it).
pub const METAS: [Meta; 32] = [
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
    meta("filter.freq", 20.0, 20000.0, 1.0, "Hz"),
    meta("filter.Q", 0.1, 24.0, 0.1, ""),
    meta("filter.drive", 0.0, 1.0, 0.01, ""),
    Meta { label: Some("Env"), ..meta("filterEnv.filterEnvelopeAmount", 0.0, 1.0, 0.01, "") },
    Meta { log: true, ..meta("lfo.freq", 0.01, 5.0, 0.01, "Hz") },
    meta("lfo.depth", 0.0, 1.0, 0.01, ""),
    Meta { log: true, ..meta("lfo2.freq", 0.01, 5.0, 0.01, "Hz") },
    meta("lfo2.depth", 0.0, 1.0, 0.01, ""),
    meta("noise.mix", 0.0, 1.0, 0.01, ""),
    meta("noise.filterFreq", 20.0, 20000.0, 1.0, "Hz"),
    meta("noise.filterQ", 0.1, 24.0, 0.1, ""),
    Meta { label: Some("FM"), ..meta("fm.amount", 0.0, 1.0, 0.01, "") },
    Meta { label: Some("Algo"), ..meta("fm.algo", 0.0, 4.0, 1.0, "") },
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

pub fn metas() -> impl Iterator<Item = &'static Meta> {
    METAS.iter().chain(std::iter::once(&MOD_RELEASE))
}

pub fn find_meta(path: &str) -> Option<&'static Meta> {
    metas().find(|m| m.path == path)
}

impl Meta {
    /// The knob of this control.
    pub fn spec(&self) -> Spec {
        let spec = Spec::knob(self.min, self.max, self.step, self.min);
        if self.log { spec.log() } else { spec }
    }
}

/// What an LFO can modulate (`SYNTH_LFO_TARGETS`): off, then every control but the LFOs'.
pub fn lfo_targets() -> Vec<&'static str> {
    std::iter::once("NOT")
        .chain(metas().map(|m| m.path).filter(|p| !p.starts_with("lfo.") && !p.starts_with("lfo2.")))
        .collect()
}

/// What one unit of an LFO's wave is worth on a target (`LFO_TARGET_SCALE`).
pub fn lfo_scale(target: &str) -> f64 {
    match target {
        "vco1.detune" | "vco2.detune" | "vco3.detune" => 100.0,
        "filter.freq" => 1000.0,
        "filter.Q" | "noise.filterQ" => 24.0,
        "noise.filterFreq" => 10_000.0,
        "envelope.attack" | "envelope.release" | "modEnvelope.attack" | "modEnvelope.release" => 0.25,
        "envelope.decay" | "envelope.sustain" | "modEnvelope.decay" | "modEnvelope.sustain" => 0.5,
        "NOT" => 0.0,
        t if find_meta(t).is_some() && !t.starts_with("lfo") => 1.0,
        _ => 0.0,
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
pub const GROUP_ORDER: [&str; 13] = [
    "scope",
    "vco1",
    "vco2",
    "vco3",
    "fm",
    "filter",
    "filterEnv",
    "lfo",
    "lfo2",
    "noise",
    "master",
    "envelope",
    "modEnvelope",
];
pub const MASTER_KEYS: [&str; 3] = ["masterVolume", "subGain", "pitchPunch"];

/// The tabs and which cards each shows (`TAB_DEFS`, `GROUP_TAB`).
pub const TABS: [&str; 4] = ["OSC", "FLT", "MOD", "ENV"];

pub fn group_tab(group: &str) -> usize {
    match group {
        "filter" | "filterEnv" | "modEnvelope" => 1,
        "lfo" | "lfo2" | "noise" => 2,
        "envelope" | "master" => 3,
        _ => 0,
    }
}

/// A card's title (`SYNTH_GROUP_LABELS`).
pub fn group_label(group: &str) -> String {
    match group {
        "scope" => "Scope".into(),
        "master" => "Master".into(),
        "filter" => "Flt".into(),
        "filterEnv" => "FltEnv".into(),
        "fm" => "FM".into(),
        "lfo" => "LFO1".into(),
        "envelope" => "Env".into(),
        "modEnvelope" => "ModEnv".into(),
        g if g.len() > 3 && g[..3].eq_ignore_ascii_case("vco") && g[3..].chars().all(|c| c.is_ascii_digit()) => {
            g.to_uppercase()
        }
        g => g.to_owned(),
    }
}

fn is_object(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Object(_)))
}

/// `Boolean(v)`.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => *n != 0.0 && !n.is_nan(),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
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
        match default {
            Value::Object(children) => {
                if !is_object(draft.get(key)) {
                    draft.insert(key, default.clone());
                    continue;
                }
                if let Some(Value::Object(group)) = draft.get_mut(key) {
                    for (child, value) in children.iter() {
                        if group.get(child).is_none() {
                            group.insert(child, value.clone());
                        }
                    }
                }
            }
            _ => {
                if draft.get(key).is_none() {
                    draft.insert(key, default.clone());
                }
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
    BYPASS_FLAGS.iter().find(|(g, _)| *g == group).is_some_and(|(_, flag)| truthy(draft.get(flag)))
}

/// Whether a card has a bypass state (the oscillators and the flagged cards).
pub fn has_power(group: &str) -> bool {
    matches!(group, "vco1" | "vco2" | "vco3") || BYPASS_FLAGS.iter().any(|(g, _)| *g == group)
}

/// The cards of `draft`, in order: the known ones,
/// then any other group it has (not a bypass flag), by name.
pub fn groups(draft: &Object) -> Vec<String> {
    let mut groups: Vec<String> = GROUP_ORDER.iter().map(|g| (*g).to_owned()).collect();
    for (key, _) in draft.iter() {
        if !MASTER_KEYS.contains(&key) && !key.starts_with("bypass") && !groups.iter().any(|g| g == key) {
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
    if matches!(group, "vco2" | "vco3") && matches!(key, "octave" | "detune" | "wave") && gain(draft, group) == 0.0 {
        if let Some(Value::Object(vco)) = draft.get_mut(group) {
            vco.insert("gain", Value::from(0.5));
            vco.remove("_savedGain");
        }
        changed.push(format!("{group}.gain"));
    } else if group == "fm" && key == "algo" && number(draft, "fm.amount").unwrap_or(0.0) == 0.0 {
        set(draft, "fm.amount", Value::from(0.3));
        draft.insert("bypassFm", Value::Bool(false));
        changed.push("fm.amount".into());
    } else if group == "noise" && key.starts_with("filter") && number(draft, "noise.mix").unwrap_or(0.0) == 0.0 {
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

/// The LFOs playing (not bypassed): LFO1, LFO2.
fn lfos(draft: &Object) -> [Option<&Object>; 2] {
    let lfo = |group: &str, flag: &str| (!truthy(draft.get(flag))).then(|| draft.get(group)?.as_object()).flatten();
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
    let freq = ordrumbox_engine::synth_preset::sync_to_hz(lfo.get("sync"), bpm)
        .unwrap_or_else(|| lfo.get("freq").filter(|v| !v.is_null()).map_or(0.0, js::to_number));
    let depth = lfo.get("depth").filter(|v| !v.is_null()).map_or(0.0, js::to_number);
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

/// A wave's value at phase `p` (0–1) of cycle `cycle`.
fn wave_at(wave: &str, p: f64, cycle: f64) -> f64 {
    use std::f64::consts::PI;
    match wave {
        "square" => {
            if (2.0 * PI * p).sin() >= 0.0 {
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

/// Points of the scope: four cycles of the three
/// oscillators with their gains, octaves, detunes, FM and sub, the drive's
/// saturation, scaled to the master volume. `time`: when (the LFOs).
pub fn scope(draft: &Object, time: f64, bpm: f64) -> Vec<f64> {
    const SAMPLES: usize = 1024;
    const FM_DEPTH_SCALE: f64 = 0.08;
    let vco = |n: usize, key: &str, default: f64| number(draft, &format!("vco{n}.{key}")).unwrap_or(default);
    let waves: Vec<String> =
        (1..=3).map(|n| text(draft, &format!("vco{n}.wave")).unwrap_or("sine").to_owned()).collect();
    let modded = |path: &str| lfo_mod_on(draft, path, time, bpm);
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
    let fm_amount = number(draft, "fm.amount").unwrap_or(0.0);
    let fm_algo = number(draft, "fm.algo").unwrap_or(0.0);
    let sub_gain = number(draft, "subGain").unwrap_or(0.0);
    let drive = number(draft, "filter.drive").or_else(|| number(draft, "drive")).unwrap_or(0.0);
    let base = 4.0 / SAMPLES as f64;
    let inc: Vec<f64> = ratio.iter().map(|r| base * r).collect();
    let depth = fm_amount * FM_DEPTH_SCALE;
    let (mut phase, mut cycle) = ([0.0f64; 3], [0.0f64; 3]);
    let mut out = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        let o2 = wave_at(&waves[1], phase[1], cycle[1]);
        let o3 = wave_at(&waves[2], phase[2], cycle[2]);
        let (mut f1, mut f2, f3) = (inc[0], inc[1], inc[2]);
        if fm_amount > 0.001 {
            match fm_algo as i64 {
                0 => f1 += o2 * depth,
                1 => f1 += o3 * depth,
                2 => {
                    f1 += o2 * depth;
                    f2 += o3 * depth;
                }
                3 => f1 += (o2 + o3) * depth,
                4 => {
                    let o1 = wave_at(&waves[0], phase[0], cycle[0]);
                    f1 += o2 * depth;
                    f2 += o1 * depth;
                }
                _ => {}
            }
        }
        for (i, f) in [f1, f2, f3].into_iter().enumerate() {
            phase[i] += f;
            let whole = phase[i].floor();
            cycle[i] += whole;
            phase[i] -= whole;
        }
        let sub = if sub_gain > 0.0 { wave_at("sine", (phase[0] * 0.5) % 1.0, 0.0) * sub_gain } else { 0.0 };
        let mut sample = (0..3).map(|i| wave_at(&waves[i], phase[i], cycle[i]) * gain[i]).sum::<f64>() + sub;
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
/// 20 kHz (log), centred on `fc` (`#drawFilterResponse`'s `drawCurve`).
pub fn filter_response(kind: &str, fc: f64, q: f64, points: usize) -> Vec<f64> {
    (0..=points)
        .map(|i| {
            let f = 20.0 * 1000f64.powf(i as f64 / points as f64);
            let d = fc * fc - f * f;
            let denom = (d * d + (fc * f / q) * (fc * f / q)).sqrt();
            let mag = match kind {
                "highpass" => f * f / denom,
                "bandpass" => fc * f / q / denom,
                _ => fc * fc / denom,
            };
            (20.0 * mag.max(1e-10).log10()).clamp(-30.0, 12.0)
        })
        .collect()
}

/// The filter curve's centre and Q with the LFOs at `time`, and the band an
/// LFO on the frequency sweeps.
pub fn filter_now(draft: &Object, time: f64, bpm: f64) -> (String, f64, f64, Option<(f64, f64)>) {
    let kind = text(draft, "filter.type").unwrap_or("lowpass").to_owned();
    let fc = number(draft, "filter.freq").unwrap_or(400.0).clamp(20.0, 20_000.0);
    let q = number(draft, "filter.Q").unwrap_or(1.0).clamp(0.1, 24.0);
    let fc = (fc + lfo_mod_on(draft, "filter.freq", time, bpm)).clamp(20.0, 20_000.0);
    let q = (q + lfo_mod_on(draft, "filter.Q", time, bpm)).clamp(0.1, 24.0);
    let depth = lfos(draft)
        .into_iter()
        .flatten()
        .filter(|l| l.get("target").and_then(Value::as_str) == Some("filter.freq"))
        .map(|l| l.get("depth").and_then(Value::as_f64).unwrap_or(0.0))
        .fold(0.0f64, f64::max);
    let band = (depth > 0.0).then(|| ((fc - depth * 1000.0).max(20.0), (fc + depth * 1000.0).min(20_000.0)));
    (kind, fc, q, band)
}

/// The module pills under the cards: label, active.
pub fn modules(draft: &Object) -> Vec<(&'static str, bool)> {
    let n = |path: &str| number(draft, path).unwrap_or(0.0);
    let flag = |key: &str| truthy(draft.get(key));
    let gain = |v: usize| number(draft, &format!("vco{v}.gain")).unwrap_or(if v == 1 { 1.0 } else { 0.0 });
    let target = |path: &str, off: &str| text(draft, path).unwrap_or(off) != off;
    let env_amount = number(draft, "filterEnv.filterEnvelopeAmount")
        .or_else(|| number(draft, "filter.filterEnvelopeAmount"))
        .unwrap_or(0.0);
    vec![
        ("VCO1", gain(1) > 0.01),
        ("VCO2", gain(2) > 0.01),
        ("VCO3", gain(3) > 0.01),
        ("Flt", !flag("bypassFilter")),
        ("Env", !flag("bypassEnv")),
        ("LFO1", !flag("bypassLfo1") && target("lfo.target", "NOT") && n("lfo.depth") > 0.0),
        ("LFO2", !flag("bypassLfo2") && target("lfo2.target", "NOT") && n("lfo2.depth") > 0.0),
        ("FM", !flag("bypassFm") && n("fm.amount") > 0.001),
        ("Mod", !flag("bypassModEnv") && target("modEnvelope.target", "off")),
        ("Ns", !flag("bypassNoise") && n("noise.mix") > 0.001),
        ("FltEnv", !flag("bypassFilterEnv") && env_amount > 0.001),
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
    let idx = keys.iter().position(|k| *k == deleted)?;
    let next = if idx + 1 < keys.len() { idx } else { idx.checked_sub(1)? };
    if keys[next] == deleted { keys.get((idx + 1) % keys.len()).copied() } else { Some(keys[next]) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Value {
        json::parse(include_str!("../tests/fixtures/synth_editor.json")).unwrap()
    }

    /// The JS model on every preset (tools/export_synth_fixtures.mjs).
    #[test]
    fn golden_presets() {
        let fixture = fixture();
        let presets = fixture.get("presets").and_then(Value::as_array).unwrap();
        assert!(presets.len() > 15);
        for case in presets {
            let key = case.get("key").and_then(Value::as_str).unwrap();
            let mut draft = case.get("source").and_then(Value::as_object).cloned().unwrap();
            hydrate(&mut draft);
            assert_eq!(Value::Object(draft.clone()).to_json(), case.get("draft").unwrap().to_json(), "{key}: draft");
            for (group, on) in case.get("bypassed").and_then(Value::as_object).unwrap().iter() {
                assert_eq!(bypassed(&draft, group), on.as_bool().unwrap(), "{key}: {group} bypassed");
            }
            let order: Vec<String> = case
                .get("groups")
                .and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|g| g.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(groups(&draft), order, "{key}: groups");
        }
        let targets: Vec<&str> =
            fixture.get("lfoTargets").and_then(Value::as_array).unwrap().iter().filter_map(Value::as_str).collect();
        assert_eq!(lfo_targets(), targets);
        for (target, scale) in fixture.get("lfoScale").and_then(Value::as_object).unwrap().iter() {
            assert_eq!(lfo_scale(target), scale.as_f64().unwrap(), "{target}");
        }
    }

    #[test]
    fn golden_lfo_modulation() {
        let fixture = fixture();
        for case in fixture.get("lfoMods").and_then(Value::as_array).unwrap() {
            let lfo = case.get("lfo").and_then(Value::as_object).unwrap();
            let times = [0.0, 0.1, 0.37, 1.5, 7.77];
            for (t, expected) in times.iter().zip(case.get("at").and_then(Value::as_array).unwrap()) {
                let got = lfo_mod(lfo, *t, 120.0);
                let expected = expected.as_f64().unwrap();
                assert!((got - expected).abs() < 1e-9, "{} at {t}: {got} vs {expected}", case.to_json());
            }
        }
    }

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
        let low = filter_response("lowpass", 1000.0, 1.0, 200);
        assert!(low[0] > -1.0 && low[200] == -30.0);
        let high = filter_response("highpass", 1000.0, 1.0, 200);
        assert!(high[0] == -30.0 && high[200] > -1.0);
        assert_eq!(
            modules(&defaults()).iter().filter(|(_, on)| *on).map(|(l, _)| *l).collect::<Vec<_>>(),
            ["VCO1", "Flt", "Env"]
        );
    }
}
