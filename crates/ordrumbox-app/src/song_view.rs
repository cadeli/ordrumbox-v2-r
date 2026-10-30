//! The song panel (`song_panel.js`), in the place of the grid: the arrangement
//! across the whole width — laid out like a DAW arrangement view
//! (`arrangement_section.js`): a ruler of measures along X, one row per
//! pattern the arrangement uses (named in a frozen first column), each clip a
//! rectangle over its measures, the cursor —, and under it the pattern list
//! on the left and the song's column on the right (its widgets are panel
//! items).
//!
//! [`SongLayout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::{Pattern, Song};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::frame;
use crate::panel::{Rect, row_at};
use crate::theme;

/// The JS grid geometry (`song_panel/layout.js`), in px per scale unit.
const LABEL_WIDTH: i32 = 74;
const MEASURE_WIDTH: i32 = 24;
const ROW_HEIGHT: i32 = 22;
const HEADER_HEIGHT: i32 = 18;
/// Gap a clip leaves to its neighbours (1 px on each side).
const CLIP_INSET: i32 = 2;
/// The pattern list's rows, the song column's width.
const LIST_ROW: i32 = 12;
const COLUMN_WIDTH: i32 = 210;
/// Height of what is under the arrangement: all the song column's widgets.
const LOWER_HEIGHT: i32 = 138;
/// Gap between the arrangement and what is under it, and between the list and the column.
const GAP: i32 = 12;

const BACKGROUND: Rgb = theme::BG;
const CELL: Rgb = theme::PANEL_2;
const MAJOR: Rgb = theme::CELL_BEAT;
const LINE: Rgb = theme::LINE;
const TEXT: Rgb = theme::TEXT;
const DIM: Rgb = theme::MUTED;
const ACCENT: Rgb = theme::PINK;
const ORPHAN: Rgb = theme::RED;
const SELECTED: Rgb = theme::SELECTED_ROW;
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
    /// Measures shown past the end of the arrangement, to drop on.
    pub room: u32,
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

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SongHit {
    /// A pattern of the list.
    Pattern(usize),
    /// A measure of the ruler.
    Ruler(u32),
    /// A clip (its index in the arrangement).
    Clip(usize),
    /// A row's pattern name.
    RowName(usize),
    /// The bare grid: a row, a measure.
    Cell { row: usize, measure: u32 },
}

/// Where the song panel's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongLayout {
    scale: i32,
    /// The pattern list.
    pub list: Rect,
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
        let list = Rect::new(margin, lower_y, (column.x - GAP * s - margin).max(0), lower_h);
        Self { scale, list, grid, column }
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

    /// Measures that fit across.
    pub fn visible_measures(&self) -> u32 {
        u32::try_from((self.grid.w - self.label_width()).max(0) / self.measure_width()).unwrap_or(0).max(1)
    }

    /// Rows that fit down.
    pub fn visible_rows(&self) -> usize {
        usize::try_from((self.grid.y + self.grid.h - self.body_top()).max(0) / self.row_height()).unwrap_or(0).max(1)
    }

    fn list_rows(&self) -> usize {
        usize::try_from(self.list.h / (LIST_ROW * self.scale)).unwrap_or(0)
    }

    /// First list row shown, so that `selected` is visible.
    fn list_first(&self, selected: usize) -> usize {
        selected.saturating_sub(self.list_rows().saturating_sub(1))
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

    /// What is at `(x, y)`.
    pub fn hit(&self, view: &SongView, x: i32, y: i32) -> Option<SongHit> {
        if self.list.contains(x, y) {
            let row = self.list_first(view.selected_pattern) + row_at(y, self.list.y, LIST_ROW * self.scale)?;
            return (row < view.patterns.len()).then_some(SongHit::Pattern(row));
        }
        let song = view.song?;
        if !self.grid.contains(x, y) || y < self.ruler_top() {
            return None;
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
    draw_arrangement(canvas, view, layout);
}

fn draw_list(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let r = layout.list;
    if view.patterns.is_empty() {
        canvas.text(r.x, r.y, "No patterns", s, DIM);
        return;
    }
    let first = layout.list_first(view.selected_pattern);
    for (i, name) in view.patterns.iter().enumerate().skip(first).take(layout.list_rows()) {
        let y = r.y + (i - first) as i32 * LIST_ROW * s;
        let selected = i == view.selected_pattern;
        if selected {
            canvas.fill_rect(r.x - 2 * s, y - 2 * s, r.w, LIST_ROW * s, SELECTED);
        }
        let x = canvas.text(r.x, y, &format!("{}.", i + 1), s, DIM) + 4 * s;
        canvas.text(x, y, &fit(name, r.x + r.w - x, s), s, if selected { ACCENT } else { TEXT });
    }
}

fn draw_arrangement(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let g = layout.grid;
    let Some(song) = view.song else {
        canvas.text(g.x, g.y, "No arrangement - Song > New arrangement makes one.", s, DIM);
        return;
    };
    let title = canvas.text(g.x, g.y, "Arrangement", s, DIM) + 8 * s;
    let total = song.length_measures().max(1.0).ceil() as u32;
    let meta = format!("{}  {} measures  {} BPM", song.name, total, ordrumbox_core::js::number_to_string(song.bpm));
    canvas.text(title, g.y, &fit(&meta, g.x + g.w - title, s), s, TEXT);
    if song.clips.is_empty() && view.ghost.is_none() {
        canvas.text(g.x, layout.ruler_top(), "No clips yet: right-click a pattern of the list to add it.", s, DIM);
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
            MAJOR
        } else {
            CELL
        };
        canvas.fill_rect(x, layout.ruler_top(), layout.measure_width() - s, HEADER_HEIGHT * s - s, color);
        let label = (m + 1).to_string();
        let tx = x + (layout.measure_width() - text_width(&label, s)) / 2;
        let text = if m == current {
            BACKGROUND
        } else if m % 4 == 0 {
            TEXT
        } else {
            DIM
        };
        canvas.text(tx, layout.ruler_top() + (HEADER_HEIGHT * s - 8 * s) / 2, &label, s, text);
    }

    // Rows: the name in the frozen column, a line under each.
    for (k, row) in view.rows.iter().enumerate().skip(first_row).take(shown_rows) {
        let y = layout.row_y(k, first_row);
        canvas.fill_rect(g.x, y + layout.row_height() - s, layout.label_width() + body_w, s, LINE);
        let name = fit(&row.name, layout.label_width() - 6 * s, s);
        canvas.text(g.x, y + (layout.row_height() - 8 * s) / 2, &name, s, if row.orphan { ORPHAN } else { TEXT });
    }
    // Measure lines, every fourth brighter.
    for m in first_measure..=first_measure + shown_measures {
        let x = layout.measure_x(f64::from(m), first_measure);
        canvas.fill_rect(x - s, layout.body_top(), s, body_h, if m % 4 == 0 { MAJOR } else { CELL });
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

    // The cursor: where the song plays, else (dimmer) where play starts.
    if (f64::from(first_measure)..f64::from(first_measure + shown_measures)).contains(&cursor_measure) {
        let x = layout.measure_x(cursor_measure, first_measure);
        canvas.fill_rect(x - s, layout.body_top(), 2 * s, body_h, if view.playing { CURSOR } else { CURSOR_IDLE });
    }

    // Scroll hints.
    let more = |canvas: &mut Canvas, text: &str, x: i32, y: i32| {
        canvas.text(x, y, text, s, DIM);
    };
    if first_measure > 0 {
        more(canvas, "<", layout.body_left() - 10 * s, layout.ruler_top() + 5 * s);
    }
    if first_measure + shown_measures < grid_total {
        more(canvas, ">", layout.body_left() + body_w + 2 * s, layout.ruler_top() + 5 * s);
    }
    if first_row + shown_rows < view.rows.len() {
        let hidden = view.rows.len() - first_row - shown_rows;
        more(canvas, &format!("+{hidden} rows"), g.x, layout.body_top() + body_h + 4 * s);
    }
}

/// Text cut to `width` pixels.
fn fit(text: &str, width: i32, scale: i32) -> String {
    let chars = usize::try_from(width / (8 * scale).max(1)).unwrap_or(0);
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
            song: library.songs.first(),
            rows: &rows,
            cursor: 1.5,
            playing: false,
            scroll: (0, 0),
            dragged_clip: None,
            ghost: None,
            room: 0,
        };
        let layout = SongLayout::new(1100, 680, 1, 0);
        let (mw, rh) = (layout.measure_width(), layout.row_height());
        let in_row = |row: usize| layout.body_top() + row as i32 * rh + rh / 2;
        let at_measure = |m: i32| layout.body_left() + m * mw + mw / 2;
        assert_eq!(layout.hit(&view, layout.list.x + 2, layout.list.y + 13), Some(SongHit::Pattern(1)));
        assert_eq!(layout.hit(&view, layout.list.x + 2, layout.list.y + 40), None);
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
    fn draws_without_panicking_at_any_size() {
        let library = library();
        let rows = rows(&library.songs[0], &library.patterns);
        let empty = Library::parse(r#"{"songs":[{"name":"e"}]}"#).unwrap().0;
        for (w, h) in [(1, 1), (40, 30), (1100, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SongLayout::new(w as i32, h as i32, 1, 1);
            for (song, playing) in [(library.songs.first(), true), (empty.songs.first(), false), (None, false)] {
                let view = SongView {
                    patterns: vec!["a".into(); 40],
                    selected_pattern: 39,
                    song,
                    rows: &rows,
                    cursor: 3.2,
                    playing,
                    scroll: (1, 0),
                    dragged_clip: song.filter(|s| !s.clips.is_empty()).map(|_| (0, 3)),
                    ghost: Some(SongGhost { name: "c".into(), row: 2, measure: 2, measure_count: 1.0 }),
                    room: 8,
                };
                draw(&mut canvas, &view, &layout);
            }
        }
    }
}
