//! The editors laid out as panel items.

use super::TrackView;
use super::tables::DEFAULT_DECAY;
use super::tables::{KNOB_BAR, Prop, TABS};
use super::tables::{TAB_FX, TAB_LFO, TAB_LOOP, TAB_SOUND};
use super::tabs::{fx_tab, generation_tab, lfo_tab, loop_tab, sound_tab, tallest_tab_bottom};
use super::values::{EditorState, has_lfo, number};
use crate::control::{Format, Spec};
use crate::panel::{Id, Item, KNOB_SIZE, Kind, Rect, Step, dim, label_width};
use crate::scale::Scale;
use crate::theme;
use ordrumbox_core::Track;

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

/// Width of a cartridge's LED, and of a screen's arrow (at scale 1).
const LED_W: i32 = 10;
const ARROW_W: i32 = 10;

/// A tab: its id, its LED's id, its label, active, LED on.
type TabSlot<'a> = (Id, Option<Id>, &'a str, bool, bool);

/// Places rows top to bottom in a column.
pub(super) struct Rows {
    pub(super) x: i32,
    pub(super) w: i32,
    pub(super) y: i32,
    pub(super) scale: Scale,
    pub(super) items: Vec<Item>,
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

    /// A cartridge's head: its `frame`, then on `line` its LED (`led`, lit
    /// when `on`) and its name, dimmed when it is off.
    pub(super) fn cartridge_head(&mut self, frame: Rect, line: Rect, (led, on): (Id, bool), name: &str) {
        let s = self.scale;
        self.items.push(Item::new(None, frame, Kind::Card));
        self.items.push(Item::new(Some(led), Rect::new(line.x, line.y, LED_W * s, line.h), Kind::Led { on }));
        let color = if on { theme::text() } else { dim() };
        self.items.push(Item::text(line.x + (LED_W + 2) * s, line.y + (line.h - 8 * s) / 2, name, color, s));
    }

    /// A one-line `screen` (its id and its kind) between the arrows that step
    /// through its choice (`step`: an arrow's id), across `r`.
    pub(super) fn screen_arrows(&mut self, r: Rect, (id, screen): (Id, Kind), step: impl Fn(Step) -> Id) {
        let s = self.scale;
        let arrow = ARROW_W * s;
        let previous = Rect::new(r.x, r.y, arrow, r.h);
        let next = Rect::new(r.x + r.w - arrow, r.y, arrow, r.h);
        for (direction, rect, glyph) in [(Step::Previous, previous, "\u{25c2}"), (Step::Next, next, "\u{25b8}")] {
            let kind = Kind::Button { label: glyph.into(), on: false };
            self.items.push(Item::new(Some(step(direction)), rect, kind));
        }
        let rect = Rect::new(r.x + arrow + 2 * s, r.y, r.w - 2 * (arrow + 2 * s), r.h);
        self.items.push(Item::new(Some(id), rect, screen));
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
    let (points, overlay) = view.sample.as_ref().map_or((Vec::new(), None), |bar| (bar.envelope.clone(), bar.overlay));
    rows.items.push(Item::new(None, Rect::new(area.x, rows.y, area.w, 32 * s), Kind::Envelope { points, overlay }));
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
                let moved = has_lfo(track, p.key) || lfo_now(p.key).is_some();
                (Id::KnobBar(p.key), spec, value, p.label, p.format.value(value, unit), moved)
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
        TAB_FX => fx_tab(&mut rows, view, state),
        TAB_SOUND => sound_tab(&mut rows, view),
        TAB_LFO => lfo_tab(&mut rows, view),
        TAB_LOOP => loop_tab(&mut rows, track),
        _ => generation_tab(&mut rows, track, view.compose.as_ref(), state),
    }
    TrackEditor { items: rows.items, sound_bottom, tabs_top, bottom }
}
