//! Where the modulation lands: the matrix, resolved once per block, and the
//! amounts it adds to each modulated value of a sample, or where it is used
//! ([`Late`]).

use std::ops::Index;

use super::patch::{Adsr, LfoTarget, MATRIX_SIZE, MatrixRoute, MatrixSource, SynthPatch};

/// A value the matrix can add to.
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

/// A value the matrix moves where it is used, from the sources of that very
/// sample (the modulation envelope as it is now, not one sample ago): what
/// opens the filter, the pitch of all three oscillators, more FM, the shape.
#[derive(Debug, Clone, Copy)]
pub(super) enum Late {
    FilterOpen,
    Pitch,
    FmBoost,
    Shape,
}

const LATE_COUNT: usize = Late::Shape as usize + 1;

/// What the matrix adds to each late value this sample.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LateMods([f64; LATE_COUNT]);

impl Index<Late> for LateMods {
    type Output = f64;

    #[inline]
    fn index(&self, late: Late) -> &f64 {
        &self.0[late as usize]
    }
}

/// Where a target lands: a slot, with what a full swing adds there, or a late value.
#[derive(Debug, Clone, Copy)]
enum Destination {
    Slot(Slot, f64),
    Late(Late),
}

/// What the matrix adds to each slot this sample.
#[derive(Debug, Clone, Copy)]
pub(super) struct Mods([f64; SLOT_COUNT]);

// By hand: `Default` is derived for arrays of up to 32 values only.
impl Default for Mods {
    fn default() -> Self {
        Self([0.0; SLOT_COUNT])
    }
}

impl Index<Slot> for Mods {
    type Output = f64;

    #[inline]
    fn index(&self, slot: Slot) -> &f64 {
        &self.0[slot as usize]
    }
}

/// `base` with what the matrix adds to `slot`.
#[inline]
pub(super) fn modulated(mods: &Mods, base: f64, slot: Slot) -> f64 {
    base + mods[slot]
}

/// Whether anything adds to `slot` this sample.
#[inline]
pub(super) fn is_moved(mods: &Mods, slot: Slot) -> bool {
    mods[slot] != 0.0
}

/// `adsr` with what the matrix adds to its four `slots`, kept in range.
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

/// Whether a line of the matrix of `p` may move `target`: a stage the
/// patch leaves at rest is then still worked out.
#[must_use]
pub(super) fn is_targeted(p: &SynthPatch, target: LfoTarget) -> bool {
    p.matrix.iter().any(|route| route.target == target)
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
/// something and write somewhere, to a slot or to a late value.
#[derive(Debug, Clone, Copy)]
pub(super) struct Matrix {
    routes: [Option<(MatrixSource, Slot, f64)>; MATRIX_SIZE],
    late_routes: [Option<(MatrixSource, Late, f64)>; MATRIX_SIZE],
}

impl Matrix {
    pub(super) fn new(routes: &[MatrixRoute; MATRIX_SIZE]) -> Self {
        let resolved = routes.map(|route| {
            if route.source == MatrixSource::Off || route.amount == 0.0 || route.amount.is_nan() {
                return None;
            }
            Some((route.source, destination(route.target)?, route.amount.clamp(-1.0, 1.0)))
        });
        Self {
            routes: resolved.map(|route| match route? {
                (source, Destination::Slot(slot, scale), amount) => Some((source, slot, amount * scale)),
                (_, Destination::Late(_), _) => None,
            }),
            late_routes: resolved.map(|route| match route? {
                (source, Destination::Late(late), amount) => Some((source, late, amount)),
                (_, Destination::Slot(..), _) => None,
            }),
        }
    }

    /// Whether a route reads `source`.
    pub(super) fn reads(&self, source: MatrixSource) -> bool {
        self.routes.iter().flatten().any(|&(s, ..)| s == source)
            || self.late_routes.iter().flatten().any(|&(s, ..)| s == source)
    }

    /// Whether no route adds to a slot (late values aside).
    pub(super) fn is_empty(&self) -> bool {
        self.routes.iter().all(Option::is_none)
    }

    /// Whether a route moves a late value.
    pub(super) fn has_late(&self) -> bool {
        self.late_routes.iter().any(Option::is_some)
    }

    /// What the late routes add at `values`.
    #[inline]
    pub(super) fn late(&self, values: &SourceValues) -> LateMods {
        let mut late = LateMods::default();
        for &(source, slot, factor) in self.late_routes.iter().flatten() {
            late.0[slot as usize] += values.of(source) * factor;
        }
        late
    }

    /// Write what the routes add at `values` into `mods`.
    #[inline]
    pub(super) fn apply(&self, values: &SourceValues, mods: &mut Mods) {
        mods.0 = [0.0; SLOT_COUNT];
        for &(source, slot, factor) in self.routes.iter().flatten() {
            mods.0[slot as usize] += values.of(source) * factor;
        }
    }
}

/// What a full swing of a matrix source on `target` adds there: cents for a pitch
/// (an octave target's included), Hz for a cutoff, seconds for an envelope
/// time; 0 when off.
#[must_use]
pub fn lfo_swing(target: LfoTarget) -> f64 {
    match destination(target) {
        Some(Destination::Slot(_, swing)) => swing,
        // A share of the effect: of the way to the top, of an octave, of the FM, of the way to full gain.
        Some(Destination::Late(_)) => 1.0,
        None => 0.0,
    }
}

/// Where `target` lands.
fn destination(target: LfoTarget) -> Option<Destination> {
    use LfoTarget as T;
    let late = match target {
        T::FilterOpen => Some(Late::FilterOpen),
        T::Pitch => Some(Late::Pitch),
        T::FmBoost => Some(Late::FmBoost),
        T::Shape => Some(Late::Shape),
        _ => None,
    };
    match late {
        Some(late) => Some(Destination::Late(late)),
        None => slot_of(target).map(|(slot, swing)| Destination::Slot(slot, swing)),
    }
}

/// The slot `target` adds to and what a full swing adds there: cents for a
/// pitch, Hz for a cutoff, seconds for an envelope time.
fn slot_of(target: LfoTarget) -> Option<(Slot, f64)> {
    use LfoTarget as T;
    Some(match target {
        T::Off | T::FilterOpen | T::Pitch | T::FmBoost | T::Shape => return None,
        T::FilterFreq => (Slot::Filter, 1000.0),
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
    fn a_route_adds_to_its_slot_only() {
        let mut routes = [MatrixRoute::default(); MATRIX_SIZE];
        routes[0] = MatrixRoute { source: MatrixSource::Lfo2, target: LfoTarget::Osc2Octave, amount: 0.5 };
        let matrix = Matrix::new(&routes);
        let mut mods = Mods::default();
        // A square starts low: half of an octave down.
        matrix.apply(&SourceValues { lfo: [0.0, -1.0], ..SourceValues::default() }, &mut mods);
        assert_eq!(modulated(&mods, 7.0, Slot::Detune2), 7.0 - 600.0);
        assert_eq!(modulated(&mods, 7.0, Slot::Detune1), 7.0);
    }

    #[test]
    fn an_empty_silent_or_aimless_route_is_off() {
        let route = MatrixRoute { source: MatrixSource::Lfo1, target: LfoTarget::FilterFreq, amount: 0.5 };
        let is_on = |route| !Matrix::new(&[route; MATRIX_SIZE]).is_empty();
        assert!(is_on(route));
        assert!(!is_on(MatrixRoute { source: MatrixSource::Off, ..route }));
        assert!(!is_on(MatrixRoute { amount: 0.0, ..route }));
        assert!(!is_on(MatrixRoute { target: LfoTarget::Off, ..route }));
    }
}
