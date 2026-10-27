//! JSON reader and writer (RFC 8259), with no dependency.
//!
//! Output is byte-compatible with the JS app, so a file written here and one
//! written by `JSON.stringify` are identical:
//! - numbers use the ECMAScript `Number#toString` format (`1e+21`, `1e-7`,
//!   `0.000001`, `-0` written `0`, non-finite written `null`);
//! - strings are escaped like `JSON.stringify` (short escapes, `\u00xx` for
//!   other control characters, everything else raw UTF-8);
//! - object keys keep JS property order: integer-like keys (`"0"`, `"12"`) first
//!   in ascending order, then the other keys in insertion order;
//! - `to_json_pretty(n)` matches `JSON.stringify(value, null, n)`.
//!
//! Reading follows `JSON.parse`, with two lenient cases: a leading UTF-8 BOM is
//! skipped (files saved by Windows editors), and a lone UTF-16 surrogate in a
//! `\u` escape becomes U+FFFD (a Rust `String` cannot hold it).

use std::fmt;

/// Nesting limit of the reader, so a hostile file cannot overflow the stack.
const MAX_DEPTH: usize = 512;

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

/// A JSON object, keeping JS property order (see the module docs).
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

    /// Set `key`. An existing key keeps its position (like a JS assignment) and
    /// its previous value is returned; a new one goes where JS would put it.
    pub fn insert(&mut self, key: impl Into<String>, value: Value) -> Option<Value> {
        let key = key.into();
        if let Some(slot) = self.get_mut(&key) {
            return Some(std::mem::replace(slot, value));
        }
        let at = match array_index(&key) {
            Some(idx) => self
                .entries
                .iter()
                .position(|(k, _)| array_index(k).is_none_or(|other| other > idx))
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
/// which JS orders before every other key.
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
    key.parse::<u32>().ok().filter(|&idx| idx != u32::MAX)
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

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
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

// ── Reader ──────────────────────────────────────────────────────────────

/// A syntax error, located for the user (1-based line and column).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub message: &'static str,
    /// Byte offset in the input.
    pub offset: usize,
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at line {}, column {}", self.message, self.line, self.column)
    }
}

impl std::error::Error for ParseError {}

/// Parse a whole JSON text.
pub fn parse(text: &str) -> Result<Value, ParseError> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parser = Parser { text, bytes: text.as_bytes(), pos: 0, depth: 0 };
    parser.skip_whitespace();
    let value = parser.parse_value()?;
    parser.skip_whitespace();
    if parser.pos < parser.bytes.len() {
        return Err(parser.error("unexpected characters after the JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn error(&self, message: &'static str) -> ParseError {
        let before = &self.bytes[..self.pos.min(self.bytes.len())];
        let line = before.iter().filter(|&&b| b == b'\n').count() + 1;
        let line_start = before.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        let column = self.text[line_start..self.pos.min(self.text.len())].chars().count() + 1;
        ParseError { message, offset: self.pos, line, column }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn skip_whitespace(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.pos += 1;
        }
    }

    fn expect_literal(&mut self, literal: &str, value: Value) -> Result<Value, ParseError> {
        if self.bytes[self.pos..].starts_with(literal.as_bytes()) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(self.error("invalid literal"))
        }
    }

    fn parse_value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b't') => self.expect_literal("true", Value::Bool(true)),
            Some(b'f') => self.expect_literal("false", Value::Bool(false)),
            Some(b'n') => self.expect_literal("null", Value::Null),
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            Some(_) => Err(self.error("unexpected character")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn enter(&mut self) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.error("nesting too deep"));
        }
        self.pos += 1; // the opening bracket
        self.skip_whitespace();
        Ok(())
    }

    /// After an item: `,` continues (true), `close` ends (false).
    fn next_item(&mut self, close: u8) -> Result<bool, ParseError> {
        self.skip_whitespace();
        match self.peek() {
            Some(b',') => {
                self.pos += 1;
                self.skip_whitespace();
                Ok(true)
            }
            Some(b) if b == close => {
                self.pos += 1;
                self.depth -= 1;
                Ok(false)
            }
            Some(_) => Err(self.error(if close == b']' { "expected ',' or ']'" } else { "expected ',' or '}'" })),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn parse_array(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        let mut items = Vec::new();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Array(items));
        }
        loop {
            items.push(self.parse_value()?);
            if !self.next_item(b']')? {
                return Ok(Value::Array(items));
            }
        }
    }

    fn parse_object(&mut self) -> Result<Value, ParseError> {
        self.enter()?;
        let mut object = Object::new();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Value::Object(object));
        }
        loop {
            if self.peek() != Some(b'"') {
                return Err(self.error("expected a string key"));
            }
            let key = self.parse_string()?;
            self.skip_whitespace();
            if self.peek() != Some(b':') {
                return Err(self.error("expected ':'"));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.parse_value()?;
            // Duplicate keys: the last one wins, at the first one's position (JSON.parse).
            object.insert(key, value);
            if !self.next_item(b'}')? {
                return Ok(Value::Object(object));
            }
        }
    }

    fn eat_digits(&mut self) -> usize {
        let start = self.pos;
        while let Some(b'0'..=b'9') = self.peek() {
            self.pos += 1;
        }
        self.pos - start
    }

    fn parse_number(&mut self) -> Result<Value, ParseError> {
        let start = self.pos;
        if self.peek() == Some(b'-') {
            self.pos += 1;
        }
        match self.peek() {
            Some(b'0') => {
                self.pos += 1;
                if let Some(b'0'..=b'9') = self.peek() {
                    return Err(self.error("leading zero in a number"));
                }
            }
            Some(b'1'..=b'9') => {
                self.eat_digits();
            }
            _ => return Err(self.error("invalid number")),
        }
        if self.peek() == Some(b'.') {
            self.pos += 1;
            if self.eat_digits() == 0 {
                return Err(self.error("digit expected after '.'"));
            }
        }
        if let Some(b'e' | b'E') = self.peek() {
            self.pos += 1;
            if let Some(b'+' | b'-') = self.peek() {
                self.pos += 1;
            }
            if self.eat_digits() == 0 {
                return Err(self.error("digit expected in exponent"));
            }
        }
        // The grammar is checked above; Rust's parser rounds like JS (and
        // overflows to infinity like JS).
        self.text[start..self.pos].parse::<f64>().map(Value::Number).map_err(|_| self.error("invalid number"))
    }

    fn parse_hex4(&mut self) -> Result<u32, ParseError> {
        let digits = self.bytes.get(self.pos..self.pos + 4).ok_or_else(|| self.error("truncated \\u escape"))?;
        let mut code = 0;
        for &b in digits {
            let nibble = (b as char).to_digit(16).ok_or_else(|| self.error("invalid \\u escape"))?;
            code = code * 16 + nibble;
        }
        self.pos += 4;
        Ok(code)
    }

    fn parse_string(&mut self) -> Result<String, ParseError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            // Copy the run of plain characters in one go (always ends on an
            // ASCII byte, so the slice is on a char boundary).
            let run_start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.text[run_start..self.pos]);

            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let escape = self.peek().ok_or_else(|| self.error("unexpected end of input"))?;
                    self.pos += 1;
                    match escape {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => out.push(self.parse_unicode_escape()?),
                        _ => {
                            self.pos -= 1;
                            return Err(self.error("invalid escape"));
                        }
                    }
                }
                Some(_) => return Err(self.error("control character in a string")),
                None => return Err(self.error("unterminated string")),
            }
        }
    }

    /// The character of a `\uXXXX` escape (the `\u` already consumed),
    /// joining a surrogate pair when one follows.
    fn parse_unicode_escape(&mut self) -> Result<char, ParseError> {
        let first = self.parse_hex4()?;
        if (0xD800..0xDC00).contains(&first) && self.bytes[self.pos..].starts_with(b"\\u") {
            let save = self.pos;
            self.pos += 2;
            let second = self.parse_hex4()?;
            if (0xDC00..0xE000).contains(&second) {
                let code = 0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00);
                return Ok(char::from_u32(code).unwrap_or(char::REPLACEMENT_CHARACTER));
            }
            self.pos = save; // not a pair: the next escape stands on its own
        }
        Ok(char::from_u32(first).unwrap_or(char::REPLACEMENT_CHARACTER))
    }
}

// ── Writer ──────────────────────────────────────────────────────────────

fn write_value(out: &mut String, value: &Value, indent: usize, level: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, *n),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            write_container(out, b'[', b']', items.iter(), indent, level, |out, item| {
                write_value(out, item, indent, level + 1);
            });
        }
        Value::Object(object) => {
            let separator = if indent > 0 { ": " } else { ":" };
            write_container(out, b'{', b'}', object.iter(), indent, level, |out, (key, item)| {
                write_string(out, key);
                out.push_str(separator);
                write_value(out, item, indent, level + 1);
            });
        }
    }
}

fn write_container<T>(
    out: &mut String,
    open: u8,
    close: u8,
    items: impl Iterator<Item = T>,
    indent: usize,
    level: usize,
    mut write_item: impl FnMut(&mut String, T),
) {
    out.push(char::from(open));
    let mut empty = true;
    for item in items {
        if !empty {
            out.push(',');
        }
        empty = false;
        if indent > 0 {
            out.push('\n');
            push_spaces(out, indent * (level + 1));
        }
        write_item(out, item);
    }
    // JSON.stringify writes an empty container as "[]"/"{}" even when indenting.
    if !empty && indent > 0 {
        out.push('\n');
        push_spaces(out, indent * level);
    }
    out.push(char::from(close));
}

fn push_spaces(out: &mut String, count: usize) {
    out.extend(std::iter::repeat_n(' ', count));
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", u32::from(c)));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// ECMAScript `Number#toString(10)`, as `JSON.stringify` writes numbers
/// (non-finite numbers are written `null`).
pub(crate) fn write_number(out: &mut String, n: f64) {
    if !n.is_finite() {
        out.push_str("null");
        return;
    }
    if n == 0.0 {
        out.push('0'); // also -0
        return;
    }
    if n < 0.0 {
        out.push('-');
    }
    // `{:e}` gives the shortest round-trip digits, like JS: "1.2345e2".
    let sci = format!("{:e}", n.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|&c| c != '.').collect();
    let k = digits.len() as i32;
    let point = exp.parse::<i32>().unwrap_or(0) + 1; // decimal point position (ES `n`)

    if k <= point && point <= 21 {
        out.push_str(&digits);
        push_zeros(out, point - k);
    } else if 0 < point && point <= 21 {
        let (int_part, frac_part) = digits.split_at(point as usize);
        out.push_str(int_part);
        out.push('.');
        out.push_str(frac_part);
    } else if -6 < point && point <= 0 {
        out.push_str("0.");
        push_zeros(out, -point);
        out.push_str(&digits);
    } else {
        let (first, rest) = digits.split_at(1);
        out.push_str(first);
        if !rest.is_empty() {
            out.push('.');
            out.push_str(rest);
        }
        let e = point - 1;
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.abs().to_string());
    }
}

fn push_zeros(out: &mut String, count: i32) {
    out.extend(std::iter::repeat_n('0', count.max(0) as usize));
}

#[cfg(test)]
mod tests {
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
