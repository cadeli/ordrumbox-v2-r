//! The narrow cards of the synth editor, the voice, the unison and the noise:
//! their controls in lines of one to three, each centred in its column, so
//! that the cards stand beside the oscillators' or the filter's, in their row.

use crate::panel::{Id, Item, Rect};
use crate::scale::Scale;
use crate::synth_cells::{Cell, GAP, Place, QUIET_H, push_head, push_row};
use crate::synth_view::{Block, Card, HEAD_H, PAD_X, PAD_Y, Source, field_cells, is_bypassed};

/// The narrow cards and the number of columns their controls take.
pub(crate) const COLUMN_GROUPS: [(&str, usize); 3] = [("voice", 1), ("unison", 1), ("noise", 3)];

/// The fields that open their card's first line, in this order; the others
/// follow in theirs: the noise's level, then its filter's frequency and resonance.
const FIRST: [&str; 3] = ["noise.mix", "noise.filterFreq", "noise.filterQ"];

/// Card `group` (index `gi` of the groups): its head, then its controls in
/// lines of `columns`, each control over its label, centred in its column.
pub(crate) fn column_block(source: &Source, (group, gi): (&str, usize), columns: usize) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, y) = (PAD_X * s, PAD_Y * s);
    let head_right = push_head(&mut items, source.draft, (group, gi), (x, y), s);
    let top = y + HEAD_H * s;
    let columns = columns.max(1);
    let rows = rows_of(source, group);
    let mut next = x;
    let mut content_right = x;
    for column in 0..columns.min(rows.len()) {
        let (pushed, right) = push_column(rows.iter().skip(column).step_by(columns), (next, top), s);
        items.extend(pushed);
        content_right = right;
        next = right + GAP * s;
    }
    let line_count = i32::try_from(rows.len().div_ceil(columns).max(1)).unwrap_or(i32::MAX);
    let (w, h) = (content_right.max(head_right) + x, top + line_count * QUIET_H * s + y);
    let card = Card {
        group: group.into(),
        rect: Rect::new(0, 0, w, h),
        bypassed: is_bypassed(source.draft, group),
        plot: None,
        framed: true,
    };
    // The columns move apart to the middle of a widened card.
    Block { items, size: (w, h), cards: vec![card], matrix: None, controls: Some((x, top)), grows_down: false }
}

/// The controls of `group` in rows, those of [`FIRST`] first: a stacked spin
/// stays with the one it goes under.
fn rows_of(source: &Source, group: &str) -> Vec<Vec<Cell>> {
    let mut rows: Vec<Vec<Cell>> = Vec::new();
    for cell in field_cells(source.draft, group, source.fields, source.lfo) {
        match (cell.place, rows.last_mut()) {
            (Place::Under, Some(last)) => last.push(cell),
            _ => rows.push(vec![cell]),
        }
    }
    let rank = |row: &Vec<Cell>| {
        let path = row.first().and_then(|cell| match cell.id {
            Id::SynthField(fi) => source.fields.get(usize::from(fi)),
            _ => None,
        });
        path.and_then(|path| FIRST.iter().position(|first| first == path)).unwrap_or(FIRST.len())
    };
    rows.sort_by_key(rank);
    rows
}

/// The controls of `rows` at `(x, top)`, one row under the other, each centred
/// in the column; the x right of the column.
fn push_column<'a>(rows: impl Iterator<Item = &'a Vec<Cell>>, (x, top): (i32, i32), s: Scale) -> (Vec<Item>, i32) {
    let mut pushed = Vec::new();
    for (row, cells) in rows.enumerate() {
        let mut items = Vec::new();
        let row_top = top + row as i32 * QUIET_H * s;
        let right = push_row(&mut items, cells.clone(), (x, row_top), QUIET_H, s);
        pushed.push((items, right - x));
    }
    let width = pushed.iter().map(|&(_, w)| w).max().unwrap_or(0);
    let mut items = Vec::new();
    for (row, w) in pushed {
        let shift = (width - w) / 2;
        items.extend(row.into_iter().map(|item| Item { rect: item.rect.moved(shift, 0), ..item }));
    }
    (items, x + width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth_family::Family;
    use crate::synth_view::{SynthLayout, all_fields, build};
    use ordrumbox_engine::synth_model as model;

    #[test]
    fn the_voice_and_the_unison_stand_beside_the_oscillators_their_controls_in_a_column() {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        // The smallest window's editor.
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        let fields = all_fields(&d);
        let card =
            |group: &str| built.cards.iter().find(|c| c.group == group).unwrap_or_else(|| panic!("{group}")).rect;
        let oscillators = card(Family::Oscillators.name());
        let mut left = oscillators.x + oscillators.w;
        for group in COLUMN_GROUPS.iter().take(2).map(|(group, _)| *group) {
            let r = card(group);
            assert_eq!((r.y, r.h), (oscillators.y, oscillators.h), "{group}: in the oscillators' row");
            assert!(r.x > left, "{group}: right of the card before");
            left = r.x + r.w;
            let controls: Vec<Rect> = model::fields(&d, group)
                .iter()
                .map(|path| {
                    let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
                    crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect
                })
                .collect();
            assert_eq!(controls.len(), 3, "{group}");
            for pair in controls.windows(2) {
                assert!(pair[1].y >= pair[0].y + pair[0].h, "{group}: one under the other");
                let middle = |c: Rect| 2 * c.x + c.w;
                assert!((middle(pair[0]) - middle(pair[1])).abs() <= 2, "{group}: centred in one column");
            }
            assert!(controls.iter().all(|c| c.x >= r.x && c.x + c.w <= r.x + r.w && c.y + c.h <= r.y + r.h), "{group}");
        }
    }

    #[test]
    fn the_noise_is_narrow_two_columns_of_controls() {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        let fields = all_fields(&d);
        let card = built.cards.iter().find(|c| c.group == "noise").expect("the noise card").rect;
        assert!(card.w < 250, "narrower than the filter's row, {card:?}");
        let rect = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect
        };
        let (mix, freq, q) = (rect("noise.mix"), rect("noise.filterFreq"), rect("noise.filterQ"));
        assert_eq!((freq.y, q.y), (mix.y, mix.y), "one line: the level, the frequency, the resonance");
        assert!(mix.x + mix.w < freq.x && freq.x + freq.w < q.x, "in this order");
        let (kind, mode, rate) = (rect("noise.filterType"), rect("noise.mode"), rect("noise.rate"));
        assert!(kind.y >= mix.y + mix.h, "the second line");
        assert!(kind.x + kind.w <= mode.x && mode.x + mode.w <= rate.x, "the filter's type, the mode, the rate");
        assert!(card.h <= HEAD_H + 2 * QUIET_H + 2 * PAD_Y, "two lines high, {card:?}");
    }
}
