//! The zoom of the UI: Ctrl++ and Ctrl+- show it a quarter of a scale unit
//! bigger or smaller (×1, ×1.25, ×1.5… up to [`Scale::MAX`]), Ctrl+0 at the
//! display's scale again, View > Zoom: fit to window at the biggest scale
//! the window shows whole. The zoom does not depend on the window's size: a
//! window too small for the layout scrolls (`scroll.rs`). The choice is kept
//! in `settings.json` (`uiZoom`, the scale units added to the display's:
//! `0.5`, `-1`).

use super::*;

use crate::scale::Scale;
use ordrumbox_core::json;

/// The key of the zoom in `settings.json`.
const KEY: &str = "uiZoom";

/// Quarters in a scale unit, as `uiZoom` counts them.
const QUARTERS: f64 = 4.0;

impl Session {
    /// The zoom of the settings file at `path`, if it has one; the zoom is
    /// saved there when changed.
    pub(super) fn load_zoom(&mut self, path: Option<PathBuf>) {
        let Some(path) = path else { return };
        // `load_master` has reported a file that cannot be read.
        if let Ok(Some(value)) = store::load_setting(&path, KEY) {
            let top = Scale::MAX.quarters() - Scale::ONE.quarters();
            let quarters =
                value.as_f64().map(|units| units * QUARTERS).filter(|q| q.fract() == 0.0 && q.abs() <= f64::from(top));
            match quarters {
                // Whole and within ±`top`: the cast is exact.
                Some(quarters) => self.chrome.zoom = quarters as i32,
                None => self.status.push(format!(
                    "settings.json: {KEY}: {} is not a zoom (a multiple of 0.25 from -{top} to {top}), none used",
                    value.to_json(),
                    top = f64::from(top) / QUARTERS,
                )),
            }
        }
        self.saving.settings_file = Some(path);
    }

    /// The display's scale factor (at least ×1), which the zoom starts from.
    pub fn set_display_scale(&mut self, scale: Scale) {
        self.chrome.display_scale = scale.max(Scale::ONE);
    }

    /// The scale the UI shows at: the display's with the zoom, from ×1 to
    /// [`Scale::MAX`], whatever the window's size (it scrolls when too small).
    pub fn wanted_scale(&self) -> Scale {
        self.chrome.display_scale.step(self.chrome.zoom, Scale::ONE, Scale::MAX)
    }

    /// The window's size in physical pixels, which "Zoom: fit to window" fits.
    pub fn set_window_size(&mut self, width: i32, height: i32) {
        self.chrome.window = (width, height);
    }

    /// Ctrl++ (`step` 1), Ctrl+- (-1) and Ctrl+0 (0, the display's scale):
    /// zoom a quarter at a time, say the scale, and save the zoom.
    pub(super) fn zoom(&mut self, step: i32) {
        let before = self.wanted_scale();
        let wanted = if step == 0 { self.chrome.display_scale } else { before.step(step, Scale::ONE, Scale::MAX) };
        let limit = if step != 0 && wanted == before { ", the limit" } else { "" };
        self.set_zoom(wanted, format!("zoom x{wanted}{limit}"));
    }

    /// View > Zoom: fit to window: the biggest scale whose layout the window
    /// shows whole, without scrollbars; ×1 when even that one scrolls.
    pub(super) fn zoom_to_fit(&mut self) {
        let (width, height) = self.chrome.window;
        let fits = |scale: &Scale| {
            let (w, h) = view::min_window_size(*scale, self.shown());
            w <= width && h <= height
        };
        let fitting = (Scale::ONE.quarters()..=Scale::MAX.quarters()).map(Scale::from_quarters).rfind(fits);
        let message = match fitting {
            Some(scale) => format!("zoom x{scale}, fit to the window"),
            None => format!("zoom x1, the window ({width}x{height}) is smaller than the layout: it scrolls"),
        };
        self.set_zoom(fitting.unwrap_or(Scale::ONE), message);
    }

    /// Show the UI at `wanted`, say `message`, and save the zoom.
    fn set_zoom(&mut self, wanted: Scale, message: String) {
        self.chrome.zoom = wanted.quarters() - self.chrome.display_scale.quarters();
        self.message = message;
        let units = f64::from(self.chrome.zoom) / QUARTERS;
        if let Some(path) = &self.saving.settings_file
            && let Err(e) = store::save_setting(path, KEY, json::Value::from(units))
        {
            self.message = format!("zoom not saved: {}: {e}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(display: Scale) -> Session {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_display_scale(display);
        s
    }

    #[test]
    fn ctrl_plus_and_minus_zoom_by_quarters_whatever_the_window() {
        let mut s = session(Scale::ONE);
        s.set_size((10, 10, Scale::ONE));
        s.run(Command::ZoomIn);
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x1.25", Scale::from_quarters(5)));
        s.run(Command::ZoomIn);
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x1.5", Scale::from_quarters(6)));
        for _ in 0..12 {
            s.run(Command::ZoomIn);
        }
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x4, the limit", Scale::MAX));
        s.run(Command::ZoomReset);
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x1", Scale::ONE));
        s.run(Command::ZoomOut);
        assert_eq!(s.message, "zoom x1, the limit");
    }

    #[test]
    fn every_quarter_scale_needs_a_window_at_least_as_big_as_the_one_below() {
        let sizes: Vec<_> = (Scale::ONE.quarters()..=Scale::MAX.quarters())
            .map(|q| view::min_window_size(Scale::from_quarters(q), view::Shown::all(true)))
            .collect();
        for pair in sizes.windows(2) {
            assert!(pair[0].0 <= pair[1].0 && pair[0].1 <= pair[1].1, "{pair:?}");
        }
        let one = sizes[0];
        assert_eq!(sizes[4], (2 * one.0, 2 * one.1), "whole scales stay exact");
    }

    #[test]
    fn a_display_at_150_percent_shows_at_1_5_and_zooms_out_to_1() {
        let mut s = session(Scale::from_factor(1.5));
        assert_eq!(s.wanted_scale(), Scale::from_quarters(6));
        s.run(Command::ZoomOut);
        s.run(Command::ZoomOut);
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x1", Scale::ONE));
    }

    #[test]
    fn fit_to_window_picks_the_biggest_scale_the_window_shows_whole() {
        let mut s = session(Scale::ONE);
        let (w, h) = view::min_window_size(Scale::from_quarters(6), view::Shown::all(true));
        s.set_window_size(w + 1, h + 1);
        s.run(Command::ZoomToFit);
        assert_eq!((s.message.as_str(), s.wanted_scale()), ("zoom x1.5, fit to the window", Scale::from_quarters(6)));
        let (big_w, big_h) = view::min_window_size(Scale::MAX, view::Shown::all(true));
        s.set_window_size(big_w * 2, big_h * 2);
        s.run(Command::ZoomToFit);
        assert_eq!(s.wanted_scale(), Scale::MAX, "never over the biggest scale");
        s.set_window_size(320, 200);
        s.run(Command::ZoomToFit);
        assert_eq!(
            (s.message.as_str(), s.wanted_scale()),
            ("zoom x1, the window (320x200) is smaller than the layout: it scrolls", Scale::ONE)
        );
        // From a display at ×2, the zoom saved is the units under it.
        let mut hidpi = session(Scale::whole(2));
        hidpi.set_window_size(w, h);
        hidpi.run(Command::ZoomToFit);
        assert_eq!(hidpi.wanted_scale(), Scale::from_quarters(6));
        assert_eq!(hidpi.chrome.zoom, -2);
    }

    #[test]
    fn the_zoom_is_saved_and_read_back() {
        let dir = crate::test_support::scratch_dir("zoom");
        let path = dir.join("settings.json");
        std::fs::write(&path, r#"{"colorScheme":2}"#).unwrap();
        let mut s = session(Scale::ONE);
        s.load_zoom(Some(path.clone()));
        s.run(Command::ZoomIn);
        s.run(Command::ZoomIn);
        assert_eq!(store::load_setting(&path, KEY), Ok(Some(json::Value::from(0.5))));
        assert!(store::load_setting(&path, "colorScheme").unwrap().is_some(), "the other settings are kept");
        let mut again = session(Scale::whole(2));
        again.load_zoom(Some(path.clone()));
        assert_eq!(again.wanted_scale(), Scale::from_quarters(10), "units from the display's scale");
        // A whole number of units, as earlier versions wrote it, still reads.
        std::fs::write(&path, r#"{"uiZoom":1}"#).unwrap();
        let mut whole = session(Scale::ONE);
        whole.load_zoom(Some(path.clone()));
        assert_eq!(whole.wanted_scale(), Scale::whole(2));
        for wrong in ["9", "0.3"] {
            std::fs::write(&path, format!(r#"{{"uiZoom":{wrong}}}"#)).unwrap();
            let mut s = session(Scale::ONE);
            s.load_zoom(Some(path.clone()));
            let expected =
                format!("settings.json: uiZoom: {wrong} is not a zoom (a multiple of 0.25 from -3 to 3), none used");
            assert_eq!(s.status, [expected]);
            assert_eq!(s.wanted_scale(), Scale::ONE);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
