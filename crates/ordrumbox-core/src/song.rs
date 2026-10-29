//! Songs (`src/model/song_schema.js`): arrangements that place patterns on a
//! timeline of 4/4 measures. Clips may overlap and may last a fraction of a
//! measure (a 3-beat pattern is 0.75 measure); only positions snap to whole
//! measures.

use std::collections::HashSet;

use crate::ids::{slugify, unique_id};
use crate::js;
use crate::json::{Object, Value};

/// Beats in one 4/4 measure.
pub const BEATS_PER_MEASURE: f64 = 4.0;
/// Tempo bounds accepted in a song file.
pub const SONG_MIN_BPM: f64 = 20.0;
pub const SONG_MAX_BPM: f64 = 300.0;
pub const DEFAULT_BPM: f64 = 120.0;

/// One placement of a pattern on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct SongClip {
    /// Pattern **id** (never an index or a name, which change).
    pub pattern_id: String,
    /// 0-based measure where the clip starts.
    pub start_measure: u32,
    /// Length in measures, > 0, possibly fractional.
    pub measure_count: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Song {
    pub id: String,
    pub name: String,
    pub description: String,
    /// One tempo for the whole song.
    pub bpm: f64,
    /// Explicit loop length in measures; `None` loops at the end of the last
    /// clip. Whole on load; an arrangement edit sets it to the end of the
    /// last clip, which may be fractional (as in the JS).
    pub loop_measures: Option<f64>,
    pub clips: Vec<SongClip>,
}

/// A clip left out on load because its pattern reference is unusable, kept so
/// the user can be told which one went stale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedClip {
    /// The referenced id, or `#index` when the clip has none.
    pub pattern: String,
    pub reason: &'static str,
}

/// Measures a pattern occupies: `beat_count / 4`, not floored (0.75 for 3 beats).
pub fn measures_for_pattern(beat_count: i32) -> f64 {
    if beat_count <= 0 { 1.0 } else { f64::from(beat_count) / BEATS_PER_MEASURE }
}

impl Song {
    /// End of the furthest clip, in measures (0 without clips).
    pub fn content_measures(&self) -> f64 {
        self.clips.iter().map(|clip| f64::from(clip.start_measure) + clip.measure_count).fold(0.0, f64::max)
    }

    /// Loop length: `loop_measure_count` floored (at
    /// least 1) when set, else [`Song::content_measures`].
    pub fn length_measures(&self) -> f64 {
        match self.loop_measures {
            Some(count) if count > 0.0 => count.floor().max(1.0),
            _ => self.content_measures(),
        }
    }

    /// Where transport `tick` is in the arrangement, in measures, fractional
    ///: wrapped on `loop_measure_count` as it is (not
    /// floored), else on [`Song::length_measures`]; 0 before the start.
    pub fn measure_at_tick(&self, tick: f64, ticks_per_beat: f64) -> f64 {
        let measure = tick / (ticks_per_beat * BEATS_PER_MEASURE);
        let total = self.loop_measures.unwrap_or_else(|| self.length_measures());
        if total <= 0.0 || measure <= 0.0 {
            return measure.max(0.0);
        }
        measure.rem_euclid(total)
    }

    /// Load one song as `normalizeSong()`: numbers coerced the JS way
    /// (`"bpm": "140"` is 140), positions floored at 0, clips referencing an
    /// unknown pattern id dropped and reported. `Err` when the song itself is
    /// unusable.
    pub fn from_json(raw: &Value, known_ids: &HashSet<String>) -> Result<(Self, Vec<DroppedClip>), &'static str> {
        let Some(object) = raw.as_object() else {
            return Err("Each song must be a JSON object");
        };
        let field = |key: &str| object.get(key).filter(|v| !v.is_null());
        let clips_raw = match field("clips") {
            None => &[][..],
            Some(Value::Array(items)) => items.as_slice(),
            Some(_) => return Err("\"clips\" must be an array"),
        };

        let mut clips = Vec::new();
        let mut dropped = Vec::new();
        for (index, raw_clip) in clips_raw.iter().enumerate() {
            let Some(clip) = raw_clip.as_object() else {
                dropped.push(DroppedClip { pattern: format!("#{index}"), reason: "clip is not an object" });
                continue;
            };
            // `pattern`: the JS key, older saves.
            let reference = ["patternId", "pattern"]
                .iter()
                .find_map(|key| clip.get(key).filter(|v| !v.is_null()))
                .map(js::to_string)
                .unwrap_or_default();
            let reference = reference.trim();
            if reference.is_empty() {
                dropped.push(DroppedClip { pattern: format!("#{index}"), reason: "clip has no pattern" });
                continue;
            }
            if !known_ids.contains(reference) {
                dropped.push(DroppedClip { pattern: reference.to_owned(), reason: "unknown pattern id" });
                continue;
            }
            let number = |key: &str| clip.get(key).map_or(f64::NAN, js::to_number);
            let start = number("startMeasure");
            let start = if start.is_nan() { 0.0 } else { start.floor().max(0.0) };
            let count = number("measureCount");
            clips.push(SongClip {
                pattern_id: reference.to_owned(),
                start_measure: start.min(f64::from(u32::MAX)) as u32,
                measure_count: if count > 0.0 { count } else { 1.0 },
            });
        }

        let text = |key: &str| field(key).map(js::to_string).unwrap_or_default();
        let bpm = field("bpm").map_or(f64::NAN, js::to_number);
        let loop_count =
            ["loopMeasures", "loopMeasureCount"].iter().find_map(|key| object.get(key)).map_or(f64::NAN, js::to_number);
        let id = match text("id").trim() {
            "" => Some(slugify(&text("name"))).filter(|slug| !slug.is_empty()).unwrap_or_else(|| "song".to_owned()),
            id => id.to_owned(),
        };
        let song = Self {
            id,
            name: text("name"),
            description: text("description"),
            bpm: if (SONG_MIN_BPM..=SONG_MAX_BPM).contains(&bpm) { bpm } else { DEFAULT_BPM },
            loop_measures: (loop_count > 0.0).then(|| loop_count.floor()),
            clips,
        };
        Ok((song, dropped))
    }

    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert("id", Value::from(self.id.as_str()));
        object.insert("name", Value::from(self.name.as_str()));
        object.insert("description", Value::from(self.description.as_str()));
        object.insert("bpm", Value::from(self.bpm));
        if let Some(count) = self.loop_measures {
            object.insert("loopMeasures", Value::from(count));
        }
        let clips = self.clips.iter().map(|clip| {
            let mut object = Object::new();
            object.insert("patternId", Value::from(clip.pattern_id.as_str()));
            object.insert("startMeasure", Value::from(clip.start_measure));
            object.insert("measureCount", Value::from(clip.measure_count));
            Value::Object(object)
        });
        object.insert("clips", Value::Array(clips.collect()));
        Value::Object(object)
    }
}

/// A clip sounding at a transport tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActiveClip {
    /// Index of the clip's pattern in the pattern list.
    pub pattern: usize,
    pub clip: usize,
    /// Repetition of the pattern since the clip started.
    pub loop_index: u64,
    /// Tick within the pattern.
    pub local_tick: u64,
    pub pattern_ticks: u64,
}

impl Song {
    /// The clips sounding at transport `tick` (`ticks_per_beat` per beat):
    /// the arrangement loops over [`Song::length_measures`]; a clip counts
    /// its repetitions from its start on the unwrapped transport.
    pub fn active_clips_at(&self, patterns: &[crate::Pattern], tick: u64, ticks_per_beat: u32) -> Vec<ActiveClip> {
        let per_measure = f64::from(ticks_per_beat) * BEATS_PER_MEASURE;
        let total = self.length_measures();
        if per_measure <= 0.0 || total <= 0.0 {
            return Vec::new();
        }
        let tick_f = tick as f64;
        let measures = (tick_f / per_measure) % total;
        let mut sources = Vec::new();
        for (index, clip) in self.clips.iter().enumerate() {
            let start = f64::from(clip.start_measure);
            let count = if clip.measure_count > 0.0 { clip.measure_count } else { 1.0 };
            if measures < start || measures >= start + count {
                continue;
            }
            // The last pattern with that id (a JS Map built in list order).
            let Some(pattern) = patterns.iter().rposition(|p| p.id == clip.pattern_id) else { continue };
            let beats = if patterns[pattern].beat_count > 0 { f64::from(patterns[pattern].beat_count) } else { 4.0 };
            let pattern_ticks = beats * f64::from(ticks_per_beat);
            let elapsed = (tick_f - start * per_measure).max(0.0);
            sources.push(ActiveClip {
                pattern,
                clip: index,
                loop_index: (elapsed / pattern_ticks).floor() as u64,
                local_tick: (elapsed % pattern_ticks).floor() as u64,
                pattern_ticks: pattern_ticks as u64,
            });
        }
        sources
    }
}

/// Load a `songs` array as `normalizeSongs()`: unusable songs skipped, a
/// repeated id renamed `<id>-alt` (then `-alt-2`…).
pub fn songs_from_json(raw: &Value, known_ids: &HashSet<String>) -> (Vec<Song>, Vec<DroppedClip>) {
    let Some(items) = raw.as_array() else { return (Vec::new(), Vec::new()) };
    let mut taken = HashSet::new();
    let mut songs = Vec::new();
    let mut dropped = Vec::new();
    for item in items {
        let Ok((mut song, song_dropped)) = Song::from_json(item, known_ids) else { continue };
        if taken.contains(&song.id) {
            song.id = unique_id(&format!("{}-alt", song.id), &mut taken);
        } else {
            taken.insert(song.id.clone());
        }
        dropped.extend(song_dropped);
        songs.push(song);
    }
    (songs, dropped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn ids(list: &[&str]) -> HashSet<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn coerces_and_clamps_like_normalize_song() {
        let raw = json::parse(
            r#"{"name":"Ébauche","bpm":"140","loopMeasureCount":7.9,
                "clips":[{"pattern":" a ","startMeasure":"2.7","measureCount":"0"},{"pattern":"b"},{}]}"#,
        )
        .unwrap();
        let (song, dropped) = Song::from_json(&raw, &ids(&["a"])).unwrap();
        assert_eq!(song.id, "ebauche");
        assert_eq!(song.bpm, 140.0);
        assert_eq!(song.loop_measures, Some(7.0));
        assert_eq!(song.clips, [SongClip { pattern_id: "a".into(), start_measure: 2, measure_count: 1.0 }]);
        assert_eq!(
            dropped,
            [
                DroppedClip { pattern: "b".into(), reason: "unknown pattern id" },
                DroppedClip { pattern: "#2".into(), reason: "clip has no pattern" },
            ]
        );
    }

    #[test]
    fn lengths() {
        let clip = |start, count| SongClip { pattern_id: "a".into(), start_measure: start, measure_count: count };
        let mut song = Song::from_json(&json::parse("{}").unwrap(), &HashSet::new()).unwrap().0;
        assert_eq!(song.length_measures(), 0.0);
        song.clips = vec![clip(8, 4.0), clip(2, 0.75)];
        assert_eq!(song.content_measures(), 12.0);
        song.loop_measures = Some(16.0);
        assert_eq!(song.length_measures(), 16.0);
        song.loop_measures = Some(0.75);
        assert_eq!(song.length_measures(), 1.0);
        assert_eq!(measures_for_pattern(3), 0.75);
    }

    #[test]
    fn renames_duplicate_ids() {
        let raw = json::parse(r#"[{"id":"x"},{"id":"x"},{"id":"x"},"bad",{"clips":1}]"#).unwrap();
        let (songs, _) = songs_from_json(&raw, &HashSet::new());
        let ids: Vec<_> = songs.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["x", "x-alt", "x-alt-2"]);
    }
}
