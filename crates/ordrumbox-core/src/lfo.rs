//! Track LFOs (`computeLfoValue()` in `src/audio/math.js`): an object
//! `{freq, min, max, phase, type}` read leniently (`parseFloat`, defaults),
//! evaluated on the transport tick. The engine samples it once per tick.

use crate::js::parse_float;
use crate::json::{Object, Value};
use crate::steps::TICK;

/// Beats in one LFO cycle at `freq` 1.
const BEATS_PER_CYCLE: f64 = 4.0;
/// Highest frequency, cycles per 4 beats.
const MAX_FREQ: f64 = 2.0;
/// Waveform names, by code.
pub const WAVE_TYPES: [&str; 5] = ["sine", "triangle", "sawtooth", "square", "random"];

/// `toFiniteNumber(parseFloat(v), fallback)`.
fn lenient(value: Option<&Value>, fallback: f64) -> f64 {
    let n = value.map_or(f64::NAN, parse_float);
    if n.is_finite() { n } else { fallback }
}

/// Value of `lfo` at transport tick `tick`, rounded to 2 decimals, between
/// its `min` and `max`.
pub fn value(lfo: &Object, tick: f64) -> f64 {
    let freq = lenient(lfo.get("freq"), 1.0);
    let min = lenient(lfo.get("min"), 0.0);
    let max = lenient(lfo.get("max"), 1.0);
    let phase = lenient(lfo.get("phase"), 0.0);
    let present = |key: &str| lfo.get(key).filter(|v| !v.is_null());
    let wave = match present("type").or_else(|| present("waveform")) {
        None => 0.0,
        Some(Value::String(name)) if WAVE_TYPES.contains(&name.as_str()) => {
            WAVE_TYPES.iter().position(|w| w == name).unwrap_or(0) as f64
        }
        Some(other) => lenient(Some(other), 0.0),
    };
    let current = tick / (f64::from(TICK) * BEATS_PER_CYCLE) * freq.min(MAX_FREQ) + phase;
    let unit = (waveform(current, wave) + 1.0) / 2.0;
    crate::js::round(100.0 * (min + unit * (max - min))) / 100.0
}

/// `getLfoWaveformValue()`: value at `phase` cycles for waveform code `wave`
/// (0 sine, 1 triangle, 2 sawtooth, 3 square, 4 random), in [-1, 1] except
/// the triangle, which the JS formula takes to [-2, 2] (kept as is: it is
/// what the app plays).
pub fn waveform(phase: f64, wave: f64) -> f64 {
    let p = (phase - 0.25) - (phase - 0.25).floor();
    if wave < 0.5 {
        (std::f64::consts::TAU * p).sin()
    } else if wave < 1.5 {
        if p < 0.25 {
            p * 4.0 - 1.0
        } else if p < 0.75 {
            3.0 - p * 4.0
        } else {
            p * 4.0 - 5.0
        }
    } else if wave < 2.5 {
        p * 2.0 - 1.0
    } else if wave < 3.5 {
        if p < 0.5 { 1.0 } else { -1.0 }
    } else {
        // Sample and hold: integer hash of the cycle index, in int32 like JS.
        let cycle = phase.floor() * 1_234_567.0 + 890_123.0;
        let mut rng = if cycle.is_finite() { cycle.trunc().rem_euclid(4_294_967_296.0) as u32 as i32 } else { 0 };
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        f64::from(rng) / 2_147_483_648.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn lfo(text: &str) -> Object {
        json::parse(text).unwrap().as_object().cloned().unwrap()
    }

    #[test]
    fn triangle_is_the_js_one() {
        // The JS "triangle" rises from -2 to 0 over the first half cycle, jumps
        // to 2 and falls back to 0: the value leaves [min, max] by half the range.
        let l = lfo(r#"{"freq":1,"min":0,"max":1,"type":"triangle"}"#);
        assert_eq!(value(&l, 0.0), -0.5);
        assert_eq!(value(&l, 32.0), 0.0);
        assert_eq!(value(&l, 64.0), 1.5);
        assert_eq!(value(&l, 96.0), 1.0);
        assert_eq!(value(&l, 128.0), -0.5);
    }

    #[test]
    fn lenient_fields() {
        let l = lfo(r#"{"freq":"0.5","min":"-1","max":2,"waveform":"square"}"#);
        assert_eq!(value(&l, 0.0), -1.0);
        assert_eq!(value(&l, 128.0), 2.0); // half a cycle at freq 0.5
        assert_eq!(value(&lfo("{}"), 0.0), 0.0);
        assert_eq!(value(&lfo(r#"{"type":"2"}"#), 64.0), 0.25); // sawtooth by code
    }
}
