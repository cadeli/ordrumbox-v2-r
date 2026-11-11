//! Value controls (`ui/components/base_control.js`, `or_knob.js`,
//! `or_slider.js`): the value side of a knob or a slider — its range, step
//! and scale, what a drag, a key, the wheel or a typed number make of the
//! value — and the display formats of the track editor, the note editor and
//! the kit editor.
//!
//! Three additions: the mouse wheel steps any control like an arrow key,
//! Shift drags a slider finely, as it already does a knob (a plain range
//! input has no fine drag), and a slider can be logarithmic.

use ordrumbox_core::num::{self, round_to};

/// Pixels of vertical drag that cross a knob's whole range (`/ 200`).
const DRAG_RANGE_PX: f64 = 200.0;

/// How a value is shown: a knob for a sound, a slider for an amount, a
/// stepper (- 4 +) for a count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Knob,
    Slider,
    Stepper,
}

/// A fraction as a percentage: 0.5 is 50 %.
const PERCENT: f64 = 100.0;

/// A control's range: `min`–`max` by `step`, `default` restored by a
/// double-click, logarithmic when asked and possible.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spec {
    pub style: Style,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
    log: bool,
    full_travel: bool,
    percent: bool,
}

impl Spec {
    pub const fn knob(min: f64, max: f64, step: f64, default: f64) -> Self {
        Self { style: Style::Knob, min, max, step, default, log: false, full_travel: false, percent: false }
    }

    pub const fn slider(min: f64, max: f64, step: f64, default: f64) -> Self {
        Self { style: Style::Slider, min, max, step, default, log: false, full_travel: false, percent: false }
    }

    /// A count: whole steps only, one per click on its - and + buttons.
    pub const fn stepper(min: f64, max: f64, step: f64, default: f64) -> Self {
        Self { style: Style::Stepper, min, max, step, default, log: false, full_travel: false, percent: false }
    }

    /// `scale: 'log'`; ignored (linear) unless `0 < min < max`.
    pub const fn log(self) -> Self {
        Self { log: self.min > 0.0 && self.max > self.min, ..self }
    }

    /// A logarithmic knob whose whole range takes `DRAG_RANGE_PX` of drag.
    /// `OrKnob` crosses it that many pixels divided by its decades, too fast
    /// for a three-decade frequency.
    pub const fn log_full_travel(self) -> Self {
        Self { full_travel: true, ..self.log() }
    }

    /// A fraction shown and typed as a percentage: 0.5 is `50 %`.
    pub const fn percent(self) -> Self {
        Self { percent: true, ..self }
    }

    /// How the user reads and types `value`: as a percentage for a [`Self::percent`] spec.
    #[must_use]
    pub fn shown(&self, value: f64) -> f64 {
        // Rounded: 0.57 * 100 is not exactly 57.
        if self.percent { round_to(value * PERCENT, 2) } else { value }
    }

    /// The value of a number typed as [`Self::shown`] shows it.
    #[must_use]
    pub fn typed_shown(&self, number: f64) -> f64 {
        if self.percent { number / PERCENT } else { number }
    }

    /// The unit [`Self::shown`] reads in, if it adds one.
    #[must_use]
    pub const fn shown_unit(&self) -> Option<&'static str> {
        if self.percent { Some("%") } else { None }
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

    /// The value at `position` (0–1) of a logarithmic range.
    fn log_value(&self, position: f64) -> f64 {
        let log_min = self.min.log10();
        10f64.powf(log_min + position * (self.max.log10() - log_min))
    }

    /// `#clampStep`: `raw` rounded to a multiple of `step`, inside the range.
    fn clamp_step(&self, raw: f64, step: f64) -> f64 {
        self.clamp((raw / step).round() * step)
    }

    /// A knob dragged `dy` pixels up (negative: down) from `start` (Shift: finely).
    pub fn drag(&self, start: f64, dy: f64, fine: bool) -> f64 {
        let step = if fine { self.step * 0.1 } else { self.step };
        let log_range = self.max.log10() - self.min.log10();
        let base = match (self.log, self.full_travel) {
            (true, true) => 1.0 / DRAG_RANGE_PX,
            (true, false) => log_range / DRAG_RANGE_PX,
            (false, _) => (self.max - self.min) / DRAG_RANGE_PX,
        };
        let sensitivity = if fine { base * 0.1 } else { base };
        if self.log {
            let pos = (self.fraction(start) + dy * sensitivity).clamp(0.0, 1.0);
            self.clamp_step(self.log_value(pos), step)
        } else {
            self.clamp_step(start + dy * sensitivity, step)
        }
    }

    /// An arrow key, or a wheel notch: one step up or down (Shift or Alt: a
    /// tenth of a step, Ctrl: ten steps). A knob keeps to its (fine) step; a
    /// slider is only kept in range (`OrSlider.handleKeydown`). A stepper
    /// has no fine step.
    pub fn nudge(&self, value: f64, up: bool, fine: bool, coarse: bool) -> f64 {
        let fine = fine && self.style != Style::Stepper;
        let multiplier = if fine {
            0.1
        } else if coarse {
            10.0
        } else {
            1.0
        };
        let delta = if up { 1.0 } else { -1.0 } * self.step * multiplier;
        match self.style {
            Style::Knob | Style::Stepper => {
                self.clamp_step(value + delta, if fine { self.step * 0.1 } else { self.step })
            }
            Style::Slider => self.clamp(value + delta),
        }
    }

    /// A slider pressed or dragged at `fraction` of its width: the range
    /// input's value there, on the step grid.
    pub fn slider_at(&self, fraction: f64) -> f64 {
        let fraction = fraction.clamp(0.0, 1.0);
        let raw = if self.log { self.log_value(fraction) } else { self.min + fraction * (self.max - self.min) };
        self.clamp(self.min + ((raw - self.min) / self.step).round() * self.step)
    }

    /// A slider dragged finely (Shift) by `dfraction` of its width from `start`.
    pub fn slider_fine(&self, start: f64, dfraction: f64) -> f64 {
        let raw = if self.log {
            self.log_value((self.fraction(start) + dfraction * 0.1).clamp(0.0, 1.0))
        } else {
            start + dfraction * (self.max - self.min) * 0.1
        };
        self.clamp_step(raw, self.step * 0.1)
    }

    /// A number typed in: a knob or a stepper on its step grid, a
    /// slider in range.
    pub fn typed(&self, number: f64) -> f64 {
        match self.style {
            Style::Knob | Style::Stepper => self.clamp_step(number, self.step),
            Style::Slider => self.clamp(number),
        }
    }
}

// ─── Display ─────────────────────────────────────────────────────────────────

/// How a value is shown.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    /// Two decimals at most: `0.5`, `0.33`.
    Fmt,
    /// The number as is.
    Plain,
    /// A fraction as a percentage: `50 %` for 0.5.
    Percent,
    /// A chance out of 100: `50 %`.
    Chance,
    /// Knob bar pitch: `+3`.
    SignedPlain,
    /// Whole milliseconds: `250 ms`.
    Millis,
    /// Filter frequency: `850Hz`, `1.2k`.
    Freq,
    /// Exactly two decimals: `0.50`.
    Fixed2,
    /// Exactly one decimal: `0.5`.
    Fixed1,
    /// A gain in dB with its sign: `+1.0`.
    SignedFixed1,
    /// The loop length as `beat.step` at this many steps per beat.
    LoopLength(i32),
    /// Three decimals at most.
    Fmt3,
    /// The nearest whole number.
    Round,
    /// Auto density: `Auto` below 0.
    Density,
    /// A seed: `new` below 1 (a new line each time).
    Seed,
    /// A level in dB, one decimal: `-18.0 dB`.
    Decibels,
    /// A compressor ratio, one decimal: `4.0:1`.
    Ratio,
    /// Milliseconds, one decimal under 10: `2.5 ms`, `120 ms`.
    FineMillis,
}

impl Format {
    /// The value's text.
    pub fn text(self, v: f64) -> String {
        match self {
            Self::Fmt => fmt(v),
            Self::Plain => num::text(v),
            Self::Percent => num::text((v * PERCENT).round()),
            Self::Chance => num::text(v.round()),
            Self::SignedPlain => format!("{}{}", if v >= 0.0 { "+" } else { "" }, num::text(v)),
            Self::Millis => format!("{} ms", num::text(v.round())),
            Self::Freq => {
                let hz = if v.is_finite() { v.round() } else { 20.0 };
                if hz >= 1000.0 {
                    format!("{}k", to_fixed_text(hz / 1000.0, 1))
                } else {
                    format!("{}Hz", num::text(hz))
                }
            }
            Self::Fixed2 => to_fixed_text(v, 2),
            Self::Fixed1 => to_fixed_text(v, 1),
            Self::SignedFixed1 => format!("{}{}", if v >= 0.0 { "+" } else { "" }, to_fixed_text(v, 1)),
            Self::LoopLength(spb) => {
                let spb = i64::from(spb.max(1));
                let step = v as i64 - 1;
                format!("{}.{}", step.div_euclid(spb) + 1, step % spb + 1)
            }
            Self::Fmt3 => num::text(round_to(v, 3)),
            Self::Round => num::text(v.round()),
            Self::Decibels => to_fixed_text(v, 1),
            Self::Ratio => format!("{}:1", to_fixed_text(v, 1)),
            Self::FineMillis => {
                let digits = if v < FINE_MILLIS_BELOW { 1 } else { 0 };
                format!("{} ms", to_fixed_text(v, digits))
            }
            Self::Seed => {
                if v < 1.0 {
                    "new".to_owned()
                } else {
                    num::text(v)
                }
            }
            Self::Density => {
                if v < 0.0 {
                    "Auto".to_owned()
                } else {
                    to_fixed_text(v, 2)
                }
            }
        }
    }

    /// The unit the format gives its text, if any: `%` for a percentage.
    pub const fn unit(self) -> &'static str {
        match self {
            Self::Percent | Self::Chance => "%",
            Self::Decibels => "dB",
            _ => "",
        }
    }

    /// The text cut to 8 characters, then the unit (the format's own when `unit` is empty).
    pub fn value(self, v: f64, unit: &str) -> String {
        let unit = if unit.is_empty() { self.unit() } else { unit };
        let text: String = self.text(v).chars().take(8).collect();
        if unit.is_empty() { text } else { format!("{text} {unit}") }
    }
}

/// Milliseconds under which [`Format::FineMillis`] shows a decimal.
const FINE_MILLIS_BELOW: f64 = 10.0;

/// `v` with two decimals at most (`0.5`, `0.33`).
pub fn fmt(v: f64) -> String {
    num::text(round_to(v, 2))
}

/// `v` with exactly `digits` decimals, rounded as [`round_to`] does (the
/// formatter alone would round an exact tie such as 0.125 to even).
pub fn to_fixed_text(v: f64, digits: usize) -> String {
    let rounded = round_to(v, digits as i32);
    format!("{rounded:.digits$}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ranges of the tables: those of the track and note editors, a
    /// logarithmic one, and a logarithmic one from 0 (linear: log needs min > 0).
    const VELOCITY: Spec = Spec::knob(0.0, 1.0, 0.01, 0.0);
    const PAN: Spec = Spec::knob(-1.0, 1.0, 0.01, -1.0);
    const PITCH: Spec = Spec::knob(-24.0, 24.0, 1.0, -24.0);
    const DECAY: Spec = Spec::knob(20.0, 5000.0, 10.0, 20.0).log();
    const FREQ: Spec = Spec::knob(20.0, 20000.0, 1.0, 20.0);
    const Q: Spec = Spec::knob(0.707, 18.707, 0.01, 0.707);
    const GAIN: Spec = Spec::knob(-24.0, 6.0, 0.1, -24.0);
    const PROB: Spec = Spec::knob(0.0, 100.0, 1.0, 0.0);
    const LFO_FREQ: Spec = Spec::knob(0.1, 2.0, 0.1, 0.1);
    const LINEAR_LOG: Spec = Spec::knob(0.0, 10.0, 0.5, 0.0).log();
    const SPECS: [Spec; 10] = [VELOCITY, PAN, PITCH, DECAY, FREQ, Q, GAIN, PROB, LFO_FREQ, LINEAR_LOG];

    /// A knob dragged: `(spec, start, dy, fine, value)`.
    const DRAGS: &[(Spec, f64, f64, bool, f64)] = &[
        (VELOCITY, 0.137, -120.0, false, 0.0),
        (VELOCITY, 0.137, -5.0, false, 0.11),
        (VELOCITY, 0.137, -5.0, true, 0.135),
        (VELOCITY, 0.137, 1.0, false, 0.14),
        (VELOCITY, 0.137, 7.0, false, 0.17),
        (VELOCITY, 0.137, 7.0, true, 0.14100000000000001),
        (VELOCITY, 0.137, 133.0, false, 0.8),
        (VELOCITY, 0.137, 133.0, true, 0.20400000000000001),
        (VELOCITY, 0.0, 250.0, false, 1.0),
        (VELOCITY, 1.0, -250.0, true, 0.875),
        (PAN, -0.726, -120.0, false, -1.0),
        (PAN, -0.726, -5.0, false, -0.78),
        (PAN, -0.726, -5.0, true, -0.731),
        (PAN, -0.726, 1.0, false, -0.72),
        (PAN, -0.726, 7.0, false, -0.66),
        (PAN, -0.726, 7.0, true, -0.719),
        (PAN, -0.726, 133.0, false, 0.6),
        (PAN, -0.726, 133.0, true, -0.593),
        (PAN, -1.0, 250.0, false, 1.0),
        (PAN, 1.0, -250.0, true, 0.75),
        (PITCH, -17.424, -120.0, false, -24.0),
        (PITCH, -17.424, -5.0, false, -19.0),
        (PITCH, -17.424, -5.0, true, -17.5),
        (PITCH, -17.424, 1.0, false, -17.0),
        (PITCH, -17.424, 7.0, false, -16.0),
        (PITCH, -17.424, 7.0, true, -17.3),
        (PITCH, -17.424, 133.0, false, 14.0),
        (PITCH, -17.424, 133.0, true, -14.200000000000001),
        (PITCH, -24.0, 250.0, false, 24.0),
        (PITCH, 24.0, -250.0, true, 18.0),
        (DECAY, 702.2600000000001, -120.0, false, 20.0),
        (DECAY, 702.2600000000001, -5.0, false, 500.0),
        (DECAY, 702.2600000000001, -5.0, true, 679.0),
        (DECAY, 702.2600000000001, 1.0, false, 750.0),
        (DECAY, 702.2600000000001, 7.0, false, 1120.0),
        (DECAY, 702.2600000000001, 7.0, true, 736.0),
        (DECAY, 702.2600000000001, 133.0, false, 5000.0),
        (DECAY, 702.2600000000001, 133.0, true, 1694.0),
        (DECAY, 20.0, 250.0, false, 5000.0),
        (DECAY, 5000.0, -250.0, true, 955.0),
        (FREQ, 2757.26, -120.0, false, 20.0),
        (FREQ, 2757.26, -5.0, false, 2258.0),
        (FREQ, 2757.26, -5.0, true, 2707.3),
        (FREQ, 2757.26, 1.0, false, 2857.0),
        (FREQ, 2757.26, 7.0, false, 3457.0),
        (FREQ, 2757.26, 7.0, true, 2827.2000000000003),
        (FREQ, 2757.26, 133.0, false, 16044.0),
        (FREQ, 2757.26, 133.0, true, 4085.9),
        (FREQ, 20.0, 250.0, false, 20000.0),
        (FREQ, 20000.0, -250.0, true, 17502.5),
        (Q, 3.173, -120.0, false, 0.707),
        (Q, 3.173, -5.0, false, 2.72),
        (Q, 3.173, -5.0, true, 3.128),
        (Q, 3.173, 1.0, false, 3.2600000000000002),
        (Q, 3.173, 7.0, false, 3.8000000000000003),
        (Q, 3.173, 7.0, true, 3.236),
        (Q, 3.173, 133.0, false, 15.14),
        (Q, 3.173, 133.0, true, 4.37),
        (Q, 0.707, 250.0, false, 18.707),
        (Q, 18.707, -250.0, true, 16.457),
        (GAIN, -19.89, -120.0, false, -24.0),
        (GAIN, -19.89, -5.0, false, -20.6),
        (GAIN, -19.89, -5.0, true, -19.960000000000004),
        (GAIN, -19.89, 1.0, false, -19.700000000000003),
        (GAIN, -19.89, 7.0, false, -18.8),
        (GAIN, -19.89, 7.0, true, -19.780000000000005),
        (GAIN, -19.89, 133.0, false, 0.1),
        (GAIN, -19.89, 133.0, true, -17.890000000000004),
        (GAIN, -24.0, 250.0, false, 6.0),
        (GAIN, 6.0, -250.0, true, 2.2500000000000004),
        (PROB, 13.700000000000001, -120.0, false, 0.0),
        (PROB, 13.700000000000001, -5.0, false, 11.0),
        (PROB, 13.700000000000001, -5.0, true, 13.5),
        (PROB, 13.700000000000001, 1.0, false, 14.0),
        (PROB, 13.700000000000001, 7.0, false, 17.0),
        (PROB, 13.700000000000001, 7.0, true, 14.100000000000001),
        (PROB, 13.700000000000001, 133.0, false, 80.0),
        (PROB, 13.700000000000001, 133.0, true, 20.400000000000002),
        (PROB, 0.0, 250.0, false, 100.0),
        (PROB, 100.0, -250.0, true, 87.5),
        (LFO_FREQ, 0.4, -120.0, false, 0.1),
        (LFO_FREQ, 0.4, -5.0, false, 0.4),
        (LFO_FREQ, 0.4, -5.0, true, 0.4000000000000001),
        (LFO_FREQ, 0.4, 1.0, false, 0.4),
        (LFO_FREQ, 0.4, 7.0, false, 0.5),
        (LFO_FREQ, 0.4, 7.0, true, 0.4100000000000001),
        (LFO_FREQ, 0.4, 133.0, false, 1.7000000000000002),
        (LFO_FREQ, 0.4, 133.0, true, 0.5300000000000001),
        (LFO_FREQ, 0.1, 250.0, false, 2.0),
        (LFO_FREQ, 2.0, -250.0, true, 1.7600000000000002),
        (LINEAR_LOG, 1.5, -120.0, false, 0.0),
        (LINEAR_LOG, 1.5, -5.0, false, 1.5),
        (LINEAR_LOG, 1.5, -5.0, true, 1.5),
        (LINEAR_LOG, 1.5, 1.0, false, 1.5),
        (LINEAR_LOG, 1.5, 7.0, false, 2.0),
        (LINEAR_LOG, 1.5, 7.0, true, 1.55),
        (LINEAR_LOG, 1.5, 133.0, false, 8.0),
        (LINEAR_LOG, 1.5, 133.0, true, 2.15),
        (LINEAR_LOG, 0.0, 250.0, false, 10.0),
        (LINEAR_LOG, 10.0, -250.0, true, 8.75),
    ];

    /// An arrow key on a knob and on a slider: `(spec, value, up, fine, coarse, knob, slider)`.
    const KEYS: &[(Spec, f64, bool, bool, bool, f64, f64)] = &[
        (VELOCITY, 0.137, true, false, false, 0.15, 0.14700000000000002),
        (VELOCITY, 0.137, true, true, false, 0.138, 0.138),
        (VELOCITY, 0.137, true, false, true, 0.24, 0.23700000000000002),
        (VELOCITY, 0.137, false, false, false, 0.13, 0.127),
        (VELOCITY, 0.137, false, true, false, 0.136, 0.136),
        (VELOCITY, 0.137, false, false, true, 0.04, 0.037000000000000005),
        (PAN, -0.726, true, false, false, -0.72, -0.716),
        (PAN, -0.726, true, true, false, -0.725, -0.725),
        (PAN, -0.726, true, false, true, -0.63, -0.626),
        (PAN, -0.726, false, false, false, -0.74, -0.736),
        (PAN, -0.726, false, true, false, -0.727, -0.727),
        (PAN, -0.726, false, false, true, -0.8300000000000001, -0.826),
        (PITCH, -17.424, true, false, false, -16.0, -16.424),
        (PITCH, -17.424, true, true, false, -17.3, -17.323999999999998),
        (PITCH, -17.424, true, false, true, -7.0, -7.4239999999999995),
        (PITCH, -17.424, false, false, false, -18.0, -18.424),
        (PITCH, -17.424, false, true, false, -17.5, -17.524),
        (PITCH, -17.424, false, false, true, -24.0, -24.0),
        (DECAY, 702.2600000000001, true, false, false, 710.0, 712.2600000000001),
        (DECAY, 702.2600000000001, true, true, false, 703.0, 703.2600000000001),
        (DECAY, 702.2600000000001, true, false, true, 800.0, 802.2600000000001),
        (DECAY, 702.2600000000001, false, false, false, 690.0, 692.2600000000001),
        (DECAY, 702.2600000000001, false, true, false, 701.0, 701.2600000000001),
        (DECAY, 702.2600000000001, false, false, true, 600.0, 602.2600000000001),
        (FREQ, 2757.26, true, false, false, 2758.0, 2758.26),
        (FREQ, 2757.26, true, true, false, 2757.4, 2757.36),
        (FREQ, 2757.26, true, false, true, 2767.0, 2767.26),
        (FREQ, 2757.26, false, false, false, 2756.0, 2756.26),
        (FREQ, 2757.26, false, true, false, 2757.2000000000003, 2757.1600000000003),
        (FREQ, 2757.26, false, false, true, 2747.0, 2747.26),
        (Q, 3.173, true, false, false, 3.18, 3.183),
        (Q, 3.173, true, true, false, 3.174, 3.174),
        (Q, 3.173, true, false, true, 3.27, 3.273),
        (Q, 3.173, false, false, false, 3.16, 3.1630000000000003),
        (Q, 3.173, false, true, false, 3.172, 3.172),
        (Q, 3.173, false, false, true, 3.0700000000000003, 3.073),
        (GAIN, -19.89, true, false, false, -19.8, -19.79),
        (GAIN, -19.89, true, true, false, -19.880000000000003, -19.88),
        (GAIN, -19.89, true, false, true, -18.900000000000002, -18.89),
        (GAIN, -19.89, false, false, false, -20.0, -19.990000000000002),
        (GAIN, -19.89, false, true, false, -19.900000000000002, -19.900000000000002),
        (GAIN, -19.89, false, false, true, -20.900000000000002, -20.89),
        (PROB, 13.700000000000001, true, false, false, 15.0, 14.700000000000001),
        (PROB, 13.700000000000001, true, true, false, 13.8, 13.8),
        (PROB, 13.700000000000001, true, false, true, 24.0, 23.700000000000003),
        (PROB, 13.700000000000001, false, false, false, 13.0, 12.700000000000001),
        (PROB, 13.700000000000001, false, true, false, 13.600000000000001, 13.600000000000001),
        (PROB, 13.700000000000001, false, false, true, 4.0, 3.700000000000001),
        (LFO_FREQ, 0.4, true, false, false, 0.5, 0.5),
        (LFO_FREQ, 0.4, true, true, false, 0.4100000000000001, 0.41000000000000003),
        (LFO_FREQ, 0.4, true, false, true, 1.4000000000000001, 1.4),
        (LFO_FREQ, 0.4, false, false, false, 0.30000000000000004, 0.30000000000000004),
        (LFO_FREQ, 0.4, false, true, false, 0.39000000000000007, 0.39),
        (LFO_FREQ, 0.4, false, false, true, 0.1, 0.1),
        (LINEAR_LOG, 1.5, true, false, false, 2.0, 2.0),
        (LINEAR_LOG, 1.5, true, true, false, 1.55, 1.55),
        (LINEAR_LOG, 1.5, true, false, true, 6.5, 6.5),
        (LINEAR_LOG, 1.5, false, false, false, 1.0, 1.0),
        (LINEAR_LOG, 1.5, false, true, false, 1.4500000000000002, 1.45),
        (LINEAR_LOG, 1.5, false, false, true, 0.0, 0.0),
    ];

    /// Numbers typed in each range of `SPECS`.
    const TYPED: [f64; 11] = [-1000000.0, -30.0, -0.5, 0.0, 0.004, 0.333, 1.26, 7.77, 19.5, 333.3, 1000000.0];
    /// What a knob, then a slider, makes of `TYPED`, range by range.
    const TYPED_VALUES: [([f64; 11], [f64; 11]); 10] = [
        (
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.33, 1.0, 1.0, 1.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 0.0, 0.004, 0.333, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
        (
            [-1.0, -1.0, -0.5, 0.0, 0.0, 0.33, 1.0, 1.0, 1.0, 1.0, 1.0],
            [-1.0, -1.0, -0.5, 0.0, 0.004, 0.333, 1.0, 1.0, 1.0, 1.0, 1.0],
        ),
        (
            [-24.0, -24.0, -1.0, 0.0, 0.0, 0.0, 1.0, 8.0, 20.0, 24.0, 24.0],
            [-24.0, -24.0, -0.5, 0.0, 0.004, 0.333, 1.26, 7.77, 19.5, 24.0, 24.0],
        ),
        (
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 330.0, 5000.0],
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 333.3, 5000.0],
        ),
        (
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 333.0, 20000.0],
            [20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 20.0, 333.3, 20000.0],
        ),
        (
            [0.707, 0.707, 0.707, 0.707, 0.707, 0.707, 1.26, 7.7700000000000005, 18.707, 18.707, 18.707],
            [0.707, 0.707, 0.707, 0.707, 0.707, 0.707, 1.26, 7.77, 18.707, 18.707, 18.707],
        ),
        (
            [-24.0, -24.0, -0.5, 0.0, 0.0, 0.30000000000000004, 1.3, 6.0, 6.0, 6.0, 6.0],
            [-24.0, -24.0, -0.5, 0.0, 0.004, 0.333, 1.26, 6.0, 6.0, 6.0, 6.0],
        ),
        (
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 8.0, 20.0, 100.0, 100.0],
            [0.0, 0.0, 0.0, 0.0, 0.004, 0.333, 1.26, 7.77, 19.5, 100.0, 100.0],
        ),
        (
            [0.1, 0.1, 0.1, 0.1, 0.1, 0.30000000000000004, 1.3, 2.0, 2.0, 2.0, 2.0],
            [0.1, 0.1, 0.1, 0.1, 0.1, 0.333, 1.26, 2.0, 2.0, 2.0, 2.0],
        ),
        (
            [0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 1.5, 8.0, 10.0, 10.0, 10.0],
            [0.0, 0.0, 0.0, 0.0, 0.004, 0.333, 1.26, 7.77, 10.0, 10.0, 10.0],
        ),
    ];

    /// Values shown by every format.
    #[expect(clippy::approx_constant, reason = "a number with many decimals, not π")]
    const SHOWN: [f64; 29] = [
        -1234.5, -24.0, -3.14159, -1.0, -0.505, -0.04, 0.0, 0.005, 0.01, 0.125, 0.333333, 0.505, 0.707, 0.999, 1.0,
        1.005, 2.5, 7.0, 12.0, 99.995, 100.0, 499.5, 999.4, 999.6, 1000.0, 1234.56, 5000.0, 12345.678, 20000.0,
    ];
    /// `(format, unit, texts of SHOWN)`.
    const FORMATS: &[(Format, &str, [&str; 29])] = &[
        (
            Format::Fmt,
            "",
            [
                "-1234.5", "-24", "-3.14", "-1", "-0.51", "-0.04", "0", "0.01", "0.01", "0.13", "0.33", "0.51", "0.71",
                "1", "1", "1", "2.5", "7", "12", "100", "100", "499.5", "999.4", "999.6", "1000", "1234.56", "5000",
                "12345.68", "20000",
            ],
        ),
        (
            Format::Plain,
            "",
            [
                "-1234.5", "-24", "-3.14159", "-1", "-0.505", "-0.04", "0", "0.005", "0.01", "0.125", "0.333333",
                "0.505", "0.707", "0.999", "1", "1.005", "2.5", "7", "12", "99.995", "100", "499.5", "999.4", "999.6",
                "1000", "1234.56", "5000", "12345.67", "20000",
            ],
        ),
        (
            Format::Percent,
            "%",
            [
                "-123450 %",
                "-2400 %",
                "-314 %",
                "-100 %",
                "-51 %",
                "-4 %",
                "0 %",
                "1 %",
                "1 %",
                "13 %",
                "33 %",
                "51 %",
                "71 %",
                "100 %",
                "100 %",
                "100 %",
                "250 %",
                "700 %",
                "1200 %",
                "10000 %",
                "10000 %",
                "49950 %",
                "99940 %",
                "99960 %",
                "100000 %",
                "123456 %",
                "500000 %",
                "1234568 %",
                "2000000 %",
            ],
        ),
        (
            Format::SignedPlain,
            "st",
            [
                "-1234.5 st",
                "-24 st",
                "-3.14159 st",
                "-1 st",
                "-0.505 st",
                "-0.04 st",
                "+0 st",
                "+0.005 st",
                "+0.01 st",
                "+0.125 st",
                "+0.33333 st",
                "+0.505 st",
                "+0.707 st",
                "+0.999 st",
                "+1 st",
                "+1.005 st",
                "+2.5 st",
                "+7 st",
                "+12 st",
                "+99.995 st",
                "+100 st",
                "+499.5 st",
                "+999.4 st",
                "+999.6 st",
                "+1000 st",
                "+1234.56 st",
                "+5000 st",
                "+12345.6 st",
                "+20000 st",
            ],
        ),
        (
            Format::Millis,
            "",
            [
                "-1235 ms", "-24 ms", "-3 ms", "-1 ms", "-1 ms", "0 ms", "0 ms", "0 ms", "0 ms", "0 ms", "0 ms",
                "1 ms", "1 ms", "1 ms", "1 ms", "1 ms", "3 ms", "7 ms", "12 ms", "100 ms", "100 ms", "500 ms",
                "999 ms", "1000 ms", "1000 ms", "1235 ms", "5000 ms", "12346 ms", "20000 ms",
            ],
        ),
        (
            Format::Freq,
            "",
            [
                "-1235Hz", "-24Hz", "-3Hz", "-1Hz", "-1Hz", "0Hz", "0Hz", "0Hz", "0Hz", "0Hz", "0Hz", "1Hz", "1Hz",
                "1Hz", "1Hz", "1Hz", "3Hz", "7Hz", "12Hz", "100Hz", "100Hz", "500Hz", "999Hz", "1.0k", "1.0k", "1.2k",
                "5.0k", "12.3k", "20.0k",
            ],
        ),
        (
            Format::Fixed2,
            "",
            [
                "-1234.50", "-24.00", "-3.14", "-1.00", "-0.51", "-0.04", "0.00", "0.01", "0.01", "0.13", "0.33",
                "0.51", "0.71", "1.00", "1.00", "1.00", "2.50", "7.00", "12.00", "100.00", "100.00", "499.50",
                "999.40", "999.60", "1000.00", "1234.56", "5000.00", "12345.68", "20000.00",
            ],
        ),
        (
            Format::Fixed1,
            "dB",
            [
                "-1234.5 dB",
                "-24.0 dB",
                "-3.1 dB",
                "-1.0 dB",
                "-0.5 dB",
                "0.0 dB",
                "0.0 dB",
                "0.0 dB",
                "0.0 dB",
                "0.1 dB",
                "0.3 dB",
                "0.5 dB",
                "0.7 dB",
                "1.0 dB",
                "1.0 dB",
                "1.0 dB",
                "2.5 dB",
                "7.0 dB",
                "12.0 dB",
                "100.0 dB",
                "100.0 dB",
                "499.5 dB",
                "999.4 dB",
                "999.6 dB",
                "1000.0 dB",
                "1234.6 dB",
                "5000.0 dB",
                "12345.7 dB",
                "20000.0 dB",
            ],
        ),
        (
            Format::SignedFixed1,
            "st",
            [
                "-1234.5 st",
                "-24.0 st",
                "-3.1 st",
                "-1.0 st",
                "-0.5 st",
                "0.0 st",
                "+0.0 st",
                "+0.0 st",
                "+0.0 st",
                "+0.1 st",
                "+0.3 st",
                "+0.5 st",
                "+0.7 st",
                "+1.0 st",
                "+1.0 st",
                "+1.0 st",
                "+2.5 st",
                "+7.0 st",
                "+12.0 st",
                "+100.0 st",
                "+100.0 st",
                "+499.5 st",
                "+999.4 st",
                "+999.6 st",
                "+1000.0 st",
                "+1234.6 st",
                "+5000.0 st",
                "+12345.7 st",
                "+20000.0 st",
            ],
        ),
        (
            Format::Density,
            "",
            [
                "Auto", "Auto", "Auto", "Auto", "Auto", "Auto", "0.00", "0.01", "0.01", "0.13", "0.33", "0.51", "0.71",
                "1.00", "1.00", "1.00", "2.50", "7.00", "12.00", "100.00", "100.00", "499.50", "999.40", "999.60",
                "1000.00", "1234.56", "5000.00", "12345.68", "20000.00",
            ],
        ),
    ];

    /// The loop point label: `(steps per beat, step, text)`.
    const LOOP_POINTS: &[(i32, f64, &str)] = &[
        (1, 1.0, "1.1"),
        (1, 2.0, "2.1"),
        (1, 4.0, "4.1"),
        (1, 5.0, "5.1"),
        (1, 9.0, "9.1"),
        (1, 17.0, "17.1"),
        (1, 128.0, "128.1"),
        (3, 1.0, "1.1"),
        (3, 2.0, "1.2"),
        (3, 4.0, "2.1"),
        (3, 5.0, "2.2"),
        (3, 9.0, "3.3"),
        (3, 17.0, "6.2"),
        (3, 128.0, "43.2"),
        (4, 1.0, "1.1"),
        (4, 2.0, "1.2"),
        (4, 4.0, "1.4"),
        (4, 5.0, "2.1"),
        (4, 9.0, "3.1"),
        (4, 17.0, "5.1"),
        (4, 128.0, "32.4"),
        (8, 1.0, "1.1"),
        (8, 2.0, "1.2"),
        (8, 4.0, "1.4"),
        (8, 5.0, "1.5"),
        (8, 9.0, "2.1"),
        (8, 17.0, "3.1"),
        (8, 128.0, "16.8"),
    ];

    /// Whether `value` is on the grid of `step`, or at an end of the range.
    fn on_grid(spec: &Spec, value: f64, step: f64) -> bool {
        let units = value / step;
        (units - units.round()).abs() < 1e-6 || value == spec.min || value == spec.max
    }

    /// Starts and drags swept by the properties.
    fn starts(spec: &Spec) -> [f64; 4] {
        [spec.min, spec.max, (spec.min + spec.max) / 2.0, spec.min + 0.137 * (spec.max - spec.min)]
    }
    const DYS: [f64; 13] = [-250.0, -120.0, -37.0, -5.0, -1.0, 0.0, 1.0, 3.0, 7.0, 50.0, 133.0, 250.0, 400.0];

    #[test]
    fn a_drag_gives_its_value() {
        for &(spec, start, dy, fine, value) in DRAGS {
            assert_eq!(spec.drag(start, dy, fine), value, "{spec:?} from {start} by {dy} (fine: {fine})");
        }
    }

    /// A drag stays in range, on the (fine) step grid, and goes further the
    /// further the pointer goes.
    #[test]
    fn a_drag_is_in_range_on_the_grid_and_monotonic() {
        for spec in SPECS {
            for start in starts(&spec) {
                for fine in [false, true] {
                    let step = if fine { spec.step * 0.1 } else { spec.step };
                    let values = DYS.map(|dy| spec.drag(start, dy, fine));
                    for (dy, v) in DYS.iter().zip(values) {
                        assert!((spec.min..=spec.max).contains(&v), "{spec:?} from {start} by {dy}: {v}");
                        assert!(on_grid(&spec, v, step), "{spec:?} from {start} by {dy}: {v} off the grid");
                    }
                    assert!(values.is_sorted(), "{spec:?} from {start} (fine: {fine}): {values:?}");
                }
            }
        }
    }

    #[test]
    fn a_key_gives_its_value() {
        for &(spec, value, up, fine, coarse, knob, slider) in KEYS {
            let row = format!("{spec:?} {value} up: {up} fine: {fine} coarse: {coarse}");
            assert_eq!(spec.nudge(value, up, fine, coarse), knob, "knob {row}");
            let slider_spec = Spec { style: Style::Slider, ..spec };
            assert_eq!(slider_spec.nudge(value, up, fine, coarse), slider, "slider {row}");
        }
    }

    /// A key moves the value its way, in range, Ctrl at least as far as no
    /// modifier; a knob keeps to its step grid.
    #[test]
    fn a_key_moves_its_way() {
        for spec in SPECS {
            let slider = Spec { style: Style::Slider, ..spec };
            for start in starts(&spec) {
                for up in [true, false] {
                    let way = |v: f64| if up { v >= start } else { v <= start };
                    for s in [spec, slider] {
                        let [plain, fine, coarse] =
                            [(false, false), (true, false), (false, true)].map(|(f, c)| s.nudge(start, up, f, c));
                        for v in [plain, fine, coarse] {
                            assert!(way(v) && (s.min..=s.max).contains(&v), "{s:?} from {start} (up: {up}): {v}");
                        }
                        assert!((coarse - start).abs() >= (plain - start).abs(), "{s:?} from {start} (up: {up})");
                        if s.style == Style::Knob {
                            assert!(on_grid(&s, plain, s.step) && on_grid(&s, coarse, s.step), "{s:?} from {start}");
                        }
                    }
                }
            }
        }
    }

    /// A stepper counts in whole steps: a key (Shift or not) or a typed
    /// number lands on its grid, in range.
    #[test]
    fn a_stepper_keeps_to_whole_steps() {
        let s = Spec::stepper(1.0, 16.0, 1.0, 1.0);
        assert_eq!([false, true].map(|fine| s.nudge(4.0, true, fine, false)), [5.0, 5.0]);
        assert_eq!(s.nudge(1.0, false, false, false), 1.0);
        assert_eq!(s.nudge(10.0, true, false, true), 16.0);
        assert_eq!([3.4, 0.0, 20.0].map(|n| s.typed(n)), [3.0, 1.0, 16.0]);
    }

    #[test]
    fn a_typed_number_gives_its_value() {
        for (spec, (knob, slider)) in SPECS.iter().zip(TYPED_VALUES) {
            let slider_spec = Spec { style: Style::Slider, ..*spec };
            assert_eq!(TYPED.map(|n| spec.typed(n)), knob, "knob {spec:?}");
            assert_eq!(TYPED.map(|n| slider_spec.typed(n)), slider, "slider {spec:?}");
        }
    }

    /// The knob's arc: 0 at the minimum, 1 at the maximum, growing in
    /// between; a logarithmic range is half way at its geometric middle.
    #[test]
    fn the_arc_grows_from_min_to_max() {
        for spec in SPECS {
            assert_eq!((spec.fraction(spec.min), spec.fraction(spec.max)), (0.0, 1.0), "{spec:?}");
            assert_eq!((spec.fraction(spec.min - 1.0), spec.fraction(spec.max + 1.0)), (0.0, 1.0), "{spec:?}");
            let sweep: Vec<f64> =
                (0..=40).map(|i| spec.fraction(spec.min + f64::from(i) / 40.0 * (spec.max - spec.min))).collect();
            assert!(sweep.is_sorted(), "{spec:?}");
        }
        assert!((DECAY.fraction((20.0_f64 * 5000.0).sqrt()) - 0.5).abs() < 1e-12);
        assert!((DECAY.fraction(510.0) - 0.586_561_872_001_496_5).abs() < 1e-12);
        assert_eq!(LINEAR_LOG.fraction(2.5), 0.25);
    }

    #[test]
    fn a_format_gives_its_text() {
        for (format, unit, texts) in FORMATS {
            assert_eq!(SHOWN.map(|v| format.value(v, unit)), *texts, "{format:?}");
        }
        for &(steps_per_beat, step, text) in LOOP_POINTS {
            assert_eq!(Format::LoopLength(steps_per_beat).value(step, ""), text, "{steps_per_beat} {step}");
        }
    }

    #[test]
    fn formats() {
        assert_eq!(Format::Fmt.text(0.505), "0.51");
        assert_eq!(Format::Fmt.text(1.0), "1");
        assert_eq!(Format::Freq.text(999.4), "999Hz");
        assert_eq!(Format::Freq.text(12_345.0), "12.3k");
        assert_eq!(Format::LoopLength(4).text(5.0), "2.1");
        assert_eq!(Format::Density.text(-1.0), "Auto");
        assert_eq!(Format::Millis.value(5000.0, ""), "5000 ms");
        assert_eq!(Format::Plain.value(0.333_333, "x"), "0.333333 x");
        assert_eq!(Format::Plain.value(0.123_456_789, ""), "0.123456");
        assert_eq!(Format::SignedFixed1.text(-0.04), "0.0");
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

    #[test]
    fn log_slider() {
        let s = Spec::slider(10.0, 1000.0, 1.0, 10.0).log();
        assert_eq!(s.slider_at(0.0), 10.0);
        assert_eq!(s.slider_at(0.5), 100.0);
        assert_eq!(s.slider_at(1.0), 1000.0);
        assert!((s.fraction(100.0) - 0.5).abs() < 1e-12);
        assert_eq!(s.slider_fine(100.0, 1.0), 158.5);
    }

    #[test]
    fn chances_and_fractions_read_and_type_as_percentages() {
        assert_eq!(Format::Chance.value(35.0, ""), "35 %");
        assert_eq!(Format::Percent.value(0.57, ""), "57 %");
        assert_eq!(Format::Plain.value(0.57, ""), "0.57");
        let spec = Spec::slider(0.0, 1.0, 0.01, 1.0).percent();
        assert_eq!((spec.shown(0.57), spec.shown_unit()), (57.0, Some("%")));
        assert_eq!(spec.typed(spec.typed_shown(50.0)), 0.5);
        let plain = Spec::slider(0.0, 1.0, 0.01, 1.0);
        assert_eq!((plain.shown(0.57), plain.typed_shown(0.5), plain.shown_unit()), (0.57, 0.5, None));
    }
}
