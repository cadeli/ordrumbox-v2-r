//! Where the synth editor's cards go: in rows, left to right in their order,
//! each row filled to the full width (its cards share the room left at its
//! end, by their widths) and each card as tall as its row, so that the cards
//! make one block with straight edges, as on a hardware panel.

use crate::panel::Rect;

/// The places of cards of sizes `sizes` (width, height), in their order, in
/// `area`, `gap` apart: one rect each, at least as large as its size.
#[must_use]
pub(crate) fn place(sizes: &[(i32, i32)], area: Rect, gap: i32) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(sizes.len());
    let mut top = area.y;
    for row in rows(sizes, area.w, gap) {
        let cards = &sizes[row];
        let h = cards.iter().map(|&(_, h)| h).max().unwrap_or(0);
        rects.extend(fill(cards, (area.x, top), area.w, h, gap));
        top += h + gap;
    }
    rects
}

/// The cards of each row: as many as fit in `width`, at least one.
fn rows(sizes: &[(i32, i32)], width: i32, gap: i32) -> Vec<std::ops::Range<usize>> {
    let mut rows = Vec::new();
    let (mut from, mut right) = (0, 0);
    for (i, &(w, _)) in sizes.iter().enumerate() {
        if i > from && right + gap + w > width {
            rows.push(from..i);
            (from, right) = (i, 0);
        }
        right += if i > from { gap + w } else { w };
    }
    if from < sizes.len() {
        rows.push(from..sizes.len());
    }
    rows
}

/// The cards of one row from `(x, top)`, `h` high, widened by their share
/// of the room up to `width` (the last takes what does not divide).
fn fill(cards: &[(i32, i32)], (x, top): (i32, i32), width: i32, h: i32, gap: i32) -> Vec<Rect> {
    let count = i32::try_from(cards.len()).unwrap_or(i32::MAX);
    let total: i32 = cards.iter().map(|&(w, _)| w).sum();
    let room = (width - total - gap * (count - 1).max(0)).max(0);
    let mut left = x;
    let mut given = 0;
    let mut rects = Vec::with_capacity(cards.len());
    for (i, &(w, _)) in cards.iter().enumerate() {
        let share = if i + 1 == cards.len() {
            room - given
        } else {
            // Within i32: room × w is at most the area's width squared.
            i32::try_from(i64::from(room) * i64::from(w) / i64::from(total.max(1))).unwrap_or(0)
        };
        given += share;
        rects.push(Rect::new(left, top, w + share, h));
        left += w + share + gap;
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_filled_to_the_width_and_cards_to_their_row() {
        let sizes = [(100, 50), (80, 30), (120, 40), (60, 60), (100, 20)];
        let rects = place(&sizes, Rect::new(10, 20, 300, 500), 8);
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
        let rects = place(&[(100, 10), (100, 10)], Rect::new(0, 0, 50, 100), 4);
        assert_eq!(rects, [Rect::new(0, 0, 100, 10), Rect::new(0, 14, 100, 10)]);
        assert!(place(&[], Rect::new(0, 0, 50, 100), 4).is_empty());
    }
}
