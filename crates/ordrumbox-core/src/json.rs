//! JSON reader and writer (RFC 8259), with no dependency.
//!
//! Output is byte-compatible with the files of the web version, so a file
//! written here and one written by `JSON.stringify` are identical:
//! - numbers use the ECMAScript `Number#toString` format (`1e+21`, `1e-7`,
//!   `0.000001`, `-0` written `0`, non-finite written `null`);
//! - strings are escaped like `JSON.stringify` (short escapes, `\u00xx` for
//!   other control characters, everything else raw UTF-8);
//! - object keys keep ECMAScript property order: integer-like keys (`"0"`, `"12"`) first
//!   in ascending order, then the other keys in insertion order;
//! - `to_json_pretty(n)` matches `JSON.stringify(value, null, n)`.
//!
//! Reading follows `JSON.parse`, with two lenient cases: a leading UTF-8 BOM is
//! skipped (files saved by Windows editors), and a lone UTF-16 surrogate in a
//! `\u` escape becomes U+FFFD (a Rust `String` cannot hold it).

use std::fmt;

use write::write_value;

pub use parse::{ParseError, parse};

mod parse;
mod write;

/// A JSON value.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(Object),
}

/// A JSON object, keeping ECMAScript property order (see the module docs).
///
/// Lookups are linear: the objects of this app have a handful of keys, where a
/// scan beats hashing and keeps the order for free.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Object {
    entries: Vec<(String, Value)>,
}

impl Object {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.entries.iter_mut().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Set `key`. An existing key keeps its position and its previous
    /// value is returned; a new one goes where ECMAScript property order puts it.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        let key = key.into();
        if let Some(slot) = self.get_mut(&key) {
            return Some(std::mem::replace(slot, value));
        }
        let at = match array_index(&key) {
            Some(index) => self
                .entries
                .iter()
                .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                .unwrap_or(self.entries.len()),
            None => self.entries.len(),
        };
        self.entries.insert(at, (key, value));
        None
    }

    /// Remove `key`, keeping the order of the other keys.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let at = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(at).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }
}

impl FromIterator<(String, Value)> for Object {
    fn from_iter<I: IntoIterator<Item = (String, Value)>>(iter: I) -> Self {
        let mut object = Self::new();
        for (key, value) in iter {
            object.insert(key, value);
        }
        object
    }
}

/// The value of an ECMAScript array-index key (canonical decimal, 0..2^32-2),
/// which ECMAScript orders before every other key.
fn array_index(key: &str) -> Option<u32> {
    let bytes = key.as_bytes();
    let canonical = match bytes {
        [b'0'] => true,
        [b'1'..=b'9', rest @ ..] => rest.iter().all(u8::is_ascii_digit),
        _ => false,
    };
    if !canonical {
        return None;
    }
    key.parse::<u32>().ok().filter(|&index| index != u32::MAX)
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// A flag is on only when it is `true`.
    pub fn is_true(&self) -> bool {
        matches!(self, Self::Bool(true))
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }

    /// The number a file means here: a number, or a string holding a finite
    /// one once trimmed (`"140"`). `None` for anything else.
    pub fn to_number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            Self::String(s) => s.trim().parse::<f64>().ok().filter(|n| n.is_finite()),
            _ => None,
        }
    }

    /// The text a file means here: a string, or a number or boolean written
    /// out (`140`, `true`). `None` for null, arrays and objects.
    pub fn to_text(&self) -> Option<String> {
        match self {
            Self::String(s) => Some(s.clone()),
            Self::Number(n) => Some(n.to_string()),
            Self::Bool(b) => Some(b.to_string()),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_array_mut(&mut self) -> Option<&mut Vec<Value>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Object> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    pub fn as_object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    /// `self[key]` when `self` is an object.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.as_object()?.get(key)
    }

    /// Compact text, as `JSON.stringify(value)`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, 0, 0);
        out
    }

    /// Indented text, as `JSON.stringify(value, null, indent)`.
    pub fn to_json_pretty(&self, indent: usize) -> String {
        let mut out = String::new();
        write_value(&mut out, self, indent, 0);
        out
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_json())
    }
}

impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Self::Number(f64::from(n))
    }
}

impl From<u32> for Value {
    fn from(n: u32) -> Self {
        Self::Number(f64::from(n))
    }
}

impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<Vec<Value>> for Value {
    fn from(items: Vec<Value>) -> Self {
        Self::Array(items)
    }
}

impl From<Object> for Value {
    fn from(object: Object) -> Self {
        Self::Object(object)
    }
}

#[cfg(test)]
mod tests {
    use super::parse::MAX_DEPTH;
    use super::*;

    fn num(n: f64) -> String {
        Value::Number(n).to_json()
    }

    #[test]
    fn numbers_match_js_to_string() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (-0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (0.1, "0.1"),
            (0.1 + 0.2, "0.30000000000000004"),
            (100.0, "100"),
            (123.456, "123.456"),
            (0.000001, "0.000001"),
            (1e-7, "1e-7"),
            (1.23e-18, "1.23e-18"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (9007199254740992.0, "9007199254740992"),
            (f64::MAX, "1.7976931348623157e+308"),
            (5e-324, "5e-324"),
            (f64::NAN, "null"),
            (f64::INFINITY, "null"),
        ];
        for &(n, expected) in cases {
            assert_eq!(num(n), expected, "for {n:e}");
        }
    }

    #[test]
    fn parses_scalars() {
        assert_eq!(parse("null").unwrap(), Value::Null);
        assert_eq!(parse(" true ").unwrap(), Value::Bool(true));
        assert_eq!(parse("false").unwrap(), Value::Bool(false));
        assert_eq!(parse("-12.5e1").unwrap(), Value::Number(-125.0));
        assert_eq!(parse("0").unwrap(), Value::Number(0.0));
        assert_eq!(parse("1e400").unwrap(), Value::Number(f64::INFINITY));
        assert_eq!(parse("\"a\\u00e9\\n\\/\"").unwrap(), Value::from("aé\n/"));
    }

    #[test]
    fn parses_surrogates() {
        assert_eq!(parse(r#""\ud83e\udd41""#).unwrap(), Value::from("🥁"));
        assert_eq!(parse(r#""\ud83e""#).unwrap(), Value::from("\u{fffd}"));
        assert_eq!(parse(r#""\ud83e\u0041""#).unwrap(), Value::from("\u{fffd}A"));
    }

    #[test]
    fn rejects_invalid_text() {
        for bad in [
            "",
            "01",
            "1.",
            "1e",
            "-",
            "+1",
            ".5",
            "[1,]",
            "{\"a\":1,}",
            "{a:1}",
            "[1 2]",
            "\"\t\"",
            "\"\\x\"",
            "tru",
            "nul",
            "[",
            "{\"a\"}",
            "1 2",
            "'a'",
            "NaN",
        ] {
            assert!(parse(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn locates_errors() {
        let err = parse("{\n  \"a\": 1,\n  \"b\": x\n}").unwrap_err();
        assert_eq!((err.line, err.column), (3, 8));
        assert_eq!(err.to_string(), "unexpected character at line 3, column 8");
    }

    #[test]
    fn limits_depth() {
        let deep = "[".repeat(MAX_DEPTH + 1) + &"]".repeat(MAX_DEPTH + 1);
        assert_eq!(parse(&deep).unwrap_err().message, "nesting too deep");
        let ok = "[".repeat(MAX_DEPTH) + &"]".repeat(MAX_DEPTH);
        assert!(parse(&ok).is_ok());
    }

    #[test]
    fn skips_bom() {
        assert_eq!(parse("\u{feff}[1]").unwrap(), Value::Array(vec![Value::from(1)]));
    }

    #[test]
    fn keeps_js_key_order() {
        let v = parse(r#"{"b":1,"2":2,"a":3,"0":4,"b":5,"10":6,"01":7}"#).unwrap();
        assert_eq!(v.to_json(), r#"{"0":4,"2":2,"10":6,"b":5,"a":3,"01":7}"#);
    }

    #[test]
    fn object_insert_and_remove() {
        let mut o = Object::new();
        assert_eq!(o.insert("x", Value::from(1)), None);
        o.insert("y", Value::from(2));
        assert_eq!(o.insert("x", Value::from(3)), Some(Value::from(1)));
        assert_eq!(Value::from(o.clone()).to_json(), r#"{"x":3,"y":2}"#);
        assert_eq!(o.remove("x"), Some(Value::from(3)));
        assert_eq!(o.remove("x"), None);
        assert_eq!(o.len(), 1);
        assert!(o.contains_key("y"));
    }

    #[test]
    fn escapes_like_json_stringify() {
        let s = Value::from("q\"b\\\u{8}\u{c}\n\r\t\u{1}\u{1f}é/🥁");
        assert_eq!(s.to_json(), r#""q\"b\\\b\f\n\r\t\u0001\u001fé/🥁""#);
    }

    #[test]
    fn pretty_matches_json_stringify() {
        let v = parse(r#"{"a":[1,[],{}],"b":{"c":null},"d":[]}"#).unwrap();
        let expected =
            "{\n  \"a\": [\n    1,\n    [],\n    {}\n  ],\n  \"b\": {\n    \"c\": null\n  },\n  \"d\": []\n}";
        assert_eq!(v.to_json_pretty(2), expected);
        assert_eq!(v.to_json_pretty(0), v.to_json());
    }

    #[test]
    fn accessors() {
        let v = parse(r#"{"n":2,"s":"x","a":[true],"o":{}}"#).unwrap();
        assert_eq!(v.get("n").and_then(Value::as_f64), Some(2.0));
        assert_eq!(v.get("s").and_then(Value::as_str), Some("x"));
        assert_eq!(v.get("a").and_then(Value::as_array).map(<[Value]>::len), Some(1));
        assert!(v.get("o").and_then(Value::as_object).is_some_and(Object::is_empty));
        assert!(v.get("missing").is_none());
        assert!(Value::Null.get("x").is_none());
    }
}
