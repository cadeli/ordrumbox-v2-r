//! The session's side of the output panel (`output_panel.js`): the master
//! bus and the shared returns' settings, sent to the player as they change
//! and saved to `settings.json` half a second after the last change, the
//! page shown, the spectrum of what plays.

use crate::scale::Scale;
use ordrumbox_core::num;
use ordrumbox_engine::strip_settings::delay_beats_used;
use ordrumbox_engine::{MasterSettings, ReturnSettings, SCOPE_SIZE};

use super::{Session, session_panel};
use crate::output::{self, OutputLayout, OutputPage, OutputView};
use crate::panel::Id;
use crate::panel::{Item, Kind, Rect};
use crate::store;
use crate::{master, returns};
use std::time::Instant;

/// The output panel's state: the master bus and the returns' settings, the
/// page shown; the spectrum's analyser and its last bins.
#[derive(Debug, Default)]
pub(super) struct OutputPanel {
    master: MasterSettings,
    returns: ReturnSettings,
    page: OutputPage,
    analyser: std::cell::RefCell<crate::spectrum::Analyser>,
    spectrum: std::cell::RefCell<Vec<u8>>,
}

impl OutputPanel {
    /// The master bus settings in use.
    pub(super) fn master(&self) -> MasterSettings {
        self.master
    }

    /// The returns' settings in use.
    pub(super) fn returns(&self) -> ReturnSettings {
        self.returns
    }
}

impl Session {
    /// The master bus and the returns' settings from `settings.json`, if it has some.
    pub(super) fn load_master(&mut self) {
        let Some(path) = store::settings_path() else { return };
        match store::load_setting(&path, master::SECTION.name) {
            Ok(Some(value)) => {
                let (master, issues) = master::SECTION.read_json(&value);
                self.status.extend(issues.into_iter().map(|issue| format!("settings.json: {issue}")));
                self.set_master(master, false);
            }
            Ok(None) => {}
            Err(e) => return self.status.push(format!("settings.json not loaded: {e}")),
        }
        // A file that cannot be read has been reported above.
        if let Ok(Some(value)) = store::load_setting(&path, returns::SECTION.name) {
            let (returns, issues) = returns::SECTION.read_json(&value);
            self.status.extend(issues.into_iter().map(|issue| format!("settings.json: {issue}")));
            self.set_returns(returns, false);
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
        self.set_master(master::SECTION.set(self.output_panel.master, key, value), true);
        let shown = master::SECTION.get(&self.output_panel.master, key);
        self.not_undoable(&format!("master: {key} {}", ordrumbox_core::num::text(shown)));
    }

    /// The shared returns at `returns` (clamped), for playback and the
    /// renders; `save`: write them to `settings.json` soon.
    pub(super) fn set_returns(&mut self, returns: ReturnSettings, save: bool) {
        self.output_panel.returns = returns.clamped();
        if let Some(controller) = self.playback.controller_mut() {
            controller.set_returns(self.output_panel.returns);
        }
        if let Some(render) = &mut self.render {
            render.returns = self.output_panel.returns;
        }
        if save {
            self.saving.master.touch();
        }
    }

    /// Setting `key` of the returns at `value`.
    pub(super) fn set_returns_value(&mut self, key: &str, value: f64) {
        self.set_returns(returns::SECTION.set(self.output_panel.returns, key, value), true);
        let shown = returns::SECTION.get(&self.output_panel.returns, key);
        self.not_undoable(&format!("returns: {key} {}", num::text(shown)));
    }

    /// Write the master and the returns' settings when they have settled; when to call again.
    pub(super) fn flush_master(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.master.ready() {
            return next;
        }
        let path = store::settings_path()?;
        let panel = &self.output_panel;
        let saved = store::save_setting(&path, master::SECTION.name, master::SECTION.to_json(&panel.master))
            .and_then(|()| store::save_setting(&path, returns::SECTION.name, returns::SECTION.to_json(&panel.returns)));
        if let Err(e) = saved {
            self.message = format!("settings not saved: {}: {e}", path.display());
        }
        None
    }

    /// The output strip's controls: its page tabs, the volume, then the
    /// page's knobs, button and sliders.
    pub(super) fn output_items(&self, layout: &mut session_panel::PanelLayout, width: i32, band: Rect, scale: Scale) {
        let out = OutputLayout::strip(width, band, scale);
        for ((i, page), rect) in OutputPage::ALL.into_iter().enumerate().zip(out.pages) {
            let kind = Kind::Tab { label: page.label().into(), active: self.output_panel.page == page };
            layout.items.push(Item::new(Some(Id::OutputPage(i as u8)), rect, kind));
        }
        let knob = |param: &output::Param, value: f64, text: String| Kind::Value {
            spec: param.spec,
            value,
            label: Some(param.label.into()),
            text,
            lfo: false,
        };
        let mut control = |param: &output::Param, rect: Rect| {
            let value = master::SECTION.get(&self.output_panel.master, param.key);
            let kind = knob(param, value, param.format.value(value, param.unit));
            layout.items.push(Item::new(Some(Id::Master(param.key)), rect, kind));
        };
        control(&output::VOLUME, out.volume);
        if self.output_panel.page == OutputPage::Returns {
            for (param, rect) in output::RETURNS.iter().zip(out.returns) {
                let value = returns::SECTION.get(&self.output_panel.returns, param.key);
                let item =
                    Item::new(Some(Id::Returns(param.key)), rect, knob(param, value, self.return_text(param, value)));
                layout.items.push(item);
            }
            return;
        }
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

    /// What a return's knob shows: its value, and for the echo's time the
    /// beats played when the tempo halves them to fit the echo.
    fn return_text(&self, param: &output::Param, value: f64) -> String {
        let text = param.format.value(value, param.unit);
        if param.key != "echoBeats" {
            return text;
        }
        let used = delay_beats_used(value, self.tempo());
        if used == value { text } else { format!("{} used", num::text(used)) }
    }

    /// The compressor button pressed, or a page tab.
    pub fn output_activate(&mut self, id: Id) {
        match id {
            Id::Master("compBypass") => {
                let bypass = if self.output_panel.master.bypass >= 0.5 { 0.0 } else { 1.0 };
                self.set_master_value("compBypass", bypass);
            }
            Id::OutputPage(i) => {
                if let Some(&page) = OutputPage::ALL.get(usize::from(i)) {
                    self.output_panel.page = page;
                }
            }
            _ => {}
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
        let spectrum = self.output_panel.spectrum.borrow().clone();
        Some(OutputView { page: self.output_panel.page, master: self.output_panel.master, spectrum })
    }
}

#[cfg(test)]
mod tests {
    use super::super::session_panel::PanelKey;
    use super::*;
    use crate::kits::KitSet;
    use crate::ui_input::Button;
    use ordrumbox_core::Library;

    use crate::test_support::{SIZE, click};

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
        click(&mut s, out.bypass.x + 4, out.bypass.y + 4);
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
        click(&mut s, left + w, f.y + 8);
        s.mouse_up();
        assert_eq!(s.output_panel.master.lowcut, 500.0);
    }

    #[test]
    fn the_echo_tab_shows_the_returns_in_place_of_the_master_bus() {
        let mut s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        let out = OutputLayout::strip(SIZE.0, s.body(SIZE).output, Scale::ONE);
        let rects = |s: &Session, id: fn(&Id) -> bool| -> Vec<Rect> {
            let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
            layout.items.iter().filter(|i| i.id.as_ref().is_some_and(id)).map(|i| i.rect).collect()
        };
        let volume = rects(&s, |id| *id == Id::Master("volume"));
        click(&mut s, out.pages[1].x + 4, out.pages[1].y + 4);
        assert_eq!(s.output_panel.page, OutputPage::Returns);
        assert_eq!(rects(&s, |id| *id == Id::Master("volume")), volume, "the volume stays put");
        assert_eq!(rects(&s, |id| matches!(id, Id::Returns(_))), out.returns);
        assert!(rects(&s, |id| *id == Id::Master("threshold")).is_empty(), "no compressor on this page");
        // The wheel on the feedback knob.
        let feedback = out.returns[1];
        assert!(s.wheel(SIZE, feedback.x + feedback.w / 2, feedback.y + 10, -10, false, false));
        assert!((s.output_panel.returns.echo_feedback - 0.45).abs() < 1e-6, "{}", s.output_panel.returns.echo_feedback);
        assert!(s.saving.master.pending());
        click(&mut s, out.pages[0].x + 4, out.pages[0].y + 4);
        assert_eq!(s.output_panel.page, OutputPage::Master);
        assert!(rects(&s, |id| matches!(id, Id::Returns(_))).is_empty());
    }

    #[test]
    fn the_echo_time_shows_the_beats_used_when_the_tempo_halves_them() {
        let time = &output::RETURNS[0];
        let s = Session::new(Library::default(), KitSet::empty(), Vec::new());
        assert_eq!(s.return_text(time, 0.75), "0.75 beat");
        let slow = Session::new(Library { bpm: 60.0, ..Library::default() }, KitSet::empty(), Vec::new());
        assert_eq!(slow.return_text(time, 4.0), "2 used");
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
