//! The note editor and the kit editor's controls as panel items.

use super::tables::DEFAULT_ATTACK;
use super::tables::{
    KIT_KNOBS, NOTE_GROUPS, NOTE_KNOBS, NOTE_TABS, NoteRow, SHAPE_KNOBS, SHAPE_RANGES, SHAPE_SLOTS, SHAPE_SWITCHES,
};
use super::values::EditorState;
use super::{KitControls, NoteView, Shape};
use crate::control::{Format, Spec};
use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;
use ordrumbox_core::Note;
use ordrumbox_engine::kit::edit::SampleField;

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
                // A short screen between its arrows, lit while the arpeggio plays (a range).
                let r = rows.row();
                rows.label(r, label, None, false);
                let choice = if *key == "arpScale" { &view.scales } else { &view.arp_direction };
                let screen = (Id::NoteSelect(key), choice.screen(view.arp_range > 0));
                rows.screen_arrows(rows.field(r, 140 * s), screen, |step| Id::NoteSelectStep(key, step));
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
/// Knobs in a row of the kit editor.
const KIT_KNOB_SLOTS: usize = 5;

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
                SampleField::Gain => view.gain,
                SampleField::Transpose => view.transpose,
                _ => view.decay,
            };
            let mut spec = value_spec(p, *default, true);
            if *field == SampleField::Decay {
                spec = spec.log();
            }
            (Id::KitKnob(*field), spec, value, p.label, p.format.value(value, unit), false)
        })
        .chain(shape_knobs(&view.shape, Id::KitKnob))
        .collect();
    // The kit editor is wider than the track editor: its knobs go five a row.
    knob_rows(&mut rows, knobs, KIT_KNOB_SLOTS);
    shape_switches(&mut rows, &view.shape, Id::KitToggle);
    let bottom = rows.y;
    (rows.items, bottom)
}

/// The id of a playback control, by its field.
pub(super) type IdOf = fn(SampleField) -> Id;

/// A knob of the knob bars: its id, range, value, label, text, and whether an LFO moves it.
type Knob<'a> = (Id, Spec, f64, &'a str, String, bool);

/// The playback knobs of a sample (start, end, attack, hold, the loop's
/// points), whose ids `knob_id` gives.
fn shape_knobs(shape: &Shape, knob_id: IdOf) -> Vec<Knob<'static>> {
    // The length of the sample is the range of the start, the end and the
    // loop's points; without one (a synth track) the knobs keep their place, at nothing.
    let length = shape.duration_ms.max(1.0);
    SHAPE_KNOBS
        .iter()
        .map(|&(field, label, step)| {
            let (value, spec) = match field {
                SampleField::Start => (shape.start_ms, Spec::knob(0.0, length, step, 0.0)),
                SampleField::End => (shape.end_ms, Spec::knob(0.0, length, step, length)),
                SampleField::LoopStart => (shape.loop_start_ms, Spec::knob(0.0, length, step, 0.0)),
                SampleField::LoopEnd => (shape.loop_end_ms, Spec::knob(0.0, length, step, length)),
                SampleField::Attack => (shape.attack_ms, Spec::knob(0.0, SHAPE_RANGES.0, step, DEFAULT_ATTACK)),
                _ => (shape.hold_ms, Spec::knob(0.0, SHAPE_RANGES.1, step, 0.0)),
            };
            (knob_id(field), spec, value, label, Format::Millis.value(value, ""), false)
        })
        .collect()
}

/// `knobs` in rows of `slots`.
fn knob_rows(rows: &mut Rows, knobs: Vec<Knob<'_>>, slots: usize) {
    let mut knobs = knobs.into_iter().peekable();
    while knobs.peek().is_some() {
        rows.knob_bar(knobs.by_ref().take(slots).collect(), slots as i32);
    }
}

/// The playback controls of a sample (start, end, attack, hold, the loop's
/// points, and the reverse, loop and gate switches), whose ids the editor
/// gives: the kit editor's and the track editor's act on the same settings,
/// each on its sample.
pub(super) fn shape_controls(rows: &mut Rows, shape: &Shape, (knob_id, toggle_id): (IdOf, IdOf)) {
    knob_rows(rows, shape_knobs(shape, knob_id), SHAPE_SLOTS);
    shape_switches(rows, shape, toggle_id);
}

/// The reverse, loop and gate switches of a sample, one a row.
fn shape_switches(rows: &mut Rows, shape: &Shape, toggle_id: IdOf) {
    let s = rows.scale;
    for (field, label) in SHAPE_SWITCHES {
        let on = match field {
            SampleField::Reverse => shape.reverse,
            SampleField::Loop => shape.looping,
            _ => shape.gated,
        };
        let r = rows.row();
        rows.label(r, label, None, false);
        let switch = rows.field(r, 36 * s);
        rows.items.push(Item::new(Some(toggle_id(field)), switch, Kind::Toggle { on }));
    }
}
