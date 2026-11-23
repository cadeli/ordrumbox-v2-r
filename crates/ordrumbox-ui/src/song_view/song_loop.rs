//! The song's loop on the ruler: its steps tinted green (grey while it is
//! off) and a mark at each of its ends, which a drag moves, on the lines of
//! the zoom.

use super::draw::Page;
use super::*;
use crate::loop_marks;
use crate::panel::Rect;
use ordrumbox_core::automation::STEPS_PER_MEASURE;

impl SongLayout {
    /// The end of the view's loop at `(x, y)` on the ruler, the nearer one
    /// when both are close.
    pub(super) fn loop_end_at(&self, view: &SongView, x: i32, y: i32) -> Option<Edge> {
        let (first, end) = view.loop_region?;
        if y >= self.body_top() || x < self.body_left() {
            return None;
        }
        let at = |step: u32| self.step_x(step, view.scroll.0);
        loop_marks::end_near((at(first), at(end)), x, self.scale)
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
    let shown = page.first_measure * STEPS_PER_MEASURE..=(page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    let x_of = |step: u32| layout.step_x(step, page.first_measure);
    let (left, right) = (x_of(*shown.start()), x_of(*shown.end()));
    let ruler = Rect::new(left, layout.ruler_top(), right - left, HEADER_HEIGHT * layout.scale - layout.scale.px());
    loop_marks::draw(canvas, ruler, (x_of(first), x_of(end)), view.is_loop_on, layout.scale);
}
