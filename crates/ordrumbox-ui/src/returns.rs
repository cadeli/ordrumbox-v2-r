//! The shared returns' settings as `settings.json` holds them
//! (`settings.returns`): the echo and the spring reverb every track sends to.

use ordrumbox_engine::ReturnSettings;

use crate::settings_section::Section;

/// The settings' keys.
pub const KEYS: [&str; 10] = [
    "echoBeats",
    "echoFeedback",
    "echoTone",
    "echoWow",
    "echoReverb",
    "echoLevel",
    "reverbRoom",
    "reverbDamping",
    "reverbSpring",
    "reverbLevel",
];

/// `settings.returns`.
pub const SECTION: Section<ReturnSettings> =
    Section { name: "returns", numbers: &KEYS, switches: &[], field, clamped: ReturnSettings::clamped };

/// The field holding setting `key`, one of [`KEYS`].
fn field<'a>(returns: &'a mut ReturnSettings, key: &str) -> &'a mut f32 {
    match key {
        "echoBeats" => &mut returns.echo_beats,
        "echoFeedback" => &mut returns.echo_feedback,
        "echoTone" => &mut returns.echo_tone,
        "echoWow" => &mut returns.echo_wow,
        "echoReverb" => &mut returns.echo_reverb,
        "echoLevel" => &mut returns.echo_level,
        "reverbRoom" => &mut returns.reverb_room,
        "reverbDamping" => &mut returns.reverb_damping,
        "reverbSpring" => &mut returns.reverb_spring,
        "reverbLevel" => &mut returns.reverb_level,
        _ => panic!("no returns setting {key:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn every_key_has_its_own_field_and_the_settings_round_trip() {
        let mut settings = ReturnSettings::default();
        for (i, key) in KEYS.iter().enumerate() {
            settings = SECTION.set(settings, key, (ReturnSettings::MIN.echo_tone + 0.01 * i as f32).into());
        }
        let values: Vec<f64> = KEYS.iter().map(|key| SECTION.get(&settings, key)).collect();
        assert!(values.windows(2).all(|w| w[0] != w[1]), "{values:?}");
        let (read, issues) = SECTION.read_json(&json::parse(r#"{"echoFeedback":2,"echoBeats":1.5}"#).unwrap());
        assert_eq!((read.echo_feedback, read.echo_beats), (0.99, 1.5));
        assert_eq!(issues, ["returns.echoFeedback: 2 out of range, 0.99 used"]);
        assert_eq!(SECTION.read_json(&SECTION.to_json(&read)), (read, Vec::new()));
    }
}
