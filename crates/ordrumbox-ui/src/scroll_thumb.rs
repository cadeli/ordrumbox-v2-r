//! The thumb of a scroll bar: where it sits for the rows shown, which first
//! row a pointer puts it on, how it is drawn beside a list, and the pointer's
//! shape over any of the UI's bars.

use crate::canvas::Canvas;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::theme;
use crate::ui_input::PointerShape;

/// Start and length of the thumb on a track `(start, len)`: `shown` rows of
/// `total` from `first`, no shorter than `min_thumb`.
pub fn thumb((start, len): (i32, i32), first: usize, shown: usize, total: usize, min_thumb: i32) -> (i32, i32) {
    let total = total.max(shown).max(1) as i64;
    let thumb = ((i64::from(len) * shown as i64 / total) as i32).max(min_thumb).min(len);
    let free = i64::from(len - thumb);
    let room = (total - shown as i64).max(1);
    (start + (free * first as i64 / room) as i32, thumb)
}

/// The first row that puts the thumb centred on `pos`, along a track `(start, len)`.
pub fn first_at(track: (i32, i32), pos: i32, shown: usize, total: usize, min_thumb: i32) -> usize {
    let last = total.saturating_sub(shown);
    let (start, len) = track;
    let (_, thumb) = self::thumb(track, 0, shown, total, min_thumb);
    let free = i64::from((len - thumb).max(1));
    let along = i64::from((pos - start - thumb / 2).clamp(0, len - thumb));
    ((along * last as i64 + free / 2) / free) as usize
}

/// The pointer `along` a bar whose thumb is `(start, len)` along it: the
/// open hand on the thumb (a drag scrolls), else the arrow (a click jumps).
#[must_use]
pub fn hover_shape((start, len): (i32, i32), along: i32) -> PointerShape {
    if (start..start + len).contains(&along) { PointerShape::Grab } else { PointerShape::Arrow }
}

/// The pointer's shape while a bar's thumb is held: the closed hand.
pub const HELD_SHAPE: PointerShape = PointerShape::Grabbing;

/// Paint the horizontal bar `bar` with its thumb `(start, len)` across it.
pub fn draw_across(canvas: &mut Canvas, bar: Rect, (start, len): (i32, i32), scale: Scale) {
    canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, theme::cell());
    canvas.fill_rect(start, bar.y + scale.px(), len, bar.h - 2 * scale.px(), theme::muted());
}

/// Paint the vertical bar `bar` with its thumb `(start, len)` along it.
pub fn draw(canvas: &mut Canvas, bar: Rect, (start, len): (i32, i32), scale: Scale) {
    canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, theme::cell());
    canvas.fill_rect(bar.x + scale.px(), start, bar.w - 2 * scale.px(), len, theme::muted());
}
