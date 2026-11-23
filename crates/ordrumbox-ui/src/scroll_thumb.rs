//! The thumb of a scroll bar: where it sits for the rows shown, which first
//! row a pointer puts it on, how it is drawn beside a list, and the pointer's
//! shape over any of the UI's bars.

use crate::canvas::Canvas;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::theme;
use crate::ui_input::PointerShape;

/// An end of a thumb or of a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

impl Edge {
    /// The span `(start, end)` with this end of it moved to `line`, and the
    /// end then held: an end dragged past the other becomes the other.
    #[must_use]
    pub fn moved_to(self, (start, end): (u32, u32), line: u32) -> (u32, u32, Edge) {
        match self {
            Edge::Start if line > end => (end, line, Edge::End),
            Edge::Start => (line, end, Edge::Start),
            Edge::End if line < start => (line, start, Edge::Start),
            Edge::End => (start, line, Edge::End),
        }
    }
}

/// Width of a zoom handle at an end of a thumb, in scale units.
pub const ZOOM_HANDLE: i32 = 6;

/// The zoom handle at `x` of the thumb `(start, len)`, `handle` pixels wide,
/// if any. A thumb too short for two handles and a middle keeps only its end handle.
#[must_use]
pub fn handle_at(x: i32, (start, len): (i32, i32), handle: i32) -> Option<Edge> {
    if !(start..start + len).contains(&x) {
        return None;
    }
    if x >= start + len - handle {
        Some(Edge::End)
    } else if x < start + handle && len >= 3 * handle {
        Some(Edge::Start)
    } else {
        None
    }
}

/// Where the handles of the thumb `(start, len)` are along it, and how wide
/// (`None`: the start's, left out), `handle` pixels at most.
#[must_use]
pub fn handle_spans((start, len): (i32, i32), handle: i32) -> (Option<(i32, i32)>, (i32, i32)) {
    let start_handle = (len >= 3 * handle).then_some((start, handle));
    (start_handle, (start + len - handle.min(len), handle.min(len)))
}

/// Paint the handles of the horizontal bar `bar`'s thumb `(start, len)`: each
/// a lighter end with a grip line.
pub fn draw_handles(canvas: &mut Canvas, bar: Rect, thumb: (i32, i32), scale: Scale) {
    let (start_handle, end_handle) = handle_spans(thumb, ZOOM_HANDLE * scale);
    for (x, w) in start_handle.into_iter().chain([end_handle]) {
        canvas.fill_rect(x, bar.y + scale.px(), w, bar.h - 2 * scale.px(), theme::text());
        canvas.fill_rect(x + w / 2 - scale.px() / 2, bar.y + 4 * scale, scale.px(), bar.h - 8 * scale, theme::muted());
    }
}

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
