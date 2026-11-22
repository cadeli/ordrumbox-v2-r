//! The automation lanes of the arrangement: rows under the patterns' rows,
//! each named by its control and instrument in the frozen column, its on
//! spans drawn step by step, and under the last row the button that adds a
//! lane.

use super::draw::Page;
use super::*;
use ordrumbox_core::automation::{AutomationLane, AutomationTarget, STEPS_PER_BEAT, STEPS_PER_MEASURE};

/// The add-automation button, as the grid's add-track button.
const ADD_AUTOMATION_HEIGHT: i32 = 14;
const ADD_AUTOMATION_GAP: i32 = 4;
const ADD_AUTOMATION_LABEL: &str = "+ NEW AUTOMATION";
/// The narrowest measure (scale units) that shows a line on each beat of a lane.
const BEAT_LINES: i32 = 32;

/// The colour of a lane's control, as the grid's mute and solo boxes.
const fn target_color(target: AutomationTarget) -> Rgb {
    match target {
        AutomationTarget::Mute => theme::ALERT,
        AutomationTarget::Solo => theme::VALUE,
    }
}

impl SongLayout {
    /// What the add-automation button takes under the rows.
    pub(super) fn add_automation_height(&self) -> i32 {
        (ADD_AUTOMATION_GAP + ADD_AUTOMATION_HEIGHT) * self.scale
    }

    /// The add-automation button, under the last row shown, in the frozen column.
    #[must_use]
    pub fn add_automation_rect(&self, view: &SongView) -> Rect {
        let s = self.scale;
        let shown = self.visible_rows().min(view.row_count().saturating_sub(view.scroll.1));
        let y = self.body_top() + shown as i32 * self.row_height() + ADD_AUTOMATION_GAP * s;
        Rect::new(self.grid.x, y, self.label_width() - 4 * s, ADD_AUTOMATION_HEIGHT * s)
    }

    /// X of `step` (from the start of the song), with `first` the first measure shown.
    fn step_x(&self, step: u32, first: u32) -> i32 {
        self.measure_x(f64::from(step) / f64::from(STEPS_PER_MEASURE), first)
    }

    /// The step shown at `x`, past the frozen column; `first` is the first measure shown.
    fn step_at(&self, x: i32, first: u32) -> u32 {
        let across = f64::from((x - self.body_left()).max(0)) / f64::from(self.measure_width());
        ((f64::from(first) + across) * f64::from(STEPS_PER_MEASURE)).floor() as u32
    }

    /// The add-automation button, a lane's name or one of its steps at
    /// `(x, y)`, in an arrangement of `total` measures with clips.
    pub(super) fn automation_hit(&self, view: &SongView, x: i32, y: i32, total: u32) -> Option<SongHit> {
        let song = view.song.filter(|song| !song.clips.is_empty())?;
        if self.add_automation_rect(view).contains(x, y) {
            return Some(SongHit::AddAutomation);
        }
        let shown = row_at(y, self.body_top(), self.row_height()).filter(|&row| row < self.visible_rows())?;
        let lane = (view.scroll.1 + shown).checked_sub(view.rows.len()).filter(|&l| l < song.automations.len())?;
        if x < self.body_left() {
            return Some(SongHit::LaneName(lane));
        }
        let step = self.step_at(x, view.scroll.0);
        (step < total * STEPS_PER_MEASURE).then_some(SongHit::LaneStep { lane, step })
    }
}

/// The lanes shown on `page`, rows after the `pattern_rows` of the patterns.
pub(super) fn draw_lanes(canvas: &mut Canvas, song: &Song, layout: &SongLayout, pattern_rows: usize, page: &Page) {
    let rows = page.first_row..page.first_row + page.shown_rows;
    for row in rows.filter(|&row| row >= pattern_rows) {
        if let Some(lane) = song.automations.get(row - pattern_rows) {
            draw_lane(canvas, lane, layout, layout.row_y(row, page.first_row), page);
        }
    }
}

/// One lane, its row at `y`: its name, a band under its steps, its on spans
/// and a mark on each point.
fn draw_lane(canvas: &mut Canvas, lane: &AutomationLane, layout: &SongLayout, y: i32, page: &Page) {
    let s = layout.scale;
    let (rh, left) = (layout.row_height(), layout.body_left());
    let color = target_color(lane.target);
    canvas.fill_rect(layout.grid.x, y + rh - s.px(), layout.label_width() + page.body_w, s.px(), line_color());
    let text_y = y + (rh - 8 * s) / 2;
    let x = canvas.text(layout.grid.x, text_y, lane.target.label(), s, color) + 4 * s;
    let name = fit(&lane.instrument, layout.grid.x + layout.label_width() - 6 * s - x, s);
    canvas.text(x, text_y, &name, s, theme::text());

    let (band_y, band_h) = (y + CLIP_INSET * s / 2, rh - 2 * CLIP_INSET * s);
    canvas.fill_rect(left, band_y, page.body_w, band_h, theme::panel());
    let first_step = page.first_measure * STEPS_PER_MEASURE;
    let end_step = (page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    let x_of = |step: u32| layout.step_x(step.clamp(first_step, end_step), page.first_measure);
    let points = lane.points();
    for (i, point) in points.iter().enumerate().filter(|(_, point)| point.is_on) {
        let end = points.get(i + 1).map_or(end_step, |next| next.step);
        let (from, to) = (x_of(point.step), x_of(end));
        if to > from {
            canvas.fill_rect(from, band_y, to - from, band_h, theme::mix(theme::panel(), color, 0.6));
        }
    }
    if layout.measure_width() >= BEAT_LINES * s {
        for step in (first_step..end_step).step_by(STEPS_PER_BEAT as usize).filter(|s| s % STEPS_PER_MEASURE != 0) {
            canvas.fill_rect(x_of(step) - s.px(), band_y, s.px(), band_h, cell_color());
        }
    }
    for point in points.iter().filter(|point| (first_step..end_step).contains(&point.step)) {
        let mark = if point.is_on { color } else { theme::muted() };
        canvas.fill_rect(x_of(point.step), band_y, 2 * s, band_h, mark);
    }
}

/// The button that adds a lane, under the last row, once the arrangement has clips.
pub(super) fn draw_add_button(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    if view.song.is_none_or(|song| song.clips.is_empty()) {
        return;
    }
    let s = layout.scale;
    let r = layout.add_automation_rect(view);
    canvas.fill_rect(r.x, r.y, r.w, r.h, line_color());
    let label = fit(ADD_AUTOMATION_LABEL, r.w, s);
    let x = r.x + (r.w - text_width(&label, s)).max(0) / 2;
    canvas.text(x, r.y + (r.h - 8 * s) / 2, &label, s, theme::text());
}
