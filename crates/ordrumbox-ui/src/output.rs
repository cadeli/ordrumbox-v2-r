//! The master bus and the shared returns as the mixer shows them: their
//! controls (keys, labels, ranges, how their values show), the
//! compressor's transfer curve and the spectrum of what plays. The master
//! bus is kept in `settings.master` (`MASTER_BUS_DEFAULTS` for what it
//! lacks), the returns in `settings.returns`.

use crate::scale::Scale;
use ordrumbox_engine::{MasterSettings, gain_reduction};

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

/// The master volume, a fader.
pub const VOLUME: Param = param("volume", "Volume", Spec::fader(0.0, 2.0, 0.01, 1.0), Format::Fixed2, "");
/// The pre-gain knob, ahead of the compressor.
pub const PRE_GAIN: Param = param("preGain", "Pre-Gain", Spec::knob(-20.0, 20.0, 0.5, 0.0), Format::SignedFixed1, "dB");
/// The compressor knobs: under 1 a step shows 3 decimals, else whole. Each
/// range is the one the master bus clamps to (`MasterSettings::MIN`, `MAX`),
/// so the value shown is the value used.
pub const COMPRESSOR: [Param; 6] = [
    param("threshold", "Threshold", Spec::knob(-60.0, 0.0, 1.0, -18.0), Format::Round, "dB"),
    param("ratio", "Ratio", Spec::knob(1.0, 20.0, 0.5, 8.0), Format::Fmt3, ""),
    param("attack", "Attack", Spec::knob(0.001, 1.0, 0.001, 0.002), Format::Fmt3, "s"),
    param("release", "Release", Spec::knob(0.01, 1.0, 0.001, 0.08), Format::Fmt3, "s"),
    param("knee", "Knee", Spec::knob(0.0, 40.0, 1.0, 3.0), Format::Round, "dB"),
    param("makeup", "Makeup", Spec::knob(0.0, 24.0, 0.5, 8.0), Format::Fmt3, "dB"),
];
/// The filter knobs.
pub const FILTERS: [Param; 2] = [
    param("lowcut", "Low Cut", Spec::knob(10.0, 500.0, 1.0, 35.0).log_full_travel(), Format::Round, "Hz"),
    param("hicut", "High Cut", Spec::knob(1000.0, 22_000.0, 100.0, 18_500.0).log_full_travel(), Format::Round, "Hz"),
];

/// Every knob and slider of the master bus.
pub fn params() -> impl Iterator<Item = &'static Param> {
    [&VOLUME, &PRE_GAIN].into_iter().chain(COMPRESSOR.iter()).chain(FILTERS.iter())
}

pub fn find(key: &str) -> Option<&'static Param> {
    params().find(|p| p.key == key)
}

/// The returns' controls: the echo's ([`ECHO_CONTROLS`] of them), then the
/// reverb's, each return's level last. Each range is the one the returns clamp to (`ReturnSettings::MIN`,
/// `MAX`). The echo's wow is set in `settings.json` only.
pub const RETURNS: [Param; 9] = [
    param("echoBeats", "Time", Spec::knob(0.25, 4.0, 0.25, 0.75), Format::Fmt, "beat"),
    param("echoFeedback", "Feedback", Spec::knob(0.0, 0.99, 0.01, 0.55), Format::Fixed2, ""),
    param("echoTone", "Tone", Spec::knob(0.05, 1.0, 0.01, 0.35), Format::Fixed2, ""),
    param("echoReverb", "To Rev", Spec::knob(0.0, 1.0, 0.01, 0.25), Format::Fixed2, ""),
    param("echoLevel", "Echo", Spec::fader(0.0, 1.0, 0.01, 1.0), Format::Fixed2, ""),
    param("reverbRoom", "Size", Spec::knob(0.0, 1.0, 0.01, 0.8), Format::Fixed2, ""),
    param("reverbDamping", "Damp", Spec::knob(0.0, 1.0, 0.01, 0.4), Format::Fixed2, ""),
    param("reverbSpring", "Spring", Spec::knob(0.0, 0.9, 0.01, 0.6), Format::Fixed2, ""),
    param("reverbLevel", "Reverb", Spec::fader(0.0, 1.0, 0.01, 1.0), Format::Fixed2, ""),
];
/// The echo's controls, the first of [`RETURNS`]: its knobs, then its level.
pub const ECHO_CONTROLS: usize = 5;

pub fn find_return(key: &str) -> Option<&'static Param> {
    RETURNS.iter().find(|p| p.key == key)
}

/// The output level for `input` dB (the curve), with the settings the bus
/// uses (clamped): the input when bypassed.
pub fn transfer(master: &MasterSettings, input: f64) -> f64 {
    let master = master.clamped();
    if master.bypass >= 0.5 {
        return input;
    }
    let (threshold, ratio, knee) =
        (f64::from(master.threshold), f64::from(master.ratio).max(1.0), f64::from(master.knee).max(0.0));
    input - gain_reduction(input, threshold, ratio, knee) + f64::from(master.makeup)
}

/// The curve's range, dB: in across, out up.
const CURVE_X: (f64, f64) = (-60.0, 0.0);
const CURVE_Y: (f64, f64) = (-60.0, 12.0);

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
pub fn draw_curve(canvas: &mut Canvas, master: &MasterSettings, r: Rect, s: Scale) {
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

/// The spectrum: a grid, then the first 256 bins as a line, louder higher.
pub fn draw_spectrum(canvas: &mut Canvas, bins: &[u8], r: Rect, s: Scale) {
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
    use ordrumbox_engine::ReturnSettings;

    #[test]
    fn every_control_is_a_saved_setting() {
        let keys: Vec<_> = params().map(|p| p.key).collect();
        assert_eq!(keys, crate::master::KEYS);
    }

    /// The master bus setting a control sets.
    fn setting(settings: &MasterSettings, key: &str) -> f32 {
        match key {
            "volume" => settings.master,
            "preGain" => settings.pre_gain,
            "threshold" => settings.threshold,
            "ratio" => settings.ratio,
            "attack" => settings.attack,
            "release" => settings.release,
            "knee" => settings.knee,
            "makeup" => settings.makeup,
            "lowcut" => settings.lowcut,
            "hicut" => settings.hicut,
            other => panic!("no setting for {other}"),
        }
    }

    #[test]
    fn every_control_spans_the_range_the_bus_uses() {
        for p in params() {
            let (min, max, default) = (p.spec.min as f32, p.spec.max as f32, p.spec.default.unwrap() as f32);
            assert_eq!(min, setting(&MasterSettings::MIN, p.key), "{} min", p.key);
            assert_eq!(max, setting(&MasterSettings::MAX, p.key), "{} max", p.key);
            assert_eq!(default, setting(&MasterSettings::default(), p.key), "{} default", p.key);
        }
    }

    #[test]
    fn every_return_knob_is_a_saved_setting_and_spans_the_range_the_returns_use() {
        let section = &crate::returns::SECTION;
        for p in &RETURNS {
            assert!(crate::returns::KEYS.contains(&p.key), "{}", p.key);
            let at = |settings: &ReturnSettings| section.get(settings, p.key) as f32;
            assert_eq!(p.spec.min as f32, at(&ReturnSettings::MIN), "{} min", p.key);
            assert_eq!(p.spec.max as f32, at(&ReturnSettings::MAX), "{} max", p.key);
            assert_eq!(p.spec.default.unwrap() as f32, at(&ReturnSettings::default()), "{} default", p.key);
        }
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
    fn the_curve_and_the_spectrum_draw_at_any_size() {
        let bins: Vec<u8> = (0..2048).map(|i| (i % 256) as u8).collect();
        for (w, h) in [(1, 1), (40, 30), (300, 200)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let r = Rect::new(0, 0, w as i32, h as i32);
            for bypass in [0.0, 1.0] {
                draw_curve(&mut canvas, &MasterSettings { bypass, ..MasterSettings::default() }, r, Scale::ONE);
            }
            draw_spectrum(&mut canvas, &bins, r, Scale::ONE);
        }
    }
}
