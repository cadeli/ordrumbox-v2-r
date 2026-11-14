//! The editors laid out as panel items.

use super::tables::{
    COMPOSE_TAB, DEFAULT_DECAY, FILTER_ICONS, FILTER_TYPES, FX_DEFS, FX_KNOBS, FxDef, GENERATION_COMPOSE,
};
use super::tables::{
    GENERATION_ENGINE, GENERATION_GROOVE, GENERATION_MANUAL, GenerationRow, KIT_KNOBS, KNOB_BAR, LFO_TARGETS,
    LOOP_PROPS, LfoTarget, NOTE_GROUPS, NOTE_KNOBS, NOTE_TABS, NoteRow, Prop, STYLE_DEFAULT, TABS, WAVE_TYPES,
    fx_label, prop, track_select_options,
};
use super::tables::{TAB_FX, TAB_LFO, TAB_LOOP, TAB_SOUND};
use super::values::{EditorState, flag, fx_on, has_lfo, index_of, loop_length, number, text_of};
use super::{Choice, ComposeView, KitControls, NoteView, TrackView};
use crate::control::{Format, Spec};
use crate::kits::Field;
use crate::panel::{Id, Item, KNOB_SIZE, Kind, LfoParam, Rect, dim, label_width};
use crate::scale::Scale;
use crate::theme;
use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Note, Track, num};
use ordrumbox_engine::strip_settings::delay_beats_used;
use ordrumbox_logic::generate;

// ─── Layout ──────────────────────────────────────────────────────────────────

pub(super) fn row_h(scale: Scale) -> i32 {
    16 * scale
}

/// Height of a row of knobs: the knob, its label, its value.
pub(super) fn knob_row_h(scale: Scale) -> i32 {
    48 * scale
}

/// Height of a row of tabs.
pub(super) const TAB_H: i32 = 14;

/// Height of the track editor's main tab bar: taller than a row of sub-tabs,
/// so it reads as the head of its card.
pub(super) const MAIN_TAB_H: i32 = 18;

/// The gap between the track editor's two cards: its sound (the sample bar
/// and the knob bar), then its tabs.
pub(super) const CARD_GAP: i32 = 10;

/// A tab: its id, its LED's id, its label, active, LED on.
type TabSlot<'a> = (Id, Option<Id>, &'a str, bool, bool);

/// Places rows top to bottom in a column.
pub(super) struct Rows {
    x: i32,
    w: i32,
    y: i32,
    scale: Scale,
    items: Vec<Item>,
}

impl Rows {
    pub(super) fn row(&mut self) -> Rect {
        let r = Rect::new(self.x, self.y, self.w, row_h(self.scale));
        self.y += row_h(self.scale) + 2 * self.scale;
        r
    }

    pub(super) fn label(&mut self, r: Rect, text: &str, key: Option<&'static str>, selected: bool) {
        let s = self.scale;
        if selected {
            self.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
        }
        let rect = Rect::new(r.x, r.y + (r.h - 8 * s) / 2, label_width(s) - 4 * s, 8 * s);
        self.items.push(Item::new(None, rect, Kind::Text { text: text.to_owned(), color: dim() }));
        if let Some(key) = key {
            self.items.push(Item::new(Some(Id::Row(key)), Rect::new(r.x, r.y, label_width(s), r.h), Kind::Highlight));
            // The row id goes under the label only: drop its highlight look.
            if let Some(last) = self.items.last_mut() {
                last.kind = Kind::Text { text: String::new(), color: dim() };
            }
        }
    }

    /// The control part of a row (after its label).
    pub(super) fn field(&self, r: Rect, w: i32) -> Rect {
        let s = self.scale;
        let x = r.x + label_width(s);
        Rect::new(x, r.y + 2 * s, w.min(r.x + r.w - x), r.h - 4 * s)
    }

    /// A row of tabs, one slot each; an empty slot (`None`) keeps its place,
    /// so the tabs after it do not move when it goes.
    pub(super) fn tabs(&mut self, ids: &[Option<TabSlot>]) {
        self.tab_row(ids, TAB_H, |label, active| Kind::Tab { label, active });
    }

    /// The track editor's main tabs: the taller, filled tab bar its tab card
    /// starts with.
    pub(super) fn main_tabs(&mut self, ids: &[Option<TabSlot>]) {
        self.tab_row(ids, MAIN_TAB_H, |label, active| Kind::MainTab { label, active });
    }

    /// A row of tabs `h` high (at scale 1), each drawn as `kind` makes it.
    pub(super) fn tab_row(&mut self, ids: &[Option<TabSlot>], h: i32, kind: fn(String, bool) -> Kind) {
        let s = self.scale;
        let r = Rect::new(self.x, self.y, self.w, h * s);
        let n = ids.len().max(1) as i32;
        for (i, (tab, led, label, active, on)) in ids.iter().enumerate().filter_map(|(i, t)| Some((i, t.as_ref()?))) {
            let x = r.x + i as i32 * r.w / n;
            let w = (i as i32 + 1) * r.w / n - i as i32 * r.w / n;
            let led_w = if led.is_some() { 10 * s } else { 0 };
            if let Some(led) = led {
                self.items.push(Item::new(Some(*led), Rect::new(x, r.y, led_w, r.h), Kind::Led { on: *on }));
            }
            let kind = kind((*label).to_owned(), *active);
            self.items.push(Item::new(Some(*tab), Rect::new(x + led_w, r.y, w - led_w - s.px(), r.h), kind));
        }
        self.y += (h + 4) * s;
    }

    pub(super) fn slider(&mut self, id: Id, spec: Spec, value: f64, label: &str, text: String, lfo: bool) {
        let r = self.row();
        let kind = Kind::Value { spec, value, label: Some(label.to_owned()), text, lfo };
        self.items.push(Item::new(Some(id), r, kind));
    }

    pub(super) fn knob_bar(&mut self, knobs: Vec<(Id, Spec, f64, &str, String, bool)>, slots: i32) {
        for (i, (id, spec, value, label, text, lfo)) in knobs.into_iter().enumerate() {
            let kind = Kind::Value { spec, value, label: Some(label.to_owned()), text, lfo };
            self.items.push(Item::new(Some(id), self.slot(i as i32, slots), kind));
        }
        self.y += knob_row_h(self.scale) + 4 * self.scale;
    }

    /// Slot `i` of a row of `slots` knobs, at the current y (which it leaves).
    pub(super) fn slot(&self, i: i32, slots: i32) -> Rect {
        let x = self.x + i * self.w / slots;
        let w = (i + 1) * self.w / slots - i * self.w / slots;
        Rect::new(x, self.y, w, knob_row_h(self.scale))
    }

    /// A control that is not a knob in a knob's slot: where the knob would
    /// be, its label under it, on the knobs' label line. The label is the
    /// setting's row (its tooltip), when it has one.
    pub(super) fn slot_choice(
        &mut self,
        slot: Rect,
        label: &str,
        key: Option<&'static str>,
        id: Option<Id>,
        kind: Kind,
    ) {
        let s = self.scale;
        let field = Rect::new(slot.x + 4 * s, slot.y + 7 * s, slot.w - 8 * s, KNOB_SIZE * s - 14 * s);
        self.items.push(Item::new(id, field, kind));
        let width = crate::canvas::text_width(label, s).min(slot.w);
        let text = Rect::new(slot.x + (slot.w - width) / 2, slot.y + (KNOB_SIZE + 3) * s, width, 8 * s);
        self.items.push(Item::new(key.map(Id::Row), text, Kind::Text { text: label.to_owned(), color: dim() }));
    }

    pub(super) fn text(&mut self, text: &str, color: u32) {
        let s = self.scale;
        let fitted: String = text.chars().take(crate::canvas::chars_in(self.w, s)).collect();
        self.items.push(Item::text(self.x, self.y, fitted, color, s));
        self.y += 12 * s;
    }

    /// A panel's title: [`Scale::title`] in the same row.
    pub(super) fn title(&mut self, text: &str) {
        let s = self.scale.title();
        let fitted: String = text.chars().take(crate::canvas::chars_in(self.w, s)).collect();
        self.items.push(Item::title(self.x, self.y, fitted, self.scale));
        self.y += 12 * self.scale;
    }
}

/// A setting's control: a knob in a bar of knobs, else a stepper for a
/// count and a slider for the rest.
pub(super) fn value_spec(p: &Prop, default: f64, knob: bool) -> Spec {
    let spec = if knob {
        Spec::knob(p.min, p.max, p.step, default)
    } else if p.count {
        Spec::stepper(p.min, p.max, p.step, default)
    } else {
        Spec::slider(p.min, p.max, p.step, default)
    };
    if p.format == Format::Percent { spec.percent() } else { spec }
}

/// Default of a setting, else its minimum.
pub(super) fn default_of(key: &str, min: f64) -> f64 {
    Track::default().field(key).and_then(|v| v.as_f64()).unwrap_or(min)
}

/// The track editor's widgets, and where its two cards go: the sound card
/// (title, sample bar, knob bar), then the tab card, which starts with the
/// main tab bar.
#[derive(Debug)]
pub struct TrackEditor {
    pub items: Vec<Item>,
    /// The y under the knob bar: the sound card's bottom.
    pub sound_bottom: i32,
    /// The y of the main tab bar: the tab card's top.
    pub tabs_top: i32,
    /// The y under the tallest tab, the same whatever the tab shown (the
    /// tab card runs down to the editor's bottom instead).
    #[cfg_attr(not(test), expect(dead_code, reason = "the tests check that nothing under it moves"))]
    pub bottom: i32,
}

/// The track editor in `area`.
pub fn track_editor(view: &TrackView, state: &EditorState, area: Rect, scale: Scale) -> TrackEditor {
    let s = scale;
    let track = view.track;
    let mut rows = Rows { x: area.x, w: area.w, y: area.y, scale: s, items: Vec::new() };
    rows.title(&view.title);

    // Sample bar: analysis, envelope with the decay end. Its place is kept
    // without a sample (a synth track), so the rows under it stay put when
    // the sound changes.
    if let Some(bar) = &view.sample {
        let info: String = bar.info.chars().take(crate::canvas::chars_in(area.w, s)).collect();
        rows.items.push(Item::text(area.x, rows.y + 2 * s, info, theme::text(), s));
    }
    rows.y += 15 * s;
    let (points, marker) = view.sample.as_ref().map_or((Vec::new(), None), |bar| (bar.envelope.clone(), bar.marker));
    rows.items.push(Item::new(None, Rect::new(area.x, rows.y, area.w, 32 * s), Kind::Envelope { points, marker }));
    rows.y += 36 * s;

    // Knob bar: while playing, a setting with an LFO shows the LFO's value.
    let lfo_now = |key: &str| view.lfo.iter().find(|(k, _)| *k == key).map(|&(_, v)| v);
    let knobs = KNOB_BAR
        .iter()
        .map(|(p, unit)| {
            if p.key == "decay" {
                let spec = value_spec(p, DEFAULT_DECAY, true).log();
                (Id::Decay, spec, view.decay, p.label, p.format.value(view.decay, unit), false)
            } else {
                let value = lfo_now(p.key).unwrap_or_else(|| number(track, p.key));
                let spec = value_spec(p, default_of(p.key, p.min), true);
                (Id::KnobBar(p.key), spec, value, p.label, p.format.value(value, unit), has_lfo(track, p.key))
            }
        })
        .collect();
    rows.knob_bar(knobs, 4);
    let sound_bottom = rows.y;

    rows.y += CARD_GAP * s;
    let tabs_top = rows.y;
    let tabs: Vec<_> =
        TABS.iter().enumerate().map(|(i, l)| Some((Id::Tab(i as u8), None, *l, state.tab == i as u8, false))).collect();
    rows.main_tabs(&tabs);

    let bottom = tallest_tab_bottom(rows.y, s);
    match state.tab {
        TAB_FX => fx_tab(&mut rows, view),
        TAB_SOUND => sound_tab(&mut rows, view),
        TAB_LFO => lfo_tab(&mut rows, view),
        TAB_LOOP => loop_tab(&mut rows, track),
        _ => generation_tab(&mut rows, track, view.compose.as_ref(), state),
    }
    TrackEditor { items: rows.items, sound_bottom, tabs_top, bottom }
}

/// The FX tab: the effects one under the other, each a strip of its own, as
/// in the LFO tab: its on / off button (named) in the label column, level
/// with its knobs and its choices.
pub(super) fn fx_tab(rows: &mut Rows, view: &TrackView) {
    for (i, fx) in FX_DEFS.iter().enumerate() {
        fx_strip(rows, view, i as u8, fx);
    }
}

/// Slots of an effect strip's knob row: the most controls an effect has.
pub(super) const FX_SLOTS: i32 = 5;

/// Width of a filter type icon, and the step from one to the next (at scale 1):
/// the four fit the label column.
pub(super) const FILTER_ICON_W: i32 = 19;
pub(super) const FILTER_ICON_STEP: i32 = 21;
/// Height of a filter type icon (at scale 1).
pub(super) const FILTER_ICON_H: i32 = 14;

pub(super) fn fx_strip(rows: &mut Rows, view: &TrackView, i: u8, fx: &FxDef) {
    let s = rows.scale;
    let track = view.track;
    let button = Rect::new(rows.x, rows.y + (KNOB_SIZE * s - row_h(s)) / 2, label_width(s) - 4 * s, row_h(s));
    let power = Kind::Button { label: fx.label.to_owned(), on: fx_on(track, fx) };
    rows.items.push(Item::new(Some(Id::FxPower(i)), button, power));
    if fx.key == "filterType" {
        // The filter's types, under its button.
        let y = button.y + button.h + 2 * s;
        for (i, (icon, value)) in FILTER_ICONS.iter().zip(FILTER_TYPES).enumerate() {
            let rect = Rect::new(rows.x + i as i32 * FILTER_ICON_STEP * s, y, FILTER_ICON_W * s, FILTER_ICON_H * s);
            let kind = Kind::Button { label: (*icon).to_owned(), on: track.filter_type == value };
            rows.items.push(Item::new(Some(Id::FilterIcon(i as u8)), rect, kind));
        }
    }
    let (x, w) = (rows.x, rows.w);
    rows.x += label_width(s);
    rows.w -= label_width(s);
    // The knobs first, then the choices: every strip reads the same way.
    let knobs = fx.controls.iter().filter_map(|&key| FX_KNOBS.iter().find(|p| p.key == key));
    let choices = fx.controls.iter().filter(|&&key| key != "filterType" && !FX_KNOBS.iter().any(|p| p.key == key));
    let mut slot = 0;
    for p in knobs {
        let rect = rows.slot(slot, FX_SLOTS);
        slot += 1;
        // A slap is one echo: no feedback to set, its knob kept greyed out.
        if p.key == "delayFeedback" && track.delay_type == "slap" {
            rows.slot_choice(rect, p.label, Some(p.key), None, Kind::Disabled { label: "one echo".into() });
            continue;
        }
        let value = view.lfo.iter().find(|(k, _)| *k == p.key).map_or_else(|| number(track, p.key), |&(_, v)| v);
        let mut spec = value_spec(p, default_of(p.key, p.min), true);
        // Ranges over decades: a logarithmic knob.
        if matches!(p.format, Format::Freq | Format::FineMillis) {
            spec = spec.log_full_travel();
        }
        let text = p.format.value(value, "");
        let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: has_lfo(track, p.key) };
        rows.items.push(Item::new(Some(Id::Fx(p.key)), rect, kind));
    }
    for &key in choices {
        let rect = rows.slot(slot, FX_SLOTS);
        slot += 1;
        let choice = if key == "sidechainSource" { view.sidechains.clone() } else { fx_choice(view, key) };
        rows.slot_choice(rect, fx_label(key), Some(key), Some(Id::TrackSelect(key)), choice.kind());
    }
    (rows.x, rows.w) = (x, w);
    rows.y += knob_row_h(s) + FX_STRIP_GAP * s;
}

/// The choice of the effect setting `key` (a select of fixed options).
fn fx_choice(view: &TrackView, key: &str) -> Choice {
    let track = view.track;
    let (values, mut labels) = track_select_options(key);
    let selected = index_of(&values, &text_of(track, key));
    if key == "delayBeats" {
        // Too long a delay plays at fewer beats: shown on the choice.
        let used = delay_beats_used(track.delay_beats, view.bpm);
        if used != track.delay_beats
            && let Some(label) = labels.get_mut(selected)
        {
            *label = format!("{label} ({} used)", num::text(used));
        }
    }
    Choice { values, labels, selected }
}

/// Pixels between two effect strips (at scale 1).
pub(super) const FX_STRIP_GAP: i32 = 4;

pub(super) fn sound_tab(rows: &mut Rows, view: &TrackView) {
    let s = rows.scale;
    let track = view.track;
    for (key, label, on) in [("mono", "Mono", track.mono), ("legato", "Legato", track.legato)] {
        let r = rows.row();
        rows.label(r, label, None, false);
        let field = rows.field(r, 36 * s);
        rows.items.push(Item::new(Some(Id::Toggle(key)), field, Kind::Toggle { on }));
    }
    let r = rows.row();
    rows.items.push(Item::new(
        Some(Id::AutoAssign),
        Rect::new(r.x, r.y, 12 * s, r.h),
        Kind::Led { on: track.auto_assign },
    ));
    rows.items.push(Item::text(r.x + 14 * s, r.y + (r.h - 8 * s) / 2, "auto", dim(), s));
    for (id, label, choice) in [
        (Id::Instrument, "Instrument", &view.instruments),
        (Id::Sample, "Sample", &view.samples),
        (Id::Synth, "Synth", &view.synths),
    ] {
        let r = rows.row();
        rows.label(r, label, None, false);
        if id == Id::Sample {
            // The sample's choice, then the button that loads one from a file.
            let field = rows.field(r, r.w);
            let load = Rect::new(field.x + field.w - LOAD_W * s, field.y, LOAD_W * s, field.h);
            let choice_field = Rect::new(field.x, field.y, field.w - (LOAD_W + 4) * s, field.h);
            rows.items.push(Item::new(Some(id), choice_field, choice.kind()));
            let button = Kind::Button { label: "File...".into(), on: false };
            rows.items.push(Item::new(Some(Id::LoadSample), load, button));
        } else {
            rows.items.push(Item::new(Some(id), rows.field(r, r.w), choice.kind()));
        }
    }
}

/// Width of the Sound tab's File... button (at scale 1): its label and a margin.
pub(super) const LOAD_W: i32 = 48;

/// The LFO tab: the five targets one under the other, each a strip of its
/// own, as in the FX tab: its on / off button (named) in the label column,
/// then its wave and its knobs (five strips under a button row each would
/// not fit the editor).
pub(super) fn lfo_tab(rows: &mut Rows, view: &TrackView) {
    for (i, target) in LFO_TARGETS.iter().enumerate() {
        lfo_strip(rows, view.track, i as u8, target);
    }
}

/// Slots of an LFO strip's knob row: the wave (two, its names are long),
/// then frequency, min, max, phase.
pub(super) const LFO_SLOTS: i32 = 6;

pub(super) fn lfo_strip(rows: &mut Rows, track: &Track, i: u8, target: &LfoTarget) {
    let s = rows.scale;
    let lfo = track.field(target.lfo_key).and_then(|v| v.as_object().cloned());
    let get = |key: &str, fallback: f64| {
        lfo.as_ref().map_or(fallback, |o| o.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN))
    };
    let wave = lfo
        .as_ref()
        .map_or_else(|| "sine".to_owned(), |o| o.get("type").and_then(Value::as_str).unwrap_or("sine").to_owned());

    // The button level with the knobs, the knobs right of the label column.
    let button = Rect::new(rows.x, rows.y + (KNOB_SIZE * s - row_h(s)) / 2, label_width(s) - 4 * s, row_h(s));
    let power = Kind::Button { label: target.label.to_owned(), on: lfo.is_some() };
    rows.items.push(Item::new(Some(Id::LfoPower(i)), button, power));
    let (x, w) = (rows.x, rows.w);
    rows.x += label_width(s);
    rows.w -= label_width(s);

    let values: Vec<String> = WAVE_TYPES.iter().map(|w| (*w).to_owned()).collect();
    let selected = index_of(&values, &wave);
    let choice = Choice { values, labels: Vec::new(), selected };
    let (first, second) = (rows.slot(0, LFO_SLOTS), rows.slot(1, LFO_SLOTS));
    let wave_slot = Rect::new(first.x, first.y, second.x + second.w - first.x, first.h);
    rows.slot_choice(wave_slot, "Wave", None, Some(Id::LfoType(i)), choice.kind());

    // Min and max: the target's range, its own end the default of each.
    let range = |default: f64| {
        let spec = Spec::knob(target.min, target.max, target.step, default);
        if target.key == "filterCutoff" { spec.log_full_travel() } else { spec }
    };
    let knobs = [
        (LfoParam::Freq, Spec::knob(0.1, 2.0, 0.1, 1.0), get("freq", 1.0)),
        (LfoParam::Min, range(target.min), get("min", target.min)),
        (LfoParam::Max, range(target.max), get("max", target.max)),
        (LfoParam::Phase, Spec::knob(0.0, 1.0, 0.01, 0.0), get("phase", 0.0)),
    ];
    for (slot, (param, spec, value)) in (2..).zip(knobs) {
        let text = crate::control::fmt(value);
        let kind = Kind::Value { spec, value, label: Some(param.label().to_owned()), text, lfo: false };
        rows.items.push(Item::new(Some(Id::Lfo(i, param)), rows.slot(slot, LFO_SLOTS), kind));
    }
    (rows.x, rows.w) = (x, w);
    rows.y += knob_row_h(s) + FX_STRIP_GAP * s;
}

pub(super) fn loop_tab(rows: &mut Rows, track: &Track) {
    let spb = track.steps_per_beat;
    let max_steps = f64::from(track.beat_count * spb);
    for p in &LOOP_PROPS {
        let default = default_of(p.key, p.min);
        let (value, spec, format) = match p.key {
            "stepsPerBeat" => (f64::from(spb), value_spec(p, default, false), p.format),
            // The loop length ends at the track's length.
            "loopLength" => (
                loop_length(track).min(max_steps),
                value_spec(&Prop { max: max_steps, ..*p }, p.min, false),
                Format::LoopLength(spb),
            ),
            _ => (track.swing_amount, value_spec(p, default, false), p.format),
        };
        rows.slider(Id::Loop(p.key), spec, value, p.label, format.value(value, ""), false);
    }
}

pub(super) fn generation_tab(rows: &mut Rows, track: &Track, compose: Option<&ComposeView>, state: &EditorState) {
    let s = rows.scale;
    // The simple view: auto, lock, a new line, three settings.
    let toggle = |rows: &mut Rows, key: &'static str, label: &str, on: bool| {
        let r = rows.row();
        rows.label(r, label, Some(key), state.selected_prop == Some(key));
        rows.items.push(Item::new(Some(Id::Toggle(key)), rows.field(r, 36 * s), Kind::Toggle { on }));
    };
    toggle(rows, "autoMode", "Auto", track.auto_mode);
    toggle(rows, "composeLock", "Lock", track.compose_lock);
    let r = rows.row();
    rows.label(r, "New line", None, false);
    let button = Kind::Button { label: "\u{21bb}".into(), on: false };
    rows.items.push(Item::new(Some(Id::Regenerate), rows.field(r, 36 * s), button));
    let melodic = instrument_family(&track.name).is_melodic();
    // The part above Advanced takes the compose view's height whatever the
    // track: Advanced stays put when the sound changes.
    let simple_bottom = simple_part_bottom(rows.y, s);
    match compose {
        Some(view) => {
            rows.text(&format!("{} / {}  (* = style)", view.style, view.shown.part), dim());
            for p in GENERATION_COMPOSE.iter().filter(|p| SIMPLE_COMPOSE.contains(&p.key)) {
                compose_slider(rows, track, view, p, state);
            }
        }
        None if melodic => {
            variant_select(rows, track, state);
            generation_slider(rows, track, &GENERATION_DENSITY, state);
        }
        None => rows.text("no part for this track in the pattern's style", dim()),
    }
    rows.y = rows.y.max(simple_bottom);
    // Everything else, folded.
    let r = rows.row();
    let label = if state.generation_advanced { "Advanced \u{25be}" } else { "Advanced \u{25b8}" };
    let button = Kind::Button { label: label.into(), on: state.generation_advanced };
    rows.items.push(Item::new(Some(Id::GenerationAdvanced), Rect::new(r.x, r.y + 2 * s, 120 * s, r.h - 4 * s), button));
    if !state.generation_advanced {
        return;
    }
    let compose_tab = compose.is_some() && state.generation_tab == COMPOSE_TAB;
    // Three slots whatever shows: Compose stays put when Auto turns Engine off.
    let tabs = if track.auto_mode {
        [
            Some((Id::GenerationTab(0), None, "Groove", !compose_tab && state.generation_tab == 0, false)),
            Some((Id::GenerationTab(1), None, "Engine", !compose_tab && state.generation_tab == 1, false)),
            None,
        ]
    } else {
        [Some((Id::GenerationTab(0), None, "Manual", !compose_tab, false)), None, None]
    };
    let [first, second, _] = tabs;
    let compose_slot = compose.map(|_| (Id::GenerationTab(COMPOSE_TAB), None, "Compose", compose_tab, false));
    rows.tabs(&[first, second, compose_slot]);
    if let (true, Some(view)) = (compose_tab, compose) {
        for p in &GENERATION_COMPOSE {
            compose_slider(rows, track, view, p, state);
        }
        return;
    }
    if !track.auto_mode {
        for p in &GENERATION_MANUAL {
            generation_slider(rows, track, p, state);
        }
        return;
    }
    for row in if state.generation_tab == 0 { &GENERATION_GROOVE } else { &GENERATION_ENGINE } {
        match row {
            // A drum line comes from its style (Compose); a melodic one has them above.
            GenerationRow::Select("autoVariant", _) | GenerationRow::Slider(Prop { key: "autoDensity", .. }) => {}
            GenerationRow::Slider(p) => generation_slider(rows, track, p, state),
            GenerationRow::Toggle(key, label) => toggle(rows, key, label, flag(track, key)),
            GenerationRow::Select(key, label) => {
                let r = rows.row();
                rows.label(r, label, Some(key), state.selected_prop == Some(key));
                let values = track_select_options(key).0;
                let selected = index_of(&values, &text_of(track, key));
                let choice = Choice { values, labels: Vec::new(), selected };
                rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * s), choice.kind()));
            }
        }
    }
}

/// The y under the Generate tab's simple part (from `top`): its tallest, a style's
/// line and its compose sliders.
pub(super) fn simple_part_bottom(top: i32, scale: Scale) -> i32 {
    let mut rows = Rows { x: 0, w: 0, y: top, scale, items: Vec::new() };
    rows.y += 12 * scale;
    for _ in SIMPLE_COMPOSE {
        rows.row();
    }
    rows.y
}

/// The rows of the Generate tab's tallest sub-tab.
pub(super) const GENERATION_ROWS: usize =
    max_len(&[GENERATION_GROOVE.len(), GENERATION_ENGINE.len(), GENERATION_COMPOSE.len(), GENERATION_MANUAL.len()]);

pub(super) const fn max_len(lens: &[usize]) -> usize {
    let mut max = 0;
    let mut i = 0;
    while i < lens.len() {
        if lens[i] > max {
            max = lens[i];
        }
        i += 1;
    }
    max
}

/// The y under the tallest tab (from `top`, under the main tabs): the Gen
/// tab's Advanced settings, or the FX tab's strips. The note editor starts
/// there whatever the tab, so it never moves.
pub(super) fn tallest_tab_bottom(top: i32, scale: Scale) -> i32 {
    let mut rows = Rows { x: 0, w: 0, y: top, scale, items: Vec::new() };
    // Auto, Lock, New line.
    for _ in 0..3 {
        rows.row();
    }
    rows.y = simple_part_bottom(rows.y, scale);
    rows.row(); // Advanced
    rows.tabs(&[]);
    for _ in 0..GENERATION_ROWS {
        rows.row();
    }
    let fx_bottom = top + FX_DEFS.len() as i32 * (knob_row_h(scale) + FX_STRIP_GAP * scale);
    rows.y.max(fx_bottom)
}

/// The compose settings of the simple view.
pub(super) const SIMPLE_COMPOSE: [&str; 3] = ["composeHitsPerMeasure", "composeComplexity", "composeVariation"];

/// A melodic track's density (the simple view).
pub(super) const GENERATION_DENSITY: Prop = prop("autoDensity", "Density", -1.0, 1.0, 0.01, Format::Density);

pub(super) fn generation_slider(rows: &mut Rows, track: &Track, p: &Prop, state: &EditorState) {
    let s = rows.scale;
    let value = match p.key {
        // Unset until the generator picks them: the amounts used meanwhile.
        "autoHitVariation" => track.auto_hit_amount(),
        "autoNoteVariation" => track.auto_note_amount(),
        key => number(track, key),
    };
    let spec = value_spec(p, default_of(p.key, p.min), false);
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let kind =
        Kind::Value { spec, value, label: Some(p.label.to_owned()), text: p.format.value(value, ""), lfo: false };
    rows.items.push(Item::new(Some(Id::Generation(p.key)), r, kind));
}

/// The variant a melodic track plays (`""`: its style's).
pub(super) fn variant_select(rows: &mut Rows, track: &Track, state: &EditorState) {
    let key = "autoVariant";
    let r = rows.row();
    rows.label(r, "Variant", Some(key), state.selected_prop == Some(key));
    let values = variant_options(track);
    let selected = index_of(&values, &text_of(track, key));
    let choice = Choice { values, labels: Vec::new(), selected };
    rows.items.push(Item::new(Some(Id::TrackSelect(key)), rows.field(r, 120 * rows.scale), choice.kind()));
}

/// The variants a melodic track can play (`""`: its style's).
pub fn variant_options(track: &Track) -> Vec<String> {
    let kind = if instrument_family(&track.name) == InstrumentFamily::Bass { "bass" } else { "melody" };
    std::iter::once(String::new()).chain(generate::variant_names(kind).into_iter().map(str::to_owned)).collect()
}

/// A setting of `settings` by its key.
pub(super) fn compose_value(key: &str, settings: &ordrumbox_logic::compose::Settings) -> f64 {
    match key {
        "composeHitsPerMeasure" => settings.hits_per_measure,
        "composeComplexity" => settings.complexity,
        "composeSyncopation" => settings.syncopation,
        "composeCoupling" => settings.coupling,
        "composeVariation" => settings.variation,
        "composeStyleWeight" => settings.style_weight,
        _ => 0.0,
    }
}

/// One compose slider: the track's value, else the style's (its text ends
/// in `*`; a double-click gives a value back to the style); `a→b` when the
/// pattern's macros make it play `b`; the hits show the most a measure holds
/// when asked for more.
pub(super) fn compose_slider(rows: &mut Rows, track: &Track, view: &ComposeView, p: &Prop, state: &EditorState) {
    let s = rows.scale;
    let own_key = matches!(p.key, "composeStyleWeight" | "composeSeed");
    let own = track.field(p.key).and_then(|v| v.as_f64());
    let value =
        if own_key { number(track, p.key) } else { own.unwrap_or_else(|| compose_value(p.key, &view.shown.base)) };
    let mut text = p.format.value(value, "");
    if !own_key {
        let used = compose_value(p.key, &view.shown.used);
        let used = if p.key == "composeHitsPerMeasure" { used.min(view.shown.open_hits as f64) } else { used };
        if (used - value).abs() >= 0.5 {
            text = format!("{text}\u{2192}{}", num::text(used.round()));
        }
        if own.is_none() {
            text.push('*');
        }
    }
    let default = if own_key { default_of(p.key, p.min) } else { STYLE_DEFAULT };
    let r = rows.row();
    if state.selected_prop == Some(p.key) {
        rows.items.push(Item::new(None, Rect::new(r.x - 2 * s, r.y, r.w + 4 * s, r.h), Kind::Highlight));
    }
    let spec = value_spec(p, default, false);
    let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text, lfo: false };
    rows.items.push(Item::new(Some(Id::Compose(p.key)), r, kind));
}

/// `NOTE_DEFAULTS[key]`, what a double-click restores.
pub(super) fn note_default(key: &str, min: f64) -> f64 {
    Note::default().field(key).and_then(|v| v.as_f64()).unwrap_or(min)
}

/// The note editor in `area`, a strip under the grid or the piano roll: on
/// the left its title and its knobs, on the right its tabs and their rows.
/// It is [`note_strip_height`] high whatever the tab.
pub fn note_editor(view: &NoteView, state: &EditorState, area: Rect, scale: Scale) -> Vec<Item> {
    let s = scale;
    let left_w = area.w * NOTE_LEFT_PERCENT / 100;
    let mut rows = Rows { x: area.x, w: left_w, y: area.y, scale: s, items: Vec::new() };
    rows.title(&view.title);
    let Some(note) = &view.note else { return rows.items };
    let knobs = NOTE_KNOBS
        .iter()
        .map(|(p, unit)| {
            let value = note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min);
            let spec = value_spec(p, note_default(p.key, p.min), true);
            (Id::NoteKnob(p.key), spec, value, p.label, p.format.value(value, unit), false)
        })
        .collect();
    rows.knob_bar(knobs, NOTE_KNOBS.len() as i32);
    let gap = NOTE_COLUMN_GAP * s;
    let mut rows = Rows { x: area.x + left_w + gap, w: area.w - left_w - gap, y: area.y, scale: s, items: rows.items };
    let tabs: Vec<_> = NOTE_TABS
        .iter()
        .enumerate()
        .map(|(i, l)| Some((Id::NoteTab(i as u8), None, *l, state.note_tab == i as u8, false)))
        .collect();
    rows.tabs(&tabs);
    for row in NOTE_GROUPS[usize::from(state.note_tab).min(NOTE_GROUPS.len() - 1)] {
        match row {
            NoteRow::Slider(p) => {
                let value = if p.key == "arpRange" {
                    view.arp_range as f64
                } else {
                    note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min)
                };
                let spec = value_spec(p, note_default(p.key, p.min), false);
                rows.slider(Id::NoteSlider(p.key), spec, value, p.label, p.format.value(value, ""), false);
            }
            NoteRow::Select(key, label) => {
                let r = rows.row();
                rows.label(r, label, None, false);
                let choice = if *key == "arpScale" { &view.scales } else { &view.arp_direction };
                rows.items.push(Item::new(Some(Id::NoteSelect(key)), rows.field(r, 140 * s), choice.kind()));
            }
        }
    }
    rows.items
}

/// The share of the note editor's width its title and knobs take, in percent.
pub(super) const NOTE_LEFT_PERCENT: i32 = 40;
/// The gap between the note editor's two columns, per scale unit.
pub(super) const NOTE_COLUMN_GAP: i32 = 16;

/// The height of the note editor: the taller of its columns, the title over
/// the knobs, the tabs over the most rows a tab has.
#[must_use]
pub fn note_strip_height(scale: Scale) -> i32 {
    let mut left = Rows { x: 0, w: 0, y: 0, scale, items: Vec::new() };
    left.y += 12 * scale;
    left.y += knob_row_h(scale) + 4 * scale;
    let mut right = Rows { x: 0, w: 0, y: 0, scale, items: Vec::new() };
    right.tabs(&[]);
    for _ in 0..NOTE_GROUPS.iter().map(|rows| rows.len()).max().unwrap_or(0) {
        right.row();
    }
    left.y.max(right.y)
}

/// The kit editor's selects and knobs at `area`; the y under them.
pub fn kit_controls(view: &KitControls, area: Rect, scale: Scale) -> (Vec<Item>, i32) {
    let s = scale;
    let mut rows = Rows { x: area.x, w: area.w, y: area.y, scale: s, items: Vec::new() };
    let r = rows.row();
    rows.label(r, "Move to:", None, false);
    rows.items.push(Item::new(Some(Id::KitKit), rows.field(r, 160 * s), view.kits.kind()));
    rows.y += 4 * s;
    let knobs = KIT_KNOBS
        .iter()
        .map(|(field, p, unit, default)| {
            let value = match field {
                Field::Gain => view.gain,
                Field::Transpose => view.transpose,
                _ => view.decay,
            };
            let mut spec = value_spec(p, *default, true);
            if *field == Field::Decay {
                spec = spec.log();
            }
            (Id::KitKnob(*field), spec, value, p.label, p.format.value(value, unit), false)
        })
        .collect();
    rows.knob_bar(knobs, 4);
    let bottom = rows.y;
    (rows.items, bottom)
}
