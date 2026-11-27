//! Painting the synth editor under its items, in its hardware look
//! (`panel/hardware.rs`): the metal over the whole editor, the orDrumbox
//! logo right of the preset bar, the cards with a title band (a family's
//! card has none: its strips carry their titles), the emblem on the metal
//! in the last card's place, and the plots on their screens — the scope,
//! the filter's response, the envelope.

use ordrumbox_core::json::Object;
use ordrumbox_engine::synth_model as model;

use crate::canvas::{Canvas, Rgb};
use crate::image::{EMBLEM, Image, LOGO};
use crate::panel::Rect;
use crate::panel::hardware::{self, LCD_DIM, LCD_GRID, LCD_TEXT};
use crate::scale::Scale;
use crate::scroll_thumb;
use crate::synth_family::Family;
use crate::synth_matrix::MatrixList;
use crate::synth_view::{Built, Card, EMBLEM_GROUP, HEAD_H, PAD_X, PAD_Y, Plot, SynthLayout};

/// The space between the preset bar and the logo, at least (at scale 1).
const LOGO_GAP: i32 = 16;

/// Paint the metal, the logo, the cards under their items (but not under the
/// emblem), and the plots.
pub fn draw(canvas: &mut Canvas, draft: &Object, built: &Built, layout: &SynthLayout, time: f64, bpm: f64) {
    let s = layout.scale();
    hardware::draw_metal(canvas, layout.panel());
    if let Some((x, y, logo_scale)) = logo_place(layout, &LOGO) {
        canvas.draw_image(x, y, &LOGO, logo_scale);
    }
    for card in &built.cards {
        let r = card.rect;
        // The emblem keeps a card's place in the rows, but the metal shows through it.
        if card.framed && card.group != EMBLEM_GROUP {
            hardware::draw_card(canvas, r, has_head(card).then_some((PAD_Y + HEAD_H - 4) * s), card.bypassed, s);
        } else if card.bypassed {
            hardware::shade_strip(canvas, r);
        }
        if card.group == EMBLEM_GROUP
            && let Some((x, y, emblem_scale)) = emblem_place(r, s)
        {
            canvas.draw_image(x, y, &EMBLEM, emblem_scale);
        }
        if let Some((plot, rect)) = card.plot {
            hardware::draw_screen(canvas, rect, s);
            let inside = Rect::new(rect.x + 2 * s, rect.y + 2 * s, rect.w - 4 * s, rect.h - 4 * s);
            match plot {
                Plot::Scope => draw_scope(canvas, &model::scope(draft, time, bpm), inside, s),
                Plot::Envelope => draw_envelope(canvas, draft, "envelope", inside, s),
                Plot::Filter => draw_filter(canvas, draft, inside, s, (time, bpm)),
            }
        }
    }
    if let Some(list) = built.matrix.filter(MatrixList::scrolls) {
        scroll_thumb::draw(canvas, list.bar, list.thumb(), s);
    }
}

/// Whether `card` has a title band: a card of its own, not a family's nor the emblem's.
fn has_head(card: &Card) -> bool {
    card.group != EMBLEM_GROUP && !Family::ALL.iter().any(|family| family.name() == card.group)
}

/// Where the emblem goes in its card `r`, and at what scale: the largest
/// quarter steps (at most its own size times `s`) that fit inside the card's
/// padding, centred; none when even a quarter does not fit.
fn emblem_place(r: Rect, s: Scale) -> Option<(i32, i32, Scale)> {
    let (w, h) = (r.w - 2 * PAD_X * s, r.h - 2 * PAD_Y * s);
    let fits = |scale: &Scale| EMBLEM.width_at(*scale) <= w && EMBLEM.height_at(*scale) <= h;
    let scale = (1..=s.quarters()).rev().map(Scale::from_quarters).find(fits)?;
    Some((r.x + (r.w - EMBLEM.width_at(scale)) / 2, r.y + (r.h - EMBLEM.height_at(scale)) / 2, scale))
}

/// Where `logo` goes, and at what scale: half its size, right-aligned in the
/// editor's head, when it fits right of the preset bar.
fn logo_place(layout: &SynthLayout, logo: &Image) -> Option<(i32, i32, Scale)> {
    let s = layout.scale();
    let logo_scale = Scale::from_quarters((s.quarters() / 2).max(1));
    let head = layout.head();
    let x = head.x + head.w - logo.width_at(logo_scale);
    let y = head.y + (head.h - logo.height_at(logo_scale)) / 2;
    (x >= layout.bar.right() + LOGO_GAP * s && logo.height_at(logo_scale) <= head.h).then_some((x, y, logo_scale))
}

/// The scope: the samples across, -1 to 1 around the middle.
fn draw_scope(canvas: &mut Canvas, samples: &[f64], r: Rect, s: Scale) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.px(), LCD_GRID);
    let n = samples.len().max(1);
    let points = samples
        .iter()
        .enumerate()
        .map(|(i, v)| (r.x + (i as i32 * r.w) / n as i32, mid - (v * f64::from(r.h / 2 - 2 * s)) as i32));
    canvas.polyline(points, (s * 3 / 2).max(1), LCD_TEXT);
}

/// The envelope: attack, decay, sustain, release.
fn draw_envelope(canvas: &mut Canvas, draft: &Object, group: &str, r: Rect, s: Scale) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.px(), LCD_GRID);
    let (points, total) = model::envelope(draft, group);
    let point = |(t, v): (f64, f64)| {
        (r.x + ((t / total) * f64::from(r.w - 1)) as i32, mid - (v * f64::from(r.h / 2 - 2 * s)) as i32)
    };
    canvas.polyline(points.into_iter().map(point), (s * 3 / 2).max(1), LCD_TEXT);
}

/// The filter's response, 20 Hz to 20 kHz across, -30 to +12 dB, and the
/// band an LFO on the frequency sweeps, at `time` and `bpm`.
fn draw_filter(canvas: &mut Canvas, draft: &Object, r: Rect, s: Scale, (time, bpm): (f64, f64)) {
    let to_x = |f: f64| r.x + (((f.log10() - 20f64.log10()) / 3.0) * f64::from(r.w)) as i32;
    let to_y = |db: f64| r.y + r.h - 1 - (((db + 30.0) / 42.0) * f64::from(r.h - 1)) as i32;
    for f in [100.0, 1000.0, 10_000.0] {
        canvas.fill_rect(to_x(f), r.y, s.px(), r.h, LCD_GRID);
    }
    for db in [0.0, -20.0] {
        canvas.fill_rect(r.x, to_y(db), r.w, s.px(), LCD_GRID);
    }
    const N: usize = 200;
    let (kind, fc, q, slope_db, band) = model::filter_now(draft, time, bpm);
    let curve = |canvas: &mut Canvas, fc: f64, color: Rgb, t: i32| {
        let response = model::filter_response(&kind, fc, q, slope_db, N);
        canvas.polyline(
            response.iter().enumerate().map(|(i, db)| (r.x + (i as i32 * (r.w - 1)) / N as i32, to_y(*db))),
            t,
            color,
        );
    };
    if let Some((low, high)) = band {
        curve(canvas, low, LCD_DIM, s.px());
        curve(canvas, high, LCD_DIM, s.px());
    }
    curve(canvas, fc, LCD_TEXT, (s * 3 / 2).max(1));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth_view::build;

    #[test]
    fn draws_at_any_size() {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        for (w, h) in [(1, 1), (40, 30), (740, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SynthLayout::new(w as i32, h as i32, Scale::ONE, 0);
            let built = build(&d, (&[], ""), &layout, 0, &|_| None);
            draw(&mut canvas, &d, &built, &layout, 0.3, 120.0);
        }
    }

    #[test]
    fn the_logo_sits_right_of_the_bar_when_it_fits() {
        for scale in [Scale::ONE, Scale::from_quarters(6), Scale::whole(2)] {
            let wide = SynthLayout::new(1400 * scale, 800 * scale, scale, 0);
            let (x, y, _) = logo_place(&wide, &LOGO).expect("room for the logo");
            assert!(x >= wide.bar.right(), "right of the bar");
            assert!(
                y >= wide.head().y && x + LOGO.width_at(Scale::from_quarters(scale.quarters() / 2)) <= 1400 * scale
            );
        }
        let narrow = SynthLayout::new(600, 680, Scale::ONE, 0);
        assert!(logo_place(&narrow, &LOGO).is_none(), "no room: no logo");
    }

    #[test]
    fn the_emblem_sits_on_the_metal_without_a_card() {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        let (w, h) = (912usize, 680usize);
        let layout = SynthLayout::new(w as i32, h as i32, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        let card = built.cards.iter().find(|c| c.group == EMBLEM_GROUP).expect("the emblem's place");
        assert!(card.framed, "still as tall as its row");

        let mut pixels = vec![0; w * h];
        draw(&mut Canvas::new(&mut pixels, w, h), &d, &built, &layout, 0.3, 120.0);
        let mut metal = vec![0; w * h];
        hardware::draw_metal(&mut Canvas::new(&mut metal, w, h), layout.panel());
        let at = |(x, y): (i32, i32)| y as usize * w + x as usize;

        let corner = (card.rect.x + 1, card.rect.y + 1);
        assert_eq!(pixels[at(corner)], metal[at(corner)], "no card under the emblem");
        let (x, y, scale) = emblem_place(card.rect, Scale::ONE).expect("room for the emblem");
        let center = (x + EMBLEM.width_at(scale) / 2, y + EMBLEM.height_at(scale) / 2);
        assert_ne!(pixels[at(center)], metal[at(center)], "the emblem is drawn");
    }
}
