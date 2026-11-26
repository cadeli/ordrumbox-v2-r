//! A synth card's controls spread across the width its row gave it: its
//! columns of controls (a knob and its label, a stack of spins) moved apart,
//! the room shared evenly around them, so that a widened card has no empty
//! end.

use crate::panel::{Item, Rect};

/// `items` whose top-left corner is right of and under `(left, top)` (a
/// card's controls, not its head) moved apart to share `room` more pixels
/// of width: each column takes half a share on each side.
pub(crate) fn spread(items: &mut [Item], (left, top): (i32, i32), room: i32) {
    if room <= 0 {
        return;
    }
    let is_control = |r: Rect| r.x >= left && r.y >= top;
    let columns = columns(items.iter().map(|i| i.rect).filter(|&r| is_control(r)));
    let count = i64::try_from(columns.len()).unwrap_or(i64::MAX).max(1);
    for item in items.iter_mut().filter(|i| is_control(i.rect)) {
        let Some(index) = columns.iter().position(|&(from, to)| (from..to).contains(&item.rect.x)) else {
            continue;
        };
        let index = i64::try_from(index).unwrap_or(0);
        // Within i32: at most `room`.
        let shift = i32::try_from(i64::from(room) * (2 * index + 1) / (2 * count)).unwrap_or(0);
        item.rect = item.rect.moved(shift, 0);
    }
}

/// The columns of `rects`, left to right: the spans across that their x
/// ranges cover, those that overlap merged.
fn columns(rects: impl Iterator<Item = Rect>) -> Vec<(i32, i32)> {
    let mut spans: Vec<(i32, i32)> = rects.map(|r| (r.x, r.x + r.w.max(1))).collect();
    spans.sort_unstable();
    let mut columns: Vec<(i32, i32)> = Vec::with_capacity(spans.len());
    for (from, to) in spans {
        match columns.last_mut() {
            Some(last) if from < last.1 => last.1 = last.1.max(to),
            _ => columns.push((from, to)),
        }
    }
    columns
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::Kind;

    fn item(x: i32, y: i32, w: i32) -> Item {
        Item::new(None, Rect::new(x, y, w, 10), Kind::Led { on: false })
    }

    #[test]
    fn columns_share_the_room_and_the_head_stays() {
        // A head, then two columns: a knob over its label (wider), and one alone.
        let mut items = [item(0, 0, 40), item(10, 20, 20), item(6, 40, 28), item(50, 20, 20)];
        spread(&mut items, (0, 20), 40);
        let xs: Vec<_> = items.iter().map(|i| i.rect.x).collect();
        // Two columns: 10 and 30 to the right, the knob and its label together.
        assert_eq!(xs, [0, 20, 16, 80]);
        let mut same = items.clone();
        spread(&mut same, (0, 20), 0);
        assert_eq!(same, items, "no room, no move");
    }
}
