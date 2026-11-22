//! A section of `settings.json` that holds a set of numbers (the master
//! bus's, the returns'): one key per setting, read over the defaults with
//! what was not taken as it is reported, and written whole.

use ordrumbox_core::json::{Object, Value};

/// A section: its key in `settings.json`, the keys of its numbers and of
/// its switches (a field at 0 or 1, written as a boolean), the field of each
/// key, and the clamp of its ranges.
#[derive(Debug)]
pub struct Section<T: 'static> {
    pub name: &'static str,
    pub numbers: &'static [&'static str],
    pub switches: &'static [&'static str],
    /// The field holding a key of `numbers` or `switches`.
    pub field: for<'a> fn(&'a mut T, &str) -> &'a mut f32,
    pub clamped: fn(&T) -> T,
}

impl<T: Copy + Default> Section<T> {
    /// Setting `key` of `settings`.
    #[must_use]
    pub fn get(&self, settings: &T, key: &str) -> f64 {
        let mut copy = *settings;
        f64::from(*(self.field)(&mut copy, key))
    }

    /// `settings` with setting `key` at `value`.
    #[must_use]
    pub fn set(&self, mut settings: T, key: &str, value: f64) -> T {
        *(self.field)(&mut settings, key) = value as f32;
        settings
    }

    /// The section read over the defaults, with what was not taken as it
    /// is: a value of the wrong type (default kept), out of range (clamped),
    /// an unknown key (ignored).
    #[must_use]
    pub fn read_json(&self, value: &Value) -> (T, Vec<String>) {
        let name = self.name;
        let mut settings = T::default();
        let mut issues = Vec::new();
        let Some(object) = value.as_object() else {
            return (settings, vec![format!("{name}: not an object ({value}), defaults used")]);
        };
        for (key, v) in object.iter() {
            if self.switches.contains(&key) {
                match v.as_bool() {
                    Some(on) => settings = self.set(settings, key, if on { 1.0 } else { 0.0 }),
                    None => issues.push(format!("{name}.{key}: not a boolean ({v}), default kept")),
                }
            } else if self.numbers.contains(&key) {
                match v.as_f64() {
                    Some(number) => settings = self.set(settings, key, number),
                    None => issues.push(format!("{name}.{key}: not a number ({v}), default kept")),
                }
            } else {
                issues.push(format!("{name}.{key}: unknown setting, ignored"));
            }
        }
        let mut clamped = (self.clamped)(&settings);
        for &key in self.numbers {
            // Shown as the `f32` they are held in: `0.99`, not its `f64` expansion.
            let (asked, used) = (*(self.field)(&mut settings, key), *(self.field)(&mut clamped, key));
            if asked != used {
                issues.push(format!("{name}.{key}: {asked} out of range, {used} used"));
            }
        }
        (clamped, issues)
    }

    /// The section as `settings.json` holds it.
    #[must_use]
    pub fn to_json(&self, settings: &T) -> Value {
        let mut object = Object::new();
        for &key in self.numbers {
            object.insert(key, Value::from(self.get(settings, key)));
        }
        for &key in self.switches {
            object.insert(key, Value::Bool(self.get(settings, key) >= 0.5));
        }
        Value::Object(object)
    }
}
