//! What older presets hold of the modulation, read as today's: the targets
//! they name the old way, each LFO's own target and depth, and the
//! modulation envelope's target, now lines of the matrix; and the pitch
//! punch, now the pitch envelope.
//!
//! `FLT` is `filter.freq`, and `VCO1` to `VCO3` (a pitch swing of 1000 cents)
//! are the oscillator's octave (1200 cents) at the same swing.

use std::borrow::Cow;

use ordrumbox_core::json::{Object, Value};

use super::{MATRIX_SIZE, MATRIX_SOURCES};

/// What `VCO1` to `VCO3` keep of a depth or an amount on `vcoN.octave`: the
/// same swing in cents.
const PITCH_SHARE: f64 = 1000.0 / 1200.0;

/// Each older name, the target it is read as, and the share of a depth or an
/// amount that keeps its swing there.
const LEGACY_TARGETS: [(&str, &str, f64); 5] = [
    ("FLT", "filter.freq", 1.0),
    // A full punch and a full swing of the envelope's offset: 24 semitones each.
    (PUNCH, "pitchEnv.amount", 1.0),
    ("VCO1", "vco1.octave", PITCH_SHARE),
    ("VCO2", "vco2.octave", PITCH_SHARE),
    ("VCO3", "vco3.octave", PITCH_SHARE),
];

/// The pitch punch: a rise of up to 24 semitones at the start of a note,
/// falling back to the note in 60 ms.
const PUNCH: &str = "pitchPunch";
/// The semitones of a full punch.
const PUNCH_SEMITONES: f64 = 24.0;
/// The decay of the pitch envelope a punch becomes: as many cents over time
/// as the punch's straight fall (an exponential one, to a hundredth).
const PUNCH_DECAY_SECONDS: f64 = 0.14;

/// Each LFO group of a preset and the matrix source it is.
const LFO_SOURCES: [(&str, &str); 2] = [("lfo", "lfo1"), ("lfo2", "lfo2")];

/// The keys an LFO group held before the matrix took its routing over.
const LFO_ROUTE_KEYS: [&str; 2] = ["target", "depth"];

/// Each target the modulation envelope had (at full depth), and the matrix target it is.
const MOD_ENV_TARGETS: [(&str, &str); 4] =
    [("filter", "filter.open"), ("pitch", "pitch"), ("fm", "fm.boost"), ("shape", "shape")];

/// The target an older `name` is read as, and the share of its depth or amount.
fn renamed(name: &str) -> Option<(&'static str, f64)> {
    LEGACY_TARGETS.iter().find(|(old, ..)| *old == name).map(|&(_, new, share)| (new, share))
}

/// Whether `preset` holds anything older: an LFO's own target or depth, or
/// an older target name in the matrix.
fn is_older(preset: &Object) -> bool {
    let group = |key: &str| preset.get(key).and_then(Value::as_object);
    let has_route = |key: &str| group(key).is_some_and(|lfo| LFO_ROUTE_KEYS.iter().any(|k| lfo.get(k).is_some()));
    let has_older_name = group("matrix").is_some_and(|matrix| {
        (1..=MATRIX_SIZE).any(|n| matrix.get(&format!("target{n}")).and_then(Value::as_str).and_then(renamed).is_some())
    });
    let has_mod_env_target = group("modEnvelope").is_some_and(|env| env.get("target").is_some());
    LFO_SOURCES.iter().any(|(key, _)| has_route(key))
        || has_older_name
        || has_mod_env_target
        || preset.get(PUNCH).is_some()
}

/// `preset` as today's presets are, copied only when it is older.
#[must_use]
pub fn upgraded(preset: &Value) -> Cow<'_, Value> {
    match preset.as_object() {
        Some(object) if is_older(object) => {
            let mut object = object.clone();
            upgrade(&mut object);
            Cow::Owned(Value::Object(object))
        }
        _ => Cow::Borrowed(preset),
    }
}

/// Bring `preset` up to date, with the same sound: older target names
/// renamed (their depth or amount scaled), and each LFO's own target and
/// depth, and the modulation envelope's target, moved to the first free
/// lines of the matrix. A target that finds no free line stays, unplayed
/// ([`stranded`]). The pitch punch becomes the pitch envelope, close to its
/// sound, when that one moves nothing; otherwise it goes.
pub fn upgrade(preset: &mut Object) {
    upgrade_punch(preset);
    if let Some(Value::Object(matrix)) = preset.get_mut("matrix") {
        for n in 1..=MATRIX_SIZE {
            rename_target(matrix, &format!("target{n}"), &format!("amount{n}"));
        }
    }
    for (key, source) in LFO_SOURCES {
        let Some(Value::Object(lfo)) = preset.get_mut(key) else {
            continue;
        };
        rename_target(lfo, "target", "depth");
        let target = lfo.get("target").and_then(Value::as_str).filter(|t| *t != "NOT").map(str::to_owned);
        let depth = lfo.get("depth").and_then(Value::to_number).unwrap_or(0.0);
        let Some(target) = target.filter(|_| depth != 0.0) else {
            // Aimed at nothing or without depth: it played nothing.
            remove_route(lfo);
            continue;
        };
        if add_line(preset, source, &target, depth)
            && let Some(Value::Object(lfo)) = preset.get_mut(key)
        {
            remove_route(lfo);
        }
    }
    upgrade_mod_env(preset);
}

/// The pitch punch of `preset`, taken out, as its pitch envelope when that
/// one has no offset.
fn upgrade_punch(preset: &mut Object) {
    let Some(punch) = preset.remove(PUNCH).and_then(|v| v.to_number()).filter(|p| p.is_finite() && *p > 0.0) else {
        return;
    };
    // To a hundredth of a semitone: 0.15 of 24 is 3.6, not 3.5999999999999996.
    let semitones = (punch.min(1.0) * PUNCH_SEMITONES * 100.0).round() / 100.0;
    match preset.get_mut("pitchEnv") {
        Some(Value::Object(env)) => {
            if env.get("amount").and_then(Value::to_number).is_none_or(|amount| amount == 0.0) {
                env.insert("amount", Value::from(semitones));
                env.insert("decay", Value::from(PUNCH_DECAY_SECONDS));
            }
        }
        _ => {
            let mut env = Object::default();
            env.insert("amount", Value::from(semitones));
            env.insert("decay", Value::from(PUNCH_DECAY_SECONDS));
            preset.insert("pitchEnv", Value::Object(env));
        }
    }
}

/// Move the modulation envelope's target of `preset` to a line `modEnv` at 1.
fn upgrade_mod_env(preset: &mut Object) {
    let Some(Value::Object(env)) = preset.get_mut("modEnvelope") else {
        return;
    };
    let Some(name) = env.get("target").and_then(Value::as_str) else {
        return;
    };
    let Some(&(_, target)) = MOD_ENV_TARGETS.iter().find(|(old, _)| *old == name) else {
        // Off, or a name no patch played.
        env.remove("target");
        return;
    };
    if add_line(preset, "modEnv", target, 1.0)
        && let Some(Value::Object(env)) = preset.get_mut("modEnvelope")
    {
        env.remove("target");
    }
}

/// The groups of an upgraded `preset` (an LFO, the modulation envelope)
/// that still hold a target: the matrix had no free line for it, and it is
/// not played.
#[must_use]
pub fn stranded(preset: &Object) -> Vec<&'static str> {
    ["lfo", "lfo2", "modEnvelope"]
        .into_iter()
        .filter(|key| preset.get(key).and_then(Value::as_object).and_then(|group| group.get("target")).is_some())
        .collect()
}

/// Take an LFO's own target and depth out of `lfo`.
fn remove_route(lfo: &mut Object) {
    for key in LFO_ROUTE_KEYS {
        lfo.remove(key);
    }
}

/// Rename an older target at `target_key` of `group`, and scale the depth or amount at `amount_key` with it.
fn rename_target(group: &mut Object, target_key: &str, amount_key: &str) {
    let Some((name, share)) = group.get(target_key).and_then(Value::as_str).and_then(renamed) else {
        return;
    };
    group.insert(target_key, Value::from(name));
    if let Some(amount) = group.get(amount_key).and_then(Value::to_number) {
        group.insert(amount_key, Value::from(amount * share));
    }
}

/// Write `source` → `target` at `amount` on the first line of the matrix of
/// `preset` that reads nothing; whether there was one.
fn add_line(preset: &mut Object, source: &str, target: &str, amount: f64) -> bool {
    if preset.get("matrix").and_then(Value::as_object).is_none() {
        preset.insert("matrix", Value::Object(Object::default()));
    }
    let Some(Value::Object(matrix)) = preset.get_mut("matrix") else {
        return false;
    };
    let reads_something = |matrix: &Object, n: usize| {
        let source = matrix.get(&format!("source{n}")).and_then(Value::as_str);
        source.is_some_and(|s| s != "off" && MATRIX_SOURCES.contains(&s))
    };
    let Some(n) = (1..=MATRIX_SIZE).find(|&n| !reads_something(matrix, n)) else {
        return false;
    };
    matrix.insert(format!("source{n}"), Value::from(source));
    matrix.insert(format!("target{n}"), Value::from(target));
    matrix.insert(format!("amount{n}"), Value::from(amount));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    fn object(text: &str) -> Object {
        json::parse(text).unwrap().as_object().cloned().unwrap()
    }

    fn at(preset: &Object, group: &str, key: &str) -> Option<Value> {
        preset.get(group).and_then(|g| g.as_object()?.get(key).cloned())
    }

    #[test]
    fn the_punch_becomes_the_pitch_envelope_when_that_one_moves_nothing() {
        let mut punched = object(r#"{"pitchPunch":0.5,"pitchEnv":{"amount":0,"decay":0.05}}"#);
        upgrade(&mut punched);
        assert_eq!(punched.get(PUNCH), None);
        assert_eq!(at(&punched, "pitchEnv", "amount"), Some(Value::from(12.0)));
        assert_eq!(at(&punched, "pitchEnv", "decay"), Some(Value::from(PUNCH_DECAY_SECONDS)));
        let mut bare = object(r#"{"pitchPunch":1}"#);
        upgrade(&mut bare);
        assert_eq!(at(&bare, "pitchEnv", "amount"), Some(Value::from(24.0)));
        // An envelope that moves keeps its offset; the punch goes.
        let mut both = object(r#"{"pitchPunch":0.5,"pitchEnv":{"amount":-7,"decay":0.2}}"#);
        assert!(is_older(&both));
        upgrade(&mut both);
        assert_eq!((at(&both, "pitchEnv", "amount"), both.get(PUNCH)), (Some(Value::from(-7.0)), None));
        let mut routed = object(r#"{"matrix":{"source1":"lfo1","target1":"pitchPunch","amount1":0.3}}"#);
        upgrade(&mut routed);
        assert_eq!(at(&routed, "matrix", "target1"), Some(Value::from("pitchEnv.amount")));
    }

    #[test]
    fn an_lfo_target_becomes_a_line_of_the_matrix() {
        let mut preset = object(
            r#"{"lfo":{"target":"VCO2","wave":"square","depth":0.6},"lfo2":{"target":"FLT","depth":0.5},
                "matrix":{"source1":"velocity","target1":"VCO1","amount1":-0.3,"source2":"off","target2":"NOT"}}"#,
        );
        upgrade(&mut preset);
        assert_eq!(at(&preset, "matrix", "target1"), Some(Value::from("vco1.octave")));
        assert_eq!(at(&preset, "matrix", "amount1"), Some(Value::from(-0.3 * PITCH_SHARE)));
        // LFO 1 takes the first free line, LFO 2 the next.
        assert_eq!(at(&preset, "matrix", "source2"), Some(Value::from("lfo1")));
        assert_eq!(at(&preset, "matrix", "target2"), Some(Value::from("vco2.octave")));
        assert_eq!(at(&preset, "matrix", "amount2"), Some(Value::from(0.6 * PITCH_SHARE)));
        assert_eq!(at(&preset, "matrix", "source3"), Some(Value::from("lfo2")));
        assert_eq!(at(&preset, "matrix", "target3"), Some(Value::from("filter.freq")));
        assert_eq!(at(&preset, "matrix", "amount3"), Some(Value::from(0.5)));
        // The LFO keeps its wave, and loses its routing.
        assert_eq!(at(&preset, "lfo", "wave"), Some(Value::from("square")));
        assert_eq!(at(&preset, "lfo", "target"), None);
        assert_eq!(at(&preset, "lfo2", "depth"), None);
        assert!(!is_older(&preset));
        assert!(stranded(&preset).is_empty());
    }

    #[test]
    fn an_lfo_aimed_at_nothing_adds_no_line() {
        let mut preset = object(r#"{"lfo":{"target":"NOT","depth":0.4},"lfo2":{"target":"filter.Q","depth":0}}"#);
        upgrade(&mut preset);
        assert!(preset.get("matrix").is_none());
        assert_eq!(at(&preset, "lfo", "target"), None);
        assert_eq!(at(&preset, "lfo2", "target"), None);
    }

    #[test]
    fn an_lfo_target_without_a_free_line_stays_and_is_named() {
        let mut matrix = Object::default();
        for n in 1..=MATRIX_SIZE {
            matrix.insert(format!("source{n}"), Value::from("key"));
        }
        let mut preset = object(r#"{"lfo":{"target":"filter.Q","depth":0.4}}"#);
        preset.insert("matrix", Value::Object(matrix));
        upgrade(&mut preset);
        assert_eq!(stranded(&preset), ["lfo"]);
    }

    #[test]
    fn the_modulation_envelope_target_becomes_a_line_at_full() {
        let mut preset = object(r#"{"modEnvelope":{"decay":0.2,"target":"fm"},"matrix":{"source1":"key"}}"#);
        upgrade(&mut preset);
        assert_eq!(at(&preset, "matrix", "source2"), Some(Value::from("modEnv")));
        assert_eq!(at(&preset, "matrix", "target2"), Some(Value::from("fm.boost")));
        assert_eq!(at(&preset, "matrix", "amount2"), Some(Value::from(1.0)));
        assert_eq!(at(&preset, "modEnvelope", "target"), None);
        let mut off = object(r#"{"modEnvelope":{"target":"off"}}"#);
        upgrade(&mut off);
        assert_eq!(at(&off, "modEnvelope", "target"), None);
        assert!(off.get("matrix").is_none());
    }

    #[test]
    fn a_current_preset_is_not_copied() {
        let preset = json::parse(r#"{"lfo":{"wave":"sine","freq":2},"matrix":{"target1":"filter.Q"}}"#).unwrap();
        assert!(matches!(upgraded(&preset), Cow::Borrowed(_)));
    }
}
