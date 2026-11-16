//! Drawing the song view: the list of patterns, the description and the
//! arrangement.

use super::*;

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
            canvas.fill_rect(r.x - 2 * s, y - s.px(), names_right - r.x + 2 * s, 2 * s, ACCENT);
        }
    }
    if total > shown {
        let bar = layout.scroll_bar(Axis::List);
        scroll_thumb::draw(canvas, bar, layout.thumb(Axis::List, first, shown, total), s);
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
    for (x, y, w, h) in [
        (f.x, f.y, f.w, s.px()),
        (f.x, f.y + f.h - s.px(), f.w, s.px()),
        (f.x, f.y, s.px(), f.h),
        (f.x + f.w - s.px(), f.y, s.px(), f.h),
    ] {
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

/// What of the arrangement is on the page: the first measure and row shown,
/// how many of each, the size of the body and where the cursor is.
struct Page {
    first_measure: u32,
    first_row: usize,
    shown_measures: u32,
    shown_rows: usize,
    /// Measures of the song and the room to add clips after them.
    grid_total: u32,
    body_w: i32,
    body_h: i32,
    cursor_measure: f64,
    /// The measure the cursor is in.
    current: u32,
}

impl Page {
    fn new(view: &SongView, layout: &SongLayout, total: u32) -> Self {
        let (first_measure, first_row) = view.scroll;
        let grid_total = total + view.room;
        let shown_measures = layout.visible_measures().min(grid_total.saturating_sub(first_measure));
        let shown_rows = layout.visible_rows().min(view.rows.len().saturating_sub(first_row));
        let cursor_measure = view.cursor.min(f64::from(total.saturating_sub(1))).max(0.0);
        Self {
            first_measure,
            first_row,
            shown_measures,
            shown_rows,
            grid_total,
            body_w: shown_measures as i32 * layout.measure_width(),
            body_h: shown_rows as i32 * layout.row_height(),
            cursor_measure,
            current: cursor_measure.floor() as u32,
        }
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
    let bpm = ordrumbox_core::num::text(song.bpm);
    let meta = format!("{}  {} measures  plays at {bpm} BPM", song.name, total);
    canvas.text(title, g.y, &fit(&meta, g.x + g.w - title, s), s, theme::text());
    if song.clips.is_empty() && view.ghost.is_none() {
        let text = "No clips yet: right-click a pattern of the list to add it.";
        canvas.text(g.x, layout.ruler_top(), text, s, theme::muted());
        return;
    }

    let page = Page::new(view, layout, total);
    draw_measure_ruler(canvas, view, layout, &page);
    draw_row_names(canvas, view, layout, &page);
    draw_measure_lines(canvas, layout, &page);
    draw_clips(canvas, view, song, layout);
    draw_dragged_pattern(canvas, view, layout, &page);
    draw_row_drop(canvas, view, layout, &page);
    // The cursor: where the song plays, else (dimmer) where play starts.
    if (f64::from(page.first_measure)..f64::from(page.first_measure + page.shown_measures))
        .contains(&page.cursor_measure)
    {
        let x = layout.measure_x(page.cursor_measure, page.first_measure);
        let color = if view.playing { CURSOR } else { CURSOR_IDLE };
        canvas.fill_rect(x - s.px(), layout.body_top(), 2 * s, page.body_h, color);
    }
    draw_scroll_bars(canvas, view, layout, &page);
}

/// The ruler: one numbered cell per measure, every fourth marked.
fn draw_measure_ruler(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    for m in page.first_measure..page.first_measure + page.shown_measures {
        let x = layout.measure_x(f64::from(m), page.first_measure);
        let looped = view.loop_region.is_some_and(|(first, end)| (first..end).contains(&m));
        let color = if m == page.current {
            CURRENT
        } else if looped {
            loop_color()
        } else if m % 4 == 0 {
            major_color()
        } else {
            cell_color()
        };
        canvas.fill_rect(x, layout.ruler_top(), layout.measure_width() - s.px(), HEADER_HEIGHT * s - s.px(), color);
        let label = (m + 1).to_string();
        let tx = x + (layout.measure_width() - text_width(&label, s)) / 2;
        let text = if m == page.current {
            background_color()
        } else if m % 4 == 0 {
            theme::text()
        } else {
            theme::muted()
        };
        canvas.text(tx, layout.ruler_top() + (HEADER_HEIGHT * s - 8 * s) / 2, &label, s, text);
    }
}

/// The rows: the name in the frozen column, a line under each.
fn draw_row_names(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    for (k, row) in view.rows.iter().enumerate().skip(page.first_row).take(page.shown_rows) {
        let y = layout.row_y(k, page.first_row);
        let line_w = layout.label_width() + page.body_w;
        canvas.fill_rect(layout.grid.x, y + layout.row_height() - s.px(), line_w, s.px(), line_color());
        let name = fit(&row.name, layout.label_width() - 6 * s, s);
        let color = if row.orphan { ORPHAN } else { theme::text() };
        canvas.text(layout.grid.x, y + (layout.row_height() - 8 * s) / 2, &name, s, color);
    }
}

/// The measure lines, every fourth brighter.
fn draw_measure_lines(canvas: &mut Canvas, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    for m in page.first_measure..=page.first_measure + page.shown_measures {
        let x = layout.measure_x(f64::from(m), page.first_measure);
        let color = if m % 4 == 0 { major_color() } else { cell_color() };
        canvas.fill_rect(x - s.px(), layout.body_top(), s.px(), page.body_h, color);
    }
}

/// A clip takes its row's colour; the one dragged is over the others.
fn draw_clips(canvas: &mut Canvas, view: &SongView, song: &Song, layout: &SongLayout) {
    let dragged = view.dragged_clip.map(|(c, _)| c);
    for i in (0..song.clips.len()).filter(|&i| Some(i) != dragged).chain(dragged) {
        if let Some(r) = layout.clip_rect(view, i) {
            let row = view.rows.iter().position(|row| row.id == song.clips[i].pattern_id).unwrap_or(0);
            canvas.fill_rect(r.x, r.y, r.w, r.h, theme::track(row));
        }
    }
}

/// A pattern dragged from the list: on its row, kept on the rows shown.
fn draw_dragged_pattern(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let Some(ghost) = &view.ghost else { return };
    let s = layout.scale;
    let row = ghost.row.clamp(page.first_row, page.first_row + layout.visible_rows() - 1);
    if let Some(r) = layout.span_rect(row, ghost.measure, ghost.measure_count, view.scroll) {
        canvas.fill_rect(r.x, r.y, r.w, r.h, ACCENT);
    }
    if ghost.row >= view.rows.len() {
        let name = fit(&ghost.name, layout.label_width() - 6 * s, s);
        let y = layout.row_y(row, page.first_row) + (layout.row_height() - 8 * s) / 2;
        canvas.text(layout.grid.x, y, &name, s, ACCENT);
    }
}

/// A row dragged by its name: a line where it would go, above or below the row it takes.
fn draw_row_drop(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let Some((from, to)) = view.row_drop.filter(|&(from, to)| from != to) else { return };
    let edge = if to > from { to + 1 } else { to };
    if (page.first_row..=page.first_row + page.shown_rows).contains(&edge) {
        let s = layout.scale;
        let y = layout.row_y(edge, page.first_row) - s.px();
        canvas.fill_rect(layout.grid.x, y, layout.label_width() + page.body_w, 2 * s, ACCENT);
    }
}

/// The scroll bars: what is shown, over the whole arrangement.
fn draw_scroll_bars(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    let bars = [
        (Axis::Measures, page.first_measure as usize, layout.visible_measures() as usize, page.grid_total as usize),
        (Axis::Rows, page.first_row, layout.visible_rows(), view.rows.len()),
    ];
    for (axis, first, shown, total) in bars {
        let r = layout.scroll_bar(axis);
        canvas.fill_rect(r.x, r.y, r.w, r.h, cell_color());
        let (start, len) = layout.thumb(axis, first, shown, total);
        let color = if total > shown { theme::muted() } else { line_color() };
        match axis {
            Axis::Measures => canvas.fill_rect(start, r.y + s.px(), len, r.h - 2 * s, color),
            Axis::Rows | Axis::List => canvas.fill_rect(r.x + s.px(), start, r.w - 2 * s, len, color),
        }
    }
}
