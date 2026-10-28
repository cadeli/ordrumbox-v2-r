//! The output panel (`output_panel.js` and its sections), in the place of
//! the grid: the master bus settings in three tabs — **vol** (the master
//! volume, the spectrum), **Comp** (pre-gain, the compressor's transfer curve,
//! its on/off button and six knobs), **Flt** (low cut, high cut) — kept in
//! `settings.json` (`settings.master`, `MASTER_BUS_DEFAULTS` for what it lacks).
//!
//! [`OutputLayout`] places everything; the controls are panel items, the
//! curve and the spectrum are drawn here.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::MasterSettings;

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::{Format, Spec};
use crate::panel::Rect;
use crate::view;

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

/// The volume knob (vol tab).
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
    param("lowcut", "Low Cut", Spec::slider(10.0, 500.0, 1.0, 35.0), Format::Round, "Hz"),
    param("hicut", "High Cut", Spec::slider(1000.0, 20_000.0, 100.0, 18_500.0), Format::Round, "Hz"),
];
/// The tabs.
pub const TABS: [&str; 3] = ["vol", "Comp", "Flt"];

/// Every knob and slider.
pub fn params() -> impl Iterator<Item = &'static Param> {
    [&VOLUME, &PRE_GAIN].into_iter().chain(COMPRESSOR.iter()).chain(FILTERS.iter())
}

pub fn find(key: &str) -> Option<&'static Param> {
    params().find(|p| p.key == key)
}

/// Setting `key` of `master`, as `settings.master` holds it.
pub fn get(master: &MasterSettings, key: &str) -> f64 {
    f64::from(match key {
        "volume" => master.master,
        "preGain" => master.pre_gain,
        "threshold" => master.threshold,
        "ratio" => master.ratio,
        "attack" => master.attack,
        "release" => master.release,
        "knee" => master.knee,
        "makeup" => master.makeup,
        "lowcut" => master.lowcut,
        "hicut" => master.hicut,
        "compBypass" => master.bypass,
        _ => 0.0,
    })
}

/// `master` with setting `key` at `value`.
pub fn set(master: MasterSettings, key: &str, value: f64) -> MasterSettings {
    let mut m = master;
    let v = value as f32;
    match key {
        "volume" => m.master = v,
        "preGain" => m.pre_gain = v,
        "threshold" => m.threshold = v,
        "ratio" => m.ratio = v,
        "attack" => m.attack = v,
        "release" => m.release = v,
        "knee" => m.knee = v,
        "makeup" => m.makeup = v,
        "lowcut" => m.lowcut = v,
        "hicut" => m.hicut = v,
        "compBypass" => m.bypass = v,
        _ => {}
    }
    m
}

/// `settings.master` read over the defaults; values out of range clamped.
pub fn from_json(value: &Value) -> MasterSettings {
    let mut master = MasterSettings::default();
    let Some(object) = value.as_object() else { return master };
    for key in params().map(|p| p.key) {
        if let Some(v) = object.get(key).and_then(Value::as_f64) {
            master = set(master, key, v);
        }
    }
    if let Some(bypass) = object.get("compBypass").and_then(Value::as_bool) {
        master.bypass = if bypass { 1.0 } else { 0.0 };
    }
    master.clamped()
}

/// `settings.master`.
pub fn to_json(master: &MasterSettings) -> Value {
    let mut object = Object::new();
    for key in params().map(|p| p.key) {
        object.insert(key, Value::from(get(master, key)));
    }
    object.insert("compBypass", Value::Bool(master.bypass >= 0.5));
    Value::Object(object)
}

/// The compressor's gain reduction at `input` dB (`#gainReductionDb`).
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
    pub title: (i32, i32),
    pub tabs: [Rect; 3],
    /// vol: the volume knob, the spectrum.
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
    pub fn new(width: i32, scale: i32) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = view::body_top(scale);
        let tab_y = top + 14 * s;
        let tabs = [0, 1, 2].map(|i| Rect::new(margin + i * 64 * s, tab_y, 60 * s, 14 * s));
        let y = tab_y + 24 * s;
        let knob = |x: i32, y: i32| Rect::new(x, y, 76 * s, 56 * s);
        let curve = Rect::new(margin + 80 * s, y, 320 * s, 140 * s);
        let header_y = curve.y + curve.h + 10 * s;
        let knobs_y = header_y + 22 * s;
        let compressor = [0, 1, 2, 3, 4, 5].map(|i| knob(margin + i * 82 * s, knobs_y));
        let slider_w = (width - 2 * margin).clamp(0, 420 * s);
        Self {
            scale,
            title: (margin, top),
            tabs,
            volume: knob(margin, y),
            spectrum: Rect::new(margin, y + 70 * s, 256 * 2 * s, 100 * 2 * s),
            pre_gain: knob(margin, y + 40 * s),
            curve,
            compressor_title: (margin, header_y + 3 * s),
            bypass: Rect::new(margin + 100 * s, header_y, 40 * s, 14 * s),
            compressor,
            filters: [Rect::new(margin, y, slider_w, 16 * s), Rect::new(margin, y + 24 * s, slider_w, 16 * s)],
        }
    }
}

const SURFACE: Rgb = 0x0024_242E;
const GRID: Rgb = 0x0038_3845;
const MUTED: Rgb = 0x0080_8090;
const ACCENT: Rgb = 0x00F2_A93B;
const WARNING: Rgb = 0x00E8_6A5A;
const SCOPE_BG: Rgb = 0x0008_1410;
const SCOPE_GRID: Rgb = 0x0010_3030;
const PHOSPHOR: Rgb = 0x0060_FF90;
const TEXT: Rgb = 0x00E6_E6EA;

/// What the output panel draws besides its controls.
#[derive(Debug)]
pub struct OutputView {
    pub tab: u8,
    pub master: MasterSettings,
    /// The analyser's bytes, one a bin (`getByteFrequencyData`).
    pub spectrum: Vec<u8>,
}

pub fn draw(canvas: &mut Canvas, view: &OutputView, layout: &OutputLayout) {
    let s = layout.scale;
    canvas.text(layout.title.0, layout.title.1, "Master", s, TEXT);
    match view.tab {
        0 => draw_spectrum(canvas, &view.spectrum, layout.spectrum, s),
        1 => {
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

/// The compressor's transfer curve (`drawCompCurve`): -60 to 0 dB in, -60 to
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

/// The spectrum (`drawSpectrum`): a grid, then the first bins (one a
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
        if let Some(p) = previous {
            line(canvas, p, (x, y), 2 * s, PHOSPHOR, None);
        }
        previous = Some((x, y));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn settings_round_trip_over_the_defaults() {
        let master = from_json(&json::parse(r#"{"volume":1.5,"compBypass":true,"hicut":99999,"ratio":"x"}"#).unwrap());
        assert_eq!((master.master, master.bypass, master.hicut, master.ratio), (1.5, 1.0, 22_000.0, 8.0));
        let back = from_json(&to_json(&master));
        assert_eq!(back, master);
        assert_eq!(to_json(&MasterSettings::default()).get("compBypass"), Some(&Value::Bool(false)));
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
            let layout = OutputLayout::new(w as i32, 1);
            for tab in 0..3 {
                let master = MasterSettings { bypass: f32::from(tab == 2), ..MasterSettings::default() };
                draw(&mut canvas, &OutputView { tab, master, spectrum: bins.clone() }, &layout);
                draw_curve(&mut canvas, &master, layout.curve, 1);
            }
        }
    }
}
