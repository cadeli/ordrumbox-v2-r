//! Editing tracks: add, move, paste, settings, loop, sound, name.

use super::{Editor, apply_updates, clean, pattern_name};
use ordrumbox_core::Track;
use ordrumbox_core::drum_taxonomy::mono_from_track_name;
use ordrumbox_core::json::Value;
use ordrumbox_core::track::{MAX_STEPS_PER_BEAT, MAX_TRACKS};
use std::collections::HashSet;

impl Editor {
    /// Whether pattern `p` can take one more track ([`MAX_TRACKS`]).
    pub fn has_track_room(&self, p: usize) -> bool {
        self.library.patterns.get(p).is_some_and(|pattern| pattern.tracks.len() < MAX_TRACKS)
    }

    /// Append a new track; its index. `None` when the pattern is full.
    pub fn add_track(&mut self, p: usize, name: &str, steps_per_beat: i32) -> Option<usize> {
        if !self.has_track_room(p) {
            return None;
        }
        let desc = format!("Add track {name}");
        self.edit(
            p,
            |_| desc,
            None,
            |pattern| {
                pattern.tracks.push(Track::new(name, pattern.beat_count, steps_per_beat));
                pattern.tracks.len() - 1
            },
        )
    }

    pub fn remove_track(&mut self, p: usize, t: usize) -> bool {
        let Some(name) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)).map(|t| t.name.clone()) else {
            return false;
        };
        self.edit(p, |_| format!("Remove track {name}"), None, |pattern| pattern.tracks.remove(t)).is_some()
    }

    /// Move track `from` so that it ends at index `to` (clamped), the others
    /// keeping their order; whether it moved.
    pub fn move_track(&mut self, p: usize, from: usize, to: usize) -> bool {
        let Some(pattern) = self.library.patterns.get(p) else { return false };
        let Some(name) = pattern.tracks.get(from).map(|t| t.name.clone()) else { return false };
        let to = to.min(pattern.tracks.len() - 1);
        if to == from {
            return false;
        }
        self.edit(
            p,
            |_| format!("Move track {name}"),
            None,
            |pattern| {
                let track = pattern.tracks.remove(from);
                pattern.tracks.insert(to, track);
            },
        )
        .is_some()
    }

    /// Insert a copy of `source` at `index` (clamped), named "`<name>` copy"
    /// (then "copy 2"…); its index. `None` when the pattern is full.
    pub fn paste_track(&mut self, p: usize, index: usize, source: &Track) -> Option<usize> {
        if !self.has_track_room(p) {
            return None;
        }
        let taken: HashSet<&str> = self.library.patterns.get(p)?.tracks.iter().map(|t| t.name.as_str()).collect();
        let mut name = format!("{} copy", source.name);
        let mut n = 2;
        while taken.contains(name.as_str()) {
            name = format!("{} copy {n}", source.name);
            n += 1;
        }
        let desc = format!("Paste track {name}");
        self.edit(
            p,
            |_| desc,
            None,
            |pattern| {
                let index = index.min(pattern.tracks.len());
                pattern.tracks.insert(index, Track { name, ..source.clone() });
                index
            },
        )
    }

    /// Set track settings by JSON key: only track settings,
    /// numbers clamped to their range, non-finite numbers and wrong types
    /// ignored. With `coalesce`, repeated edits of the same settings of the
    /// same track are one undo step.
    pub fn update_track(
        &mut self,
        p: usize,
        t: usize,
        updates: &[(&str, Value)],
        desc: Option<&str>,
        coalesce: bool,
    ) -> bool {
        let mut keys: Vec<&str> = updates.iter().map(|(k, _)| *k).filter(|k| Track::is_setting(k)).collect();
        keys.sort_unstable();
        let coalesce = coalesce.then(|| format!("track:{p}:{t}:{}", keys.join(",")));
        self.edit_track(
            p,
            t,
            |track| desc.map_or_else(|| format!("Update {}", track.name), str::to_owned),
            coalesce,
            |track| apply_updates(track, updates),
        )
        .is_some()
    }

    /// [`Self::update_track`] on every track of pattern `p`, `updates(t, track)`
    /// for each, as one undo step (the JS `recordTransaction`).
    pub fn update_tracks(
        &mut self,
        p: usize,
        desc: &str,
        mut updates: impl FnMut(usize, &Track) -> Vec<(&'static str, Value)>,
    ) -> bool {
        self.edit(
            p,
            |_| desc.to_owned(),
            None,
            |pattern| {
                for (t, track) in pattern.tracks.iter_mut().enumerate() {
                    let updates = updates(t, track);
                    apply_updates(track, &updates);
                }
            },
        )
        .is_some()
    }

    /// Flip a boolean setting (`mute`, `solo`…).
    pub fn toggle_track(&mut self, p: usize, t: usize, key: &str) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let on = track.field(key).and_then(|v| v.as_bool()) != Some(true);
        let desc = format!("{key} on {}", track.name);
        self.update_track(p, t, &[(key, Value::Bool(on))], Some(&desc), false)
    }

    /// `setStepsPerBeat`: clamped to 1–8; notes keep their position in the
    /// beat (by percent), the loop point stays inside the track.
    pub fn set_steps_per_beat(&mut self, p: usize, t: usize, value: f64, coalesce: bool) -> bool {
        let target = value.clamp(1.0, MAX_STEPS_PER_BEAT as f64).round() as i32;
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        if !value.is_finite() || target == track.steps_per_beat {
            return false;
        }
        let coalesce = coalesce.then(|| format!("track:{p}:{t}:stepsPerBeat"));
        self.edit_track(
            p,
            t,
            |track| format!("Steps per beat on {}", track.name),
            coalesce,
            |track| {
                let old = track.steps_per_beat;
                track.steps_per_beat = target;
                for note in &mut track.notes {
                    let percent = note.step_percent(old);
                    note.step_percent = Some(percent);
                    note.step = ((percent / 100.0 * f64::from(target)).round() as i32).min(target - 1);
                }
                let max_steps = track.beat_count * target;
                if track.loop_length.is_some_and(|l| l > max_steps) {
                    track.loop_length = Some(max_steps);
                }
            },
        )
        .is_some()
    }

    /// Next grid: 1 → 2 … 8 → 1.
    pub fn incr_steps_per_beat(&mut self, p: usize, t: usize) -> bool {
        let Some(track) = self.library.patterns.get(p).and_then(|p| p.tracks.get(t)) else { return false };
        let next = if track.steps_per_beat >= 8 { 1 } else { track.steps_per_beat + 1 };
        self.set_steps_per_beat(p, t, f64::from(next), false)
    }

    /// The loop one step shorter, wrapping to the track end.
    pub fn shorten_loop(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Shorten loop on {}", track.name),
            None,
            |track| {
                let full = track.steps_per_beat * track.beat_count;
                let next = track.loop_length.unwrap_or(full) - 1;
                track.loop_length = Some(if next < 1 { full } else { next });
            },
        )
        .is_some()
    }

    /// No notes, loop over the whole track.
    /// Shorten track `t` to its shortest loop, when its notes repeat one
    ///; whether it changed.
    pub fn compact_track(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(p, t, |track| format!("Compact {}", track.name), None, Track::compact_loop).unwrap_or(false)
    }

    /// Pattern > Compact tracks (the tools panel's button): every track of
    /// pattern `p`, one undo step each; how many changed.
    pub fn compact_pattern(&mut self, p: usize) -> usize {
        let tracks = self.library.patterns.get(p).map_or(0, |pattern| pattern.tracks.len());
        (0..tracks).filter(|&t| self.compact_track(p, t)).count()
    }

    pub fn clean_track(&mut self, p: usize, t: usize) -> bool {
        self.edit_track(p, t, |track| format!("Clean {}", track.name), None, clean).is_some()
    }

    /// Every track cleaned, as one step (the JS records one per track).
    pub fn clean_pattern(&mut self, p: usize) -> bool {
        self.edit(
            p,
            |pattern| format!("Clean \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.tracks.iter_mut().for_each(clean),
        )
        .is_some()
    }

    /// Play sample `sample_id`: auto-assign and synth off.
    pub fn change_track_sound(&mut self, p: usize, t: usize, sample_id: &str) -> bool {
        self.edit_track(
            p,
            t,
            |track| format!("Sound on {}", track.name),
            None,
            |track| {
                track.sample_path = sample_id.to_owned();
                track.auto_assign = false;
                track.use_synth = false;
            },
        )
        .is_some()
    }

    /// Rename track `t`; a track renamed to a bass becomes mono. The tracks
    /// of the pattern that took it as their sidechain key follow its name, in
    /// the same undo step.
    pub fn change_track_name(&mut self, p: usize, t: usize, name: &str) -> bool {
        let Some(pattern) = self.library().patterns.get(p) else { return false };
        let Some(old) = pattern.tracks.get(t).map(|track| track.name.clone()) else { return false };
        let keyed: Vec<usize> = pattern
            .tracks
            .iter()
            .enumerate()
            .filter(|(i, track)| *i != t && track.sidechain_source == old)
            .map(|(i, _)| i)
            .collect();
        self.batch(&format!("Rename track → {name}"), |editor| {
            let renamed = editor
                .edit_track(
                    p,
                    t,
                    |_| format!("Rename track → {name}"),
                    None,
                    |track| {
                        track.name = name.to_owned();
                        track.mono |= mono_from_track_name(name);
                    },
                )
                .is_some();
            for other in keyed {
                editor.edit_track(p, other, |_| String::new(), None, |track| track.sidechain_source = name.to_owned());
            }
            renamed
        })
    }
}
