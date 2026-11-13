//! The edits the editors' widgets make: a track's settings, sound, loop,
//! LFOs and filter, the note editor's note, a kit sample's knobs, and the
//! sounds assigned to a pattern's tracks. Each one is an undo step named by
//! its control.

use ordrumbox_core::SetField;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::assign::synth_for;

use super::session_panel::{NoteTarget, filter_on, history_label, number};
use super::{Session, session_grid};
use crate::editors::{self, FILTER_TYPES, FX_DEFS, KNOB_BAR, LFO_TARGETS, STYLE_DEFAULT};
use crate::kits::Field;
use crate::menu::Command;
use crate::panel::{Dropdown, Id, LfoParam};
use crate::picker::Purpose;
use std::path::Path;
use std::time::Instant;

impl Session {
    /// View > Track editor.
    pub fn toggle_track_editor(&mut self) {
        self.chrome.show_track_editor = !self.chrome.show_track_editor;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
    }

    /// View > Note editor.
    pub fn toggle_note_editor(&mut self) {
        self.chrome.show_note_editor = !self.chrome.show_note_editor;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
    }

    /// View > Master: the master strip along the bottom, shown or folded.
    pub fn toggle_master_strip(&mut self) {
        self.chrome.show_master_strip = !self.chrome.show_master_strip;
        self.chrome.widgets.focus = None;
        self.chrome.widgets.dropdown = None;
    }

    /// H: every track of pattern `p` on the synth patch that matches its name, written to the
    /// track; a random patch for a name nothing matches.
    pub fn convert_to_generated_sounds(&mut self, p: usize) {
        if self.editor.library().patterns.get(p).is_none() {
            self.message = "No pattern selected".into();
            return;
        }
        let keys: Vec<String> = self.synths.presets.keys().into_iter().map(str::to_owned).collect();
        if keys.is_empty() {
            self.message = "No synth patches loaded".into();
            return;
        }
        let (rng, synths) = (&mut self.generators.rng, &self.synths.presets);
        let mut random = Vec::new();
        let changed = self.editor.update_tracks(p, "Convert tracks to generated sounds", |_, track| {
            let mut pick = |n: usize| rng.below(n as u64) as usize;
            let matched = synth_for(&track.name, synths, &mut pick);
            let key = matched.unwrap_or_else(|| {
                let key = keys[pick(keys.len())].clone();
                random.push(format!("{}→{key}", track.name));
                key
            });
            vec![
                ("useSynth", Value::Bool(true)),
                ("autoAssign", Value::Bool(false)),
                ("synthPreset", Value::String(key)),
            ]
        });
        if changed {
            self.edited();
        }
        random.dedup();
        self.message = if random.is_empty() {
            "All tracks converted to generated sounds".into()
        } else {
            format!("No synth patch for: {} (random)", random.join(", "))
        };
    }

    /// J: every track of pattern `p` back on auto-assign (the kit's
    /// sound for its name).
    pub fn auto_assign_all_tracks(&mut self, p: usize) {
        if self.editor.library().patterns.get(p).is_none() {
            self.message = "No pattern selected".into();
            return;
        }
        let changed = self.editor.update_tracks(p, "Auto-assign all tracks", |_, _| {
            vec![("autoAssign", Value::Bool(true)), ("useSynth", Value::Bool(false))]
        });
        if changed {
            self.edited();
        }
        self.message = "All tracks auto-assigned".into();
    }

    /// K: a random sample, of any kit, on every track of pattern `p`.
    pub fn assign_random_samples(&mut self, p: usize) {
        if self.editor.library().patterns.get(p).is_none() {
            self.message = "No pattern selected".into();
            return;
        }
        let mut urls: Vec<String> =
            self.kits.kits().iter().flat_map(|k| k.samples.iter().map(|i| i.path.clone())).collect();
        urls.sort_unstable();
        urls.dedup();
        if urls.is_empty() {
            self.message = "No samples loaded".into();
            return;
        }
        let rng = &mut self.generators.rng;
        let changed = self.editor.update_tracks(p, "Random samples on all tracks", |_, _| {
            let url = urls[rng.below(urls.len() as u64) as usize].clone();
            vec![
                ("autoAssign", Value::Bool(false)),
                ("useSynth", Value::Bool(false)),
                ("samplePath", Value::String(url)),
            ]
        });
        if changed {
            self.edited();
        }
        self.message = "Random samples assigned".into();
    }

    // ─── What the widgets do ─────────────────────────────────────────────────

    pub(super) fn track_name(&self, t: usize) -> String {
        self.track(t).map_or_else(String::new, |tr| tr.name.clone())
    }

    /// `cmd.updateTrack` on the edited track; a step of history.
    fn update_track(&mut self, t: usize, updates: &[(&str, Value)], desc: &str, coalesce: bool) {
        if self.editor.update_track(self.pattern_index, t, updates, Some(desc), coalesce) {
            self.edited();
        }
    }

    /// A row's label clicked.
    pub(super) fn select_row(&mut self, id: Id) {
        if let Id::Generation(key) | Id::Compose(key) | Id::Loop(key) | Id::Fx(key) | Id::Row(key) = id {
            self.editors.selected_prop = Some(key);
        }
    }

    /// A knob or a slider set to `value`.
    pub(super) fn set_value(&mut self, id: Id, value: f64) {
        if let Id::Bpm = id {
            return self.set_tempo(value);
        }
        if let Id::Macro(key) = id {
            return self.set_macro(key, value);
        }
        if let Id::Master(key) = id {
            return self.set_master_value(key, value);
        }
        if let Id::SynthField(fi) = id {
            return self.synth_value(fi, value);
        }
        if let Id::KitKnob(field) = id {
            return self.set_kit_value(field, value);
        }
        if let Id::TrackVolume(t) = id {
            let desc = format!("Volume on {}", self.track_name(t));
            return self.update_track(t, &[("volume", number(value))], &desc, true);
        }
        let Some(t) = self.editor_track() else { return };
        let name = self.track_name(t);
        match id {
            Id::KnobBar(key) => {
                let label = KNOB_BAR.iter().find(|(p, _)| p.key == key).map_or(key, |(p, _)| p.label);
                self.update_track(t, &[(key, number(value))], &format!("{label} on {name}"), true);
            }
            Id::Fx(key) => {
                self.update_track(t, &[(key, number(value))], &format!("{} on {name}", history_label(id)), true)
            }
            Id::Generation(key) => {
                self.update_track(t, &[(key, number(value))], &format!("{} on {name}", history_label(id)), true)
            }
            Id::Compose(key) => {
                // A double-click gives the setting back to the style.
                let value = if value <= STYLE_DEFAULT { Value::Null } else { number(value) };
                self.update_track(t, &[(key, value)], &format!("{} on {name}", history_label(id)), true)
            }
            Id::Loop(key) => self.loop_slider(t, key, value),
            Id::Decay => {
                // The sample's decay (the sound registry's in the JS): not undoable.
                if let Some((kit, sample)) = self.track_sample_path(t).and_then(|u| self.kits.find_path(&u)) {
                    self.set_kit_sample_value(kit, sample, Field::Decay, value);
                }
            }
            Id::Lfo(i, param) => self.lfo_value(t, i, param, value),
            Id::NoteKnob(key) | Id::NoteSlider(key) => self.note_value(t, key, value),
            _ => {}
        }
    }

    /// Track `t` on synth preset `key` ("none": back on its sample), one undo step.
    pub(super) fn assign_synth(&mut self, t: usize, key: &str) {
        let updates: Vec<(&str, Value)> = if key == "none" {
            vec![("useSynth", Value::Bool(false))]
        } else {
            vec![
                ("useSynth", Value::Bool(true)),
                ("autoAssign", Value::Bool(false)),
                ("synthPreset", Value::String(key.to_owned())),
            ]
        };
        let name = self.track_name(t);
        self.update_track(t, &updates, &format!("Synth on {name}"), false);
    }

    /// `onLoopSlider`.
    fn loop_slider(&mut self, t: usize, key: &'static str, value: f64) {
        let Some(track) = self.track(t) else { return };
        let desc = format!("{} on {}", history_label(Id::Loop(key)), track.name);
        let max_steps = f64::from(track.beat_count * track.steps_per_beat);
        match key {
            "stepsPerBeat" => {
                if self.editor.set_steps_per_beat(self.pattern_index, t, value, true) {
                    self.edited();
                }
            }
            "loopLength" => self.update_track(t, &[(key, number(value.min(max_steps)))], &desc, true),
            _ => self.update_track(t, &[(key, number(value))], &desc, true),
        }
    }

    /// The LFO object of target `i` (a default one first).
    fn lfo_object(&self, t: usize, i: u8, kind: &str) -> (Object, &'static str, bool) {
        let target = &LFO_TARGETS[usize::from(i).min(LFO_TARGETS.len() - 1)];
        let current = self.track(t).and_then(|tr| tr.field(target.lfo_key)).and_then(|v| v.as_object().cloned());
        let created = current.is_none();
        let object = current.unwrap_or_else(|| {
            let mut o = Object::new();
            o.insert("type", Value::String(kind.to_owned()));
            o.insert("freq", number(1.0));
            o.insert("min", number(target.min));
            o.insert("max", number(target.max));
            o.insert("phase", number(0.0));
            o
        });
        (object, target.lfo_key, created)
    }

    fn lfo_value(&mut self, t: usize, i: u8, param: LfoParam, value: f64) {
        let (mut object, lfo_key, _) = self.lfo_object(t, i, "sine");
        object.insert(param.key(), number(value));
        let desc = format!("LFO {} on {}", param.key(), self.track_name(t));
        self.update_track(t, &[(lfo_key, Value::Object(object))], &desc, true);
    }

    fn note_value(&mut self, t: usize, key: &'static str, value: f64) {
        let Some(note) = self.shown_note(t) else { return };
        let (updates, what) = if key == "arpRange" {
            let range = value.max(0.0) as usize;
            (
                vec![("arp", editors::arp_value(&note, &self.generators.scale_list, None, None, Some(range)))],
                "arp".to_owned(),
            )
        } else {
            (vec![(key, number(value))], history_label(Id::NoteSlider(key)))
        };
        let desc = format!("Edit note {what} on {}", self.track_name(t));
        self.update_note(t, &updates, &desc, true);
    }

    /// `cmd.updateNote` on the note editor's note (a detached default note
    /// is changed in place, without history).
    fn update_note(&mut self, t: usize, updates: &[(&str, Value)], desc: &str, coalesce: bool) {
        match self.note_target(t) {
            NoteTarget::Note(n) => {
                if self.editor.update_note(self.pattern_index, t, n, updates, Some(desc), coalesce) {
                    self.edited();
                }
            }
            NoteTarget::Detached => {
                let mut note = self.shown_note(t).unwrap_or_default();
                for (key, value) in updates {
                    match note.set_field(key, value) {
                        SetField::Set => {}
                        SetField::WrongType => self.message = format!("note {key}: wrong type ({value}), not set"),
                        SetField::Unknown => self.message = format!("note {key}: no such setting, not set"),
                    }
                }
                self.selection.detached = Some((t, note));
            }
            NoteTarget::Empty { .. } => {}
        }
    }

    fn set_kit_value(&mut self, field: Field, value: f64) {
        let Some(sample) = self.kit_editor.sample else { return };
        self.set_kit_sample_value(self.kit_index, sample, field, value);
    }

    /// A kit knob: the kit changes now, the player and the disk shortly after.
    fn set_kit_sample_value(&mut self, kit: usize, sample: usize, field: Field, value: f64) {
        if let Some(message) = self.kits.set_value(kit, sample, field, value) {
            self.not_undoable(message);
            self.saving.kits.touch();
        }
    }

    /// Apply waiting kit knob edits when due; when they will be.
    pub(super) fn flush_kits(&mut self) -> Option<Instant> {
        if let Err(next) = self.saving.kits.ready() {
            return next;
        }
        let message = self.message.clone();
        self.kits_changed(message);
        None
    }

    /// An option picked in the open list.
    pub(super) fn pick(&mut self, index: usize) {
        let Some(mut dropdown) = self.chrome.widgets.dropdown.take() else { return };
        let Some(value) = dropdown.values.get(index).cloned() else { return };
        // A menu that opens another stays shown beside it, this line highlighted.
        dropdown.at = index;
        match dropdown.id {
            Id::GridMenu => self.grid_menu_pick(&value, dropdown),
            Id::TrackNameMenu(target) => self.track_name_menu_pick(target, &value, dropdown),
            Id::KitInstrumentMenu(sample) => self.kit_instrument_menu_pick(sample, &value, dropdown),
            Id::InstrumentMenu => self.instrument_menu_pick(&value, dropdown),
            Id::ControlMenu => self.control_menu_pick(&value, dropdown.control),
            id => self.select(id, &value),
        }
    }

    /// The Sound tab's instrument menus under `rect` (`current` is the
    /// track's instrument): the categories of the instruments that have samples.
    pub(super) fn instrument_menu(&mut self, current: &str, rect: crate::panel::Rect) {
        let sampled = self.sampled_instruments();
        let (values, labels) = session_grid::category_lines_where(|id| sampled.contains(id));
        let title = format!("Instrument: {current}");
        let menu = Dropdown::menu(Id::InstrumentMenu, rect.x, rect.y + rect.h, title, values, labels);
        self.chrome.widgets.dropdown = Some(menu);
    }

    /// A line of the Sound tab's instrument menus, picked in `menu`: a
    /// category opens its instruments beside it, an instrument is the track's.
    fn instrument_menu_pick(&mut self, value: &str, menu: Dropdown) {
        let sampled = self.sampled_instruments();
        if self.category_submenu_where(Id::InstrumentMenu, value, menu, |id| sampled.contains(id)) {
            return;
        }
        if let Some(id) = session_grid::picked_instrument(value) {
            self.select(Id::Instrument, id);
        }
    }

    /// A select set to `value`.
    pub(super) fn select(&mut self, id: Id, value: &str) {
        match id {
            Id::PatternList => {
                // `onPatternSelectChange`: that pattern.
                if let Ok(index) = value.parse::<usize>() {
                    self.step_pattern(index as isize - self.pattern_index as isize);
                }
                return;
            }
            Id::KitList => {
                if let Ok(index) = value.parse::<usize>() {
                    self.run(Command::Kit(index));
                }
                return;
            }
            Id::SongList => {
                if let Ok(index) = value.parse::<usize>() {
                    self.select_song(index);
                }
                return;
            }
            Id::SongMenu => return self.song_menu_pick(value),
            Id::StyleList => return self.choose_style(value),
            Id::Beats => {
                if let Ok(beats) = value.parse::<i32>() {
                    self.set_length(beats);
                }
                return;
            }
            Id::RollTrack => {
                if let Ok(t) = value.parse::<usize>() {
                    self.select_roll_track(t);
                }
                return;
            }
            Id::RollMenu => return self.roll_menu_pick(value),
            // Picked with where they are (`pick`).
            Id::GridMenu | Id::TrackNameMenu(_) | Id::KitInstrumentMenu(_) | Id::InstrumentMenu => return,
            Id::SynthPreset | Id::SynthField(_) => return self.synth_select(id, value),
            Id::KitKit => {
                let target = self.kits.kits().iter().position(|k| k.name == value);
                if let (Some(sample), Some(target)) = (self.kit_editor.sample, target)
                    && let Some(message) = self.kits.move_to_kit(self.kit_index, sample, target)
                {
                    self.kits_changed(message);
                }
                return;
            }
            Id::KitSampleInstrument(sample) => return self.kit_set_instrument(sample, value),
            _ => {}
        }
        let Some(t) = self.editor_track() else { return };
        let name = self.track_name(t);
        match id {
            Id::TrackSelect(key) => {
                let v = if editors::NUMERIC_SELECTS.contains(&key) {
                    Value::from(value).to_number().map_or_else(|| Value::from(value), number)
                } else {
                    Value::String(value.to_owned())
                };
                self.update_track(t, &[(key, v)], &format!("{} on {name}", history_label(id)), false);
            }
            Id::LfoType(i) => {
                let (mut object, lfo_key, _) = self.lfo_object(t, i, value);
                object.insert("type", Value::String(value.to_owned()));
                self.update_track(t, &[(lfo_key, Value::Object(object))], &format!("LFO type on {name}"), false);
            }
            Id::Instrument => {
                // The instrument's first sample (the current kit's first).
                if let Some((_, url, _)) = self.samples_for(value).into_iter().next()
                    && self.editor.change_track_sound(self.pattern_index, t, &url)
                {
                    self.edited();
                }
            }
            Id::Sample => {
                if !value.is_empty() && self.editor.change_track_sound(self.pattern_index, t, value) {
                    self.edited();
                }
            }
            Id::Synth => self.assign_synth(t, value),
            Id::NoteSelect(key) => {
                let Some(note) = self.shown_note(t) else { return };
                let (field, arp) = if key == "arpScale" {
                    ("arpScale", editors::arp_value(&note, &self.generators.scale_list, Some(value), None, None))
                } else {
                    ("arpDirection", editors::arp_value(&note, &self.generators.scale_list, None, Some(value), None))
                };
                let updates = [(field, Value::String(value.to_owned())), ("arp", arp)];
                self.update_note(t, &updates, &format!("Edit note {} on {name}", history_label(id)), false);
            }
            _ => {}
        }
    }

    /// A button, LED, tab or row clicked.
    pub(super) fn activate(&mut self, id: Id) {
        match id {
            Id::Tab(i) => self.editors.tab = i,
            Id::GenerationTab(i) => self.editors.generation_tab = i,
            Id::GenerationAdvanced => self.editors.generation_advanced = !self.editors.generation_advanced,
            Id::NoteTab(i) => self.editors.note_tab = i,
            Id::Lane(i) => self.editors.lane = i,
            Id::Row(_) => self.select_row(id),
            Id::LoadSample => self.open_picker(Purpose::ReplaceTrackSample),
            Id::SongButton(button) => self.song_button(button),
            Id::Transport => self.toggle_play(),
            Id::View(i) => self.show_view(i),
            Id::Mode(i) => self.show_mode(i == 1),
            Id::Master(_) => self.output_activate(id),
            Id::SynthTab(_) | Id::SynthButton(_) | Id::SynthPower(_) | Id::SynthIcon(..) | Id::SynthField(_) => {
                self.synth_activate(id);
            }
            Id::GeneratePattern => self.run(Command::GeneratePattern),
            Id::AutoAll => self.run(Command::AutoAll),
            Id::SongName | Id::SongDescription | Id::PatternDescription => self.text_entry(id),
            Id::KitText(key) => self.kit_text_entry(key),
            Id::KitAutoInstrument(sample) => self.kit_auto_instrument(sample),
            // Only a title, with a tooltip.
            Id::KitPlayedBy => {}
            _ => {
                let Some(t) = self.editor_track() else { return };
                self.activate_on_track(t, id);
            }
        }
    }

    fn activate_on_track(&mut self, t: usize, id: Id) {
        let Some(track) = self.track(t).cloned() else { return };
        let name = &track.name;
        match id {
            Id::Regenerate => self.regenerate_track(t),
            Id::FxPower(i) => {
                // `toggleFxByKey`.
                let fx = &FX_DEFS[usize::from(i).min(FX_DEFS.len() - 1)];
                let updates = if fx.key == "filterType" {
                    if editors::fx_on(&track, fx) {
                        self.editors.prev_filter_type = Some(track.filter_type.clone());
                        vec![("filterType", Value::String("allpass".into()))]
                    } else {
                        let back = self.editors.prev_filter_type.clone().unwrap_or_else(|| "lowpass".into());
                        filter_on(&track, back)
                    }
                } else {
                    let on = editors::number(&track, fx.key) > 0.0;
                    vec![(fx.key, number(if on { 0.0 } else { fx.on_value }))]
                };
                self.update_track(t, &updates, &format!("Toggle {} on {name}", fx.label), false);
            }
            Id::FilterIcon(i) => {
                let value = FILTER_TYPES[usize::from(i).min(FILTER_TYPES.len() - 1)];
                let current = track.filter_type.clone();
                let updates = if current == value {
                    vec![("filterType", Value::String("allpass".into()))]
                } else {
                    filter_on(&track, value.to_owned())
                };
                self.editors.prev_filter_type = (current != value).then_some(current);
                self.update_track(t, &updates, &format!("Filter type on {name}"), false);
            }
            Id::LfoPower(i) => {
                let (object, lfo_key, created) = self.lfo_object(t, i, "sine");
                let target = &LFO_TARGETS[usize::from(i).min(LFO_TARGETS.len() - 1)];
                let value = if created { Value::Object(object) } else { Value::Null };
                self.update_track(t, &[(lfo_key, value)], &format!("LFO {} on {name}", target.label), false);
            }
            Id::Toggle(key) => {
                let on = track.field(key).and_then(|v| v.as_bool()).unwrap_or(false);
                self.update_track(t, &[(key, Value::Bool(!on))], &format!("{} on {name}", history_label(id)), false);
            }
            Id::AutoAssign => {
                // `toggleAuto`: back on, the synth goes and the sample is picked by name.
                let on = !track.auto_assign;
                let mut updates = vec![("autoAssign", Value::Bool(on))];
                if on {
                    updates.push(("useSynth", Value::Bool(false)));
                    updates.push(("synthPreset", Value::Null));
                }
                self.update_track(t, &updates, &format!("Auto-assign on {name}"), false);
            }
            _ => {}
        }
    }

    /// The load button's file: the track's sample replaced.
    pub(super) fn replace_track_sample(&mut self, path: &Path) {
        let Some(t) = self.editor_track() else { return };
        let Some((kit, sample)) = self.track_sample_path(t).and_then(|u| self.kits.find_path(&u)) else {
            self.message = "this track plays no sample of the kits".into();
            return;
        };
        match self.kits.replace_sample(kit, sample, path) {
            Ok(message) => {
                self.track_analysis.borrow_mut().clear();
                self.kits_changed(message);
            }
            Err(e) => self.message = e,
        }
    }
}
