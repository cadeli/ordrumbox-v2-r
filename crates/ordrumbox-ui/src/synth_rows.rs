//! Where the synth editor's cards go: in rows, left to right in their order,
//! each row filled to the full width (its cards share the room left at its
//! end, by their widths) and each card as tall as its row, so that the cards
//! make one block with straight edges, as on a hardware panel. A card of a
//! fixed width takes no share. One card can stand in a column at the right
//! edge instead, as tall as the rows beside it.

use crate::panel::Rect;

/// The size a card needs, and whether it keeps its width in its row
/// (it takes no share of the room, unless no card of the row grows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Size {
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) is_fixed: bool,
}

impl From<(i32, i32)> for Size {
    fn from((w, h): (i32, i32)) -> Self {
        Self { w, h, is_fixed: false }
    }
}

/// A card out of the rows: card `index` of the order, at the right edge,
/// beside the `row_count` rows of the cards after it, which start a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Column {
    pub(crate) index: usize,
    pub(crate) row_count: usize,
}

/// The places of cards of sizes `sizes` (width, height), in their order, in
/// `area`, `gap` apart, `column` (if any) beside the rows: one rect each, at
/// least as large as its size.
#[must_use]
pub(crate) fn place(sizes: &[Size], area: Rect, gap: i32, column: Option<Column>) -> Vec<Rect> {
    let full = |cards, top| flow(cards, (area.x, top), area.w, gap, usize::MAX);
    let Some(Column { index, row_count }) = column else { return full(sizes, area.y).0 };
    let Some(((&Size { w: column_w, h: column_h, .. }, after), before)) =
        sizes.get(index..).and_then(<[_]>::split_first).zip(sizes.get(..index))
    else {
        return full(sizes, area.y).0;
    };
    let (mut rects, bottom) = full(before, area.y);
    let top = bottom.map_or(area.y, |b| b + gap);
    let column_w = column_w.min(area.w);
    let (beside, beside_bottom) = flow(after, (area.x, top), (area.w - column_w - gap).max(0), gap, row_count);
    let column_bottom = beside_bottom.unwrap_or(top).max(top + column_h);
    let below = after.get(beside.len()..).unwrap_or_default();
    rects.push(Rect::new(area.x + area.w - column_w, top, column_w, column_bottom - top));
    rects.extend(beside);
    rects.extend(full(below, column_bottom + gap).0);
    rects
}

/// The cards `sizes` in at most `row_limit` rows `width` wide from `(x, top)`:
/// the rects of those placed, in order, and the bottom of the last row.
fn flow(sizes: &[Size], (x, top): (i32, i32), width: i32, gap: i32, row_limit: usize) -> (Vec<Rect>, Option<i32>) {
    let mut rects = Vec::with_capacity(sizes.len());
    let mut row_top = top;
    let mut bottom = None;
    let mut rest = sizes;
    for _ in 0..row_limit {
        let count = row_len(rest, width, gap);
        if count == 0 {
            break;
        }
        let (cards, after) = rest.split_at(count);
        let h = cards.iter().map(|size| size.h).max().unwrap_or(0);
        rects.extend(fill(cards, (x, row_top), width, h, gap));
        bottom = Some(row_top + h);
        row_top += h + gap;
        rest = after;
    }
    (rects, bottom)
}

/// How many of `sizes` the next row holds: as many as fit in `width`, at
/// least one (none when there is none).
fn row_len(sizes: &[Size], width: i32, gap: i32) -> usize {
    let mut right = -gap;
    let fit = sizes.iter().take_while(|size| {
        right += gap + size.w;
        right <= width
    });
    fit.count().max(sizes.len().min(1))
}

/// The cards of one row from `(x, top)`, `h` high, those that grow widened
/// by their share of the room up to `width`, in proportion to their widths
/// (the last that grows takes what does not divide; the last card, when
/// none grows).
fn fill(cards: &[Size], (x, top): (i32, i32), width: i32, h: i32, gap: i32) -> Vec<Rect> {
    let count = i32::try_from(cards.len()).unwrap_or(i32::MAX);
    let total: i32 = cards.iter().map(|size| size.w).sum();
    let room = (width - total - gap * (count - 1).max(0)).max(0);
    let growing: i32 = cards.iter().filter(|size| !size.is_fixed).map(|size| size.w).sum();
    let last = cards.iter().rposition(|size| !size.is_fixed).unwrap_or(cards.len().saturating_sub(1));
    let mut left = x;
    let mut given = 0;
    let mut rects = Vec::with_capacity(cards.len());
    for (i, size) in cards.iter().enumerate() {
        let share = if i == last {
            room - given
        } else if size.is_fixed {
            0
        } else {
            // Within i32: room × w is at most the area's width squared.
            i32::try_from(i64::from(room) * i64::from(size.w) / i64::from(growing.max(1))).unwrap_or(0)
        };
        given += share;
        rects.push(Rect::new(left, top, size.w + share, h));
        left += size.w + share + gap;
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_filled_to_the_width_and_cards_to_their_row() {
        let sizes = [(100, 50), (80, 30), (120, 40), (60, 60), (100, 20)];
        let rects = place(&sizes.map(Size::from), Rect::new(10, 20, 300, 500), 8, None);
        // 100 + 80 + 120 and their gaps: 316, too wide for 300.
        let rows: Vec<_> = rects.iter().map(|r| r.y).collect();
        assert_eq!(rows, [20, 20, 78, 78, 78]);
        for (i, r) in rects.iter().enumerate() {
            assert!(r.w >= sizes[i].0 && r.h >= sizes[i].1, "{i}: {r:?}");
        }
        assert_eq!(rects[0].h, 50, "as tall as its row");
        assert_eq!(rects[1].h, 50);
        assert_eq!(rects[3].h, 60);
        // Every row ends at the area's right edge, its cards `gap` apart.
        for pair in rects.windows(2).filter(|p| p[0].y == p[1].y) {
            assert_eq!(pair[0].x + pair[0].w + 8, pair[1].x);
        }
        assert_eq!(rects[1].x + rects[1].w, 310);
        assert_eq!(rects[4].x + rects[4].w, 310);
    }

    #[test]
    fn a_card_wider_than_the_area_has_a_row_of_its_own() {
        let rects = place(&[(100, 10), (100, 10)].map(Size::from), Rect::new(0, 0, 50, 100), 4, None);
        assert_eq!(rects, [Rect::new(0, 0, 100, 10), Rect::new(0, 14, 100, 10)]);
        assert!(place(&[], Rect::new(0, 0, 50, 100), 4, None).is_empty());
    }

    #[test]
    fn a_column_stands_at_the_right_edge_beside_the_rows_after_it() {
        // A row of two, the column, then cards in rows narrowed by it, then a full row.
        let sizes = [(100, 50), (100, 50), (60, 30), (130, 40), (100, 20), (90, 20), (200, 30)];
        let column = Column { index: 2, row_count: 2 };
        let rects = place(&sizes.map(Size::from), Rect::new(0, 0, 300, 500), 10, Some(column));
        assert_eq!(rects[0], Rect::new(0, 0, 145, 50), "the first row takes the full width");
        assert_eq!(rects[1].x + rects[1].w, 300);
        // Rows of 300 - 60 - 10 = 230: 130 alone, then 100 and 90.
        assert_eq!(rects[2], Rect::new(240, 60, 60, 40 + 10 + 20), "as tall as its two rows");
        assert_eq!(rects[3], Rect::new(0, 60, 230, 40));
        assert_eq!((rects[4].y, rects[5].x + rects[5].w), (110, 230));
        assert_eq!(rects[6], Rect::new(0, 140, 300, 30), "under the column, the full width");
    }

    #[test]
    fn a_column_taller_than_its_rows_pushes_the_next_ones_down() {
        let rects = place(
            &[(60, 100), (100, 20), (100, 20)].map(Size::from),
            Rect::new(0, 0, 200, 500),
            10,
            Some(Column { index: 0, row_count: 1 }),
        );
        assert_eq!(rects[0], Rect::new(140, 0, 60, 100));
        assert_eq!(rects[1], Rect::new(0, 0, 130, 20));
        assert_eq!(rects[2], Rect::new(0, 110, 200, 20));
    }

    #[test]
    fn a_card_of_a_fixed_width_takes_no_share_of_the_room() {
        let fixed = |w, h| Size { w, h, is_fixed: true };
        let sizes = [Size::from((100, 20)), fixed(60, 20), Size::from((100, 20)), fixed(80, 20)];
        let rects = place(&sizes, Rect::new(0, 0, 400, 100), 10, None);
        assert_eq!((rects[1].w, rects[3].w), (60, 80), "the fixed cards keep their width");
        assert_eq!(rects[0].w + rects[2].w, 400 - 60 - 80 - 30, "the others share the room");
        assert_eq!(rects[3].x + rects[3].w, 400);
        // A row of fixed cards still reaches the right edge: its last card takes the room.
        let rects = place(&[fixed(50, 10), fixed(50, 10)], Rect::new(0, 0, 200, 100), 10, None);
        assert_eq!((rects[0].w, rects[1].w), (50, 140));
    }
}
