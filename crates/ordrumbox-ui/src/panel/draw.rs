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
        Kind::Title { text } => {
            canvas.text(r.x, r.y, text, scale.title(), TITLE);
        }
        Kind::Text { text, color } => {
            canvas.text(r.x, r.y, &fit(text, r.w.max(text_width(text, scale)), scale), scale, *color);
        }
        Kind::Value { spec, value, label, text, lfo } => {
            let fraction = spec.fraction(*value);
            let color = if *lfo { LFO } else { arc_color() };
            match spec.style {
                Style::Knob => draw_knob_item(canvas, r, fraction, color, label.as_deref(), text, scale),
                Style::Slider => draw_slider_item(canvas, r, fraction, color, label.as_deref(), text, scale),
                Style::Stepper => {
                    draw_stepper_item(canvas, r, (*value > spec.min, *value < spec.max), label.as_deref(), text, scale)
                }
            }
        }
        Kind::Select { labels, selected, .. } => {
            canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
            let label = labels.get(*selected).map_or("", String::as_str);
            let ty = r.y + (r.h - 8 * scale) / 2;
            canvas.text(r.x + 4 * scale, ty, &fit(label, r.w - 16 * scale, scale), scale, theme::text());
            canvas.text(r.x + r.w - 10 * scale, ty, "v", scale, dim());
        }
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

/// A stepper: minus and plus buttons around the value; a button is greyed
/// out at its end of the range (`live`: the minus can go, the plus can go).
fn draw_stepper_item(canvas: &mut Canvas, r: Rect, live: (bool, bool), label: Option<&str>, text: &str, scale: Scale) {
    let t = scale.px();
    let ty = r.y + (r.h - 8 * scale) / 2;
    if let Some(label) = label {
        canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, dim());
    }
    let [down, field, up] = stepper_parts(r, label.is_some(), scale);
    for (button, glyph, live) in [(down, "-", live.0), (up, "+", live.1)] {
        canvas.fill_rect(button.x + t, button.y + t, button.w - 2 * t, button.h - 2 * t, field_color());
        let x = button.x + (button.w - text_width(glyph, scale)) / 2;
        canvas.text(x, ty, glyph, scale, if live { theme::text() } else { dim() });
    }
    let text = fit(text, field.w, scale);
    canvas.text(field.x + (field.w - text_width(&text, scale)) / 2, ty, &text, scale, value_color());
}

/// A sample's waveform, mirrored around the middle and scaled to its own
/// peak: a quiet sample shows its shape as well as a loud one.
fn draw_envelope_item(canvas: &mut Canvas, r: Rect, points: &[f64], overlay: Option<&Overlay>, scale: Scale) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
    let peak = points.iter().fold(0.0f64, |m, v| m.max(v.abs()));
    let gain = if peak > 0.0 { 1.0 / peak } else { 0.0 };
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, scale.px(), theme::mix(field_color(), ENVELOPE, 0.4));
    let n = points.len() as i32;
    for (i, v) in points.iter().enumerate() {
        let i = i as i32;
        let x = r.x + i * r.w / n.max(1);
        let w = ((i + 1) * r.w / n.max(1) - i * r.w / n.max(1)).max(1);
        let half = ((v.abs() * gain).min(1.0) * f64::from(r.h / 2)).round() as i32;
        if half > 0 {
            canvas.fill_rect(x, mid - half, w, 2 * half, ENVELOPE);
        }
    }
    if let Some(overlay) = overlay {
        overlay.draw(canvas, (r.x, r.y, r.w, r.h), scale);
    }
}
