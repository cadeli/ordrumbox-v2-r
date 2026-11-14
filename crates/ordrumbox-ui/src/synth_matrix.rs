//! The synth editor's Matrix card: one line per route, a source, a target
//! and an amount, each a field of the preset's `matrix` group.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model::{self as model, MATRIX_SIZE, MATRIX_SOURCES};

use crate::control::{Format, Spec};
use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;

/// Widths of a line's source and target lists, scale units; the amount takes the rest.
const SOURCE_W: i32 = 62;
const TARGET_W: i32 = 120;
const GAP: i32 = 4;
/// A line's height, with the space under it.
const LINE_H: i32 = 16;

/// The list of field `path`: its options as (value, label), `NOT` shown as "off".
fn select(draft: &Object, path: &str, options: &[&str]) -> Kind {
    let label = |option: &str| if option == "NOT" { "off".to_owned() } else { option.to_owned() };
    let current = model::get(draft, path).and_then(Value::to_text).unwrap_or_default();
    Kind::Select {
        values: options.iter().map(|&option| option.to_owned()).collect(),
        labels: options.iter().map(|&option| label(option)).collect(),
        selected: options.iter().position(|&option| option == current).unwrap_or(0),
    }
}

/// Push the lines of the matrix, from `inner`'s left at height `y`; the height under them.
/// `fields` is [`super::synth_view::all_fields`], where a field's place is its id.
pub(super) fn push_lines(
    items: &mut Vec<Item>,
    draft: &Object,
    fields: &[String],
    inner: Rect,
    y: i32,
    s: Scale,
) -> i32 {
    let targets = model::lfo_targets();
    let field_id = |name: &str, n: usize| {
        let path = format!("matrix.{name}{n}");
        fields.iter().position(|f| *f == path).map(|fi| Id::SynthField(fi as u16))
    };
    let amount_x = inner.x + (SOURCE_W + TARGET_W + 2 * GAP) * s;
    let amount_w = (inner.w - (SOURCE_W + TARGET_W + 2 * GAP) * s).max(1);
    let spec = Spec::slider(-1.0, 1.0, 0.01, 0.0);
    for n in 1..=MATRIX_SIZE {
        let top = y + (n as i32 - 1) * (LINE_H + 2) * s;
        let source = Rect::new(inner.x, top, SOURCE_W * s, 13 * s);
        let target = Rect::new(inner.x + (SOURCE_W + GAP) * s, top, TARGET_W * s, 13 * s);
        let amount = Rect::new(amount_x, top, amount_w, 13 * s);
        items.push(Item::new(
            field_id("source", n),
            source,
            select(draft, &format!("matrix.source{n}"), &MATRIX_SOURCES),
        ));
        items.push(Item::new(field_id("target", n), target, select(draft, &format!("matrix.target{n}"), &targets)));
        let value = model::number(draft, &format!("matrix.amount{n}")).unwrap_or(0.0).clamp(spec.min, spec.max);
        let text = Format::Fmt.value(value, "");
        items.push(Item::new(
            field_id("amount", n),
            amount,
            Kind::Value { spec, value, label: None, text, lfo: false },
        ));
    }
    y + MATRIX_SIZE as i32 * (LINE_H + 2) * s
}
