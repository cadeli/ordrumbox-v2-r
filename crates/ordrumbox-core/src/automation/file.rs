//! Automation lanes in a song file: `{instrument, target, points}`, each
//! point `{step, value}`, a `null` value giving the control back to the
//! pattern.

use super::{AutomationLane, AutomationPoint, AutomationTarget, kept};
use crate::json::{Object, Value};
use crate::record::Issue;

impl AutomationLane {
    /// A lane read from a file (`at` its JSON path): points with no usable
    /// step or value left out, a repeated step kept once (the last one), a
    /// value out of 0 to 1 brought into it, all reported.
    pub fn from_json(raw: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<Self> {
        let Some(object) = raw.as_object() else {
            issues.push(Issue::new(at, "an automation lane must be an object, skipped"));
            return None;
        };
        let instrument = object.get("instrument").and_then(Value::to_text).unwrap_or_default();
        let instrument = instrument.trim();
        if instrument.is_empty() {
            issues.push(Issue::new(format!("{at}.instrument"), "no instrument, lane skipped"));
            return None;
        }
        let key = object.get("target").and_then(Value::to_text).unwrap_or_default();
        let Some(target) = AutomationTarget::from_key(&key) else {
            issues.push(Issue::new(format!("{at}.target"), format!("unknown target \"{key}\", lane skipped")));
            return None;
        };
        let mut lane = Self::new(instrument, target);
        let points = match object.get("points").filter(|v| !v.is_null()) {
            None => &[][..],
            Some(Value::Array(items)) => items.as_slice(),
            Some(other) => {
                issues.push(Issue::new(format!("{at}.points"), format!("not an array ({other}), no points")));
                &[][..]
            }
        };
        for (i, raw_point) in points.iter().enumerate() {
            let Some(point) = point_from_json(raw_point, &format!("{at}.points[{i}]"), issues) else { continue };
            match lane.points.binary_search_by_key(&point.step, |p| p.step) {
                Ok(index) => {
                    let message = format!("step {} repeated, the last point kept", point.step);
                    issues.push(Issue::new(format!("{at}.points[{i}]"), message));
                    lane.points[index] = point;
                }
                Err(index) => lane.points.insert(index, point),
            }
        }
        Some(lane)
    }

    #[must_use]
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("instrument", Value::from(self.instrument.as_str()));
        object.insert("target", Value::from(self.target.key()));
        let points = self.points.iter().map(|point| {
            let mut object = Object::new();
            object.insert("step", Value::from(point.step));
            object.insert("value", point.value.map_or(Value::Null, Value::from));
            Value::Object(object)
        });
        object.insert("points", Value::Array(points.collect()));
        Value::Object(object)
    }
}

/// A point read from a file (`at` its JSON path); `None`, reported, when its
/// step or its value is missing or unusable.
fn point_from_json(raw: &Value, at: &str, issues: &mut Vec<Issue>) -> Option<AutomationPoint> {
    let object = raw.as_object();
    let step = object.and_then(|p| p.get("step")).and_then(Value::to_number);
    let Some(step) = step.filter(|s| s.is_finite() && *s >= 0.0 && *s <= f64::from(u32::MAX)) else {
        issues.push(Issue::new(at, "no usable step, point skipped"));
        return None;
    };
    let value = match object.and_then(|p| p.get("value")) {
        Some(Value::Null) => None,
        Some(raw) if raw.to_number().is_some_and(f64::is_finite) => raw.to_number(),
        Some(other) => {
            issues.push(Issue::new(format!("{at}.value"), format!("not a number ({other}), point skipped")));
            return None;
        }
        None => {
            issues.push(Issue::new(format!("{at}.value"), "no value, point skipped"));
            return None;
        }
    };
    if let Some(raw) = value.filter(|&v| kept(v) != v) {
        issues.push(Issue::new(format!("{at}.value"), format!("{raw} kept as {}", kept(raw))));
    }
    Some(AutomationPoint { step: step.floor() as u32, value: value.map(kept) })
}
