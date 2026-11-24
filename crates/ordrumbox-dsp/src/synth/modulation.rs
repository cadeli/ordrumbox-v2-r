//! Where an LFO lands: its route, resolved once per block, and the amounts
//! the two LFOs add to each modulated value of a sample.

use std::ops::Index;

use super::patch::{Adsr, LfoTarget, MATRIX_SIZE, MatrixRoute, MatrixSource, SynthLfo, SynthPatch};
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
    Width1,
    Width2,
    Width3,
    NoiseRate,
    PitchEnvAmount,
    PitchEnvDecay,
    Ring,
}

const SLOT_COUNT: usize = Slot::Ring as usize + 1;

pub(super) const DETUNE_SLOTS: [Slot; 3] = [Slot::Detune1, Slot::Detune2, Slot::Detune3];
pub(super) const GAIN_SLOTS: [Slot; 3] = [Slot::Gain1, Slot::Gain2, Slot::Gain3];
pub(super) const WIDTH_SLOTS: [Slot; 3] = [Slot::Width1, Slot::Width2, Slot::Width3];
pub(super) const ENV_SLOTS: [Slot; 4] = [Slot::EnvAttack, Slot::EnvDecay, Slot::EnvSustain, Slot::EnvRelease];
pub(super) const MOD_ENV_SLOTS: [Slot; 4] =
    [Slot::ModEnvAttack, Slot::ModEnvDecay, Slot::ModEnvSustain, Slot::ModEnvRelease];

/// What one LFO adds this sample: all zero but the slot of its route.
#[derive(Debug, Clone, Copy)]
pub(super) struct LfoMods([f64; SLOT_COUNT]);

// By hand: `Default` is derived for arrays of up to 32 values only.
impl Default for LfoMods {
    fn default() -> Self {
        Self([0.0; SLOT_COUNT])
    }
}

impl Index<Slot> for LfoMods {
    type Output = f64;

    #[inline]
    fn index(&self, slot: Slot) -> &f64 {
        &self.0[slot as usize]
    }
}

/// What modulates a sample: the two LFOs' targets, then the matrix.
pub(super) type Mods = [LfoMods; 3];

/// `base` with what the LFOs and the matrix add to `slot`.
#[inline]
pub(super) fn modulated(mods: &Mods, base: f64, slot: Slot) -> f64 {
    base + mods[0][slot] + mods[1][slot] + mods[2][slot]
}

/// Whether anything adds to `slot` this sample.
#[inline]
pub(super) fn is_moved(mods: &Mods, slot: Slot) -> bool {
    mods[0][slot] != 0.0 || mods[1][slot] != 0.0 || mods[2][slot] != 0.0
}

/// `adsr` with what the LFOs and the matrix add to its four `slots`, kept in range.
#[inline]
pub(super) fn modulated_adsr(mods: &Mods, adsr: Adsr, slots: [Slot; 4]) -> Adsr {
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

/// Whether an LFO or a line of the matrix of `p` may move `target`: a
/// stage the patch leaves at rest is then still worked out.
#[must_use]
pub(super) fn is_targeted(p: &SynthPatch, target: LfoTarget) -> bool {
    p.lfo1.target == target || p.lfo2.target == target || p.matrix.iter().any(|route| route.target == target)
}

/// The values the sources of the matrix have at one sample.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct SourceValues {
    pub(super) lfo: [f64; 2],
    pub(super) envelope: f64,
    pub(super) mod_envelope: f64,
    pub(super) velocity: f64,
    pub(super) key: f64,
    pub(super) random: f64,
}

impl SourceValues {
    #[inline]
    fn of(&self, source: MatrixSource) -> f64 {
        match source {
            MatrixSource::Off => 0.0,
            MatrixSource::Lfo1 => self.lfo[0],
            MatrixSource::Lfo2 => self.lfo[1],
            MatrixSource::Envelope => self.envelope,
            MatrixSource::ModEnvelope => self.mod_envelope,
            MatrixSource::Velocity => self.velocity,
            MatrixSource::Key => self.key,
            MatrixSource::Random => self.random,
        }
    }
}

/// The matrix of a patch, resolved once per block: the routes that read
/// something and write somewhere.
#[derive(Debug, Clone, Copy)]
pub(super) struct Matrix {
    routes: [Option<(MatrixSource, Slot, f64)>; MATRIX_SIZE],
}

impl Matrix {
    pub(super) fn new(routes: &[MatrixRoute; MATRIX_SIZE]) -> Self {
        Self {
            routes: routes.map(|route| {
                if route.source == MatrixSource::Off || route.amount == 0.0 || route.amount.is_nan() {
                    return None;
                }
                let (slot, scale) = destination(route.target)?;
                Some((route.source, slot, route.amount.clamp(-1.0, 1.0) * scale))
            }),
        }
    }

    /// Whether a route reads `source`.
    pub(super) fn reads(&self, source: MatrixSource) -> bool {
        self.routes.iter().flatten().any(|&(s, ..)| s == source)
    }

    /// Whether any route is on.
    pub(super) fn is_empty(&self) -> bool {
        self.routes.iter().all(Option::is_none)
    }

    /// Write what the routes add at `values` into `mods`.
    #[inline]
    pub(super) fn apply(&self, values: &SourceValues, mods: &mut LfoMods) {
        mods.0 = [0.0; SLOT_COUNT];
        for &(source, slot, factor) in self.routes.iter().flatten() {
            mods.0[slot as usize] += values.of(source) * factor;
        }
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
        T::Osc1Width => (Slot::Width1, 0.5),
        T::Osc2Width => (Slot::Width2, 0.5),
        T::Osc3Width => (Slot::Width3, 0.5),
        T::NoiseRate => (Slot::NoiseRate, 8000.0),
        T::PitchEnvAmount => (Slot::PitchEnvAmount, 24.0),
        T::PitchEnvDecay => (Slot::PitchEnvDecay, 0.25),
        T::Osc2Ring => (Slot::Ring, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_lfo_adds_to_its_slot_only() {
        let lfo = SynthLfo { target: LfoTarget::Osc2Octave, wave: Wave::Square, freq: 2.0, depth: 0.5 };
        let route = LfoRoute::new(&lfo, false).unwrap();
        let mut mods = [LfoMods::default(); 3];
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
