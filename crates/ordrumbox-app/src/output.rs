//! The output panel (`output_panel.js` and its sections), in the place of
//! the grid: the master bus settings in three tabs — **Vol** (the master
//! volume, the spectrum), **Comp** (pre-gain, the compressor's transfer curve,
//! its on/off button and six knobs), **Flt** (low cut, high cut) — kept in
//! `settings.json` (`settings.master`, `MASTER_BUS_DEFAULTS` for what it lacks).
//!
//! [`OutputLayout`] places everything; the controls are panel items, the
//! curve and the spectrum are drawn here.

use ordrumbox_engine::MasterSettings;

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::{Format, Spec};
use crate::panel::Rect;
use crate::theme;

/// A master bus setting: its `settings.master` key, label, range, step and
/// how its value shows (`COMPRESSOR_PARAMS`, the master and filter controls).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Param {
    pub key: &'static str,
    pub label: &'static str,
    pub spec: Spec,
    pub format: Format,
    pub unit: &'static str,
}

const fn param(key: &'static str, label: &'static str, spec: Spec, format: Format, unit: &'static str) -> Param {
    Param { key, label, spec, format, unit }
}

/// The volume knob (Vol tab).
pub const VOLUME: Param = param("volume", "Volume", Spec::knob(0.0, 2.0, 0.01, 1.0), Format::Fixed2, "");
/// The pre-gain knob (Comp tab).
pub const PRE_GAIN: Param = param("preGain", "Pre-Gain", Spec::knob(-20.0, 20.0, 0.5, 0.0), Format::SignedFixed1, "dB");
/// The compressor knobs (`COMPRESSOR_PARAMS`): under 1 a step shows 3 decimals, else whole.
pub const COMPRESSOR: [Param; 6] = [
    param("threshold", "Threshold", Spec::knob(-40.0, 0.0, 1.0, -18.0), Format::Round, "dB"),
    param("ratio", "Ratio", Spec::knob(1.0, 20.0, 0.5, 8.0), Format::Fmt3, ""),
    param("attack", "Attack", Spec::knob(0.0, 1.0, 0.001, 0.002), Format::Fmt3, "s"),
    param("release", "Release", Spec::knob(0.0, 1.0, 0.001, 0.08), Format::Fmt3, "s"),
    param("knee", "Knee", Spec::knob(0.0, 40.0, 1.0, 3.0), Format::Round, "dB"),
    param("makeup", "Makeup", Spec::knob(0.0, 24.0, 0.5, 8.0), Format::Fmt3, "dB"),
];
/// The filter sliders (Flt tab).
pub const FILTERS: [Param; 2] = [
    param("lowcut", "Low Cut", Spec::slider(10.0, 500.0, 1.0, 35.0).log(), Format::Round, "Hz"),
    param("hicut", "High Cut", Spec::slider(1000.0, 20_000.0, 100.0, 18_500.0).log(), Format::Round, "Hz"),
];
/// The tabs.
pub const TABS: [&str; 3] = ["Vol", "Comp", "Flt"];

/// Every knob and slider.
pub fn params() -> impl Iterator<Item = &'static Param> {
    [&VOLUME, &PRE_GAIN].into_iter().chain(COMPRESSOR.iter()).chain(FILTERS.iter())
}

pub fn find(key: &str) -> Option<&'static Param> {
    params().find(|p| p.key == key)
}

/// The compressor's gain reduction at `input` dB.
fn gain_reduction(input: f64, threshold: f64, ratio: f64, knee: f64) -> f64 {
    let over = input - threshold;
    if over <= -knee / 2.0 {
        return 0.0;
    }
    if knee > 0.0 && over < knee / 2.0 {
        let d = over + knee / 2.0;
        return (1.0 - 1.0 / ratio) * d * d / (2.0 * knee);
    }
    over * (1.0 - 1.0 / ratio)
}

/// The output level for `input` dB (the curve): the input when bypassed.
pub fn transfer(master: &MasterSettings, input: f64) -> f64 {
    if master.bypass >= 0.5 {
        return input;
    }
    let (threshold, ratio, knee) =
        (f64::from(master.threshold), f64::from(master.ratio).max(1.0), f64::from(master.knee).max(0.0));
    input - gain_reduction(input, threshold, ratio, knee) + f64::from(master.makeup)
}

/// The curve's range, dB.
const CURVE_X: (f64, f64) = (-60.0, 0.0);
const CURVE_Y: (f64, f64) = (-60.0, 12.0);

/// Where the output panel's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputLayout {
    scale: i32,
    /// The band the panel is drawn in (the bottom strip; empty in the full layout).
    pub band: Rect,
    pub title: (i32, i32),
    pub tabs: [Rect; 3],
    /// Vol: the volume knob, the spectrum.
    pub volume: Rect,
    pub spectrum: Rect,
    /// Comp: the pre-gain knob, the curve, the on/off button, the six knobs.
    pub pre_gain: Rect,
    pub curve: Rect,
    pub compressor_title: (i32, i32),
    pub bypass: Rect,
    pub compressor: [Rect; 6],
    /// Flt: the two sliders.
    pub filters: [Rect; 2],
}

impl OutputLayout {
    /// The compact bottom strip: the title and the three tabs stacked on the
    /// left, the active tab's controls in one horizontal row across `band`.
    pub fn strip(width: i32, band: Rect, scale: i32) -> Self {
        let s = scale;
        let pad = 8 * s;
        let top = band.y + pad;
        let inner_h = (band.h - 2 * pad).max(1);
        let title = (band.x + pad, top);
        // The tabs, stacked under the title on the left.
        let tab_w = 56 * s;
        let tabs = [0, 1, 2].map(|i| Rect::new(band.x + pad, top + 13 * s + i * 15 * s, tab_w, 12 * s));
        // The controls fill what is left of the width, one row, vertically centred.
        let cx = band.x + pad + tab_w + 16 * s;
        let right = width - pad;
        let knob_w = 76 * s;
        let knob_h = inner_h.min(64 * s);
        let knob_y = band.y + (band.h - knob_h) / 2;
        let knob = |x: i32, w: i32| Rect::new(x, knob_y, w, knob_h);
        let after_knobs = |n: i32| cx + n * (knob_w + 4 * s);
        // Vol: the volume knob, then the spectrum filling the rest.
        let volume = knob(cx, knob_w);
        let spectrum = Rect::new(cx + knob_w + 12 * s, top, (right - (cx + knob_w + 12 * s)).max(0), inner_h);
        // Comp: pre-gain + six knobs + the on/off button, the curve on the right.
        let pre_gain = knob(cx, knob_w);
        let compressor = [0, 1, 2, 3, 4, 5].map(|i| knob(after_knobs(i + 1), knob_w));
        let bypass = Rect::new(after_knobs(7), band.y + (band.h - 12 * s) / 2, 40 * s, 12 * s);
        let curve_x = after_knobs(7) + 52 * s;
        let curve = Rect::new(curve_x, top, (right - curve_x).max(0), inner_h);
        // Flt: the two sliders, one under the other, across the width.
        let filters = [
            Rect::new(cx, band.y + band.h / 2 - 18 * s, (right - cx).max(0), 16 * s),
            Rect::new(cx, band.y + band.h / 2 + 2 * s, (right - cx).max(0), 16 * s),
        ];
        Self {
            scale,
            band,
            title,
            tabs,
            volume,
            spectrum,
            pre_gain,
            curve,
            compressor_title: (curve_x, band.y + 2 * s),
            bypass,
            compressor,
            filters,
        }
    }
}

const SURFACE: Rgb = theme::PANEL_2;
const GRID: Rgb = theme::LINE;
const MUTED: Rgb = theme::MUTED;
const ACCENT: Rgb = theme::PINK;
const WARNING: Rgb = theme::YELLOW;
const SCOPE_BG: Rgb = theme::SCREEN;
const SCOPE_GRID: Rgb = theme::PANEL_2;
const PHOSPHOR: Rgb = theme::CYAN;

/// What the output panel draws besides its controls.
#[derive(Debug)]
pub struct OutputView {
    pub tab: u8,
    pub master: MasterSettings,
    /// The analyser's bytes, one a bin.
    pub spectrum: Vec<u8>,
}

pub fn draw(canvas: &mut Canvas, view: &OutputView, layout: &OutputLayout) {
    let s = layout.scale;
    if layout.band.h > 0 {
        // The strip: a panel along the bottom, edged on top.
        canvas.fill_rect(layout.band.x, layout.band.y, layout.band.w, layout.band.h, theme::PANEL);
        canvas.fill_rect(layout.band.x, layout.band.y, layout.band.w, s.max(1), theme::LINE);
    }
    canvas.text(layout.title.0, layout.title.1, "Master", s, ACCENT);
    match view.tab {
        0 if layout.spectrum.w > 8 * s && layout.spectrum.h > 8 * s => {
            draw_spectrum(canvas, &view.spectrum, layout.spectrum, s);
        }
        1 if layout.curve.w > 8 * s && layout.curve.h > 8 * s => {
            draw_curve(canvas, &view.master, layout.curve, s);
            canvas.text(layout.compressor_title.0, layout.compressor_title.1, "COMPRESSOR", s, MUTED);
        }
        _ => {}
    }
}

/// A line from `(x0, y0)` to `(x1, y1)`, `t` thick; `dash`: on and off lengths.
fn line(canvas: &mut Canvas, (x0, y0): (i32, i32), (x1, y1): (i32, i32), t: i32, color: Rgb, dash: Option<(i32, i32)>) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        if let Some((on, off)) = dash
            && i % (on + off) >= on
        {
            continue;
        }
        let x = x0 + (x1 - x0) * i / steps;
        let y = y0 + (y1 - y0) * i / steps;
        canvas.fill_rect(x - t / 2, y - t / 2, t, t, color);
    }
}

/// The compressor's transfer curve: -60 to 0 dB in, -60 to
/// +12 dB out, the unity line, the threshold, the curve.
fn draw_curve(canvas: &mut Canvas, master: &MasterSettings, r: Rect, s: i32) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, SURFACE);
    let x_px = |db: f64| r.x + ((db - CURVE_X.0) / (CURVE_X.1 - CURVE_X.0) * f64::from(r.w)) as i32;
    let y_px = |db: f64| r.y + r.h - ((db - CURVE_Y.0) / (CURVE_Y.1 - CURVE_Y.0) * f64::from(r.h)) as i32;
    for db in [-60.0, -40.0, -20.0, 0.0] {
        let (x, y) = (x_px(db).min(r.x + r.w - 1), y_px(db));
        canvas.fill_rect(x, r.y, s.max(1), r.h, GRID);
        canvas.fill_rect(r.x, y, r.w, s.max(1), GRID);
    }
    let (from, to) = (CURVE_X.0.max(CURVE_Y.0), CURVE_X.1.min(CURVE_Y.1));
    line(canvas, (x_px(from), y_px(from)), (x_px(to), y_px(to)), s.max(1), MUTED, Some((6 * s, 6 * s)));
    let bypass = master.bypass >= 0.5;
    let threshold = f64::from(master.threshold);
    if !bypass && (CURVE_X.0..=CURVE_X.1).contains(&threshold) {
        let x = x_px(threshold).min(r.x + r.w - 1);
        line(canvas, (x, r.y), (x, r.y + r.h - 1), s.max(1), WARNING, Some((4 * s, 4 * s)));
    }
    let color = if bypass { MUTED } else { ACCENT };
    let mut previous = None;
    for px in 0..=r.w {
        let db = CURVE_X.0 + f64::from(px) / f64::from(r.w) * (CURVE_X.1 - CURVE_X.0);
        let y = y_px(transfer(master, db)).clamp(r.y, r.y + r.h - 1);
        let point = (r.x + px.min(r.w - 1), y);
        if let Some(p) = previous {
            line(canvas, p, point, 2 * s, color, None);
        }
        previous = Some(point);
    }
    canvas.text(r.x + 4 * s, r.y + 4 * s, "12 dB", s, MUTED);
    canvas.text(r.x + 4 * s, r.y + r.h - 12 * s, "-60 dB", s, MUTED);
    canvas.text(r.x + r.w - 4 * s - text_width("0 dB", s), r.y + r.h - 12 * s, "0 dB", s, MUTED);
    if bypass {
        let label = "BYPASSED";
        canvas.text(r.x + (r.w - text_width(label, s)) / 2, r.y + r.h / 2 - 4 * s, label, s, MUTED);
    }
}

/// The spectrum: a grid, then the first bins (one a
/// column of the JS canvas, 256) as a line, louder higher.
fn draw_spectrum(canvas: &mut Canvas, bins: &[u8], r: Rect, s: i32) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, SCOPE_BG);
    for i in 1..8 {
        canvas.fill_rect(r.x + r.w * i / 8, r.y, s.max(1), r.h, SCOPE_GRID);
    }
    for i in 1..4 {
        canvas.fill_rect(r.x, r.y + r.h * i / 4, r.w, s.max(1), SCOPE_GRID);
    }
    let count = bins.len().min(256);
    let mut previous = None;
    for (i, &v) in bins.iter().take(count).enumerate() {
        let x = r.x + (i as i32 * r.w) / count as i32;
        let y = r.y + r.h - 1 - (i32::from(v) * (r.h - 1)) / 255;
        // Low to high: pink, violet, cyan, lime, yellow.
        let band = [theme::PINK, theme::VIOLET, PHOSPHOR, theme::LIME, theme::YELLOW][i * 5 / count.max(1)];
        if let Some(p) = previous {
            line(canvas, p, (x, y), 2 * s, band, None);
        }
        previous = Some((x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_control_is_a_saved_setting() {
        let keys: Vec<_> = params().map(|p| p.key).collect();
        assert_eq!(keys, crate::master::KEYS);
    }

    #[test]
    fn the_curve_follows_the_compressor() {
        let master = MasterSettings::default(); // threshold -18, ratio 8, knee 3, makeup 8
        assert_eq!(transfer(&master, -40.0), -32.0);
        // Well over the knee: 1/8 of what is over the threshold, plus makeup.
        assert!((transfer(&master, 0.0) - (-18.0 + 18.0 / 8.0 + 8.0)).abs() < 1e-9);
        assert_eq!(transfer(&MasterSettings { bypass: 1.0, ..master }, -10.0), -10.0);
    }

    #[test]
    fn draws_every_tab_at_any_size() {
        let bins: Vec<u8> = (0..2048).map(|i| (i % 256) as u8).collect();
        for (w, h) in [(1, 1), (40, 30), (1100, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let (wi, hi) = (w as i32, h as i32);
            let band = Rect::new(0, (hi - 88).max(0), wi, hi.min(88));
            let layout = OutputLayout::strip(wi, band, 1);
            for tab in 0..3 {
                let master = MasterSettings { bypass: f32::from(tab == 2), ..MasterSettings::default() };
                draw(&mut canvas, &OutputView { tab, master, spectrum: bins.clone() }, &layout);
            }
        }
    }
}
