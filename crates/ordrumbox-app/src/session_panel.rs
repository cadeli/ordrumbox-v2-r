//! The session's side of the editors' widgets: what the track editor, the
//! note editor, the kit editor's controls and the grid's volume sliders show
//! (built at every frame from the library, the kits and the player), and
//! what a click, a drag, the wheel, a key or a typed number on them does —
//! the JS handlers of `track_editor/*.js`, `note_editor/sync_section.js`,
//! `drumkit_manager.js` and the pattern panel's volume sliders.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::js;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::TrackSound;
use ordrumbox_engine::assign::assign_track;

use super::*;
use crate::control::{Spec, Style};
use crate::editors::{
    self, ARP_TYPES, Choice, FILTER_TYPES, FX_DEFS, FX_KNOBS, GEN_ENGINE, GEN_GROOVE, GEN_MANUAL, GenRow, KIT_KNOBS,
    KNOB_BAR, KitControls, LFO_TARGETS, LOOP_PROPS, NOTE_GROUPS, NOTE_KNOBS, NoteRow, NoteView, SampleBar, TrackView,
};
use crate::kits::Field;
use crate::panel::{self, Item, Kind, LfoParam, Rect};
use crate::view::{KitLayout, Layout};

/// Kit knob edits reach the player and the disk this long after the last one.
const KITS_DELAY: Duration = Duration::from_millis(200);
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

/// A mouse button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
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
enum NoteTarget {
    /// Note `n` of the track.
    Note(usize),
    /// A step without a note.
    Empty { beat: i32, step: i32 },
    /// The default note shown for a track without notes.
    Detached,
}

/// Width of the track editor on a `width`-wide window.
fn panel_width(width: i32, scale: i32) -> i32 {
    (360 * scale).min(width / 2)
}

fn num(v: f64) -> Value {
    Value::Number(v)
}

impl Session {
    // ─── What is shown ───────────────────────────────────────────────────────

    /// The track the track editor edits: the focused one, else the first.
    fn editor_track(&self) -> Option<usize> {
        self.focus_track().or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| 0))
    }

    /// The widgets of a `width` × `height` window.
    pub(super) fn panel_layout(&self, width: i32, height: i32, scale: i32) -> PanelLayout {
        let mut layout = PanelLayout::default();
        let s = scale;
        // The header's pattern and kit lists (`rebuildPatternSelect`, `rebuildDrumkitSelect`).
        let (pattern_rect, kit_rect) = view::header_selects(scale);
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
        if self.kit_editor.is_some() {
            let kit_layout = KitLayout::new(width, height, scale, self.status.len());
            let area = kit_layout.controls_area();
            layout.details_top = area.y;
            if let Some(view) = self.kit_controls() {
                let (items, bottom) = editors::kit_controls(&view, area, scale);
                layout.items.extend(items);
                layout.details_top = bottom;
            }
            return layout;
        }
        if self.song_view {
            self.song_items(&mut layout, width, height, scale);
            return layout;
        }
        let top = view::content_top(scale);
        let bottom = view::footer_top(height, scale, self.status.len()) - 8 * s;
        layout.right = self.track_editor_width(width, scale);
        if layout.right > 0 {
            let back = Rect::new(width - layout.right, top - 4 * s, layout.right, bottom - top + 8 * s);
            layout.items.push(Item::new(None, back, Kind::Highlight));
            let area = Rect::new(back.x + 10 * s, top, back.w - 20 * s, bottom - top);
            match self.editor_track().and_then(|t| Some((t, self.track_view(t)?))) {
                Some((t, view)) => {
                    let (items, y) = editors::track_editor(&view, &self.editors, area, scale);
                    layout.items.extend(items);
                    let note = self.note_view(t);
                    let area = Rect::new(area.x, y + 4 * s, area.w, bottom - y);
                    layout.items.extend(editors::note_editor(&note, &self.editors, area, scale));
                }
                None => layout.items.push(Item::text(area.x, area.y, "No track", panel::DIM, s)),
            }
        }
        if let Some(pattern) = self.pattern() {
            let grid = Layout::new(width - layout.right, height, scale, pattern, self.status.len());
            let spec = Spec::slider(0.0, 1.0, 0.01, 1.0);
            for (t, track) in pattern.tracks.iter().enumerate().take(grid.tracks()) {
                let kind = Kind::Value { spec, value: track.velocity, label: None, text: String::new(), lfo: false };
                layout.items.push(Item::new(Some(Id::TrackVolume(t)), grid.volume_rect(t), kind));
            }
            let spec = Spec::slider(0.0, 2.0, 0.01, 1.0);
            let kind = Kind::Value { spec, value: self.master_volume, label: None, text: String::new(), lfo: false };
            layout.items.push(Item::new(Some(Id::MasterVolume), grid.master_rect(), kind));
        }
        layout
    }

    /// Width the track editor takes on the right of a `width`-wide window (0: none).
    pub fn track_editor_width(&self, width: i32, scale: i32) -> i32 {
        if self.show_track_editor && self.kit_editor.is_none() && !self.song_view && width >= 480 * scale {
            panel_width(width, scale)
        } else {
            0
        }
    }

    /// A drop-down list or a number entry is open (it takes every key and click).
    pub fn popup_open(&self) -> bool {
        self.dropdown.is_some() || self.entry.is_some()
    }

    /// The url of the sample track `t` plays (none for a synth track).
    fn track_sample_url(&self, t: usize) -> Option<String> {
        let track = self.track(t)?;
        if track.use_soft_synth {
            return None;
        }
        let library = self.editor.library();
        let sound = match (&self.controller, &self.render) {
            (Some(c), _) => c.track_sound(library, self.pattern_index, t),
            (None, Some(render)) => {
                let kit = self.kits.kits.get(self.kit_index).map_or("", |k| k.name.as_str());
                assign_track(track, &render.bank, kit, &self.synths, &mut |_| 0)
            }
            (None, None) => TrackSound::None,
        };
        let bank =
            self.controller.as_ref().map(|c| c.bank().clone()).or_else(|| self.render.as_ref().map(|r| r.bank.clone()));
        match sound {
            TrackSound::Sample(i) => bank.and_then(|b| b.get(i).map(|s| s.instrument.url.clone())),
            _ => (!track.use_auto_assign_sound).then(|| track.sample_id.clone()),
        }
    }

    fn analysis_of(&self, url: &str) -> Option<Analysis> {
        if let Some(a) = self.track_analysis.borrow().get(url) {
            return Some(a.clone());
        }
        let analysis = analysis::analyze(self.kits.buffer(url)?);
        self.track_analysis.borrow_mut().insert(url.to_owned(), analysis.clone());
        Some(analysis)
    }

    fn track_view(&self, t: usize) -> Option<TrackView<'_>> {
        let track = self.track(t)?;
        let url = self.track_sample_url(t);
        let place = url.as_deref().and_then(|u| self.kits.find_url(u));
        let instrument = place.and_then(|(k, i)| self.kits.sample(k, i));
        let kit_name = place.and_then(|(k, _)| self.kits.kits.get(k)).map(|k| k.name.as_str());

        // `getSoundInfo`.
        let info = if track.use_soft_synth {
            track.synth_sound_key.clone()
        } else {
            instrument.map(|i| {
                let name = if i.display_name.is_empty() { &i.key } else { &i.display_name };
                kit_name.map_or_else(|| name.clone(), |k| format!("{k}/{name}"))
            })
        };
        let title = format!("Track: {}{}", track.name, info.map(|i| format!(" - {i}")).unwrap_or_default());

        // Sample bar: analysis text, envelope, decay end (`sound.decay ?? 0`).
        let decay = instrument.and_then(|i| i.decay).unwrap_or(0.0);
        let sample = url.as_deref().filter(|_| !track.use_soft_synth).and_then(|u| self.analysis_of(u)).map(|a| {
            let pitch = a.note.map_or_else(|| "—".to_owned(), |n| format!("{}{}", n.name, n.octave));
            let duration = format!("{} ms", crate::control::to_fixed_text(a.duration_sec * 1000.0, 0));
            let peak = format!("{} dB", crate::control::to_fixed_text(a.peak_db, 1));
            let marker = (a.duration_sec > 0.0).then(|| (decay / 1000.0 / a.duration_sec).min(1.0));
            SampleBar { info: format!("{pitch} · {duration} · {peak}"), envelope: a.envelope.clone(), marker }
        });

        // Instruments that have samples, sorted; the current one.
        let keys: std::collections::HashSet<&str> =
            self.kits.kits.iter().flat_map(|k| k.instruments.iter().map(|i| i.key.as_str())).collect();
        let mut ids: Vec<String> =
            catalog().instruments.iter().map(|i| i.id.clone()).filter(|id| keys.contains(id.as_str())).collect();
        ids.sort();
        let current_id = match instrument.map(|i| i.key.as_str()) {
            Some(key) if keys.contains(key) => key.to_owned(),
            _ if keys.contains(track.name.as_str()) => track.name.clone(),
            _ => ids.first().cloned().unwrap_or_else(|| "KICK".into()),
        };
        let instruments = Choice { selected: editors::index_of(&ids, &current_id), values: ids, labels: Vec::new() };

        // Samples of that instrument: the current kit's first.
        let current_url = url.clone().unwrap_or_else(|| track.sample_id.clone());
        let found = self.samples_for(&current_id);
        let samples = if found.is_empty() {
            Choice { values: vec![String::new()], labels: vec!["— no samples —".into()], selected: 0 }
        } else {
            let values: Vec<String> = found.iter().map(|(_, url, _)| url.clone()).collect();
            let labels = found.iter().map(|(kit, _, name)| format!("{kit}/{name}")).collect();
            Choice { selected: editors::index_of(&values, &current_url), values, labels }
        };

        // Synth presets.
        let keys = self.synths.keys();
        let current = if track.use_soft_synth {
            track.synth_sound_key.clone().unwrap_or_else(|| "BASS1".into())
        } else {
            "none".into()
        };
        let mut values: Vec<String> = std::iter::once("none").chain(keys.iter().copied()).map(str::to_owned).collect();
        if track.use_soft_synth && !keys.contains(&current.as_str()) {
            values.push(current.clone());
        }
        let synths = Choice { selected: editors::index_of(&values, &current), values, labels: Vec::new() };

        // LFO values while playing (the LFO step watch).
        let tick = self.controller.as_ref().and_then(Controller::tick);
        let lfo = match tick {
            Some(tick) => LFO_TARGETS
                .iter()
                .filter_map(|target| {
                    let object = track.field(target.lfo_key)?;
                    Some((target.key, ordrumbox_core::lfo::value(object.as_object()?, tick as f64)))
                })
                .collect(),
            None => Vec::new(),
        };
        Some(TrackView { track, title, sample, decay, instruments, samples, synths, lfo })
    }

    /// Every sample of instrument `id`: (kit, url, name), the current kit's
    /// first, then by kit and name (`getSamplesForInstrument`).
    fn samples_for(&self, id: &str) -> Vec<(String, String, String)> {
        let current = self.kits.kits.get(self.kit_index).map_or("", |k| k.name.as_str());
        let mut found: Vec<(String, String, String)> = self
            .kits
            .kits
            .iter()
            .flat_map(|kit| {
                kit.instruments.iter().filter(|i| i.key == id).map(|i| {
                    let name = if i.display_name.is_empty() { i.url.clone() } else { i.display_name.clone() };
                    (kit.name.clone(), i.url.clone(), name)
                })
            })
            .collect();
        found.sort_by(|a, b| {
            (a.0 != current)
                .cmp(&(b.0 != current))
                .then_with(|| editors::locale_cmp(&a.0, &b.0))
                .then_with(|| editors::locale_cmp(&a.2, &b.2))
        });
        found
    }

    fn note_target(&self, t: usize) -> NoteTarget {
        let Some(track) = self.track(t) else { return NoteTarget::Detached };
        if let Some((st, n)) = self.selected.filter(|&(st, n)| st == t && n < track.notes.len()) {
            debug_assert_eq!(st, t);
            return NoteTarget::Note(n);
        }
        match self.note_focus {
            Some(NoteFocus::Step { track: ft, beat, step }) if ft == t => {
                match track.notes.iter().position(|n| n.beat == beat && n.beat_step == step) {
                    Some(n) => NoteTarget::Note(n),
                    None => NoteTarget::Empty { beat, step },
                }
            }
            _ if track.notes.is_empty() => NoteTarget::Detached,
            _ => NoteTarget::Note(0),
        }
    }

    /// The note the note editor shows (its copy for a detached note).
    fn shown_note(&self, t: usize) -> Option<Note> {
        match self.note_target(t) {
            NoteTarget::Note(n) => self.track(t)?.notes.get(n).cloned(),
            NoteTarget::Empty { .. } => None,
            NoteTarget::Detached => {
                Some(self.detached.as_ref().filter(|(dt, _)| *dt == t).map_or_else(Note::default, |(_, n)| n.clone()))
            }
        }
    }

    fn note_view(&self, t: usize) -> NoteView {
        let name = self.track(t).map_or("", |tr| tr.name.as_str());
        let note = self.shown_note(t);
        let title = match (self.note_target(t), &note) {
            (NoteTarget::Empty { beat, step }, _) => format!("{name} [beat {} step {}] — no note", beat + 1, step + 1),
            (_, Some(n)) => format!("{name} [beat {} step {}]", n.beat + 1, n.beat_step + 1),
            _ => name.to_owned(),
        };
        let Some(n) = &note else {
            return NoteView {
                title,
                note: None,
                scales: Choice::default(),
                arp_type: Choice::default(),
                arp_range: 0,
            };
        };
        let state = editors::arp_state(n, &self.scale_list);
        let names = self.scale_list.names.clone();
        let scale = n.arp_scale.clone().unwrap_or_else(|| state.scale.clone());
        let scales = Choice { selected: editors::index_of(&names, &scale), values: names, labels: Vec::new() };
        let types: Vec<String> = ARP_TYPES.iter().map(|t| (*t).to_owned()).collect();
        let kind = n.arp_type.clone().unwrap_or_else(|| state.kind.clone());
        let arp_type = Choice { selected: editors::index_of(&types, &kind), values: types, labels: Vec::new() };
        NoteView { title, note, scales, arp_type, arp_range: state.range }
    }

    fn kit_controls(&self) -> Option<KitControls> {
        let sample = self.kit_editor?;
        let instrument = self.kits.sample(self.kit_index, sample)?;
        let names: Vec<String> = self.kits.kits.iter().map(|k| k.name.clone()).collect();
        let kits =
            Choice { selected: self.kit_index.min(names.len().saturating_sub(1)), values: names, labels: Vec::new() };
        let ids: Vec<String> = catalog().instruments.iter().map(|i| i.id.clone()).collect();
        let instruments =
            Choice { selected: editors::index_of(&ids, &instrument.key), values: ids, labels: Vec::new() };
        Some(KitControls {
            kits,
            instruments,
            gain: instrument.gain_db,
            tune: instrument.tune,
            decay: instrument.decay.unwrap_or(0.0),
        })
    }

    // ─── The mouse ───────────────────────────────────────────────────────────

    /// A press at `(x, y)`; whether the widgets took it.
    pub fn mouse_down(&mut self, size: (i32, i32, i32), x: i32, y: i32, button: Button, shift: bool) -> bool {
        self.panel_size = size;
        let (width, height, scale) = size;
        if let Some(dropdown) = &self.dropdown {
            let picked = (button == Button::Left).then(|| dropdown.hit(width, height, scale, x, y)).flatten();
            match picked {
                Some(i) => self.pick(i),
                None => self.dropdown = None,
            }
            return true;
        }
        if let Some(entry) = &self.entry {
            if !entry.rect(width, height, scale).contains(x, y) {
                self.entry = None;
            }
            return true;
        }
        let layout = self.panel_layout(width, height, scale);
        let Some(item) = panel::hit(&layout.items, x, y).cloned() else {
            self.focus = None;
            // A click on the track editor's background is its own.
            return layout.right > 0 && x >= width - layout.right;
        };
        let Some(id) = item.id else { return false };
        if button == Button::Right {
            if let Kind::Value { spec, value, .. } = item.kind {
                self.focus = Some(id);
                self.open_entry(id, spec, value);
            }
            return true;
        }
        let now = Instant::now();
        let double = self.last_click.is_some_and(|(last, at)| last == id && now - at < DOUBLE_CLICK);
        self.last_click = Some((id, now));
        match item.kind {
            Kind::Value { spec, value, label, text, .. } => {
                self.focus = Some(id);
                if double {
                    // Double-click: the default value.
                    self.last_click = None;
                    self.set_value(id, spec.default);
                    return true;
                }
                let mut drag = Drag { id, spec, start: value, x, y, bar: None, fine: false };
                if spec.style == Style::Slider {
                    let (left, w) = panel::slider_bar(item.rect, label.is_some(), !text.is_empty(), scale);
                    if x < left - 2 * scale {
                        // The label: selects the row.
                        self.select_row(id);
                        return true;
                    }
                    if shift {
                        drag.fine = true;
                    } else {
                        drag.bar = Some((left, w));
                        let v = spec.slider_at(f64::from(x - left) / f64::from(w));
                        drag.start = v;
                        self.set_value(id, v);
                    }
                }
                self.drag = Some(drag);
            }
            Kind::Select { values, labels, selected } => {
                self.focus = Some(id);
                self.dropdown = Some(Dropdown::new(id, item.rect, values, labels, selected));
            }
            _ => self.activate(id),
        }
        true
    }

    /// The mouse moved; whether to redraw.
    pub fn mouse_move(&mut self, size: (i32, i32, i32), x: i32, y: i32, shift: bool) -> bool {
        let (width, height, scale) = size;
        if let Some(dropdown) = &mut self.dropdown {
            if let Some(i) = dropdown.hit(width, height, scale, x, y).filter(|&i| i != dropdown.at) {
                dropdown.at = i;
                return true;
            }
            return false;
        }
        let Some(drag) = self.drag else { return false };
        let value = match (drag.spec.style, drag.bar) {
            (Style::Knob, _) => drag.spec.drag(drag.start, f64::from(drag.y - y), shift),
            (Style::Slider, Some((left, w))) => drag.spec.slider_at(f64::from(x - left) / f64::from(w.max(1))),
            (Style::Slider, None) => {
                let w = self.slider_width(size, drag.id).max(1);
                drag.spec.slider_fine(drag.start, f64::from(x - drag.x) / f64::from(w))
            }
        };
        // `commitValue`: only a value that moved.
        if self.current_value(size, drag.id) != Some(value) {
            self.set_value(drag.id, value);
            return true;
        }
        false
    }

    /// The mouse over the grid: the step it is on, for the note tooltip; whether it changed.
    pub fn grid_hover(&mut self, size: (i32, i32, i32), x: i32, y: i32) -> bool {
        let (width, height, scale) = size;
        let right = self.track_editor_width(width, scale);
        let hover = match self.pattern() {
            Some(pattern) if self.kit_editor.is_none() && !self.song_view && self.drag.is_none() => {
                match Layout::new(width - right, height, scale, pattern, self.status.len()).hit(pattern, x, y) {
                    Some(view::Hit::Cell { track, beat, step }) => Some((track, beat, step)),
                    _ => None,
                }
            }
            _ => None,
        };
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    /// The tooltip of the note under the mouse (the first of its step), as
    /// `PointerSection.onMouseOver` shows it.
    pub(super) fn draw_tooltip(&self, canvas: &mut Canvas, scale: i32, right: i32) {
        if self.song_view {
            return self.draw_song_tooltip(canvas, scale);
        }
        let Some((t, beat, step)) = self.hover else { return };
        if self.menu.is_some() || self.popup_open() || self.overlay.is_some() || self.kit_editor.is_some() {
            return;
        }
        let Some(pattern) = self.pattern() else { return };
        let Some(track) = pattern.tracks.get(t) else { return };
        let Some(note) = track.notes.iter().find(|n| n.beat == beat && n.beat_step == step) else { return };
        let layout = Layout::new(canvas.width() - right, canvas.height(), scale, pattern, self.status.len());
        let cell = layout.cell_rect(t, beat, step, track.steps_per_beat);
        view::draw_tooltip(canvas, &editors::note_tooltip(note, track.pitch), cell, scale);
    }

    pub fn mouse_up(&mut self) {
        self.drag = None;
    }

    /// The wheel over the widgets (one notch: one step); whether it was theirs.
    pub fn wheel(&mut self, size: (i32, i32, i32), x: i32, y: i32, lines: isize, shift: bool, ctrl: bool) -> bool {
        if let Some(dropdown) = &mut self.dropdown {
            dropdown.scroll(-lines);
            return true;
        }
        if lines == 0 || self.entry.is_some() {
            return false;
        }
        let (width, height, scale) = size;
        let layout = self.panel_layout(width, height, scale);
        let Some(Item { id: Some(id), kind: Kind::Value { spec, value, .. }, .. }) =
            panel::hit(&layout.items, x, y).cloned()
        else {
            return false;
        };
        let mut v = value;
        for _ in 0..lines.unsigned_abs() {
            v = spec.nudge(v, lines > 0, shift, ctrl);
        }
        if v != value {
            self.set_value(id, v);
        }
        true
    }

    fn current_value(&self, size: (i32, i32, i32), id: Id) -> Option<f64> {
        let layout = self.panel_layout(size.0, size.1, size.2);
        match panel::find(&layout.items, id)?.kind {
            Kind::Value { value, .. } => Some(value),
            _ => None,
        }
    }

    fn slider_width(&self, size: (i32, i32, i32), id: Id) -> i32 {
        let layout = self.panel_layout(size.0, size.1, size.2);
        match panel::find(&layout.items, id) {
            Some(Item { rect, kind: Kind::Value { label, text, .. }, .. }) => {
                panel::slider_bar(*rect, label.is_some(), !text.is_empty(), size.2).1
            }
            _ => 1,
        }
    }

    // ─── The keyboard ────────────────────────────────────────────────────────

    /// A key for the widgets (the focused one, the open list or entry);
    /// whether it was theirs.
    pub fn panel_key(&mut self, size: (i32, i32, i32), key: PanelKey, shift: bool, ctrl: bool) -> bool {
        self.panel_size = size;
        if let Some(entry) = &mut self.entry {
            match key {
                PanelKey::Enter => self.commit_entry(),
                PanelKey::Escape => self.entry = None,
                PanelKey::Backspace => {
                    entry.text.pop();
                }
                PanelKey::Space if entry.spec.is_none() => entry.text.push(' '),
                _ => {}
            }
            return true;
        }
        if let Some(dropdown) = &mut self.dropdown {
            match key {
                PanelKey::Up => dropdown.step(-1),
                PanelKey::Down => dropdown.step(1),
                PanelKey::PageUp => dropdown.step(-10),
                PanelKey::PageDown => dropdown.step(10),
                PanelKey::Home => dropdown.step(isize::MIN / 2),
                PanelKey::End => dropdown.step(isize::MAX / 2),
                PanelKey::Enter | PanelKey::Space => {
                    let at = dropdown.at;
                    self.pick(at);
                }
                PanelKey::Escape => self.dropdown = None,
                _ => {}
            }
            return true;
        }
        let layout = self.panel_layout(size.0, size.1, size.2);
        let focusable: Vec<Item> =
            layout.items.into_iter().filter(|i| !matches!(i.id, Some(Id::TrackVolume(_) | Id::MasterVolume))).collect();
        if key == PanelKey::Tab {
            let next = panel::next_focus(&focusable, self.focus, if shift { -1 } else { 1 });
            if next.is_none() {
                return false;
            }
            self.focus = next;
            return true;
        }
        let Some(id) = self.focus else { return false };
        let Some(item) = panel::find(&focusable, id).cloned() else {
            self.focus = None;
            return false;
        };
        if key == PanelKey::Escape {
            self.focus = None;
            return true;
        }
        match item.kind {
            Kind::Value { spec, value, .. } => {
                let up = match key {
                    PanelKey::Up | PanelKey::Right | PanelKey::Plus => true,
                    PanelKey::Down | PanelKey::Left | PanelKey::Minus => false,
                    PanelKey::Enter => {
                        self.open_entry(id, spec, value);
                        return true;
                    }
                    _ => return false,
                };
                let v = spec.nudge(value, up, shift, ctrl);
                if v != value {
                    self.set_value(id, v);
                }
                true
            }
            Kind::Select { values, labels, selected } => {
                let delta = match key {
                    PanelKey::Up | PanelKey::Left | PanelKey::Minus => -1,
                    PanelKey::Down | PanelKey::Right | PanelKey::Plus => 1,
                    PanelKey::Enter | PanelKey::Space => {
                        self.dropdown = Some(Dropdown::new(id, item.rect, values, labels, selected));
                        return true;
                    }
                    _ => return false,
                };
                let at = (selected as isize + delta).clamp(0, values.len() as isize - 1) as usize;
                if at != selected
                    && let Some(value) = values.get(at)
                {
                    self.select(id, value);
                }
                true
            }
            _ => {
                if matches!(key, PanelKey::Enter | PanelKey::Space) {
                    self.activate(id);
                    return true;
                }
                false
            }
        }
    }

    /// A typed character (the number entry, the open list's letter jump).
    pub fn panel_text(&mut self, c: char) -> bool {
        if let Some(entry) = &mut self.entry {
            let accepted = match entry.spec {
                Some(_) => c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E'),
                None => !c.is_control(),
            };
            if accepted {
                entry.text.push(c);
            }
            return true;
        }
        if let Some(dropdown) = &mut self.dropdown {
            if c.is_alphanumeric() {
                dropdown.jump(c);
            }
            return true;
        }
        false
    }

    /// View > Track editor.
    pub fn toggle_track_editor(&mut self) {
        self.show_track_editor = !self.show_track_editor;
        self.focus = None;
        self.dropdown = None;
    }

    // ─── Number entry ────────────────────────────────────────────────────────

    /// `promptNumericInput`: `Enter value for LABEL (min–max unit):`.
    fn open_entry(&mut self, id: Id, spec: Spec, value: f64) {
        let (label, unit) = control_name(id);
        let unit = if unit.is_empty() { String::new() } else { format!(" {unit}") };
        let title = format!(
            "Enter value for {label} ({}–{}{unit}):",
            js::number_to_string(spec.min),
            js::number_to_string(spec.max)
        );
        self.entry = Some(Entry { id, spec: Some(spec), title, text: js::number_to_string(value) });
    }

    fn commit_entry(&mut self) {
        let Some(entry) = self.entry.take() else { return };
        let Some(spec) = entry.spec else { return self.commit_text(entry.id, &entry.text) };
        if entry.text.trim().is_empty() {
            return;
        }
        let number = js::parse_float(&Value::String(entry.text));
        if number.is_nan() {
            return;
        }
        self.set_value(entry.id, spec.typed(number));
    }

    // ─── What the widgets do ─────────────────────────────────────────────────

    fn track_name(&self, t: usize) -> String {
        self.track(t).map_or_else(String::new, |tr| tr.name.clone())
    }

    /// `cmd.updateTrack` on the edited track; a step of history.
    fn update_track(&mut self, t: usize, updates: &[(&str, Value)], desc: &str, coalesce: bool) {
        if self.editor.update_track(self.pattern_index, t, updates, Some(desc), coalesce) {
            self.edited();
        }
    }

    /// A row's label clicked (`selectedPropKey`).
    fn select_row(&mut self, id: Id) {
        if let Id::Gen(key) | Id::Loop(key) | Id::Fx(key) | Id::Row(key) = id {
            self.editors.selected_prop = Some(key);
        }
    }

    /// A knob or a slider set to `value`.
    fn set_value(&mut self, id: Id, value: f64) {
        if let Id::MasterVolume = id {
            self.master_volume = value;
            if let Some(controller) = &mut self.controller {
                controller.set_volume(value as f32);
            }
            return;
        }
        if let Id::KitKnob(field) = id {
            return self.set_kit_value(field, value);
        }
        if let Id::TrackVolume(t) = id {
            let desc = format!("Volume on {}", self.track_name(t));
            return self.update_track(t, &[("velocity", num(value))], &desc, true);
        }
        let Some(t) = self.editor_track() else { return };
        let name = self.track_name(t);
        match id {
            Id::KnobBar(key) => {
                let label = KNOB_BAR.iter().find(|(p, _)| p.key == key).map_or(key, |(p, _)| p.label);
                self.update_track(t, &[(key, num(value))], &format!("{label} on {name}"), true);
            }
            Id::Fx(key) => {
                let label = FX_KNOBS.iter().find(|p| p.key == key).map_or(key, |p| p.label);
                self.update_track(t, &[(key, num(value))], &format!("{label} on {name}"), true);
            }
            Id::Gen(key) => self.update_track(t, &[(key, num(value))], &format!("{key} on {name}"), true),
            Id::Loop(key) => self.loop_slider(t, key, value),
            Id::Decay => {
                // The sample's decay (the sound registry's in the JS): not undoable.
                if let Some((kit, sample)) = self.track_sample_url(t).and_then(|u| self.kits.find_url(&u)) {
                    self.set_kit_sample_value(kit, sample, Field::Decay, value);
                }
            }
            Id::Lfo(param) => self.lfo_value(t, param, value),
            Id::NoteKnob(key) | Id::NoteSlider(key) => self.note_value(t, key, value),
            _ => {}
        }
    }

    /// `onLoopSlider`.
    fn loop_slider(&mut self, t: usize, key: &str, value: f64) {
        let Some(track) = self.track(t) else { return };
        let name = track.name.clone();
        let max_steps = f64::from(track.beat_count * track.steps_per_beat);
        match key {
            "stepsPerBeat" => {
                if self.editor.set_steps_per_beat(self.pattern_index, t, value, true) {
                    self.edited();
                }
            }
            "loopAtStep" => {
                self.update_track(t, &[(key, num(value.min(max_steps)))], &format!("{key} on {name}"), true)
            }
            _ => self.update_track(t, &[(key, num(value))], &format!("{key} on {name}"), true),
        }
    }

    /// The selected LFO target's LFO object (a default one first).
    fn lfo_object(&self, t: usize, kind: &str) -> (Object, &'static str, bool) {
        let target = &LFO_TARGETS[usize::from(self.editors.lfo_target.min(4))];
        let current = self.track(t).and_then(|tr| tr.field(target.lfo_key)).and_then(|v| v.as_object().cloned());
        let created = current.is_none();
        let object = current.unwrap_or_else(|| {
            let mut o = Object::new();
            o.insert("type", Value::String(kind.to_owned()));
            o.insert("freq", num(1.0));
            o.insert("min", num(target.min));
            o.insert("max", num(target.max));
            o.insert("phase", num(0.0));
            o
        });
        (object, target.lfo_key, created)
    }

    fn lfo_value(&mut self, t: usize, param: LfoParam, value: f64) {
        let (mut object, lfo_key, _) = self.lfo_object(t, "sine");
        object.insert(param.key(), num(value));
        let desc = format!("LFO {} on {}", param.key(), self.track_name(t));
        self.update_track(t, &[(lfo_key, Value::Object(object))], &desc, true);
    }

    fn note_value(&mut self, t: usize, key: &'static str, value: f64) {
        let Some(note) = self.shown_note(t) else { return };
        let (updates, what) = if key == "arpRange" {
            let range = value.max(0.0) as usize;
            (vec![("arp", editors::arp_value(&note, &self.scale_list, None, None, Some(range)))], "arp")
        } else {
            (vec![(key, num(value))], key)
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
                    let _ = note.set_field(key, value);
                }
                self.detached = Some((t, note));
            }
            NoteTarget::Empty { .. } => {}
        }
    }

    fn set_kit_value(&mut self, field: Field, value: f64) {
        let Some(sample) = self.kit_editor else { return };
        self.set_kit_sample_value(self.kit_index, sample, field, value);
    }

    /// A kit knob: the kit changes now, the player and the disk shortly after.
    fn set_kit_sample_value(&mut self, kit: usize, sample: usize, field: Field, value: f64) {
        if let Some(message) = self.kits.set_value(kit, sample, field, value) {
            self.message = message;
            self.kits_dirty = Some(Instant::now());
        }
    }

    /// Apply waiting kit knob edits when due; when they will be.
    pub(super) fn flush_kits(&mut self) -> Option<Instant> {
        let since = self.kits_dirty?;
        let due = since + KITS_DELAY;
        if Instant::now() < due {
            return Some(due);
        }
        self.kits_dirty = None;
        let message = self.message.clone();
        self.kits_changed(message);
        None
    }

    /// An option picked in the open list.
    fn pick(&mut self, index: usize) {
        let Some(dropdown) = self.dropdown.take() else { return };
        if let Some(value) = dropdown.values.get(index) {
            self.select(dropdown.id, value);
        }
    }

    /// A select set to `value`.
    fn select(&mut self, id: Id, value: &str) {
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
            Id::KitKit => {
                let target = self.kits.kits.iter().position(|k| k.name == value);
                if let (Some(sample), Some(target)) = (self.kit_editor, target)
                    && let Some(message) = self.kits.move_to_kit(self.kit_index, sample, target)
                {
                    self.kits_changed(message);
                }
                return;
            }
            Id::KitInstrument => {
                if let Some(sample) = self.kit_editor
                    && let Some(message) = self.kits.set_instrument(self.kit_index, sample, value)
                {
                    self.kits_changed(message);
                }
                return;
            }
            _ => {}
        }
        let Some(t) = self.editor_track() else { return };
        let name = self.track_name(t);
        match id {
            Id::TrackSelect(key) => {
                let v = if key == "delayTime" {
                    num(js::parse_float(&Value::String(value.to_owned())))
                } else {
                    Value::String(value.to_owned())
                };
                self.update_track(t, &[(key, v)], &format!("{key} on {name}"), false);
            }
            Id::LfoType => {
                let (mut object, lfo_key, _) = self.lfo_object(t, value);
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
            Id::Synth => {
                let updates: Vec<(&str, Value)> = if value == "none" {
                    vec![("useSoftSynth", Value::Bool(false))]
                } else {
                    vec![
                        ("useSoftSynth", Value::Bool(true)),
                        ("useAutoAssignSound", Value::Bool(false)),
                        ("synthSoundKey", Value::String(value.to_owned())),
                    ]
                };
                self.update_track(t, &updates, &format!("Synth on {name}"), false);
            }
            Id::NoteSelect(key) => {
                let Some(note) = self.shown_note(t) else { return };
                let (field, arp) = if key == "arpScale" {
                    ("_arpScale", editors::arp_value(&note, &self.scale_list, Some(value), None, None))
                } else {
                    ("_arpType", editors::arp_value(&note, &self.scale_list, None, Some(value), None))
                };
                let updates = [(field, Value::String(value.to_owned())), ("arp", arp)];
                self.update_note(t, &updates, &format!("Edit note {key} on {name}"), false);
            }
            _ => {}
        }
    }

    /// A button, LED, tab or row clicked.
    fn activate(&mut self, id: Id) {
        match id {
            Id::Tab(i) => self.editors.tab = i,
            Id::FxTab(i) => self.editors.fx_tab = i,
            Id::GenTab(i) => self.editors.gen_tab = i,
            Id::LfoTab(i) => self.editors.lfo_target = i,
            Id::NoteTab(i) => self.editors.note_tab = i,
            Id::Row(_) => self.select_row(id),
            Id::LoadSample => self.open_picker(Purpose::ReplaceTrackSample),
            Id::SongButton(button) => self.song_button(button),
            Id::SongName | Id::SongDescription => self.song_text_entry(id),
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
            Id::FxLed(i) => {
                // `toggleFxByKey`.
                let fx = &FX_DEFS[usize::from(i.min(3))];
                let updates = if fx.key == "filterType" {
                    if editors::fx_on(&track, fx) {
                        self.editors.prev_filter_type = Some(track.filter_type.clone());
                        vec![("filterType", Value::String("allpass".into()))]
                    } else {
                        let back = self.editors.prev_filter_type.clone().unwrap_or_else(|| "lowpass".into());
                        vec![("filterType", Value::String(back))]
                    }
                } else {
                    let on = editors::number(&track, fx.key) > 0.0;
                    vec![(fx.key, num(if on { 0.0 } else { 0.5 }))]
                };
                self.update_track(t, &updates, &format!("Toggle {} on {name}", fx.key), false);
            }
            Id::FilterIcon(i) => {
                let value = FILTER_TYPES[usize::from(i.min(2))];
                let current = track.filter_type.clone();
                let next = if current == value { "allpass" } else { value };
                self.editors.prev_filter_type = (current != value).then_some(current);
                self.update_track(
                    t,
                    &[("filterType", Value::String(next.into()))],
                    &format!("Filter type on {name}"),
                    false,
                );
            }
            Id::LfoLed(i) => {
                self.editors.lfo_target = i;
                let target = &LFO_TARGETS[usize::from(i.min(4))];
                let on = track.field(target.lfo_key).is_some_and(|v| !v.is_null());
                let value = if on { Value::Null } else { Value::Object(self.lfo_object(t, "sine").0) };
                self.update_track(t, &[(target.lfo_key, value)], &format!("LFO {} on {name}", target.key), false);
            }
            Id::Toggle(key) => {
                let on = track.field(key).and_then(|v| v.as_bool()).unwrap_or(false);
                self.update_track(t, &[(key, Value::Bool(!on))], &format!("{key} on {name}"), false);
            }
            Id::AutoAssign => {
                // `toggleAuto`: back on, the synth goes and the sample is picked by name.
                let on = !track.use_auto_assign_sound;
                let mut updates = vec![("useAutoAssignSound", Value::Bool(on))];
                if on {
                    updates.push(("useSoftSynth", Value::Bool(false)));
                    updates.push(("synthSoundKey", Value::Null));
                }
                self.update_track(t, &updates, &format!("Auto-assign on {name}"), false);
            }
            _ => {}
        }
    }

    /// The load button's file: the track's sample replaced.
    pub(super) fn replace_track_sample(&mut self, path: &Path) {
        let Some(t) = self.editor_track() else { return };
        let Some((kit, sample)) = self.track_sample_url(t).and_then(|u| self.kits.find_url(&u)) else {
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

/// Label and unit of a control, for the number entry's title.
fn control_name(id: Id) -> (&'static str, &'static str) {
    let prop = |list: &[editors::Prop], key: &str| list.iter().find(|p| p.key == key).map_or("", |p| p.label);
    match id {
        Id::KnobBar(key) => KNOB_BAR.iter().find(|(p, _)| p.key == key).map_or(("", ""), |(p, u)| (p.label, *u)),
        Id::Decay => ("Decay", ""),
        Id::Fx(key) => (prop(&FX_KNOBS, key), ""),
        Id::Gen(key) => {
            let rows = GEN_GROOVE.iter().chain(GEN_ENGINE.iter());
            let label = rows
                .filter_map(|r| match r {
                    GenRow::Slider(p) if p.key == key => Some(p.label),
                    _ => None,
                })
                .next()
                .unwrap_or_else(|| prop(&GEN_MANUAL, key));
            (label, "")
        }
        Id::Loop(key) => (prop(&LOOP_PROPS, key), ""),
        Id::Lfo(param) => (
            match param {
                LfoParam::Freq => "Freq",
                LfoParam::Min => "Min",
                LfoParam::Max => "Max",
                LfoParam::Phase => "Phase",
            },
            "",
        ),
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
        Id::TrackVolume(_) => ("Volume", ""),
        Id::MasterVolume => ("Master Gain", ""),
        _ => ("", ""),
    }
}
