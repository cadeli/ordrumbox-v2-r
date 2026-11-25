//! The tools on styles, scales and the automatic generation: list the
//! styles and scales, generate or regenerate a pattern or a track, auto mode,
//! the macros, a random track (`GENERATION_TOOLS`, used only when asked).

use super::answers::*;
use super::arguments::*;
use super::{Tools, learned_styles};
use crate::generate::{self, MelodicKind};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Styles, style};

impl Tools<'_> {
    /// The styles the generation follows.
    pub(super) fn generation_styles(&mut self) -> Styles {
        learned_styles(self.learned_styles, self.styles, self.editor).clone()
    }

    pub(super) fn list_styles(&mut self) -> Value {
        let styles = learned_styles(self.learned_styles, self.styles, self.editor);
        let styles = styles.list.iter().map(|style| {
            object([
                ("name", Value::from(style.name.as_str())),
                ("learnedFromPatterns", style.learned.map_or(Value::Null, |(patterns, _)| count(patterns))),
                ("beatCount", Value::from(style.beat_count)),
                ("stepsPerBeat", Value::from(style.steps_per_beat)),
                ("swingAmount", Value::from(style.swing_amount)),
                ("bpmMin", Value::from(style.tempo.0)),
                ("bpmMax", Value::from(style.tempo.1)),
                ("tracks", strings(&style.tracks)),
            ])
        });
        object([("styles", Value::Array(styles.collect()))])
    }

    pub(super) fn list_scales(&self) -> Value {
        let list = self.scale_list;
        let scales = list.names.iter().zip(&list.steps).filter_map(|(name, steps)| {
            let steps = steps.as_ref()?.iter().map(|&s| Value::from(s)).collect();
            Some(object([("name", Value::from(name.as_str())), ("degrees", Value::Array(steps))]))
        });
        object([("scales", Value::Array(scales.collect()))])
    }

    /// The styles the generation follows, and argument `style` among them
    /// (by its name, any case) when given.
    pub(super) fn style(&mut self, args: &Value) -> Result<(Styles, Option<String>), String> {
        let styles = self.generation_styles();
        let style = match optional_text(args, "style")?.filter(|s| !s.trim().is_empty()) {
            Some(wanted) => {
                let found = styles.names().into_iter().find(|name| same_name(name, wanted)).map(str::to_owned);
                Some(found.ok_or_else(|| format!("No style \"{}\" (listStyles gives them)", wanted.trim()))?)
            }
            None => None,
        };
        Ok((styles, style))
    }

    /// A generated pattern as a tool shows it: its summary and its style.
    pub(super) fn generated_json(&self, p: usize) -> Value {
        let pattern = &self.library().patterns[p];
        let mut json = pattern_summary(p, pattern);
        let style = pattern.extra.get(generate::STYLE_KEY).and_then(Value::as_str);
        if let Some(object) = json.as_object_mut() {
            object.insert("style", style.map_or(Value::Null, Value::from));
        }
        json
    }

    pub(super) fn generate_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let (styles, style) = self.style(args)?;
        let rng = &mut *self.rng;
        let p = self.editor.generate_pattern(self.scales, &styles, style.as_deref(), &mut || rng.next_f64());
        Ok(self.generated_json(p))
    }

    pub(super) fn regenerate_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let (styles, style) = self.style(args)?;
        let rng = &mut *self.rng;
        self.editor.regenerate_pattern(p, self.scales, &styles, style.as_deref(), &mut || rng.next_f64());
        Ok(self.generated_json(p))
    }

    pub(super) fn toggle_generation(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let part = text(args, "part")?;
        let (styles, style) = self.style(args)?;
        let rng = &mut *self.rng;
        let rnd = &mut || rng.next_f64();
        let on = match part {
            "drums" => self.editor.toggle_drums(p, self.scales, &styles, style.as_deref(), rnd),
            "bass" => self.editor.toggle_melodic(p, MelodicKind::Bass, self.scales, &styles, rnd),
            "chords" => self.editor.toggle_melodic(p, MelodicKind::Chords, self.scales, &styles, rnd),
            _ => return Err(format!("part: \"{part}\" is not drums, bass or chords")),
        };
        let on = on.ok_or_else(|| format!("No {part} generated for this pattern"))?;
        let mut answer = self.generated_json(p);
        if let Some(object) = answer.as_object_mut() {
            object.insert("part", Value::from(part));
            object.insert("auto", Value::Bool(on));
        }
        Ok(answer)
    }

    pub(super) fn toggle_pattern_auto(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let on = self.editor.toggle_pattern_auto(p).ok_or("The pattern has no track to put in auto mode")?;
        Ok(object([("auto", Value::Bool(on))]))
    }

    pub(super) fn set_macro(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let name = text(args, "macro")?;
        let key = match name {
            "energy" => style::ENERGY,
            "complexity" => style::COMPLEXITY,
            "variation" => style::VARIATION,
            _ => return Err(format!("macro: \"{name}\" is not energy, complexity or variation")),
        };
        let value = optional_number(args, "value")?.ok_or("value is required")?;
        if !(0.0..=100.0).contains(&value) {
            return Err(format!("value: {value} is outside 0 to 100"));
        }
        let styles = self.generation_styles();
        let rng = &mut *self.rng;
        self.editor.set_pattern_macro(p, key, value, (self.scales, &styles), &mut || rng.next_f64());
        let used = self.library().patterns[p].extra.get(key).and_then(Value::as_f64).unwrap_or(value);
        Ok(object([("macro", Value::from(name)), ("value", Value::from(used))]))
    }

    /// Track `t` of pattern `p`: its name and how many notes it has.
    pub(super) fn track_notes(&self, p: usize, t: usize) -> Value {
        let track = &self.library().patterns[p].tracks[t];
        object([("trackName", Value::from(track.name.as_str())), ("noteCount", count(track.notes.len()))])
    }

    pub(super) fn regenerate_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        let track = &self.library().patterns[p].tracks[t];
        if track.compose_lock {
            return Err(format!("{} is locked: set composeLock false with updateTrack first", track.name));
        }
        let styles = self.generation_styles();
        let rng = &mut *self.rng;
        if !self.editor.regenerate_track(p, t, self.scales, &styles, &mut || rng.next_f64()) {
            return Err("No new line for this track".into());
        }
        Ok(self.track_notes(p, t))
    }

    pub(super) fn randomize_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        let rng = &mut *self.rng;
        self.editor.randomize_track(p, t, &mut || rng.next_f64());
        Ok(self.track_notes(p, t))
    }
}
