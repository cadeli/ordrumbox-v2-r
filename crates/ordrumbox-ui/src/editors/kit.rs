//! The kit editor's controls as panel items, and the playback controls of a
//! sample it shares with the track editor.

use super::tables::DEFAULT_ATTACK;
use super::tables::{KIT_KNOBS, SHAPE_KNOBS, SHAPE_RANGES, SHAPE_SLOTS, SHAPE_SWITCHES};
use super::{KitControls, Shape};
use crate::control::{Format, Spec};
use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;
use ordrumbox_engine::kit::edit::SampleField;

use super::layout::*;

/// Knobs in a row of the kit editor.
const KIT_KNOB_SLOTS: usize = 5;

/// The kit editor's selects and knobs at `area`.
pub fn kit_controls(view: &KitControls, area: Rect, scale: Scale) -> Vec<Item> {
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
    rows.items
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
