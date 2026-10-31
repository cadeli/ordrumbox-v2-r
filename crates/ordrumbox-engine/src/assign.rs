//! Which sound each track plays (`logic/services/auto_assign.js`).
//!
//! A track with auto-assign on gets a sample by name, in tiers: an exact key
//! in the selected kit, then in another kit, then the catalog's substitutes
//!, then any sample. Each tier picks at random among
//! its candidates; `pick(n)` makes that choice (an index below `n`), so the
//! JS choices can be reproduced. A synth track without a valid preset gets a
//! random one. The same matching picks a synth preset for a track name
//! ([`synth_for`]), the presets standing as one kit.

use ordrumbox_core::drum_taxonomy::instrument_family;
use ordrumbox_core::instruments::{self, Catalog};
use ordrumbox_core::{Pattern, Track};

use crate::kit::{BankSample, SampleBank};
use crate::synth_preset::SynthPresets;

/// What a track plays.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum TrackSound {
    /// Nothing (no sample found, no preset).
    #[default]
    None,
    /// A sample of the bank, by index.
    Sample(usize),
    /// A synth preset, by key.
    Synth(String),
}

fn normalize(name: &str) -> String {
    name.trim().to_uppercase()
}

/// What the matching sees of a sound: the kit it belongs to and its key.
#[derive(Debug, Clone, Copy)]
struct Candidate<'a> {
    kit: &'a str,
    key: &'a str,
}

/// Kit name the synth presets are matched under.
const SYNTH_KIT: &str = "synth";

/// The key a preset is matched by: `KICK_SYNTH` plays a `KICK` track, `BASS0`
/// to `BASS5` a `BASS` track.
fn preset_match_key(key: &str) -> &str {
    key.strip_suffix("_SYNTH").unwrap_or(key).trim_end_matches(|c: char| c.is_ascii_digit())
}

/// The auto-assignment over a list of sounds, with kit `kit` selected.
pub struct AutoAssign<'a> {
    candidates: Vec<Candidate<'a>>,
    kit: &'a str,
    catalog: &'a Catalog,
    pick: &'a mut dyn FnMut(usize) -> usize,
}

impl std::fmt::Debug for AutoAssign<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoAssign").field("kit", &self.kit).field("candidates", &self.candidates.len()).finish()
    }
}

impl<'a> AutoAssign<'a> {
    pub fn new(bank: &'a SampleBank, kit: &'a str, pick: &'a mut dyn FnMut(usize) -> usize) -> Self {
        let candidates = bank.samples.iter().map(|s: &'a BankSample| Candidate { kit: &s.kit, key: &s.sample.key });
        Self { candidates: candidates.collect(), kit, catalog: instruments::catalog(), pick }
    }

    /// The matching over the synth presets, all in one kit.
    fn over_synths(synths: &'a SynthPresets, pick: &'a mut dyn FnMut(usize) -> usize) -> Self {
        let candidates = synths.keys().into_iter().map(|key| Candidate { kit: SYNTH_KIT, key: preset_match_key(key) });
        Self { candidates: candidates.collect(), kit: SYNTH_KIT, catalog: instruments::catalog(), pick }
    }

    fn pick_from(&mut self, candidates: &[usize]) -> Option<usize> {
        if candidates.is_empty() {
            return None;
        }
        let i = (self.pick)(candidates.len());
        candidates.get(i.min(candidates.len() - 1)).copied()
    }

    /// Candidates of `kit` (`Some`), of every kit but `exclude` (`exclude`), or all.
    fn in_scope(&self, kit: Option<&str>, exclude: Option<&str>) -> Vec<(usize, Candidate<'a>)> {
        let in_it = |c: &Candidate<'a>| kit.is_none_or(|k| c.kit == k) && exclude.is_none_or(|k| c.kit != k);
        self.candidates.iter().copied().enumerate().filter(|(_, c)| in_it(c)).collect()
    }

    /// Candidates whose key is one of `names` (normalized).
    fn exact(&self, names: &[String], kit: Option<&str>, exclude: Option<&str>) -> Vec<usize> {
        self.in_scope(kit, exclude).iter().filter(|(_, c)| names.contains(&normalize(c.key))).map(|(i, _)| *i).collect()
    }

    /// `#resolveInstrumentId`: a catalog id for the track name, else the name.
    fn resolve_instrument_id(&self, name: &str) -> String {
        if self.catalog.by_exact_id(name).is_some() {
            return name.to_owned();
        }
        match self.catalog.from_file_name(name) {
            Some(found) if self.catalog.by_exact_id(&found.id).is_some() => found.id.clone(),
            _ => name.to_owned(),
        }
    }

    /// `autoAssignTrackSounds`: the candidate for a track named `name`; `None`
    /// when there is no candidate or the selected kit does not exist.
    pub fn sample_for(&mut self, name: &str) -> Option<usize> {
        if self.kit.is_empty() {
            return None; // no kit selected: the track keeps no sample
        }
        if let Some(found) = self.matching(name) {
            return Some(found);
        }
        let all: Vec<usize> = (0..self.candidates.len()).collect();
        self.pick_from(&all)
    }

    /// The candidate that matches the track name `name`: by key, then by the
    /// catalog's substitutes.
    fn matching(&mut self, name: &str) -> Option<usize> {
        let match_name = self.resolve_instrument_id(name);
        let mut names = vec![normalize(name)];
        if normalize(&match_name) != names[0] {
            names.push(normalize(&match_name));
        }
        let kit = self.kit;
        let tier1 = self.exact(&names, Some(kit), None);
        if let Some(found) = self.pick_from(&tier1) {
            return Some(found);
        }
        let tier2 = self.exact(&names, None, Some(kit));
        if let Some(found) = self.pick_from(&tier2) {
            return Some(found);
        }
        self.equivalence(&match_name)
    }

    /// `findSoundEquivalence`: the catalog substitutes of `track_name`.
    fn equivalence(&mut self, track_name: &str) -> Option<usize> {
        let replacements = self.catalog.by_exact_id(track_name)?.subst.clone();
        let kit = self.kit;
        for target in &replacements {
            for scope in [Some(kit), None] {
                let ids = self.exact(&[normalize(target)], scope, None);
                if let Some(found) = self.pick_from(&ids) {
                    return Some(found);
                }
            }
        }
        for target in &replacements {
            if let Some(found) = self.by_track_name(target, Some(kit)).or_else(|| self.by_track_name(target, None)) {
                return Some(found);
            }
            let synonyms = self.catalog.by_exact_id(target).map(|i| i.synonyms.clone()).unwrap_or_default();
            for synonym in synonyms.iter().filter(|s| !s.contains(['.', '*', '^', '$'])) {
                if let Some(found) =
                    self.by_key_containing(Some(kit), synonym).or_else(|| self.by_key_containing(None, synonym))
                {
                    return Some(found);
                }
            }
        }
        None
    }

    /// `getSampleIdByKeyContaining`.
    fn by_key_containing(&mut self, kit: Option<&str>, search: &str) -> Option<usize> {
        let search = search.trim().to_uppercase();
        if search.is_empty() {
            return None;
        }
        let ids: Vec<usize> = self
            .in_scope(kit, None)
            .iter()
            .filter(|(_, c)| c.key.to_uppercase().contains(&search))
            .map(|(i, _)| *i)
            .collect();
        self.pick_from(&ids)
    }

    /// `getSampleIdFromTrackname`: exact key, else a candidate of the same
    /// coarse type (only drum sounds for a drum instrument).
    fn by_track_name(&mut self, name: &str, kit: Option<&str>) -> Option<usize> {
        let kind = instrument_family(name);
        let wanted = normalize(name);
        let drums_only = self.catalog.by_id(name).is_some_and(|i| i.drum);
        let is_drum_sound = |key: &str| self.catalog.by_id(key).is_none_or(|i| i.drum);
        let (mut exact, mut same_type) = (Vec::new(), Vec::new());
        for (i, candidate) in self.in_scope(kit, None) {
            let key = candidate.key;
            if normalize(key) == wanted {
                exact.push(i);
            } else if instrument_family(key) == kind && (!drums_only || is_drum_sound(key)) {
                same_type.push(i);
            }
        }
        let candidates = if exact.is_empty() { same_type } else { exact };
        self.pick_from(&candidates)
    }
}

/// The synth preset whose name matches a track named `name`, by the rules of
/// the auto-assignment; `None` when none does.
pub fn synth_for(name: &str, synths: &SynthPresets, pick: &mut dyn FnMut(usize) -> usize) -> Option<String> {
    let found = AutoAssign::over_synths(synths, pick).matching(name)?;
    synths.keys().get(found).map(|key| (*key).to_owned())
}

/// What every track of `pattern` plays: synth tracks
/// their preset (a random one when missing or unknown), tracks with
/// auto-assign their sample by name in kit `kit`, the others the sample
/// their `sampleId` names.
pub fn assign_pattern(
    pattern: &Pattern,
    bank: &SampleBank,
    kit: &str,
    synths: &SynthPresets,
    pick: &mut dyn FnMut(usize) -> usize,
) -> Vec<TrackSound> {
    pattern.tracks.iter().map(|track| assign_track(track, bank, kit, synths, pick)).collect()
}

pub fn assign_track(
    track: &Track,
    bank: &SampleBank,
    kit: &str,
    synths: &SynthPresets,
    pick: &mut dyn FnMut(usize) -> usize,
) -> TrackSound {
    if track.use_synth {
        let key = track.synth_preset.as_deref().filter(|k| synths.get(k).is_some()).map(str::to_owned);
        return match key {
            Some(key) => TrackSound::Synth(key),
            None if synths.is_empty() => TrackSound::None,
            None => {
                let keys = synths.keys();
                TrackSound::Synth(keys[pick(keys.len()).min(keys.len() - 1)].to_owned())
            }
        };
    }
    if !track.auto_assign {
        return bank.find_path(&track.sample_path).map_or(TrackSound::None, TrackSound::Sample);
    }
    if bank.samples.is_empty() {
        return TrackSound::None;
    }
    AutoAssign::new(bank, kit, pick).sample_for(&track.name).map_or(TrackSound::None, TrackSound::Sample)
}

#[cfg(test)]
mod tests {
    use ordrumbox_core::json;

    use super::*;

    fn synths(keys: &[&str]) -> SynthPresets {
        let entries: Vec<String> = keys.iter().map(|key| format!("\"{key}\":{{}}")).collect();
        SynthPresets::from_json(&json::parse(&format!("{{{}}}", entries.join(","))).unwrap())
    }

    #[test]
    fn a_track_gets_the_preset_of_its_name() {
        let presets = synths(&["BASS0", "SN", "TOM", "KICK_SYNTH", "SNARE_SYNTH", "CLAP_SYNTH", "CHH_SYNTH"]);
        let mut first = |_: usize| 0;
        for (name, want) in
            [("KICK", "KICK_SYNTH"), ("SNARE", "SNARE_SYNTH"), ("CLAP", "CLAP_SYNTH"), ("CHH", "CHH_SYNTH")]
        {
            assert_eq!(synth_for(name, &presets, &mut first).as_deref(), Some(want));
        }
    }

    #[test]
    fn a_numbered_preset_plays_the_track_of_its_name() {
        let presets = synths(&["TOM", "BELL1", "BASS0", "BASS1"]);
        let (mut first, mut last) = (|_: usize| 0, |n: usize| n - 1);
        assert_eq!(synth_for("BASS", &presets, &mut first).as_deref(), Some("BASS0"));
        assert_eq!(synth_for("BASS", &presets, &mut last).as_deref(), Some("BASS1"));
    }

    #[test]
    fn a_name_nothing_matches_gets_no_preset() {
        let mut first = |_: usize| 0;
        assert_eq!(synth_for("KICK", &synths(&[]), &mut first), None);
        assert_eq!(synth_for("ZZZ", &synths(&["PIANO"]), &mut first), None);
    }
}
