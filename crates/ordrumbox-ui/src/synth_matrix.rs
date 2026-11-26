//! The synth editor's Matrix card: a line per route in use and one free
//! line to start a new one, each a source and a target (spins) and an
//! amount, fields of the preset's `matrix` group; [`LINES_SHOWN`] of them
//! at once, the others along a scroll bar.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_engine::synth_preset::MATRIX_SOURCES;

use crate::control::{Format, Spec};
use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;
use crate::scroll_thumb;

/// Widths of a line's source and target lists and of its amount, scale units.
const SOURCE_W: i32 = 62;
const TARGET_W: i32 = 120;
const AMOUNT_W: i32 = 96;
const GAP: i32 = 4;
/// A line's height, with the space under it.
const LINE_H: i32 = 16;

/// The spin of list field `path`: its options as (value, label), `NOT` shown as "off".
fn spin(draft: &Object, path: &str, options: &[&str]) -> Kind {
    let label = |option: &str| if option == "NOT" { "off".to_owned() } else { option.to_owned() };
    let current = model::get(draft, path).and_then(Value::to_text).unwrap_or_default();
    Kind::spin(
        options.iter().map(|&option| option.to_owned()).collect(),
        options.iter().map(|&option| label(option)).collect(),
        options.iter().position(|&option| option == current).unwrap_or(0),
    )
}

/// The lines the card shows at once; more scroll, along a bar beside them.
pub const LINES_SHOWN: usize = 6;
/// The scroll bar's width and the space left of it (scale units), its shortest thumb.
const BAR_W: i32 = 6;
const BAR_GAP: i32 = 4;
const MIN_THUMB: i32 = 10;

/// The lines of the matrix listed (those in use, then a free one), from
/// the first one scrolled to, and the scroll bar beside them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatrixList {
    /// The scroll bar, its room kept whether it scrolls or not.
    pub bar: Rect,
    /// The first line listed that is shown.
    pub first: usize,
    /// How many lines are listed.
    pub total: usize,
    scale: Scale,
}

impl MatrixList {
    /// More lines are listed than shown: the scroll bar is there.
    #[must_use]
    pub const fn scrolls(&self) -> bool {
        self.total > LINES_SHOWN
    }

    /// The furthest the list scrolls to.
    #[must_use]
    pub const fn last_first(&self) -> usize {
        self.total.saturating_sub(LINES_SHOWN)
    }

    /// Start and length of the thumb, down the bar.
    #[must_use]
    pub fn thumb(&self) -> (i32, i32) {
        scroll_thumb::thumb((self.bar.y, self.bar.h), self.first, LINES_SHOWN, self.total, MIN_THUMB * self.scale)
    }

    /// The first line that puts the thumb centred on `y`.
    #[must_use]
    pub fn first_at(&self, y: i32) -> usize {
        scroll_thumb::first_at((self.bar.y, self.bar.h), y, LINES_SHOWN, self.total, MIN_THUMB * self.scale)
    }

    /// The same, moved by `(x, y)`.
    #[must_use]
    pub const fn moved(self, x: i32, y: i32) -> Self {
        Self { bar: self.bar.moved(x, y), ..self }
    }
}

/// Push the lines of the matrix listed from `(x, y)`, from line `first` of
/// them (kept in range); the x right of the card's content, the y under it
/// (the same whatever is listed), and the list.
/// `fields` is [`super::synth_view::all_fields`], where a field's place is its id.
pub(super) fn push_lines(
    items: &mut Vec<Item>,
    draft: &Object,
    fields: &[String],
    (x, y, first): (i32, i32, usize),
    s: Scale,
) -> (i32, i32, MatrixList) {
    let targets = model::lfo_targets();
    let field_id = |name: &str, n: usize| {
        let path = format!("matrix.{name}{n}");
        fields.iter().position(|f| *f == path).map(|fi| Id::SynthField(fi as u16))
    };
    let listed = model::matrix_lines_listed(draft);
    let first = first.min(listed.len().saturating_sub(LINES_SHOWN));
    let amount_x = x + (SOURCE_W + TARGET_W + 2 * GAP) * s;
    let spec = Spec::slider(-1.0, 1.0, 0.01, 0.0);
    for (slot, n) in listed.iter().skip(first).take(LINES_SHOWN).enumerate() {
        let top = y + slot as i32 * (LINE_H + 2) * s;
        let source = Rect::new(x, top, SOURCE_W * s, 13 * s);
        let target = Rect::new(x + (SOURCE_W + GAP) * s, top, TARGET_W * s, 13 * s);
        let amount = Rect::new(amount_x, top, AMOUNT_W * s, 13 * s);
        items.push(Item::new(
            field_id("source", *n),
            source,
            spin(draft, &format!("matrix.source{n}"), &MATRIX_SOURCES),
        ));
        items.push(Item::new(field_id("target", *n), target, spin(draft, &format!("matrix.target{n}"), &targets)));
        let value = model::number(draft, &format!("matrix.amount{n}")).unwrap_or(0.0).clamp(spec.min, spec.max);
        let text = Format::Fmt.value(value, "");
        items.push(Item::new(
            field_id("amount", *n),
            amount,
            Kind::Value { spec, value, label: None, text, lfo: false },
        ));
    }
    let lines_h = LINES_SHOWN as i32 * (LINE_H + 2) * s;
    let bar = Rect::new(amount_x + (AMOUNT_W + BAR_GAP) * s, y, BAR_W * s, lines_h - (LINE_H + 2 - 13) * s);
    let list = MatrixList { bar, first, total: listed.len(), scale: s };
    (bar.x + bar.w, y + lines_h, list)
}
