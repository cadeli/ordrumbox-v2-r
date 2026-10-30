//! The MCP tools (`mcp.rs`) run on the session: what the JS server did to
//! `song.json`, done to the library the window shows. A tool that edits
//! is one undo step and is saved like any edit; its arguments are checked
//! before anything changes, and a number brought inside its range is
//! told in the answer.

use ordrumbox_core::SetField;
use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::song::clamp_bpm;

use super::*;
use crate::editors::note_range;
use crate::mcp::object;

/// Track settings a tool does not set (`mcp.rs` does not offer them).
const NOT_EDITED: [&str; 1] = ["name"];

fn count(n: usize) -> Value {
    Value::from(n as f64)
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|s| Value::from(s.as_str())).collect())
}

/// Argument `key`, which must be a string.
fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    optional_text(args, key)?.ok_or_else(|| format!("{key} is required"))
}

fn optional_text<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("{key}: not a string ({other})")),
    }
}

fn optional_number(args: &Value, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) if n.is_finite() => Ok(Some(*n)),
        Some(other) => Err(format!("{key}: not a number ({other})")),
    }
}

/// A whole number from 0 (a step, a measure, an index).
fn optional_whole(args: &Value, key: &str) -> Result<Option<u32>, String> {
    match optional_number(args, key)? {
        None => Ok(None),
        Some(n) if n >= 0.0 && n.fract() == 0.0 && n <= f64::from(u32::MAX) => Ok(Some(n as u32)),
        Some(n) => Err(format!("{key}: {n} is not a whole number from 0")),
    }
}

fn optional_bool(args: &Value, key: &str) -> Result<Option<bool>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(*b)),
        Some(other) => Err(format!("{key}: not true or false ({other})")),
    }
}

fn list<'a>(args: &'a Value, key: &str) -> Result<&'a [Value], String> {
    match args.get(key) {
        Some(Value::Array(items)) => Ok(items),
        None | Some(Value::Null) => Err(format!("{key} is required")),
        Some(other) => Err(format!("{key}: not an array ({other})")),
    }
}

fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_uppercase() == b.trim().to_uppercase()
}

/// The note settings of `fields` (every key but the ones in `skip`), by
/// their current key, checked: known, of the right type, inside the note
/// editor's ranges.
fn note_fields(fields: &Object, skip: &[&str], at: &str) -> Result<Vec<(&'static str, Value)>, String> {
    let mut checked = Vec::new();
    for (key, value) in fields.iter().filter(|(key, _)| !skip.contains(key)) {
        let canonical = Note::canonical_key(key).filter(|k| !["beat", "step"].contains(k));
        let Some(canonical) = canonical else { return Err(format!("{at}.{key}: not a note setting")) };
        if Note::default().set_field(key, value) == SetField::WrongType {
            return Err(format!("{at}.{key}: wrong type ({value})"));
        }
        if let (Some(n), Some((min, max))) = (value.as_f64(), note_range(canonical))
            && !(min..=max).contains(&n)
        {
            return Err(format!("{at}.{key}: {n} is outside {min} to {max}"));
        }
        checked.push((canonical, value.clone()));
    }
    Ok(checked)
}

/// The track settings of `fields`, by their current key, checked: known
/// and of the right type (numbers are brought inside their range by the edit).
fn track_fields(fields: &Object, at: &str) -> Result<Vec<(&'static str, Value)>, String> {
    let mut checked = Vec::new();
    for (key, value) in fields.iter() {
        let canonical = Track::canonical_key(key).filter(|k| Track::is_setting(k) && !NOT_EDITED.contains(k));
        let Some(canonical) = canonical else { return Err(format!("{at}.{key}: not a track setting")) };
        if Track::default().set_field(key, value) == SetField::WrongType {
            return Err(format!("{at}.{key}: wrong type ({value})"));
        }
        checked.push((canonical, value.clone()));
    }
    Ok(checked)
}

/// A note as a tool shows it: its absolute step, and the fields away from
/// their default.
fn note_json(note: &Note, steps_per_beat: i32) -> Value {
    let defaults = Note::default();
    let mut object = Object::new();
    object.insert("step", Value::from(note.beat * steps_per_beat + note.step));
    for &key in Note::KEYS {
        let value = note.field(key).unwrap_or_default();
        if value != defaults.field(key).unwrap_or_default() {
            object.insert(key, value);
        }
    }
    Value::Object(object)
}

/// A track as a tool shows it: every setting, its notes.
fn track_json(track: &Track) -> Value {
    let mut object = Object::new();
    for &key in Track::KEYS {
        object.insert(key, track.field(key).unwrap_or_default());
    }
    let notes = track.notes.iter().map(|n| note_json(n, track.steps_per_beat)).collect();
    object.insert("notes", Value::Array(notes));
    Value::Object(object)
}

fn pattern_summary(index: usize, pattern: &Pattern) -> Value {
    object([
        ("index", count(index)),
        ("id", Value::from(pattern.id.as_str())),
        ("name", pattern.name.as_deref().map_or(Value::Null, Value::from)),
        ("bpm", Value::from(pattern.bpm)),
        ("beatCount", Value::from(pattern.beat_count)),
        ("trackCount", count(pattern.tracks.len())),
        ("tags", strings(&pattern.tags)),
    ])
}

impl Session {
    /// Start the MCP link: its port goes to `mcp.json`. A failure is a
    /// status line; the app works without it.
    pub fn start_mcp(&mut self, waker: update::Waker) {
        let Some(endpoint) = store::mcp_endpoint_path() else {
            self.status.push("MCP not available: no user data folder".into());
            return;
        };
        match mcp_link::Server::start(&endpoint, waker) {
            Ok(server) => self.mcp = Some(server),
            Err(e) => self.status.push(format!("MCP not available: {e}")),
        }
    }

    /// Run the MCP calls waiting; whether there was one.
    pub fn poll_mcp(&mut self) -> bool {
        let mut called = false;
        while let Some(request) = self.mcp.as_ref().and_then(mcp_link::Server::poll) {
            let outcome = self.mcp_call(&request.name, &request.arguments);
            request.answer(outcome);
            called = true;
        }
        called
    }

    /// Run tool `name` on `args` (an object); what it answers.
    pub(super) fn mcp_call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let revision = self.editor.revision();
        let outcome = match name {
            "listPatterns" => Ok(self.mcp_list_patterns()),
            "loadPattern" => self.mcp_pattern(text(args, "patternName")?).map(|p| {
                let pattern = &self.editor.library().patterns[p];
                let mut json = pattern_summary(p, pattern);
                if let Some(object) = json.as_object_mut() {
                    object.insert("description", Value::from(pattern.description.as_str()));
                    object.insert("tracks", Value::Array(pattern.tracks.iter().map(track_json).collect()));
                }
                json
            }),
            "createNewPattern" => self.mcp_create_pattern(args),
            "selectPattern" => self.mcp_select_pattern(args),
            "addNotesToPattern" => self.mcp_add_notes(args),
            "updateTrack" => self.mcp_update_track(args),
            "setPatternBpm" => self.mcp_set_bpm(args),
            "setPatternBeatCount" => self.mcp_set_beat_count(args),
            "setPatternTags" => self.mcp_set_tags(args),
            "setPatternDescription" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let description = text(args, "description")?;
                self.editor.set_pattern_description(p, description);
                Ok(object([("description", Value::from(description))]))
            }
            "savePatternToJson" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let dir = store::exports_dir().ok_or("no user data folder to export to")?;
                let pattern = self.editor.library().patterns.get(p).ok_or("No pattern selected")?;
                let path = dir.join(pattern_json_file_name(pattern));
                self.write_pattern_json(p, &path)?;
                Ok(object([("filePath", Value::from(path.display().to_string()))]))
            }
            "listAllInstrumentsNames" => Ok(Self::mcp_instruments()),
            "listKitSamples" => Ok(self.mcp_kits()),
            "selectKit" => {
                let wanted = text(args, "kit")?;
                let found = self.kits.kits.iter().position(|k| same_name(&k.name, wanted));
                let index = found.ok_or_else(|| format!("No kit named \"{wanted}\" (listKitSamples gives them)"))?;
                self.select_kit(index);
                self.message = format!("kit {}", self.kits.kits[index].name);
                Ok(object([("kit", Value::from(self.kits.kits[index].name.as_str()))]))
            }
            "analyzeSamples" => self.mcp_analyze(args),
            "listArrangements" => Ok(self.mcp_arrangements()),
            "createArrangement" => self.mcp_create_arrangement(args),
            "addPatternToArrangement" => self.mcp_add_clip(args),
            "removePatternFromArrangement" => self.mcp_remove_clips(args),
            "play" => self.mcp_play(args),
            "stop" => {
                if let Some(controller) = &mut self.controller {
                    controller.stop();
                }
                self.message = "stopped".into();
                Ok(object([("playing", Value::Bool(false))]))
            }
            "undo" => {
                self.undo();
                return Ok(object([("message", Value::from(self.message.as_str()))]));
            }
            _ => Err(format!("Unknown tool: {name}")),
        };
        if self.editor.revision() != revision {
            self.edited();
            self.clamp_selection();
            self.clamp_cursor_to_grid();
        }
        outcome
    }

    /// A pattern by id, else by name (trimmed, any case).
    fn mcp_pattern(&self, reference: &str) -> Result<usize, String> {
        let patterns = &self.editor.library().patterns;
        let by_id = patterns.iter().position(|p| p.id == reference.trim());
        by_id
            .or_else(|| patterns.iter().position(|p| p.name.as_deref().is_some_and(|n| same_name(n, reference))))
            .ok_or_else(|| format!("No pattern named \"{}\" (listPatterns gives them)", reference.trim()))
    }

    /// An arrangement by name (any case) or index, else the selected one.
    fn mcp_song(&self, args: &Value) -> Result<usize, String> {
        let songs = &self.editor.library().songs;
        let found = match args.get("arrangement") {
            None | Some(Value::Null) => Some(self.editor.selected_song())
                .filter(|&s| s < songs.len())
                .ok_or("No arrangement: createArrangement"),
            Some(Value::String(name)) => {
                songs.iter().position(|s| same_name(&s.name, name)).ok_or("No such arrangement")
            }
            Some(Value::Number(n)) if n.fract() == 0.0 && *n >= 0.0 && (*n as usize) < songs.len() => Ok(*n as usize),
            Some(_) => Err("No such arrangement"),
        };
        found.map_err(|e| format!("{e} (listArrangements gives them)"))
    }

    fn mcp_list_patterns(&self) -> Value {
        let patterns = self.editor.library().patterns.iter().enumerate();
        object([
            ("patterns", Value::Array(patterns.map(|(i, p)| pattern_summary(i, p)).collect())),
            ("shown", count(self.pattern_index)),
        ])
    }

    fn mcp_create_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let name = text(args, "patternName")?.trim();
        if name.is_empty() {
            return Err("patternName is empty".into());
        }
        if self.mcp_pattern(name).is_ok() {
            return Err(format!("The library has a pattern named \"{name}\" already"));
        }
        let p = self.editor.add_pattern(Some(name));
        Ok(pattern_summary(p, &self.editor.library().patterns[p]))
    }

    fn mcp_select_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let count = self.editor.library().patterns.len();
        let target = match (optional_text(args, "patternName")?, optional_whole(args, "index")?) {
            (Some(name), _) if !name.trim().is_empty() => Some(self.mcp_pattern(name)?),
            (_, Some(index)) if (index as usize) < count => Some(index as usize),
            (_, Some(index)) => return Err(format!("index: {index}, the library has {count} patterns")),
            _ => None,
        };
        if let Some(p) = target {
            self.show_pattern(p);
        }
        let pattern = self.pattern().ok_or("No pattern in the library")?;
        Ok(pattern_summary(self.pattern_index, pattern))
    }

    fn mcp_add_notes(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let pattern = &self.editor.library().patterns[p];
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
            if step / steps_per_beat as u32 >= ordrumbox_logic::editor::MAX_BEATS as u32 {
                let max = ordrumbox_logic::editor::MAX_BEATS;
                return Err(format!(
                    "{at}.step: {step} is past the {max} beats of a pattern ({steps_per_beat} steps a beat)"
                ));
            }
            planned.push((track, step, note_fields(fields, &["trackName", "step"], &at)?));
        }
        if pattern.tracks.len() + new_tracks.len() > MAX_TRACKS {
            return Err(format!("a pattern holds at most {MAX_TRACKS} tracks"));
        }
        let name = pattern_name(pattern);
        let desc = format!("Add {} notes to \"{name}\"", planned.len());
        let (mut created, mut updated) = (0, 0);
        self.editor.batch(&desc, |editor| {
            for (track, step, fields) in &planned {
                let t = match editor.library().patterns[p].tracks.iter().position(|t| same_name(&t.name, track)) {
                    Some(t) => t,
                    None => editor.add_track(p, track, DEFAULT_STEPS_PER_BEAT).ok_or("no room for a track")?,
                };
                let steps_per_beat = editor.library().patterns[p].tracks[t].steps_per_beat.max(1);
                let (beat, beat_step) = ((*step as i32) / steps_per_beat, (*step as i32) % steps_per_beat);
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
        let pattern = &self.editor.library().patterns[p];
        Ok(object([
            ("created", count(created)),
            ("updated", count(updated)),
            ("tracksAdded", strings(&new_tracks)),
            ("beatCount", Value::from(pattern.beat_count)),
        ]))
    }

    fn mcp_update_track(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let name = text(args, "trackName")?.trim().to_uppercase();
        let updates = args.get("updates").and_then(Value::as_object).ok_or("updates: not an object")?;
        let updates = track_fields(updates, "updates")?;
        let note_updates = match args.get("noteUpdates") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Object(fields)) => note_fields(fields, &[], "noteUpdates")?,
            Some(other) => return Err(format!("noteUpdates: not an object ({other})")),
        };
        let pattern = &self.editor.library().patterns[p];
        let existing = pattern.tracks.iter().position(|t| same_name(&t.name, &name));
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
        let track = &self.editor.library().patterns[p].tracks[t];
        let settings = updates.iter().map(|(key, _)| (*key, track.field(key).unwrap_or_default()));
        Ok(object([
            ("action", Value::from(if existing.is_some() { "updated" } else { "added" })),
            ("trackName", Value::from(track.name.as_str())),
            ("settings", object(settings)),
            ("notesUpdated", count(notes)),
        ]))
    }

    fn mcp_set_bpm(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let asked = optional_number(args, "bpm")?.ok_or("bpm is required")?;
        // As the tempo field: whole, inside the bounds.
        let bpm = clamp_bpm(asked.round());
        self.editor.set_pattern_bpm(p, bpm, false);
        let mut answer = object([("bpm", Value::from(bpm))]);
        if bpm != asked
            && let Some(object) = answer.as_object_mut()
        {
            object.insert("note", Value::from(format!("{asked} asked, {bpm} used (whole, 20 to 300)")));
        }
        Ok(answer)
    }

    fn mcp_set_beat_count(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let max = ordrumbox_logic::editor::MAX_BEATS;
        let beats = optional_whole(args, "beatCount")?.ok_or("beatCount is required")?;
        if !(1..=max as u32).contains(&beats) {
            return Err(format!("beatCount: {beats} is outside 1 to {max}"));
        }
        self.editor.set_pattern_beat_count(p, f64::from(beats));
        Ok(object([("beatCount", Value::from(self.editor.library().patterns[p].beat_count))]))
    }

    fn mcp_set_tags(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let tags = list(args, "tags")?
            .iter()
            .map(|t| t.as_str().map(str::to_owned).ok_or_else(|| format!("tags: {t} is not a string")));
        let tags = tags.collect::<Result<Vec<_>, _>>()?;
        self.editor.set_pattern_tags(p, &tags);
        Ok(object([("tags", strings(&self.editor.library().patterns[p].tags))]))
    }

    fn mcp_instruments() -> Value {
        let instruments = catalog().instruments.iter().map(|i| {
            object([
                ("id", Value::from(i.id.as_str())),
                ("drum", Value::Bool(i.drum)),
                ("pan", Value::from(i.pan.as_str())),
            ])
        });
        let instruments: Vec<Value> = instruments.collect();
        object([("count", count(instruments.len())), ("instruments", Value::Array(instruments))])
    }

    fn mcp_kits(&self) -> Value {
        let kits = self.kits.kits.iter().map(|kit| {
            let samples = kit.samples.iter().map(|s| {
                object([
                    ("key", Value::from(s.key.as_str())),
                    ("name", Value::from(s.display_name.as_str())),
                    ("path", Value::from(s.path.as_str())),
                ])
            });
            object([("name", Value::from(kit.name.as_str())), ("samples", Value::Array(samples.collect()))])
        });
        let current = self.kits.kits.get(self.kit_index).map_or(Value::Null, |k| Value::from(k.name.as_str()));
        object([("kit", current), ("kits", Value::Array(kits.collect()))])
    }

    fn mcp_analyze(&self, args: &Value) -> Result<Value, String> {
        let mut results = Vec::new();
        for item in list(args, "samples")? {
            let path = item.as_str().ok_or_else(|| format!("samples: {item} is not a string"))?;
            let result = match self.kits.buffer(path) {
                Some(buffer) => {
                    let a = analysis::analyze(buffer);
                    object([
                        ("samplePath", Value::from(path)),
                        ("durationSec", Value::from(a.duration_sec)),
                        ("peakDb", Value::from(a.peak_db)),
                        ("rmsDb", Value::from(a.rms_db)),
                        ("fundamentalHz", a.fundamental_hz.map_or(Value::Null, Value::from)),
                        ("pitchConfidence", Value::from(a.pitch_confidence)),
                        ("note", a.note.map_or(Value::Null, |n| Value::from(n.to_string()))),
                    ])
                }
                None => object([
                    ("samplePath", Value::from(path)),
                    ("error", Value::from("no sample of the kits has this path (listKitSamples gives them)")),
                ]),
            };
            results.push(result);
        }
        Ok(object([("results", Value::Array(results))]))
    }

    fn song_json(&self, s: usize) -> Value {
        let library = self.editor.library();
        let song = &library.songs[s];
        let clips = song.clips.iter().map(|clip| {
            let name = library.pattern(&clip.pattern_id).and_then(|p| p.name.as_deref());
            object([
                ("patternId", Value::from(clip.pattern_id.as_str())),
                ("patternName", name.map_or(Value::Null, Value::from)),
                ("startMeasure", Value::from(clip.start_measure)),
                ("measureCount", Value::from(clip.measure_count)),
            ])
        });
        object([
            ("index", count(s)),
            ("id", Value::from(song.id.as_str())),
            ("name", Value::from(song.name.as_str())),
            ("description", Value::from(song.description.as_str())),
            ("bpm", Value::from(song.bpm)),
            ("lengthMeasures", Value::from(song.length_measures())),
            ("clips", Value::Array(clips.collect())),
        ])
    }

    fn mcp_arrangements(&self) -> Value {
        let songs = (0..self.editor.library().songs.len()).map(|s| self.song_json(s));
        object([("arrangements", Value::Array(songs.collect())), ("selected", count(self.editor.selected_song()))])
    }

    /// A clip of `createArrangement`: its pattern, start and length.
    fn mcp_clip(&self, item: &Value, at: &str) -> Result<(String, u32, Option<f64>), String> {
        let p = self.mcp_pattern(text(item, "patternName").map_err(|e| format!("{at}.{e}"))?)?;
        let start = optional_whole(item, "startMeasure").map_err(|e| format!("{at}.{e}"))?.unwrap_or(0);
        let length = optional_number(item, "measureCount").map_err(|e| format!("{at}.{e}"))?;
        if length.is_some_and(|l| l <= 0.0) {
            return Err(format!("{at}.measureCount: not above 0"));
        }
        Ok((self.editor.library().patterns[p].id.clone(), start, length))
    }

    fn mcp_create_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let name = text(args, "name")?.trim();
        if name.is_empty() {
            return Err("name is empty".into());
        }
        let description = optional_text(args, "description")?;
        let bpm = optional_number(args, "bpm")?;
        let loop_measures = optional_number(args, "loopMeasures")?;
        if loop_measures.is_some_and(|l| l < 0.0) {
            return Err("loopMeasures: below 0".into());
        }
        let clips = match args.get("clips") {
            None | Some(Value::Null) => Vec::new(),
            Some(_) => {
                let items = list(args, "clips")?.iter().enumerate();
                items.map(|(i, item)| self.mcp_clip(item, &format!("clips[{i}]"))).collect::<Result<_, _>>()?
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
        self.song_scroll = (0, 0);
        let mut answer = self.song_json(s);
        let used = self.editor.library().songs[s].bpm;
        if let (Some(asked), Some(object)) = (bpm.filter(|&b| b != used), answer.as_object_mut()) {
            object.insert("note", Value::from(format!("bpm {asked} asked, {used} used (20 to 300)")));
        }
        Ok(answer)
    }

    fn mcp_add_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = self.mcp_song(args)?;
        let (id, start, length) = self.mcp_clip(args, "arguments")?;
        let clip = self.editor.add_pattern_at_measure(&id, start, length, Some(s))?;
        let clip = object([
            ("patternId", Value::from(clip.pattern_id.as_str())),
            ("startMeasure", Value::from(clip.start_measure)),
            ("measureCount", Value::from(clip.measure_count)),
        ]);
        Ok(object([("clip", clip), ("arrangement", self.song_json(s))]))
    }

    fn mcp_remove_clips(&mut self, args: &Value) -> Result<Value, String> {
        let s = self.mcp_song(args)?;
        let start = optional_whole(args, "startMeasure")?;
        let pattern = optional_text(args, "patternName")?.filter(|n| !n.trim().is_empty());
        if start.is_none() && pattern.is_none() {
            return Err("Give startMeasure, patternName, or both".into());
        }
        let id = pattern.map(|name| self.mcp_pattern(name)).transpose()?;
        let id = id.map(|p| self.editor.library().patterns[p].id.clone());
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
        Ok(object([("removed", count(removed)), ("arrangement", self.song_json(s))]))
    }

    fn mcp_play(&mut self, args: &Value) -> Result<Value, String> {
        let song = optional_bool(args, "song")?.unwrap_or(false);
        let Some(controller) = &mut self.controller else {
            return Err("no audio device: orDrumbox plays nothing".into());
        };
        controller.stop();
        if song {
            if self.editor.library().songs.is_empty() {
                return Err("No arrangement to play: createArrangement".into());
            }
            self.song_cursor = 0;
            self.toggle(Mode::Song(self.editor.selected_song()), 0);
        } else {
            self.toggle(Mode::Pattern(self.pattern_index), 0);
        }
        let library = self.editor.library();
        let playing = match song {
            true => library.songs.get(self.editor.selected_song()).map(|s| s.name.clone()),
            false => self.pattern().map(pattern_name),
        };
        self.message = format!("playing {}", playing.as_deref().unwrap_or_default());
        Ok(object([("playing", playing.map_or(Value::Null, Value::from))]))
    }
}

/// Steps a beat of a track added by a tool (as the grid's).
const DEFAULT_STEPS_PER_BEAT: i32 = 4;

/// Pattern `p` at least `beats` long: the next multiple of 4 beats (at most
/// 16). A track that looped over its whole length still does.
fn grow_pattern(editor: &mut Editor, p: usize, beats: i32) {
    let pattern = &editor.library().patterns[p];
    if beats <= pattern.beat_count {
        return;
    }
    let whole: Vec<usize> = (0..pattern.tracks.len())
        .filter(|&t| {
            let track = &pattern.tracks[t];
            track.loop_length == Some(track.beat_count * track.steps_per_beat)
        })
        .collect();
    let beats = ((beats + 3) / 4 * 4).min(ordrumbox_logic::editor::MAX_BEATS);
    editor.set_pattern_beat_count(p, f64::from(beats));
    for t in whole {
        let length = beats * editor.library().patterns[p].tracks[t].steps_per_beat;
        editor.update_track(p, t, &[("loopLength", Value::from(length))], None, false);
    }
}

/// A pattern's name, its id when it has none.
fn pattern_name(pattern: &Pattern) -> String {
    pattern.name.clone().unwrap_or_else(|| pattern.id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::json;

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":2}]}]},
                {"name":"jazz"}]}"#,
        )
        .unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    fn call(session: &mut Session, name: &str, args: &str) -> Result<Value, String> {
        session.mcp_call(name, &json::parse(args).unwrap())
    }

    fn get<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
        path.iter().fold(value, |v, key| v.get(key).unwrap_or_else(|| panic!("no {key} in {v}")))
    }

    #[test]
    fn notes_are_added_in_one_undo_step() {
        let mut s = session();
        let before = s.editor.library().clone();
        let answer = call(
            &mut s,
            "addNotesToPattern",
            r#"{"patternName":"ROCK","notes":[{"trackName":"kick","step":4,"velocity":0.5},
                {"trackName":"snare","step":20,"probability":0.5},{"trackName":"KICK","step":8}]}"#,
        )
        .unwrap();
        assert_eq!(answer.to_json(), r#"{"created":2,"updated":1,"tracksAdded":["SNARE"],"beatCount":8}"#);
        let pattern = &s.editor.library().patterns[0];
        assert_eq!(pattern.tracks[1].name, "SNARE");
        // KICK has no loop point; SNARE, added on 4 beats, looped over them
        // and loops over the 8 now.
        assert_eq!(pattern.tracks.iter().map(|t| t.loop_length).collect::<Vec<_>>(), [None, Some(32)]);
        assert_eq!((pattern.tracks[0].notes[2].beat, pattern.tracks[0].notes[2].velocity), (1, 0.5));
        assert_eq!((pattern.tracks[1].notes[0].beat, pattern.tracks[1].notes[0].probability), (5, 0.5));
        assert_eq!(s.editor.history().undo_len(), 1);
        assert_eq!(s.message, "Add 3 notes to \"rock\"");
        s.undo();
        assert_eq!(s.editor.library(), &before);
    }

    #[test]
    fn wrong_arguments_change_nothing() {
        let mut s = session();
        let refused = [
            ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":0,"velocity":2}]}"#),
            ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":0,"cycle":"x"}]}"#),
            ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":64}]}"#),
            ("addNotesToPattern", r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":1.5}]}"#),
            ("addNotesToPattern", r#"{"patternName":"pop","notes":[]}"#),
            ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"loudness":1}}"#),
            ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"name":"X"}}"#),
            ("updateTrack", r#"{"patternName":"rock","trackName":"KICK","updates":{"mute":1}}"#),
            ("setPatternBeatCount", r#"{"patternName":"rock","beatCount":20}"#),
            ("createNewPattern", r#"{"patternName":" Jazz "}"#),
            ("createArrangement", r#"{"name":"song","clips":[{"patternName":"pop"}]}"#),
            ("addPatternToArrangement", r#"{"patternName":"rock"}"#),
            ("setColorScheme", r#"{"scheme":2}"#),
        ];
        for (tool, args) in refused {
            assert!(call(&mut s, tool, args).is_err(), "{tool} {args}");
        }
        assert_eq!(s.editor.history().undo_len(), 0);
        let error = call(&mut s, "addNotesToPattern", refused[0].1).unwrap_err();
        assert_eq!(error, "notes[0].velocity: 2 is outside 0 to 1");
    }

    #[test]
    fn a_track_is_updated_or_added() {
        let mut s = session();
        let args = r#"{"patternName":"rock","trackName":"kick","updates":{"volume":3,"mute":true,"velocity":0.5},
            "noteUpdates":{"probability":0.25}}"#;
        let answer = call(&mut s, "updateTrack", args).unwrap();
        // `velocity`, the web version's key, is `volume`: the later one wins.
        assert_eq!(
            answer.to_json(),
            r#"{"action":"updated","trackName":"KICK","settings":{"volume":0.5,"mute":true},"notesUpdated":2}"#
        );
        assert!(s.editor.library().patterns[0].tracks[0].notes.iter().all(|n| n.probability == 0.25));
        let answer = call(&mut s, "updateTrack", r#"{"patternName":"rock","trackName":"chh","updates":{"pan":-4}}"#);
        assert_eq!(get(&answer.unwrap(), &["settings", "pan"]).as_f64(), Some(-1.0));
        assert_eq!(s.editor.library().patterns[0].tracks[1].name, "CHH");
        assert_eq!(s.editor.history().undo_len(), 2);
    }

    #[test]
    fn patterns_are_listed_shown_and_set() {
        let mut s = session();
        let created = call(&mut s, "createNewPattern", r#"{"patternName":"house"}"#).unwrap();
        assert_eq!(get(&created, &["index"]).as_f64(), Some(2.0));
        let shown = call(&mut s, "selectPattern", r#"{"patternName":"house"}"#).unwrap();
        assert_eq!((s.pattern_index, get(&shown, &["name"]).as_str()), (2, Some("house")));
        call(&mut s, "selectPattern", r#"{"index":1}"#).unwrap();
        assert_eq!(s.pattern_index, 1);
        assert!(call(&mut s, "selectPattern", r#"{"index":3}"#).is_err());
        let bpm = call(&mut s, "setPatternBpm", r#"{"patternName":"rock","bpm":128.4}"#).unwrap();
        assert_eq!(bpm.to_json(), r#"{"bpm":128,"note":"128.4 asked, 128 used (whole, 20 to 300)"}"#);
        call(&mut s, "setPatternTags", r#"{"patternName":"rock","tags":["dance"," "]}"#).unwrap();
        call(&mut s, "setPatternDescription", r#"{"patternName":"rock","description":"four on the floor"}"#).unwrap();
        let listed = call(&mut s, "listPatterns", "{}").unwrap();
        let rock = &get(&listed, &["patterns"]).as_array().unwrap()[0];
        assert_eq!(
            rock.to_json(),
            r#"{"index":0,"id":"rock","name":"rock","bpm":128,"beatCount":4,"trackCount":1,"tags":["dance"]}"#
        );
        let loaded = call(&mut s, "loadPattern", r#"{"patternName":"rock"}"#).unwrap();
        assert_eq!(get(&loaded, &["description"]).as_str(), Some("four on the floor"));
        let track = &get(&loaded, &["tracks"]).as_array().unwrap()[0];
        assert_eq!(get(track, &["notes"]).to_json(), r#"[{"step":0},{"step":8,"beat":2}]"#);
        assert_eq!(get(track, &["volume"]).as_f64(), Some(1.0));
    }

    #[test]
    fn arrangements_are_made_and_edited() {
        let mut s = session();
        let args = r#"{"name":"Song","bpm":500,"clips":[{"patternName":"rock"},{"patternName":"jazz","startMeasure":1,"measureCount":2}]}"#;
        let made = call(&mut s, "createArrangement", args).unwrap();
        assert_eq!(get(&made, &["bpm"]).as_f64(), Some(300.0));
        assert_eq!(get(&made, &["note"]).as_str(), Some("bpm 500 asked, 300 used (20 to 300)"));
        assert_eq!(get(&made, &["lengthMeasures"]).as_f64(), Some(3.0));
        assert_eq!(s.editor.history().undo_len(), 1);
        let added =
            call(&mut s, "addPatternToArrangement", r#"{"patternName":"ROCK","startMeasure":3,"arrangement":"song"}"#);
        assert_eq!(
            get(&added.unwrap(), &["clip"]).to_json(),
            r#"{"patternId":"rock","startMeasure":3,"measureCount":1}"#
        );
        let removed =
            call(&mut s, "removePatternFromArrangement", r#"{"patternName":"rock","arrangement":0}"#).unwrap();
        assert_eq!(get(&removed, &["removed"]).as_f64(), Some(2.0));
        assert!(call(&mut s, "removePatternFromArrangement", r#"{"startMeasure":9}"#).is_err());
        let listed = call(&mut s, "listArrangements", "{}").unwrap();
        let clips = get(&get(&listed, &["arrangements"]).as_array().unwrap()[0], &["clips"]).to_json();
        assert_eq!(clips, r#"[{"patternId":"jazz","patternName":"jazz","startMeasure":1,"measureCount":2}]"#);
    }
}
