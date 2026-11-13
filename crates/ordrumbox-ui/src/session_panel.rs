//! The session's side of the editors' widgets: what the track editor, the
//! note editor, the kit editor's controls and the grid's volume sliders show
//! (built at every frame from the library, the kits and the player), and
//! what a click, a drag, the wheel, a key or a typed number on them does —
//! the JS handlers of `track_editor/*.js`, `note_editor/sync_section.js`,
//! `drumkit_manager.js` and the pattern panel's volume sliders.
//!
//! This file lays the widgets out; `session_panel_views.rs` builds what they
//! show, `session_panel_input.rs` takes the mouse and the keys, and
//! `session_panel_edit.rs` makes the edits.

use crate::scale::Scale;
use ordrumbox_core::json::Value;
use ordrumbox_core::style::{self, Macros};
use ordrumbox_core::{num, song};

use super::Session;
use super::session_labels::style_label;
use super::session_types::Workspace;
use crate::control::Spec;
use crate::editors::{
    self, FX_DEFS, FX_KNOBS, GENERATION_COMPOSE, GENERATION_ENGINE, GENERATION_GROOVE, GENERATION_MANUAL,
    GenerationRow, KIT_KNOBS, KNOB_BAR, LOOP_PROPS, NOTE_GROUPS, NOTE_KNOBS, NoteRow,
};
use crate::frame;
use crate::panel::Id;
use crate::panel::{self, Item, Kind, Rect};
use crate::view;
use crate::view::{KitLayout, Layout};
use ordrumbox_core::{Pattern, Track};
use ordrumbox_logic::generate;
use std::time::Duration;

/// The ⟳ button's label: the pattern made again.
const GENERATE_LABEL: &str = "\u{21bb}";

/// Two clicks on a widget this close are a double-click.
pub(super) const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// The editors' widgets on screen.
#[derive(Debug, Default)]
pub struct PanelLayout {
    pub items: Vec<Item>,
    /// Width the track editor takes on the right (0: not shown).
    pub right: i32,
    /// Where the kit editor's sample details start, under its controls.
    pub details_top: i32,
}

/// The keys the widgets read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelKey {
    Up,
    Down,
    Left,
    Right,
    Minus,
    Plus,
    Enter,
    Space,
    Escape,
    Tab,
    PageUp,
    PageDown,
    Home,
    End,
    Backspace,
}

/// What the note editor edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoteTarget {
    /// Note `n` of the track.
    Note(usize),
    /// A step without a note.
    Empty { beat: i32, step: i32 },
    /// The default note shown for a track without notes.
    Detached,
}

/// Width of the track editor on a `width`-wide window.
fn panel_width(width: i32, scale: Scale) -> i32 {
    (360 * scale).min(width / 2)
}

pub(super) fn number(v: f64) -> Value {
    Value::Number(v)
}

impl Session {
    // ─── What is shown ───────────────────────────────────────────────────────

    /// The track the track editor edits: the focused one, else the first.
    pub(super) fn editor_track(&self) -> Option<usize> {
        self.focus_track().or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| 0))
    }

    /// The widgets of a `width` × `height` window.
    pub(super) fn panel_layout(&self, width: i32, height: i32, scale: Scale) -> PanelLayout {
        let mut layout = PanelLayout::default();
        let s = scale;
        // The header's pattern and kit lists (`rebuildPatternSelect`, `rebuildDrumkitSelect`).
        let (pattern_rect, kit_rect) = {
            let c = self.header_controls(scale);
            (c.pattern, c.kit)
        };
        let library = self.editor.library();
        if !library.patterns.is_empty() {
            let labels = library
                .patterns
                .iter()
                .enumerate()
                .map(|(i, p)| p.name.clone().unwrap_or_else(|| format!("Pattern {i}")))
                .collect();
            let values = (0..library.patterns.len()).map(|i| i.to_string()).collect();
            let selected = self.pattern_index.min(library.patterns.len() - 1);
            layout.items.push(Item::new(
                Some(Id::PatternList),
                pattern_rect,
                Kind::Select { values, labels, selected },
            ));
        }
        if !self.kits.kits.is_empty() {
            let labels = self.kits.kits.iter().map(|k| k.name.clone()).collect();
            let values = (0..self.kits.kits.len()).map(|i| i.to_string()).collect();
            let selected = self.kit_index.min(self.kits.kits.len() - 1);
            layout.items.push(Item::new(Some(Id::KitList), kit_rect, Kind::Select { values, labels, selected }));
        }
        if let Some(pattern) = self.pattern() {
            self.header_items(&mut layout, pattern, scale);
        }
        // The view buttons (the main editor, then the kit editor): the one shown lit.
        let shown = [Workspace::Grid, Workspace::PianoRoll, Workspace::Song, Workspace::Synth];
        let views = self.header_controls(scale).views;
        let kit_open = self.kit_editor.is_some();
        for (i, (rect, label)) in views.iter().zip(view::VIEWS).enumerate() {
            // The song's slot has no button: the mode switch is its own.
            if shown.get(i) == Some(&Workspace::Song) {
                continue;
            }
            let on = if i == 4 { kit_open } else { !kit_open && self.workspace == shown[i] };
            layout.items.push(Item::new(Some(Id::View(i as u8)), *rect, Kind::Button { label: label.into(), on }));
        }
        // The window split: the main editor top-left, the track editor down the
        // right, the output strip along the bottom (always shown).
        let right = self.track_editor_width(width, scale);
        let body = self.body((width, height, scale));
        layout.right = right;
        // The track editor, always down the right edge (when shown).
        if right > 0 {
            let top = frame::body_top(scale);
            let bottom = body.main_bottom;
            let back = Rect::new(width - right, top - 4 * s, right, bottom - top + 8 * s);
            layout.items.push(Item::new(None, back, Kind::Panel));
            let area = Rect::new(back.x + 10 * s, top, back.w - 20 * s, bottom - top);
            // Each editor in a card of its own, from `from` to `to`, a few
            // pixels around its rows.
            let card = |from: i32, to: i32| {
                let (x, w) = (back.x + 4 * s, back.w - 8 * s);
                Item::new(None, Rect::new(x, from - 3 * s, w, to - from + 5 * s), Kind::Card)
            };
            match self.editor_track().and_then(|t| self.track_view(t)) {
                // Two cards: the sound (sample bar, knobs), then the tabs.
                Some(view) => {
                    let editor = editors::track_editor(&view, &self.editors, area, scale);
                    layout.items.push(card(top, editor.sound_bottom));
                    layout.items.push(card(editor.tabs_top, bottom));
                    layout.items.extend(editor.items);
                }
                None => {
                    layout.items.push(card(top, bottom));
                    layout.items.push(Item::text(area.x, area.y, "No track", panel::dim(), s));
                }
            }
        }
        // The note editor, under the grid or the piano roll, in a card of its own.
        if body.note.h > 0 {
            let r = body.note;
            let pad = 6 * s;
            let back = Rect::new(r.x - pad, r.y - 4 * s, r.w + 2 * pad, r.h + 6 * s);
            layout.items.push(Item::new(None, back, Kind::Card));
            match self.editor_track() {
                Some(t) => layout.items.extend(editors::note_editor(&self.note_view(t), &self.editors, r, scale)),
                None => layout.items.push(Item::text(r.x, r.y, "No note", panel::dim(), s)),
            }
        }
        // The output strip along the bottom, in the app only, unless folded.
        if self.output_shown() {
            self.output_items(&mut layout, width, body.output, scale);
        }
        // The main editor (top-left): exactly one of the kit editor, the song
        // panel, the piano roll, the synth editor, or the grid.
        if kit_open {
            let kit_layout = KitLayout::new(body.main_w, body.editor_h, scale, self.status.len());
            let area = kit_layout.controls_area();
            layout.details_top = area.y;
            layout.items.extend(self.kit_items(&kit_layout, scale));
            if let Some(view) = self.kit_controls() {
                let (items, bottom) = editors::kit_controls(&view, area, scale);
                layout.items.extend(items);
                layout.details_top = bottom;
            }
        } else if self.workspace == Workspace::Song {
            self.song_items(&mut layout, width, height, scale);
        } else if self.workspace == Workspace::PianoRoll {
            self.roll_items(&mut layout, (width, height, scale));
        } else if self.workspace == Workspace::Synth {
            self.synth_items(&mut layout, (width, height, scale));
        } else if let Some(pattern) = self.pattern() {
            let grid =
                Layout::new(body.main_w, body.editor_h, scale, pattern, self.status.len(), self.chrome.current_page);
            for (i, (_, label)) in editors::LANES.iter().enumerate() {
                let kind = Kind::Tab { label: (*label).to_owned(), active: usize::from(self.editors.lane) == i };
                layout.items.push(Item::new(Some(Id::Lane(i as u8)), grid.lane_tab_rect(i), kind));
            }
            let spec = Spec::slider(0.0, 1.0, 0.01, 1.0);
            for (t, track) in pattern.tracks.iter().enumerate().take(grid.tracks()) {
                let kind = Kind::Value { spec, value: track.volume, label: None, text: String::new(), lfo: false };
                layout.items.push(Item::new(Some(Id::TrackVolume(t)), grid.volume_rect(t), kind));
            }
        }
        layout
    }

    /// The header's controls: a plugin's without the song view button.
    pub(super) fn header_controls(&self, scale: Scale) -> view::HeaderControls {
        let controls = view::header_controls(scale);
        if self.host.hosted { controls.hosted() } else { controls }
    }

    /// The header's transport and generation controls (the toolbar's
    /// transport and the pattern settings panel of the JS).
    fn header_items(&self, layout: &mut PanelLayout, pattern: &Pattern, scale: Scale) {
        let controls = self.header_controls(scale);
        // A plugin has no song (no mode switch) and no transport: the host plays, at its own tempo. The
        // view shows the host's there (`view::Transport::Host`).
        if !self.host.hosted {
            let song = self.workspace == Workspace::Song;
            for (i, (rect, label)) in controls.mode.iter().zip(view::MODES).enumerate() {
                let kind = Kind::Button { label: label.into(), on: song == (i == 1) };
                layout.items.push(Item::new(Some(Id::Mode(i as u8)), *rect, kind));
            }
            let playing = self.playing();
            let label = if playing { "STOP" } else { "PLAY" };
            layout.items.push(Item::new(
                Some(Id::Transport),
                controls.play,
                Kind::Button { label: label.into(), on: playing },
            ));
            let spec = Spec::knob(song::MIN_BPM, song::MAX_BPM, 1.0, song::DEFAULT_BPM);
            let bpm = self.tempo();
            let text = num::text(bpm);
            let kind = Kind::Value { spec, value: bpm, label: None, text, lfo: false };
            layout.items.push(Item::new(Some(Id::Bpm), controls.bpm, kind));
        }
        let beats: Vec<String> = (1..=ordrumbox_logic::editor::MAX_BEATS).map(|b| b.to_string()).collect();
        let selected = usize::try_from(pattern.beat_count - 1).unwrap_or(0).min(beats.len() - 1);
        let kind = Kind::Select { values: beats.clone(), labels: beats, selected };
        layout.items.push(Item::new(Some(Id::Beats), controls.beats, kind));
        // The generation: the pattern's style, ⟳, AUTO, the macros.
        let styles = self.shown_styles();
        let genre = pattern.extra.get(generate::GENRE).and_then(|g| g.as_str()).unwrap_or_default();
        let values: Vec<String> =
            std::iter::once(String::new()).chain(styles.list.iter().map(|s| s.name.clone())).collect();
        let labels: Vec<String> =
            std::iter::once("random".to_owned()).chain(styles.list.iter().map(style_label)).collect();
        let selected = styles.get(genre).and_then(|st| values.iter().position(|v| *v == st.name)).unwrap_or(0);
        layout.items.push(Item::new(Some(Id::StyleList), controls.style, Kind::Select { values, labels, selected }));
        let button = Kind::Button { label: GENERATE_LABEL.into(), on: false };
        layout.items.push(Item::new(Some(Id::GeneratePattern), controls.generate, button));
        let button = Kind::Button { label: "AUTO".into(), on: self.pattern_auto() };
        layout.items.push(Item::new(Some(Id::AutoAll), controls.auto, button));
        let macros = Macros::of(pattern);
        for (rect, (key, _)) in controls.macros.iter().zip(view::MACROS) {
            let value = match key {
                style::ENERGY => macros.energy,
                style::COMPLEXITY => macros.complexity,
                _ => macros.variation,
            };
            let spec = Spec::knob(0.0, 100.0, 1.0, style::MACRO_NEUTRAL);
            let kind = Kind::Value { spec, value, label: None, text: num::text(value), lfo: false };
            layout.items.push(Item::new(Some(Id::Macro(key)), *rect, kind));
        }
    }

    /// Width the track editor takes on the right of a `width`-wide window (0:
    /// none). It is shown for every main editor (grid, piano roll, song, synth,
    /// the kit editor) whenever it is toggled on and the window is wide enough.
    pub fn track_editor_width(&self, width: i32, scale: Scale) -> i32 {
        if self.chrome.show_track_editor && width >= 480 * scale { panel_width(width, scale) } else { 0 }
    }

    /// A drop-down list or a number entry is open (it takes every key and click).
    pub fn popup_open(&self) -> bool {
        self.chrome.popup_open()
    }

    /// A number or text entry is open. It is the one place that keeps Space:
    /// a text entry types a space in it.
    pub fn entry_open(&self) -> bool {
        self.chrome.widgets.entry.is_some()
    }

    /// The track editor on its Generate tab.
    pub fn show_generation_tab(&mut self) {
        self.chrome.show_track_editor = true;
        self.editors.tab = editors::GEN_TAB;
    }
}

/// Label and unit of a control, for the number entry's title.
pub(super) fn control_name(id: Id) -> (&'static str, &'static str) {
    let prop = |list: &[editors::Prop], key: &str| list.iter().find(|p| p.key == key).map_or("", |p| p.label);
    match id {
        Id::KnobBar(key) => KNOB_BAR.iter().find(|(p, _)| p.key == key).map_or(("", ""), |(p, u)| (p.label, *u)),
        Id::Decay => ("Decay", ""),
        Id::Bpm => ("BPM", ""),
        Id::Master(key) => crate::output::find(key).map_or(("", ""), |p| (p.label, p.unit)),
        Id::SynthField(_) => ("value", ""),
        Id::Fx(key) => (prop(&FX_KNOBS, key), ""),
        Id::Generation(key) => {
            let rows = GENERATION_GROOVE.iter().chain(GENERATION_ENGINE.iter());
            let label = rows
                .filter_map(|r| match r {
                    GenerationRow::Slider(p) if p.key == key => Some(p.label),
                    _ => None,
                })
                .next()
                .unwrap_or_else(|| prop(&GENERATION_MANUAL, key));
            (label, "")
        }
        Id::Compose(key) => (prop(&GENERATION_COMPOSE, key), ""),
        Id::Loop(key) => (prop(&LOOP_PROPS, key), ""),
        Id::Lfo(_, param) => (param.label(), ""),
        Id::NoteKnob(key) => NOTE_KNOBS.iter().find(|(p, _)| p.key == key).map_or(("", ""), |(p, u)| (p.label, *u)),
        Id::NoteSlider(key) => {
            let label = NOTE_GROUPS
                .iter()
                .flat_map(|g| g.iter())
                .find_map(|r| match r {
                    NoteRow::Slider(p) if p.key == key => Some(p.label),
                    _ => None,
                })
                .unwrap_or("");
            (label, "")
        }
        Id::KitKnob(field) => {
            KIT_KNOBS.iter().find(|(f, ..)| *f == field).map_or(("", ""), |(_, p, u, _)| (p.label, *u))
        }
        Id::TrackSelect("autoVariant") => ("Variant", ""),
        Id::TrackSelect(key) => (editors::fx_label(key), ""),
        Id::Toggle("mono") => ("Mono", ""),
        Id::Toggle("autoMode") => ("Auto", ""),
        Id::Toggle("composeLock") => ("Lock", ""),
        Id::Toggle(key) => {
            let mut rows = GENERATION_GROOVE.iter().chain(GENERATION_ENGINE.iter());
            let label = rows.find_map(|r| match r {
                GenerationRow::Toggle(k, label) if *k == key => Some(*label),
                _ => None,
            });
            (label.unwrap_or(""), "")
        }
        Id::NoteSelect(key) => {
            let label = NOTE_GROUPS.iter().flat_map(|g| g.iter()).find_map(|r| match r {
                NoteRow::Select(k, label) if *k == key => Some(*label),
                _ => None,
            });
            (label.unwrap_or(""), "")
        }
        Id::TrackVolume(_) => ("Volume", ""),
        _ => ("", ""),
    }
}

/// The edits that set the filter of `track` to `filter_type`: from the bypass,
/// a cutoff at its 20 Hz default moves to [`editors::FILTER_ON_CUTOFF`].
pub(super) fn filter_on(track: &Track, filter_type: String) -> Vec<(&'static str, Value)> {
    let mut updates = vec![("filterType", Value::String(filter_type))];
    if track.filter_type == "allpass" && track.filter_cutoff <= Track::default().filter_cutoff {
        updates.push(("filterCutoff", number(editors::FILTER_ON_CUTOFF)));
    }
    updates
}

/// What an edit of control `id` is called in the undo history: its label,
/// after its effect's for an effect setting (`Dly Mix`), else its key.
pub(super) fn history_label(id: Id) -> String {
    let key = match id {
        Id::Fx(key)
        | Id::TrackSelect(key)
        | Id::Generation(key)
        | Id::Compose(key)
        | Id::Loop(key)
        | Id::Toggle(key) => key,
        Id::NoteSlider(key) | Id::NoteKnob(key) | Id::NoteSelect(key) => key,
        _ => "",
    };
    let label = Some(control_name(id).0).filter(|l| !l.is_empty()).unwrap_or(key);
    match FX_DEFS.iter().find(|fx| fx.controls.contains(&key)) {
        Some(fx) => format!("{} {label}", fx.label),
        None => label.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::OverlayKey;
    use super::*;
    use crate::canvas::Canvas;
    use crate::editors::STYLE_DEFAULT;
    use crate::kits::KitSet;
    use crate::menu::Command;
    use crate::ui_input::Button;
    use ordrumbox_core::{Library, Styles};
    use ordrumbox_engine::SynthPresets;

    #[test]
    fn history_names_an_edit_by_its_label() {
        assert_eq!(history_label(Id::Fx("delayMix")), "Delay Mix");
        assert_eq!(history_label(Id::TrackSelect("delayBeats")), "Delay Beats");
        assert_eq!(history_label(Id::TrackSelect("autoVariant")), "Variant");
        assert_eq!(history_label(Id::Generation("autoPitchChance")), "Pitch");
        assert_eq!(history_label(Id::Generation("hitVariation")), "Hit Variation");
        assert_eq!(history_label(Id::Compose("composeSyncopation")), "Syncopation");
        assert_eq!(history_label(Id::Loop("loopLength")), "Loop Length");
        assert_eq!(history_label(Id::Toggle("pitchScaleLock")), "Scale Lock");
        assert_eq!(history_label(Id::NoteSlider("euclideanPulses")), "Pulses");
        assert_eq!(history_label(Id::NoteSelect("arpDirection")), "Direction");
        assert_eq!(history_label(Id::NoteKnob("velocity")), "Velocity");
    }

    use crate::test_support::{SIZE, click};

    #[test]
    fn the_mouse_over_a_tab_or_a_knob_shows_what_it_is() {
        let mut s = session();
        let (width, height, scale) = SIZE;
        let center = |s: &Session, id: Id| {
            let item = panel::find(&s.panel_layout(width, height, scale).items, id).cloned().expect("shown");
            (item.rect.x + item.rect.w / 2, item.rect.y + item.rect.h / 2)
        };
        let (x, y) = center(&s, Id::Tab(editors::TAB_LOOP));
        assert!(s.control_hover(SIZE, x, y));
        assert_eq!(s.chrome.control_hover.map(|(id, _)| id), Some(Id::Tab(editors::TAB_LOOP)));
        assert!(!s.control_hover(SIZE, x, y), "unchanged");
        let (x, y) = center(&s, Id::KnobBar("transpose"));
        assert!(s.control_hover(SIZE, x, y));
        let mut pixels = vec![0; (width * height) as usize];
        s.draw(&mut Canvas::new(&mut pixels, width as usize, height as usize), scale);
        // Off every control: no tooltip.
        assert!(s.control_hover(SIZE, 1, height - 1));
        assert_eq!(s.chrome.control_hover, None);
    }

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","bpm":100,"beatCount":4,"tracks":[{"name":"KICK","notes":[{}]},{"name":"SNARE"}]}]}"#,
        )
        .unwrap();
        let mut session = Session::new(library, KitSet::empty(), Vec::new());
        session.set_size(SIZE);
        session
    }

    fn center(r: Rect) -> (i32, i32) {
        (r.x + r.w / 2, r.y + r.h / 2)
    }

    #[test]
    fn h_j_k_change_every_track_in_one_undo_step() {
        let mut s = session();
        // No synth patches, no samples: nothing changes.
        s.run(Command::ConvertToSynth);
        assert_eq!(s.message, "No synth patches loaded");
        s.run(Command::RandomSamples);
        assert_eq!(s.message, "No samples loaded");
        assert!(s.editor.history().next_undo().is_none());

        let presets = ordrumbox_core::json::parse(r#"{"BASS0":{},"KICK_SYNTH":{},"SNARE_SYNTH":{}}"#).unwrap();
        s.set_asset_synths(SynthPresets::from_json(&presets).unwrap());
        s.run(Command::ConvertToSynth);
        let tracks = &s.pattern().unwrap().tracks;
        assert!(tracks.iter().all(|t| t.use_synth && !t.auto_assign));
        let keys: Vec<_> = tracks.iter().map(|t| t.synth_preset.as_deref()).collect();
        assert_eq!(keys, [Some("KICK_SYNTH"), Some("SNARE_SYNTH")]);
        assert_eq!(s.message, "All tracks converted to generated sounds");
        assert_eq!(s.editor.history().next_undo(), Some("Convert tracks to generated sounds"));

        s.run(Command::AutoAssignAll);
        let tracks = &s.pattern().unwrap().tracks;
        assert!(tracks.iter().all(|t| !t.use_synth && t.auto_assign));
        // One undo brings both tracks back to their synth.
        s.run(Command::Undo);
        assert!(s.pattern().unwrap().tracks.iter().all(|t| t.use_synth));
    }

    #[test]
    fn a_synth_track_shows_none_as_its_sample() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"BASS","useSynth":true,"synthPreset":"b"},{"name":"KICK"}]}]}"#,
        )
        .unwrap();
        let s = Session::new(library, KitSet::empty(), Vec::new());
        let shown = |t| s.track_view(t).map(|v| v.samples.labels[v.samples.selected].clone());
        assert_eq!(shown(0).as_deref(), Some("none"));
        assert_eq!(shown(1).as_deref(), Some("— no samples —"));
    }

    #[test]
    fn every_track_editor_tab_fits_the_smallest_window() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","beatCount":4,"tracks":[{"name":"KICK","notes":[{}]},{"name":"SNARE","autoMode":true}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, KitSet::empty(), Vec::new());
        s.status = vec!["a status line".into()];
        let mut tallest = 0;
        for scale in (Scale::ONE.quarters()..=Scale::MAX.quarters()).map(Scale::from_quarters) {
            let (width, height) = view::min_window_size(scale, view::Shown::all(true));
            let body = view::Body::new(width, height, scale, s.track_editor_width(width, scale), 1, true, true);
            for (track, tab, sub, note_tab, advanced) in (0..2).flat_map(|t| {
                (0..6u8).flat_map(move |tab| {
                    (0..5u8).flat_map(move |sub| (0..5u8).flat_map(move |n| [false, true].map(|a| (t, tab, sub, n, a))))
                })
            }) {
                s.editors.tab = tab;
                s.editors.generation_tab = sub;
                s.editors.note_tab = note_tab;
                s.editors.generation_advanced = advanced;
                s.selection.cursor = Some(view::Cursor { track, beat: 0, step: 0 });
                let layout = s.panel_layout(width, height, scale);
                let bottom = (layout.items.iter())
                    .filter(|i| i.rect.x >= width - layout.right && i.rect.y < body.output.y)
                    .filter(|i| !matches!(i.kind, Kind::Panel | Kind::Card))
                    .map(|i| i.rect.y + i.rect.h)
                    .max()
                    .unwrap();
                assert!(bottom <= body.main_bottom, "scale {scale} tab {tab}/{sub} note tab {note_tab}: {bottom}");
                if scale == Scale::ONE {
                    tallest = tallest.max(bottom - frame::body_top(scale));
                }
                // The note editor, under the grid: inside its strip.
                let strip = body.note;
                for item in layout.items.iter().filter(|i| i.rect.x < width - layout.right && i.rect.y >= strip.y) {
                    if matches!(item.kind, Kind::Panel | Kind::Card) || item.rect.y >= body.output.y {
                        continue;
                    }
                    let r = item.rect;
                    assert!(r.y + r.h <= body.main_bottom, "scale {scale} note tab {note_tab}: {r:?}");
                    assert!(r.x + r.w <= strip.x + strip.w, "scale {scale} note tab {note_tab}: {r:?}");
                }
            }
            let views = view::header_controls(scale).views;
            assert!(views[4].x + views[4].w <= width);
        }
        // The constant is the tallest tab, not more: the window is no taller than it needs.
        assert_eq!(tallest, view::TRACK_EDITOR_HEIGHT);
    }

    #[test]
    fn the_mode_switch_goes_between_the_pattern_and_the_song() {
        let mut s = session();
        let c = view::header_controls(Scale::ONE);
        let lit = |s: &Session| {
            let items = s.panel_layout(SIZE.0, SIZE.1, SIZE.2).items;
            [0, 1].map(|i| match panel::find(&items, Id::Mode(i)).map(|item| &item.kind) {
                Some(Kind::Button { on, .. }) => *on,
                other => panic!("mode {i}: {other:?}"),
            })
        };
        assert_eq!(lit(&s), [true, false]);
        click(&mut s, c.mode[1].x + 2, c.mode[1].y + 2);
        s.mouse_up();
        assert!(s.song_view_open());
        assert_eq!(lit(&s), [false, true]);
        // Again: nothing changes. The pattern's button brings the grid back.
        click(&mut s, c.mode[1].x + 2, c.mode[1].y + 2);
        s.mouse_up();
        assert!(s.song_view_open());
        click(&mut s, c.mode[0].x + 2, c.mode[0].y + 2);
        s.mouse_up();
        assert!(!s.song_view_open());
        assert_eq!(lit(&s), [true, false]);
        // The view buttons leave the song too, and the switch follows.
        s.run(crate::menu::Command::SongView);
        assert_eq!(lit(&s), [false, true]);
        s.run(crate::menu::Command::SongView);
        assert_eq!(lit(&s), [true, false]);
    }

    #[test]
    fn the_tempo_knob_is_the_songs_in_the_song_panel_else_the_sessions() {
        let mut s = session();
        let (x, y) = center(view::header_controls(Scale::ONE).bpm);
        s.run(crate::menu::Command::SongView);
        s.run(crate::menu::Command::NewSong);
        s.chrome.widgets.entry.as_mut().unwrap().text = "verse".into();
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert!(s.song_view_open());
        let song_bpm = s.song().unwrap().bpm;
        assert!(s.wheel(SIZE, x, y, 2, false, false));
        assert_eq!((s.song().unwrap().bpm, s.editor.library().bpm), (song_bpm + 2.0, 100.0));
        s.run(crate::menu::Command::SongView);
        assert!(!s.song_view_open());
        assert!(s.wheel(SIZE, x, y, 3, false, false));
        assert_eq!((s.song().unwrap().bpm, s.editor.library().bpm), (song_bpm + 2.0, 103.0));
    }

    #[test]
    fn header_controls_set_tempo_length_and_generation() {
        let mut s = session();
        let c = view::header_controls(Scale::ONE);
        // The tempo knob: the wheel steps it, a typed number sets it (its
        // right-click menu's "Type a value...").
        let (x, y) = center(c.bpm);
        assert!(s.wheel(SIZE, x, y, 3, false, false));
        assert_eq!(s.editor.library().bpm, 103.0);
        s.mouse_down(SIZE, x, y, Button::Right, false);
        s.pick(0);
        s.chrome.widgets.entry.as_mut().unwrap().text = "140".into();
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.editor.library().bpm, 140.0);
        // The beats list: 8.
        let (x, y) = center(c.beats);
        click(&mut s, x, y);
        for _ in 0..4 {
            s.panel_key(SIZE, PanelKey::Down, false, false);
        }
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.pattern().unwrap().beat_count, 8);
        // AUTO: every track in auto mode, the button lit.
        let (x, y) = center(c.auto);
        click(&mut s, x, y);
        assert!(s.pattern().unwrap().tracks.iter().all(|t| t.auto_mode));
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        assert!(matches!(panel::find(&layout.items, Id::AutoAll).unwrap().kind, Kind::Button { on: true, .. }));
    }

    #[test]
    fn a_pattern_in_the_chosen_style_and_its_drum_track_composed_from_the_gen_tab() {
        let mut s = session();
        let text = include_str!("../../../assets/data/styles.json");
        s.generators.styles = Styles::from_json(&ordrumbox_core::json::parse(text).unwrap()).0;
        s.update();
        // The header's style list: the pattern made again in reggae.
        s.select(Id::StyleList, "reggae");
        assert_eq!(s.message, "generated in reggae");
        let pattern = s.pattern().unwrap();
        assert_eq!(pattern.extra.get(generate::GENRE).and_then(|g| g.as_str()), Some("reggae"));
        assert_eq!(pattern.tracks[0].name, "KICK");
        assert_eq!(pattern.tracks[0].notes.iter().map(|n| (n.beat, n.step)).collect::<Vec<_>>(), [(2, 0)]);
        assert!(pattern.tracks.iter().all(|t| t.name != "TOM"));

        // The Generate tab of the KICK: the style's hits, then the track's own.
        s.editors.tab = editors::GEN_TAB;
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        let hits = panel::find(&layout.items, Id::Compose("composeHitsPerMeasure")).unwrap();
        assert!(matches!(&hits.kind, Kind::Value { value, text, .. } if *value == 1.0 && text == "1*"));
        s.set_value(Id::Compose("composeHitsPerMeasure"), 3.0);
        assert_eq!(s.track(0).unwrap().compose_hits_per_measure, Some(3.0));
        assert_eq!(s.editor.history().next_undo(), Some("Measure Hits on KICK"));
        // ↻: a new line with three kicks, the third beat among them.
        let regenerate = panel::find(&s.panel_layout(SIZE.0, SIZE.1, Scale::ONE).items, Id::Regenerate).unwrap().rect;
        let (x, y) = center(regenerate);
        click(&mut s, x, y);
        let kick = s.track(0).unwrap();
        assert_eq!(kick.notes.len(), 3);
        assert!(kick.notes.iter().any(|n| (n.beat, n.step) == (2, 0)));
        assert!(kick.notes.iter().all(|n| (n.beat, n.step) != (0, 0)), "one drop: never the downbeat");
        // A double-click gives the setting back to the style.
        s.set_value(Id::Compose("composeHitsPerMeasure"), STYLE_DEFAULT);
        assert_eq!(s.track(0).unwrap().compose_hits_per_measure, None);

        // The ENERGY knob: the one-drop's single kick played twice, its third beat kept.
        s.set_value(Id::Macro(style::ENERGY), 100.0);
        assert_eq!(s.editor.history().next_undo(), Some("Set energy → 100"));
        let kick = s.track(0).unwrap();
        assert_eq!(kick.notes.len(), 2);
        assert!(kick.notes.iter().any(|n| (n.beat, n.step) == (2, 0)));
        let layout = s.panel_layout(SIZE.0, SIZE.1, Scale::ONE);
        let hits = panel::find(&layout.items, Id::Compose("composeHitsPerMeasure")).unwrap();
        assert!(matches!(&hits.kind, Kind::Value { text, .. } if text == "1\u{2192}2*"), "{:?}", hits.kind);
        // The same knob value makes the same lines.
        let lines = s.pattern().unwrap().tracks.clone();
        s.set_value(Id::Macro(style::ENERGY), 60.0);
        s.set_value(Id::Macro(style::ENERGY), 100.0);
        assert_eq!(s.pattern().unwrap().tracks, lines);

        // A locked KICK stays through ⟳; the other tracks get new lines.
        s.run(Command::LockTrack);
        assert!(s.track(0).unwrap().compose_lock);
        let kick = s.track(0).unwrap().clone();
        let (x, y) =
            center(panel::find(&s.panel_layout(SIZE.0, SIZE.1, Scale::ONE).items, Id::GeneratePattern).unwrap().rect);
        click(&mut s, x, y);
        assert_eq!(s.pattern().unwrap().tracks.iter().find(|t| t.name == "KICK"), Some(&kick));
        assert_eq!(s.editor.history().next_undo(), Some("Generate \"p\""));
    }

    #[test]
    fn the_grid_names_what_each_track_plays() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"tracks":[{"name":"KICK"},{"name":"BASS","useSynth":true,"synthPreset":"BASS5"},
                {"name":"X","autoAssign":false,"samplePath":"real/x.wav"}]}]}"#,
        )
        .unwrap();
        let s = Session::new(library, KitSet::empty(), Vec::new());
        // No samples loaded: nothing for the kick, the preset, the sample set by hand.
        let labels: Vec<String> = (0..3).map(|t| s.track_sound_label(t)).collect();
        assert_eq!(labels, ["no sound", "synth BASS5", "real/x.wav"]);
        let lines: Vec<(String, String)> = (0..3).map(|t| s.track_sound_lines(t)).map(|l| (l.source, l.name)).collect();
        let owned = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(lines, [owned("", "no sound"), owned("synth", "BASS5"), owned("real", "x")]);
    }

    #[test]
    fn the_pattern_menu_renames_and_describes() {
        let mut s = session();
        s.run(Command::RenamePattern);
        s.chrome.widgets.entry.as_mut().unwrap().text = " Verse ".into();
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.pattern().unwrap().name.as_deref(), Some("Verse"));
        s.run(Command::PatternDescription);
        assert_eq!(s.chrome.widgets.entry.as_ref().map(|e| e.text.as_str()), Some(""));
        s.panel_text('x');
        s.panel_key(SIZE, PanelKey::Space, false, false);
        s.panel_text('y');
        s.panel_key(SIZE, PanelKey::Enter, false, false);
        assert_eq!(s.pattern().unwrap().description, "x y");
        s.undo();
        assert_eq!(s.pattern().unwrap().description, "");
    }

    #[test]
    fn the_output_strip_and_the_track_editor_stay_with_every_main_editor() {
        /// The strip's knobs are there, all inside the bottom band.
        fn strip(band: Rect, layout: &PanelLayout) {
            let items: Vec<&Item> = layout.items.iter().filter(|i| matches!(i.id, Some(Id::Master(_)))).collect();
            assert!(items.len() >= 2, "the strip's controls are missing");
            for item in items {
                assert!(
                    item.rect.y >= band.y && item.rect.y + item.rect.h <= band.y + band.h,
                    "a strip control is outside the band"
                );
            }
        }
        let mut s = session();
        // The grid: its volume sliders, the strip and the track editor together.
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        strip(s.body(SIZE).output, &layout);
        assert!(layout.items.iter().any(|i| matches!(i.id, Some(Id::TrackVolume(_)))));
        assert!(layout.right > 0);
        // The song panel keeps both.
        s.toggle_song_view();
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        strip(s.body(SIZE).output, &layout);
        assert!(layout.items.iter().any(|i| matches!(i.id, Some(Id::SongName))));
        assert!(layout.right > 0);
        // The kit editor opens over the main editor; the strip stays below.
        s.toggle_song_view();
        s.toggle_kit_editor();
        let layout = s.panel_layout(SIZE.0, SIZE.1, SIZE.2);
        strip(s.body(SIZE).output, &layout);
        assert!(layout.right > 0);
    }

    #[test]
    fn factory_reset_opens_a_confirmation_overlay() {
        let mut s = session();
        assert!(!s.overlay_open());
        s.run(Command::FactoryReset);
        assert!(s.overlay_open());
        // Esc cancels without touching anything.
        s.overlay_key(OverlayKey::Escape);
        assert!(!s.overlay_open());
    }
}
