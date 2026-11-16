//! The note editor and the kit editor's controls as panel items.

use super::tables::DEFAULT_ATTACK;
use super::tables::{
    KIT_KNOBS, NOTE_GROUPS, NOTE_KNOBS, NOTE_TABS, NoteRow, SHAPE_KNOBS, SHAPE_RANGES, SHAPE_SWITCHES,
};
use super::values::EditorState;
use super::{KitControls, NoteView, Shape};
use crate::control::{Format, Spec};
use crate::kits::Field;
use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;
use ordrumbox_core::Note;

use super::layout::*;

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
                } else if p.key == "arpOctaves" {
                    view.arp_octaves as f64
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
    shape_controls(&mut rows, &view.shape, (Id::KitKnob, Id::KitToggle));
    let bottom = rows.y;
    (rows.items, bottom)
}

/// The id of a playback control, by its field.
pub(super) type IdOf = fn(Field) -> Id;

/// The playback controls of a sample (start, end, attack, hold, and the
/// reverse and loop switches), whose ids the editor gives: the kit editor's
/// and the track editor's act on the same settings, each on its sample.
pub(super) fn shape_controls(rows: &mut Rows, shape: &Shape, (knob_id, toggle_id): (IdOf, IdOf)) {
    let s = rows.scale;
    // The length of the sample is the range of the start and the end; without
    // one (a synth track) the knobs keep their place, at nothing.
    let length = shape.duration_ms.max(1.0);
    let knobs = SHAPE_KNOBS
        .iter()
        .map(|&(field, label, step)| {
            let (value, spec) = match field {
                Field::Start => (shape.start_ms, Spec::knob(0.0, length, step, 0.0)),
                Field::End => (shape.end_ms, Spec::knob(0.0, length, step, length)),
                Field::Attack => (shape.attack_ms, Spec::knob(0.0, SHAPE_RANGES.0, step, DEFAULT_ATTACK)),
                _ => (shape.hold_ms, Spec::knob(0.0, SHAPE_RANGES.1, step, 0.0)),
            };
            (knob_id(field), spec, value, label, Format::Millis.value(value, ""), false)
        })
        .collect();
    rows.knob_bar(knobs, 4);
    for (field, label) in SHAPE_SWITCHES {
        let on = match field {
            Field::Reverse => shape.reverse,
            Field::Loop => shape.looping,
            _ => shape.gated,
        };
        let r = rows.row();
        rows.label(r, label, None, false);
        let switch = rows.field(r, 36 * s);
        rows.items.push(Item::new(Some(toggle_id(field)), switch, Kind::Toggle { on }));
    }
}
