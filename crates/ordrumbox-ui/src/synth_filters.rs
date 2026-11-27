//! The synth editor's filter card: Filter and Filter 2 side by side, each
//! with its head over its controls, Filter 1's response curve under its head
//! and left of its faders. Filter 2 has no card of its own; its routing (in
//! series or in parallel) is a spin in its head. The faders and the curve
//! take the height of the card's row.

use crate::panel::Id;
use crate::panel::Rect;
use crate::synth_cells::{GAP, KNOB_H, push_head, push_head_spin, push_row};
use crate::synth_family::BESIDE_GAP;
use crate::synth_strip::STRIP_GAP;
use crate::synth_view::{Block, Card, HEAD_H, PAD_X, PAD_Y, PLOT_GAP, PLOT_W, Plot, Source, field_cells, is_bypassed};

/// The group merged into Filter's card; [`crate::synth_view::build`] skips its own.
pub(crate) const MERGED_GROUP: &str = "filter2";
/// The field of Filter 2 whose spin stands in its head, not in its row.
const HEAD_FIELD: &str = "filter2.routing";

/// The card of Filter (`fi` in the groups) with Filter 2 (`f2i`) beside it.
pub(crate) fn filter_block(source: &Source, (fi, f2i): (usize, usize)) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, y) = (PAD_X * s, PAD_Y * s);
    let top = y + HEAD_H * s;
    let head_right = push_head(&mut items, source.draft, ("filter", fi), (x, y), s);
    let plot = Rect::new(x, top, PLOT_W * s, KNOB_H * s);
    let left = plot.x + plot.w + PLOT_GAP * s;
    let first_right =
        push_row(&mut items, field_cells(source.draft, "filter", source.fields, source.lfo), (left, top), KNOB_H, s);
    let beside = first_right.max(head_right) + BESIDE_GAP * s;
    let mut second_head = push_head(&mut items, source.draft, (MERGED_GROUP, f2i), (beside, y), s);
    let head_id = source.fields.iter().position(|f| f == HEAD_FIELD).map(|fi| Id::SynthField(fi as u16));
    let (in_head, row): (Vec<_>, Vec<_>) = field_cells(source.draft, MERGED_GROUP, source.fields, source.lfo)
        .into_iter()
        .partition(|c| Some(c.id) == head_id);
    for cell in in_head {
        second_head = push_head_spin(&mut items, cell, (second_head + GAP * s, y), s);
    }
    let second_right = push_row(&mut items, row, (beside, top), KNOB_H, s).max(second_head);
    let (w, h) = (second_right + PAD_X * s, top + KNOB_H * s + PAD_Y * s);
    let strip_h = (HEAD_H + KNOB_H) * s;
    let half_gap = STRIP_GAP * s / 2;
    let frame = Card {
        group: "filter".into(),
        rect: Rect::new(0, 0, w, h),
        bypassed: is_bypassed(source.draft, "filter"),
        plot: None,
        framed: true,
    };
    let merged = Card {
        group: MERGED_GROUP.into(),
        rect: Rect::new(beside, y - half_gap, second_right - beside, strip_h + 2 * half_gap),
        bypassed: is_bypassed(source.draft, MERGED_GROUP),
        plot: None,
        framed: false,
    };
    let curve =
        Card { group: "filter".into(), rect: plot, bypassed: false, plot: Some((Plot::Filter, plot)), framed: false };
    Block { items, size: (w, h), cards: vec![frame, merged, curve], matrix: None, controls: None, grows_down: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::Scale;
    use crate::synth_view::{SynthLayout, all_fields, build};
    use ordrumbox_engine::synth_model as model;

    #[test]
    fn filter_2_is_beside_filter_1_in_its_card_the_curve_left_of_its_controls() {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        let fields = all_fields(&d);
        let rect = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect
        };
        let frame = built.cards.iter().find(|c| c.group == "filter" && c.framed).expect("the filter card").rect;
        let merged = built.cards.iter().find(|c| c.group == MERGED_GROUP).expect("Filter 2's strip").rect;
        let (_, curve) =
            built.cards.iter().find_map(|c| c.plot.filter(|(plot, _)| *plot == Plot::Filter)).expect("curve");
        assert!(merged.x > frame.x && merged.x + merged.w <= frame.x + frame.w, "in the filter's card");
        assert!(merged.y >= frame.y && merged.y + merged.h <= frame.y + frame.h);
        assert!(rect("filter2.type").x > rect("filter.drive").x + rect("filter.drive").w, "beside Filter 1");
        assert_eq!(rect("filter2.type").y, rect("filter.type").y, "same row");
        assert!(curve.x + curve.w < rect("filter.type").x, "left of the controls");
        for path in ["filter2.freq", "filter2.Q", "filter2.balance"] {
            assert!(rect(path).x >= merged.x, "{path}: in its strip");
        }
        // The routing in Filter 2's head, over its controls, inside its strip.
        let routing = rect(HEAD_FIELD);
        assert!(routing.y + routing.h <= rect("filter2.type").y, "in the head: {routing:?}");
        assert!(routing.x > merged.x && routing.x + routing.w <= merged.x + merged.w, "{routing:?} {merged:?}");
        // The row is as tall as the noise card: the faders and the curve take its height.
        let foot = frame.y + frame.h - PAD_Y;
        assert!(curve.h > KNOB_H, "{curve:?}");
        assert_eq!(curve.y + curve.h, foot, "the curve");
        for path in ["filter.freq", "filter.Q", "filter.drive", "filter2.freq", "filter2.Q"] {
            assert_eq!((rect(path).y, rect(path).y + rect(path).h), (curve.y, foot), "{path}");
        }
        assert!(merged.y + merged.h <= frame.y + frame.h, "Filter 2's strip in the card");
        // In a row no taller than it (narrow, the noise has a row of its own), they keep a knob's height.
        let narrow = build(&d, (&[], ""), &SynthLayout::new(740, 680, Scale::ONE, 0), 0, &|_| None);
        let (_, curve) =
            narrow.cards.iter().find_map(|c| c.plot.filter(|(plot, _)| *plot == Plot::Filter)).expect("curve");
        assert_eq!(curve.h, KNOB_H);
        let type_fi = fields.iter().position(|f| f == "filter.type").expect("filter.type") as u16;
        let narrow_type = crate::panel::find(&narrow.items, Id::SynthField(type_fi)).expect("filter.type").rect;
        assert_eq!(rect("filter.type").h, narrow_type.h, "a spin keeps its height");
    }
}
