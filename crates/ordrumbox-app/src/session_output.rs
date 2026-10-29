//! The session's side of the output panel (`output_panel.js`): the master
//! bus settings sent to the player as they change and saved to
//! `settings.json` half a second after the last change (`persistMaster`),
//! the tabs, the spectrum of what plays.

use ordrumbox_engine::{MasterSettings, SCOPE_SIZE};

use super::*;
use crate::output::{self, OutputLayout, OutputView};
use crate::panel::{Item, Kind, Rect};

/// Master changes reach `settings.json` this long after the last one.
const MASTER_DELAY: Duration = Duration::from_millis(500);

impl Session {
    /// The master bus settings from `settings.json`, if it has some.
    pub(super) fn load_master(&mut self) {
        let Some(path) = store::settings_path() else { return };
        match store::load_master(&path) {
            Ok(Some(master)) => self.set_master(master, false),
            Ok(None) => {}
            Err(e) => self.status.push(format!("settings.json not loaded: {e}")),
        }
    }

    /// Use `master` (clamped); `save`: write it to `settings.json` soon.
    pub(super) fn set_master(&mut self, master: MasterSettings, save: bool) {
        self.master = master.clamped();
        if let Some(controller) = &mut self.controller {
            controller.set_master(self.master);
        }
        if save {
            self.master_dirty = Some(Instant::now());
        }
    }

    /// Setting `key` of the master bus at `value`.
    pub(super) fn set_master_value(&mut self, key: &str, value: f64) {
        self.set_master(output::set(self.master, key, value), true);
    }

    /// Write the master settings when they have settled; when to call again.
    pub(super) fn flush_master(&mut self) -> Option<Instant> {
        let since = self.master_dirty?;
        let due = since + MASTER_DELAY;
        if Instant::now() < due {
            return Some(due);
        }
        self.master_dirty = None;
        if let Some(path) = store::settings_path()
            && let Err(e) = store::save_master(&path, &self.master)
        {
            self.message = format!("settings not saved: {}: {e}", path.display());
        }
        None
    }

    /// The output strip's controls: the tabs, then the tab's knobs and sliders.
    pub(super) fn output_items(&self, layout: &mut session_panel::PanelLayout, width: i32, band: Rect, scale: i32) {
        let out = OutputLayout::strip(width, band, scale);
        for (i, (rect, label)) in out.tabs.iter().zip(output::TABS).enumerate() {
            let kind = Kind::Tab { label: label.into(), active: usize::from(self.output_tab) == i };
            layout.items.push(Item::new(Some(Id::OutTab(i as u8)), *rect, kind));
        }
        let mut control = |param: &output::Param, rect: Rect| {
            let value = output::get(&self.master, param.key);
            let text = param.format.value(value, param.unit);
            let kind = Kind::Value { spec: param.spec, value, label: Some(param.label.into()), text, lfo: false };
            layout.items.push(Item::new(Some(Id::Master(param.key)), rect, kind));
        };
        match self.output_tab {
            0 => control(&output::VOLUME, out.volume),
            1 => {
                control(&output::PRE_GAIN, out.pre_gain);
                for (param, rect) in output::COMPRESSOR.iter().zip(out.compressor) {
                    control(param, rect);
                }
            }
            _ => {
                for (param, rect) in output::FILTERS.iter().zip(out.filters) {
                    control(param, rect);
                }
            }
        }
        if self.output_tab == 1 {
            // The compressor's on/off button (lit: on).
            let kind = Kind::Toggle { on: self.master.bypass < 0.5 };
            layout.items.push(Item::new(Some(Id::Master("compBypass")), out.bypass, kind));
        }
    }

    /// A tab or the compressor button pressed.
    pub fn output_activate(&mut self, id: Id) {
        match id {
            Id::OutTab(i) => self.output_tab = i.min(2),
            Id::Master("compBypass") => {
                let bypass = if self.master.bypass >= 0.5 { 0.0 } else { 1.0 };
                self.set_master_value("compBypass", bypass);
            }
            _ => {}
        }
    }

    /// What the output strip draws: on the Vol tab, the spectrum of the
    /// last samples played (the analyser keeps its smoothing between frames).
    /// The strip is always shown.
    pub(super) fn output_view(&self) -> Option<OutputView> {
        if self.output_tab == 0
            && let Some(controller) = &self.controller
        {
            let mut samples = [0.0f32; SCOPE_SIZE];
            controller.scope(&mut samples);
            *self.spectrum.borrow_mut() = self.analyser.borrow_mut().bytes(&samples);
        }
        Some(OutputView { tab: self.output_tab, master: self.master, spectrum: self.spectrum.borrow().clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;

    const SIZE: (i32, i32, i32) = (1100, 680, 1);

    #[test]
    fn knobs_tabs_and_the_compressor_button() {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        // The output strip is always shown along the bottom.
        let out = {
            let band = s.body(SIZE).output;
            OutputLayout::strip(SIZE.0, band, 1)
        };
        // Vol: the wheel on the volume knob.
        let (x, y) = (out.volume.x + out.volume.w / 2, out.volume.y + 10);
        assert!(s.wheel(SIZE, x, y, -10, false, false));
        assert!((s.master.master - 0.9).abs() < 1e-6);
        assert!(s.master_dirty.is_some());
        // Comp: the button bypasses the compressor.
        s.mouse_down(SIZE, out.tabs[1].x + 4, out.tabs[1].y + 4, Button::Left, false);
        assert_eq!(s.output_tab, 1);
        s.mouse_down(SIZE, out.bypass.x + 4, out.bypass.y + 4, Button::Left, false);
        assert_eq!(s.master.bypass, 1.0);
        // A typed threshold.
        let t = out.compressor[0];
        s.mouse_down(SIZE, t.x + t.w / 2, t.y + 10, Button::Right, false);
        s.entry.as_mut().unwrap().text = "-30".into();
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.master.threshold, -30.0);
        // Flt: a click on the low cut slider's bar.
        s.mouse_down(SIZE, out.tabs[2].x + 4, out.tabs[2].y + 4, Button::Left, false);
        let f = out.filters[0];
        let (left, w) = crate::panel::slider_bar(f, true, true, 1);
        s.mouse_down(SIZE, left + w, f.y + 8, Button::Left, false);
        s.mouse_up();
        assert_eq!(s.master.lowcut, 500.0);
    }
}
