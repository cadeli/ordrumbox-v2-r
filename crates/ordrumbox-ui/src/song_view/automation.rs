//! The automation lanes of the arrangement: under the patterns' rows, each
//! over one to four rows, named by its control and instrument in the frozen
//! column (its bottom edge there sets how tall it is), its values drawn step
//! by step (none where it leaves the pattern's); under the last row, the
//! button that adds a lane.

use super::draw::Page;
use super::*;
use ordrumbox_core::automation::{
    AutomationKind, AutomationLane, AutomationTarget, STEPS_PER_BEAT, STEPS_PER_MEASURE, is_on,
};

/// The add-automation button, as the grid's add-track button.
const ADD_AUTOMATION_HEIGHT: i32 = 14;
const ADD_AUTOMATION_GAP: i32 = 4;
const ADD_AUTOMATION_LABEL: &str = "+ NEW AUTOMATION";
/// The narrowest measure (scale units) that shows a line on each beat of a lane.
const BEAT_LINES: i32 = 32;
/// How far (scale units) above a lane's bottom edge a press in the frozen
/// column takes the edge.
const EDGE_GRAB: i32 = 3;

/// Each lane of `song`: its index, its first row counted from the lanes'
/// first, and how many rows it takes.
fn lane_rows(song: &Song) -> impl Iterator<Item = (usize, usize, usize)> + '_ {
    song.automations.iter().enumerate().scan(0, |next, (i, lane)| {
        let rows = lane.row_count() as usize;
        let first = *next;
        *next += rows;
        Some((i, first, rows))
    })
}

/// The rows `song`'s lanes take, under the patterns'.
pub(super) fn lanes_row_count(song: &Song) -> usize {
    song.automations.iter().map(|lane| lane.row_count() as usize).sum()
}

/// A canvas drawn into between two heights only: the rows shown, so that a
/// lane scrolled half out of view draws nothing over the ruler or under
/// the rows.
struct Rows<'c, 'a> {
    canvas: &'c mut Canvas<'a>,
    top: i32,
    bottom: i32,
}

impl Rows<'_, '_> {
    fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Rgb) {
        let (from, to) = (y.max(self.top), (y + h).min(self.bottom));
        if to > from {
            self.canvas.fill_rect(x, from, w, to - from, color);
        }
    }
}

/// The colour of a lane's control, as the grid's mute and solo boxes.
const fn target_color(target: AutomationTarget) -> Rgb {
    match target {
        AutomationTarget::Mute => theme::ALERT,
        AutomationTarget::Solo => theme::VALUE,
        AutomationTarget::Volume => theme::FOCUS,
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
    pub(super) fn step_x(&self, step: u32, first: u32) -> i32 {
        self.measure_x(f64::from(step) / f64::from(STEPS_PER_MEASURE), first)
    }

    /// The band a lane's values are drawn in, its rows at `y`, `rows` of
    /// them: its top and height.
    fn band(&self, y: i32, rows: usize) -> (i32, i32) {
        let s = self.scale;
        (y + CLIP_INSET * s / 2, rows as i32 * self.row_height() - 2 * CLIP_INSET * s)
    }

    /// The band lane `lane` of the view's song is drawn in, where it is
    /// (above or under the rows shown when scrolled out).
    #[must_use]
    pub fn lane_band(&self, view: &SongView, lane: usize) -> Option<(i32, i32)> {
        let (_, first, rows) = lane_rows(view.song?).nth(lane)?;
        Some(self.band(self.row_y(view.rows.len() + first, view.scroll.1), rows))
    }

    /// How many rows lane `lane` takes with its bottom edge dragged to `y`:
    /// the nearest row line, 1 to [`AutomationLane::MAX_ROWS`].
    #[must_use]
    pub fn lane_rows_at(&self, view: &SongView, lane: usize, y: i32) -> u32 {
        let Some((_, first, _)) = view.song.and_then(|song| lane_rows(song).nth(lane)) else { return 1 };
        let top = self.row_y(view.rows.len() + first, view.scroll.1);
        let rows = (f64::from(y - top) / f64::from(self.row_height().max(1))).round();
        rows.clamp(1.0, f64::from(AutomationLane::MAX_ROWS)) as u32
    }

    /// Whether `y` is on the line of `level` over a lane's band `(top,
    /// height)`, a few pixels either way: where a press grabs a segment.
    #[must_use]
    pub fn is_on_level_line(&self, (top, height): (i32, i32), level: f64, y: i32) -> bool {
        let line = top + height - (f64::from(height) * level).round() as i32;
        (y - line).abs() <= EDGE_GRAB * self.scale
    }

    /// The level `y` points at over a lane's band `(top, height)`: 1 at its
    /// top, 0 at its bottom, kept within them.
    #[must_use]
    pub fn level_at(&self, (top, height): (i32, i32), y: i32) -> f64 {
        (1.0 - f64::from(y - top) / f64::from(height.max(1))).clamp(0.0, 1.0)
    }

    /// The step shown at `x`, past the frozen column; `first` is the first measure shown.
    #[must_use]
    pub fn step_at(&self, x: i32, first: u32) -> u32 {
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
        let offset = (view.scroll.1 + shown).checked_sub(view.rows.len())?;
        let (lane, first, rows) = lane_rows(song).find(|&(_, first, rows)| (first..first + rows).contains(&offset))?;
        if x < self.body_left() {
            let bottom = self.row_y(view.rows.len() + first + rows, view.scroll.1);
            let at_edge = bottom - y <= EDGE_GRAB * self.scale;
            return Some(if at_edge { SongHit::LaneEdge(lane) } else { SongHit::LaneName(lane) });
        }
        let step = self.step_at(x, view.scroll.0);
        (step < total * STEPS_PER_MEASURE).then_some(SongHit::LaneStep { lane, step })
    }
}

/// The lanes shown on `page`, rows after the `pattern_rows` of the patterns.
pub(super) fn draw_lanes(canvas: &mut Canvas, song: &Song, layout: &SongLayout, pattern_rows: usize, page: &Page) {
    let top = layout.body_top();
    let mut canvas = Rows { canvas, top, bottom: top + page.shown_rows as i32 * layout.row_height() };
    let shown = page.first_row..page.first_row + page.shown_rows;
    for (i, first, rows) in lane_rows(song) {
        let row = pattern_rows + first;
        if row + rows <= shown.start || row >= shown.end {
            continue;
        }
        draw_lane(&mut canvas, &song.automations[i], layout, (layout.row_y(row, page.first_row), rows), page);
    }
}

/// One lane, its `rows` rows at `y`: its name (in the first of them shown),
/// a band under its steps, its values and the step and beat lines over them.
fn draw_lane(canvas: &mut Rows, lane: &AutomationLane, layout: &SongLayout, (y, rows): (i32, usize), page: &Page) {
    let s = layout.scale;
    let (rh, left) = (layout.row_height(), layout.body_left());
    let color = target_color(lane.target);
    let bottom = y + rows as i32 * rh;
    canvas.fill_rect(layout.grid.x, bottom - s.px(), layout.label_width() + page.body_w, s.px(), line_color());
    // Its name in the first of its rows shown.
    let hidden = ((canvas.top - y).max(0) + rh - 1) / rh.max(1);
    let text_y = y + hidden * rh + (rh - 8 * s) / 2;
    if text_y + 8 * s <= canvas.bottom.min(bottom) {
        let x = canvas.canvas.text(layout.grid.x, text_y, lane.target.label(), s, color) + 4 * s;
        let name = fit(&lane.instrument, layout.grid.x + layout.label_width() - 6 * s - x, s);
        canvas.canvas.text(x, text_y, &name, s, theme::text());
    }

    let (band_y, band_h) = layout.band(y, rows);
    canvas.fill_rect(left, band_y, page.body_w, band_h, theme::panel());
    draw_values(canvas, lane, layout, (band_y, band_h), page);
    let first_step = page.first_measure * STEPS_PER_MEASURE;
    let end_step = (page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    let x_of = |step: u32| layout.step_x(step.clamp(first_step, end_step), page.first_measure);
    if layout.measure_width() >= MeasureZoom::STEPS * s {
        let tick_h = band_h / 4;
        for step in (first_step..end_step).filter(|s| s % STEPS_PER_BEAT != 0) {
            canvas.fill_rect(x_of(step) - s.px(), band_y + band_h - tick_h, s.px(), tick_h, cell_color());
        }
    }
    if layout.measure_width() >= BEAT_LINES * s {
        for step in (first_step..end_step).step_by(STEPS_PER_BEAT as usize).filter(|s| s % STEPS_PER_MEASURE != 0) {
            canvas.fill_rect(x_of(step) - s.px(), band_y, s.px(), band_h, cell_color());
        }
    }
}

/// A lane's values over its band `(top, height)`. A switch: each from its
/// point to the next, on full and in its colour, off full and faint, with a
/// mark where it changes. A level: column by column, as high as it is where
/// it glides too, its top drawn. Nothing where the lane leaves the pattern's.
fn draw_values(canvas: &mut Rows, lane: &AutomationLane, layout: &SongLayout, band: (i32, i32), page: &Page) {
    match lane.target.kind() {
        AutomationKind::Switch => draw_switch(canvas, lane, layout, band, page),
        AutomationKind::Level => draw_level(canvas, lane, layout, band, page),
    }
}

fn draw_switch(canvas: &mut Rows, lane: &AutomationLane, layout: &SongLayout, (top, height): (i32, i32), page: &Page) {
    let s = layout.scale;
    let color = target_color(lane.target);
    let first_step = page.first_measure * STEPS_PER_MEASURE;
    let end_step = (page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    let x_of = |step: u32| layout.step_x(step.clamp(first_step, end_step), page.first_measure);
    let points = lane.points();
    for (i, point) in points.iter().enumerate() {
        let end = points.get(i + 1).map_or(end_step, |next| next.step);
        let (from, to) = (x_of(point.step), x_of(end));
        let Some(value) = point.value.filter(|_| to > from) else { continue };
        let fill = if is_on(value) {
            theme::mix(theme::panel(), color, 0.6)
        } else {
            theme::mix(theme::panel(), theme::muted(), 0.25)
        };
        canvas.fill_rect(from, top, to - from, height, fill);
    }
    for point in points.iter().filter(|point| (first_step..end_step).contains(&point.step)) {
        let mark = match point.value.map(is_on) {
            Some(true) => color,
            Some(false) => theme::muted(),
            None => line_color(),
        };
        canvas.fill_rect(x_of(point.step), top, 2 * s, height, mark);
    }
}

fn draw_level(canvas: &mut Rows, lane: &AutomationLane, layout: &SongLayout, (top, height): (i32, i32), page: &Page) {
    let s = layout.scale;
    let color = target_color(lane.target);
    let fill = theme::mix(theme::panel(), color, 0.45);
    let left = layout.body_left();
    for x in left..left + page.body_w {
        // The middle of the pixel's column.
        let position = (layout.measure_under(x, page.first_measure) + 0.5 / f64::from(layout.measure_width().max(1)))
            * f64::from(STEPS_PER_MEASURE);
        let Some(level) = lane.level_at(position) else { continue };
        let h = (f64::from(height) * level).round() as i32;
        canvas.fill_rect(x, top + height - h, 1, h, fill);
        canvas.fill_rect(x, top + height - h, 1, s.px().min(h.max(1)), color);
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
