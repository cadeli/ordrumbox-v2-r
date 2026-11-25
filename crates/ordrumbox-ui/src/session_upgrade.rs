//! The reminder after an upgrade: the first start of a version other than the one that last ran
//! says that Help > Factory reset... restores the demos as shipped (the new ones are added by `session_new_demos.rs`). The version that ran is the `version`
//! setting of `settings.json`.

use ordrumbox_core::json::Value;

use super::Session;
use crate::{store, update};

/// The setting holding the version that ran last.
const VERSION_KEY: &str = "version";

/// What to tell at the start of `current`, when `saved` ran before it. Nothing for a first start (no
/// version saved) or the same version.
#[must_use]
fn upgrade_line(saved: Option<&str>, current: &str) -> Option<String> {
    let saved = saved.filter(|saved| *saved != current)?;
    Some(format!(
        "orDrumbox {current} (was {saved}): Help > Factory reset... restores the demos as shipped, your files are backed up"
    ))
}

impl Session {
    /// At start of the program: tell of an upgrade, and keep the version that runs.
    pub fn note_upgrade(&mut self) {
        let Some(path) = store::settings_path() else { return };
        let saved = match store::load_setting(&path, VERSION_KEY) {
            Ok(value) => value,
            // `load_master` has reported the file already.
            Err(_) => return,
        };
        let saved = saved.as_ref().and_then(Value::as_str);
        if let Some(line) = upgrade_line(saved, update::CURRENT) {
            self.status.push(line.clone());
            self.message = line;
        }
        if saved != Some(update::CURRENT)
            && let Err(e) = store::save_setting(&path, VERSION_KEY, Value::from(update::CURRENT))
        {
            self.status.push(format!("settings not saved: {}: {e}", path.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::upgrade_line;

    #[test]
    fn a_first_start_or_the_same_version_says_nothing() {
        assert_eq!(upgrade_line(None, "0.2.0"), None);
        assert_eq!(upgrade_line(Some("0.2.0"), "0.2.0"), None);
    }

    #[test]
    fn another_version_points_to_the_factory_reset() {
        let line = upgrade_line(Some("0.1.2"), "0.2.0").expect("an upgrade");
        assert!(line.contains("0.2.0") && line.contains("0.1.2") && line.contains("Factory reset"), "{line}");
    }
}
