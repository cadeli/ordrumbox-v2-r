//! The JSON writer: compact or indented text of a [`Value`], byte for byte
//! as `JSON.stringify` writes it (numbers, escapes, key order: `json.rs`).

use super::Value;

pub(super) fn write_value(out: &mut String, value: &Value, indent: usize, level: usize) {
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
fn write_number(out: &mut String, n: f64) {
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
    // `{:e}` gives the shortest round-trip digits: "1.2345e2".
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
