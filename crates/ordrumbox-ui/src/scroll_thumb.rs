//! The thumb of a vertical scroll bar beside a list: where it sits for the
//! rows shown, which first row a pointer puts it on, and how it is drawn.

use crate::canvas::Canvas;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::theme;

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

/// Paint the bar `bar` with its thumb `(start, len)` along it.
pub fn draw(canvas: &mut Canvas, bar: Rect, (start, len): (i32, i32), scale: Scale) {
    canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, theme::cell());
    canvas.fill_rect(bar.x + scale.px(), start, bar.w - 2 * scale.px(), len, theme::muted());
}
