//! The master bus and the shared returns' settings (the mixer shows them):
//! sent to the player as they change and saved to `settings.json` half a
//! second after the last change.

use ordrumbox_core::num;
use ordrumbox_engine::{MasterSettings, ReturnSettings};

use super::Session;
use crate::store;
use crate::{master, returns};
use std::time::Instant;

/// The master bus and the returns' settings in use.
#[derive(Debug, Default)]
pub(super) struct OutputPanel {
    master: MasterSettings,
    returns: ReturnSettings,
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
        self.not_undoable(&format!("master: {key} {}", num::text(shown)));
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
}
