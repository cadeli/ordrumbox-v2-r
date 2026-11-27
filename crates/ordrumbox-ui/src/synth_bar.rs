//! The synth editor's preset bar, above the cards: the preset list, its
//! buttons (delete, new, duplicate, revert, tags, export, import).

use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;

/// The bar's buttons: id, label, width (scale units).
pub const BUTTONS: [(&str, &str, i32); 7] = [
    ("delete", "x", 20),
    ("new", "+", 20),
    ("duplicate", "Duplicate", 60),
    ("revert", "Revert", 56),
    ("tags", "Tags", 44),
    ("export", "Export", 56),
    ("import", "Import", 56),
];

/// The heights of the bar's controls, the width of the preset list, and the
/// space between two of them (at scale 1).
const BAR_H: i32 = 14;
const PRESET_W: i32 = 180;
const GAP: i32 = 4;

/// Where the bar's controls are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub preset: Rect,
    pub buttons: [Rect; BUTTONS.len()],
}

impl Bar {
    /// The bar from `(x, top)`.
    #[must_use]
    pub fn new(x: i32, top: i32, s: Scale) -> Self {
        let preset = Rect::new(x, top, PRESET_W * s, BAR_H * s);
        let mut x = preset.x + preset.w + 2 * GAP * s;
        let buttons = BUTTONS.map(|(_, _, w)| {
            let r = Rect::new(x, top, w * s, BAR_H * s);
            x += (w + GAP) * s;
            r
        });
        Self { preset, buttons }
    }

    /// The x right of its last button.
    #[must_use]
    pub fn right(&self) -> i32 {
        self.buttons.last().map_or(self.preset.x + self.preset.w, |b| b.x + b.w)
    }
}

/// The bar's items for preset `current` of `presets`.
pub fn push_bar(items: &mut Vec<Item>, (presets, current): (&[String], &str), bar: &Bar) {
    let values = presets.to_vec();
    let selected = presets.iter().position(|p| p == current).unwrap_or(0);
    items.push(Item::new(
        Some(Id::SynthPreset),
        bar.preset,
        Kind::Select { values: values.clone(), labels: values, selected },
    ));
    for ((id, label, _), rect) in BUTTONS.iter().zip(bar.buttons) {
        items.push(Item::new(Some(Id::SynthButton(id)), rect, Kind::Button { label: (*label).into(), on: false }));
    }
}
