//! The editors' cartridges: the Sound tab's synth (its LED plays the
//! track's synth preset or its sample, its screen and its arrows choose the
//! preset), an effect's LED and the arrows of its type, of an LFO's wave,
//! of an arpeggio's scale and direction.

use super::Session;
use super::session_panel::{filter_on, number};
use crate::editors::{self, EditorState, FxDef};
use crate::panel::{Id, Step};
use ordrumbox_core::Track;
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

    /// An LFO cartridge's arrows: the wave before or after the one shown, set
    /// as its screen's list sets it (an LFO off goes on at it).
    pub(super) fn step_lfo_wave(&mut self, t: usize, i: u8, step: Step) {
        let Some(track) = self.track(t) else { return };
        let target = &editors::LFO_TARGETS[usize::from(i).min(editors::LFO_TARGETS.len() - 1)];
        let waves = editors::WAVE_TYPES;
        let at = waves.iter().position(|w| *w == editors::lfo_wave(track, target)).unwrap_or(0);
        if let Some(wave) = step.from(at, waves.len()).and_then(|i| waves.get(i)) {
            self.select(Id::LfoType(i), wave);
        }
    }

    /// The arrows of an arpeggio's scale or direction screen: the one before or
    /// after the one shown, set as its list sets it.
    pub(super) fn step_note_select(&mut self, t: usize, key: &'static str, step: Step) {
        let view = self.note_view(t);
        let choice = if key == "arpScale" { view.scales } else { view.arp_direction };
        if let Some(value) = step.from(choice.selected, choice.values.len()).and_then(|i| choice.values.get(i)) {
            self.select(Id::NoteSelect(key), value);
        }
    }

    /// An effect cartridge's arrows: the type before or after the one shown,
    /// set as its screen's list sets it.
    pub(super) fn step_track_select(&mut self, t: usize, key: &'static str, step: Step) {
        let Some(track) = self.track(t) else { return };
        let values = editors::track_select_options(key).0;
        let at = editors::index_of(&values, &editors::shown_value(track, key, &self.editors));
        if let Some(value) = step.from(at, values.len()).and_then(|i| values.get(i)).cloned() {
            self.select(Id::TrackSelect(key), &value);
        }
    }
}

/// What an effect's LED writes: off, its amount at 0 (the filter, its
/// bypass); on, its amount back (`on_value` when it had none) and, when it
/// had none, the type it shows.
pub(super) fn fx_switch(track: &Track, fx: &FxDef, state: &EditorState) -> Vec<(&'static str, Value)> {
    let is_filter = fx.key == "filterType";
    if editors::fx_on(track, fx) {
        let off = if is_filter { Value::String("allpass".into()) } else { number(0.0) };
        return vec![(fx.key, off)];
    }
    if is_filter {
        return filter_on(track, editors::shown_value(track, fx.key, state));
    }
    let amount = editors::number(track, fx.key);
    let mut updates = vec![(fx.key, number(if amount > 0.0 { amount } else { fx.on_value }))];
    if let Some(key) = fx.type_key() {
        let shown = editors::shown_value(track, key, state);
        if shown != editors::text_of(track, key) {
            updates.push((key, Value::String(shown)));
        }
    }
    updates
}
