//! The colour scheme (`theme.rs`): C cycles through them, and the choice
//! is kept in `settings.json` (`colorScheme`, from 1, as in the web version,
//! which knows the first three).

use super::*;

use crate::theme;
use ordrumbox_core::json;

/// The key of the colour scheme in `settings.json`.
const KEY: &str = "colorScheme";

impl Session {
    /// The colour scheme of the settings file at `path`, if it names one;
    /// the scheme is saved there when changed.
    pub(super) fn load_color_scheme(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else { return };
        // `load_master` has reported a file that cannot be read.
        if let Ok(Some(value)) = store::load_setting(&path, KEY) {
            let number = value.as_f64().filter(|n| n.fract() == 0.0 && *n >= 1.0).map(|n| n as usize);
            if !number.is_some_and(theme::set_scheme) {
                self.status.push(format!(
                    "settings.json: {KEY}: {} is not a colour scheme (1 to {}), {} used",
                    value.to_json(),
                    theme::SCHEMES.len(),
                    theme::scheme().name
                ));
            }
        }
        self.color_scheme = theme::scheme_number();
        self.color_scheme_file = Some(path);
    }

    /// C: use the next colour scheme, round again, and save it.
    pub(super) fn next_color_scheme(&mut self) {
        let number = theme::next_scheme(theme::scheme_number());
        theme::set_scheme(number);
        self.color_scheme = number;
        self.message = format!("colour scheme {number}: {}", theme::scheme().name);
        if let Some(path) = &self.color_scheme_file
            && let Err(e) = store::save_setting(path, KEY, json::Value::from(number as u32))
        {
            self.message = format!("colour scheme not saved: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ordrumbox-color-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn c_cycles_the_schemes_and_saves_the_choice() {
        let dir = folder("cycle");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"colorScheme":2,"master":{"volume":0.5}}"#).unwrap();
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.load_color_scheme(Some(path.clone()));
        assert_eq!(theme::scheme_number(), 2);
        s.run(Command::NextColorScheme);
        assert_eq!(s.message, "colour scheme 3: Deep sea");
        assert_eq!(store::load_setting(&path, KEY), Ok(Some(json::Value::from(3))));
        s.run(Command::NextColorScheme);
        assert_eq!(s.message, "colour scheme 4: Crimson");
        s.run(Command::NextColorScheme);
        assert_eq!(s.message, "colour scheme 5: Toxic");
        s.run(Command::NextColorScheme);
        assert_eq!(theme::scheme_number(), 1);
        assert_eq!(store::load_setting(&path, KEY), Ok(Some(json::Value::from(1))));
        assert!(store::load_setting(&path, "master").unwrap().is_some(), "the other settings are kept");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn an_unknown_scheme_is_reported_and_the_first_used() {
        let dir = folder("unknown");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"colorScheme":7}"#).unwrap();
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.load_color_scheme(Some(path));
        assert_eq!(theme::scheme_number(), 1);
        assert_eq!(s.status, ["settings.json: colorScheme: 7 is not a colour scheme (1 to 5), Indigo used"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
