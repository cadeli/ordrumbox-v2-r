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
        let hovered = view.hovered == Some(SongHit::Pattern(i));
        if selected || hovered {
            let color = if selected { selected_color() } else { theme::highlight() };
            canvas.fill_rect(r.x - 2 * s, y - 2 * s, r.w, LIST_ROW * s, color);
        }
        if hovered {
            canvas.text(names_right - text_width(MENU_MARK, s), y, MENU_MARK, s, theme::muted());
        }
        let number = format!("{}.", i + 1);
        let x = r.x + (LIST_NUMBER_CHARS * crate::font::WIDTH * s - text_width(&number, s)).max(0);
        let x = canvas.text(x, y, &number, s, theme::muted()) + 4 * s;
        // The room of the mark is kept, so that a name keeps its length under the pointer.
        let room = names_right - x - text_width(MENU_MARK, s) - 2 * s;
        canvas.text(x, y, &fit(name, room, s), s, if selected { ACCENT } else { theme::text() });
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
        scroll_thumb::draw(canvas, bar, layout.bar_thumb(view, Axis::List), s);
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
    draw_tags(canvas, view.tags.unwrap_or_default(), layout);
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

/// The selected pattern's tags on their line: `Tags`, then its genre, then
/// its features dimmer, as many as fit; a hint when it has none.
fn draw_tags(canvas: &mut Canvas, tags: &[String], layout: &SongLayout) {
    let s = layout.scale;
    let line = layout.tags_line();
    let end = line.x + line.w;
    // Under the frame, as far below it as the title is above.
    let y = line.y + line.h - 8 * s;
    let mut x = canvas.text(line.x, y, "Tags", s, theme::muted()) + 8 * s;
    if tags.is_empty() {
        canvas.text(x, y, &fit("(click: genre first, then features)", end - x, s), s, theme::muted());
        return;
    }
    let genre = tags.iter().position(|tag| !ordrumbox_core::tags::is_feature(tag));
    for (i, tag) in tags.iter().enumerate() {
        let text = if i + 1 < tags.len() { format!("{tag},") } else { tag.clone() };
        let shown = fit(&text, end - x, s);
        let color = if Some(i) == genre { theme::text() } else { theme::muted() };
        x = canvas.text(x, y, &shown, s, color) + text_width(" ", s);
        if shown != text {
            break;
        }
    }
}

/// What of the arrangement is on the page: the first measure and row shown,
/// how many of each, the size of the body and where the cursor is.
pub(super) struct Page {
    pub(super) first_measure: u32,
    pub(super) first_row: usize,
    pub(super) shown_measures: u32,
    pub(super) shown_rows: usize,
    /// Measures of the song and the room to add clips after them.
    pub(super) grid_total: u32,
    pub(super) body_w: i32,
    body_h: i32,
    cursor_measure: f64,
}

impl Page {
    fn new(view: &SongView, layout: &SongLayout, total: u32) -> Self {
        let (first_measure, first_row) = view.scroll;
        let grid_total = total + view.room;
        let shown_measures = layout.visible_measures().min(grid_total.saturating_sub(first_measure));
        let shown_rows = layout.visible_rows().min(view.row_count().saturating_sub(first_row));
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
    super::ruler::draw(canvas, view, layout, &page);
    draw_row_names(canvas, view, layout, &page);
    super::automation::draw_lanes(canvas, song, layout, view, &page);
    draw_measure_lines(canvas, layout, &page);
    draw_clips(canvas, view, song, layout);
    draw_dragged_pattern(canvas, view, layout, &page);
    draw_row_drop(canvas, view, layout, &page);
    // The cursor, from the ruler down: where the song plays, else (dimmer)
    // where play starts.
    if (f64::from(page.first_measure)..f64::from(page.first_measure + page.shown_measures))
        .contains(&page.cursor_measure)
    {
        let x = layout.measure_x(page.cursor_measure, page.first_measure);
        let color = if view.playing { CURSOR } else { CURSOR_IDLE };
        let top = layout.ruler_top();
        canvas.fill_rect(x - s.px(), top, 2 * s, layout.body_top() - top + page.body_h, color);
    }
    draw_scroll_bars(canvas, view, layout, &page);
    super::automation::draw_add_button(canvas, view, layout);
}

/// The rows: the name in the frozen column, a line under each.
fn draw_row_names(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    for (k, row) in view.rows.iter().enumerate().skip(page.first_row).take(page.shown_rows) {
        let y = layout.row_y(k, page.first_row);
        let line_w = layout.label_width() + page.body_w;
        canvas.fill_rect(layout.grid.x, y + layout.row_height() - s.px(), line_w, s.px(), line_color());
        let text_y = y + (layout.row_height() - 8 * s) / 2;
        if view.hovered == Some(SongHit::RowName(k)) {
            draw_hovered_name(canvas, layout, (y, layout.row_height() - s.px()), text_y);
        }
        let name = fit(&row.name, layout.label_width() - 6 * s - text_width(MENU_MARK, s), s);
        let color = if row.orphan { ORPHAN } else { theme::text() };
        canvas.text(layout.grid.x, text_y, &name, s, color);
    }
}

/// A name of the frozen column under the pointer, its row at `(top, height)`:
/// a lighter cell and, at its right end, the mark of its menu.
pub(super) fn draw_hovered_name(canvas: &mut Canvas, layout: &SongLayout, (top, height): (i32, i32), text_y: i32) {
    let s = layout.scale;
    let w = layout.label_width() - 4 * s;
    canvas.fill_rect(layout.grid.x - 2 * s, top, w, height, theme::highlight());
    canvas.text(layout.grid.x + w - 4 * s - text_width(MENU_MARK, s), text_y, MENU_MARK, s, theme::muted());
}

/// The narrowest measure (scale units) that keeps its line: narrower, only every fourth is drawn.
const MINOR_LINES: i32 = 6;

/// The measure lines, every fourth brighter; only those when they are too close.
fn draw_measure_lines(canvas: &mut Canvas, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    let minor = layout.measure_width() >= MINOR_LINES * s;
    for m in page.first_measure..=page.first_measure + page.shown_measures {
        if !minor && m % 4 != 0 {
            continue;
        }
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
            let color = theme::track(row);
            let color =
                if view.hovered == Some(SongHit::Clip(i)) { theme::mix(color, theme::WHITE, 0.25) } else { color };
            canvas.fill_rect(r.x, r.y, r.w, r.h, color);
            if view.selected_clip == Some(i) {
                draw_outline(canvas, r, 2 * layout.scale.px(), ACCENT);
            }
        }
    }
}

/// A frame `thickness` thick inside `r`, in `color`.
fn draw_outline(canvas: &mut Canvas, r: Rect, thickness: i32, color: Rgb) {
    let t = thickness.min(r.w / 2).min(r.h / 2).max(1);
    canvas.fill_rect(r.x, r.y, r.w, t, color);
    canvas.fill_rect(r.x, r.y + r.h - t, r.w, t, color);
    canvas.fill_rect(r.x, r.y, t, r.h, color);
    canvas.fill_rect(r.x + r.w - t, r.y, t, r.h, color);
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
        (Axis::Measures, layout.visible_measures() as usize, page.grid_total as usize),
        (Axis::Rows, layout.visible_rows(), view.row_count()),
    ];
    for (axis, shown, total) in bars {
        let r = layout.scroll_bar(axis);
        canvas.fill_rect(r.x, r.y, r.w, r.h, cell_color());
        let (start, len) = layout.bar_thumb(view, axis);
        let color = if total > shown { theme::muted() } else { line_color() };
        match axis {
            Axis::Measures => canvas.fill_rect(start, r.y + s.px(), len, r.h - 2 * s, color),
            Axis::Rows | Axis::List => canvas.fill_rect(r.x + s.px(), start, r.w - 2 * s, len, color),
        }
    }
    draw_zoom_handles(canvas, view, layout);
}

/// The handles at the ends of the measures' thumb, which zoom: each a
/// lighter end with a grip line.
fn draw_zoom_handles(canvas: &mut Canvas, view: &SongView, layout: &SongLayout) {
    let s = layout.scale;
    let r = layout.scroll_bar(Axis::Measures);
    let (start_handle, end_handle) = layout.handle_spans(layout.bar_thumb(view, Axis::Measures));
    for (x, w) in start_handle.into_iter().chain([end_handle]) {
        canvas.fill_rect(x, r.y + s.px(), w, r.h - 2 * s, theme::text());
        canvas.fill_rect(x + w / 2 - s.px() / 2, r.y + 4 * s, s.px(), r.h - 8 * s, theme::muted());
    }
}
