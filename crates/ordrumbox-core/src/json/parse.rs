//! The JSON reader: a [`Value`] from text, as `JSON.parse` reads it (the
//! lenient cases: `json.rs`), or a [`ParseError`] located for the user.

use std::fmt;

use super::{Object, Value};

/// Nesting limit of the reader, so a hostile file cannot overflow the stack.
pub(super) const MAX_DEPTH: usize = 512;

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
        // The grammar is checked above; Rust's parser rounds to nearest (and
        // overflows to infinity).
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
