//! Typed records mapped onto JSON objects, declared once as a field table.
//!
//! `json_record!` turns a table of `field: Type = default => "jsonKey"` rows
//! into the struct, its `Default`, the ordered key list and a by-key setter, so
//! a key, its type and its default are written in one place only (the JS
//! equivalents are `NOTE_DEFAULTS` / `NOTE_KEY_ORDER` / `TRACK_DEFAULTS`).
//!
//! A row may add the keys older files (and the JS app) use for the field:
//! `=> "newKey" | "oldKey"`. Both are read; only the first is written.

use crate::json::{Object, Value};

/// A Rust type that one JSON field maps to.
pub trait JsonField: Sized {
    /// The value, or `None` when the JSON value has the wrong type.
    fn from_json(value: &Value) -> Option<Self>;
    fn to_json(&self) -> Value;
}

impl JsonField for bool {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_bool()
    }
    fn to_json(&self) -> Value {
        Value::Bool(*self)
    }
}

impl JsonField for f64 {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_f64().filter(|n| n.is_finite())
    }
    fn to_json(&self) -> Value {
        Value::Number(*self)
    }
}

impl JsonField for i32 {
    /// Integral numbers only: a grid position or a count of 2.5 is a broken file.
    fn from_json(value: &Value) -> Option<Self> {
        let n = value.as_f64()?;
        (n.fract() == 0.0 && n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX)).then_some(n as i32)
    }
    fn to_json(&self) -> Value {
        Value::Number(f64::from(*self))
    }
}

impl JsonField for String {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_str().map(str::to_owned)
    }
    fn to_json(&self) -> Value {
        Value::String(self.clone())
    }
}

/// Kept verbatim (shapes the model does not interpret yet, like `arp`).
impl JsonField for Value {
    fn from_json(value: &Value) -> Option<Self> {
        Some(value.clone())
    }
    fn to_json(&self) -> Value {
        self.clone()
    }
}

impl JsonField for Object {
    fn from_json(value: &Value) -> Option<Self> {
        value.as_object().cloned()
    }
    fn to_json(&self) -> Value {
        Value::Object(self.clone())
    }
}

/// `null` is `None`; anything else must be a valid `T`.
impl<T: JsonField> JsonField for Option<T> {
    fn from_json(value: &Value) -> Option<Self> {
        if value.is_null() { Some(None) } else { T::from_json(value).map(Some) }
    }
    fn to_json(&self) -> Value {
        self.as_ref().map_or(Value::Null, T::to_json)
    }
}

/// Outcome of setting a record field by its JSON key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetField {
    Set,
    /// The key is a field, but the value has the wrong type: the field keeps its value.
    WrongType,
    /// The key is not a field of this record.
    Unknown,
}

/// A problem met while loading a file. The load goes on: the faulty value is
/// replaced by its default (or the faulty item skipped) and reported here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    /// Where, as a JSON path: `patterns[3].tracks[1].notes[0].velocity`.
    pub at: String,
    pub message: String,
}

impl Issue {
    pub(crate) fn new(at: impl Into<String>, message: impl Into<String>) -> Self {
        Self { at: at.into(), message: message.into() }
    }
}

/// Report `key` when an earlier key of the same object named the same
/// `field` (a web-version name and its current one): the later value is used.
pub(crate) fn check_duplicate(
    seen: &mut Vec<&'static str>,
    field: Option<&'static str>,
    key: &str,
    at: &str,
    issues: &mut Vec<Issue>,
) {
    let Some(field) = field else { return };
    if seen.contains(&field) {
        issues.push(Issue::new(
            format!("{at}.{key}"),
            format!("same setting as an earlier key ({field}), this value used"),
        ));
    } else {
        seen.push(field);
    }
}

/// Round to 2 decimals like the JS exporter: `Math.round(v * 100) / 100`.
pub(crate) fn round2(value: f64) -> f64 {
    crate::js::round(value * 100.0) / 100.0
}

macro_rules! json_record {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty = $default:expr => $key:literal $(| $web:literal)*, )*
        }
        other_fields {
            $( $(#[$ometa:meta])* $ofield:ident : $oty:ty = $odefault:expr, )*
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
            $( $(#[$ometa])* pub $ofield: $oty, )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self { $( $field: $default, )* $( $ofield: $odefault, )* }
            }
        }

        impl $name {
            /// JSON keys of the table fields, in declaration order.
            pub const KEYS: &'static [&'static str] = &[$( $key ),*];

            /// The key a field is written under, for its key or one of its
            /// web-version keys; `None` for a key that names no field.
            pub fn canonical_key(key: &str) -> Option<&'static str> {
                match key {
                    $( $key $(| $web)* => Some($key), )*
                    _ => None,
                }
            }

            /// Set the field named `key`. `null` resets it to its default
            /// (what `?? default` does on the JS side).
            pub fn set_field(&mut self, key: &str, value: &$crate::json::Value) -> $crate::record::SetField {
                use $crate::record::{JsonField, SetField};
                match key {
                    $( $key $(| $web)* => match <$ty as JsonField>::from_json(value) {
                        Some(v) => { self.$field = v; SetField::Set }
                        None if value.is_null() => { self.$field = $default; SetField::Set }
                        None => SetField::WrongType,
                    }, )*
                    _ => SetField::Unknown,
                }
            }

            /// The value of the field named `key`, as JSON.
            pub fn field(&self, key: &str) -> Option<$crate::json::Value> {
                use $crate::record::JsonField;
                match key {
                    $( $key $(| $web)* => Some(<$ty as JsonField>::to_json(&self.$field)), )*
                    _ => None,
                }
            }

            /// `(key, value, holds its default)` for each table field, in declaration order.
            pub(crate) fn table_fields(&self) -> Vec<(&'static str, $crate::json::Value, bool)> {
                use $crate::record::JsonField;
                vec![ $( ($key, <$ty as JsonField>::to_json(&self.$field), self.$field == $default), )* ]
            }
        }
    };
}

pub(crate) use json_record;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round2_matches_js_math_round() {
        assert_eq!(round2(0.81234), 0.81);
        assert_eq!(round2(0.876), 0.88);
        assert_eq!(round2(1234.567), 1234.57);
        assert_eq!(round2(-0.025), -0.02); // JS: Math.round(-2.5) = -2
        assert_eq!(round2(0.125), 0.13);
        assert_eq!(round2(1.0), 1.0);
    }

    #[test]
    fn field_types() {
        assert_eq!(i32::from_json(&Value::from(3)), Some(3));
        assert_eq!(i32::from_json(&Value::Number(2.5)), None);
        assert_eq!(f64::from_json(&Value::from("1")), None);
        assert_eq!(Option::<i32>::from_json(&Value::Null), Some(None));
        assert_eq!(Option::<i32>::from_json(&Value::from(7)), Some(Some(7)));
        assert_eq!(Option::<i32>::from_json(&Value::from("7")), None);
        assert_eq!(Option::<String>::None.to_json(), Value::Null);
    }
}
