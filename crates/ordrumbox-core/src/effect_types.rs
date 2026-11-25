//! The filter and delay types of a track's strip, and the names the files
//! write them as.

use crate::json::Value;
use crate::record::JsonField;

/// An enum of named types: its variants, each with the name a file writes,
/// and its default. A file's unknown name is refused (its `Issue` lists the
/// names known), so the field keeps its default.
macro_rules! named_types {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $text:literal, )*
        }
        default $default:ident;
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name {
            $( $(#[$vmeta])* $variant, )*
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$default
            }
        }

        impl $name {
            /// Every type, in the order the files' names are listed.
            pub const ALL: &'static [Self] = &[$( Self::$variant ),*];

            /// The name a file writes.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $( Self::$variant => $text, )*
                }
            }

            /// The type a file's `name` stands for; `None` for an unknown name.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $( $text => Some(Self::$variant), )*
                    _ => None,
                }
            }
        }

        /// A name of [`Self::ALL`].
        impl JsonField for $name {
            fn from_json(value: &Value) -> Option<Self> {
                value.as_str().and_then(Self::from_name)
            }
            fn to_json(&self) -> Value {
                Value::from(self.name())
            }
            fn refusal(value: &Value) -> String {
                if value.as_str().is_none() {
                    return crate::record::refusal(value);
                }
                let names: Vec<&str> = Self::ALL.iter().map(|t| t.name()).collect();
                format!("unknown name ({}), default kept; one of {}", value.to_json(), names.join(", "))
            }
        }
    };
}

named_types! {
    /// A track's filter: `Allpass` skips it.
    pub enum FilterType {
        Allpass => "allpass",
        Lowpass => "lowpass",
        Highpass => "highpass",
        Bandpass => "bandpass",
        Notch => "notch",
    }
    default Allpass;
}

named_types! {
    /// A track's delay: `None` skips it, `Slap` plays one echo, `Tape` has
    /// wow and a darker feedback, `PingPong` starts on the left and
    /// alternates.
    pub enum DelayType {
        None => "none",
        Slap => "slap",
        Tape => "tape",
        PingPong => "pingpong",
    }
    default Tape;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_type_reads_back_its_name() {
        for &t in FilterType::ALL {
            assert_eq!(FilterType::from_json(&t.to_json()), Some(t));
        }
        for &t in DelayType::ALL {
            assert_eq!(DelayType::from_json(&t.to_json()), Some(t));
        }
    }

    #[test]
    fn an_unknown_name_is_refused_with_the_names_known() {
        assert_eq!(FilterType::from_json(&Value::from("Lowpass")), None);
        assert_eq!(DelayType::from_json(&Value::from(1)), None);
        let message = DelayType::refusal(&Value::from("echo"));
        assert_eq!(message, r#"unknown name ("echo"), default kept; one of none, slap, tape, pingpong"#);
        assert_eq!(DelayType::refusal(&Value::from(1)), crate::record::refusal(&Value::from(1)));
    }
}
