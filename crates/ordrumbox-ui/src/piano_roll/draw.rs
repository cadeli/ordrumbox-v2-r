//! Drawing the piano roll: the keys and rows, the step columns, the notes
//! and their ghosts, the zone, the cursor and the playhead.

use super::*;

/// Draw the piano roll; `title` names the track.
pub fn draw(canvas: &mut Canvas, view: &RollView, layout: &RollLayout, title: &str) {
    let s = layout.scale;
    canvas.text(layout.title.x, layout.title.y, title, s, theme::text());
    if view.pages() > 1 {
        draw_pager(canvas, view, layout);
    }

    let (start, end) = view.page_steps();
    let first_row = layout.first_row(view);
    let grid_h = (view.top_row - first_row + 1) * layout.row;
    draw_rows(canvas, view, layout);
    draw_columns(canvas, view, layout, grid_h);
    // The loop point.
    if let Some(loop_length) = view.track.loop_length.map(i64::from).filter(|&l| l > start && l <= end) {
        canvas.fill_rect(layout.step_x(view, loop_length) - s.px(), layout.grid.y, 2 * s, grid_h, LOOP_MARK);
    }
    draw_ghosts(canvas, view, layout);
    for n in 0..view.track.notes.len() {
        if let Some(r) = layout.note_rect(view, n) {
            draw_note(canvas, view, n, r, s);
        }
    }
    if let Some(zone) = &view.zone {
        draw_zone(canvas, view, layout, zone);
    }
    // The cursor, when no note is selected.
    if let Some((step, row)) = view.cursor.filter(|_| view.selected.is_none())
        && step >= start
        && step < end
        && layout.shown_row(view, row)
    {
        let r = Rect::new(layout.step_x(view, step), layout.row_y(view.top_row, row), layout.cell, layout.row);
        stroke(canvas, r, 2 * s, CURSOR);
    }
    if let Some(step) = view.playhead.filter(|&p| p >= start && p < end) {
        canvas.fill_rect(layout.step_x(view, step), layout.grid.y, 2 * s, grid_h, PLAYHEAD);
    }
    if first_row > 0 || view.top_row < TOTAL_KEYS - 1 {
        let hint = format!("{} - {}", midi_name(row_midi(view.top_row)), midi_name(row_midi(first_row)));
        canvas.text(layout.keys.x, layout.grid.y + grid_h + 4 * s, &hint, s, theme::muted());
    }
}

/// The buttons that turn the pages, and the page's number between them.
fn draw_pager(canvas: &mut Canvas, view: &RollView, layout: &RollLayout) {
    let s = layout.scale;
    for (rect, label, enabled) in
        [(layout.previous, "<", view.page > 0), (layout.next, ">", view.page < view.pages() - 1)]
    {
        canvas.fill_rect(rect.x, rect.y, rect.w, rect.h, if enabled { column_half_color() } else { row_black_color() });
        canvas.text(
            rect.x + (rect.w - 8 * s) / 2,
            rect.y + 3 * s,
            label,
            s,
            if enabled { theme::text() } else { theme::muted() },
        );
    }
    let info = format!("{}/{}", view.page + 1, view.pages());
    let x = layout.previous.x
        + layout.previous.w
        + (layout.next.x - layout.previous.x - layout.previous.w - text_width(&info, s)) / 2;
    canvas.text(x, layout.title.y, &info, s, theme::text());
}

/// The keys and the row backgrounds, an octave line under each C.
fn draw_rows(canvas: &mut Canvas, view: &RollView, layout: &RollLayout) {
    let s = layout.scale;
    let (start, end) = view.page_steps();
    let width = (end - start) as i32 * layout.cell;
    for row in layout.first_row(view)..=view.top_row {
        let y = layout.row_y(view.top_row, row);
        let midi = row_midi(row);
        let black = BLACK_KEYS.contains(&midi.rem_euclid(12));
        let c = midi.rem_euclid(12) == 0;
        let k = layout.keys;
        canvas.fill_rect(
            k.x,
            y,
            k.w - s.px(),
            layout.row - s.px(),
            if black { key_black_color() } else { key_white_color() },
        );
        if c {
            canvas.text(
                k.x + k.w - 8 * s - text_width(&midi_name(midi), s),
                y + (layout.row - 8 * s) / 2,
                &midi_name(midi),
                s,
                background_color(),
            );
        }
        canvas.fill_rect(
            layout.grid.x,
            y,
            width,
            layout.row,
            if black { row_black_color() } else { row_white_color() },
        );
        if c {
            canvas.fill_rect(layout.grid.x, y + layout.row - s.px(), width, s.px(), octave_color());
        }
    }
}

/// The step columns: beats, half beats, steps.
fn draw_columns(canvas: &mut Canvas, view: &RollView, layout: &RollLayout, grid_h: i32) {
    let spb = steps_per_beat(view.track);
    let (start, end) = view.page_steps();
    for step in start..end {
        let in_beat = (step % i64::from(spb)) as i32;
        let color = if in_beat == 0 {
            column_beat_color()
        } else if spb >= 4 && in_beat * 2 == spb {
            column_half_color()
        } else {
            column_step_color()
        };
        canvas.fill_rect(layout.step_x(view, step), layout.grid.y, layout.scale.px(), grid_h, color);
    }
}

/// The repeats a note makes (euclidean, retriggers), as small squares.
fn draw_ghosts(canvas: &mut Canvas, view: &RollView, layout: &RollLayout) {
    let (start, end) = view.page_steps();
    for (n, note) in view.track.notes.iter().enumerate() {
        let row = note_row(note, view.track);
        for sub in &view.subs[n] {
            let ghost_row = row + sub.pitch_offset.round() as i32;
            if sub.step < start || sub.step >= end || !layout.shown_row(view, ghost_row) {
                continue;
            }
            let (x, y) = (layout.step_x(view, sub.step), layout.row_y(view.top_row, ghost_row));
            // The same small rounded square as in the pattern grid: half a cell, centred.
            let base = if sub.kind == SubKind::Euclidean { theme::GENERATED } else { theme::muted() };
            let color = theme::note(base, note.velocity);
            let (w, h) = ((layout.cell / 2).max(1), (layout.row / 2).max(1));
            let (gx, gy) = (x + (layout.cell - w) / 2, y + (layout.row - h) / 2);
            let gradient = (theme::mix(color, 0xff_ff_ff, NOTE_LIGHT), theme::mix(color, 0x00_00_00, NOTE_DARK));
            canvas.fill_rounded_gradient(gx, gy, w, h, gradient, layout.scale.px());
        }
    }
}

/// Note `n` in `r`: its relief, its ring when selected, its velocity bar and
/// its trigger marker.
fn draw_note(canvas: &mut Canvas, view: &RollView, n: usize, r: Rect, s: Scale) {
    let note = &view.track.notes[n];
    let lit = view.playhead.is_some_and(|step| view.sounding(n, step));
    let color = theme::mix(
        background_color(),
        if lit { PLAYING } else { view.color },
        note_opacity(note.velocity.clamp(0.0, 1.0)),
    );
    let radius = (2 * s.px()).min(r.w / 2).min(r.h / 2);
    let selected = view.selected == Some(n);
    // A playing note glows in the margin around it.
    if lit {
        canvas.shade_rect(r.x - s.px(), r.y - s.px(), r.w + 2 * s, r.h + 2 * s, PLAYING, NOTE_GLOW);
    }
    // Relief: a soft shadow in the margin below, a gradient, a light top edge.
    canvas.shade_rect(r.x + radius, r.y + r.h, r.w - 2 * radius, s.px(), 0x00_00_00, NOTE_SHADOW);
    // A selected note is ringed: a white rounded shape, the note drawn inside it.
    let body = if selected {
        let ring = s.px();
        canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (SELECTED, SELECTED), radius);
        Rect::new(r.x + ring, r.y + ring, r.w - 2 * ring, r.h - 2 * ring)
    } else {
        r
    };
    let light = if lit { NOTE_LIGHT_PLAYING } else { NOTE_LIGHT };
    canvas.fill_rounded_gradient(
        body.x,
        body.y,
        body.w,
        body.h,
        (theme::mix(color, 0xff_ff_ff, light), theme::mix(color, 0x00_00_00, NOTE_DARK)),
        (radius - if selected { s.px() } else { 0 }).max(0),
    );
    canvas.shade_rect(body.x + radius, body.y, body.w - 2 * radius, s.px(), 0xff_ff_ff, NOTE_SHEEN);
    // The velocity, as a bar along the bottom edge.
    let bar = ((f64::from(body.w - 2 * radius) * note.velocity.clamp(0.0, 1.0)).round() as i32).max(s.px());
    canvas.shade_rect(body.x + radius, body.y + body.h - 2 * s, bar, s.px(), 0xff_ff_ff, VELOCITY_BAR);
    // The trigger kind, a marker in the top right corner: probability wins over the cycle.
    let size = 3 * s.px();
    let (mx, my) = (body.x + body.w - size - s.px(), body.y + s.px());
    if note.probability < 1.0 {
        canvas.fill_rounded_gradient(mx, my, size, size, (TRIG_RAND, TRIG_RAND), size / 2);
    } else if note.cycle > 1 {
        diamond(canvas, mx, my, size, TRIG_FIXED);
    }
}

/// The zone over its steps and its rows on the page: shaded, outlined.
fn draw_zone(canvas: &mut Canvas, view: &RollView, layout: &RollLayout, zone: &RollSpan) {
    let (start, end) = view.page_steps();
    let (first, last) = (zone.start_step.max(start), zone.end_step.min(end));
    let (low, high) = (zone.low_row.max(layout.first_row(view)), zone.high_row.min(view.top_row));
    if first >= last || low > high {
        return;
    }
    let (left, right) = (layout.step_x(view, first), layout.step_x(view, last));
    let (top, bottom) = (layout.row_y(view.top_row, high), layout.row_y(view.top_row, low) + layout.row);
    let r = Rect::new(left, top, right - left, bottom - top);
    canvas.shade_rect(r.x, r.y, r.w, r.h, ZONE, ZONE_OPACITY);
    stroke(canvas, r, layout.scale.px(), ZONE);
}

/// A diamond filling the `size` square at `(x, y)`.
fn diamond(canvas: &mut Canvas, x: i32, y: i32, size: i32, color: Rgb) {
    let half = size / 2;
    for row in 0..size {
        let inset = (half - row).abs().min(half);
        canvas.fill_rect(x + inset, y + row, size - 2 * inset, 1, color);
    }
}
