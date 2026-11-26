//! The synth editor's preset bar, above the cards: the preset list, its
//! buttons (delete, new, duplicate, revert, tags, export, import), the
//! button that shows the advanced view, and the preset's profile.

use ordrumbox_core::json::Object;
use ordrumbox_engine::synth_model::{self as model, CardView};

use crate::panel::{Id, Item, Kind, Rect};
use crate::scale::Scale;

/// The bar's buttons: id, label, width (scale units).
pub const BUTTONS: [(&str, &str, i32); 8] = [
    ("delete", "x", 20),
    ("new", "+", 20),
    ("duplicate", "Duplicate", 60),
    ("revert", "Revert", 56),
    ("tags", "Tags", 44),
    ("export", "Export", 56),
    ("import", "Import", 56),
    (ADVANCED, "Advanced", 64),
];

/// The button that switches between the profile's cards and the others.
pub const ADVANCED: &str = "advanced";

/// The heights of the bar's controls, the width of the preset list and of
/// the profile's, and the space between two of them (at scale 1).
const BAR_H: i32 = 14;
const PRESET_W: i32 = 180;
const PROFILE_W: i32 = 64;
const GAP: i32 = 4;

/// Where the bar's controls are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    pub preset: Rect,
    pub buttons: [Rect; BUTTONS.len()],
    pub profile: Rect,
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
        let profile = Rect::new(x + GAP * s, top, PROFILE_W * s, BAR_H * s);
        Self { preset, buttons, profile }
    }
}

/// The values of the profile's list: none (every card), then each profile.
fn profile_options() -> (Vec<String>, Vec<String>) {
    std::iter::once((String::new(), model::NO_PROFILE_LABEL.to_owned()))
        .chain(model::PROFILES.iter().map(|(value, label)| ((*value).to_owned(), (*label).to_owned())))
        .unzip()
}

/// The bar's items for preset `current` of `presets` (`draft`, as edited),
/// its advanced button lit while `view` is the advanced one.
pub fn push_bar(
    items: &mut Vec<Item>,
    (presets, current): (&[String], &str),
    draft: &Object,
    bar: &Bar,
    view: CardView,
) {
    let values = presets.to_vec();
    let selected = presets.iter().position(|p| p == current).unwrap_or(0);
    items.push(Item::new(
        Some(Id::SynthPreset),
        bar.preset,
        Kind::Select { values: values.clone(), labels: values, selected },
    ));
    for ((id, label, _), rect) in BUTTONS.iter().zip(bar.buttons) {
        let on = *id == ADVANCED && view == CardView::Advanced;
        items.push(Item::new(Some(Id::SynthButton(id)), rect, Kind::Button { label: (*label).into(), on }));
    }
    let (values, labels) = profile_options();
    let profile = model::profile(draft).unwrap_or_default();
    let selected = values.iter().position(|v| v == profile).unwrap_or(0);
    items.push(Item::new(Some(Id::SynthProfile), bar.profile, Kind::Select { values, labels, selected }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_profile_list_shows_the_preset_s_and_the_button_its_view() {
        let mut draft = model::defaults();
        let bar = Bar::new(0, 0, Scale::ONE);
        let items = |draft: &Object, view| {
            let mut items = Vec::new();
            push_bar(&mut items, (&["a".to_owned()], "a"), draft, &bar, view);
            items
        };
        let profile = |items: &[Item]| match &crate::panel::find(items, Id::SynthProfile).unwrap().kind {
            Kind::Select { labels, selected, .. } => labels[*selected].clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(profile(&items(&draft, CardView::Main)), model::NO_PROFILE_LABEL);
        model::set_profile(&mut draft, Some("bass"));
        assert_eq!(profile(&items(&draft, CardView::Main)), "Bass");
        let advanced = |view| match crate::panel::find(&items(&draft, view), Id::SynthButton(ADVANCED)).unwrap().kind {
            Kind::Button { on, .. } => on,
            ref other => panic!("{other:?}"),
        };
        assert!(!advanced(CardView::Main) && advanced(CardView::Advanced));
        assert!(bar.buttons.iter().all(|b| b.x + b.w < bar.profile.x), "the profile last");
    }
}
