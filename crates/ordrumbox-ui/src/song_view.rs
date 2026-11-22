//! The song panel (`song_panel.js`), in the place of the grid: the arrangement
//! across the whole width — laid out like a DAW arrangement view
//! (`arrangement_section.js`): a ruler of measures along X, one row per
//! pattern the arrangement uses (named in a frozen first column), each clip a
//! rectangle over its measures, the cursor —, and under it the pattern list
//! on the left, the selected pattern's description (wrapped, scrolled by the
//! wheel) and the song's column on the right (its widgets are panel items).
//!
//! [`SongLayout`] places everything; drawing and mouse hit-testing both use it.

use crate::scale::Scale;
use crate::scroll_thumb;
use ordrumbox_core::{Pattern, Song};

use crate::canvas::{Canvas, Rgb, fit, text_width};
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
/// Thickness of the arrangement's rows' scroll bar.
const GRID_BAR: i32 = 8;
/// Thickness of the measures' scroll bar, whose thumb's ends zoom (`zoom.rs`).
const MEASURE_BAR: i32 = 14;
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
const ACCENT: Rgb = theme::ACCENT;
const ORPHAN: Rgb = theme::ALERT;
fn selected_color() -> Rgb {
    theme::selected_row()
}
/// The song cursor: where it plays, else where it will start.
const CURSOR: Rgb = theme::LIVE;
const CURSOR_IDLE: Rgb = theme::FOCUS;
const CURRENT: Rgb = theme::LIVE;

/// The ruler's measures that play in a loop.
fn loop_color() -> Rgb {
    theme::mix(cell_color(), theme::LIVE, 0.45)
}

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
    /// The measures that play in a loop, `first..end`.
    pub loop_region: Option<(u32, u32)>,
    /// How wide a measure is drawn.
    pub zoom: MeasureZoom,
}

impl SongView<'_> {
    /// The arrangement's rows: one per pattern, then one per automation lane.
    #[must_use]
    pub fn row_count(&self) -> usize {
        self.rows.len() + self.song.map_or(0, |song| song.automations.len())
    }
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

impl Axis {
    /// The part of `(x, y)` along this bar.
    #[must_use]
    pub const fn along(self, (x, y): (i32, i32)) -> i32 {
        match self {
            Self::Measures => x,
            Self::Rows | Self::List => y,
        }
    }
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
    /// A handle at an end of the measures' thumb: a drag zooms.
    ZoomHandle(Edge),
    /// The button under the rows that adds an automation lane.
    AddAutomation,
    /// An automation lane's name (its index in the song's lanes).
    LaneName(usize),
    /// A step of an automation lane, from the start of the song.
    LaneStep { lane: usize, step: u32 },
}

/// Where the song panel's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SongLayout {
    scale: Scale,
    /// The pattern list.
    pub list: Rect,
    /// The selected pattern's description: its title line, then its frame.
    pub description: Rect,
    /// The arrangement: its title line, then the ruler and the rows.
    pub grid: Rect,
    /// The song's column (its widgets).
    pub column: Rect,
    /// How wide a measure is drawn.
    zoom: MeasureZoom,
}

impl SongLayout {
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize, zoom: MeasureZoom) -> Self {
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
        Self { scale, list, description, grid, column, zoom }
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
        (self.zoom.units() * self.scale).max(1)
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

    fn measure_bar(&self) -> i32 {
        MEASURE_BAR * self.scale
    }

    /// Measures that fit across, left of the rows' scroll bar.
    pub fn visible_measures(&self) -> u32 {
        u32::try_from(self.measures_width() / self.measure_width()).unwrap_or(0).max(1)
    }

    /// Rows that fit down, over the measures' scroll bar.
    pub fn visible_rows(&self) -> usize {
        let h = self.grid.y + self.grid.h - self.measure_bar() - self.body_top() - self.add_automation_height();
        usize::try_from(h.max(0) / self.row_height()).unwrap_or(0).max(1)
    }

    /// The scroll bar along `axis`: under the measures, right of the rows,
    /// or right of the list.
    pub fn scroll_bar(&self, axis: Axis) -> Rect {
        let (g, l, bar) = (self.grid, self.list, self.grid_bar());
        let bottom = g.y + g.h - self.measure_bar();
        match axis {
            Axis::Measures => {
                Rect::new(self.body_left(), bottom, (g.x + g.w - bar - self.body_left()).max(0), self.measure_bar())
            }
            Axis::Rows => Rect::new(g.x + g.w - bar, self.body_top(), bar, (bottom - self.body_top()).max(0)),
            Axis::List => Rect::new(l.x + l.w - bar, l.y, bar, l.h),
        }
    }

    /// Start and length of `axis`'s track, along it.
    fn track(&self, axis: Axis) -> (i32, i32) {
        let r = self.scroll_bar(axis);
        (
            axis.along((r.x, r.y)),
            match axis {
                Axis::Measures => r.w,
                Axis::Rows | Axis::List => r.h,
            },
        )
    }

    /// Start and length of `axis`'s thumb: `shown` of `total` from `first`.
    pub fn thumb(&self, axis: Axis, first: usize, shown: usize, total: usize) -> (i32, i32) {
        scroll_thumb::thumb(self.track(axis), first, shown, total, MIN_THUMB * self.scale)
    }

    /// `axis`'s thumb for what `view` shows: its first measure, row or
    /// pattern, of all of them.
    #[must_use]
    pub fn bar_thumb(&self, view: &SongView, axis: Axis) -> (i32, i32) {
        let (first, shown, total) = match axis {
            Axis::Measures => {
                let total = view.song.map_or(0, |song| song.length_measures().max(1.0).ceil() as u32) + view.room;
                (view.scroll.0 as usize, self.visible_measures() as usize, total as usize)
            }
            Axis::Rows => (view.scroll.1, self.visible_rows(), view.row_count()),
            Axis::List => (view.list_scroll, self.list_rows(), view.patterns.len()),
        };
        self.thumb(axis, first, shown, total)
    }

    /// The first measure or row that puts `axis`'s thumb centred on `(x, y)`.
    pub fn first_at(&self, axis: Axis, x: i32, y: i32, shown: usize, total: usize) -> usize {
        let pos = axis.along((x, y));
        scroll_thumb::first_at(self.track(axis), pos, shown, total, MIN_THUMB * self.scale)
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
        // The grid is as wide as the arrangement.
        let total = song.length_measures().max(1.0).ceil() as u32;
        if self.scroll_bar(Axis::Measures).contains(x, y) {
            let handle = self.zoom_handle(x, self.bar_thumb(view, Axis::Measures));
            return Some(handle.map_or(SongHit::ScrollBar(Axis::Measures), SongHit::ZoomHandle));
        }
        if self.scroll_bar(Axis::Rows).contains(x, y) {
            return Some(SongHit::ScrollBar(Axis::Rows));
        }
        if let Some(hit) = self.automation_hit(view, x, y, total) {
            return Some(hit);
        }
        let row = row_at(y, self.body_top(), self.row_height())
            .map(|row| view.scroll.1 + row)
            .filter(|&row| row < view.rows.len());
        if x < self.body_left() {
            return row.map(SongHit::RowName);
        }
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

mod automation;
mod draw;
mod zoom;
pub use draw::draw;
pub use zoom::{Edge, MeasureZoom};

/// `text` in lines of at most `chars` characters, cut at spaces (a word longer
/// than a line is cut in it) and at line breaks.
pub fn wrap(text: &str, chars: usize) -> Vec<String> {
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
            zoom: MeasureZoom::default(),
            loop_region: None,
        };
        let layout = SongLayout::new(1100, 680, Scale::ONE, 0, MeasureZoom::default());
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
        let zooms = [MeasureZoom::MIN, MEASURE_WIDTH, MeasureZoom::MAX].map(MeasureZoom::new);
        for ((w, h), zoom) in [(1, 1), (40, 30), (1100, 680)].into_iter().flat_map(|size| zooms.map(|z| (size, z))) {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SongLayout::new(w as i32, h as i32, Scale::ONE, 1, zoom);
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
                    loop_region: Some((1, 3)),
                    zoom,
                    description_scroll: 3,
                };
                draw(&mut canvas, &view, &layout);
            }
        }
    }
}
