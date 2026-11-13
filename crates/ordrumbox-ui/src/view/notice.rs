//! Tooltips, toasts and notices drawn over the screen.

use super::*;

/// The hover bubble over a note (`pp-tooltip`): centered above `cell`, kept in the window.
pub fn draw_tooltip(canvas: &mut Canvas, text: &str, cell: Rect, scale: Scale) {
    draw_tooltip_lines(canvas, &[text], cell, scale);
}

/// A bubble of several lines over `cell`: above it, or below it when there
/// is no room above, kept in the window.
pub fn draw_tooltip_lines<S: AsRef<str>>(canvas: &mut Canvas, lines: &[S], cell: Rect, scale: Scale) {
    let line_h = 10 * scale;
    let widest = lines.iter().map(|l| text_width(l.as_ref(), scale)).max().unwrap_or(0);
    let (w, h) = (widest + 12 * scale, lines.len() as i32 * line_h + 4 * scale);
    let x = (cell.x + cell.w / 2 - w / 2).clamp(0, (canvas.width() - w).max(0));
    let above = cell.y - h - 4 * scale;
    let y = if above >= 0 { above } else { (cell.y + cell.h + 4 * scale).min((canvas.height() - h).max(0)) };
    canvas.fill_rect(x, y, w, h, theme::line());
    canvas.fill_rect(x + scale.px(), y + scale.px(), w - 2 * scale, h - 2 * scale, theme::shade());
    for (i, line) in lines.iter().enumerate() {
        canvas.text(x + 6 * scale, y + 3 * scale + i as i32 * line_h, line.as_ref(), scale, theme::text());
    }
}

/// How opaque a toast's background is (0–255): the window shows through.
pub(super) const TOAST_OPACITY: u8 = 225;

/// Where the toast of `message` goes in a window `width` wide, its lower
/// edge at `bottom`, and the text it shows.
pub(super) fn toast_layout(width: i32, message: &str, bottom: i32, scale: Scale) -> (Rect, String) {
    let big = scale.times(2);
    let (margin, pad_x, pad_y) = (margin(scale), 24 * scale, 12 * scale);
    let room = width - 2 * margin - 2 * pad_x;
    let text: String = message.chars().take(chars_in(room, big)).collect();
    let w = (text_width(&text, big) + 2 * pad_x).max((320 * scale).min(width - 2 * margin));
    let h = font::SIZE * big + 2 * pad_y;
    (Rect::new((width - w) / 2, bottom - h, w, h), text)
}

/// The space between two stacked toasts.
#[must_use]
pub fn toast_gap(scale: Scale) -> i32 {
    6 * scale
}

/// The rectangle of the toast of `message` (a click on it hides it).
#[must_use]
pub fn toast_rect(width: i32, message: &str, bottom: i32, scale: Scale) -> Rect {
    toast_layout(width, message, bottom, scale).0
}

/// A new message as a toast centred over the bottom of the window, in a
/// larger font, its lower edge at `bottom`: over the rest, so that nothing
/// moves.
pub fn draw_toast(canvas: &mut Canvas, message: &str, bottom: i32, scale: Scale) {
    let big = scale.times(2);
    let (pad_y, border) = (12 * scale, scale.px());
    let (Rect { x, y, w, h }, text) = toast_layout(canvas.width(), message, bottom, scale);
    // Each pixel shaded once, so that the window shows through evenly.
    canvas.shade_rect(x + border, y + border, w - 2 * border, h - 2 * border, theme::toast(), TOAST_OPACITY);
    let side = h - 2 * border;
    let edges = [
        (x, y, w, border),
        (x, y + h - border, w, border),
        (x, y + border, border, side),
        (x + w - border, y + border, border, side),
    ];
    for (sx, sy, sw, sh) in edges {
        canvas.shade_rect(sx, sy, sw, sh, theme::toast_edge(), TOAST_OPACITY);
    }
    let text_x = x + (w - text_width(&text, big)) / 2;
    canvas.text(text_x, y + pad_y, &text, big, theme::text());
}

/// A panel over the window: a title and lines of text (the key list, about).
pub fn draw_notice(canvas: &mut Canvas, title: &str, lines: &[String], scale: Scale) {
    draw_notice_part(canvas, title, lines, scale, None);
}

/// A notice with two buttons under its lines: `buttons.0` is the default
/// (ENTER closes, nothing done), `focus` says which one ENTER would run,
/// shown in brackets and in colour.
pub fn draw_notice_choice(
    canvas: &mut Canvas,
    title: &str,
    lines: &[String],
    buttons: [&str; 2],
    focus: usize,
    scale: Scale,
) {
    draw_notice_part(canvas, title, lines, scale, Some((buttons, focus)));
}

/// Height of a notice's line, per scale unit.
pub(super) const NOTICE_LINE: i32 = 11;

/// How many lines a notice shows at most in a window `height` pixels high.
pub fn notice_rows(height: i32, scale: Scale) -> usize {
    let room = height - menu::bar_height(scale) - 24 * scale;
    usize::try_from(room / (NOTICE_LINE * scale) - 3).unwrap_or(0).max(1)
}

/// A notice too long for the window, from line `scroll` on: as many lines
/// as fit, the title saying which and how to see the others.
pub fn draw_notice_scrolled(canvas: &mut Canvas, title: &str, lines: &[String], scroll: usize, scale: Scale) {
    let rows = notice_rows(canvas.height(), scale);
    if lines.len() <= rows {
        return draw_notice(canvas, title, lines, scale);
    }
    let first = scroll.min(lines.len() - rows);
    let title = format!("{title}  {}-{} of {}, wheel or arrows: more", first + 1, first + rows, lines.len());
    draw_notice_part(canvas, &title, &lines[first..first + rows], scale, None);
}

/// Where a notice of `lines` is in a window of `window` pixels: its left,
/// top, width and height. `buttons_width` is the width of its line of
/// buttons, when it has one.
pub fn notice_box(
    window: (i32, i32),
    title: &str,
    lines: &[String],
    buttons_width: i32,
    has_buttons: bool,
    scale: Scale,
) -> (i32, i32, i32, i32) {
    let line_h = NOTICE_LINE * scale;
    let close = "ESC or click: close";
    // The title shares its line with `close`.
    let title_width = text_width(title, scale.title()) + 16 * scale + text_width(close, scale);
    let lines_width = lines.iter().map(|l| text_width(l, scale)).max().unwrap_or(0).max(buttons_width);
    let width = lines_width.max(title_width) + 32 * scale;
    let height = (lines.len() as i32 + 3 + i32::from(has_buttons)) * line_h + 16 * scale;
    let x = ((window.0 - width) / 2).max(0);
    let y = ((window.1 - height) / 2).max(menu::bar_height(scale));
    (x, y, width, height)
}

/// The line of a notice's `lines` that `(px, py)` is on, in a window of
/// `window` pixels; `None` outside the notice or off its lines.
pub fn notice_line_at(
    window: (i32, i32),
    title: &str,
    lines: &[String],
    scale: Scale,
    px: i32,
    py: i32,
) -> Option<usize> {
    let (x, y, width, height) = notice_box(window, title, lines, 0, false, scale);
    if px < x || px >= x + width || py < y || py >= y + height {
        return None;
    }
    let line_h = NOTICE_LINE * scale;
    let first = y + 10 * scale + 2 * line_h;
    usize::try_from((py - first).div_euclid(line_h)).ok().filter(|&i| py >= first && i < lines.len())
}

/// Whether `(px, py)` is inside a notice of `lines`.
pub fn notice_contains(window: (i32, i32), title: &str, lines: &[String], scale: Scale, px: i32, py: i32) -> bool {
    let (x, y, width, height) = notice_box(window, title, lines, 0, false, scale);
    (x..x + width).contains(&px) && (y..y + height).contains(&py)
}

/// A notice showing `lines`, as wide as the widest of them (`choice` adds
/// its two buttons as one more line).
pub(super) fn draw_notice_part(
    canvas: &mut Canvas,
    title: &str,
    lines: &[String],
    scale: Scale,
    choice: Option<([&str; 2], usize)>,
) {
    let line_h = NOTICE_LINE * scale;
    let close = "ESC or click: close";
    // The buttons share one line, the one ENTER runs in brackets.
    let buttons = choice.map(|(buttons, focus)| {
        let (a, b) = (buttons[0], buttons[1]);
        let first = if focus == 0 { format!("[ {a} ]") } else { a.to_owned() };
        let second = if focus == 0 { b.to_owned() } else { format!("[ {b} ]") };
        (format!("{first}   {second}"), first, second, focus)
    });
    let buttons_width = buttons.as_ref().map_or(0, |(line, ..)| text_width(line, scale));
    let (x, y, width, height) =
        notice_box((canvas.width(), canvas.height()), title, lines, buttons_width, buttons.is_some(), scale);
    canvas.fill_rect(x - 4 * scale, y - 4 * scale, width + 8 * scale, height + 8 * scale, theme::shade());
    canvas.fill_rect(x, y, width, height, theme::line());
    canvas.fill_rect(x + scale.px(), y + scale.px(), width - 2 * scale, height - 2 * scale, theme::panel());
    canvas.text(x + 16 * scale, y + 10 * scale, title, scale.title(), ACCENT);
    for (i, line) in lines.iter().enumerate() {
        canvas.text(x + 16 * scale, y + 10 * scale + (i as i32 + 2) * line_h, line, scale, theme::text());
    }
    if let Some((_, first, second, focus)) = &buttons {
        let y = y + 10 * scale + (lines.len() as i32 + 2) * line_h;
        let (first_color, second_color) = if *focus == 0 { (ACCENT, theme::muted()) } else { (theme::muted(), ACCENT) };
        canvas.text(x + 16 * scale, y, first, scale, first_color);
        canvas.text(x + 16 * scale + text_width(first, scale) + 3 * scale, y, second, scale, second_color);
    }
    canvas.text(x + width - 16 * scale - text_width(close, scale), y + 10 * scale, close, scale, theme::muted());
}
