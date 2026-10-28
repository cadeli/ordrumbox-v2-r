//! Value controls (`ui/components/base_control.js`, `or_knob.js`,
//! `or_slider.js`): the value side of a knob or a slider — its range, step
//! and scale, what a drag, a key, the wheel or a typed number make of the
//! value — and the display formats of the track editor, the note editor and
//! the kit editor, all computed as the JS computes them.
//!
//! Two additions: the mouse wheel steps any control like an arrow key, and
//! Shift drags a slider finely, as it already does a knob (the JS range
//! input has no fine drag).

use ordrumbox_core::js;

/// Pixels of vertical drag that cross a knob's whole range (`/ 200`).
const DRAG_RANGE_PX: f64 = 200.0;

/// Knob or slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Knob,
    Slider,
}

/// A control's range: `min`–`max` by `step`, `default` restored by a
/// double-click, logarithmic when asked and possible (`OrKnob` only).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub style: Style,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    log: bool,
}

impl Spec {
    pub const fn knob(min: f64, max: f64, step: f64, default: f64) -> Self {
        Self { style: Style::Knob, min, max, step, default, log: false }
    }

    pub const fn slider(min: f64, max: f64, step: f64, default: f64) -> Self {
        Self { style: Style::Slider, min, max, step, default, log: false }
    }

    /// `scale: 'log'`; ignored (linear) unless `0 < min < max`, as in the JS.
    pub fn log(self) -> Self {
        Self { log: self.min > 0.0 && self.max > self.min, ..self }
    }

    fn clamp(&self, value: f64) -> f64 {
        value.max(self.min).min(self.max)
    }

    /// Where `value` sits, 0–1 (the knob's arc is 270° of it).
    pub fn fraction(&self, value: f64) -> f64 {
        let f = if self.log {
            (value.max(self.min).log10() - self.min.log10()) / (self.max.log10() - self.min.log10())
        } else {
            (value - self.min) / (self.max - self.min)
        };
        if f.is_nan() { 0.0 } else { f.clamp(0.0, 1.0) }
    }

    /// `#clampStep`: `raw` rounded to a multiple of `step`, inside the range.
    fn clamp_step(&self, raw: f64, step: f64) -> f64 {
        self.clamp(js::round(raw / step) * step)
    }

    /// A knob dragged `dy` pixels up (negative: down) from `start` (Shift: finely).
    pub fn drag(&self, start: f64, dy: f64, fine: bool) -> f64 {
        let step = if fine { self.step * 0.1 } else { self.step };
        let log_min = self.min.log10();
        let log_range = self.max.log10() - log_min;
        let base = if self.log { log_range / DRAG_RANGE_PX } else { (self.max - self.min) / DRAG_RANGE_PX };
        let sensitivity = if fine { base * 0.1 } else { base };
        if self.log {
            let start_pos = (start.max(self.min).log10() - log_min) / log_range;
            let pos = (start_pos + dy * sensitivity).clamp(0.0, 1.0);
            self.clamp_step(10f64.powf(log_min + pos * log_range), step)
        } else {
            self.clamp_step(start + dy * sensitivity, step)
        }
    }

    /// An arrow key, or a wheel notch: one step up or down (Shift or Alt: a
    /// tenth of a step, Ctrl: ten steps). A knob keeps to its (fine) step; a
    /// slider is only kept in range (`OrSlider.handleKeydown`).
    pub fn nudge(&self, value: f64, up: bool, fine: bool, coarse: bool) -> f64 {
        let multiplier = if fine {
            0.1
        } else if coarse {
            10.0
        } else {
            1.0
        };
        let delta = if up { 1.0 } else { -1.0 } * self.step * multiplier;
        match self.style {
            Style::Knob => self.clamp_step(value + delta, if fine { self.step * 0.1 } else { self.step }),
            Style::Slider => self.clamp(value + delta),
        }
    }

    /// A slider pressed or dragged at `fraction` of its width: the range
    /// input's value there, on the step grid.
    pub fn slider_at(&self, fraction: f64) -> f64 {
        let raw = self.min + fraction.clamp(0.0, 1.0) * (self.max - self.min);
        self.clamp(self.min + js::round((raw - self.min) / self.step) * self.step)
    }

    /// A slider dragged finely (Shift) by `dfraction` of its width from `start`.
    pub fn slider_fine(&self, start: f64, dfraction: f64) -> f64 {
        self.clamp_step(start + dfraction * (self.max - self.min) * 0.1, self.step * 0.1)
    }

    /// A number typed in (`sanitizePromptInput`): a knob on its step grid, a
    /// slider in range.
    pub fn typed(&self, number: f64) -> f64 {
        match self.style {
            Style::Knob => self.clamp_step(number, self.step),
            Style::Slider => self.clamp(number),
        }
    }
}

// ─── Display ─────────────────────────────────────────────────────────────────

/// How a value is shown (the `format` of each control in the JS).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    /// `fmt`: two decimals at most.
    Fmt,
    /// `String(v)`.
    Plain,
    /// Knob bar velocity: a percentage (`knobFormat`).
    Percent,
    /// Knob bar pitch: `+3`.
    SignedPlain,
    /// `${Math.round(v)} ms`.
    Millis,
    /// Filter frequency: `850Hz`, `1.2k`.
    Freq,
    /// `toFixed(2)`.
    Fixed2,
    /// `toFixed(1)`.
    Fixed1,
    /// `formatSignedDb`: `+1.0`.
    SignedFixed1,
    /// The loop point as `beat.step` at this many steps per beat.
    LoopPoint(i32),
    /// `parseFloat(v.toFixed(3))`.
    Fmt3,
    /// `Math.round(v)`.
    Round,
    /// Auto density: `Auto` below 0.
    Density,
}

impl Format {
    /// The value's text (the JS formatter's result, as `String()` shows it).
    pub fn text(self, v: f64) -> String {
        match self {
            Self::Fmt => fmt(v),
            Self::Plain => js::number_to_string(v),
            Self::Percent => js::number_to_string(js::round(v * 100.0)),
            Self::SignedPlain => format!("{}{}", if v >= 0.0 { "+" } else { "" }, js::number_to_string(v)),
            Self::Millis => format!("{} ms", js::number_to_string(js::round(v))),
            Self::Freq => {
                let hz = js::round(if v.is_finite() { v } else { 20.0 });
                if hz >= 1000.0 {
                    format!("{}k", to_fixed_text(hz / 1000.0, 1))
                } else {
                    format!("{}Hz", js::number_to_string(hz))
                }
            }
            Self::Fixed2 => to_fixed_text(v, 2),
            Self::Fixed1 => to_fixed_text(v, 1),
            Self::SignedFixed1 => format!("{}{}", if v >= 0.0 { "+" } else { "" }, to_fixed_text(v, 1)),
            Self::LoopPoint(spb) => {
                let spb = i64::from(spb.max(1));
                let step = v as i64 - 1;
                format!("{}.{}", step.div_euclid(spb) + 1, step % spb + 1)
            }
            Self::Fmt3 => js::number_to_string(js::to_fixed(v, 3)),
            Self::Round => js::number_to_string(js::round(v)),
            Self::Density => {
                if v < 0.0 {
                    "Auto".to_owned()
                } else {
                    to_fixed_text(v, 2)
                }
            }
        }
    }

    /// `formatValue`: the text cut to 8 characters, then the unit.
    pub fn value(self, v: f64, unit: &str) -> String {
        let text: String = self.text(v).chars().take(8).collect();
        if unit.is_empty() { text } else { format!("{text} {unit}") }
    }
}

/// `fmt`: `parseFloat(Number(v).toFixed(2))`.
pub fn fmt(v: f64) -> String {
    js::number_to_string(js::to_fixed(v, 2))
}

/// `v.toFixed(digits)`.
pub fn to_fixed_text(v: f64, digits: usize) -> String {
    if !v.is_finite() {
        return js::number_to_string(v);
    }
    format!("{:.*}", digits, js::to_fixed(v, digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ordrumbox_core::json::{self, Value};

    fn num(v: &Value) -> f64 {
        v.as_f64().unwrap()
    }

    fn rows<'a>(case: &'a Value, key: &str) -> &'a [Value] {
        case.get(key).and_then(Value::as_array).unwrap()
    }

    /// Every drag, key, typed number and arc of the JS OrKnob / OrSlider
    /// (tools/export_control_fixtures.mjs), and every format.
    #[test]
    fn golden_controls() {
        let fixture = json::parse(include_str!("../tests/fixtures/controls.json")).unwrap();
        let mut checked = 0;
        for case in fixture.get("cases").and_then(Value::as_array).unwrap() {
            let (min, max, step) =
                (num(case.get("min").unwrap()), num(case.get("max").unwrap()), num(case.get("step").unwrap()));
            let mut knob = Spec::knob(min, max, step, min);
            if case.get("scale").and_then(Value::as_str) == Some("log") {
                knob = knob.log();
            }
            let slider = Spec::slider(min, max, step, min);
            let name = case.get("name").and_then(Value::as_str).unwrap();
            for row in rows(case, "drags") {
                let r = row.as_array().unwrap();
                let got = knob.drag(num(&r[0]), num(&r[1]), r[2].as_bool().unwrap());
                assert_eq!(got, num(&r[3]), "{name} drag {r:?}");
                checked += 1;
            }
            for (key, spec) in [("knobKeys", knob), ("sliderKeys", slider)] {
                for row in rows(case, key) {
                    let r = row.as_array().unwrap();
                    let b = |i: usize| r[i].as_bool().unwrap();
                    let got = spec.nudge(num(&r[0]), b(1), b(2) || b(3), b(4));
                    assert_eq!(got, num(&r[5]), "{name} {key} {r:?}");
                    checked += 1;
                }
            }
            for row in rows(case, "arcs") {
                let r = row.as_array().unwrap();
                assert!((knob.fraction(num(&r[0])) - num(&r[1])).abs() < 1e-12, "{name} arc {r:?}");
            }
            for (key, spec) in [("knobTyped", knob), ("sliderTyped", slider)] {
                for row in rows(case, key) {
                    let r = row.as_array().unwrap();
                    assert_eq!(spec.typed(num(&r[0])), num(&r[1]), "{name} {key} {r:?}");
                }
            }
        }
        assert!(checked > 2000);
        for format in fixture.get("formats").and_then(Value::as_array).unwrap() {
            let f = match format.get("name").and_then(Value::as_str).unwrap() {
                "Fmt" => Format::Fmt,
                "Plain" => Format::Plain,
                "Percent" => Format::Percent,
                "SignedPlain" => Format::SignedPlain,
                "Millis" => Format::Millis,
                "Freq" => Format::Freq,
                "Fixed2" => Format::Fixed2,
                "Fixed1" => Format::Fixed1,
                "SignedFixed1" => Format::SignedFixed1,
                "Density" => Format::Density,
                other => panic!("format {other}"),
            };
            let unit = format.get("unit").and_then(Value::as_str).unwrap();
            for row in rows(format, "values") {
                let r = row.as_array().unwrap();
                assert_eq!(f.value(num(&r[0]), unit), r[1].as_str().unwrap(), "{f:?} {r:?}");
            }
        }
        for row in fixture.get("loopPoints").and_then(Value::as_array).unwrap() {
            let r = row.as_array().unwrap();
            assert_eq!(Format::LoopPoint(num(&r[0]) as i32).value(num(&r[1]), ""), r[2].as_str().unwrap());
        }
    }

    #[test]
    fn formats() {
        assert_eq!(Format::Fmt.text(0.505), "0.51");
        assert_eq!(Format::Fmt.text(1.0), "1");
        assert_eq!(Format::Freq.text(999.4), "999Hz");
        assert_eq!(Format::Freq.text(12_345.0), "12.3k");
        assert_eq!(Format::LoopPoint(4).text(5.0), "2.1");
        assert_eq!(Format::Density.text(-1.0), "Auto");
        assert_eq!(Format::Millis.value(5000.0, ""), "5000 ms");
        assert_eq!(Format::Plain.value(0.333_333, "x"), "0.333333 x");
        assert_eq!(Format::Plain.value(0.123_456_789, ""), "0.123456");
        assert_eq!(Format::SignedFixed1.text(-0.04), "-0.0");
    }

    #[test]
    fn knob_drag_and_keys() {
        let pan = Spec::knob(-1.0, 1.0, 0.01, 0.0);
        assert_eq!(pan.drag(0.0, 100.0, false), 1.0);
        assert_eq!(pan.drag(0.0, 10.0, true), 0.01);
        assert_eq!(pan.nudge(0.5, false, false, true), 0.4);
        let decay = Spec::knob(20.0, 5000.0, 10.0, 500.0).log();
        assert!((decay.fraction(20.0)).abs() < 1e-12 && (decay.fraction(5000.0) - 1.0).abs() < 1e-12);
        assert_eq!(decay.drag(500.0, -1000.0, false), 20.0);
        let linear = Spec::knob(0.0, 10.0, 1.0, 0.0).log();
        assert_eq!(linear.fraction(5.0), 0.5); // log needs min > 0
    }

    #[test]
    fn slider_grid() {
        let s = Spec::slider(1.0, 8.0, 1.0, 4.0);
        assert_eq!(s.slider_at(0.0), 1.0);
        assert_eq!(s.slider_at(0.5), 5.0);
        assert_eq!(s.slider_at(2.0), 8.0);
        assert_eq!(s.nudge(8.0, true, false, false), 8.0);
        assert_eq!(s.nudge(4.0, true, true, false), 4.1);
        assert_eq!(s.typed(99.0), 8.0);
        assert_eq!(s.slider_fine(4.0, 0.6), 4.4);
    }
}
