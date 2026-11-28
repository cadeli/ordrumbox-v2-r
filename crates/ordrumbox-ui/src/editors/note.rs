//! The note editor as panel items, in the synth editor's hardware look: one
//! row of cards, the note's own (its knobs) then one a group of settings
//! (trigger, retrigger, euclidean, arpeggio, timing), all on one screen. A
//! value that runs on is a vertical fader, a whole number or a choice a spin.

use super::NoteView;
use super::layout::knob_row_h;
use super::tables::{NOTE_CARDS, NOTE_GROUPS, NOTE_KNOBS, NoteRow};
use crate::canvas::{fit, text_width};
use crate::control::{Format, Spec};
use crate::panel::hardware::CARD_HEAD;
use crate::panel::{Id, Item, Kind, Rect, SPIN_SCREEN_H, Skin, spin_width};
use crate::scale::Scale;
use crate::theme;
use ordrumbox_core::Note;

/// `NOTE_DEFAULTS[key]`, what a double-click restores.
pub(super) fn note_default(key: &str, min: f64) -> f64 {
    Note::default().field(key).and_then(|v| v.as_f64()).unwrap_or(min)
}

/// The space inside a card around its controls, and between two cards (at scale 1).
const PAD: i32 = 6;
const GAP: i32 = 8;
/// The space under a card's title band, and the height of its controls (at scale 1).
const HEAD_GAP: i32 = 4;
const CONTENT_H: i32 = 66;
/// The width of a knob's column and of a fader's (at scale 1).
const KNOB_W: i32 = 46;
const FADER_W: i32 = 36;
/// The line of a fader's value under it (at scale 1).
const VALUE_LINE: i32 = 10;
/// A spin's label line, and the space between two spins of a column (at scale 1).
const SPIN_LABEL_H: i32 = 11;
const SPIN_GAP: i32 = 4;
/// The characters a spin's screen holds: a count's, a choice's.
const COUNT_CHARS: i32 = 3;
const CHOICE_CHARS: i32 = 9;
/// The spins a column holds, one under the other.
const SPINS_PER_COLUMN: usize = 2;

/// A column of a card.
#[derive(Debug, Clone, Copy)]
enum Column {
    /// A knob of [`NOTE_KNOBS`], by its index.
    Knob(usize),
    Fader(NoteRow),
    Spins(&'static [NoteRow]),
}

impl Column {
    /// Its width at `s`: its control's, or its widest label's.
    fn width(self, s: Scale) -> i32 {
        match self {
            Self::Knob(k) => (KNOB_W * s).max(NOTE_KNOBS.get(k).map_or(0, |(p, _)| text_width(p.label, s) + 4 * s)),
            Self::Fader(row) => (FADER_W * s).max(text_width(row.label(), s) + 4 * s),
            Self::Spins(rows) => {
                rows.iter().map(|row| spin_w(row, s).max(text_width(row.label(), s))).max().unwrap_or(0)
            }
        }
    }
}

/// The width of the spin of `row`: a count's or a choice's, whatever it shows.
fn spin_w(row: &NoteRow, s: Scale) -> i32 {
    spin_width(if matches!(row, NoteRow::Select(..)) { CHOICE_CHARS } else { COUNT_CHARS }, s)
}

/// The columns of a group's card: a fader each, the other controls by two in
/// a column of spins.
fn columns(rows: &'static [NoteRow]) -> Vec<Column> {
    let mut columns = Vec::new();
    let mut rest = rows;
    while let Some((first, after)) = rest.split_first() {
        if matches!(first, NoteRow::Fader(_)) {
            columns.push(Column::Fader(*first));
            rest = after;
            continue;
        }
        let spins = rest.iter().take(SPINS_PER_COLUMN).take_while(|row| !matches!(row, NoteRow::Fader(_))).count();
        let (column, after) = rest.split_at(spins);
        columns.push(Column::Spins(column));
        rest = after;
    }
    columns
}

/// The cards, left to right: their titles (`None`: the note's own, which
/// shows the note) and their columns.
fn cards() -> Vec<(Option<&'static str>, Vec<Column>)> {
    let note = (None, (0..NOTE_KNOBS.len()).map(Column::Knob).collect());
    let groups = NOTE_CARDS.iter().zip(NOTE_GROUPS).map(|(title, rows)| (Some(*title), columns(rows)));
    std::iter::once(note).chain(groups).collect()
}

/// The narrowest a card is: its columns, or its title.
fn card_width(title: Option<&str>, columns: &[Column], s: Scale) -> i32 {
    let content: i32 = columns.iter().map(|c| c.width(s)).sum();
    (content + 2 * PAD * s).max(title.map_or(0, |t| text_width(t, s) + 2 * PAD * s))
}

/// The note editor in `area`, a strip under the grid or the piano roll: its
/// cards side by side, widened together to fill it. They stay put whatever
/// the note: without one, they are empty.
pub fn note_editor(view: &NoteView, area: Rect, scale: Scale) -> Vec<Item> {
    let s = scale;
    let cards = cards();
    let widths: Vec<i32> = cards.iter().map(|(title, columns)| card_width(*title, columns, s)).collect();
    let n = widths.len() as i32;
    let extra = (area.w - widths.iter().sum::<i32>() - (n - 1) * GAP * s).max(0);
    let mut items = Vec::new();
    let mut x = area.x;
    for (i, ((title, columns), natural)) in (0..n).zip(cards.iter().zip(widths)) {
        let w = natural + extra * (i + 1) / n - extra * i / n;
        let card = Rect::new(x, area.y, w, area.h);
        items.push(Item::new(None, card, Kind::Card).skinned(Skin::Card));
        let title = title.unwrap_or(&view.title);
        let title_y = area.y + (CARD_HEAD * s - 8 * s) / 2 + s.px();
        items.push(Item::text(x + PAD * s, title_y, fit(title, w - 2 * PAD * s, s), theme::text(), s));
        if let Some(note) = &view.note {
            card_controls(&mut items, (view, note), columns, card, s);
        }
        x += w + GAP * s;
    }
    items
}

/// The controls of `columns` across `card`, each centred in its share of the
/// card's width.
fn card_controls(items: &mut Vec<Item>, shown: (&NoteView, &Note), columns: &[Column], card: Rect, s: Scale) {
    let inner = Rect::new(card.x + PAD * s, card.y + (CARD_HEAD + HEAD_GAP) * s, card.w - 2 * PAD * s, CONTENT_H * s);
    let natural: i32 = columns.iter().map(|c| c.width(s)).sum();
    let (extra, n) = ((inner.w - natural).max(0), columns.len().max(1) as i32);
    let mut x = inner.x;
    for (i, column) in (0..n).zip(columns) {
        let w = column.width(s) + extra * (i + 1) / n - extra * i / n;
        let slot = Rect::new(x, inner.y, w, inner.h);
        match *column {
            Column::Knob(k) => items.push(knob(k, shown.1, slot, s)),
            Column::Fader(row) => fader(items, row, shown.1, slot, s),
            Column::Spins(rows) => {
                let mut y = slot.y;
                for row in rows {
                    let spin_w = spin_w(row, s);
                    let spin_x = slot.x + (slot.w - spin_w) / 2;
                    items.push(Item::text(
                        spin_x,
                        y,
                        fit(row.label(), slot.w - (spin_x - slot.x), s),
                        theme::text(),
                        s,
                    ));
                    let rect = Rect::new(spin_x, y + SPIN_LABEL_H * s, spin_w, SPIN_SCREEN_H * s);
                    items.push(Item::new(Some(spin_id(row)), rect, spin_kind(row, shown)).skinned(Skin::Card));
                    y += (SPIN_LABEL_H + SPIN_SCREEN_H + SPIN_GAP) * s;
                }
            }
        }
        x += w;
    }
}

/// Knob `k` of [`NOTE_KNOBS`], at the middle of `slot`.
fn knob(k: usize, note: &Note, slot: Rect, s: Scale) -> Item {
    let (p, unit) = &NOTE_KNOBS[k.min(NOTE_KNOBS.len() - 1)];
    let value = note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min);
    let spec = Spec::knob(p.min, p.max, p.step, note_default(p.key, p.min));
    let spec = if p.format == Format::Percent { spec.percent() } else { spec };
    let kind =
        Kind::Value { spec, value, label: Some(p.label.to_owned()), text: p.format.value(value, unit), lfo: false };
    let w = Column::Knob(k).width(s);
    let rect = Rect::new(slot.x + (slot.w - w) / 2, slot.y + (slot.h - knob_row_h(s)) / 2, w, knob_row_h(s));
    Item::new(Some(Id::NoteKnob(p.key)), rect, kind).skinned(Skin::Card)
}

/// The fader of `row` down `slot`, its label under it, then its value.
fn fader(items: &mut Vec<Item>, row: NoteRow, note: &Note, slot: Rect, s: Scale) {
    let Some(p) = row.prop() else { return };
    let value = note.field(p.key).and_then(|v| v.as_f64()).unwrap_or(p.min);
    let spec = Spec::fader(p.min, p.max, p.step, note_default(p.key, p.min));
    let spec = if p.format == Format::Percent { spec.percent() } else { spec };
    let kind = Kind::Value { spec, value, label: Some(p.label.to_owned()), text: String::new(), lfo: false };
    let rect = Rect::new(slot.x, slot.y, slot.w, slot.h - VALUE_LINE * s);
    items.push(Item::new(Some(Id::NoteFader(p.key)), rect, kind).skinned(Skin::Card));
    let text = fit(&p.format.value(value, ""), slot.w, s);
    let text_x = slot.x + (slot.w - text_width(&text, s)) / 2;
    items.push(Item::text(text_x, slot.y + slot.h - 8 * s, text, theme::text(), s));
}

/// The id of the spin of `row`: a count's, or a choice's.
fn spin_id(row: &NoteRow) -> Id {
    match row {
        NoteRow::Select(key, _) => Id::NoteSelect(key),
        _ => Id::NoteSpin(row.key()),
    }
}

/// The spin of `row`: the whole numbers of its range, or the choice it makes.
fn spin_kind(row: &NoteRow, (view, note): (&NoteView, &Note)) -> Kind {
    match row {
        NoteRow::Select(key, _) => if *key == "arpScale" { &view.scales } else { &view.arp_direction }.spin(),
        NoteRow::Count(p) | NoteRow::Fader(p) => {
            let value = match p.key {
                "arpRange" => view.arp_range as f64,
                "arpOctaves" => view.arp_octaves as f64,
                key => note.field(key).and_then(|v| v.as_f64()).unwrap_or(p.min),
            };
            // Whole numbers of a short range: rounding is meant.
            let (low, high) = (p.min.round() as i32, p.max.round() as i32);
            let current = value.round().clamp(p.min, p.max) as i32;
            let values: Vec<String> = (low..=high).map(|v| v.to_string()).collect();
            Kind::spin(values.clone(), values, usize::try_from(current - low).unwrap_or(0))
        }
    }
}

/// The height of the note editor: a card's title band over its controls.
#[must_use]
pub fn note_strip_height(scale: Scale) -> i32 {
    (CARD_HEAD + HEAD_GAP + CONTENT_H + PAD) * scale
}

/// The narrowest the note editor's cards fit in, side by side.
#[cfg(test)]
#[must_use]
pub fn note_strip_width(scale: Scale) -> i32 {
    let cards = cards();
    let widths: i32 = cards.iter().map(|(title, columns)| card_width(*title, columns, scale)).sum();
    widths + (cards.len() as i32 - 1) * GAP * scale
}
