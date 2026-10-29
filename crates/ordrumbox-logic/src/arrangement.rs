//! The song (arrangement) commands (`song_commands.js`): clips placed and
//! removed, arrangements added, removed and selected, each edit one undo
//! step.
//!
//! Clips reference a pattern by its id; the placement commands take an id or
//! a name and report what they could not do (the JS `reportUserError`
//! messages). Every clip edit moves the arrangement's loop onto the end of
//! its last clip, so the grid and the playback never show
//! or play measures nothing is placed on.

use ordrumbox_core::song::{DEFAULT_BPM, clamp_bpm, measures_for_pattern};
use ordrumbox_core::{Song, SongClip};

use super::*;

/// The loop length after a clip edit: the end of the last clip, none
/// without clips.
fn follow_content(song: &mut Song) {
    let measures = song.content_measures();
    song.loop_measures = (measures > 0.0).then_some(measures);
}

impl Editor {
    /// The arrangement the song panel edits.
    pub fn selected_song(&self) -> usize {
        self.selected_song
    }

    /// Select arrangement `index`, clamped to the list (`setSelectedSongIdx`;
    /// not an undo step); the index selected.
    pub fn set_selected_song(&mut self, index: usize) -> usize {
        self.selected_song = index.min(self.library.songs.len().saturating_sub(1));
        self.selected_song
    }

    /// Set a field of the library header (`songInfos`: `name`,
    /// `description`), as the song panel does: not an undo step.
    pub fn set_library_info(&mut self, key: &str, value: &str) -> bool {
        if self.library.info.get(key).and_then(Value::as_str) == Some(value) {
            return false;
        }
        self.library.info.insert(key, Value::from(value));
        self.revision += 1;
        true
    }

    /// Arrangement `song`, else the selected one, if it exists.
    fn song_index(&self, song: Option<usize>) -> Option<usize> {
        let index = song.unwrap_or(self.selected_song);
        (index < self.library.songs.len()).then_some(index)
    }

    /// `#requireSong`: the arrangement to edit, or what to tell the user.
    fn require_song(&self, song: Option<usize>) -> Result<usize, String> {
        self.song_index(song).ok_or_else(|| "No arrangement to edit — create a song first".to_owned())
    }

    /// `#patternByRef`: a pattern by id, else by name (trimmed, any case).
    fn pattern_by_ref(&self, reference: &str) -> Option<&Pattern> {
        let needle = reference.trim();
        if needle.is_empty() {
            return None;
        }
        let patterns = &self.library.patterns;
        patterns.iter().find(|p| p.id == needle).or_else(|| {
            let upper = needle.to_uppercase();
            patterns.iter().find(|p| p.name.as_deref().unwrap_or_default().trim().to_uppercase() == upper)
        })
    }

    /// Run `edit` on the arrangements and record it when it changed
    /// something; `select`: the arrangement undo and redo select.
    fn edit_songs<R>(
        &mut self,
        desc: String,
        select: Option<(usize, usize)>,
        edit: impl FnOnce(&mut Vec<Song>) -> R,
    ) -> R {
        let before = self.library.songs.clone();
        let result = edit(&mut self.library.songs);
        if self.library.songs != before {
            let change = Change::Songs { before, after: self.library.songs.clone(), select };
            self.record(desc, change, None);
            self.revision += 1;
        }
        result
    }

    // ─── Clips ───────────────────────────────────────────────────────────────

    /// Place `clip` in arrangement `song` (else the selected one): a length
    /// of 0 or less is 1. Whether there was an arrangement.
    pub fn add_song_clip(&mut self, song: Option<usize>, clip: SongClip) -> bool {
        let Some(s) = self.song_index(song) else { return false };
        if clip.pattern_id.is_empty() {
            return false;
        }
        let measure_count = if clip.measure_count > 0.0 { clip.measure_count } else { 1.0 };
        let clip = SongClip { measure_count, ..clip };
        let desc = format!("Add \"{}\" at measure {}", clip.pattern_id, u64::from(clip.start_measure) + 1);
        self.edit_songs(desc, None, |songs| {
            songs[s].clips.push(clip);
            follow_content(&mut songs[s]);
        });
        true
    }

    /// Remove the clips at `indices` (out of range ones ignored) in one undo
    /// step; whether one was removed.
    pub fn remove_song_clips(&mut self, song: Option<usize>, indices: &[usize]) -> bool {
        let Some(s) = self.song_index(song) else { return false };
        let clips = &self.library.songs[s].clips;
        let mut targets: Vec<usize> = indices.iter().copied().filter(|&i| i < clips.len()).collect();
        targets.sort_unstable_by(|a, b| b.cmp(a));
        targets.dedup();
        let desc = match targets.as_slice() {
            [] => return false,
            [one] => format!("Remove clip \"{}\"", clips[*one].pattern_id),
            many => format!("Remove {} clips", many.len()),
        };
        self.edit_songs(desc, None, |songs| {
            for &i in &targets {
                songs[s].clips.remove(i);
            }
            follow_content(&mut songs[s]);
        });
        true
    }

    /// Place a pattern (an id or a name) at `start` (0-based measure), for
    /// `measure_count` measures, else the pattern's own length
    ///; the clip added.
    pub fn add_pattern_at_measure(
        &mut self,
        reference: &str,
        start: u32,
        measure_count: Option<f64>,
        song: Option<usize>,
    ) -> Result<SongClip, String> {
        let s = self.require_song(song)?;
        let Some(pattern) = self.pattern_by_ref(reference) else {
            return Err(format!("No pattern named \"{}\" in the library", reference.trim()));
        };
        let duration = match measure_count {
            Some(count) if count.is_finite() && count > 0.0 => count,
            _ => measures_for_pattern(pattern.beat_count),
        };
        let clip = SongClip { pattern_id: pattern.id.clone(), start_measure: start, measure_count: duration };
        self.add_song_clip(Some(s), clip.clone());
        Ok(clip)
    }

    /// Repeat the clip starting at `start` right after itself, as long
    ///; the clip added.
    pub fn repeat_pattern_at_measure(&mut self, start: u32, song: Option<usize>) -> Result<SongClip, String> {
        let s = self.require_song(song)?;
        let Some(clip) = self.library.songs[s].clips.iter().find(|c| c.start_measure == start).cloned() else {
            return Err(format!("No clip at measure {}", u64::from(start) + 1));
        };
        // `startMeasure + measureCount`, floored by `addSongClip`.
        let next = (f64::from(clip.start_measure) + clip.measure_count).floor().min(f64::from(u32::MAX)) as u32;
        self.add_pattern_at_measure(&clip.pattern_id, next, Some(clip.measure_count), Some(s))
    }

    /// Remove every clip starting at `start`, in one undo step
    ///; the clips removed.
    pub fn remove_pattern_at_measure(&mut self, start: u32, song: Option<usize>) -> Result<Vec<SongClip>, String> {
        let s = self.require_song(song)?;
        let clips = &self.library.songs[s].clips;
        let indices: Vec<usize> = (0..clips.len()).filter(|&i| clips[i].start_measure == start).collect();
        if indices.is_empty() {
            return Err(format!("No clip at measure {}", u64::from(start) + 1));
        }
        let removed = indices.iter().map(|&i| clips[i].clone()).collect();
        self.remove_song_clips(Some(s), &indices);
        Ok(removed)
    }

    /// Remove every clip of a pattern (an id or a name; an id the library no
    /// longer knows too, for an orphaned row), in one undo step
    ///; the clips removed.
    pub fn remove_pattern_clips(&mut self, reference: &str, song: Option<usize>) -> Result<Vec<SongClip>, String> {
        let s = self.require_song(song)?;
        let id = match self.pattern_by_ref(reference) {
            Some(pattern) => pattern.id.clone(),
            None => reference.trim().to_owned(),
        };
        if id.is_empty() {
            return Err("No pattern given to remove".to_owned());
        }
        let clips = &self.library.songs[s].clips;
        let indices: Vec<usize> = (0..clips.len()).filter(|&i| clips[i].pattern_id == id).collect();
        let removed = indices.iter().map(|&i| clips[i].clone()).collect();
        self.remove_song_clips(Some(s), &indices);
        Ok(removed)
    }

    // ─── Arrangements ────────────────────────────────────────────────────────

    /// Append an empty arrangement named `name` (trimmed; empty: "Untitled")
    /// with a unique id, and select it; its index.
    pub fn add_song(&mut self, name: &str) -> usize {
        let name = match name.trim() {
            "" => "Untitled",
            name => name,
        };
        let mut taken: HashSet<String> = self.library.songs.iter().map(|s| s.id.clone()).collect();
        let slug = slugify(name);
        let song = Song {
            id: unique_id(if slug.is_empty() { "song" } else { &slug }, &mut taken),
            name: name.to_owned(),
            description: String::new(),
            bpm: DEFAULT_BPM,
            loop_measures: None,
            clips: Vec::new(),
        };
        let index = self.library.songs.len();
        let previous = self.selected_song.min(index.saturating_sub(1));
        let desc = format!("Add arrangement \"{name}\"");
        self.edit_songs(desc, Some((previous, index)), |songs| songs.push(song));
        self.selected_song = index;
        index
    }

    /// Set the tempo (brought inside the tempo bounds), the description and the
    /// loop length (0: the arrangement loops at the end of its last clip) of
    /// arrangement `song`, else the selected one; what is `None` stays.
    pub fn set_song_settings(
        &mut self,
        song: Option<usize>,
        bpm: Option<f64>,
        description: Option<&str>,
        loop_measures: Option<f64>,
    ) -> Result<(), String> {
        let s = self.require_song(song)?;
        let desc = format!("Set \"{}\"", self.library.songs[s].name);
        self.edit_songs(desc, None, |songs| {
            let song = &mut songs[s];
            if let Some(bpm) = bpm.filter(|b| b.is_finite()) {
                song.bpm = clamp_bpm(bpm);
            }
            if let Some(description) = description {
                description.clone_into(&mut song.description);
            }
            if let Some(count) = loop_measures.filter(|c| c.is_finite()) {
                song.loop_measures = (count > 0.0).then_some(count);
            }
        });
        Ok(())
    }

    /// Replace the whole library (`applyToAppState`: a song loaded or
    /// imported), arrangement `selected_song` selected (clamped); one undo step.
    pub fn load_library(&mut self, library: Library, selected_song: usize) {
        let previous = self.selected_song;
        let before = Box::new(std::mem::replace(&mut self.library, library));
        self.set_selected_song(selected_song);
        let change =
            Change::Library { before, after: Box::new(self.library.clone()), select: (previous, self.selected_song) };
        self.record("Load song".to_owned(), change, None);
        self.revision += 1;
    }

    /// Remove arrangement `index` (else the selected one); the selection
    /// stays where it was, inside the list. Whether one was removed.
    pub fn remove_song(&mut self, index: Option<usize>) -> bool {
        let index = index.unwrap_or(self.selected_song);
        let Some(song) = self.library.songs.get(index) else { return false };
        let desc = format!("Remove arrangement \"{}\"", song.name);
        let after = self.selected_song.min(self.library.songs.len().saturating_sub(2));
        self.edit_songs(desc, Some((index, after)), |songs| songs.remove(index));
        self.selected_song = after;
        true
    }
}
