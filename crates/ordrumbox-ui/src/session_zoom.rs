//! The zoom of the UI: Ctrl++ and Ctrl+- show it one whole scale bigger or
//! smaller than the display's, Ctrl+0 at the display's again. The window
//! must hold the layout at that scale (`view::fit_scale`); the choice is kept
//! in `settings.json` (`uiZoom`, steps from the display's scale).

use super::*;

use ordrumbox_core::json;

/// The key of the zoom in `settings.json`.
const KEY: &str = "uiZoom";

/// The largest scale the UI is drawn at.
pub const MAX_SCALE: i32 = 4;

impl Session {
    /// The zoom of the settings file at `path`, if it has one; the zoom is
    /// saved there when changed.
    pub(super) fn load_zoom(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else { return };
        // `load_master` has reported a file that cannot be read.
        if let Ok(Some(value)) = store::load_setting(&path, KEY) {
            let steps = value.as_f64().filter(|n| n.fract() == 0.0 && n.abs() < f64::from(MAX_SCALE));
            match steps {
                Some(steps) => self.chrome.zoom = steps as i32,
                None => self.status.push(format!(
                    "settings.json: {KEY}: {} is not a zoom (-{top} to {top}), none used",
                    value.to_json(),
                    top = MAX_SCALE - 1
                )),
            }
        }
        self.saving.settings_file = Some(path);
    }

    /// The display's scale factor (≥ 1), which the zoom starts from.
    pub fn set_display_scale(&mut self, scale: i32) {
        self.chrome.display_scale = scale.max(1);
    }

    /// The scale asked for: the display's with the zoom, from 1 to [`MAX_SCALE`].
    fn wanted_scale(&self) -> i32 {
        (self.chrome.display_scale + self.chrome.zoom).clamp(1, MAX_SCALE)
    }

    /// The scale the UI shows at in a window of `width` × `height` physical
    /// pixels: the one asked for, else the largest smaller one that fits.
    pub fn fit_scale(&self, width: i32, height: i32) -> i32 {
        view::fit_scale(width, height, self.wanted_scale(), !self.hosted)
    }

    /// Ctrl++ (`step` 1), Ctrl+- (-1) and Ctrl+0 (0, the display's scale):
    /// zoom, say at what scale the window shows, and save the zoom.
    pub(super) fn zoom(&mut self, step: i32) {
        let display = self.chrome.display_scale;
        let before = self.wanted_scale();
        let wanted = if step == 0 { display } else { (before + step).clamp(1, MAX_SCALE) };
        self.chrome.zoom = wanted - display;
        let (width, height, _) = self.chrome.panel_size;
        let shown = self.fit_scale(width, height);
        self.message = if step != 0 && wanted == before {
            format!("zoom x{wanted}, the limit")
        } else if shown < wanted {
            format!("zoom x{wanted}: the window is too small for it, x{shown} shown")
        } else {
            format!("zoom x{wanted}")
        };
        if let Some(path) = &self.saving.settings_file
            && let Err(e) = store::save_setting(path, KEY, json::Value::from(self.chrome.zoom))
        {
            self.message = format!("zoom not saved: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(display: i32, window: (i32, i32)) -> Session {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_display_scale(display);
        s.set_size((window.0, window.1, 1));
        s
    }

    #[test]
    fn ctrl_plus_and_minus_zoom_by_whole_scales_within_the_window() {
        let (w, h) = view::min_window_size(1, true);
        let mut s = session(1, (3 * w, 3 * h));
        assert_eq!(s.fit_scale(3 * w, 3 * h), 1);
        s.run(Command::ZoomIn);
        assert_eq!((s.message.as_str(), s.fit_scale(3 * w, 3 * h)), ("zoom x2", 2));
        s.run(Command::ZoomIn);
        assert_eq!(s.fit_scale(3 * w, 3 * h), 3);
        s.run(Command::ZoomIn);
        assert_eq!(s.message, "zoom x4: the window is too small for it, x3 shown");
        assert_eq!(s.fit_scale(3 * w, 3 * h), 3);
        s.run(Command::ZoomIn);
        assert_eq!(s.message, "zoom x4, the limit");
        s.run(Command::ZoomReset);
        assert_eq!((s.message.as_str(), s.fit_scale(3 * w, 3 * h)), ("zoom x1", 1));
        s.run(Command::ZoomOut);
        assert_eq!(s.message, "zoom x1, the limit");
    }

    #[test]
    fn a_hidpi_display_zooms_out_to_scale_1() {
        let (w, h) = view::min_window_size(2, true);
        let mut s = session(2, (w, h));
        assert_eq!(s.fit_scale(w, h), 2);
        s.run(Command::ZoomOut);
        assert_eq!((s.message.as_str(), s.fit_scale(w, h)), ("zoom x1", 1));
    }

    #[test]
    fn the_zoom_is_saved_and_read_back() {
        let dir = std::env::temp_dir().join(format!("ordrumbox-zoom-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"colorScheme":2}"#).unwrap();
        let mut s = session(1, (0, 0));
        s.load_zoom(Some(path.clone()));
        s.run(Command::ZoomIn);
        assert_eq!(store::load_setting(&path, KEY), Ok(Some(json::Value::from(1))));
        assert!(store::load_setting(&path, "colorScheme").unwrap().is_some(), "the other settings are kept");
        let mut again = session(2, (0, 0));
        again.load_zoom(Some(path.clone()));
        assert_eq!(again.wanted_scale(), 3, "steps from the display's scale");
        std::fs::write(&path, r#"{"uiZoom":9}"#).unwrap();
        let mut wrong = session(1, (0, 0));
        wrong.load_zoom(Some(path));
        assert_eq!(wrong.status, ["settings.json: uiZoom: 9 is not a zoom (-3 to 3), none used"]);
        assert_eq!(wrong.wanted_scale(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
