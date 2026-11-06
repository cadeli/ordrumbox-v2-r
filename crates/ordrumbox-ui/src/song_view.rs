//! The song panel (`song_panel.js`), in the place of the grid: the arrangement
//! across the whole width — laid out like a DAW arrangement view
//! (`arrangement_section.js`): a ruler of measures along X, one row per
//! pattern the arrangement uses (named in a frozen first column), each clip a
//! rectangle over its measures, the cursor —, and under it the pattern list
//! on the left, the selected pattern's description (wrapped, scrolled by the
//! wheel) and the song's column on the right (its widgets are panel items).
//!
//! [`SongLayout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::{Pattern, Song};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::frame;
use crate::panel::{Rect, row_at};
use crate::theme;

/// The JS grid geometry (`song_panel/layout.js`), in px per scale unit, but
/// for the frozen column: wide enough for a 16-character pattern name (8 px
/// a glyph) and its 6 px margin, where the JS one holds 8.
pub const LABEL_WIDTH: i32 = 134;
const MEASURE_WIDTH: i32 = 24;
const ROW_HEIGHT: i32 = 22;
const HEADER_HEIGHT: i32 = 18;
/// Gap a clip leaves to its neighbours (1 px on each side).
const CLIP_INSET: i32 = 2;
/// The pattern list's rows, the song column's width.
const LIST_ROW: i32 = 12;
const COLUMN_WIDTH: i32 = 210;
/// Characters of a pattern name in the list, and of its number ("99.").
const LIST_NAME_CHARS: i32 = 16;
const LIST_NUMBER_CHARS: i32 = 3;
/// The pattern list's width: its number, a 4 px gap, its name.
const LIST_WIDTH: i32 = (LIST_NUMBER_CHARS + LIST_NAME_CHARS) * crate::font::WIDTH + 4;
/// Space between the description's frame and its text.
const DESCRIPTION_PAD: i32 = 4;
/// Width of the description's scroll bar.
const SCROLL_BAR: i32 = 3;
/// Thickness of the arrangement's scroll bars.
const GRID_BAR: i32 = 8;
/// Shortest thumb of a scroll bar.
const MIN_THUMB: i32 = 12;
/// Height of what is under the arrangement: all the song column's widgets.
const LOWER_HEIGHT: i32 = 138;
/// Gap between the arrangement and what is under it, and between the list and the column.
const GAP: i32 = 12;

fn background_color() -> Rgb {
    theme::bg()
}
fn cell_color() -> Rgb {
    theme::panel_2()
}
fn major_color() -> Rgb {
    theme::cell_beat()
}
fn line_color() -> Rgb {
    theme::line()
}
const ACCENT: Rgb = theme::PINK;
const ORPHAN: Rgb = theme::RED;
fn selected_color() -> Rgb {
    theme::selected_row()
}
const CURSOR: Rgb = theme::LIME;
const CURSOR_IDLE: Rgb = theme::CYAN;
const CURRENT: Rgb = theme::LIME;

/// One row of the arrangement: a pattern it uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    /// Its pattern is no longer in the library (the row stays, so the
    /// arrangement does not hide part of itself).
    pub orphan: bool,
}

/// The patterns `song` uses, in library order, then the missing ones
///.
pub fn rows(song: &Song, patterns: &[Pattern]) -> Vec<Row> {
    let used = |id: &str| song.clips.iter().any(|c| c.pattern_id == id);
    let mut rows: Vec<Row> = patterns
        .iter()
        .filter(|p| !p.id.is_empty() && used(&p.id))
        .map(|p| Row { id: p.id.clone(), name: p.name.clone().unwrap_or_else(|| p.id.clone()), orphan: false })
        .collect();
    for clip in &song.clips {
        if !rows.iter().any(|r| r.id == clip.pattern_id) {
            rows.push(Row { id: clip.pattern_id.clone(), name: clip.pattern_id.clone(), orphan: true });
        }
    }
    rows
}

/// What the song panel shows.
#[derive(Debug)]
pub struct SongView<'a> {
    /// The library's patterns (their names) and the selected one.
    pub patterns: Vec<String>,
    pub selected_pattern: usize,
    /// The list's first row shown.
    pub list_scroll: usize,
    pub song: Option<&'a Song>,
    pub rows: &'a [Row],
    /// The cursor, in measures: where the song plays, else where it starts.
    pub cursor: f64,
    pub playing: bool,
    /// First measure and first row shown.
    pub scroll: (u32, usize),
    /// A clip being dragged, and the measure it would start at.
    pub dragged_clip: Option<(usize, u32)>,
    /// A pattern being dragged over the arrangement.
    pub ghost: Option<SongGhost>,
    /// A row being dragged by its name, and the row it would take.
    pub row_drop: Option<(usize, usize)>,
    /// A pattern being dragged along the list, and the place it would take.
    pub list_drop: Option<(usize, usize)>,
    /// Measures shown past the end of the arrangement, to drop on.
    pub room: u32,
    /// The selected pattern's description (`None` without a pattern), and
    /// its first line shown.
    pub description: Option<&'a str>,
    pub description_scroll: usize,
}

/// Where a pattern dragged from the list would land.
#[derive(Debug, Clone, PartialEq)]
pub struct SongGhost {
    pub name: String,
    /// Its row; past the last one for a pattern the arrangement does not use yet.
    pub row: usize,
    pub measure: u32,
    pub measure_count: f64,
}

/// A scroll bar: across the arrangement's measures, down its rows, or down
/// the pattern list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Measures,
    Rows,
    List,
}

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongHit {
    /// A pattern of the list.
    Pattern(usize),
    /// The selected pattern's description.
    Description,
    /// A measure of the ruler.
    Ruler(u32),
    /// A clip (its index in the arrangement).
    Clip(usize),
    /// A row's pattern name.
    RowName(usize),
    /// The bare grid: a row, a measure.
    Cell { row: usize, measure: u32 },
    /// A scroll bar.
    ScrollBar(Axis),
}

/// Where the song panel's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongLayout {
    scale: i32,
    /// The pattern list.
    pub list: Rect,
    /// The selected pattern's description: its title line, then its frame.
    pub description: Rect,
    /// The arrangement: its title line, then the ruler and the rows.
    pub grid: Rect,
    /// The song's column (its widgets).
    pub column: Rect,
}

impl SongLayout {
    pub fn new(width: i32, height: i32, scale: i32, status_lines: usize) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = frame::body_top(scale);
        let bottom = frame::footer_top(height, scale, status_lines) - margin / 2;
        let h = (bottom - top).max(0);
        let full_w = (width - 2 * margin).max(0);
        // A short window keeps half its height for the arrangement.
        let lower_h = (LOWER_HEIGHT * s).min(h / 2);
        let lower_y = bottom.max(top) - lower_h;
        let grid = Rect::new(margin, top, full_w, (lower_y - GAP * s - top).max(0));
        let column_w = (COLUMN_WIDTH * s).min(full_w / 3);
        let column = Rect::new(width - margin - column_w, lower_y, column_w, lower_h);
        let list = Rect::new(margin, lower_y, (LIST_WIDTH * s).min(full_w / 4), lower_h);
        let description_x = list.x + list.w + GAP * s;
        let description = Rect::new(description_x, lower_y, (column.x - GAP * s - description_x).max(0), lower_h);
        Self { scale, list, description, grid, column }
    }

    /// The description's frame, under its title line.
    fn description_frame(&self) -> Rect {
        let d = self.description;
        let title = 12 * self.scale;
        Rect::new(d.x, d.y + title, d.w, (d.h - title).max(0))
    }

    /// `text` in the lines the description's frame shows.
    pub fn description_lines(&self, text: &str) -> Vec<String> {
        let s = self.scale;
        let w = self.description_frame().w - 2 * DESCRIPTION_PAD * s - SCROLL_BAR * s;
        wrap(text, crate::canvas::chars_in(w, s))
    }

    /// Lines of the description that fit in its frame.
    pub fn description_rows(&self) -> usize {
        let h = self.description_frame().h - 2 * DESCRIPTION_PAD * self.scale;
        usize::try_from(h / (LIST_ROW * self.scale)).unwrap_or(0).max(1)
    }

    fn label_width(&self) -> i32 {
        LABEL_WIDTH * self.scale
    }

    fn measure_width(&self) -> i32 {
        MEASURE_WIDTH * self.scale
    }

    fn row_height(&self) -> i32 {
        ROW_HEIGHT * self.scale
    }

    /// Top of the ruler, under the title line.
    fn ruler_top(&self) -> i32 {
        self.grid.y + 14 * self.scale
    }

    /// Top of the first row.
    fn body_top(&self) -> i32 {
        self.ruler_top() + HEADER_HEIGHT * self.scale
    }

    /// Left of the first measure shown.
    fn body_left(&self) -> i32 {
        self.grid.x + self.label_width()
    }

    fn grid_bar(&self) -> i32 {
        GRID_BAR * self.scale
    }

    /// Measures that fit across, left of the rows' scroll bar.
    pub fn visible_measures(&self) -> u32 {
        let w = self.grid.w - self.label_width() - self.grid_bar();
        u32::try_from(w.max(0) / self.measure_width()).unwrap_or(0).max(1)
    }

    /// Rows that fit down, over the measures' scroll bar.
    pub fn visible_rows(&self) -> usize {
        let h = self.grid.y + self.grid.h - self.grid_bar() - self.body_top();
        usize::try_from(h.max(0) / self.row_height()).unwrap_or(0).max(1)
    }

    /// The scroll bar along `axis`: under the measures, right of the rows,
    /// or right of the list.
    pub fn scroll_bar(&self, axis: Axis) -> Rect {
        let (g, l, bar) = (self.grid, self.list, self.grid_bar());
        let bottom = g.y + g.h - bar;
        match axis {
            Axis::Measures => Rect::new(self.body_left(), bottom, (g.x + g.w - bar - self.body_left()).max(0), bar),
            Axis::Rows => Rect::new(g.x + g.w - bar, self.body_top(), bar, (bottom - self.body_top()).max(0)),
            Axis::List => Rect::new(l.x + l.w - bar, l.y, bar, l.h),
        }
    }

    /// Start and length of `axis`'s track, along it.
    fn track(&self, axis: Axis) -> (i32, i32) {
        let r = self.scroll_bar(axis);
        match axis {
            Axis::Measures => (r.x, r.w),
            Axis::Rows | Axis::List => (r.y, r.h),
        }
    }

    /// Start and length of `axis`'s thumb: `shown` of `total` from `first`.
    fn thumb(&self, axis: Axis, first: usize, shown: usize, total: usize) -> (i32, i32) {
        let (start, len) = self.track(axis);
        let total = total.max(shown).max(1) as i64;
        let thumb = ((i64::from(len) * shown as i64 / total) as i32).max(MIN_THUMB * self.scale).min(len);
        let free = i64::from(len - thumb);
        let room = (total - shown as i64).max(1);
        (start + (free * first as i64 / room) as i32, thumb)
    }

    /// The first measure or row that puts `axis`'s thumb centred on `(x, y)`.
    pub fn scroll_at(&self, axis: Axis, x: i32, y: i32, shown: usize, total: usize) -> usize {
        let last = total.saturating_sub(shown);
        let (start, len) = self.track(axis);
        let (_, thumb) = self.thumb(axis, 0, shown, total);
        let pos = match axis {
            Axis::Measures => x,
            Axis::Rows | Axis::List => y,
        };
        let free = i64::from((len - thumb).max(1));
        let along = i64::from((pos - start - thumb / 2).clamp(0, len - thumb));
        ((along * last as i64 + free / 2) / free) as usize
    }

    /// Patterns that fit down the list.
    pub fn list_rows(&self) -> usize {
        usize::try_from(self.list.h / (LIST_ROW * self.scale)).unwrap_or(0).max(1)
    }

    /// The place in a list of `count` patterns at `y`, kept on the rows shown;
    /// `first` is the first one shown.
    pub fn list_drop_row(&self, y: i32, first: usize, count: usize) -> usize {
        let down = usize::try_from((y - self.list.y).max(0) / (LIST_ROW * self.scale)).unwrap_or(0);
        (first + down.min(self.list_rows() - 1)).min(count.saturating_sub(1))
    }

    /// The largest first row of a list of `count` patterns: the last ones at the bottom.
    pub fn list_last_scroll(&self, count: usize) -> usize {
        count.saturating_sub(self.list_rows())
    }

    /// X of measure `m` (fractional), with `first` the first measure shown.
    fn measure_x(&self, m: f64, first: u32) -> i32 {
        self.body_left() + ((m - f64::from(first)) * f64::from(self.measure_width())).round() as i32
    }

    fn row_y(&self, row: usize, first: usize) -> i32 {
        self.body_top() + (row as i32 - first as i32) * self.row_height()
    }

    /// Clip `index` of the view's arrangement, where it is drawn (clipped to
    /// the grid); `None` when its row is not shown.
    pub fn clip_rect(&self, view: &SongView, index: usize) -> Option<Rect> {
        let clip = view.song?.clips.get(index)?;
        let row = view.rows.iter().position(|r| r.id == clip.pattern_id)?;
        let start = view.dragged_clip.filter(|&(c, _)| c == index).map_or(clip.start_measure, |(_, m)| m);
        self.span_rect(row, start, clip.measure_count, view.scroll)
    }

    /// Where `measure_count` measures from `start` on `row` are drawn.
    fn span_rect(&self, row: usize, start: u32, measure_count: f64, scroll: (u32, usize)) -> Option<Rect> {
        if row < scroll.1 || row >= scroll.1 + self.visible_rows() {
            return None;
        }
        let s = self.scale;
        let x = self.measure_x(f64::from(start), scroll.0);
        // A clip may last a fraction of a measure: its width is not rounded to a cell.
        let w = ((measure_count * f64::from(self.measure_width())) as i32 - CLIP_INSET * s).max(4 * s);
        let end = self.body_left() + self.visible_measures() as i32 * self.measure_width();
        let (left, right) = (x.max(self.body_left()), (x + w).min(end));
        let y = self.row_y(row, scroll.1) + CLIP_INSET * s / 2;
        (right > left).then(|| Rect::new(left, y, right - left, self.row_height() - 2 * CLIP_INSET * s))
    }

    /// The measure shown at `x`, kept on the page; `first` is the first one shown.
    pub fn measure_at(&self, x: i32, first: u32) -> u32 {
        let across = u32::try_from((x - self.body_left()).max(0) / self.measure_width()).unwrap_or(0);
        first + across.min(self.visible_measures() - 1)
    }

    /// The row shown at `y`, kept on the page; `first` is the first one shown.
    pub fn drop_row(&self, y: i32, first: usize) -> usize {
        let down = usize::try_from((y - self.body_top()).max(0) / self.row_height()).unwrap_or(0);
        first + down.min(self.visible_rows().saturating_sub(1))
    }

    /// What is at `(x, y)`.
    pub fn hit(&self, view: &SongView, x: i32, y: i32) -> Option<SongHit> {
        if self.list.contains(x, y) {
            if view.patterns.len() > self.list_rows() && self.scroll_bar(Axis::List).contains(x, y) {
                return Some(SongHit::ScrollBar(Axis::List));
            }
            let row = view.list_scroll + row_at(y, self.list.y, LIST_ROW * self.scale)?;
            return (row < view.patterns.len()).then_some(SongHit::Pattern(row));
        }
        if self.description.contains(x, y) {
            return view.description.map(|_| SongHit::Description);
        }
        let song = view.song?;
        if !self.grid.contains(x, y) || y < self.ruler_top() {
            return None;
        }
        if let Some(axis) = [Axis::Measures, Axis::Rows].into_iter().find(|&a| self.scroll_bar(a).contains(x, y)) {
            return Some(SongHit::ScrollBar(axis));
        }
        let row = row_at(y, self.body_top(), self.row_height())
            .map(|row| view.scroll.1 + row)
            .filter(|&row| row < view.rows.len());
        if x < self.body_left() {
            return row.map(SongHit::RowName);
        }
        // The grid is as wide as the arrangement.
        let total = song.length_measures().max(1.0).ceil() as u32;
        let measure = view.scroll.0 + u32::try_from((x - self.body_left()) / self.measure_width()).ok()?;
        if measure >= total {
            return None;
        }
        if y < self.body_top() {
            return Some(SongHit::Ruler(measure));
        }
        let row = row?;
        // The last clip drawn is on top.
        if let Some(clip) =
            (0..song.clips.len()).rev().find(|&i| self.clip_rect(view, i).is_some_and(|r| r.contains(x, y)))
        {
            return Some(SongHit::Clip(clip));
        }
        Some(SongHit::Cell { row, measure })
    }
}

/// Draw the list and the arrangement (the column is panel items).
pub fn draw(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    draw_list(canvas, view, layout);
    draw_description(canvas, view, layout);
    draw_arrangement(canvas, view, layout);
}

fn draw_list(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let r = layout.list;
    if view.patterns.is_empty() {
        canvas.text(r.x, r.y, "No patterns", s, theme::muted());
        return;
    }
    let (first, shown, total) = (view.list_scroll, layout.list_rows(), view.patterns.len());
    // The names stop short of the scroll bar, when there is one.
    let names_right = if total > shown { layout.scroll_bar(Axis::List).x - 2 * s } else { r.x + r.w };
    for (i, name) in view.patterns.iter().enumerate().skip(first).take(shown) {
        let y = r.y + (i - first) as i32 * LIST_ROW * s;
        let selected = i == view.selected_pattern;
        if selected {
            canvas.fill_rect(r.x - 2 * s, y - 2 * s, r.w, LIST_ROW * s, selected_color());
        }
        let number = format!("{}.", i + 1);
        let x = r.x + (LIST_NUMBER_CHARS * crate::font::WIDTH * s - text_width(&number, s)).max(0);
        let x = canvas.text(x, y, &number, s, theme::muted()) + 4 * s;
        canvas.text(x, y, &fit(name, names_right - x, s), s, if selected { ACCENT } else { theme::text() });
    }
    // A pattern dragged along the list: a line where it would go, above or below the one it takes.
    if let Some((from, to)) = view.list_drop.filter(|&(from, to)| from != to) {
        let edge = if to > from { to + 1 } else { to };
        if (first..=first + shown).contains(&edge) {
            let y = r.y + (edge - first) as i32 * LIST_ROW * s - 2 * s;
            canvas.fill_rect(r.x - 2 * s, y - s, names_right - r.x + 2 * s, 2 * s, ACCENT);
        }
    }
    if total > shown {
        let bar = layout.scroll_bar(Axis::List);
        canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, cell_color());
        let (start, len) = layout.thumb(Axis::List, first, shown, total);
        canvas.fill_rect(bar.x + s, start, bar.w - 2 * s, len, theme::muted());
    }
}

/// The selected pattern's description: wrapped in its frame, from the first
/// line shown, with a scroll bar when it is longer than the frame.
fn draw_description(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let Some(text) = view.description else { return };
    let d = layout.description;
    if d.w <= 0 {
        return;
    }
    canvas.text(d.x, d.y, &fit("Pattern description", d.w, s), s, theme::muted());
    let f = layout.description_frame();
    for (x, y, w, h) in
        [(f.x, f.y, f.w, s), (f.x, f.y + f.h - s, f.w, s), (f.x, f.y, s, f.h), (f.x + f.w - s, f.y, s, f.h)]
    {
        canvas.fill_rect(x, y, w, h, line_color());
    }
    let (x, top) = (f.x + DESCRIPTION_PAD * s, f.y + DESCRIPTION_PAD * s);
    if text.is_empty() {
        canvas.text(x, top, &fit("(click to write one)", f.w - 2 * DESCRIPTION_PAD * s, s), s, theme::muted());
        return;
    }
    let lines = layout.description_lines(text);
    let rows = layout.description_rows();
    for (k, line) in lines.iter().skip(view.description_scroll).take(rows).enumerate() {
        canvas.text(x, top + k as i32 * LIST_ROW * s, line, s, theme::text());
    }
    if lines.len() > rows {
        // The thumb: the lines shown, over the whole text.
        let track_h = f.h - 2 * DESCRIPTION_PAD * s;
        let bar_x = f.x + f.w - DESCRIPTION_PAD * s / 2 - SCROLL_BAR * s;
        let thumb_y = top + track_h * view.description_scroll as i32 / lines.len() as i32;
        let thumb_h = (track_h * rows as i32 / lines.len() as i32).max(4 * s);
        canvas.fill_rect(bar_x, top, SCROLL_BAR * s, track_h, cell_color());
        canvas.fill_rect(bar_x, thumb_y.min(top + track_h - thumb_h), SCROLL_BAR * s, thumb_h, theme::muted());
    }
}

fn draw_arrangement(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let g = layout.grid;
    let Some(song) = view.song else {
        canvas.text(g.x, g.y, "No arrangement - Song > New arrangement makes one.", s, theme::muted());
        return;
    };
    let title = canvas.text(g.x, g.y, "Arrangement", s, theme::muted()) + 8 * s;
    let total = song.length_measures().max(1.0).ceil() as u32;
    // The arrangement's own tempo, which it plays at (the header's knob in this panel).
    let bpm = ordrumbox_core::js::number_to_string(song.bpm);
    let meta = format!("{}  {} measures  plays at {bpm} BPM", song.name, total);
    canvas.text(title, g.y, &fit(&meta, g.x + g.w - title, s), s, theme::text());
    if song.clips.is_empty() && view.ghost.is_none() {
        canvas.text(
            g.x,
            layout.ruler_top(),
            "No clips yet: right-click a pattern of the list to add it.",
            s,
            theme::muted(),
        );
        return;
    }

    let (first_measure, first_row) = view.scroll;
    let grid_total = total + view.room;
    let shown_measures = layout.visible_measures().min(grid_total.saturating_sub(first_measure));
    let shown_rows = layout.visible_rows().min(view.rows.len().saturating_sub(first_row));
    let body_w = shown_measures as i32 * layout.measure_width();
    let body_h = shown_rows as i32 * layout.row_height();
    let cursor_measure = view.cursor.min(f64::from(total.saturating_sub(1))).max(0.0);
    let current = cursor_measure.floor() as u32;

    // Ruler: one numbered cell per measure, every fourth marked.
    for m in first_measure..first_measure + shown_measures {
        let x = layout.measure_x(f64::from(m), first_measure);
        let color = if m == current {
            CURRENT
        } else if m % 4 == 0 {
            major_color()
        } else {
            cell_color()
        };
        canvas.fill_rect(x, layout.ruler_top(), layout.measure_width() - s, HEADER_HEIGHT * s - s, color);
        let label = (m + 1).to_string();
        let tx = x + (layout.measure_width() - text_width(&label, s)) / 2;
        let text = if m == current {
            background_color()
        } else if m % 4 == 0 {
            theme::text()
        } else {
            theme::muted()
        };
        canvas.text(tx, layout.ruler_top() + (HEADER_HEIGHT * s - 8 * s) / 2, &label, s, text);
    }

    // Rows: the name in the frozen column, a line under each.
    for (k, row) in view.rows.iter().enumerate().skip(first_row).take(shown_rows) {
        let y = layout.row_y(k, first_row);
        canvas.fill_rect(g.x, y + layout.row_height() - s, layout.label_width() + body_w, s, line_color());
        let name = fit(&row.name, layout.label_width() - 6 * s, s);
        canvas.text(
            g.x,
            y + (layout.row_height() - 8 * s) / 2,
            &name,
            s,
            if row.orphan { ORPHAN } else { theme::text() },
        );
    }
    // Measure lines, every fourth brighter.
    for m in first_measure..=first_measure + shown_measures {
        let x = layout.measure_x(f64::from(m), first_measure);
        canvas.fill_rect(x - s, layout.body_top(), s, body_h, if m % 4 == 0 { major_color() } else { cell_color() });
    }

    // A clip takes its row's colour; the one dragged is over the others.
    let dragged = view.dragged_clip.map(|(c, _)| c);
    for i in (0..song.clips.len()).filter(|&i| Some(i) != dragged).chain(dragged) {
        if let Some(r) = layout.clip_rect(view, i) {
            let row = view.rows.iter().position(|row| row.id == song.clips[i].pattern_id).unwrap_or(0);
            canvas.fill_rect(r.x, r.y, r.w, r.h, theme::track(row));
        }
    }

    // A pattern dragged from the list: on its row, kept on the rows shown.
    if let Some(ghost) = &view.ghost {
        let row = ghost.row.clamp(first_row, first_row + layout.visible_rows() - 1);
        if let Some(r) = layout.span_rect(row, ghost.measure, ghost.measure_count, view.scroll) {
            canvas.fill_rect(r.x, r.y, r.w, r.h, ACCENT);
        }
        if ghost.row >= view.rows.len() {
            let name = fit(&ghost.name, layout.label_width() - 6 * s, s);
            canvas.text(g.x, layout.row_y(row, first_row) + (layout.row_height() - 8 * s) / 2, &name, s, ACCENT);
        }
    }

    // A row dragged by its name: a line where it would go, above or below the row it takes.
    if let Some((from, to)) = view.row_drop.filter(|&(from, to)| from != to) {
        let edge = if to > from { to + 1 } else { to };
        if (first_row..=first_row + shown_rows).contains(&edge) {
            let y = layout.row_y(edge, first_row) - s;
            canvas.fill_rect(g.x, y, layout.label_width() + body_w, 2 * s, ACCENT);
        }
    }

    // The cursor: where the song plays, else (dimmer) where play starts.
    if (f64::from(first_measure)..f64::from(first_measure + shown_measures)).contains(&cursor_measure) {
        let x = layout.measure_x(cursor_measure, first_measure);
        canvas.fill_rect(x - s, layout.body_top(), 2 * s, body_h, if view.playing { CURSOR } else { CURSOR_IDLE });
    }

    // The scroll bars: what is shown, over the whole arrangement.
    let bars = [
        (Axis::Measures, first_measure as usize, layout.visible_measures() as usize, grid_total as usize),
        (Axis::Rows, first_row, layout.visible_rows(), view.rows.len()),
    ];
    for (axis, first, shown, total) in bars {
        let r = layout.scroll_bar(axis);
        canvas.fill_rect(r.x, r.y, r.w, r.h, cell_color());
        let (start, len) = layout.thumb(axis, first, shown, total);
        let color = if total > shown { theme::muted() } else { line_color() };
        match axis {
            Axis::Measures => canvas.fill_rect(start, r.y + s, len, r.h - 2 * s, color),
            Axis::Rows | Axis::List => canvas.fill_rect(r.x + s, start, r.w - 2 * s, len, color),
        }
    }
}

/// `text` in lines of at most `chars` characters, cut at spaces (a word longer
/// than a line is cut in it) and at line breaks.
fn wrap(text: &str, chars: usize) -> Vec<String> {
    let mut lines = Vec::new();
    if chars == 0 {
        return lines;
    }
    for paragraph in text.split('\n') {
        let mut line: Vec<char> = Vec::new();
        for word in paragraph.split_whitespace() {
            let word: Vec<char> = word.chars().collect();
            if !line.is_empty() && line.len() + 1 + word.len() <= chars {
                line.push(' ');
                line.extend(&word);
                continue;
            }
            if !line.is_empty() {
                lines.push(line.drain(..).collect());
            }
            let mut rest = &word[..];
            while rest.len() > chars {
                lines.push(rest[..chars].iter().collect());
                rest = &rest[chars..];
            }
            line.extend(rest);
        }
        lines.push(line.into_iter().collect());
    }
    lines
}

/// Text cut to `width` pixels.
fn fit(text: &str, width: i32, scale: i32) -> String {
    let chars = crate::canvas::chars_in(width, scale);
    text.chars().take(chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn library() -> Library {
        Library::parse(
            r#"{"songs":[{"name":"s","clips":[{"pattern":"b","startMeasure":0,"measureCount":2},
                {"pattern":"a","startMeasure":2,"measureCount":0.75},{"pattern":"b","startMeasure":4,"measureCount":1}]}],
                "patterns":[{"name":"a"},{"name":"b"},{"name":"c"}]}"#,
        )
        .unwrap()
        .0
    }

    #[test]
    fn rows_are_the_patterns_used_then_the_missing_ones() {
        let mut library = library();
        let ids: Vec<_> = rows(&library.songs[0], &library.patterns).into_iter().map(|r| r.id).collect();
        assert_eq!(ids, ["a", "b"]);
        library.patterns.remove(0);
        let rows = rows(&library.songs[0], &library.patterns);
        assert_eq!(rows[1], Row { id: "a".into(), name: "a".into(), orphan: true });
    }

    #[test]
    fn hit_testing_matches_the_drawing() {
        let library = library();
        let rows = rows(&library.songs[0], &library.patterns);
        let view = SongView {
            patterns: vec!["a".into(), "b".into(), "c".into()],
            selected_pattern: 0,
            list_scroll: 0,
            song: library.songs.first(),
            rows: &rows,
            cursor: 1.5,
            playing: false,
            scroll: (0, 0),
            dragged_clip: None,
            ghost: None,
            row_drop: None,
            list_drop: None,
            room: 0,
            description: Some("x"),
            description_scroll: 0,
        };
        let layout = SongLayout::new(1100, 680, 1, 0);
        let (mw, rh) = (layout.measure_width(), layout.row_height());
        let in_row = |row: usize| layout.body_top() + row as i32 * rh + rh / 2;
        let at_measure = |m: i32| layout.body_left() + m * mw + mw / 2;
        assert_eq!(layout.hit(&view, layout.list.x + 2, layout.list.y + 13), Some(SongHit::Pattern(1)));
        assert_eq!(layout.hit(&view, layout.list.x + 2, layout.list.y + 40), None);
        assert!(layout.list.w < layout.description.w);
        let d = layout.description;
        assert_eq!(layout.hit(&view, d.x + 2, d.y + d.h - 2), Some(SongHit::Description));
        assert_eq!(layout.hit(&view, at_measure(3), layout.ruler_top() + 2), Some(SongHit::Ruler(3)));
        assert_eq!(layout.hit(&view, at_measure(5), layout.ruler_top() + 2), None); // past the end
        assert_eq!(layout.hit(&view, layout.grid.x + 2, in_row(1)), Some(SongHit::RowName(1)));
        assert_eq!(layout.hit(&view, at_measure(1), in_row(1)), Some(SongHit::Clip(0)));
        assert_eq!(layout.hit(&view, at_measure(4), in_row(1)), Some(SongHit::Clip(2)));
        // A 0.75-measure clip: its last quarter is bare grid.
        assert_eq!(layout.hit(&view, layout.body_left() + 2 * mw + 4, in_row(0)), Some(SongHit::Clip(1)));
        assert_eq!(
            layout.hit(&view, layout.body_left() + 3 * mw - 2, in_row(0)),
            Some(SongHit::Cell { row: 0, measure: 2 })
        );
        assert_eq!(layout.hit(&view, at_measure(1), in_row(2)), None);
        // Scrolled by 2 measures and a row.
        let scrolled = SongView { scroll: (2, 1), ..view };
        assert_eq!(layout.hit(&scrolled, at_measure(2), in_row(0)), Some(SongHit::Clip(2)));
        assert_eq!(layout.hit(&scrolled, at_measure(0), layout.ruler_top() + 2), Some(SongHit::Ruler(2)));
        // The measure under the pointer stays on the page.
        assert_eq!(layout.measure_at(at_measure(3), 2), 5);
        assert_eq!(layout.measure_at(0, 2), 2);
        assert_eq!(layout.measure_at(i32::MAX / 2, 0), layout.visible_measures() - 1);
    }

    #[test]
    fn wrap_cuts_at_spaces_and_inside_long_words() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("abcdefghij k", 4), ["abcd", "efgh", "ij k"]);
        assert_eq!(wrap("a\n\nb", 4), ["a", "", "b"]);
        assert!(wrap("a", 0).is_empty());
    }

    #[test]
    fn draws_without_panicking_at_any_size() {
        let library = library();
        let rows = rows(&library.songs[0], &library.patterns);
        let empty = Library::parse(r#"{"songs":[{"name":"e"}]}"#).unwrap().0;
        let long = "a long description of the pattern ".repeat(20);
        for (w, h) in [(1, 1), (40, 30), (1100, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SongLayout::new(w as i32, h as i32, 1, 1);
            for (song, playing) in [(library.songs.first(), true), (empty.songs.first(), false), (None, false)] {
                let view = SongView {
                    patterns: vec!["a".into(); 40],
                    selected_pattern: 39,
                    list_scroll: 30,
                    song,
                    rows: &rows,
                    cursor: 3.2,
                    playing,
                    scroll: (1, 0),
                    dragged_clip: song.filter(|s| !s.clips.is_empty()).map(|_| (0, 3)),
                    ghost: Some(SongGhost { name: "c".into(), row: 2, measure: 2, measure_count: 1.0 }),
                    row_drop: Some((0, 2)),
                    list_drop: Some((39, 31)),
                    room: 8,
                    description: song.map(|_| long.as_str()),
                    description_scroll: 3,
                };
                draw(&mut canvas, &view, &layout);
            }
        }
    }
}
