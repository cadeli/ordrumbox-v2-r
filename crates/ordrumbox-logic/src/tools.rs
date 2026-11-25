//! The MCP tools that only edit the library (`ordrumbox-app/src/mcp.rs`
//! declares them): patterns, tracks, notes, arrangements and the generators,
//! from JSON arguments to a JSON answer, through the [`Editor`]. A tool that
//! edits is one undo step; its arguments are checked before anything
//! changes, and a number brought inside its range is told in the answer.
//! The tools that play, read or write files, or show something stay in the
//! app, and use the lookups and the views of this module.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::song::clamp_bpm;
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_core::{Library, Note, Rng, ScaleList, Scales, SetField, Styles};

use crate::compose;
use ordrumbox_core::steps::MAX_BEATS;

use crate::editor::Editor;

/// Track settings a tool does not set (`mcp.rs` does not offer them).
const NOT_EDITED: [&str; 1] = ["name"];

/// Steps a beat of a track added by a tool (as the grid's).
const DEFAULT_STEPS_PER_BEAT: i32 = 4;

/// What the library tools work on: the editor, and what the generators read.
#[derive(Debug)]
pub struct Tools<'a> {
    pub editor: &'a mut Editor,
    pub scales: &'a Scales,
    /// The scale names, in file order.
    pub scale_list: &'a ScaleList,
    /// The styles of `styles.json`.
    pub styles: &'a Styles,
    /// Those styles with what the library's tags teach ([`learned_styles`]).
    pub learned_styles: &'a mut (Styles, Option<u64>),
    pub rng: &'a mut Rng,
    /// The synth presets a track may play, by key.
    pub synth_presets: &'a [&'a str],
    /// The range of a note setting, as the note editor allows it.
    pub note_range: fn(&str) -> Option<(f64, f64)>,
}

impl Tools<'_> {
    /// Run tool `name` on `args` (an object); what it answers, `None` for a
    /// tool that is not one of the library's.
    pub fn call(&mut self, name: &str, args: &Value) -> Option<Result<Value, String>> {
        let outcome = match name {
            "listPatterns" => Ok(self.list_patterns()),
            "loadPattern" => self.load_pattern(args),
            "createNewPattern" => self.create_pattern(args),
            "renamePattern" => self.rename_pattern(args),
            "duplicatePattern" => self.duplicate_pattern(args),
            "addNotesToPattern" => self.add_notes(args),
            "deleteNotes" => self.delete_notes(args),
            "removeTrack" => self.remove_track(args),
            "copyTrack" => self.copy_track(args),
            "updateTrack" => self.update_track(args),
            "renameTrack" => self.rename_track(args),
            "setBpm" => self.set_bpm(args),
            "setPatternBeatCount" => self.set_beat_count(args),
            "setPatternTags" => self.set_tags(args),
            "setPatternDescription" => self.set_description(args),
            "compactPattern" => self.compact_pattern(args),
            "listStyles" => Ok(self.list_styles()),
            "listScales" => Ok(self.list_scales()),
            "listAllInstrumentsNames" => Ok(instruments()),
            "generatePattern" => self.generate_pattern(args),
            "regeneratePattern" => self.regenerate_pattern(args),
            "toggleGeneration" => self.toggle_generation(args),
            "togglePatternAuto" => self.toggle_pattern_auto(args),
            "setPatternMacro" => self.set_macro(args),
            "regenerateTrack" => self.regenerate_track(args),
            "randomizeTrack" => self.randomize_track(args),
            "listArrangements" => Ok(self.list_arrangements()),
            "createArrangement" => self.create_arrangement(args),
            "addPatternToArrangement" => self.add_clip(args),
            "removePatternFromArrangement" => self.remove_clips(args),
            "updateArrangement" => self.update_arrangement(args),
            "renameArrangement" => self.rename_arrangement(args),
            "moveClip" => self.move_clip(args),
            "repeatClip" => self.repeat_clip(args),
            _ => return None,
        };
        Some(outcome)
    }

    fn library(&self) -> &Library {
        self.editor.library()
    }

    fn pattern(&self, reference: &str) -> Result<usize, String> {
        find_pattern(self.library(), reference)
    }

    /// The pattern named by argument `patternName`.
    fn pattern_arg(&self, args: &Value) -> Result<usize, String> {
        self.pattern(text(args, "patternName")?)
    }

    fn list_patterns(&self) -> Value {
        let patterns = self.library().patterns.iter().enumerate();
        object([
            ("bpm", Value::from(self.library().bpm)),
            ("patterns", Value::Array(patterns.map(|(i, p)| pattern_summary(i, p)).collect())),
        ])
    }

    fn load_pattern(&self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let pattern = &self.library().patterns[p];
        let mut json = pattern_summary(p, pattern);
        if let Some(object) = json.as_object_mut() {
            object.insert("description", Value::from(pattern.description.as_str()));
            object.insert("tracks", Value::Array(pattern.tracks.iter().map(track_json).collect()));
        }
        Ok(json)
    }

    /// Argument `key`, trimmed, as the name of pattern `p` (else of a new
    /// one): not empty, no other pattern's.
    fn new_name<'a>(&self, args: &'a Value, key: &str, p: Option<usize>) -> Result<&'a str, String> {
        let name = text(args, key)?.trim();
        if name.is_empty() {
            return Err(format!("{key} is empty"));
        }
        match self.pattern(name) {
            Ok(other) if Some(other) != p => Err(format!("The library has a pattern named \"{name}\" already")),
            _ => Ok(name),
        }
    }

    fn create_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let name = self.new_name(args, "patternName", None)?;
        let p = self.editor.add_pattern(Some(name));
        Ok(pattern_summary(p, &self.library().patterns[p]))
    }

    fn rename_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let name = self.new_name(args, "newName", Some(p))?;
        self.editor.rename_pattern(p, name);
        Ok(pattern_summary(p, &self.library().patterns[p]))
    }

    fn duplicate_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let name = match optional_text(args, "newName")? {
            Some(_) => Some(self.new_name(args, "newName", None)?),
            None => None,
        };
        let desc = format!("Duplicate pattern \"{}\"", pattern_name(&self.library().patterns[p]));
        let copy = self.editor.batch(&desc, |editor| {
            let copy = editor.duplicate_pattern(p).ok_or("the pattern was not copied")?;
            if let Some(name) = name {
                editor.rename_pattern(copy, name);
            }
            Ok::<_, String>(copy)
        })?;
        Ok(pattern_summary(copy, &self.library().patterns[copy]))
    }

    /// The note settings of `fields` (every key but the ones in `skip`), by
    /// their current key, checked: known, of the right type, inside the note
    /// editor's ranges.
    fn note_fields(&self, fields: &Object, skip: &[&str], at: &str) -> Result<Vec<(&'static str, Value)>, String> {
        let mut checked = Vec::new();
        for (key, value) in fields.iter().filter(|(key, _)| !skip.contains(key)) {
            let canonical = Note::canonical_key(key).filter(|k| !["beat", "step"].contains(k));
            let Some(canonical) = canonical else { return Err(format!("{at}.{key}: not a note setting")) };
            if Note::default().set_field(key, value) == SetField::WrongType {
                return Err(format!("{at}.{key}: wrong type ({value})"));
            }
            if let (Some(n), Some((min, max))) = (value.as_f64(), (self.note_range)(canonical))
                && !(min..=max).contains(&n)
            {
                return Err(format!("{at}.{key}: {n} is outside {min} to {max}"));
            }
            checked.push((canonical, value.clone()));
        }
        Ok(checked)
    }

    fn add_notes(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let pattern = &self.library().patterns[p];
        let mut planned = Vec::new();
        let mut new_tracks: Vec<String> = Vec::new();
        for (i, item) in list(args, "notes")?.iter().enumerate() {
            let at = format!("notes[{i}]");
            let fields = item.as_object().ok_or_else(|| format!("{at}: not an object"))?;
            let track = text(item, "trackName").map_err(|e| format!("{at}.{e}"))?.trim().to_uppercase();
            let step = optional_whole(item, "step").map_err(|e| format!("{at}.{e}"))?;
            let step = step.ok_or_else(|| format!("{at}.step is required"))?;
            let existing = pattern.tracks.iter().find(|t| same_name(&t.name, &track));
            if existing.is_none() && !new_tracks.contains(&track) {
                new_tracks.push(track.clone());
            }
            let steps_per_beat = existing.map_or(DEFAULT_STEPS_PER_BEAT, |t| t.steps_per_beat).max(1);
            if step / steps_per_beat as u32 >= MAX_BEATS as u32 {
                return Err(format!(
                    "{at}.step: {step} is past the {MAX_BEATS} beats of a pattern ({steps_per_beat} steps a beat)"
                ));
            }
            planned.push((track, step, self.note_fields(fields, &["trackName", "step"], &at)?));
        }
        if pattern.tracks.len() + new_tracks.len() > MAX_TRACKS {
            return Err(format!("a pattern holds at most {MAX_TRACKS} tracks"));
        }
        let desc = format!("Add {} notes to \"{}\"", planned.len(), pattern_name(pattern));
        let (mut created, mut updated) = (0, 0);
        self.editor.batch(&desc, |editor| {
            for (track, step, fields) in &planned {
                let t = match editor.library().patterns[p].tracks.iter().position(|t| same_name(&t.name, track)) {
                    Some(t) => t,
                    None => editor.add_track(p, track, DEFAULT_STEPS_PER_BEAT).ok_or("no room for a track")?,
                };
                let (beat, beat_step) = beat_and_step(*step, editor.library().patterns[p].tracks[t].steps_per_beat);
                grow_pattern(editor, p, beat + 1);
                let notes = &editor.library().patterns[p].tracks[t].notes;
                let n = match notes.iter().position(|n| n.beat == beat && n.step == beat_step) {
                    Some(n) => {
                        updated += 1;
                        n
                    }
                    None => {
                        created += 1;
                        editor.add_note(p, t, beat, beat_step, 0.0).ok_or("the note was not added")?
                    }
                };
                editor.update_note(p, t, n, fields, None, false);
            }
            Ok::<(), String>(())
        })?;
        let pattern = &self.library().patterns[p];
        Ok(object([
            ("created", count(created)),
            ("updated", count(updated)),
            ("tracksAdded", strings(&new_tracks)),
            ("beatCount", Value::from(pattern.beat_count)),
        ]))
    }

    fn delete_notes(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let pattern = &self.library().patterns[p];
        let mut planned: Vec<(usize, i32, i32)> = Vec::new();
        for (i, item) in list(args, "notes")?.iter().enumerate() {
            let at = format!("notes[{i}]");
            let track = text(item, "trackName").map_err(|e| format!("{at}.{e}"))?;
            let t = track_index(pattern, track)
                .ok_or_else(|| format!("{at}.trackName: the pattern has no track \"{}\"", track.trim()))?;
            let step = optional_whole(item, "step").map_err(|e| format!("{at}.{e}"))?;
            let step = step.ok_or_else(|| format!("{at}.step is required"))?;
            let (beat, beat_step) = beat_and_step(step, pattern.tracks[t].steps_per_beat);
            if !planned.contains(&(t, beat, beat_step)) {
                planned.push((t, beat, beat_step));
            }
        }
        let at_step = |&(t, beat, beat_step): &(usize, i32, i32)| {
            pattern.tracks[t].notes.iter().filter(|n| n.beat == beat && n.step == beat_step).count()
        };
        let deleted: usize = planned.iter().map(at_step).sum();
        if deleted == 0 {
            return Err("No note at those steps".into());
        }
        let desc = format!("Delete {deleted} notes in \"{}\"", pattern_name(pattern));
        self.editor.batch(&desc, |editor| {
            for &(t, beat, beat_step) in &planned {
                editor.delete_step(p, t, beat, beat_step);
            }
        });
        Ok(object([("deleted", count(deleted))]))
    }

    fn remove_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        // As the window: a pattern keeps a track.
        if self.library().patterns[p].tracks.len() <= 1 {
            return Err("The last track of a pattern stays".into());
        }
        let name = self.library().patterns[p].tracks[t].name.clone();
        self.editor.remove_track(p, t);
        Ok(object([("removed", Value::from(name)), ("tracks", self.track_names(p))]))
    }

    fn track_names(&self, p: usize) -> Value {
        Value::Array(self.library().patterns[p].tracks.iter().map(|t| Value::from(t.name.as_str())).collect())
    }

    fn copy_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        let to = match optional_text(args, "toPatternName")?.filter(|n| !n.trim().is_empty()) {
            Some(name) => self.pattern(name)?,
            None => p,
        };
        let library = self.library();
        let source = library.patterns[p].tracks[t].clone();
        let new_name = optional_text(args, "newName")?.map(str::trim).filter(|n| !n.is_empty());
        let new_name = new_name.map_or_else(|| source.name.clone(), str::to_uppercase);
        let destination = pattern_name(&library.patterns[to]);
        if track_index(&library.patterns[to], &new_name).is_some() {
            return Err(format!("\"{destination}\" has a track named {new_name} already: give newName"));
        }
        if !self.editor.has_track_room(to) {
            return Err(format!("a pattern holds at most {MAX_TRACKS} tracks"));
        }
        let desc = format!("Copy track {} to \"{destination}\"", source.name);
        let copy = self.editor.batch(&desc, |editor| {
            let copy = editor.paste_track(to, usize::MAX, &source).ok_or("no room for a track")?;
            editor.change_track_name(to, copy, &new_name);
            Ok::<_, String>(copy)
        })?;
        let track = &self.library().patterns[to].tracks[copy];
        Ok(object([
            ("patternName", Value::from(destination)),
            ("trackName", Value::from(track.name.as_str())),
            ("noteCount", count(track.notes.len())),
        ]))
    }

    fn update_track(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let name = text(args, "trackName")?.trim().to_uppercase();
        let updates = args.get("updates").and_then(Value::as_object).ok_or("updates: not an object")?;
        let updates = track_fields(updates, "updates")?;
        let preset = updates.iter().find(|(key, _)| *key == "synthPreset").and_then(|(_, v)| v.as_str());
        if let Some(preset) = preset.filter(|p| !self.synth_presets.contains(p)) {
            return Err(format!("updates.synthPreset: no preset \"{preset}\" (listSynthPresets gives them)"));
        }
        let note_updates = match args.get("noteUpdates") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Object(fields)) => self.note_fields(fields, &[], "noteUpdates")?,
            Some(other) => return Err(format!("noteUpdates: not an object ({other})")),
        };
        let pattern = &self.library().patterns[p];
        let existing = track_index(pattern, &name);
        if existing.is_none() && !self.editor.has_track_room(p) {
            return Err(format!("a pattern holds at most {MAX_TRACKS} tracks"));
        }
        let desc = format!("Update {name} in \"{}\"", pattern_name(pattern));
        let (t, notes) = self.editor.batch(&desc, |editor| {
            let t = match existing {
                Some(t) => t,
                None => editor.add_track(p, &name, DEFAULT_STEPS_PER_BEAT).ok_or("no room for a track")?,
            };
            editor.update_track(p, t, &updates, None, false);
            let notes = if note_updates.is_empty() { 0 } else { editor.library().patterns[p].tracks[t].notes.len() };
            for n in 0..notes {
                editor.update_note(p, t, n, &note_updates, None, false);
            }
            Ok::<_, String>((t, notes))
        })?;
        let track = &self.library().patterns[p].tracks[t];
        let settings = updates.iter().map(|(key, _)| (*key, track.field(key).unwrap_or_default()));
        Ok(object([
            ("action", Value::from(if existing.is_some() { "updated" } else { "added" })),
            ("trackName", Value::from(track.name.as_str())),
            ("settings", object(settings)),
            ("notesUpdated", count(notes)),
        ]))
    }

    fn rename_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        let name = text(args, "newName")?.trim().to_uppercase();
        if name.is_empty() {
            return Err("newName is empty".into());
        }
        let pattern = &self.library().patterns[p];
        if track_index(pattern, &name).is_some_and(|other| other != t) {
            return Err(format!("\"{}\" has a track named {name} already", pattern_name(pattern)));
        }
        self.editor.change_track_name(p, t, &name);
        Ok(object([("trackName", Value::from(name)), ("tracks", self.track_names(p))]))
    }

    fn set_bpm(&mut self, args: &Value) -> Result<Value, String> {
        let asked = optional_number(args, "bpm")?.ok_or("bpm is required")?;
        // As the tempo field: whole, inside the bounds.
        let bpm = clamp_bpm(asked.round());
        self.editor.set_bpm(bpm, false);
        let mut answer = object([("bpm", Value::from(bpm))]);
        if bpm != asked
            && let Some(object) = answer.as_object_mut()
        {
            object.insert("note", Value::from(format!("{asked} asked, {bpm} used (whole, 20 to 300)")));
        }
        Ok(answer)
    }

    fn set_beat_count(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let beats = optional_whole(args, "beatCount")?.ok_or("beatCount is required")?;
        if !(1..=MAX_BEATS as u32).contains(&beats) {
            return Err(format!("beatCount: {beats} is outside 1 to {MAX_BEATS}"));
        }
        self.editor.set_pattern_beat_count(p, f64::from(beats));
        Ok(object([("beatCount", Value::from(self.library().patterns[p].beat_count))]))
    }

    fn set_tags(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let tags = list(args, "tags")?
            .iter()
            .map(|t| t.as_str().map(str::to_owned).ok_or_else(|| format!("tags: {t} is not a string")));
        let tags = tags.collect::<Result<Vec<_>, _>>()?;
        self.editor.set_pattern_tags(p, &tags);
        Ok(object([("tags", strings(&self.library().patterns[p].tags))]))
    }

    fn set_description(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let description = text(args, "description")?;
        self.editor.set_pattern_description(p, description);
        Ok(object([("description", Value::from(description))]))
    }

    fn compact_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let desc = format!("Compact the tracks of \"{}\"", pattern_name(&self.library().patterns[p]));
        let compacted = self.editor.batch(&desc, |editor| editor.compact_pattern(p));
        Ok(object([("tracksCompacted", count(compacted))]))
    }
}

/// `written` with what the library's tags teach, learned again when the
/// library changed since `cache` was (its revision).
pub fn learned_styles<'s>(cache: &'s mut (Styles, Option<u64>), written: &Styles, editor: &Editor) -> &'s Styles {
    let revision = editor.revision();
    if cache.1 != Some(revision) {
        *cache = (compose::learn(written, &editor.library().patterns), Some(revision));
    }
    &cache.0
}

#[path = "tools/arrangements.rs"]
mod arrangements;
#[path = "tools/generation.rs"]
mod generation;
#[path = "tools/helpers.rs"]
mod helpers;
pub use helpers::*;

#[cfg(test)]
#[path = "tools/tests.rs"]
mod tests;
