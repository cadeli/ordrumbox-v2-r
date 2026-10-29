//! Which sound each track plays (`logic/services/auto_assign.js`).
//!
//! A track with auto-assign on gets a sample by name, in tiers: an exact key
//! in the selected kit, then in another kit, then the catalog's substitutes
//! (`findSoundEquivalence`), then any sample. Each tier picks at random among
//! its candidates; `pick(n)` makes that choice (an index below `n`), so the
//! JS choices can be reproduced. A synth track without a valid preset gets a
//! random one.

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

/// The auto-assignment over a bank, with kit `kit` selected.
pub struct AutoAssign<'a> {
    bank: &'a SampleBank,
    kit: &'a str,
    catalog: &'a Catalog,
    pick: &'a mut dyn FnMut(usize) -> usize,
}

impl std::fmt::Debug for AutoAssign<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoAssign").field("kit", &self.kit).field("samples", &self.bank.samples.len()).finish()
    }
}

impl<'a> AutoAssign<'a> {
    pub fn new(bank: &'a SampleBank, kit: &'a str, pick: &'a mut dyn FnMut(usize) -> usize) -> Self {
        Self { bank, kit, catalog: instruments::catalog(), pick }
    }

    fn pick_from(&mut self, candidates: &[usize]) -> Option<usize> {
        if candidates.is_empty() {
            return None;
        }
        let i = (self.pick)(candidates.len());
        candidates.get(i.min(candidates.len() - 1)).copied()
    }

    /// Samples of `kit` (`Some`), of every kit but `exclude` (`exclude`), or all.
    fn in_scope(&self, kit: Option<&str>, exclude: Option<&str>) -> impl Iterator<Item = (usize, &'a BankSample)> + 'a {
        let (kit, exclude) = (kit.map(str::to_owned), exclude.map(str::to_owned));
        self.bank.samples.iter().enumerate().filter(move |(_, s)| {
            kit.as_deref().is_none_or(|k| s.kit == k) && exclude.as_deref().is_none_or(|k| s.kit != k)
        })
    }

    /// Samples whose key is one of `names` (normalized).
    fn exact(&self, names: &[String], kit: Option<&str>, exclude: Option<&str>) -> Vec<usize> {
        self.in_scope(kit, exclude).filter(|(_, s)| names.contains(&normalize(&s.sample.key))).map(|(i, _)| i).collect()
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

    /// `autoAssignTrackSounds`: the sample for a track named `name`; `None`
    /// when the bank is empty or the selected kit does not exist.
    pub fn sample_for(&mut self, name: &str) -> Option<usize> {
        if self.kit.is_empty() {
            return None; // no kit selected: the track keeps no sample
        }
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
        if let Some(found) = self.equivalence(&match_name) {
            return Some(found);
        }
        let all: Vec<usize> = (0..self.bank.samples.len()).collect();
        self.pick_from(&all)
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
            .filter(|(_, s)| s.sample.key.to_uppercase().contains(&search))
            .map(|(i, _)| i)
            .collect();
        self.pick_from(&ids)
    }

    /// `getSampleIdFromTrackname`: exact key, else a sample of the same
    /// coarse type (only drum sounds for a drum instrument).
    fn by_track_name(&mut self, name: &str, kit: Option<&str>) -> Option<usize> {
        let kind = instrument_family(name);
        let wanted = normalize(name);
        let drums_only = self.catalog.by_id(name).is_some_and(|i| i.drum);
        let is_drum_sound = |key: &str| self.catalog.by_id(key).is_none_or(|i| i.drum);
        let (mut exact, mut same_type) = (Vec::new(), Vec::new());
        for (i, sample) in self.in_scope(kit, None) {
            let key = &sample.sample.key;
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

/// What every track of `pattern` plays (`autoAssignSounds`): synth tracks
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
    if track.use_soft_synth {
        let key = track.synth_sound_key.as_deref().filter(|k| synths.get(k).is_some()).map(str::to_owned);
        return match key {
            Some(key) => TrackSound::Synth(key),
            None if synths.is_empty() => TrackSound::None,
            None => {
                let keys = synths.keys();
                TrackSound::Synth(keys[pick(keys.len()).min(keys.len() - 1)].to_owned())
            }
        };
    }
    if !track.use_auto_assign_sound {
        return bank.find_url(&track.sample_id).map_or(TrackSound::None, TrackSound::Sample);
    }
    if bank.samples.is_empty() {
        return TrackSound::None;
    }
    AutoAssign::new(bank, kit, pick).sample_for(&track.name).map_or(TrackSound::None, TrackSound::Sample)
}
