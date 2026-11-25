//! The tools on arrangements: list, create, rename and update one, and add,
//! remove, move or repeat its clips.

use super::Tools;
use super::helpers::*;
use ordrumbox_core::json::Value;

impl Tools<'_> {
    pub(super) fn list_arrangements(&self) -> Value {
        let library = self.library();
        let songs = (0..library.songs.len()).map(|s| song_json(library, s));
        object([("arrangements", Value::Array(songs.collect())), ("selected", count(self.editor.selected_song()))])
    }

    /// A clip of `createArrangement`: its pattern, start and length.
    pub(super) fn clip(&self, item: &Value, at: &str) -> Result<(String, u32, Option<f64>), String> {
        let p = self.pattern(text(item, "patternName").map_err(|e| format!("{at}.{e}"))?)?;
        let start = optional_whole(item, "startMeasure").map_err(|e| format!("{at}.{e}"))?.unwrap_or(0);
        let length = optional_number(item, "measureCount").map_err(|e| format!("{at}.{e}"))?;
        if length.is_some_and(|l| l <= 0.0) {
            return Err(format!("{at}.measureCount: not above 0"));
        }
        Ok((self.library().patterns[p].id.clone(), start, length))
    }

    pub(super) fn create_arrangement(&mut self, args: &Value) -> Result<Value, String> {
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
    pub(super) fn song_json_with_bpm(&self, s: usize, asked: Option<f64>) -> Value {
        let mut answer = song_json(self.library(), s);
        let used = self.library().songs[s].bpm;
        if let (Some(asked), Some(object)) = (asked.filter(|&b| b != used), answer.as_object_mut()) {
            object.insert("note", Value::from(format!("bpm {asked} asked, {used} used (20 to 300)")));
        }
        answer
    }

    pub(super) fn add_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let (id, start, length) = self.clip(args, "arguments")?;
        let clip = self.editor.add_pattern_at_measure(&id, start, length, Some(s))?;
        Ok(object([("clip", clip_json(&clip)), ("arrangement", song_json(self.library(), s))]))
    }

    pub(super) fn remove_clips(&mut self, args: &Value) -> Result<Value, String> {
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

    pub(super) fn update_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let (description, bpm, loop_measures) = song_settings(args)?;
        if description.is_none() && bpm.is_none() && loop_measures.is_none() {
            return Err("Give description, bpm, loopMeasures, or several".into());
        }
        self.editor.set_song_settings(Some(s), bpm, description, loop_measures)?;
        Ok(self.song_json_with_bpm(s, bpm))
    }

    pub(super) fn rename_arrangement(&mut self, args: &Value) -> Result<Value, String> {
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

    pub(super) fn move_clip(&mut self, args: &Value) -> Result<Value, String> {
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

    pub(super) fn repeat_clip(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(self.editor, args)?;
        let start = optional_whole(args, "startMeasure")?.ok_or("startMeasure is required")?;
        let clip = self.editor.repeat_pattern_at_measure(start, Some(s))?;
        Ok(object([("clip", clip_json(&clip)), ("arrangement", song_json(self.library(), s))]))
    }
}
