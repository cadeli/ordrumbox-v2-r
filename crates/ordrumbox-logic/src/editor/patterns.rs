//! Editing patterns and the song settings: add, move, rename, tempo, length, tags.

use super::*;

impl Editor {
    /// A new empty pattern (`createPattern`; default name `NewPat_<count>`)
    /// with an id no pattern of the library has.
    pub(super) fn blank_pattern(&self, name: Option<&str>) -> Pattern {
        let name = name.map_or_else(|| format!("NewPat_{}", self.library.patterns.len()), str::to_owned);
        let mut taken: HashSet<String> = self.library.patterns.iter().map(|p| p.id.clone()).collect();
        Pattern {
            id: unique_id(&slugify(&name), &mut taken),
            name: Some(name),
            beat_count: DEFAULT_BEAT_COUNT,
            ..Pattern::default()
        }
    }

    /// Append an empty pattern (`addPattern`; default name `NewPat_<count>`)
    /// with a unique id; its index.
    pub fn add_pattern(&mut self, name: Option<&str>) -> usize {
        let pattern = self.blank_pattern(name);
        let desc = format!("Add pattern \"{}\"", pattern_name(&pattern));
        self.edit_patterns(desc, |patterns| {
            patterns.push(pattern);
            patterns.len() - 1
        })
    }

    /// Append a copy of pattern `p` named "`<name>` copy", with an id made
    /// from that name; its index.
    pub fn duplicate_pattern(&mut self, p: usize) -> Option<usize> {
        let source = self.library.patterns.get(p)?;
        let name = format!("{} copy", source.name.as_deref().unwrap_or("Pattern"));
        let mut taken: HashSet<String> = self.library.patterns.iter().map(|p| p.id.clone()).collect();
        let copy = Pattern { id: unique_id(&slugify(&name), &mut taken), name: Some(name.clone()), ..source.clone() };
        Some(self.edit_patterns(format!("Duplicate pattern → \"{name}\""), |patterns| {
            patterns.push(copy);
            patterns.len() - 1
        }))
    }

    /// Remove pattern `index`, unless it is the last one.
    pub fn remove_pattern(&mut self, index: usize) -> bool {
        if self.library.patterns.len() <= 1 || index >= self.library.patterns.len() {
            return false;
        }
        let name = pattern_name(&self.library.patterns[index]).to_owned();
        self.edit_patterns(format!("Remove pattern \"{name}\""), |patterns| patterns.remove(index));
        true
    }

    /// Move pattern `from` so that it ends at index `to` (clamped), the others
    /// keeping their order; whether it moved. Songs name patterns by id, so
    /// only the order of the list and of the arrangement rows changes.
    pub fn move_pattern(&mut self, from: usize, to: usize) -> bool {
        let Some(pattern) = self.library.patterns.get(from) else { return false };
        let to = to.min(self.library.patterns.len() - 1);
        if to == from {
            return false;
        }
        let desc = format!("Move pattern \"{}\"", pattern_name(pattern));
        self.edit_patterns(desc, |patterns| {
            let pattern = patterns.remove(from);
            patterns.insert(to, pattern);
        });
        true
    }

    /// Rename (trimmed; an empty name keeps the old one). The id stays.
    pub fn rename_pattern(&mut self, p: usize, name: &str) -> bool {
        let name = name.trim();
        self.edit(
            p,
            |pattern| format!("Rename pattern → \"{}\"", pattern_name(pattern)),
            None,
            |pattern| {
                if !name.is_empty() {
                    pattern.name = Some(name.to_owned());
                }
            },
        )
        .is_some()
    }

    /// The library's tempo, every pattern's (inside the tempo bounds; 0 or
    /// not a number sets the default 120). With `coalesce`, a run of changes
    /// is one step. Whether it changed.
    pub fn set_bpm(&mut self, bpm: f64, coalesce: bool) -> bool {
        let bpm = if bpm.is_finite() && bpm != 0.0 { clamp_bpm(bpm) } else { DEFAULT_BPM };
        let before = self.library.bpm;
        if bpm == before {
            return false;
        }
        self.library.bpm = bpm;
        let coalesce = coalesce.then(|| "bpm".to_owned());
        self.record(format!("Set BPM → {bpm}"), Change::Tempo { before, after: bpm }, coalesce);
        self.revision += 1;
        true
    }

    /// Length in beats, 1–16 (out of range: 4); every track follows and its
    /// loop point stays inside.
    pub fn set_pattern_beat_count(&mut self, p: usize, beats: f64) -> bool {
        let requested = beats.round();
        let applied = if requested.is_finite() && (1.0..=f64::from(MAX_BEATS)).contains(&requested) {
            requested as i32
        } else {
            DEFAULT_BEAT_COUNT
        };
        self.edit(
            p,
            |_| format!("Set pattern length → {applied} beats"),
            None,
            |pattern| {
                for track in &mut pattern.tracks {
                    let max_steps = applied * track.steps_per_beat;
                    if track.loop_length.is_some_and(|l| l > max_steps) {
                        track.loop_length = Some(max_steps);
                    }
                    track.beat_count = applied;
                }
                pattern.beat_count = applied;
            },
        )
        .is_some()
    }

    /// Tags (trimmed, empty ones left out).
    pub fn set_pattern_tags(&mut self, p: usize, tags: &[String]) -> bool {
        let tags: Vec<String> = tags.iter().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()).collect();
        self.edit(
            p,
            |pattern| format!("Set tags on \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.tags = tags,
        )
        .is_some()
    }

    pub fn set_pattern_description(&mut self, p: usize, description: &str) -> bool {
        self.edit(
            p,
            |pattern| format!("Set description on \"{}\"", pattern_name(pattern)),
            None,
            |pattern| pattern.description = description.to_owned(),
        )
        .is_some()
    }
}
