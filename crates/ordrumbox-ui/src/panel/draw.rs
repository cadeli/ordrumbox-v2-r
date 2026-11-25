//! Drawing the items of a panel.

use super::*;

fn fill_circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, color: Rgb) {
    for dy in -r..=r {
        let half = f64::from(r * r - dy * dy).sqrt() as i32;
        canvas.fill_rect(cx - half, cy + dy, 2 * half + 1, 1, color);
    }
}

/// A label centred in `r`, cut to its width so it never spills out.
fn centered_label(canvas: &mut Canvas, r: Rect, label: &str, scale: Scale, color: Rgb) {
    let label = fit(label, r.w - 2 * scale, scale);
    let x = r.x + (r.w - text_width(&label, scale)) / 2;
    canvas.text(x, r.y + (r.h - 8 * scale) / 2, &label, scale, color);
}

/// A knob: a 270° arc from bottom left, lit up to `fraction`, a disc and its pointer.
fn draw_knob(canvas: &mut Canvas, cx: i32, cy: i32, size: i32, fraction: f64, color: Rgb, scale: Scale) {
    let r = f64::from(size) / 2.0;
    let dot = scale.px() * if size >= 20 * scale { 2 } else { 1 };
    let start = 135f64.to_radians();
    let sweep = 270f64.to_radians();
    let steps = (r * 6.0) as i32 + 24;
    for i in 0..=steps {
        let f = f64::from(i) / f64::from(steps);
        let a = start + sweep * f;
        let x = cx + (a.cos() * (r - f64::from(dot))) as i32;
        let y = cy + (a.sin() * (r - f64::from(dot))) as i32;
        canvas.fill_rect(x - dot / 2, y - dot / 2, dot, dot, if f <= fraction { color } else { border_color() });
    }
    let disc = (r * 0.62) as i32;
    fill_circle(canvas, cx, cy, disc, disc_color());
    let a = start + sweep * fraction;
    for k in 0..=disc {
        let x = cx + (a.cos() * f64::from(k)) as i32;
        let y = cy + (a.sin() * f64::from(k)) as i32;
        canvas.fill_rect(x - scale.px() / 2, y - scale.px() / 2, scale.px(), scale.px(), theme::text());
    }
}

/// Draw `items`; `focus` gets an outline.
pub fn draw(canvas: &mut Canvas, items: &[Item], focus: Option<Id>, scale: Scale) {
    let t = scale.px();
    for item in items {
        let r = item.rect;
        draw_item(canvas, item, scale);
        if item.id.is_some() && item.id == focus {
            stroke(canvas, Rect::new(r.x - 2 * t, r.y - 2 * t, r.w + 4 * t, r.h + 4 * t), t, FOCUS);
        }
    }
}

/// One item, by what it is.
fn draw_item(canvas: &mut Canvas, item: &Item, scale: Scale) {
    let t = scale.px();
    let r = item.rect;
    match &item.kind {
        Kind::Highlight => canvas.fill_rect(r.x, r.y, r.w, r.h, selected_row_color()),
        Kind::Panel => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, theme::bg());
            canvas.fill_rect(r.x, r.y, 2 * t, r.h, theme::ACCENT);
        }
        Kind::Card => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
            canvas.fill_rect(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, background_color());
        }
        Kind::Mark { color } => canvas.fill_rect(r.x, r.y, r.w, r.h, *color),
        Kind::Title { text } => {
            canvas.text(r.x, r.y, text, scale.title(), TITLE);
        }
        Kind::Text { text, color } => {
            canvas.text(r.x, r.y, &fit(text, r.w.max(text_width(text, scale)), scale), scale, *color);
        }
        Kind::Value { spec, value, label, text, lfo } => {
            draw_value_item(canvas, r, (spec, *value, *lfo), (label.as_deref(), text), scale);
        }
        Kind::Select { labels, selected, .. } => draw_select_item(canvas, r, labels, *selected, scale),
        Kind::Screen { labels, selected, lit, .. } => draw_screen(canvas, r, labels, *selected, *lit, scale),
        Kind::Toggle { on } => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ON } else { field_color() });
            let text = if *on { "ON" } else { "OFF" };
            let color = if *on { background_color() } else { dim() };
            canvas.text(r.x + (r.w - text_width(text, scale)) / 2, r.y + (r.h - 8 * scale) / 2, text, scale, color);
        }
        Kind::Led { on } => {
            let s = 6 * scale;
            let (x, y) = (r.x + (r.w - s) / 2, r.y + (r.h - s) / 2);
            canvas.fill_rect(x, y, s, s, if *on { ON } else { border_color() });
        }
        Kind::Tab { label, active } => {
            if *active {
                canvas.fill_rect(r.x, r.y + r.h - 2 * t, r.w, 2 * t, ACTIVE);
            }
            let color = if *active { ACTIVE } else { dim() };
            centered_label(canvas, r, label, scale, color);
        }
        Kind::MainTab { label, active } => {
            let (fill, color) = if *active { (ACTIVE, background_color()) } else { (field_color(), theme::text()) };
            canvas.fill_rect(r.x, r.y, r.w, r.h, fill);
            // The line under the bar, through the gap to the next tab.
            canvas.fill_rect(r.x, r.y + r.h - 2 * t, r.w + t, 2 * t, ACTIVE);
            centered_label(canvas, r, label, scale, color);
        }
        Kind::Button { label, on } => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ACTIVE } else { field_color() });
            let color = if *on { background_color() } else { theme::text() };
            centered_label(canvas, r, label, scale, color);
        }
        Kind::Disabled { label } => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
            centered_label(canvas, r, label, scale, theme::disabled());
        }
        Kind::Envelope { points, overlay } => draw_envelope_item(canvas, r, points, overlay.as_ref(), scale),
    }
}

/// A knob, slider, stepper or fader (by the style of `spec`) at `value`, in
/// the LFO's color when `lfo` drives it, with its `label` and `text`.
fn draw_value_item(
    canvas: &mut Canvas,
    r: Rect,
    (spec, value, lfo): (&Spec, f64, bool),
    (label, text): (Option<&str>, &str),
    scale: Scale,
) {
    let fraction = spec.fraction(value);
    let color = if lfo { LFO } else { arc_color() };
    match spec.style {
        Style::Knob => draw_knob_item(canvas, r, fraction, color, label, text, scale),
        Style::Slider => draw_slider_item(canvas, r, fraction, color, label, text, scale),
        Style::Stepper => {
            let room = StepperRoom { down: value > spec.min, up: value < spec.max };
            draw_stepper_item(canvas, r, room, label, text, scale);
        }
        Style::Fader => draw_fader_item(canvas, r, fraction, color, text, scale),
    }
}

/// A drop-down list showing the label of option `selected`, and its mark.
fn draw_select_item(canvas: &mut Canvas, r: Rect, labels: &[String], selected: usize, scale: Scale) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
    let label = labels.get(selected).map_or("", String::as_str);
    let ty = r.y + (r.h - 8 * scale) / 2;
    canvas.text(r.x + 4 * scale, ty, &fit(label, r.w - 16 * scale, scale), scale, theme::text());
    canvas.text(r.x + r.w - 10 * scale, ty, MENU_MARK, scale, dim());
}

/// A one-line screen: dark glass in a frame, the label of `selected` lit or dimmed.
fn draw_screen(canvas: &mut Canvas, r: Rect, labels: &[String], selected: usize, lit: bool, scale: Scale) {
    let t = scale.px();
    canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
    canvas.fill_rect(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, theme::screen());
    let label = labels.get(selected).map_or("", String::as_str);
    let color = if lit { ON } else { dim() };
    let ty = r.y + (r.h - 8 * scale) / 2;
    canvas.text(r.x + 3 * scale, ty, &fit(label, r.w - 6 * scale, scale), scale, color);
}

/// A knob with its value: under it in a tall item, beside it in a short one.
fn draw_knob_item(
    canvas: &mut Canvas,
    r: Rect,
    fraction: f64,
    color: Rgb,
    label: Option<&str>,
    text: &str,
    scale: Scale,
) {
    let size = knob_size(r, scale);
    if r.h < 40 * scale {
        draw_knob(canvas, r.x + size / 2, r.y + r.h / 2, size, fraction, color, scale);
        let x = r.x + size + 6 * scale;
        canvas.text(x, r.y + (r.h - 8 * scale) / 2, &fit(text, r.x + r.w - x, scale), scale, value_color());
        return;
    }
    let cx = r.x + r.w / 2;
    draw_knob(canvas, cx, r.y + size / 2, size, fraction, color, scale);
    let mut y = r.y + size + 3 * scale;
    if let Some(label) = label {
        let label = fit(label, r.w, scale);
        canvas.text(cx - text_width(&label, scale) / 2, y, &label, scale, dim());
        y += 10 * scale;
    }
    let text = fit(text, r.w, scale);
    canvas.text(cx - text_width(&text, scale) / 2, y, &text, scale, value_color());
}

/// A slider: a thin line lit up to the value, a square handle, the value text.
fn draw_slider_item(
    canvas: &mut Canvas,
    r: Rect,
    fraction: f64,
    color: Rgb,
    label: Option<&str>,
    text: &str,
    scale: Scale,
) {
    let t = scale.px();
    let ty = r.y + (r.h - 8 * scale) / 2;
    if let Some(label) = label {
        canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, dim());
    }
    let (left, width) = slider_bar(r, label.is_some(), !text.is_empty(), scale);
    let bar_y = r.y + r.h / 2 - scale.px() / 2;
    canvas.fill_rect(left, bar_y, width, 2 * t, border_color());
    let filled = (fraction * f64::from(width)) as i32;
    canvas.fill_rect(left, bar_y, filled, 2 * t, color);
    let thumb = left + filled - 4 * scale;
    canvas.fill_rect(thumb, r.y + r.h / 2 - 4 * scale, 8 * scale, 8 * scale, handle_color());
    let x = left + width + 6 * scale;
    canvas.text(x, ty, &fit(text, r.x + r.w - x, scale), scale, value_color());
}

/// A fader: a groove lit from the foot up to the value, a wide handle, the
/// value text under it.
fn draw_fader_item(canvas: &mut Canvas, r: Rect, fraction: f64, color: Rgb, text: &str, scale: Scale) {
    let t = scale.px();
    let (top, height) = fader_bar(r, scale);
    let cx = r.x + r.w / 2;
    canvas.fill_rect(cx - t, top, 2 * t, height, border_color());
    let filled = (fraction * f64::from(height)) as i32;
    canvas.fill_rect(cx - t, top + height - filled, 2 * t, filled, color);
    let (handle_w, handle_h) = ((r.w - 4 * scale).min(24 * scale), 8 * scale);
    canvas.fill_rect(cx - handle_w / 2, top + height - filled - handle_h / 2, handle_w, handle_h, handle_color());
    let text = fit(text, r.w, scale);
    canvas.text(cx - text_width(&text, scale) / 2, top + height + 8 * scale, &text, scale, value_color());
}

/// Which buttons of a stepper can still move its value: the minus, the plus.
#[derive(Debug, Clone, Copy)]
struct StepperRoom {
    down: bool,
    up: bool,
}

/// A stepper: minus and plus buttons around the value; a button is greyed
/// out at its end of the range.
fn draw_stepper_item(canvas: &mut Canvas, r: Rect, room: StepperRoom, label: Option<&str>, text: &str, scale: Scale) {
    let t = scale.px();
    let ty = r.y + (r.h - 8 * scale) / 2;
    if let Some(label) = label {
        canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, dim());
    }
    let [down, field, up] = stepper_parts(r, label.is_some(), scale);
    for (button, glyph, live) in [(down, "-", room.down), (up, "+", room.up)] {
        canvas.fill_rect(button.x + t, button.y + t, button.w - 2 * t, button.h - 2 * t, field_color());
        let x = button.x + (button.w - text_width(glyph, scale)) / 2;
        canvas.text(x, ty, glyph, scale, if live { theme::text() } else { dim() });
    }
    let text = fit(text, field.w, scale);
    canvas.text(field.x + (field.w - text_width(&text, scale)) / 2, ty, &text, scale, value_color());
}

/// A sample's waveform, mirrored around the middle and scaled to its own
/// peak: a quiet sample shows its shape as well as a loud one
/// (`Canvas::draw_profile` does the drawing).
fn draw_envelope_item(canvas: &mut Canvas, r: Rect, points: &[f64], overlay: Option<&Overlay>, scale: Scale) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
    let peak = points.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let gain = if peak > 0.0 { 1.0 / peak } else { 0.0 };
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, scale.px(), theme::mix(field_color(), ENVELOPE, 0.4));
    for direction in [Direction::Up, Direction::Down] {
        canvas.draw_profile((r.x, mid, r.w), r.h / 2, (points, gain), direction, ENVELOPE);
    }
    if let Some(overlay) = overlay {
        overlay.draw(canvas, (r.x, r.y, r.w, r.h), scale);
    }
}
