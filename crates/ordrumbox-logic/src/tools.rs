//! The MCP tools that only edit the library (`ordrumbox-app/src/mcp.rs`
//! declares them): patterns, tracks, notes, arrangements and the generators,
//! from JSON arguments to a JSON answer, through the [`Editor`]. A tool that
//! edits is one undo step; its arguments are checked before anything
//! changes, and a number brought inside its range is told in the answer.
//! The tools that play, read or write files, or show something stay in the
//! app, and use the lookups and the views of this module.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::song::{SongClip, clamp_bpm};
use ordrumbox_core::track::MAX_TRACKS;
use ordrumbox_core::{Library, Note, Pattern, Rng, ScaleList, Scales, SetField, Styles, Track, style};

use crate::compose;
use crate::editor::{Editor, MAX_BEATS};
use crate::generate::{self, MelodicKind};

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

    /// The styles the generation follows.
    fn generation_styles(&mut self) -> Styles {
        learned_styles(self.learned_styles, self.styles, self.editor).clone()
    }

    fn list_styles(&mut self) -> Value {
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

    fn list_scales(&self) -> Value {
        let list = self.scale_list;
        let scales = list.names.iter().zip(&list.steps).filter_map(|(name, steps)| {
            let steps = steps.as_ref()?.iter().map(|&s| Value::from(s)).collect();
            Some(object([("name", Value::from(name.as_str())), ("degrees", Value::Array(steps))]))
        });
        object([("scales", Value::Array(scales.collect()))])
    }

    /// The styles the generation follows, and argument `style` among them
    /// (by its name, any case) when given.
    fn style(&mut self, args: &Value) -> Result<(Styles, Option<String>), String> {
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
    fn generated_json(&self, p: usize) -> Value {
        let pattern = &self.library().patterns[p];
        let mut json = pattern_summary(p, pattern);
        let style = pattern.extra.get(generate::GENRE).and_then(Value::as_str);
        if let Some(object) = json.as_object_mut() {
            object.insert("style", style.map_or(Value::Null, Value::from));
        }
        json
    }

    fn generate_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let (styles, style) = self.style(args)?;
        let rng = &mut *self.rng;
        let p = self.editor.generate_pattern(self.scales, &styles, style.as_deref(), &mut || rng.next_f64());
        Ok(self.generated_json(p))
    }

    fn regenerate_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let (styles, style) = self.style(args)?;
        let rng = &mut *self.rng;
        self.editor.regenerate_pattern(p, self.scales, &styles, style.as_deref(), &mut || rng.next_f64());
        Ok(self.generated_json(p))
    }

    fn toggle_generation(&mut self, args: &Value) -> Result<Value, String> {
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

    fn toggle_pattern_auto(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.pattern_arg(args)?;
        let on = self.editor.toggle_pattern_auto(p).ok_or("The pattern has no track to put in auto mode")?;
        Ok(object([("auto", Value::Bool(on))]))
    }

    fn set_macro(&mut self, args: &Value) -> Result<Value, String> {
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
    fn track_notes(&self, p: usize, t: usize) -> Value {
        let track = &self.library().patterns[p].tracks[t];
        object([("trackName", Value::from(track.name.as_str())), ("noteCount", count(track.notes.len()))])
    }

    fn regenerate_track(&mut self, args: &Value) -> Result<Value, String> {
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

    fn randomize_track(&mut self, args: &Value) -> Result<Value, String> {
        let (p, t) = find_track(self.library(), args)?;
        let rng = &mut *self.rng;
        self.editor.randomize_track(p, t, &mut || rng.next_f64());
        Ok(self.track_notes(p, t))
    }

    fn list_arrangements(&self) -> Value {
        let library = self.library();
        let songs = (0..library.songs.len()).map(|s| song_json(library, s));
        object([("arrangements", Value::Array(songs.collect())), ("selected", count(self.editor.selected_song()))])
    }

    /// A clip of `createArrangement`: its pattern, start and length.
    fn clip(&self, item: &Value, at: &str) -> Result<(String, u32, Option<f64>), String> {
        let p = self.pattern(text(item, "patternName").map_err(|e| format!("{at}.{e}"))?)?;
        let start = optional_whole(item, "startMeasure").map_err(|e| format!("{at}.{e}"))?.unwrap_or(0);
        let length = optional_number(item, "measureCount").map_err(|e| format!("{at}.{e}"))?;
        if length.is_some_and(|l| l <= 0.0) {
            return Err(format!("{at}.measureCount: not above 0"));
        }
        Ok((self.library().patterns[p].id.clone(), start, length))
    }

    fn create_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let name = text(args, "name")?.trim();
        if name.is_empty() {
            return Err("name is empty".into());
        }
        let (description, bpm, loop_measures) = song_settings(args)?;
        let clips = match args.get("clips") {
            None | Some(Value::Null) => Vec::new(),
            Some(_) => {
                let items = list(args, "clips")?.iter().enumerate();
                items.map(|(i, item)| self.clip(item, &format!("clips[{i}]"))).collect::<Result<_, _>>()?
            }
        };
        let s = self.editor.batch(&format!("Add arrangement \"{name}\""), |editor| {
            let s = editor.add_song(name);
            for (id, start, length) in &clips {
                editor.add_pattern_at_measure(id, *start, *length, Some(s))?;
            }
            editor.set_song_settings(Some(s), bpm, description, loop_measures)?;
            Ok::<_, String>(s)
        })?;
        Ok(self.song_json_with_bpm(s, bpm))
    }

    /// [`song_json`], with a note when the tempo used is not `asked`.
    fn song_json_with_bpm(&self, s: usize, asked: Option<f64>) -> Value {
        let mut answer = song_json(self.library(), s);
        let used = self.library().songs[s].bpm;
        if let (Some(asked), Some(object)) = (asked.filter(|&b| b != used), answer.as_object_mut()) {
            object.insert("note", Value::from(format!("bpm {asked} asked, {used} used (20 to 300)")));
        }
        answer
    }

    fn add_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let (id, start, length) = self.clip(args, "arguments")?;
        let clip = self.editor.add_pattern_at_measure(&id, start, length, Some(s))?;
        Ok(object([("clip", clip_json(&clip)), ("arrangement", song_json(self.library(), s))]))
    }

    fn remove_clips(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let start = optional_whole(args, "startMeasure")?;
        let pattern = optional_text(args, "patternName")?.filter(|n| !n.trim().is_empty());
        if start.is_none() && pattern.is_none() {
            return Err("Give startMeasure, patternName, or both".into());
        }
        let id = pattern.map(|name| self.pattern(name)).transpose()?;
        let id = id.map(|p| self.library().patterns[p].id.clone());
        let removed = self.editor.batch("Remove clips", |editor| {
            let at = editor.library().songs[s].clips.iter().filter(|c| Some(c.start_measure) == start).count();
            let mut removed = 0;
            if let Some(start) = start.filter(|_| at > 0) {
                removed += editor.remove_pattern_at_measure(start, Some(s))?.len();
            }
            if let Some(id) = &id {
                removed += editor.remove_pattern_clips(id, Some(s))?.len();
            }
            Ok::<_, String>(removed)
        })?;
        if removed == 0 {
            return Err("No matching clip to remove".into());
        }
        Ok(object([("removed", count(removed)), ("arrangement", song_json(self.library(), s))]))
    }

    fn update_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let (description, bpm, loop_measures) = song_settings(args)?;
        if description.is_none() && bpm.is_none() && loop_measures.is_none() {
            return Err("Give description, bpm, loopMeasures, or several".into());
        }
        self.editor.set_song_settings(Some(s), bpm, description, loop_measures)?;
        Ok(self.song_json_with_bpm(s, bpm))
    }

    fn rename_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let name = text(args, "newName")?.trim();
        if name.is_empty() {
            return Err("newName is empty".into());
        }
        let songs = &self.library().songs;
        if songs.iter().enumerate().any(|(other, song)| other != s && same_name(&song.name, name)) {
            return Err(format!("The library has an arrangement named \"{name}\" already"));
        }
        self.editor.rename_song(Some(s), name)?;
        Ok(song_json(self.library(), s))
    }

    fn move_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let start = optional_whole(args, "startMeasure")?.ok_or("startMeasure is required")?;
        let to = optional_whole(args, "toMeasure")?.ok_or("toMeasure is required")?;
        let pattern = optional_text(args, "patternName")?.filter(|n| !n.trim().is_empty());
        let id = pattern.map(|name| self.pattern(name)).transpose()?;
        let id = id.map(|p| self.library().patterns[p].id.clone());
        let clips = &self.library().songs[s].clips;
        let found: Vec<usize> = (0..clips.len())
            .filter(|&i| clips[i].start_measure == start && id.as_ref().is_none_or(|id| clips[i].pattern_id == *id))
            .collect();
        let index = match found.as_slice() {
            [] => return Err(format!("No clip at measure {start}")),
            [index] => *index,
            _ => return Err(format!("Several clips start at measure {start}: give patternName")),
        };
        self.editor.move_song_clip(Some(s), index, to);
        Ok(song_json(self.library(), s))
    }

    fn repeat_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let start = optional_whole(args, "startMeasure")?.ok_or("startMeasure is required")?;
        let clip = self.editor.repeat_pattern_at_measure(start, Some(s))?;
        Ok(object([("clip", clip_json(&clip)), ("arrangement", song_json(self.library(), s))]))
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

#[path = "tools/helpers.rs"]
mod helpers;
pub use helpers::*;

#[cfg(test)]
#[path = "tools/tests.rs"]
mod tests;
