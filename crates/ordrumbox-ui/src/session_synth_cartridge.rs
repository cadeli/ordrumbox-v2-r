//! The Sound tab's synth cartridge: its LED plays the track's synth preset
//! or its sample, its screen and its arrows choose the preset.

use super::Session;
use crate::panel::Step;
use ordrumbox_core::json::Value;

impl Session {
    /// Track `t` on synth preset `key`, one undo step.
    pub(super) fn assign_synth(&mut self, t: usize, key: &str) {
        let updates = [
            ("useSynth", Value::Bool(true)),
            ("autoAssign", Value::Bool(false)),
            ("synthPreset", Value::String(key.to_owned())),
        ];
        let name = self.track_name(t);
        self.update_track(t, &updates, &format!("Synth on {name}"), false);
    }

    /// The synth cartridge's preset: played at once when the synth is on,
    /// kept for its LED when it is off.
    pub(super) fn pick_synth(&mut self, t: usize, key: &str) {
        let Some(track) = self.track(t) else { return };
        if track.use_synth {
            return self.assign_synth(t, key);
        }
        let name = self.track_name(t);
        let updates = [("synthPreset", Value::String(key.to_owned()))];
        self.update_track(t, &updates, &format!("Synth preset on {name}"), false);
    }

    /// The synth cartridge's LED: the preset it shows played, or the sample back.
    pub(super) fn toggle_synth_on(&mut self, t: usize) {
        let Some(track) = self.track(t) else { return };
        if track.use_synth {
            let name = self.track_name(t);
            return self.update_track(t, &[("useSynth", Value::Bool(false))], &format!("Synth off on {name}"), false);
        }
        let choice = self.synth_choice(track);
        match choice.values.get(choice.selected).cloned() {
            Some(key) => self.assign_synth(t, &key),
            None => self.message = "No synth presets loaded.".into(),
        }
    }

    /// The synth cartridge's arrows: the preset before or after the one shown.
    pub(super) fn step_synth(&mut self, t: usize, step: Step) {
        let Some(track) = self.track(t) else { return };
        let choice = self.synth_choice(track);
        let key = step.from(choice.selected, choice.values.len()).and_then(|i| choice.values.get(i)).cloned();
        match key {
            Some(key) => self.pick_synth(t, &key),
            None => self.message = "No synth presets loaded.".into(),
        }
    }
}
