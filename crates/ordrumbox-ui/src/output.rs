//! The output panel, a strip along the bottom of the window: the master bus
//! settings in one row — the master volume and the spectrum; pre-gain, the
//! compressor's six knobs, its on/off button and transfer curve; low cut and
//! high cut — kept in `settings.json` (`settings.master`,
//! `MASTER_BUS_DEFAULTS` for what it lacks).
//!
//! [`OutputLayout`] places everything; the controls are panel items, the
//! curve and the spectrum are drawn here.

use crate::scale::Scale;
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

/// The volume knob.
pub const VOLUME: Param = param("volume", "Volume", Spec::knob(0.0, 2.0, 0.01, 1.0), Format::Fixed2, "");
/// The pre-gain knob, ahead of the compressor.
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
/// The filter sliders.
pub const FILTERS: [Param; 2] = [
    param("lowcut", "Low Cut", Spec::slider(10.0, 500.0, 1.0, 35.0).log(), Format::Round, "Hz"),
    param("hicut", "High Cut", Spec::slider(1000.0, 20_000.0, 100.0, 18_500.0).log(), Format::Round, "Hz"),
];

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

/// Height of the output strip, at scale 1: a knob between two pads.
pub const STRIP_HEIGHT: i32 = KNOB_HEIGHT + 2 * PAD;
/// The strip's inner margin, at scale 1.
const PAD: i32 = 8;
/// A knob's slot: the knob, its label and its value, at scale 1.
const KNOB_WIDTH: i32 = 60;
const KNOB_HEIGHT: i32 = 48;
/// The "Master" title's column, at scale 1.
const TITLE_WIDTH: i32 = 44;
/// The spectrum's width, at scale 1.
const SPECTRUM_WIDTH: i32 = 120;
/// The compressor's transfer curve's width, at scale 1.
const CURVE_WIDTH: i32 = 96;
/// The compressor's on/off button, at scale 1.
const BYPASS_SIZE: (i32, i32) = (36, 14);
/// The space between two groups, a divider in its middle, at scale 1.
const GROUP_GAP: i32 = 12;

/// Where the output panel's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutputLayout {
    scale: Scale,
    /// The band the panel is drawn in (the bottom strip).
    pub band: Rect,
    pub title: (i32, i32),
    /// The volume knob, the spectrum.
    pub volume: Rect,
    pub spectrum: Rect,
    /// The pre-gain knob, the six knobs, the on/off button, the curve.
    pub pre_gain: Rect,
    pub compressor: [Rect; 6],
    pub bypass: Rect,
    pub curve: Rect,
    /// The two sliders, one under the other.
    pub filters: [Rect; 2],
    /// The dividers between the groups: their x.
    dividers: [i32; 2],
}

impl OutputLayout {
    /// The strip: the title, then every group in one row across `band`.
    pub fn strip(width: i32, band: Rect, scale: Scale) -> Self {
        let s = scale;
        let knob_h = KNOB_HEIGHT * s;
        let knob_y = band.y + (band.h - knob_h) / 2;
        let knob = |x: i32| Rect::new(x, knob_y, KNOB_WIDTH * s, knob_h);
        let title = (band.x + PAD * s, band.y + (band.h - 8 * s) / 2);
        // The volume, then the spectrum.
        let volume = knob(title.0 + TITLE_WIDTH * s);
        let spectrum = Rect::new(volume.x + volume.w + PAD * s, knob_y, SPECTRUM_WIDTH * s, knob_h);
        // The compressor: pre-gain, six knobs, the on/off button, the curve.
        let comp_x = spectrum.x + spectrum.w + GROUP_GAP * s;
        let pre_gain = knob(comp_x);
        let compressor = [1, 2, 3, 4, 5, 6].map(|i| knob(comp_x + i * KNOB_WIDTH * s));
        let (bypass_w, bypass_h) = (BYPASS_SIZE.0 * s, BYPASS_SIZE.1 * s);
        let bypass =
            Rect::new(comp_x + 7 * KNOB_WIDTH * s + 4 * s, band.y + (band.h - bypass_h) / 2, bypass_w, bypass_h);
        let curve = Rect::new(bypass.x + bypass.w + PAD * s, knob_y, CURVE_WIDTH * s, knob_h);
        // The filters fill what is left of the width.
        let filter_x = curve.x + curve.w + GROUP_GAP * s;
        let filter_w = (width - PAD * s - filter_x).max(0);
        let filters = [
            Rect::new(filter_x, band.y + band.h / 2 - 18 * s, filter_w, 16 * s),
            Rect::new(filter_x, band.y + band.h / 2 + 2 * s, filter_w, 16 * s),
        ];
        let dividers = [comp_x - GROUP_GAP * s / 2, filter_x - GROUP_GAP * s / 2];
        Self { scale, band, title, volume, spectrum, pre_gain, compressor, bypass, curve, filters, dividers }
    }
}

fn surface_color() -> Rgb {
    theme::panel_2()
}
fn grid_color() -> Rgb {
    theme::line()
}
fn muted_color() -> Rgb {
    theme::muted()
}
const ACCENT: Rgb = theme::ACCENT;
/// The compressor's threshold, a value the user set.
const THRESHOLD: Rgb = theme::VALUE;
fn scope_bg_color() -> Rgb {
    theme::screen()
}
fn scope_grid_color() -> Rgb {
    theme::panel_2()
}
/// The scope's and the spectrum's trace: the sound playing.
const PHOSPHOR: Rgb = theme::LIVE;

/// What the output panel draws besides its controls.
#[derive(Debug)]
pub struct OutputView {
    pub master: MasterSettings,
    /// The analyser's bytes, one a bin.
    pub spectrum: Vec<u8>,
}

pub fn draw(canvas: &mut Canvas, view: &OutputView, layout: &OutputLayout) {
    let s = layout.scale;
    if layout.band.h > 0 {
        // The strip: a panel along the bottom, edged on top.
        canvas.fill_rect(layout.band.x, layout.band.y, layout.band.w, layout.band.h, theme::panel());
        canvas.fill_rect(layout.band.x, layout.band.y, layout.band.w, s.px(), theme::line());
    }
    canvas.text(layout.title.0, layout.title.1, "Master", s, ACCENT);
    if layout.spectrum.h > 8 * s {
        draw_spectrum(canvas, &view.spectrum, layout.spectrum, s);
        draw_curve(canvas, &view.master, layout.curve, s);
    }
    for x in layout.dividers {
        canvas.fill_rect(x, layout.volume.y, s.px(), layout.volume.h, theme::line());
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
fn draw_curve(canvas: &mut Canvas, master: &MasterSettings, r: Rect, s: Scale) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, surface_color());
    let x_px = |db: f64| r.x + ((db - CURVE_X.0) / (CURVE_X.1 - CURVE_X.0) * f64::from(r.w)) as i32;
    let y_px = |db: f64| r.y + r.h - ((db - CURVE_Y.0) / (CURVE_Y.1 - CURVE_Y.0) * f64::from(r.h)) as i32;
    for db in [-60.0, -40.0, -20.0, 0.0] {
        let (x, y) = (x_px(db).min(r.x + r.w - 1), y_px(db));
        canvas.fill_rect(x, r.y, s.px(), r.h, grid_color());
        canvas.fill_rect(r.x, y, r.w, s.px(), grid_color());
    }
    let (from, to) = (CURVE_X.0.max(CURVE_Y.0), CURVE_X.1.min(CURVE_Y.1));
    line(canvas, (x_px(from), y_px(from)), (x_px(to), y_px(to)), s.px(), muted_color(), Some((6 * s, 6 * s)));
    let bypass = master.bypass >= 0.5;
    let threshold = f64::from(master.threshold);
    if !bypass && (CURVE_X.0..=CURVE_X.1).contains(&threshold) {
        let x = x_px(threshold).min(r.x + r.w - 1);
        line(canvas, (x, r.y), (x, r.y + r.h - 1), s.px(), THRESHOLD, Some((4 * s, 4 * s)));
    }
    let color = if bypass { muted_color() } else { ACCENT };
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
    // The scale's ends, when they fit side by side.
    if text_width("-60 dB", s) + text_width("0 dB", s) + 16 * s <= r.w {
        canvas.text(r.x + 4 * s, r.y + 4 * s, "12 dB", s, muted_color());
        canvas.text(r.x + 4 * s, r.y + r.h - 12 * s, "-60 dB", s, muted_color());
        canvas.text(r.x + r.w - 4 * s - text_width("0 dB", s), r.y + r.h - 12 * s, "0 dB", s, muted_color());
    }
    if bypass {
        let label = "BYPASSED";
        canvas.text(r.x + (r.w - text_width(label, s)) / 2, r.y + r.h / 2 - 4 * s, label, s, muted_color());
    }
}

/// The spectrum: a grid, then the first bins (one a
/// column of the JS canvas, 256) as a line, louder higher.
fn draw_spectrum(canvas: &mut Canvas, bins: &[u8], r: Rect, s: Scale) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, scope_bg_color());
    for i in 1..8 {
        canvas.fill_rect(r.x + r.w * i / 8, r.y, s.px(), r.h, scope_grid_color());
    }
    for i in 1..4 {
        canvas.fill_rect(r.x, r.y + r.h * i / 4, r.w, s.px(), scope_grid_color());
    }
    let count = bins.len().min(256);
    let mut previous = None;
    for (i, &v) in bins.iter().take(count).enumerate() {
        let x = r.x + (i as i32 * r.w) / count as i32;
        let y = r.y + r.h - 1 - (i32::from(v) * (r.h - 1)) / 255;
        if let Some(p) = previous {
            line(canvas, p, (x, y), 2 * s, PHOSPHOR, None);
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
    fn draws_at_any_size() {
        let bins: Vec<u8> = (0..2048).map(|i| (i % 256) as u8).collect();
        for (w, h) in [(1, 1), (40, 30), (1100, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let (wi, hi) = (w as i32, h as i32);
            let band = Rect::new(0, (hi - STRIP_HEIGHT).max(0), wi, hi.min(STRIP_HEIGHT));
            let layout = OutputLayout::strip(wi, band, Scale::ONE);
            for bypass in [0.0, 1.0] {
                let master = MasterSettings { bypass, ..MasterSettings::default() };
                draw(&mut canvas, &OutputView { master, spectrum: bins.clone() }, &layout);
            }
        }
    }
}
