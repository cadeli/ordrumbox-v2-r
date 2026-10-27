//! The JS coercions the file loaders rely on: `Number(x)`, `String(x)`,
//! `parseFloat(x)` and truthiness.
//!
//! `song_schema.js` reads song files with them (`Number(raw.bpm)`,
//! `String(raw.id ?? '')`), so `"bpm": "140"` is a valid tempo there. They are
//! reproduced here so a file loads the same way in both apps.

use crate::json::{self, Value};

/// `Number(value)`. Objects (and arrays of more than one item) give NaN.
pub fn to_number(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(b) => f64::from(u8::from(*b)),
        Value::Number(n) => *n,
        Value::String(s) => string_to_number(s),
        Value::Array(_) => string_to_number(&to_string(value)),
        Value::Object(_) => f64::NAN,
    }
}

/// `String(value)`.
pub fn to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => number_to_string(*n),
        Value::String(s) => s.clone(),
        // Array#join: null items become empty strings.
        Value::Array(items) => items
            .iter()
            .map(|item| if item.is_null() { String::new() } else { to_string(item) })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// `Math.round(x)`: ties go up (`-2.5` → `-2`), unlike `f64::round` which goes
/// away from zero. `x - floor(x)` is exact, so there is no `x + 0.5` error.
pub fn round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

pub use crate::timsort::sort_by;

/// `Number(x.toFixed(digits))`: rounded on the exact decimal value of `x`,
/// halves away from zero (`(1.005).toFixed(2)` is `"1.00"`: 1.005 is stored
/// as 1.00499…). Unlike `Math.round(x * 100) / 100`, there is no
/// multiplication error.
pub fn to_fixed(x: f64, digits: usize) -> f64 {
    if !x.is_finite() || x.abs() >= 1e21 {
        return x;
    }
    // A double has at most 1074 fraction digits: 1100 print it exactly.
    let exact = format!("{:.1100}", x.abs());
    let (int_part, fraction) = exact.split_once('.').unwrap_or((&exact, ""));
    let mut kept: Vec<u8> = int_part.bytes().chain(fraction.bytes().take(digits)).collect();
    if fraction.as_bytes().get(digits).is_some_and(|&d| d >= b'5') {
        // Carry the round-up through the kept digits.
        let mut i = kept.len();
        loop {
            if i == 0 {
                kept.insert(0, b'1');
                break;
            }
            i -= 1;
            if kept[i] == b'9' {
                kept[i] = b'0';
            } else {
                kept[i] += 1;
                break;
            }
        }
    }
    let split = kept.len() - digits;
    let text = format!("{}.{}", String::from_utf8_lossy(&kept[..split]), String::from_utf8_lossy(&kept[split..]));
    let value: f64 = text.parse().unwrap_or(f64::NAN);
    if x < 0.0 { -value } else { value }
}

/// `String(n)`.
pub fn number_to_string(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_owned();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_owned();
    }
    let mut out = String::new();
    json::write_number(&mut out, n);
    out
}

/// ECMAScript StringToNumber: trimmed decimal, `Infinity`, or a `0x`/`0o`/`0b`
/// integer; the empty string is 0, anything else NaN.
fn string_to_number(s: &str) -> f64 {
    let s = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if s.is_empty() {
        return 0.0;
    }
    let radix = match s.get(..2) {
        Some("0x" | "0X") => 16,
        Some("0o" | "0O") => 8,
        Some("0b" | "0B") => 2,
        _ => 10,
    };
    if radix != 10 {
        let digits = &s[2..];
        if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
            return f64::NAN;
        }
        return digits.chars().fold(0.0, |acc, c| acc * f64::from(radix) + f64::from(c.to_digit(radix).unwrap_or(0)));
    }
    let (sign, unsigned) = match s.as_bytes()[0] {
        b'-' => (-1.0, &s[1..]),
        b'+' => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    if unsigned == "Infinity" {
        return sign * f64::INFINITY;
    }
    if is_decimal_literal(unsigned) { unsigned.parse::<f64>().map_or(f64::NAN, |n| sign * n) } else { f64::NAN }
}

/// `digits [. digits] [e[+-]digits]` or `. digits [e…]`, without sign. Rust's
/// parser alone would also take `inf` and `nan`, which JS rejects.
fn is_decimal_literal(s: &str) -> bool {
    let (mantissa, exponent) = match s.find(['e', 'E']) {
        Some(i) => (&s[..i], Some(&s[i + 1..])),
        None => (s, None),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all_digits = |p: &str| p.bytes().all(|b| b.is_ascii_digit());
    let mantissa_ok = all_digits(int_part) && all_digits(frac_part) && !(int_part.is_empty() && frac_part.is_empty());
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && all_digits(e)
    });
    mantissa_ok && exponent_ok
}

/// `parseFloat(value)`: the longest decimal prefix of `String(value)` after
/// leading white space (`"0.5 beats"` is 0.5), NaN when there is none.
pub fn parse_float(value: &Value) -> f64 {
    if let Value::Number(n) = value {
        return *n;
    }
    let text = to_string(value);
    let s = text.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (sign, rest) = match s.as_bytes().first() {
        Some(b'-') => (-1.0, &s[1..]),
        Some(b'+') => (1.0, &s[1..]),
        _ => (1.0, s),
    };
    if rest.starts_with("Infinity") {
        return sign * f64::INFINITY;
    }
    let bytes = rest.as_bytes();
    let digits_from = |mut i: usize| {
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        i
    };
    let int_end = digits_from(0);
    let mut end = int_end;
    if bytes.get(end) == Some(&b'.') {
        let frac_end = digits_from(end + 1);
        if int_end > 0 || frac_end > end + 1 {
            end = frac_end;
        }
    }
    if end == 0 {
        return f64::NAN;
    }
    if matches!(bytes.get(end), Some(b'e' | b'E')) {
        let sign_len = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
        let exp_end = digits_from(end + 1 + sign_len);
        if exp_end > end + 1 + sign_len {
            end = exp_end;
        }
    }
    rest[..end].parse::<f64>().map_or(f64::NAN, |n| sign * n)
}

/// JS truthiness (`!!value`).
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(text: &str) -> f64 {
        to_number(&json::parse(text).unwrap())
    }

    #[test]
    fn number_coercion_matches_js() {
        assert_eq!(num("null"), 0.0);
        assert_eq!(num("true"), 1.0);
        assert_eq!(num("\"140\""), 140.0);
        assert_eq!(num("\"  2.5  \""), 2.5);
        assert_eq!(num("\"\""), 0.0);
        assert_eq!(num("\"1.\""), 1.0);
        assert_eq!(num("\".5\""), 0.5);
        assert_eq!(num("\"-1e3\""), -1000.0);
        assert_eq!(num("\"0x1A\""), 26.0);
        assert_eq!(num("\"0b101\""), 5.0);
        assert_eq!(num("\"-Infinity\""), f64::NEG_INFINITY);
        assert_eq!(num("[]"), 0.0);
        assert_eq!(num("[\"7\"]"), 7.0);
        for nan in ["\"abc\"", "\"1,2\"", "\"inf\"", "\"NaN\"", "\"-0x1\"", "\"1e\"", "\".\"", "[1,2]", "{}"] {
            assert!(num(nan).is_nan(), "{nan}");
        }
    }

    #[test]
    fn parse_float_matches_js() {
        let pf = |text: &str| parse_float(&json::parse(text).unwrap());
        assert_eq!(pf("2.5"), 2.5);
        assert_eq!(pf("\"0.5 beats\""), 0.5);
        assert_eq!(pf("\"  -3e2x\""), -300.0);
        assert_eq!(pf("\"1e\""), 1.0);
        assert_eq!(pf("\".5\""), 0.5);
        assert_eq!(pf("\"5.\""), 5.0);
        assert_eq!(pf("\"-Infinityx\""), f64::NEG_INFINITY);
        assert_eq!(pf("[\"7\", 2]"), 7.0);
        for nan in ["\"\"", "\".\"", "\"abc\"", "null", "true", "{}"] {
            assert!(pf(nan).is_nan(), "{nan}");
        }
        assert_eq!(pf("\"0x10\""), 0.0);
        assert_eq!(pf("\"0x\""), 0.0);
    }

    #[test]
    fn to_fixed_matches_js() {
        assert_eq!(to_fixed(1.005, 2), 1.0); // stored as 1.00499…
        assert_eq!(to_fixed(0.125, 2), 0.13); // exact tie: up
        assert_eq!(to_fixed(-0.125, 2), -0.13); // away from zero
        assert_eq!(to_fixed(0.995, 2), 0.99); // stored as 0.99499…
        assert_eq!(to_fixed(9.999, 2), 10.0);
        assert_eq!(to_fixed(0.7250000000000001, 2), 0.73);
        assert_eq!(to_fixed(0.1 + 0.2, 2), 0.3);
        assert_eq!(to_fixed(42.0, 0), 42.0);
        assert_eq!(to_fixed(2.5, 0), 3.0);
        assert!(to_fixed(f64::NAN, 2).is_nan());
    }

    #[test]
    fn truthiness() {
        for (text, expected) in
            [("0", false), ("1", true), ("\"\"", false), ("\"0\"", true), ("null", false), ("[]", true)]
        {
            assert_eq!(truthy(&json::parse(text).unwrap()), expected, "{text}");
        }
    }

    #[test]
    fn string_coercion_matches_js() {
        assert_eq!(to_string(&Value::from(140)), "140");
        assert_eq!(to_string(&Value::Number(1e21)), "1e+21");
        assert_eq!(to_string(&Value::Bool(false)), "false");
        assert_eq!(to_string(&json::parse("[1,null,\"a\"]").unwrap()), "1,,a");
        assert_eq!(to_string(&json::parse("{}").unwrap()), "[object Object]");
        assert_eq!(number_to_string(f64::NAN), "NaN");
        assert_eq!(number_to_string(-f64::INFINITY), "-Infinity");
    }
}
