//! The song's loop on the ruler: its steps tinted green (grey while it is
//! off) and a mark at each of its ends, which a drag moves, on the lines of
//! the zoom.

use super::draw::Page;
use super::*;
use ordrumbox_core::automation::STEPS_PER_MEASURE;

/// How far (scale units) from an end of the loop a press still takes it.
const END_GRAB: i32 = 4;
/// An end mark's width (scale units).
const END_WIDTH: i32 = 2;
/// The loop's colour, and how much of it covers the ruler's cells.
const LOOP: Rgb = theme::LIVE;
const LOOP_ALPHA: u8 = 90;

impl SongLayout {
    /// The end of the view's loop at `(x, y)` on the ruler, the nearer one
    /// when both are close.
    pub(super) fn loop_end_at(&self, view: &SongView, x: i32, y: i32) -> Option<Edge> {
        let (first, end) = view.loop_region?;
        if y >= self.body_top() || x < self.body_left() {
            return None;
        }
        let distance = |step: u32| (self.step_x(step, view.scroll.0) - x).abs();
        let grab = END_GRAB * self.scale;
        let (to_first, to_end) = (distance(first), distance(end));
        if to_end <= grab && to_end <= to_first { Some(Edge::End) } else { (to_first <= grab).then_some(Edge::Start) }
    }

    /// The line between steps nearest to `x`, on the lines `snap` steps
    /// apart, within the `total` steps of the song; `first` is the first
    /// measure shown.
    #[must_use]
    pub fn loop_line_at(&self, x: i32, first: u32, snap: u32, total: u32) -> u32 {
        let snap = snap.max(1);
        let steps = self.measure_under(x, first) * f64::from(STEPS_PER_MEASURE);
        let line = (steps / f64::from(snap)).round().max(0.0) as u32 * snap;
        line.min(total)
    }
}

/// The view's loop over the ruler of `page`.
pub(super) fn draw(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let Some((first, end)) = view.loop_region else { return };
    let s = layout.scale;
    let shown = page.first_measure * STEPS_PER_MEASURE..=(page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    let x_of = |step: u32| layout.step_x(step.clamp(*shown.start(), *shown.end()), page.first_measure);
    let (from, to) = (x_of(first), x_of(end));
    let (top, height) = (layout.ruler_top(), HEADER_HEIGHT * s - s.px());
    let color = if view.is_loop_on { LOOP } else { theme::muted() };
    if to > from {
        canvas.tint_rect(from, top, to - from, height, color, LOOP_ALPHA);
    }
    for (step, x) in [(first, from), (end, to - END_WIDTH * s)] {
        if shown.contains(&step) {
            canvas.fill_rect(x, top, END_WIDTH * s, height, color);
        }
    }
}
