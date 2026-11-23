//! A loop drawn over a ruler, the song's or a pattern's: its span tinted
//! green (grey while it is off), a mark at each end, and the end a press is
//! near.

use crate::canvas::{Canvas, Rgb};
use crate::panel::Rect;
use crate::scale::Scale;
use crate::scroll_thumb::Edge;
use crate::theme;

/// How far (scale units) from an end of the loop a press still takes it.
const END_GRAB: i32 = 4;
/// An end mark's width (scale units).
const END_WIDTH: i32 = 2;
/// The loop's colour, and how much of it covers the ruler.
const LOOP: Rgb = theme::LIVE;
const LOOP_ALPHA: u8 = 90;

/// The loop whose ends are at `ends` (x of its start, x of its end) over
/// `ruler`: the part in sight tinted, and the marks of the ends in sight.
pub fn draw(canvas: &mut Canvas, ruler: Rect, (from, to): (i32, i32), is_on: bool, scale: Scale) {
    let shown = ruler.x..=ruler.x + ruler.w;
    let color = if is_on { LOOP } else { theme::muted() };
    let (left, right) = (from.clamp(*shown.start(), *shown.end()), to.clamp(*shown.start(), *shown.end()));
    if right > left {
        canvas.tint_rect(left, ruler.y, right - left, ruler.h, color, LOOP_ALPHA);
    }
    for x in [from, to - END_WIDTH * scale] {
        if shown.contains(&x) {
            canvas.fill_rect(x, ruler.y, END_WIDTH * scale, ruler.h, color);
        }
    }
}

/// The end of the loop whose ends are at `ends` that `x` is near enough to
/// take, the nearer one when both are close.
#[must_use]
pub fn end_near((from, to): (i32, i32), x: i32, scale: Scale) -> Option<Edge> {
    let grab = END_GRAB * scale;
    let (to_first, to_end) = ((from - x).abs(), (to - x).abs());
    if to_end <= grab && to_end <= to_first { Some(Edge::End) } else { (to_first <= grab).then_some(Edge::Start) }
}
