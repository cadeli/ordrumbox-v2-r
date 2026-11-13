//! The session's side of the output panel (`output_panel.js`): the master
//! bus settings sent to the player as they change and saved to
//! `settings.json` half a second after the last change,
//! the spectrum of what plays.

use crate::scale::Scale;
use ordrumbox_engine::{MasterSettings, SCOPE_SIZE};

use super::*;
use crate::master;
use crate::output::{self, OutputLayout, OutputView};
use crate::panel::{Item, Kind, Rect};

/// The output panel's state: the master bus settings; the spectrum's analyser
/// and its last bins.
#[derive(Debug, Default)]
pub(super) struct OutputPanel {
    master: MasterSettings,
    analyser: std::cell::RefCell<crate::spectrum::Analyser>,
    spectrum: std::cell::RefCell<Vec<u8>>,
}

impl OutputPanel {
    /// The master bus settings in use.
    pub(super) fn master(&self) -> MasterSettings {
        self.master
    }
}

impl Session {
    /// The master bus settings from `settings.json`, if it has some.
    pub(super) fn load_master(&mut self) {
        let Some(path) = store::settings_path() else { return };
        match store::load_setting(&path, "master") {
            Ok(Some(value)) => {
                let (master, issues) = master::from_json(&value);
                self.status.extend(issues.into_iter().map(|issue| format!("settings.json: {issue}")));
                self.set_master(master, false);
            }
            Ok(None) => {}
            Err(e) => self.status.push(format!("settings.json not loaded: {e}")),
        }
    }

    /// Use `master` (clamped); `save`: write it to `settings.json` soon.
    pub(super) fn set_master(&mut self, master: MasterSettings, save: bool) {
        self.output_panel.master = master.clamped();
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_master(self.output_panel.master);
        }
        if save {
            self.saving.master.touch();
        }
    }

    /// Setting `key` of the master bus at `value`.
    pub(super) fn set_master_value(&mut self, key: &str, value: f64) {
        self.set_master(master::set(self.output_panel.master, key, value), true);
        let shown = master::get(&self.output_panel.master, key);
        self.not_undoable(format!("master: {key} {}", ordrumbox_core::num::text(shown)));
    }

    /// Write the master settings when they have settled; when to call again.
    pub(super) fn flush_master(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.master.ready() {
            return next;
        }
        if let Some(path) = store::settings_path()
            && let Err(e) = store::save_setting(&path, "master", master::to_json(&self.output_panel.master))
        {
            self.message = format!("settings not saved: {}: {e}", path.display());
        }
        None
    }

    /// The output strip's controls: its knobs, its button and its sliders.
    pub(super) fn output_items(&self, layout: &mut session_panel::PanelLayout, width: i32, band: Rect, scale: Scale) {
        let out = OutputLayout::strip(width, band, scale);
        let mut control = |param: &output::Param, rect: Rect| {
            let value = master::get(&self.output_panel.master, param.key);
            let text = param.format.value(value, param.unit);
            let kind = Kind::Value { spec: param.spec, value, label: Some(param.label.into()), text, lfo: false };
            layout.items.push(Item::new(Some(Id::Master(param.key)), rect, kind));
        };
        control(&output::VOLUME, out.volume);
        control(&output::PRE_GAIN, out.pre_gain);
        for (param, rect) in output::COMPRESSOR.iter().zip(out.compressor) {
            control(param, rect);
        }
        for (param, rect) in output::FILTERS.iter().zip(out.filters) {
            control(param, rect);
        }
        // The compressor's on/off button (lit: on).
        let kind = Kind::Toggle { on: self.output_panel.master.bypass < 0.5 };
        layout.items.push(Item::new(Some(Id::Master("compBypass")), out.bypass, kind));
    }

    /// The compressor button pressed.
    pub fn output_activate(&mut self, id: Id) {
        if id == Id::Master("compBypass") {
            let bypass = if self.output_panel.master.bypass >= 0.5 { 0.0 } else { 1.0 };
            self.set_master_value("compBypass", bypass);
        }
    }

    /// What the output strip draws: the spectrum of the last samples played (the analyser keeps its smoothing between frames).
    /// `None` in a plugin (it has no output strip) or when View > Master folded it.
    pub(super) fn output_view(&self) -> Option<OutputView> {
        if !self.output_shown() {
            return None;
        }
        if let Some(controller) = self.playback.controller() {
            let mut samples = [0.0f32; SCOPE_SIZE];
            controller.scope(&mut samples);
            *self.output_panel.spectrum.borrow_mut() = self.output_panel.analyser.borrow_mut().bytes(&samples);
        }
        Some(OutputView { master: self.output_panel.master, spectrum: self.output_panel.spectrum.borrow().clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;

    const SIZE: (i32, i32, Scale) = (1100, 680, Scale::ONE);

    #[test]
    fn knobs_sliders_and_the_compressor_button() {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        // The output strip is always shown along the bottom.
        let out = {
            let band = s.body(SIZE).output;
            OutputLayout::strip(SIZE.0, band, Scale::ONE)
        };
        // The wheel on the volume knob.
        let (x, y) = (out.volume.x + out.volume.w / 2, out.volume.y + 10);
        assert!(s.wheel(SIZE, x, y, -10, false, false));
        assert!((s.output_panel.master.master - 0.9).abs() < 1e-6);
        assert!(s.saving.master.pending());
        // The button bypasses the compressor.
        s.mouse_down(SIZE, out.bypass.x + 4, out.bypass.y + 4, Button::Left, false);
        assert_eq!(s.output_panel.master.bypass, 1.0);
        // A typed threshold (right-click menu: "Type a value...").
        let t = out.compressor[0];
        s.mouse_down(SIZE, t.x + t.w / 2, t.y + 10, Button::Right, false);
        s.pick(0);
        s.chrome.widgets.entry.as_mut().unwrap().text = "-30".into();
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.output_panel.master.threshold, -30.0);
        // A click on the low cut slider's bar.
        let f = out.filters[0];
        let (left, w) = crate::panel::slider_bar(f, true, true, Scale::ONE);
        s.mouse_down(SIZE, left + w, f.y + 8, Button::Left, false);
        s.mouse_up();
        assert_eq!(s.output_panel.master.lowcut, 500.0);
    }

    #[test]
    fn view_master_folds_the_strip() {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        let shown = s.body(SIZE);
        s.run(crate::menu::Command::MasterStrip);
        // No strip, nothing of it to click: the editors reach down to where it was.
        let folded = s.body(SIZE);
        assert_eq!(folded.output.h, 0);
        assert!(folded.main_bottom > shown.main_bottom);
        assert!(s.output_view().is_none());
        assert!(!s.panel_layout(SIZE.0, SIZE.1, Scale::ONE).items.iter().any(|i| matches!(i.id, Some(Id::Master(_)))));
        assert!(!s.shown().output);
        s.run(crate::menu::Command::MasterStrip);
        assert_eq!(s.body(SIZE), shown);
        assert!(s.output_view().is_some());
    }
}
