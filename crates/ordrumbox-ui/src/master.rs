//! The master bus settings as `settings.json` holds them (`settings.master`).

use ordrumbox_engine::MasterSettings;

use crate::settings_section::Section;

/// The numeric settings' keys (`compBypass`, a boolean, is apart).
pub const KEYS: [&str; 10] =
    ["volume", "preGain", "threshold", "ratio", "attack", "release", "knee", "makeup", "lowcut", "hicut"];

/// `settings.master`.
pub const SECTION: Section<MasterSettings> =
    Section { name: "master", numbers: &KEYS, switches: &["compBypass"], field, clamped: MasterSettings::clamped };

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

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json::{self, Value};

    #[test]
    fn settings_round_trip_over_the_defaults() {
        let (master, issues) = SECTION
            .read_json(&json::parse(r#"{"volume":1.5,"compBypass":true,"hicut":99999,"ratio":"x","gain":1}"#).unwrap());
        assert_eq!((master.master, master.bypass, master.hicut, master.ratio), (1.5, 1.0, 22_000.0, 8.0));
        assert_eq!(
            issues,
            [
                "master.ratio: not a number (\"x\"), default kept",
                "master.gain: unknown setting, ignored",
                "master.hicut: 99999 out of range, 22000 used",
            ]
        );
        assert_eq!(SECTION.read_json(&SECTION.to_json(&master)), (master, Vec::new()));
        assert_eq!(SECTION.to_json(&MasterSettings::default()).get("compBypass"), Some(&Value::Bool(false)));
    }
}
