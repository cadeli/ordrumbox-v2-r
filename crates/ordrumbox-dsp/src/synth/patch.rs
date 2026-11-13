//! What a synth note plays with: the patch, its LFOs, envelopes and bypasses,
//! and the choices they are made of.

use crate::lfo::Wave;
use crate::slope::FilterMode;

/// What an LFO modulates, in the order of the preset files' codes (0 to 33).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoTarget {
    #[default]
    Off,
    /// The filter cutoff (as [`Self::FilterFreq`]).
    Filter,
    /// The pitch of an oscillator, widely (1000 cents at full depth).
    Osc1,
    Osc2,
    Osc3,
    Volume,
    Osc1Gain,
    Osc1Detune,
    Osc1Octave,
    Osc2Gain,
    Osc2Detune,
    Osc2Octave,
    Osc3Gain,
    Osc3Detune,
    Osc3Octave,
    FilterFreq,
    FilterEnvAmount,
    FilterQ,
    NoiseMix,
    SubGain,
    PitchPunch,
    Drive,
    NoiseFilterFreq,
    NoiseFilterQ,
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

impl LfoTarget {
    /// Every target, each at its code.
    pub const ALL: [Self; 34] = [
        Self::Off,
        Self::Filter,
        Self::Osc1,
        Self::Osc2,
        Self::Osc3,
        Self::Volume,
        Self::Osc1Gain,
        Self::Osc1Detune,
        Self::Osc1Octave,
        Self::Osc2Gain,
        Self::Osc2Detune,
        Self::Osc2Octave,
        Self::Osc3Gain,
        Self::Osc3Detune,
        Self::Osc3Octave,
        Self::FilterFreq,
        Self::FilterEnvAmount,
        Self::FilterQ,
        Self::NoiseMix,
        Self::SubGain,
        Self::PitchPunch,
        Self::Drive,
        Self::NoiseFilterFreq,
        Self::NoiseFilterQ,
        Self::FmAmount,
        Self::FmAlgorithm,
        Self::EnvAttack,
        Self::EnvDecay,
        Self::EnvSustain,
        Self::EnvRelease,
        Self::ModEnvAttack,
        Self::ModEnvDecay,
        Self::ModEnvSustain,
        Self::ModEnvRelease,
    ];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// What the modulation envelope moves, in the order of the preset files' codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModEnvTarget {
    #[default]
    Off,
    /// Opens the filter.
    Filter,
    /// Raises the pitch, an octave at full depth.
    Pitch,
    /// Adds FM.
    Fm,
    /// Raises the oscillators' gains towards 1.
    Shape,
}

impl ModEnvTarget {
    /// Every target, each at its code.
    pub const ALL: [Self; 5] = [Self::Off, Self::Filter, Self::Pitch, Self::Fm, Self::Shape];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// One LFO of a patch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SynthLfo {
    pub target: LfoTarget,
    pub wave: Wave,
    /// Hz; 0 or less is off.
    pub freq: f64,
    pub depth: f64,
}

/// An ADSR, seconds and level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adsr {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

/// Stages switched off (`bypass*` of the preset).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SynthBypass {
    pub noise: bool,
    pub filter: bool,
    pub env: bool,
    pub lfo1: bool,
    pub lfo2: bool,
    pub fm: bool,
    pub mod_env: bool,
    pub filter_env: bool,
}

/// Everything one note plays with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthPatch {
    /// Hz, before detune.
    pub osc_freq: [f64; 3],
    pub osc_gain: [f64; 3],
    /// Cents.
    pub osc_detune: [f64; 3],
    pub osc_wave: [Wave; 3],
    pub noise_mix: f64,
    pub noise_filter_type: FilterMode,
    pub noise_filter_freq: f64,
    pub noise_filter_q: f64,
    pub filter_type: FilterMode,
    pub filter_freq: f64,
    pub filter_q: f64,
    /// dB per octave: 12, or 24 (two filter stages).
    pub filter_slope: f64,
    pub drive: f64,
    pub pitch_punch: f64,
    pub sub_gain: f64,
    pub envelope: Adsr,
    pub volume: f64,
    pub pan: f64,
    /// Peak level of the note.
    pub velocity: f64,
    pub lfo1: SynthLfo,
    pub lfo2: SynthLfo,
    pub filter_env_amount: f64,
    pub fm_amount: f64,
    /// Which oscillators modulate which, 0 to 4. A number and not an `enum`:
    /// an LFO sweeps it ([`LfoTarget::FmAlgorithm`]), and it is rounded to
    /// the nearest routing sample by sample.
    pub fm_algorithm: f64,
    pub mod_envelope: Adsr,
    pub mod_env_target: ModEnvTarget,
    pub mod_env_depth: f64,
    pub bypass: SynthBypass,
}

impl Default for SynthPatch {
    fn default() -> Self {
        let lfo = SynthLfo { target: LfoTarget::Off, wave: Wave::Sine, freq: 1.0, depth: 0.0 };
        Self {
            osc_freq: [440.0; 3],
            osc_gain: [0.5; 3],
            osc_detune: [0.0; 3],
            osc_wave: [Wave::Sine; 3],
            noise_mix: 0.0,
            noise_filter_type: FilterMode::Lowpass,
            noise_filter_freq: 1000.0,
            noise_filter_q: 0.7,
            filter_type: FilterMode::Lowpass,
            filter_freq: 1000.0,
            filter_q: 0.7,
            filter_slope: 12.0,
            drive: 0.0,
            pitch_punch: 0.0,
            sub_gain: 0.0,
            envelope: Adsr { attack: 0.01, decay: 0.1, sustain: 0.7, release: 0.1 },
            volume: 0.8,
            pan: 0.0,
            velocity: 0.8,
            lfo1: lfo,
            lfo2: lfo,
            filter_env_amount: 0.0,
            fm_amount: 0.0,
            fm_algorithm: 0.0,
            mod_envelope: Adsr { attack: 0.01, decay: 0.1, sustain: 0.0, release: 0.1 },
            mod_env_target: ModEnvTarget::Off,
            mod_env_depth: 0.0,
            bypass: SynthBypass::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_sits_at_its_code() {
        for (code, target) in LfoTarget::ALL.iter().enumerate() {
            assert_eq!(usize::from(target.code()), code);
        }
        for (code, target) in ModEnvTarget::ALL.iter().enumerate() {
            assert_eq!(usize::from(target.code()), code);
        }
    }
}
