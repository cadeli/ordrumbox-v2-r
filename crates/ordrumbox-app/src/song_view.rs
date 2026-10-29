//! The song panel (`song_panel.js`), in the place of the grid: the pattern
//! list on the left, the arrangement in the middle — laid out like a DAW
//! arrangement view (`arrangement_section.js`): a ruler of measures along X,
//! one row per pattern the arrangement uses (named in a frozen first
//! column), each clip a rectangle over its measures, the cursor —, and the
//! song's column on the right (its widgets are panel items).
//!
//! [`SongLayout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::{Pattern, Song};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::panel::Rect;
use crate::theme;
use crate::view;

/// The JS grid geometry (`song_panel/layout.js`), in px per scale unit.
const LABEL_WIDTH: i32 = 74;
const MEASURE_WIDTH: i32 = 24;
const ROW_HEIGHT: i32 = 22;
const HEADER_HEIGHT: i32 = 18;
/// Gap a clip leaves to its neighbours (1 px on each side).
const CLIP_INSET: i32 = 2;
/// The pattern list's rows and width, the song column's width.
const LIST_ROW: i32 = 12;
const LIST_WIDTH: i32 = 150;
const COLUMN_WIDTH: i32 = 210;

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
/// (`#computeRows`).
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
        let top = view::body_top(scale);
        let bottom = view::footer_top(height, scale, status_lines) - margin / 2;
        let h = (bottom - top).max(0);
        let list = Rect::new(margin, top, LIST_WIDTH * s, h);
        let column_w = (COLUMN_WIDTH * s).min((width - 2 * margin) / 3).max(0);
        let column = Rect::new(width - margin - column_w, top, column_w, h);
        let grid_x = list.x + list.w + 12 * s;
        let grid = Rect::new(grid_x, top, (column.x - 12 * s - grid_x).max(0), h);
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
        if row < view.scroll.1 || row >= view.scroll.1 + self.visible_rows() {
            return None;
        }
        let s = self.scale;
        let x = self.measure_x(f64::from(clip.start_measure), view.scroll.0);
        // A clip may last a fraction of a measure: its width is not rounded to a cell.
        let w = ((clip.measure_count * f64::from(self.measure_width())) as i32 - CLIP_INSET * s).max(4 * s);
        let end = self.body_left() + self.visible_measures() as i32 * self.measure_width();
        let (left, right) = (x.max(self.body_left()), (x + w).min(end));
        let y = self.row_y(row, view.scroll.1) + CLIP_INSET * s / 2;
        (right > left).then(|| Rect::new(left, y, right - left, self.row_height() - 2 * CLIP_INSET * s))
    }

    /// What is at `(x, y)`.
    pub fn hit(&self, view: &SongView, x: i32, y: i32) -> Option<SongHit> {
        if self.list.contains(x, y) {
            let row = self.list_first(view.selected_pattern)
                + usize::try_from((y - self.list.y) / (LIST_ROW * self.scale)).ok()?;
            return (row < view.patterns.len()).then_some(SongHit::Pattern(row));
        }
        let song = view.song?;
        if !self.grid.contains(x, y) || y < self.ruler_top() {
            return None;
        }
        let row = (y >= self.body_top())
            .then(|| view.scroll.1 + usize::try_from((y - self.body_top()) / self.row_height()).unwrap_or(0))
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
    if song.clips.is_empty() {
        canvas.text(g.x, layout.ruler_top(), "No clips yet: right-click a pattern of the list to add it.", s, DIM);
        return;
    }

    let (first_measure, first_row) = view.scroll;
    let shown_measures = layout.visible_measures().min(total.saturating_sub(first_measure));
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

    // A clip takes its row's colour.
    for (i, clip) in song.clips.iter().enumerate() {
        if let Some(r) = layout.clip_rect(view, i) {
            let row = view.rows.iter().position(|row| row.id == clip.pattern_id).unwrap_or(0);
            canvas.fill_rect(r.x, r.y, r.w, r.h, theme::track(row));
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
    if first_measure + shown_measures < total {
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
                };
                draw(&mut canvas, &view, &layout);
            }
        }
    }
}
