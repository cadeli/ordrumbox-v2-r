//! Where an LFO lands: its route, resolved once per block, and the amounts
//! the two LFOs add to each modulated value of a sample.

use std::ops::Index;

use super::patch::{Adsr, LfoTarget, SynthLfo};
use crate::lfo::{self, Wave};

/// A value an LFO can add to.
#[derive(Debug, Clone, Copy)]
pub(super) enum Slot {
    Filter,
    Detune1,
    Detune2,
    Detune3,
    Volume,
    Gain1,
    Gain2,
    Gain3,
    FilterEnvAmount,
    FilterQ,
    NoiseMix,
    SubGain,
    PitchPunch,
    Drive,
    NoiseFreq,
    NoiseQ,
    FmAmount,
    FmAlgorithm,
    EnvAttack,
    EnvDecay,
    EnvSustain,
    EnvRelease,
    ModEnvAttack,
    ModEnvDecay,
    ModEnvSustain,
    ModEnvRelease,
}

const SLOT_COUNT: usize = Slot::ModEnvRelease as usize + 1;

pub(super) const DETUNE_SLOTS: [Slot; 3] = [Slot::Detune1, Slot::Detune2, Slot::Detune3];
pub(super) const GAIN_SLOTS: [Slot; 3] = [Slot::Gain1, Slot::Gain2, Slot::Gain3];
pub(super) const ENV_SLOTS: [Slot; 4] = [Slot::EnvAttack, Slot::EnvDecay, Slot::EnvSustain, Slot::EnvRelease];
pub(super) const MOD_ENV_SLOTS: [Slot; 4] =
    [Slot::ModEnvAttack, Slot::ModEnvDecay, Slot::ModEnvSustain, Slot::ModEnvRelease];

/// What one LFO adds this sample: all zero but the slot of its route.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LfoMods([f64; SLOT_COUNT]);

impl Index<Slot> for LfoMods {
    type Output = f64;

    #[inline]
    fn index(&self, slot: Slot) -> &f64 {
        &self.0[slot as usize]
    }
}

/// `base` with what the two LFOs add to `slot`.
#[inline]
pub(super) fn modulated(mods: &[LfoMods; 2], base: f64, slot: Slot) -> f64 {
    base + mods[0][slot] + mods[1][slot]
}

/// `adsr` with what the two LFOs add to its four `slots`, kept in range.
#[inline]
pub(super) fn modulated_adsr(mods: &[LfoMods; 2], adsr: Adsr, slots: [Slot; 4]) -> Adsr {
    let [attack, decay, sustain, release] = slots;
    Adsr {
        attack: modulated(mods, adsr.attack, attack).max(0.0),
        decay: modulated(mods, adsr.decay, decay).max(0.0),
        sustain: super::clamp(modulated(mods, adsr.sustain, sustain), 0.0, 1.0),
        release: modulated(mods, adsr.release, release).max(0.0),
    }
}

/// An LFO that is on: the slot it adds to, and its swing there.
#[derive(Debug, Clone, Copy)]
pub(super) struct LfoRoute {
    slot: Slot,
    /// What a full depth adds, in the slot's unit.
    scale: f64,
    wave: Wave,
    depth: f64,
}

impl LfoRoute {
    /// The route of `lfo`; `None` when it is bypassed, stopped or aimed at nothing.
    pub(super) fn new(lfo: &SynthLfo, is_bypassed: bool) -> Option<Self> {
        if is_bypassed || lfo.freq <= 0.0 || lfo.freq.is_nan() {
            return None;
        }
        let (slot, scale) = destination(lfo.target)?;
        Some(Self { slot, scale, wave: lfo.wave, depth: lfo.depth })
    }

    /// Write this LFO's amount at `phase` (cycles) into `mods`.
    #[inline]
    pub(super) fn apply(&self, phase: f64, mods: &mut LfoMods) {
        mods.0[self.slot as usize] = lfo::wave(phase, self.wave) * self.depth * self.scale;
    }
}

/// The slot `target` adds to and what a full swing adds there: cents for a
/// pitch, Hz for a cutoff, seconds for an envelope time.
fn destination(target: LfoTarget) -> Option<(Slot, f64)> {
    use LfoTarget as T;
    Some(match target {
        T::Off => return None,
        T::Filter | T::FilterFreq => (Slot::Filter, 1000.0),
        T::Osc1 => (Slot::Detune1, 1000.0),
        T::Osc2 => (Slot::Detune2, 1000.0),
        T::Osc3 => (Slot::Detune3, 1000.0),
        T::Volume => (Slot::Volume, 1.0),
        T::Osc1Gain => (Slot::Gain1, 1.0),
        T::Osc2Gain => (Slot::Gain2, 1.0),
        T::Osc3Gain => (Slot::Gain3, 1.0),
        T::Osc1Detune => (Slot::Detune1, 100.0),
        T::Osc2Detune => (Slot::Detune2, 100.0),
        T::Osc3Detune => (Slot::Detune3, 100.0),
        T::Osc1Octave => (Slot::Detune1, 1200.0),
        T::Osc2Octave => (Slot::Detune2, 1200.0),
        T::Osc3Octave => (Slot::Detune3, 1200.0),
        T::FilterEnvAmount => (Slot::FilterEnvAmount, 1.0),
        T::FilterQ => (Slot::FilterQ, 24.0),
        T::NoiseMix => (Slot::NoiseMix, 1.0),
        T::SubGain => (Slot::SubGain, 1.0),
        T::PitchPunch => (Slot::PitchPunch, 1.0),
        T::Drive => (Slot::Drive, 1.0),
        T::NoiseFilterFreq => (Slot::NoiseFreq, 10_000.0),
        T::NoiseFilterQ => (Slot::NoiseQ, 24.0),
        T::FmAmount => (Slot::FmAmount, 1.0),
        T::FmAlgorithm => (Slot::FmAlgorithm, 1.0),
        T::EnvAttack => (Slot::EnvAttack, 0.25),
        T::EnvDecay => (Slot::EnvDecay, 0.5),
        T::EnvSustain => (Slot::EnvSustain, 0.5),
        T::EnvRelease => (Slot::EnvRelease, 0.25),
        T::ModEnvAttack => (Slot::ModEnvAttack, 0.25),
        T::ModEnvDecay => (Slot::ModEnvDecay, 0.5),
        T::ModEnvSustain => (Slot::ModEnvSustain, 0.5),
        T::ModEnvRelease => (Slot::ModEnvRelease, 0.25),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_lfo_adds_to_its_slot_only() {
        let lfo = SynthLfo { target: LfoTarget::Osc2Octave, wave: Wave::Square, freq: 2.0, depth: 0.5 };
        let route = LfoRoute::new(&lfo, false).unwrap();
        let mut mods = [LfoMods::default(); 2];
        // A square starts low: half of an octave down.
        route.apply(0.0, &mut mods[1]);
        assert_eq!(modulated(&mods, 7.0, Slot::Detune2), 7.0 - 600.0);
        assert_eq!(modulated(&mods, 7.0, Slot::Detune1), 7.0);
    }

    #[test]
    fn a_bypassed_stopped_or_idle_lfo_has_no_route() {
        let lfo = SynthLfo { target: LfoTarget::Filter, wave: Wave::Sine, freq: 2.0, depth: 0.5 };
        assert!(LfoRoute::new(&lfo, false).is_some());
        assert!(LfoRoute::new(&lfo, true).is_none());
        assert!(LfoRoute::new(&SynthLfo { freq: 0.0, ..lfo }, false).is_none());
        assert!(LfoRoute::new(&SynthLfo { target: LfoTarget::Off, ..lfo }, false).is_none());
    }
}
