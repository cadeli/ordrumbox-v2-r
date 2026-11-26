//! What a synth note plays with: the patch, its LFOs, envelopes and bypasses,
//! and the choices they are made of.

use crate::lfo::Wave;
use crate::slope::FilterMode;

/// What a line of the matrix modulates: one control each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LfoTarget {
    #[default]
    Off,
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
    /// The pulse width of an oscillator's square wave (half a cycle at full depth).
    Osc1Width,
    Osc2Width,
    Osc3Width,
    /// The clock of the console noise (8 kHz at full depth).
    NoiseRate,
    /// The pitch envelope's offset (24 semitones at full depth) and its decay (0.25 s).
    PitchEnvAmount,
    PitchEnvDecay,
    /// How far oscillator 1 ring-modulates oscillator 2.
    Osc2Ring,
    /// Opens the filter towards its top: at 1, all the way.
    FilterOpen,
    /// The pitch of the three oscillators, an octave at 1.
    Pitch,
    /// More FM: at 1, twice the FM amount.
    FmBoost,
    /// Raises the oscillators' gains towards 1: at 1, all at full.
    Shape,
    /// The second filter's cutoff.
    Filter2Freq,
}

impl LfoTarget {
    /// Every target, each at its code.
    pub const ALL: [Self; 41] = [
        Self::Off,
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
        Self::Osc1Width,
        Self::Osc2Width,
        Self::Osc3Width,
        Self::NoiseRate,
        Self::PitchEnvAmount,
        Self::PitchEnvDecay,
        Self::Osc2Ring,
        Self::FilterOpen,
        Self::Pitch,
        Self::FmBoost,
        Self::Shape,
        Self::Filter2Freq,
    ];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// The clock rate of the console noise a patch starts with, Hz.
pub const NOISE_RATE_DEFAULT: f64 = 8000.0;

/// What the noise generator draws its samples from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseMode {
    /// A new random value every sample.
    #[default]
    White,
    /// A 15-bit shift register clocked at [`SynthPatch::noise_rate`]: the console's rumble.
    Long,
    /// The same register cut to 7 bits: a short loop, a pitched, metallic buzz.
    Short,
}

impl NoiseMode {
    /// Every mode, each at its code.
    pub const ALL: [Self; 3] = [Self::White, Self::Long, Self::Short];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// What a matrix route reads: a value that moves while a note plays, or is
/// fixed by it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MatrixSource {
    /// The route is empty.
    #[default]
    Off,
    /// LFO 1 and 2 at their own rate and wave, from -1 to 1, whatever they target.
    Lfo1,
    Lfo2,
    /// The amplitude envelope, 0 to 1 (as it stood one sample ago).
    Envelope,
    /// The modulation envelope, 0 to 1 (as it stood one sample ago).
    ModEnvelope,
    /// The note's velocity, 0 to 1.
    Velocity,
    /// The note's pitch around C3, -1 to 1 over two octaves down and up.
    Key,
    /// A value drawn once per note, -1 to 1.
    Random,
}

impl MatrixSource {
    /// Every source, each at its code.
    pub const ALL: [Self; 8] =
        [Self::Off, Self::Lfo1, Self::Lfo2, Self::Envelope, Self::ModEnvelope, Self::Velocity, Self::Key, Self::Random];

    /// Its code: its place in [`Self::ALL`].
    #[must_use]
    pub const fn code(self) -> u8 {
        self as u8
    }
}

/// Routes of a patch's modulation matrix: room for the eight lines of an
/// older preset, the targets of its two LFOs and its modulation envelope's.
pub const MATRIX_SIZE: usize = 11;

/// One line of the matrix: `source`, times `amount`, times what one unit is
/// worth on `target`, is added to the target's value, every sample.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct MatrixRoute {
    pub source: MatrixSource,
    pub target: LfoTarget,
    /// From -1 to 1.
    pub amount: f64,
}

/// One LFO of a patch: a source of the matrix ([`MatrixSource::Lfo1`],
/// [`MatrixSource::Lfo2`]), which says where it lands and how far.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SynthLfo {
    pub wave: Wave,
    /// Hz; 0 or less is off.
    pub freq: f64,
    /// Seconds from the note's start over which the LFO fades in, from nothing (0: at once).
    pub delay_seconds: f64,
    /// Whether the LFO starts its cycle with each note, instead of running on the engine clock.
    pub is_retriggered: bool,
}

impl SynthLfo {
    /// An LFO of `wave` at `freq` Hz on the engine clock, at full depth from the note's start.
    #[must_use]
    pub const fn free(wave: Wave, freq: f64) -> Self {
        Self { wave, freq, delay_seconds: 0.0, is_retriggered: false }
    }
}

/// An ADSR, seconds and level.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Adsr {
    pub attack: f64,
    pub decay: f64,
    pub sustain: f64,
    pub release: f64,
}

/// The pitch envelope: a note starts `amount` semitones off its pitch and
/// falls back to it exponentially, as an analog drum's pitch does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PitchEnv {
    /// Semitones, up or down; 0 is off.
    pub amount: f64,
    /// Seconds the offset takes to fall to a hundredth of itself.
    pub decay: f64,
}

/// The filter envelope: its stages, and which way it moves the cutoff.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FilterEnv {
    /// Its own ADSR; `None`: up over the amplitude envelope's attack, down
    /// over its decay, then off (the presets written before it had one).
    pub adsr: Option<Adsr>,
    /// Whether it closes the filter (towards its lowest cutoff) instead of opening it.
    pub is_inverted: bool,
}

/// The second filter, after the first: a 12 dB/oct stage of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter2 {
    pub mode: FilterMode,
    pub freq: f64,
    pub q: f64,
}

/// How a track's notes share a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VoiceMode {
    /// Each note a voice of its own.
    #[default]
    Poly,
    /// One voice: each note restarts the envelopes and glides from the pitch of the one before.
    Mono,
    /// One voice: a note that comes while the one before is held glides to
    /// its pitch and keeps the envelopes going; any other restarts them, without gliding.
    Legato,
}

impl VoiceMode {
    /// Every mode, each at its code.
    pub const ALL: [Self; 3] = [Self::Poly, Self::Mono, Self::Legato];
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
    /// Each oscillator switched off: not heard, and modulating nothing (FM,
    /// ring; oscillator 1 its sub-oscillator too), whatever the matrix does.
    pub osc: [bool; 3],
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
    /// Share of a cycle a square wave is high, 0–1 (0.5: the plain square).
    pub osc_width: [f64; 3],
    /// How far oscillator 1 ring-modulates oscillator 2, 0–1 (1: oscillator 2
    /// sounds as the product of the two).
    pub ring: f64,
    /// Whether oscillator 2 restarts its cycle with each cycle of oscillator 1 (hard sync).
    pub is_osc2_synced: bool,
    pub noise_mix: f64,
    pub noise_mode: NoiseMode,
    /// Hz the shift register of a [`NoiseMode::Long`] or [`NoiseMode::Short`] noise is clocked at.
    pub noise_rate: f64,
    pub noise_filter_type: FilterMode,
    pub noise_filter_freq: f64,
    pub noise_filter_q: f64,
    pub filter_type: FilterMode,
    pub filter_freq: f64,
    pub filter_q: f64,
    /// dB per octave: 12, or 24 (two filter stages).
    pub filter_slope: f64,
    pub drive: f64,
    pub pitch_env: PitchEnv,
    pub sub_gain: f64,
    pub envelope: Adsr,
    pub volume: f64,
    pub pan: f64,
    /// Peak level of the note.
    pub velocity: f64,
    pub lfo1: SynthLfo,
    pub lfo2: SynthLfo,
    pub filter_env_amount: f64,
    pub filter_env: FilterEnv,
    /// The second filter; `None` when off.
    pub filter2: Option<Filter2>,
    pub voice_mode: VoiceMode,
    /// Seconds a glide takes to bring the pitch within a hundredth of the
    /// interval of the note (0: none). Mono and legato only.
    pub glide_seconds: f64,
    pub fm_amount: f64,
    /// Which oscillators modulate which, 0 to 4. A number and not an `enum`:
    /// an LFO sweeps it ([`LfoTarget::FmAlgorithm`]), and it is rounded to
    /// the nearest routing sample by sample.
    pub fm_algorithm: f64,
    /// The modulation envelope: a source of the matrix ([`MatrixSource::ModEnvelope`]).
    pub mod_envelope: Adsr,
    pub bypass: SynthBypass,
    /// Layers each oscillator sounds as, 1 (none) to [`super::MAX_UNISON`].
    pub unison_count: usize,
    /// Cents between the pitch and the outermost layer, on each side.
    pub unison_detune: f64,
    /// How far the layers spread over the stereo field, 0–1.
    pub unison_spread: f64,
    /// The modulation matrix: where the LFOs and the other sources land.
    pub matrix: [MatrixRoute; MATRIX_SIZE],
    /// The note's velocity before the patch's levels, 0–1: [`MatrixSource::Velocity`].
    pub note_velocity: f64,
    /// The note's pitch from C3 in octaves, -1 to 1: [`MatrixSource::Key`].
    pub note_key: f64,
}

impl Default for SynthPatch {
    fn default() -> Self {
        let lfo = SynthLfo::free(Wave::Sine, 1.0);
        Self {
            osc_freq: [440.0; 3],
            osc_gain: [0.5; 3],
            osc_detune: [0.0; 3],
            osc_wave: [Wave::Sine; 3],
            osc_width: [0.5; 3],
            ring: 0.0,
            is_osc2_synced: false,
            noise_mix: 0.0,
            noise_mode: NoiseMode::White,
            noise_rate: NOISE_RATE_DEFAULT,
            noise_filter_type: FilterMode::Lowpass,
            noise_filter_freq: 1000.0,
            noise_filter_q: 0.7,
            filter_type: FilterMode::Lowpass,
            filter_freq: 1000.0,
            filter_q: 0.7,
            filter_slope: 12.0,
            drive: 0.0,
            pitch_env: PitchEnv { amount: 0.0, decay: 0.05 },
            sub_gain: 0.0,
            envelope: Adsr { attack: 0.01, decay: 0.1, sustain: 0.7, release: 0.1 },
            volume: 0.8,
            pan: 0.0,
            velocity: 0.8,
            lfo1: lfo,
            lfo2: lfo,
            filter_env_amount: 0.0,
            filter_env: FilterEnv { adsr: None, is_inverted: false },
            filter2: None,
            voice_mode: VoiceMode::Poly,
            glide_seconds: 0.0,
            fm_amount: 0.0,
            fm_algorithm: 0.0,
            mod_envelope: Adsr { attack: 0.01, decay: 0.1, sustain: 0.0, release: 0.1 },
            bypass: SynthBypass::default(),
            unison_count: 1,
            unison_detune: 0.0,
            unison_spread: 0.0,
            matrix: [MatrixRoute::default(); MATRIX_SIZE],
            note_velocity: 0.8,
            note_key: 0.0,
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
        for (code, source) in MatrixSource::ALL.iter().enumerate() {
            assert_eq!(usize::from(source.code()), code);
        }
        for (code, mode) in NoiseMode::ALL.iter().enumerate() {
            assert_eq!(usize::from(mode.code()), code);
        }
    }
}
