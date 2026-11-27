//! The synth editor's hardware look: brushed metal around cards in the
//! colour scheme's hue, each with a darker title band; black knobs ringed
//! with ticks, faders with a light cap, round LEDs, dark buttons (yellow when
//! on) and green-on-black screens. The cards take their colours from the
//! scheme ([`Cards`]); the metal and the controls are the same in every one.
//! Gradients and soft edges are computed as it draws: nothing is an image.

use super::*;
use crate::control::Style;

/// The metal around the cards, top to bottom.
const METAL: (Rgb, Rgb) = (0x00C6_CCCF, 0x009C_A4A8);
/// Text on the metal.
const METAL_TEXT: Rgb = 0x0022_3036;
/// A screen: its glass, its frame, what it shows and its grid.
const LCD: Rgb = 0x0008_2329;
const LCD_EDGE: Rgb = 0x0003_1215;
pub(crate) const LCD_TEXT: Rgb = 0x008C_F5D2;
pub(crate) const LCD_DIM: Rgb = 0x0034_7A70;
pub(crate) const LCD_GRID: Rgb = 0x0013_3D43;
/// A knob: its body, the ring around it, its pointer and the ticks of its travel.
const KNOB: (Rgb, Rgb) = (0x0062_6B70, 0x000A_0C0E);
const KNOB_RIM: Rgb = 0x0004_0506;
const POINTER: Rgb = 0x00F4_F6F7;
/// A fader's cap and its line.
const CAP: (Rgb, Rgb) = (0x00F5_F7F8, 0x0096_A0A4);
const CAP_LINE: Rgb = 0x0028_3236;
/// A button, a button that is on (and a switch), their frame and text.
const BUTTON: (Rgb, Rgb) = (0x003C_5156, 0x0013_2227);
const BUTTON_ON: (Rgb, Rgb) = (0x00F8_F592, 0x00D6_D056);
const BUTTON_EDGE: Rgb = 0x0007_1114;
const BUTTON_TEXT: Rgb = 0x00EE_F6F7;
const BUTTON_ON_TEXT: Rgb = 0x001E_1E10;
/// A LED, lit and dark, and the glint on a lit one.
const LED_ON: (Rgb, Rgb) = (0x00C8_FFAE, 0x0036_C824);
const LED_OFF: (Rgb, Rgb) = (0x002E_5A32, 0x0016_3519);
const LED_RIM: Rgb = 0x0008_1A0A;
/// The 11 ticks around a knob's 270°.
const TICKS: i32 = 11;
const KNOB_START: f64 = 135.0;
const KNOB_SWEEP: f64 = 270.0;

/// The colours of the cards and of what is drawn in the card's hue, from a
/// colour scheme: its line colour, the scheme's middle tone, makes the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cards {
    /// A card, top to bottom, and a card switched off.
    body: (Rgb, Rgb),
    body_off: (Rgb, Rgb),
    edge: Rgb,
    /// A card's title band, and the line under it.
    head: (Rgb, Rgb),
    head_line: Rgb,
    /// Text on a card, and a greyed-out one.
    label: Rgb,
    label_dim: Rgb,
    /// The ticks around a knob, a fader's slot and the part of it under the cap.
    tick: Rgb,
    slot: Rgb,
    slot_lit: Rgb,
    /// A knob greyed out: its body.
    knob_off: (Rgb, Rgb),
}

impl Cards {
    /// The cards of `scheme`.
    #[must_use]
    pub(crate) fn from_scheme(scheme: &theme::Scheme) -> Self {
        let tone = theme::mix(scheme.line, scheme.panel_2, 0.25);
        let dark = theme::mix(scheme.panel, scheme.bg, 0.5);
        Self {
            body: (theme::mix(tone, scheme.text, 0.06), theme::mix(tone, scheme.panel_2, 0.35)),
            body_off: (theme::mix(tone, scheme.bg, 0.4), theme::mix(tone, scheme.bg, 0.55)),
            edge: dark,
            head: (theme::mix(tone, scheme.panel_2, 0.3), theme::mix(tone, scheme.panel_2, 0.55)),
            head_line: dark,
            label: scheme.text,
            label_dim: scheme.muted,
            tick: theme::mix(tone, scheme.bg, 0.6),
            slot: scheme.screen,
            slot_lit: scheme.muted,
            knob_off: (theme::mix(tone, scheme.text, 0.12), theme::mix(tone, scheme.bg, 0.2)),
        }
    }

    /// The cards of the scheme in use.
    fn current() -> Self {
        Self::from_scheme(theme::scheme())
    }
}

/// Paint the metal over `r`: a vertical gradient, brushed across.
pub(crate) fn draw_metal(canvas: &mut Canvas, r: Rect) {
    for row in 0..r.h.max(0) {
        let base = theme::mix(METAL.0, METAL.1, f64::from(row) / f64::from(r.h.max(1)));
        // A fixed scatter of light and dark rows: the brushing.
        let grain = (row.wrapping_mul(7919) ^ row.wrapping_mul(104_729) >> 3).rem_euclid(9) - 4;
        let color = if grain >= 0 {
            theme::mix(base, 0x00FF_FFFF, f64::from(grain) * 0.015)
        } else {
            theme::mix(base, 0, f64::from(-grain) * 0.015)
        };
        canvas.fill_rect(r.x, r.y + row, r.w, 1, color);
    }
}

/// Paint a card over `r`: its frame and body (darker when `bypassed`), and,
/// `head_h` high, its title band.
pub(crate) fn draw_card(canvas: &mut Canvas, r: Rect, head_h: Option<i32>, bypassed: bool, s: Scale) {
    let (t, cards) = (s.px(), Cards::current());
    let radius = 4 * s;
    canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (cards.edge, cards.edge), radius);
    let body = if bypassed { cards.body_off } else { cards.body };
    canvas.fill_rounded_gradient(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, body, radius - t);
    if let Some(h) = head_h {
        canvas.fill_rounded_gradient(r.x + t, r.y + t, r.w - 2 * t, h, cards.head, radius - t);
        // The band's foot is square: only its top corners are rounded.
        canvas.fill_rect(r.x + t, r.y + t + h / 2, r.w - 2 * t, h - h / 2, cards.head.1);
        canvas.fill_rect(r.x + t, r.y + t + h, r.w - 2 * t, t, cards.head_line);
    }
}

/// Shade `r`, a strip of a card that is switched off.
pub(crate) fn shade_strip(canvas: &mut Canvas, r: Rect) {
    canvas.shade_rect(r.x, r.y, r.w, r.h, Cards::current().body_off.1, 170);
}

/// Paint a screen's glass over `r`, framed.
pub(crate) fn draw_screen(canvas: &mut Canvas, r: Rect, s: Scale) {
    let t = s.px();
    canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (LCD_EDGE, LCD_EDGE), 2 * s);
    canvas.fill_rounded_gradient(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, (LCD, LCD), s.px());
}

/// Draw `item` in its hardware skin; false for a kind the skin leaves to
/// the flat look.
pub(super) fn draw_item(canvas: &mut Canvas, item: &Item, s: Scale) -> bool {
    let r = item.rect;
    match &item.kind {
        Kind::Text { text, .. } => {
            let color = if item.skin == Skin::Metal { METAL_TEXT } else { Cards::current().label };
            canvas.text(r.x, r.y, &fit(text, r.w.max(text_width(text, s)), s), s, color);
        }
        Kind::Value { spec, value, label, lfo, .. } if r.h >= 40 * s => {
            let fraction = spec.fraction(*value);
            match spec.style {
                Style::Knob => draw_knob(canvas, r, (fraction, *lfo), label.as_deref(), s),
                Style::Fader => draw_fader(canvas, r, (fraction, *lfo), label.as_deref(), s),
                Style::Slider | Style::Stepper => return false,
            }
        }
        Kind::Disabled { label, style } if r.h >= 40 * s => match style {
            Style::Fader => draw_fader_off(canvas, r, label, s),
            _ => draw_knob_off(canvas, r, label, s),
        },
        Kind::Spin { labels, selected, curve, .. } => draw_spin(canvas, r, (labels, *selected), curve, s),
        Kind::Toggle { on } => draw_button(canvas, r, if *on { "ON" } else { "OFF" }, *on, s),
        Kind::Button { label, on } => draw_button(canvas, r, label, *on, s),
        Kind::Led { on } => draw_led(canvas, r, *on, s),
        Kind::Select { labels, selected, .. } => {
            draw_screen(canvas, r, s);
            let label = labels.get(*selected).map_or("", String::as_str);
            let ty = r.y + (r.h - 8 * s) / 2;
            canvas.text(r.x + 4 * s, ty, &fit(label, r.w - 16 * s, s), s, LCD_TEXT);
            canvas.text(r.x + r.w - 10 * s, ty, MENU_MARK, s, LCD_DIM);
        }
        _ => return false,
    }
    true
}

/// The point at `fraction` of a knob's travel, `radius` from `(cx, cy)`.
fn knob_point((cx, cy): (f64, f64), radius: f64, fraction: f64) -> (i32, i32) {
    let a = (KNOB_START + KNOB_SWEEP * fraction).to_radians();
    // Pixels: rounding to the nearest is meant.
    ((cx + a.cos() * radius).round() as i32, (cy + a.sin() * radius).round() as i32)
}

/// A knob at the top of `r`, its label under it: ticks around, a black body,
/// its pointer at `fraction` (in the LFO's colour, with the ticks it has
/// passed, when `lfo` moves it).
fn draw_knob(canvas: &mut Canvas, r: Rect, (fraction, lfo): (f64, bool), label: Option<&str>, s: Scale) {
    let (size, cards) = (knob_size(r, s), Cards::current());
    let centre = (f64::from(r.x) + f64::from(r.w) / 2.0, f64::from(r.y) + f64::from(size) / 2.0);
    let outer = f64::from(size) / 2.0;
    let tick_len = f64::from(3 * s);
    for i in 0..TICKS {
        let f = f64::from(i) / f64::from(TICKS - 1);
        let color = if lfo && f <= fraction { theme::CHANCE } else { cards.tick };
        let (a, b) = (knob_point(centre, outer - tick_len, f), knob_point(centre, outer, f));
        canvas.line(a, b, s.px(), color);
    }
    let body = outer - tick_len - f64::from(s.px());
    canvas.fill_disc(centre, body, (KNOB_RIM, KNOB_RIM));
    canvas.fill_disc(centre, body - f64::from(s.px()), KNOB);
    let pointer = if lfo { theme::CHANCE } else { POINTER };
    let (a, b) = (knob_point(centre, body * 0.3, fraction), knob_point(centre, body * 0.85, fraction));
    canvas.line(a, b, (s * 3 / 2).max(1), pointer);
    if let Some(label) = label {
        label_under(canvas, r, r.y + size + 3 * s, label, cards.label, s);
    }
}

/// A knob greyed out: a flat body without a pointer, its label dimmed.
fn draw_knob_off(canvas: &mut Canvas, r: Rect, label: &str, s: Scale) {
    let (size, cards) = (knob_size(r, s), Cards::current());
    let centre = (f64::from(r.x) + f64::from(r.w) / 2.0, f64::from(r.y) + f64::from(size) / 2.0);
    canvas.fill_disc(centre, f64::from(size) / 2.0 - f64::from(4 * s), cards.knob_off);
    label_under(canvas, r, r.y + size + 3 * s, label, cards.label_dim, s);
}

/// A fader greyed out: its slot, dim and without a cap, its label dim under it.
fn draw_fader_off(canvas: &mut Canvas, r: Rect, label: &str, s: Scale) {
    let ((top, height), cards) = (fader_bar(r, s), Cards::current());
    let slot_w = 4 * s;
    let x = r.x + r.w / 2 - slot_w / 2;
    canvas.fill_rounded_gradient(x, top - 2 * s, slot_w, height + 4 * s, cards.knob_off, slot_w / 2);
    label_under(canvas, r, top + height + 8 * s, label, cards.label_dim, s);
}

/// `label` centred across `r` at `y`, cut to its width.
fn label_under(canvas: &mut Canvas, r: Rect, y: i32, label: &str, color: Rgb, s: Scale) {
    let label = fit(label, r.w, s);
    canvas.text(r.x + (r.w - text_width(&label, s)) / 2, y, &label, s, color);
}

/// A fader: a dark slot, lit under its cap, the cap at `fraction`, its label under it.
fn draw_fader(canvas: &mut Canvas, r: Rect, (fraction, lfo): (f64, bool), label: Option<&str>, s: Scale) {
    let ((top, height), cards) = (fader_bar(r, s), Cards::current());
    let cx = r.x + r.w / 2;
    let slot_w = 4 * s;
    canvas.fill_rounded_gradient(
        cx - slot_w / 2,
        top - 2 * s,
        slot_w,
        height + 4 * s,
        (cards.slot, cards.slot),
        slot_w / 2,
    );
    // The value's place in the slot: truncating to a pixel is meant.
    let filled = (fraction * f64::from(height)) as i32;
    let lit = if lfo { theme::CHANCE } else { cards.slot_lit };
    canvas.fill_rect(cx - s.px(), top + height - filled, 2 * s.px(), filled, lit);
    let (cap_w, cap_h) = ((r.w - 4 * s).min(16 * s), 9 * s);
    let cap_y = top + height - filled - cap_h / 2;
    canvas.fill_rounded_gradient(cx - cap_w / 2, cap_y, cap_w, cap_h, (BUTTON_EDGE, BUTTON_EDGE), 2 * s);
    let t = s.px();
    canvas.fill_rounded_gradient(cx - cap_w / 2 + t, cap_y + t, cap_w - 2 * t, cap_h - 2 * t, CAP, s.px());
    canvas.fill_rect(cx - cap_w / 2 + 2 * t, cap_y + cap_h / 2, cap_w - 4 * t, t, CAP_LINE);
    if let Some(label) = label {
        label_under(canvas, r, top + height + 8 * s, label, cards.label, s);
    }
}

/// A spin: its screen (option `selected` of `labels`, or the wave of
/// `curve`), its ▲ over its ▼, each dimmed at its end of the list.
fn draw_spin(canvas: &mut Canvas, r: Rect, (labels, selected): (&[String], usize), curve: &[f64], s: Scale) {
    let t = s.px();
    let [down, screen, up] = spin_parts(r, s);
    draw_screen(canvas, screen, s);
    let inside = Rect::new(screen.x + 2 * t, screen.y + 2 * t, screen.w - 4 * t, screen.h - 4 * t);
    if curve.is_empty() {
        let label = labels.get(selected).map_or("", String::as_str);
        let label = fit(label, inside.w, s);
        let x = inside.x + (inside.w - text_width(&label, s)) / 2;
        canvas.text(x, inside.y + (inside.h - 8 * s) / 2, &label, s, LCD_TEXT);
    } else {
        let mid = inside.y + inside.h / 2;
        let reach = f64::from(inside.h / 2 - t);
        let last = curve.len().saturating_sub(1).max(1) as i32;
        // Pixels: truncating is meant.
        let points = curve
            .iter()
            .enumerate()
            .map(|(i, v)| (inside.x + i as i32 * (inside.w - 1) / last, mid - (v * reach) as i32));
        canvas.polyline(points, t, LCD_TEXT);
    }
    let live = [(up, true, selected + 1 < labels.len()), (down, false, selected > 0)];
    for (button, points_up, live) in live {
        let b = Rect::new(button.x + t, button.y + t, button.w - t, button.h - 2 * t);
        canvas.fill_rounded_gradient(b.x, b.y, b.w, b.h, BUTTON, t);
        draw_arrow(canvas, b, points_up, if live { BUTTON_TEXT } else { Cards::current().label_dim }, s);
    }
}

/// A small triangle in the middle of `r`, pointing up or down.
fn draw_arrow(canvas: &mut Canvas, r: Rect, points_up: bool, color: Rgb, s: Scale) {
    let rows = 3 * s;
    let (cx, top) = (r.x + r.w / 2, r.y + (r.h - rows) / 2);
    for row in 0..rows {
        let half = if points_up { row } else { rows - 1 - row };
        canvas.fill_rect(cx - half, top + row, 2 * half + 1, 1, color);
    }
}

/// A button (or a switch) with `label`: dark, yellow when `on`.
fn draw_button(canvas: &mut Canvas, r: Rect, label: &str, on: bool, s: Scale) {
    let t = s.px();
    canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (BUTTON_EDGE, BUTTON_EDGE), 3 * s);
    let (fill, text) = if on { (BUTTON_ON, BUTTON_ON_TEXT) } else { (BUTTON, BUTTON_TEXT) };
    canvas.fill_rounded_gradient(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, fill, 3 * s - t);
    let label = fit(label, r.w - 2 * s, s);
    canvas.text(r.x + (r.w - text_width(&label, s)) / 2, r.y + (r.h - 8 * s) / 2, &label, s, text);
}

/// A round LED in the middle of `r`: green and glinting when `on`.
fn draw_led(canvas: &mut Canvas, r: Rect, on: bool, s: Scale) {
    let radius = f64::from(r.w.min(r.h).min(10 * s)) / 2.0;
    let centre = (f64::from(r.x) + f64::from(r.w) / 2.0, f64::from(r.y) + f64::from(r.h) / 2.0);
    canvas.fill_disc(centre, radius, (LED_RIM, LED_RIM));
    canvas.fill_disc(centre, radius - f64::from(s.px()), if on { LED_ON } else { LED_OFF });
    if on {
        let glint = (centre.0 - radius * 0.3, centre.1 - radius * 0.3);
        canvas.fill_disc(glint, radius * 0.28, (0x00FF_FFFF, 0x00E8_FFE0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cards_take_the_schemes_hue_and_their_labels_read_on_them() {
        let all: Vec<Cards> = theme::SCHEMES.iter().map(Cards::from_scheme).collect();
        for (scheme, cards) in theme::SCHEMES.iter().zip(&all) {
            for ground in [cards.body.0, cards.body.1, cards.head.0, cards.head.1] {
                assert!(theme::contrast(cards.label, ground) >= 4.5, "{}: label on {ground:06X}", scheme.name);
            }
            assert!(theme::contrast(cards.label_dim, cards.body.1) >= 2.0, "{}: greyed out", scheme.name);
        }
        assert!(all.windows(2).all(|pair| pair[0].body != pair[1].body), "one look per scheme");
    }
}
