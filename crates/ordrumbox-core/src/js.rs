//! The JS coercions the file loaders rely on: `Number(x)` and `String(x)`.
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
