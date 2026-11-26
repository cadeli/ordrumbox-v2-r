//! One synth note: three oscillators with FM and unison, a sub-oscillator, filtered
//! noise, drive, a TPT state-variable filter with its envelope, an ADSR, a
//! modulation envelope, a pitch envelope, ring modulation and hard sync of
//! oscillator 2 by oscillator 1, and two LFOs routed to any of their
//! targets, then equal-power pan.
//!
//! [`SynthVoice::start`] starts a note of a [`SynthPatch`] and
//! [`SynthVoice::release_at`] releases it. Starting, releasing and rendering
//! never allocate.
//!
//! The sub-oscillator sounds an octave below oscillator 1, the pitch punch
//! reaches two octaves, FM follows the note and goes through zero, each note
//! has its own noise, and the filter opens up to 0.45 × the sample rate.

use std::f64::consts::PI;

use crate::lfo::{self, Wave};
use crate::slope::{self, FilterMode};
use crate::svf::{Svf, tap, tap_weights};

mod cross_mod;
mod envelope;
mod filter;
mod modulation;
mod oscillator;
pub mod patch;
mod pitch_env;
mod sources;
mod unison;

pub use cross_mod::ring;
use cross_mod::{Moved, cycle_starts, restart};
use envelope::{Envelope, FilterEnvelope, Segment};
use filter::{coefficients, cutoff_coefficient};
pub use modulation::lfo_swing;
use modulation::{
    DETUNE_SLOTS, ENV_SLOTS, GAIN_SLOTS, Late, LateMods, MOD_ENV_SLOTS, Matrix, Mods, Slot, SourceValues, WIDTH_SLOTS,
    is_moved, is_targeted, modulated, modulated_adsr,
};
pub use oscillator::{FM_DEVIATION, fm_modulators};
use oscillator::{
    LFSR_SEED, NOISE_SEED, WIDTH_MAX, WIDTH_MIN, WaveTables, fm_frequencies, lfsr_sample, lfsr_step, noise_sample,
    noise_seed, oscillator, phase_step, sin_lookup, sine_table, xorshift32,
};
use patch::{Adsr, LfoTarget, MatrixSource, NoiseMode, SynthLfo, SynthPatch};
pub use pitch_env::MAX_PITCH_ENV_SEMITONES;
use pitch_env::{PitchFall, fall_per_sample};
pub use unison::MAX_UNISON;
use unison::{Layer, layers, start_phases, unison_oscillator};

/// Cents to a natural-log ratio.
const LN2_OVER_1200: f64 = std::f64::consts::LN_2 / 1200.0;
/// A level, an amount or a depth at or below this does nothing: its stage is skipped.
const SILENT: f64 = 0.001;

/// `x` inside `lo`–`hi`; `lo > hi` gives `lo` instead of a panic.
#[inline]
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.min(hi).max(lo)
}

/// The frequency ratio of a detune of `cents`.
#[inline]
fn cents_ratio(cents: f64) -> f64 {
    if cents == 0.0 { 1.0 } else { (cents * LN2_OVER_1200).exp() }
}

/// What a block of a note is rendered with: everything of the patch that no
/// sample changes, worked out once.
#[derive(Debug)]
struct Block<'a> {
    tables: WaveTables<'a>,
    sample_rate: f64,
    /// Phase step of the two LFOs, cycles per sample.
    lfo_steps: [f64; 2],
    /// Whether the modulation envelope runs: the matrix reads it, and it is not bypassed.
    runs_mod_env: bool,
    /// Whether a route of the matrix moves a late value ([`modulation::Late`]).
    has_late: bool,
    matrix: Matrix,
    /// The wave of each LFO the matrix reads; `None` when it is not read, stopped or bypassed.
    matrix_lfo_waves: [Option<Wave>; 2],
    filter_mode: FilterMode,
    filter_weights: [f64; 3],
    is_steep: bool,
    /// The filter's Q, coefficient and damping while nothing modulates it.
    q: f64,
    g: f64,
    k: f64,
    /// Pulse width of each oscillator, within what the band-limiting allows.
    widths: [f64; 3],
    /// Whether the pitch envelope runs: it has an offset, or something may give it one.
    has_pitch_env: bool,
    /// The share of the pitch envelope's offset a sample keeps, while nothing modulates its decay.
    pitch_fall: f64,
    /// Whether oscillator 2 may be ring-modulated: it is, or something may make it so.
    has_ring: bool,
    /// Whether oscillator 2 is hard-synced to oscillator 1.
    is_synced: bool,
    noise_mode: NoiseMode,
    /// Shift register steps a sample, 0–1.
    noise_step: f64,
    noise_weights: [f64; 3],
    noise_g: f64,
    noise_k: f64,
    pan_left: f64,
    pan_right: f64,
    /// The layers of each oscillator and how many are used.
    layers: [Layer; MAX_UNISON],
    layer_count: usize,
    /// Whether the layers differ from one side to the other: the left and
    /// right channels are then filtered apart.
    is_stereo: bool,
}

impl<'a> Block<'a> {
    fn new(p: &SynthPatch, sample_rate: f64, source: &'a [f32]) -> Self {
        let pan = clamp(p.pan, -1.0, 1.0);
        let q = clamp(p.filter_q, 0.1, 20.0);
        let layer_count = p.unison_count.clamp(1, MAX_UNISON);
        let matrix = Matrix::new(&p.matrix);
        let lfo_wave = |lfo: &SynthLfo, is_bypassed: bool, source| {
            (matrix.reads(source) && !is_bypassed && lfo.freq > 0.0).then_some(lfo.wave)
        };
        let matrix_lfo_waves = [
            lfo_wave(&p.lfo1, p.bypass.lfo1, MatrixSource::Lfo1),
            lfo_wave(&p.lfo2, p.bypass.lfo2, MatrixSource::Lfo2),
        ];
        Self {
            tables: WaveTables { sine: sine_table(), source },
            sample_rate,
            lfo_steps: [p.lfo1.freq / sample_rate, p.lfo2.freq / sample_rate],
            runs_mod_env: matrix.reads(MatrixSource::ModEnvelope) && !p.bypass.mod_env,
            has_late: matrix.has_late(),
            matrix,
            matrix_lfo_waves,
            filter_mode: p.filter_type,
            filter_weights: tap_weights(p.filter_type),
            is_steep: slope::is_steep(p.filter_slope),
            q,
            g: cutoff_coefficient(p.filter_freq, sample_rate),
            k: 1.0 / q,
            widths: p.osc_width.map(|width| clamp(width, WIDTH_MIN, WIDTH_MAX)),
            has_pitch_env: p.pitch_env.amount != 0.0 || is_targeted(p, LfoTarget::PitchEnvAmount),
            pitch_fall: fall_per_sample(p.pitch_env.decay, sample_rate),
            has_ring: !p.bypass.osc[0] && (p.ring > 0.0 || is_targeted(p, LfoTarget::Osc2Ring)),
            is_synced: p.is_osc2_synced,
            noise_mode: p.noise_mode,
            noise_step: clamp(p.noise_rate / sample_rate, 0.0, 1.0),
            noise_weights: tap_weights(p.noise_filter_type),
            noise_g: cutoff_coefficient(p.noise_filter_freq, sample_rate),
            noise_k: 1.0 / clamp(p.noise_filter_q, 0.1, 20.0),
            pan_left: ((pan + 1.0) * PI / 4.0).cos(),
            pan_right: ((pan + 1.0) * PI / 4.0).sin(),
            layers: layers(layer_count, p.unison_detune, p.unison_spread),
            layer_count,
            is_stereo: layer_count > 1 && p.unison_spread > SILENT,
        }
    }
}

/// One playing synth note.
#[derive(Debug, Clone)]
pub struct SynthVoice {
    active: bool,
    track: usize,
    serial: u64,
    patch: SynthPatch,
    /// Trigger and release times, seconds on the engine clock (-1: none).
    start_time: f64,
    release_time: f64,
    /// Phase and cycle count of each layer of each oscillator; layer 0 is on the pitch.
    phase: [[f64; MAX_UNISON]; 3],
    osc_cycle: [[f64; MAX_UNISON]; 3],
    rng: u32,
    /// The pulse width of each oscillator for the sample being worked out.
    width: [f64; 3],
    pitch_fall: PitchFall,
    /// The shift register of the console noise, and how far its next step is, 0–1.
    lfsr: u16,
    lfsr_phase: f64,
    /// The last value the noise generator gave, held between two register steps.
    noise_held: f64,
    /// What the matrix's sources read that a sample does not work out: the
    /// envelopes' last levels, and the value drawn for the note.
    sources: SourceValues,
    lfo_phase: [f64; 2],
    lfo_seeded: bool,
    filter: Svf,
    /// Second filter stage (24 dB/oct only).
    filter_2: Svf,
    /// The same two stages for the right channel, when the layers are spread.
    filter_right: Svf,
    filter_right_2: Svf,
    noise_filter: Svf,
    env: Envelope,
    filter_env: FilterEnvelope,
    mod_env: Envelope,
}

impl Default for SynthVoice {
    fn default() -> Self {
        sine_table(); // built here, not on the audio thread
        Self {
            active: false,
            track: 0,
            serial: 0,
            patch: SynthPatch::default(),
            start_time: -1.0,
            release_time: -1.0,
            phase: [[0.0; MAX_UNISON]; 3],
            osc_cycle: [[0.0; MAX_UNISON]; 3],
            rng: NOISE_SEED,
            width: [0.5; 3],
            pitch_fall: PitchFall::START,
            lfsr: LFSR_SEED,
            lfsr_phase: 0.0,
            noise_held: 0.0,
            sources: SourceValues::default(),
            lfo_phase: [0.0; 2],
            lfo_seeded: false,
            filter: Svf::default(),
            filter_2: Svf::default(),
            filter_right: Svf::default(),
            filter_right_2: Svf::default(),
            noise_filter: Svf::default(),
            env: Envelope::new(Segment::Idle),
            filter_env: FilterEnvelope::new(Segment::Idle),
            mod_env: Envelope::new(Segment::Idle),
        }
    }
}

impl SynthVoice {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn track(&self) -> usize {
        self.track
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Start a note at `start_time` (seconds on the clock [`Self::render`]
    /// is given), on a fresh voice.
    pub fn start(&mut self, patch: &SynthPatch, track: usize, start_time: f64, serial: u64) {
        *self = Self {
            active: true,
            track,
            serial,
            patch: *patch,
            start_time,
            rng: noise_seed(serial),
            sources: SourceValues {
                velocity: clamp(patch.note_velocity, 0.0, 1.0),
                key: clamp(patch.note_key, -1.0, 1.0),
                random: noise_sample(xorshift32(noise_seed(serial))),
                ..SourceValues::default()
            },
            env: Envelope::new(Segment::Attack),
            filter_env: FilterEnvelope::new(Segment::Attack),
            mod_env: Envelope::new(Segment::Attack),
            ..Self::default()
        };
        if patch.unison_count > 1 {
            // Each oscillator's layers start apart: the saws of a supersaw
            // do not rise together.
            for (n, phases) in self.phase.iter_mut().enumerate() {
                *phases = start_phases(noise_seed(serial).rotate_left(8 * n as u32 + 5));
            }
        }
    }

    /// Release the note at `time` (seconds, same clock), replacing any release
    /// already set.
    pub fn release_at(&mut self, time: f64) {
        if self.active {
            self.release_time = time;
        }
    }

    /// ADD the next `left.len()` frames to `left`/`right`; `first_frame` is
    /// the clock position of the first one (in frames at `sample_rate`).
    /// The random wave of the oscillators holds frames of `source`, one a
    /// cycle; with no frame, it draws pseudo-random values.
    pub fn render(&mut self, sample_rate: f32, first_frame: u64, source: &[f32], left: &mut [f32], right: &mut [f32]) {
        if !self.active {
            return;
        }
        let patch = self.patch;
        let block = Block::new(&patch, f64::from(sample_rate), source);
        // Without a route, every slot stays 0 for the whole block.
        let mut mods = Mods::default();
        let has_matrix = !block.matrix.is_empty();
        for (i, (out_left, out_right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let time = (first_frame + i as u64) as f64 / block.sample_rate;
            let t = time - self.start_time;
            if t < 0.0 {
                continue;
            }
            self.step_lfos(&patch, &block, time);
            if has_matrix {
                block.matrix.apply(&self.sources, &mut mods);
            }
            let y = self.sample(&patch, &block, &mods, time, t);
            *out_left += (y[0] * block.pan_left) as f32;
            *out_right += (y[1] * block.pan_right) as f32;
        }
        if self.env.is_idle() {
            self.active = false;
        }
    }

    /// Move the LFOs on by a sample and set the values the matrix reads of them.
    #[inline]
    fn step_lfos(&mut self, p: &SynthPatch, b: &Block, time: f64) {
        if self.lfo_seeded {
            self.lfo_phase[0] += b.lfo_steps[0];
            self.lfo_phase[1] += b.lfo_steps[1];
        } else {
            // LFOs run on the engine clock, not from the note's start.
            self.lfo_phase = [time * p.lfo1.freq, time * p.lfo2.freq];
            self.lfo_seeded = true;
        }
        for ((wave, phase), value) in b.matrix_lfo_waves.iter().zip(self.lfo_phase).zip(&mut self.sources.lfo) {
            *value = wave.map_or(0.0, |wave| lfo::wave(phase, wave));
        }
    }

    /// Whether the note's release has come at `time`.
    #[inline]
    fn is_released(&self, time: f64) -> bool {
        self.release_time > 0.0 && time >= self.release_time
    }

    /// The left and right samples at `time`, `t` seconds into the note, before the pan.
    #[inline]
    fn sample(&mut self, p: &SynthPatch, b: &Block, mods: &Mods, time: f64, t: f64) -> [f64; 2] {
        let env = modulated_adsr(mods, p.envelope, ENV_SLOTS);
        self.step_mod_env(p, b, mods, time, t);
        // After the modulation envelope: the late values read it as it is now.
        let late = if b.has_late { b.matrix.late(&self.sources) } else { LateMods::default() };
        let filter_open = late[Late::FilterOpen];
        let filter_env_amount = modulated(mods, p.filter_env_amount, Slot::FilterEnvAmount).max(0.0);
        let cutoff = self.cutoff(p, mods, &env, filter_env_amount, filter_open, t);

        let mut gains = [0.0; 3];
        for ((gain, base), slot) in gains.iter_mut().zip(p.osc_gain).zip(GAIN_SLOTS) {
            *gain = clamp(modulated(mods, base, slot), 0.0, 1.0);
        }
        self.width = pulse_widths(b, mods, p.osc_width);
        let pitch_env = if b.has_pitch_env { self.pitch_env_ratio(p, b, mods) } else { 1.0 };
        let frequencies = self.frequencies(p, b, mods, &mut gains, (&late, pitch_env));
        // After the matrix and the shape, which may raise a gain: off stays silent.
        for (gain, is_off) in gains.iter_mut().zip(p.bypass.osc) {
            if is_off {
                *gain = 0.0;
            }
        }
        let oscillators = self.oscillators(p, b, mods, frequencies, gains);
        let noise = self.noise(p, b, mods);
        let mut dry = [oscillators[0] * (1.0 - p.noise_mix) + noise, oscillators[1] * (1.0 - p.noise_mix) + noise];

        let drive = clamp(modulated(mods, p.drive, Slot::Drive), 0.0, 1.0);
        if drive > SILENT {
            let saturate = |x: f64| {
                let driven = x * (1.0 + drive * 3.5);
                driven / (1.0 + driven.abs() * 0.6)
            };
            dry[0] = saturate(dry[0]);
            dry[1] = if b.is_stereo { saturate(dry[1]) } else { dry[0] };
        }

        let is_swept = (filter_env_amount > SILENT && !self.filter_env.is_idle()) || filter_open.abs() > SILENT;
        let filtered = if p.bypass.filter { dry } else { self.filter(b, mods, dry, cutoff, is_swept) };

        if self.is_released(time) {
            self.env.release(t);
        }
        let peak = p.velocity;
        let level = self.env.step(t, env, peak, |norm| peak * norm * norm);
        if peak > 0.0 {
            self.sources.envelope = level / peak;
        }
        let volume = modulated(mods, p.volume, Slot::Volume).max(0.0);
        let gain = (if p.bypass.env { 1.0 } else { level }) * volume;
        [filtered[0] * gain, filtered[1] * gain]
    }

    /// Move the modulation envelope on by a sample, when the matrix reads it,
    /// and set the value it reads (0–1).
    #[inline]
    fn step_mod_env(&mut self, p: &SynthPatch, b: &Block, mods: &Mods, time: f64, t: f64) {
        if !b.runs_mod_env {
            return;
        }
        if self.is_released(time) {
            self.mod_env.release(t);
        }
        let level = self.mod_env.step(t, modulated_adsr(mods, p.mod_envelope, MOD_ENV_SLOTS), 1.0, |norm| norm);
        self.sources.mod_envelope = level;
    }

    /// The filter cutoff of this sample, Hz: the patch's, moved by the matrix,
    /// opened by the filter envelope and by `filter_open` (a share of the way to the top).
    #[inline]
    fn cutoff(
        &mut self,
        p: &SynthPatch,
        mods: &Mods,
        env: &Adsr,
        filter_env_amount: f64,
        filter_open: f64,
        t: f64,
    ) -> f64 {
        let mut cutoff = modulated(mods, p.filter_freq, Slot::Filter);
        if !p.bypass.filter_env && filter_env_amount > SILENT && !self.filter_env.is_idle() {
            let level = self.filter_env.step(t, env.attack, env.decay);
            cutoff += (crate::MAX_FILTER_FREQ - cutoff) * filter_env_amount * level;
        }
        if filter_open.abs() > SILENT {
            cutoff += (crate::MAX_FILTER_FREQ - cutoff) * filter_open;
        }
        cutoff
    }

    /// `x` (left, right) through the main filter at `cutoff` Hz. The
    /// coefficients are the block's unless an LFO or an envelope moves them
    /// (`is_swept`) for this sample. The right channel has its own filter
    /// when the layers are spread, else it is the left one.
    #[inline]
    fn filter(&mut self, b: &Block, mods: &Mods, x: [f64; 2], cutoff: f64, is_swept: bool) -> [f64; 2] {
        let is_modulated = is_swept || is_moved(mods, Slot::Filter) || is_moved(mods, Slot::FilterQ);
        let (g, k) = if is_modulated {
            let q = clamp(modulated(mods, b.q, Slot::FilterQ), 0.1, 20.0);
            (cutoff_coefficient(cutoff, b.sample_rate), 1.0 / q)
        } else {
            (b.g, b.k)
        };
        let left = filter_channel(&mut self.filter, &mut self.filter_2, b, g, k, x[0]);
        let right = if b.is_stereo {
            filter_channel(&mut self.filter_right, &mut self.filter_right_2, b, g, k, x[1])
        } else {
            left
        };
        [left, right]
    }
}

/// The pulse widths of this sample: the block's, unless an LFO or the matrix moves one.
#[inline]
fn pulse_widths(b: &Block, mods: &Mods, base: [f64; 3]) -> [f64; 3] {
    if !WIDTH_SLOTS.iter().any(|&slot| is_moved(mods, slot)) {
        return b.widths;
    }
    std::array::from_fn(|n| clamp(modulated(mods, base[n], WIDTH_SLOTS[n]), WIDTH_MIN, WIDTH_MAX))
}

/// `x` through the filter stages `first` and `second` (24 dB/oct only), at
/// coefficient `g` and damping `k`.
#[inline]
fn filter_channel(first: &mut Svf, second: &mut Svf, b: &Block, g: f64, k: f64, x: f64) -> f64 {
    if !b.is_steep {
        return tap(first.step(x, &coefficients(g, k)), b.filter_weights);
    }
    let (k1, k2, gain_2) = slope::stages(k, b.filter_mode);
    let stage_1 = tap(first.step(x, &coefficients(g, k1)), b.filter_weights);
    tap(second.step(stage_1, &coefficients(g, k2)), b.filter_weights) * gain_2
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::patch::{LfoTarget, MATRIX_SIZE, MatrixRoute, SynthBypass};
    use super::*;

    fn render(voice: &mut SynthVoice, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        voice.render(48_000.0, 0, &[], &mut l, &mut r);
        l
    }

    #[test]
    fn note_starts_on_time_and_ends_after_its_release() {
        let mut voice = SynthVoice::default();
        let patch = SynthPatch { volume: 1.0, ..SynthPatch::default() };
        voice.start(&patch, 3, 100.0 / 48_000.0, 1);
        voice.release_at(0.05);
        assert!(voice.is_active() && voice.track() == 3);
        let out = render(&mut voice, 48_000 / 4);
        assert!(out[..100].iter().all(|&s| s == 0.0));
        assert!(out[100..2400].iter().any(|s| s.abs() > 0.01));
        // Release 0.1 s from 0.05 s: silent from 0.15 s.
        assert!(out[7300..].iter().all(|&s| s == 0.0));
        assert!(!voice.is_active());
    }

    /// A held note of `patch` at 100 Hz, with no filter and no envelope, from
    /// its start. Oscillators not set are silent.
    fn held(patch: SynthPatch, frames: usize) -> Vec<f32> {
        let mut voice = SynthVoice::default();
        let bypass = SynthBypass { filter: true, env: true, ..patch.bypass };
        let base = SynthPatch { osc_freq: [100.0; 3], volume: 1.0, bypass, ..patch };
        voice.start(&base, 0, 0.0, 1);
        render(&mut voice, frames)
    }

    /// Strength of `freq` in `signal` at 48 kHz.
    fn tone_level(signal: &[f32], freq: f64) -> f64 {
        let (mut re, mut im) = (0.0, 0.0);
        for (i, &x) in signal.iter().enumerate() {
            let phase = TAU * freq * i as f64 / 48_000.0;
            re += f64::from(x) * phase.cos();
            im += f64::from(x) * phase.sin();
        }
        (re * re + im * im).sqrt() / signal.len() as f64
    }

    /// Rising zero crossings of `signal`.
    fn cycles(signal: &[f32]) -> usize {
        signal.windows(2).filter(|pair| pair[0] < 0.0 && pair[1] >= 0.0).count()
    }

    #[test]
    fn the_pitch_envelope_starts_off_the_note_and_falls_back_to_it() {
        let swept = |amount: f64| {
            let pitch_env = super::patch::PitchEnv { amount, decay: 0.05 };
            held(SynthPatch { osc_gain: [1.0, 0.0, 0.0], pitch_env, ..SynthPatch::default() }, 24_000)
        };
        let (up, down, plain) = (swept(24.0), swept(-12.0), swept(0.0));
        // The first cycle ends after 480 samples at 100 Hz, sooner from above, later from below.
        let first_cycle = |out: &[f32]| out.windows(2).position(|pair| pair[0] < 0.0 && pair[1] >= 0.0);
        let (up_end, down_end) = (first_cycle(&up).unwrap(), first_cycle(&down).unwrap());
        assert!(up_end < 200 && down_end > 600 && first_cycle(&plain) == Some(479), "{up_end} {down_end}");
        // Back on the note: 20 cycles in the last 0.2 s, one more or less by where its phase stands.
        assert!(cycles(&up[14_400..]).abs_diff(cycles(&plain[14_400..])) <= 1);
    }

    /// Oscillator 2 alone, a sawtooth 17 semitones above oscillator 1 (100 Hz).
    fn osc2_saw(is_osc2_synced: bool, unison_count: usize) -> Vec<f32> {
        let patch = SynthPatch {
            osc_gain: [0.0, 1.0, 0.0],
            osc_wave: [Wave::Sine, Wave::Sawtooth, Wave::Sine],
            osc_detune: [0.0, 1700.0, 0.0],
            is_osc2_synced,
            unison_count,
            unison_detune: 20.0,
            ..SynthPatch::default()
        };
        held(patch, 4800)
    }

    #[test]
    fn a_synced_oscillator_2_repeats_with_oscillator_1() {
        // One cycle of oscillator 1 at 100 Hz is 480 samples.
        let largest_gap =
            |out: &[f32]| out[480..4320].iter().zip(&out[960..]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(largest_gap(&osc2_saw(true, 1)) < 0.05, "{}", largest_gap(&osc2_saw(true, 1)));
        assert!(largest_gap(&osc2_saw(false, 1)) > 0.5);
        let layered = osc2_saw(true, 4);
        assert!(layered.iter().all(|s| s.is_finite()) && layered.iter().any(|s| s.abs() > 0.1));
    }

    #[test]
    fn ring_modulation_leaves_the_sum_and_difference() {
        let ring_of = |ring: f64| {
            let patch = SynthPatch {
                osc_gain: [0.0, 1.0, 0.0],
                // Oscillator 2 at 300 Hz.
                osc_detune: [0.0, 1200.0 * 3.0_f64.log2(), 0.0],
                ring,
                ..SynthPatch::default()
            };
            let out = held(patch, 48_000);
            [200.0, 300.0, 400.0].map(|freq| tone_level(&out, freq))
        };
        let (plain, ringed) = (ring_of(0.0), ring_of(1.0));
        assert!(plain[1] > 0.3 && plain[0] < 0.01, "{plain:?}");
        // sin(a) sin(b) = (cos(a - b) - cos(a + b)) / 2.
        assert!(ringed[1] < 0.01 && ringed[0] > 0.08 && ringed[2] > 0.08, "{ringed:?}");
    }

    #[test]
    fn a_24_db_lowpass_falls_12_db_per_octave_faster() {
        let saw = |filter_slope: f64| {
            let mut voice = SynthVoice::default();
            let patch = SynthPatch {
                osc_freq: [100.0; 3],
                osc_gain: [1.0, 0.0, 0.0],
                osc_wave: [Wave::Sawtooth; 3],
                filter_freq: 1_000.0,
                filter_slope,
                volume: 1.0,
                bypass: SynthBypass { env: true, ..SynthBypass::default() },
                ..SynthPatch::default()
            };
            voice.start(&patch, 0, 0.0, 1);
            let out = render(&mut voice, 48_000);
            [2_000.0, 4_000.0].map(|f| 20.0 * tone_level(&out[24_000..], f).log10())
        };
        let (one, two) = (saw(12.0), saw(24.0));
        let extra = (one[1] - two[1]) - (one[0] - two[0]);
        assert!((extra - 12.0).abs() < 2.0, "{one:?} {two:?}");
    }

    #[test]
    fn an_oscillator_switched_off_is_not_heard_and_modulates_nothing() {
        let fm = SynthPatch { osc_gain: [1.0, 0.0, 0.0], fm_amount: 1.0, ..SynthPatch::default() };
        let plain = held(SynthPatch { fm_amount: 0.0, ..fm }, 4_800);
        assert_ne!(held(fm, 4_800), plain, "silent, oscillator 2 modulates");
        let off = SynthBypass { osc: [false, true, false], ..SynthBypass::default() };
        assert_eq!(held(SynthPatch { bypass: off, ..fm }, 4_800), plain);
        assert_eq!(
            held(SynthPatch { osc_gain: [1.0; 3], bypass: off, ..fm }, 4_800),
            held(SynthPatch { osc_gain: [1.0, 0.0, 1.0], fm_amount: 0.0, ..fm }, 4_800)
        );
        let first_off = SynthBypass { osc: [true, false, false], ..SynthBypass::default() };
        let sub = SynthPatch { osc_gain: [1.0, 0.0, 0.0], sub_gain: 1.0, bypass: first_off, ..SynthPatch::default() };
        assert!(held(sub, 4_800).iter().all(|s| *s == 0.0), "oscillator 1 takes its sub-oscillator with it");
    }

    #[test]
    fn the_sub_sounds_an_octave_below_with_no_dc() {
        let out = held(SynthPatch { osc_gain: [0.0; 3], sub_gain: 1.0, ..SynthPatch::default() }, 48_000);
        let dc = out.iter().map(|&x| f64::from(x)).sum::<f64>() / out.len() as f64;
        assert!(dc.abs() < 1e-3, "{dc}");
        assert!(tone_level(&out, 50.0) > 100.0 * tone_level(&out, 100.0));
    }

    #[test]
    fn fm_keeps_the_pitch_and_follows_the_note() {
        // The carrier goes well below zero on every cycle of its modulator.
        let spectrum = |freq: f64| {
            let mut voice = SynthVoice::default();
            let bypass = SynthBypass { filter: true, env: true, ..SynthBypass::default() };
            let patch = SynthPatch {
                osc_freq: [freq, freq * 2.0, freq],
                osc_gain: [1.0, 0.0, 0.0],
                fm_amount: 1.0,
                bypass,
                ..SynthPatch::default()
            };
            voice.start(&patch, 0, 0.0, 1);
            let out = render(&mut voice, 48_000);
            [1.0, 1.5, 3.0, 5.0].map(|k| tone_level(&out, freq * k))
        };
        let (low, high) = (spectrum(200.0), spectrum(400.0));
        // Only odd multiples of the note: sidebands at the carrier ± 2n × the note.
        assert!(low[0] > 50.0 * low[1], "{low:?}");
        for (a, b) in low.iter().zip(&high) {
            assert!((a - b).abs() < 0.03, "{low:?} {high:?}");
        }
    }

    #[test]
    fn each_note_has_its_own_noise_and_replays_it() {
        let note = |serial: u64| {
            let mut voice = SynthVoice::default();
            let patch = SynthPatch { noise_mix: 1.0, ..SynthPatch::default() };
            voice.start(&patch, 0, 0.0, serial);
            render(&mut voice, 2048)
        };
        assert_eq!(note(3), note(3));
        assert_ne!(note(3), note(4));
    }

    fn routed(route: MatrixRoute) -> SynthPatch {
        let mut matrix = [MatrixRoute::default(); MATRIX_SIZE];
        matrix[3] = route;
        SynthPatch { osc_gain: [1.0, 0.0, 0.0], matrix, ..SynthPatch::default() }
    }

    fn peak(out: &[f32]) -> f32 {
        out.iter().fold(0.0, |peak, s| peak.max(s.abs()))
    }

    #[test]
    fn an_empty_matrix_changes_nothing() {
        let plain = SynthPatch { osc_gain: [1.0, 0.0, 0.0], ..SynthPatch::default() };
        let silent_routes = routed(MatrixRoute { source: MatrixSource::Lfo1, target: LfoTarget::Volume, amount: 0.0 });
        assert_eq!(held(plain, 2_000), held(silent_routes, 2_000));
    }

    #[test]
    fn velocity_through_the_matrix_sets_the_volume() {
        let route = |velocity| {
            let patch = routed(MatrixRoute { source: MatrixSource::Velocity, target: LfoTarget::Volume, amount: -1.0 });
            // The volume of 1 falls to 1 - velocity.
            peak(&held(SynthPatch { note_velocity: velocity, ..patch }, 2_000))
        };
        assert!(route(0.0) > 0.4, "no velocity leaves the volume");
        assert!(route(0.9) < route(0.0) * 0.2, "a high velocity takes it down");
    }

    #[test]
    fn a_negative_amount_inverts_the_source() {
        let at = |amount| {
            let patch = routed(MatrixRoute { source: MatrixSource::Key, target: LfoTarget::Osc1Detune, amount });
            let mut voice = SynthVoice::default();
            voice.start(&SynthPatch { note_key: 1.0, ..patch }, 0, 0.0, 1);
            render(&mut voice, 4_800)
        };
        // 100 cents either way from 440 Hz: the two renders cannot be the same.
        assert_ne!(at(1.0), at(-1.0));
    }

    #[test]
    fn the_modulation_envelope_is_read_without_a_target() {
        let patch = routed(MatrixRoute { source: MatrixSource::ModEnvelope, target: LfoTarget::Volume, amount: -1.0 });
        let patch = SynthPatch { mod_envelope: Adsr { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.1 }, ..patch };
        // The envelope sits at 1 and the volume goes to 0, once the first sample has set it.
        let out = held(patch, 2_000);
        assert!(peak(&out[10..]) < 1e-3, "{}", peak(&out[10..]));
    }

    /// The console noise of `mode` at `rate` Hz, 48 kHz, `count` samples.
    fn console_noise(mode: NoiseMode, rate: f64, count: usize) -> Vec<f64> {
        let patch = SynthPatch { noise_mode: mode, noise_rate: rate, ..SynthPatch::default() };
        let block = Block::new(&patch, 48_000.0, &[]);
        let mods = Mods::default();
        let mut voice = SynthVoice::default();
        (0..count).map(|_| voice.noise_source(&patch, &block, &mods)).collect()
    }

    #[test]
    fn the_short_noise_loops_after_93_steps_and_the_long_one_does_not() {
        let short = console_noise(NoiseMode::Short, 48_000.0, 400);
        assert!(short.iter().all(|x| x.abs() == 1.0));
        assert!((0..300).all(|i| short[i] == short[i + 93]));
        let long = console_noise(NoiseMode::Long, 48_000.0, 400);
        assert!((0..300).any(|i| long[i] != long[i + 93]));
    }

    #[test]
    fn an_lfo_sweeps_the_pulse_width_and_the_noise_clock() {
        let lfo = SynthLfo { wave: Wave::Square, freq: 2.0 };
        let route = MatrixRoute { source: MatrixSource::Lfo1, target: LfoTarget::Osc1Width, amount: 1.0 };
        let held = |patch: SynthPatch| {
            let mut voice = SynthVoice::default();
            voice.start(&patch, 0, 0.0, 1);
            let (mut l, mut r) = (vec![0.0; 24_000], vec![0.0; 24_000]);
            voice.render(48_000.0, 0, &[], &mut l, &mut r);
            l
        };
        let base = SynthPatch {
            osc_freq: [100.0; 3],
            osc_gain: [1.0, 0.0, 0.0],
            osc_wave: [Wave::Square; 3],
            volume: 1.0,
            bypass: SynthBypass { filter: true, env: true, ..SynthBypass::default() },
            ..SynthPatch::default()
        };
        // Share of the samples above zero: the width follows a square LFO: its floor, then its ceiling.
        let high = |out: &[f32]| out.iter().filter(|x| **x > 0.0).count() as f64 / out.len() as f64;
        let swept = held(SynthPatch { lfo1: lfo, matrix: [route; MATRIX_SIZE], ..base });
        // The LFO reads a quarter cycle late: low for the first 6000 samples, then high for 12000.
        assert!(high(&swept[..5000]) < 0.05, "{}", high(&swept[..5000]));
        assert!(high(&swept[8000..16_000]) > 0.9, "{}", high(&swept[8000..16_000]));
        assert!((high(&held(base)) - 0.5).abs() < 0.02);
    }

    #[test]
    fn the_noise_register_holds_its_value_between_two_steps() {
        // A clock at a sixth of the sample rate: each value lasts six samples.
        let slow = console_noise(NoiseMode::Long, 8_000.0, 600);
        assert!(slow[6..].chunks(6).all(|run| run.iter().all(|x| *x == run[0])));
    }
}
