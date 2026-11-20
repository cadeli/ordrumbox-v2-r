//! The editors' cartridges: the Sound tab's synth (its LED plays the
//! track's synth preset or its sample, its screen and its arrows choose the
//! preset), an effect's LED and its type, and the arrows of every screen
//! (a preset, an effect's type, an LFO's wave, an arpeggio's scale and
//! direction).

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
            None => self.message = NO_SYNTH_PRESETS.into(),
        }
    }

    /// A screen's arrows: the value before or after the one screen `id`
    /// shows, set as its list sets it (`Id::Synth`, `Id::LfoType`,
    /// `Id::TrackSelect`, `Id::NoteSelect`).
    pub(super) fn step_screen(&mut self, t: usize, id: Id, step: Step) {
        let Some(choice) = self.screen_choice(t, id) else { return };
        match choice.stepped(step) {
            Some(value) => self.select(id, value),
            None if id == Id::Synth => self.message = NO_SYNTH_PRESETS.into(),
            None => self.message = "Nothing to choose from.".into(),
        }
    }

    /// What screen `id` shows on track `t`; `None` when it is no screen.
    fn screen_choice(&self, t: usize, id: Id) -> Option<editors::Choice> {
        let track = self.track(t)?;
        match id {
            Id::Synth => Some(self.synth_choice(track)),
            Id::LfoType(i) => editors::lfo_target(i).map(|target| editors::lfo_wave_choice(track, target)),
            Id::TrackSelect(key) => Some(editors::select_choice(track, key, &self.editors)),
            Id::NoteSelect(key) => {
                let view = self.note_view(t);
                Some(if key == "arpScale" { view.scales } else { view.arp_direction })
            }
            _ => None,
        }
    }
}

/// Why the synth cartridge has nothing to play.
const NO_SYNTH_PRESETS: &str = "No synth presets loaded.";

/// What an effect's LED writes: off, its amount at 0 (the filter, its
/// bypass, its type kept in `state` for the way back); on, the type it shows
/// when it has none, else its amount at `on_value`.
pub(super) fn fx_switch(track: &Track, fx: &FxDef, state: &mut EditorState) -> Vec<(&'static str, Value)> {
    if editors::fx_on(track, fx) {
        if fx.is_filter() {
            state.prev_filter_type = Some(track.filter_type.clone());
            return vec![(fx.key, Value::String("allpass".into()))];
        }
        return vec![(fx.key, number(0.0))];
    }
    if let Some(key) = fx.type_key() {
        let shown = editors::shown_value(track, key, state);
        if fx.is_filter() || shown != editors::text_of(track, key) {
            return fx_type_updates(track, fx, &shown);
        }
    }
    vec![(fx.key, number(fx.on_value))]
}

/// What choosing type `value` of effect `fx` writes: the type and, for a
/// type its list offers, the effect on (the filter out of its bypass, an
/// effect without an amount at its `on_value`), as an LFO goes on at a wave.
pub(super) fn fx_type_updates(track: &Track, fx: &FxDef, value: &str) -> Vec<(&'static str, Value)> {
    if fx.is_filter() {
        return filter_on(track, value.to_owned());
    }
    let Some(key) = fx.type_key() else { return Vec::new() };
    let mut updates = vec![(key, Value::String(value.to_owned()))];
    let offered = editors::track_select_options(key).0.iter().any(|v| v == value);
    if offered && editors::number(track, fx.key) <= 0.0 {
        updates.push((fx.key, number(fx.on_value)));
    }
    updates
}
