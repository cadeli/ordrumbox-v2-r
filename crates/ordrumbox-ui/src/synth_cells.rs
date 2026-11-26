//! The parts every card of the synth editor shares: its head (the power LED
//! on the left, then the title) and its controls in one row, each over its
//! label — the knobs, the spins (four characters wide), the switches and
//! the LEDs. The oscillator strips and the other cards are built from them.

use ordrumbox_core::json::Object;
use ordrumbox_engine::synth_model as model;

use crate::canvas::text_width;
use crate::panel::{Id, Item, Kind, Rect, SPIN_SCREEN_H, spin_width};
use crate::scale::Scale;
use crate::theme;

/// A knob's place in a row (at scale 1).
pub(crate) const KNOB_W: i32 = 64;
pub(crate) const KNOB_H: i32 = 56;
/// The space between two controls (at scale 1).
const GAP: i32 = 8;
/// How far a screen sits below the top of a knob, so that their labels line up.
const SCREEN_TOP: i32 = 5;
/// The characters a spin's screen holds.
const SPIN_CHARS: i32 = 4;
/// The width of a switch or a LED (at scale 1).
const SWITCH_W: i32 = 40;
/// The power LED's size, and where the title starts right of it (at scale 1).
const POWER_W: i32 = 14;
const POWER_H: i32 = 13;
const TITLE_X: i32 = 18;

/// A control of a row: its id, what it is, the label under it.
#[derive(Debug)]
pub(crate) struct Cell {
    pub id: Id,
    pub kind: Kind,
    pub label: String,
}

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

/// Push `cells` side by side from `x`, their tops at `top`; the x right of
/// the last one (`x` itself when there is none). A row is [`KNOB_H`] high.
pub(crate) fn push_row(
    items: &mut Vec<Item>,
    cells: impl IntoIterator<Item = Cell>,
    x: i32,
    top: i32,
    s: Scale,
) -> i32 {
    let mut right = x;
    let mut next = x;
    for Cell { id, kind, label } in cells {
        let w = if let Kind::Value { spec, value, text, lfo, .. } = kind {
            let kind = Kind::Value { spec, value, label: Some(label), text, lfo };
            items.push(Item::new(Some(id), Rect::new(next, top, KNOB_W * s, KNOB_H * s), kind));
            KNOB_W * s
        } else {
            let control_w = if matches!(kind, Kind::Spin { .. }) { spin_width(SPIN_CHARS, s) } else { SWITCH_W * s };
            let label_w = text_width(&label, s);
            let w = control_w.max(label_w);
            let screen = Rect::new(next + (w - control_w) / 2, top + SCREEN_TOP * s, control_w, SPIN_SCREEN_H * s);
            items.push(Item::new(Some(id), screen, kind));
            items.push(Item::text(next + (w - label_w) / 2, screen.y + screen.h + 4 * s, label, theme::muted(), s));
            w
        };
        right = next + w;
        next = right + GAP * s;
    }
    right
}
