//! The master bus settings as `settings.json` holds them (`settings.master`):
//! one key per setting, read over the defaults and written whole.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::MasterSettings;

/// The numeric settings' keys (`compBypass`, a boolean, is apart).
pub const KEYS: [&str; 10] =
    ["volume", "preGain", "threshold", "ratio", "attack", "release", "knee", "makeup", "lowcut", "hicut"];

/// The field holding setting `key`; `key` is one of [`KEYS`] or `compBypass`.
fn field<'a>(master: &'a mut MasterSettings, key: &str) -> &'a mut f32 {
    match key {
        "volume" => &mut master.master,
        "preGain" => &mut master.pre_gain,
        "threshold" => &mut master.threshold,
        "ratio" => &mut master.ratio,
        "attack" => &mut master.attack,
        "release" => &mut master.release,
        "knee" => &mut master.knee,
        "makeup" => &mut master.makeup,
        "lowcut" => &mut master.lowcut,
        "hicut" => &mut master.hicut,
        "compBypass" => &mut master.bypass,
        _ => panic!("no master setting {key:?}"),
    }
}

/// Setting `key` of `master`.
pub fn get(master: &MasterSettings, key: &str) -> f64 {
    let mut copy = *master;
    f64::from(*field(&mut copy, key))
}

/// `master` with setting `key` at `value`.
pub fn set(mut master: MasterSettings, key: &str, value: f64) -> MasterSettings {
    *field(&mut master, key) = value as f32;
    master
}

/// `settings.master` read over the defaults, with what was not taken as it
/// is: a value of the wrong type (default kept), out of range (clamped), an
/// unknown key (ignored).
pub fn from_json(value: &Value) -> (MasterSettings, Vec<String>) {
    let mut master = MasterSettings::default();
    let mut issues = Vec::new();
    let Some(object) = value.as_object() else {
        return (master, vec![format!("master: not an object ({value}), defaults used")]);
    };
    for (key, v) in object.iter() {
        if key == "compBypass" {
            match v.as_bool() {
                Some(bypass) => master.bypass = if bypass { 1.0 } else { 0.0 },
                None => issues.push(format!("master.compBypass: not a boolean ({v}), default kept")),
            }
        } else if KEYS.contains(&key) {
            match v.as_f64() {
                Some(number) => master = set(master, key, number),
                None => issues.push(format!("master.{key}: not a number ({v}), default kept")),
            }
        } else {
            issues.push(format!("master.{key}: unknown setting, ignored"));
        }
    }
    let clamped = master.clamped();
    for key in KEYS {
        let (asked, used) = (get(&master, key), get(&clamped, key));
        if asked != used {
            issues.push(format!("master.{key}: {asked} out of range, {used} used"));
        }
    }
    (clamped, issues)
}

/// `settings.master`.
pub fn to_json(master: &MasterSettings) -> Value {
    let mut object = Object::new();
    for key in KEYS {
        object.insert(key, Value::from(get(master, key)));
    }
    object.insert("compBypass", Value::Bool(master.bypass >= 0.5));
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn settings_round_trip_over_the_defaults() {
        let (master, issues) =
            from_json(&json::parse(r#"{"volume":1.5,"compBypass":true,"hicut":99999,"ratio":"x","gain":1}"#).unwrap());
        assert_eq!((master.master, master.bypass, master.hicut, master.ratio), (1.5, 1.0, 22_000.0, 8.0));
        assert_eq!(
            issues,
            [
                "master.ratio: not a number (\"x\"), default kept",
                "master.gain: unknown setting, ignored",
                "master.hicut: 99999 out of range, 22000 used",
            ]
        );
        assert_eq!(from_json(&to_json(&master)), (master, Vec::new()));
        assert_eq!(to_json(&MasterSettings::default()).get("compBypass"), Some(&Value::Bool(false)));
    }
}
