//! The parts every card of the synth editor shares: its head (the power LED
//! on the left, then the title) and its controls in one row, each over its
//! label — the knobs, the faders (`synth_faders.rs`), the spins (four characters wide), the switches and
//! the LEDs; spins stacked in a column take their label on their left. The
//! oscillator strips and the other cards are built from them.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;

use crate::canvas::text_width;
use crate::control::{Format, Style};
use crate::panel::{Id, Item, Kind, Rect, SPIN_SCREEN_H, spin_width};
use crate::scale::Scale;
use crate::theme;

/// A knob's place in a row (at scale 1).
pub(crate) const KNOB_W: i32 = 64;
pub(crate) const KNOB_H: i32 = 56;
/// The height of a row whose knobs say their value in their tooltip only:
/// a knob over its label (at scale 1). Tall enough for a knob drawn over
/// its label, not beside it.
pub(crate) const QUIET_H: i32 = 40;
/// The height of the envelopes' faders (at scale 1): two quiet rows, the
/// noise's two lines, so that the filter's faders, in the noise's row, take
/// it too.
pub(crate) const ENVELOPE_H: i32 = 2 * QUIET_H;
/// A knob's place in a quiet row (at scale 1): as wide as its label needs,
/// the knob over it being no wider.
const QUIET_KNOB_W: i32 = 44;
/// A fader's place in a row (at scale 1): as high as a knob, narrow.
const FADER_W: i32 = 20;
/// The space between two controls (at scale 1).
pub(crate) const GAP: i32 = 8;
/// How far a screen sits below the top of a knob, so that their labels line up.
const SCREEN_TOP: i32 = 5;
/// The characters a spin's screen holds.
const SPIN_CHARS: i32 = 4;
/// The space between two screens stacked in a column, and between a stacked
/// screen's label and it (at scale 1).
const STACK_GAP: i32 = 6;
/// The height of a line of text (at scale 1).
const TEXT_H: i32 = 8;
/// The width of a switch or a LED (at scale 1).
const SWITCH_W: i32 = 40;
/// The power LED's size, and where the title starts right of it (at scale 1).
const POWER_W: i32 = 14;
const POWER_H: i32 = 13;
const TITLE_X: i32 = 18;
/// The height of a spin in a card's head (at scale 1): inside the head's 18.
const HEAD_SPIN_H: i32 = 16;

/// A control of a row: its id, what it is, its label, where it goes.
#[derive(Debug, Clone)]
pub(crate) struct Cell {
    pub id: Id,
    pub kind: Kind,
    pub label: String,
    pub place: Place,
}

/// Where a cell goes in its row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    /// Right of the cell before, its label under it.
    Beside,
    /// Under the cell before, in one column, each label on the left of its
    /// control: for spins and switches, two of which fit in a row's height.
    Under,
}

// Two stacked screens fit in a row.
const _: () = assert!(SCREEN_TOP + 2 * SPIN_SCREEN_H + STACK_GAP <= KNOB_H);
// A screen and its label fit in a quiet row.
const _: () = assert!(SCREEN_TOP + SPIN_SCREEN_H + 4 + TEXT_H <= QUIET_H);

/// The head of card `group` (index `gi` of the groups) at `(x, y)`: its
/// power LED, when it has one, then its title, in the same place on every
/// card. The x right of the title.
pub(crate) fn push_head(
    items: &mut Vec<Item>,
    draft: &Object,
    (group, gi): (&str, usize),
    (x, y): (i32, i32),
    s: Scale,
) -> i32 {
    if model::has_power(group) {
        let rect = Rect::new(x, y, POWER_W * s, POWER_H * s);
        items.push(Item::new(Some(Id::SynthPower(gi as u8)), rect, Kind::Led { on: !model::bypassed(draft, group) }));
    }
    let title = model::group_label(group);
    let title_x = x + TITLE_X * s;
    let right = title_x + text_width(&title, s);
    items.push(Item::text(title_x, y + 3 * s, title, theme::text(), s));
    right
}

/// Push the spin of `cell` into a card's head at `x`, `y` the head's top:
/// as high as the head, with no label (its tooltip names it). The x right of it.
pub(crate) fn push_head_spin(items: &mut Vec<Item>, Cell { id, kind, .. }: Cell, (x, y): (i32, i32), s: Scale) -> i32 {
    let w = screen_width(&kind, s);
    items.push(Item::new(Some(id), Rect::new(x, y, w, HEAD_SPIN_H * s), kind));
    x + w
}

/// The width of the head of card `group`: its power LED and its title.
#[must_use]
pub(crate) fn head_width(group: &str, s: Scale) -> i32 {
    TITLE_X * s + text_width(&model::group_label(group), s)
}

/// Whether a control of `kind` is a fader (a stage of an envelope, a filter's frequency).
#[must_use]
pub(crate) fn is_fader(kind: &Kind) -> bool {
    matches!(kind, Kind::Value { spec, .. } if spec.style == Style::Fader)
}

/// Push `cells` side by side from `x`, their tops at `top`, in a row
/// `row_h` high at scale 1 ([`KNOB_H`], or [`QUIET_H`]); the x right of
/// the last one (`x` itself when there is none).
pub(crate) fn push_row(
    items: &mut Vec<Item>,
    cells: impl IntoIterator<Item = Cell>,
    (x, top): (i32, i32),
    row_h: i32,
    s: Scale,
) -> i32 {
    let mut columns: Vec<Vec<Cell>> = Vec::new();
    for cell in cells {
        match (cell.place, columns.last_mut()) {
            (Place::Under, Some(column)) => column.push(cell),
            _ => columns.push(vec![cell]),
        }
    }
    let mut right = x;
    let mut next = x;
    for column in columns {
        let w = match <[Cell; 1]>::try_from(column) {
            Ok([cell]) => push_cell(items, cell, (next, top), row_h, s),
            Err(column) => push_stack(items, column, next, top, s),
        };
        right = next + w;
        next = right + GAP * s;
    }
    right
}

/// The width of a knob in a row `row_h` high at scale 1.
fn knob_width(row_h: i32, s: Scale) -> i32 {
    if row_h == QUIET_H { QUIET_KNOB_W * s } else { KNOB_W * s }
}

/// The width of the screen of `kind`, a spin or a switch: a spin holds
/// [`SPIN_CHARS`], or its longest option (the LFOs' `1/16T`), whichever it
/// shows, so that its width does not change with it.
fn screen_width(kind: &Kind, s: Scale) -> i32 {
    let Kind::Spin { labels, .. } = kind else { return SWITCH_W * s };
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    spin_width(i32::try_from(longest).unwrap_or(SPIN_CHARS).max(SPIN_CHARS), s)
}

/// Push `cell` alone in its column at `next`, its label under it; its width.
fn push_cell(
    items: &mut Vec<Item>,
    Cell { id, kind, label, .. }: Cell,
    (next, top): (i32, i32),
    row_h: i32,
    s: Scale,
) -> i32 {
    if let Kind::Value { spec, value, text, lfo, .. } = kind {
        let w = if spec.style == Style::Fader { FADER_W * s } else { knob_width(row_h, s) };
        let kind = Kind::Value { spec, value, label: Some(label), text, lfo };
        items.push(Item::new(Some(id), Rect::new(next, top, w, row_h * s), kind));
        return w;
    }
    // A knob or a fader greyed out keeps its place, its label on it.
    if let Kind::Disabled { style, .. } = kind {
        let w = if style == Style::Fader { FADER_W * s } else { knob_width(row_h, s) };
        items.push(Item::new(Some(id), Rect::new(next, top, w, row_h * s), kind));
        return w;
    }
    let control_w = screen_width(&kind, s);
    let label_w = text_width(&label, s);
    let w = control_w.max(label_w);
    let screen = Rect::new(next + (w - control_w) / 2, top + SCREEN_TOP * s, control_w, SPIN_SCREEN_H * s);
    items.push(Item::new(Some(id), screen, kind));
    items.push(Item::text(next + (w - label_w) / 2, screen.y + screen.h + 4 * s, label, theme::muted(), s));
    w
}

/// The tooltip of a control of `draft` that does not write its value
/// (field `path`, called `name` on card `group`): the card and the name,
/// then the value.
#[must_use]
pub(crate) fn value_tip(draft: &Object, (group, path): (&str, &str), name: &str) -> Option<[String; 2]> {
    let meta = model::find_meta(path);
    let value =
        model::number(draft, path).or_else(|| model::extra_value(draft, path)).or_else(|| meta.map(|m| m.default))?;
    let unit = meta.map_or("", |m| m.unit);
    Some([format!("{} {name}", model::group_label(group)), Format::Fmt.value(value, unit)])
}

/// The tooltip of any control of `draft` (field `path`): its card and the
/// name under it, then its value: a number in its unit, a switch on or off,
/// an option as the preset writes it.
#[must_use]
pub(crate) fn field_tip(draft: &Object, path: &str) -> Option<[String; 2]> {
    let card = model::borrowing_card(path).or_else(|| path.split_once('.').map(|(group, _)| group)).unwrap_or("master");
    let key = path.rsplit('.').next().unwrap_or(path);
    let name = model::find_meta(path).and_then(|m| m.label).unwrap_or(key);
    let value = match model::get(draft, path) {
        Some(Value::Bool(on)) => on_off(*on),
        Some(Value::String(text)) => text.clone(),
        None if model::SWITCHES.contains(&path) => on_off(false),
        // A number unset and with no default (the semitones) is 0, as its spin shows.
        _ => value_tip(draft, (card, path), name).map_or_else(|| Format::Fmt.value(0.0, ""), |[_, value]| value),
    };
    Some([tip_title(card, name), value])
}

/// The first line of a tooltip: the card's title and the control's name
/// (capitalized), the title alone when the name repeats it (FM's amount).
fn tip_title(card: &str, name: &str) -> String {
    let title = model::group_label(card);
    if title.eq_ignore_ascii_case(name) {
        return title;
    }
    let mut chars = name.chars();
    let first = chars.next().map(|c| c.to_uppercase().collect::<String>()).unwrap_or_default();
    format!("{title} {first}{}", chars.as_str())
}

/// A switch's state, as a tooltip says it.
fn on_off(on: bool) -> String {
    if on { "On" } else { "Off" }.to_owned()
}

/// Push `column`, spins or switches one under the other at `next`, each
/// label on the left of its screen; its width.
fn push_stack(items: &mut Vec<Item>, column: Vec<Cell>, next: i32, top: i32, s: Scale) -> i32 {
    let label_w = column.iter().map(|c| text_width(&c.label, s)).max().unwrap_or(0);
    let control_w = column.iter().map(|c| screen_width(&c.kind, s)).max().unwrap_or(0);
    let screen_x = next + label_w + STACK_GAP * s;
    let mut y = top + SCREEN_TOP * s;
    for Cell { id, kind, label, .. } in column {
        let screen = Rect::new(screen_x, y, screen_width(&kind, s), SPIN_SCREEN_H * s);
        items.push(Item::new(Some(id), screen, kind));
        items.push(Item::text(next, y + (SPIN_SCREEN_H - TEXT_H) * s / 2, label, theme::muted(), s));
        y += (SPIN_SCREEN_H + STACK_GAP) * s;
    }
    label_w + STACK_GAP * s + control_w
}
