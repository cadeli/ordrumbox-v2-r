//! One synth note: three oscillators with FM, a sub-oscillator, filtered
//! noise, drive, a TPT state-variable filter with its envelope, an ADSR, a
//! modulation envelope and two LFOs routed to any of 33 targets, then
//! equal-power pan.
//!
//! [`SynthVoice::start`] starts a note of a [`SynthPatch`] and
//! [`SynthVoice::release_at`] releases it. Starting, releasing and rendering
//! never allocate.
//!
//! The sub-oscillator sounds an octave below oscillator 1, the pitch punch
//! reaches two octaves, FM follows the note and goes through zero, each note
//! has its own noise, and the filter opens up to 0.45 × the sample rate.

use std::f64::consts::PI;

use crate::slope::{self, FilterMode};
use crate::svf::{Svf, tap, tap_weights};

mod envelope;
mod filter;
mod modulation;
mod oscillator;
pub mod patch;

use envelope::{Envelope, FilterEnvelope, Segment};
use filter::{coefficients, cutoff_coefficient};
use modulation::{
    DETUNE_SLOTS, ENV_SLOTS, GAIN_SLOTS, LfoMods, LfoRoute, MOD_ENV_SLOTS, Slot, modulated, modulated_adsr,
};
pub use oscillator::FM_DEVIATION;
use oscillator::{
    NOISE_SEED, SineTable, fm_frequencies, noise_sample, noise_seed, oscillator, sin_lookup, sine_table, xorshift32,
};
use patch::{Adsr, ModEnvTarget, SynthPatch};

/// Cents to a natural-log ratio.
const LN2_OVER_1200: f64 = std::f64::consts::LN_2 / 1200.0;
/// Pitch rise of a full pitch punch at the start of a note, cents.
const PITCH_PUNCH_CENTS: f64 = 2400.0;
/// How long the pitch punch lasts, seconds.
const PITCH_PUNCH_SECONDS: f64 = 0.06;
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
struct Block {
    table: &'static SineTable,
    sample_rate: f64,
    lfos: [Option<LfoRoute>; 2],
    /// Phase step of the two LFOs, cycles per sample.
    lfo_steps: [f64; 2],
    /// What the modulation envelope moves; off when it is bypassed or has no depth.
    mod_target: ModEnvTarget,
    filter_mode: FilterMode,
    filter_weights: [f64; 3],
    is_steep: bool,
    /// The filter's Q, coefficient and damping while nothing modulates it.
    q: f64,
    g: f64,
    k: f64,
    noise_weights: [f64; 3],
    noise_g: f64,
    noise_k: f64,
    pan_left: f64,
    pan_right: f64,
}

impl Block {
    fn new(p: &SynthPatch, sample_rate: f64) -> Self {
        let pan = clamp(p.pan, -1.0, 1.0);
        let q = clamp(p.filter_q, 0.1, 20.0);
        let has_mod_env = !p.bypass.mod_env && p.mod_env_depth > SILENT;
        Self {
            table: sine_table(),
            sample_rate,
            lfos: [LfoRoute::new(&p.lfo1, p.bypass.lfo1), LfoRoute::new(&p.lfo2, p.bypass.lfo2)],
            lfo_steps: [p.lfo1.freq / sample_rate, p.lfo2.freq / sample_rate],
            mod_target: if has_mod_env { p.mod_env_target } else { ModEnvTarget::Off },
            filter_mode: p.filter_type,
            filter_weights: tap_weights(p.filter_type),
            is_steep: slope::is_steep(p.filter_slope),
            q,
            g: cutoff_coefficient(p.filter_freq, sample_rate),
            k: 1.0 / q,
            noise_weights: tap_weights(p.noise_filter_type),
            noise_g: cutoff_coefficient(p.noise_filter_freq, sample_rate),
            noise_k: 1.0 / clamp(p.noise_filter_q, 0.1, 20.0),
            pan_left: ((pan + 1.0) * PI / 4.0).cos(),
            pan_right: ((pan + 1.0) * PI / 4.0).sin(),
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
    phase: [f64; 3],
    osc_cycle: [f64; 3],
    rng: u32,
    lfo_phase: [f64; 2],
    lfo_seeded: bool,
    filter: Svf,
    /// Second filter stage (24 dB/oct only).
    filter_2: Svf,
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
            phase: [0.0; 3],
            osc_cycle: [0.0; 3],
            rng: NOISE_SEED,
            lfo_phase: [0.0; 2],
            lfo_seeded: false,
            filter: Svf::default(),
            filter_2: Svf::default(),
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
            env: Envelope::new(Segment::Attack),
            filter_env: FilterEnvelope::new(Segment::Attack),
            mod_env: Envelope::new(Segment::Attack),
            ..Self::default()
        };
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
    pub fn render(&mut self, sample_rate: f32, first_frame: u64, left: &mut [f32], right: &mut [f32]) {
        if !self.active {
            return;
        }
        let patch = self.patch;
        let block = Block::new(&patch, f64::from(sample_rate));
        // Each LFO writes its one slot; the others stay 0 for the whole block.
        let mut mods = [LfoMods::default(); 2];
        for (i, (out_left, out_right)) in left.iter_mut().zip(right.iter_mut()).enumerate() {
            let time = (first_frame + i as u64) as f64 / block.sample_rate;
            let t = time - self.start_time;
            if t < 0.0 {
                continue;
            }
            self.step_lfos(&patch, &block, time, &mut mods);
            let y = self.sample(&patch, &block, &mods, time, t);
            *out_left += (y * block.pan_left) as f32;
            *out_right += (y * block.pan_right) as f32;
        }
        if self.env.is_idle() {
            self.active = false;
        }
    }

    /// Move the LFOs on by a sample and write what they add into `mods`.
    #[inline]
    fn step_lfos(&mut self, p: &SynthPatch, b: &Block, time: f64, mods: &mut [LfoMods; 2]) {
        if self.lfo_seeded {
            self.lfo_phase[0] += b.lfo_steps[0];
            self.lfo_phase[1] += b.lfo_steps[1];
        } else {
            // LFOs run on the engine clock, not from the note's start.
            self.lfo_phase = [time * p.lfo1.freq, time * p.lfo2.freq];
            self.lfo_seeded = true;
        }
        for ((route, phase), mods) in b.lfos.iter().zip(self.lfo_phase).zip(mods) {
            if let Some(route) = route {
                route.apply(phase, mods);
            }
        }
    }

    /// Whether the note's release has come at `time`.
    #[inline]
    fn is_released(&self, time: f64) -> bool {
        self.release_time > 0.0 && time >= self.release_time
    }

    /// The sample at `time`, `t` seconds into the note, before the pan.
    #[inline]
    fn sample(&mut self, p: &SynthPatch, b: &Block, mods: &[LfoMods; 2], time: f64, t: f64) -> f64 {
        let env = modulated_adsr(mods, p.envelope, ENV_SLOTS);
        let mod_level = self.mod_env_level(p, b, mods, time, t);
        let filter_env_amount = modulated(mods, p.filter_env_amount, Slot::FilterEnvAmount).max(0.0);
        let cutoff = self.cutoff(p, b, mods, &env, filter_env_amount, mod_level, t);

        let mut gains = [0.0; 3];
        for ((gain, base), slot) in gains.iter_mut().zip(p.osc_gain).zip(GAIN_SLOTS) {
            *gain = clamp(modulated(mods, base, slot), 0.0, 1.0);
        }
        let frequencies = self.frequencies(p, b, mods, &mut gains, mod_level, t);
        let oscillators = self.oscillators(p, b, mods, frequencies, gains);
        let mut dry = oscillators * (1.0 - p.noise_mix) + self.noise(p, b, mods);

        let drive = clamp(modulated(mods, p.drive, Slot::Drive), 0.0, 1.0);
        if drive > SILENT {
            let driven = dry * (1.0 + drive * 3.5);
            dry = driven / (1.0 + driven.abs() * 0.6);
        }

        let is_swept = (filter_env_amount > SILENT && !self.filter_env.is_idle())
            || (b.mod_target == ModEnvTarget::Filter && mod_level > SILENT);
        let filtered = if p.bypass.filter { dry } else { self.filter(b, mods, dry, cutoff, is_swept) };

        if self.is_released(time) {
            self.env.release(t);
        }
        let peak = p.velocity;
        let level = self.env.step(t, env, peak, |norm| peak * norm * norm);
        let volume = modulated(mods, p.volume, Slot::Volume).max(0.0);
        filtered * ((if p.bypass.env { 1.0 } else { level }) * volume)
    }

    /// The level of the modulation envelope (0–1); 0 when it moves nothing.
    #[inline]
    fn mod_env_level(&mut self, p: &SynthPatch, b: &Block, mods: &[LfoMods; 2], time: f64, t: f64) -> f64 {
        if b.mod_target == ModEnvTarget::Off {
            return 0.0;
        }
        if self.is_released(time) {
            self.mod_env.release(t);
        }
        self.mod_env.step(t, modulated_adsr(mods, p.mod_envelope, MOD_ENV_SLOTS), 1.0, |norm| norm)
    }

    /// The filter cutoff of this sample, Hz: the patch's, moved by the LFOs
    /// and opened by the filter envelope and the modulation envelope.
    #[inline]
    #[expect(clippy::too_many_arguments, reason = "the values of one sample, worked out by `sample`")]
    fn cutoff(
        &mut self,
        p: &SynthPatch,
        b: &Block,
        mods: &[LfoMods; 2],
        env: &Adsr,
        filter_env_amount: f64,
        mod_level: f64,
        t: f64,
    ) -> f64 {
        let mut cutoff = modulated(mods, p.filter_freq, Slot::Filter);
        if !p.bypass.filter_env && filter_env_amount > SILENT && !self.filter_env.is_idle() {
            let level = self.filter_env.step(t, env.attack, env.decay);
            cutoff += (crate::MAX_FILTER_FREQ - cutoff) * filter_env_amount * level;
        }
        if b.mod_target == ModEnvTarget::Filter && mod_level > SILENT {
            cutoff += (crate::MAX_FILTER_FREQ - cutoff) * p.mod_env_depth * mod_level;
        }
        cutoff
    }

    /// The frequencies of the three oscillators for this sample, Hz: detune,
    /// pitch punch, FM and what the modulation envelope adds, which also
    /// raises `gains` when it shapes.
    #[inline]
    fn frequencies(
        &self,
        p: &SynthPatch,
        b: &Block,
        mods: &[LfoMods; 2],
        gains: &mut [f64; 3],
        mod_level: f64,
        t: f64,
    ) -> [f64; 3] {
        let mut detuned = [0.0; 3];
        for (n, frequency) in detuned.iter_mut().enumerate() {
            *frequency = p.osc_freq[n] * cents_ratio(modulated(mods, p.osc_detune[n], DETUNE_SLOTS[n]));
        }
        let pitch_punch = clamp(modulated(mods, p.pitch_punch, Slot::PitchPunch), 0.0, 1.0);
        if pitch_punch > SILENT && t < PITCH_PUNCH_SECONDS {
            let punch = (((1.0 - t / PITCH_PUNCH_SECONDS) * pitch_punch * PITCH_PUNCH_CENTS) * LN2_OVER_1200).exp();
            detuned = detuned.map(|frequency| frequency * punch);
        }

        let fm_amount = clamp(modulated(mods, p.fm_amount, Slot::FmAmount), 0.0, 1.0);
        let algorithm = clamp(modulated(mods, p.fm_algorithm.round(), Slot::FmAlgorithm).round(), 0.0, 4.0);
        let carriers = (detuned[0], detuned[1]);
        let [mut f1, mut f2, mut f3] = detuned;
        if !p.bypass.fm && fm_amount > SILENT {
            (f1, f2) = fm_frequencies(fm_amount, algorithm, carriers, self.raw_oscillators(p, b, detuned));
        }
        if mod_level <= SILENT {
            return [f1, f2, f3];
        }
        // Only while the modulation envelope is up, and always the same arm.
        match b.mod_target {
            ModEnvTarget::Pitch => {
                let pitch = ((p.mod_env_depth * mod_level * 1200.0) * LN2_OVER_1200).exp();
                [f1, f2, f3] = [f1 * pitch, f2 * pitch, f3 * pitch];
            }
            ModEnvTarget::Fm if !p.bypass.fm => {
                let amount = fm_amount * p.mod_env_depth * mod_level;
                if amount > SILENT {
                    let (a, b) = fm_frequencies(amount, algorithm, carriers, self.raw_oscillators(p, b, detuned));
                    f1 += a - carriers.0;
                    f2 += b - carriers.1;
                }
            }
            ModEnvTarget::Shape => {
                *gains = gains.map(|gain| clamp(gain + p.mod_env_depth * mod_level * (1.0 - gain), 0.0, 1.0));
            }
            ModEnvTarget::Off | ModEnvTarget::Filter | ModEnvTarget::Fm => {}
        }
        [f1, f2, f3]
    }

    /// The three oscillators where they stand, at full gain: what FM
    /// modulates with.
    #[inline]
    fn raw_oscillators(&self, p: &SynthPatch, b: &Block, frequencies: [f64; 3]) -> [f64; 3] {
        let mut raw = [0.0; 3];
        for (n, value) in raw.iter_mut().enumerate() {
            let dt = (frequencies[n] / b.sample_rate).min(0.49);
            *value = oscillator(b.table, p.osc_wave[n], self.phase[n], dt, self.osc_cycle[n]);
        }
        raw
    }

    /// Move the oscillators on by a sample at `frequencies` and mix them,
    /// with the sub-oscillator, at `gains`.
    #[inline]
    fn oscillators(
        &mut self,
        p: &SynthPatch,
        b: &Block,
        mods: &[LfoMods; 2],
        frequencies: [f64; 3],
        gains: [f64; 3],
    ) -> f64 {
        let mut outputs = [0.0; 3];
        for (n, output) in outputs.iter_mut().enumerate() {
            // A negative frequency runs the phase backwards: through-zero FM
            // keeps the note's pitch, where stopping at zero would raise it.
            let dt = clamp(frequencies[n] / b.sample_rate, -0.49, 0.49);
            let advanced = self.phase[n] + dt;
            self.osc_cycle[n] += advanced.floor();
            self.phase[n] = advanced.rem_euclid(1.0);
            *output = oscillator(b.table, p.osc_wave[n], self.phase[n], dt.abs(), self.osc_cycle[n]) * gains[n];
        }
        let sub_gain = clamp(modulated(mods, p.sub_gain, Slot::SubGain), 0.0, 1.0);
        // One cycle over two of oscillator 1: an octave below it.
        let sub = if sub_gain > SILENT {
            sin_lookup(b.table, (self.osc_cycle[0].rem_euclid(2.0) + self.phase[0]) * 0.5) * sub_gain
        } else {
            0.0
        };
        outputs[0] + outputs[1] + outputs[2] + sub
    }

    /// The next sample of filtered noise, at its level in the mix.
    #[inline]
    fn noise(&mut self, p: &SynthPatch, b: &Block, mods: &[LfoMods; 2]) -> f64 {
        self.rng = xorshift32(self.rng);
        if p.bypass.noise {
            return 0.0;
        }
        let level = clamp(modulated(mods, p.noise_mix, Slot::NoiseMix), 0.0, 1.0);
        let raw = noise_sample(self.rng) * level;
        let freq = modulated(mods, p.noise_filter_freq, Slot::NoiseFreq);
        let q = clamp(modulated(mods, p.noise_filter_q, Slot::NoiseQ), 0.1, 20.0);
        let (g, k) = if freq != p.noise_filter_freq || q != p.noise_filter_q {
            (cutoff_coefficient(freq, b.sample_rate), 1.0 / q)
        } else {
            (b.noise_g, b.noise_k)
        };
        tap(self.noise_filter.step(raw, &coefficients(g, k)), b.noise_weights)
    }

    /// `x` through the main filter at `cutoff` Hz. The coefficients are the
    /// block's unless an LFO or an envelope moves them (`is_swept`) for this
    /// sample.
    #[inline]
    fn filter(&mut self, b: &Block, mods: &[LfoMods; 2], x: f64, cutoff: f64, is_swept: bool) -> f64 {
        let is_modulated = is_swept
            || mods[0][Slot::Filter] != 0.0
            || mods[1][Slot::Filter] != 0.0
            || mods[0][Slot::FilterQ] != 0.0
            || mods[1][Slot::FilterQ] != 0.0;
        let (g, k) = if is_modulated {
            let q = clamp(modulated(mods, b.q, Slot::FilterQ), 0.1, 20.0);
            (cutoff_coefficient(cutoff, b.sample_rate), 1.0 / q)
        } else {
            (b.g, b.k)
        };
        if !b.is_steep {
            return tap(self.filter.step(x, &coefficients(g, k)), b.filter_weights);
        }
        let (k1, k2, gain_2) = slope::stages(k, b.filter_mode);
        let first = tap(self.filter.step(x, &coefficients(g, k1)), b.filter_weights);
        tap(self.filter_2.step(first, &coefficients(g, k2)), b.filter_weights) * gain_2
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::patch::SynthBypass;
    use super::*;
    use crate::lfo::Wave;

    fn render(voice: &mut SynthVoice, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        voice.render(48_000.0, 0, &mut l, &mut r);
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
        let bypass = SynthBypass { filter: true, env: true, ..SynthBypass::default() };
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
    fn the_sub_sounds_an_octave_below_with_no_dc() {
        let out = held(SynthPatch { osc_gain: [0.0; 3], sub_gain: 1.0, ..SynthPatch::default() }, 48_000);
        let dc = out.iter().map(|&x| f64::from(x)).sum::<f64>() / out.len() as f64;
        assert!(dc.abs() < 1e-3, "{dc}");
        assert!(tone_level(&out, 50.0) > 100.0 * tone_level(&out, 100.0));
    }

    #[test]
    fn a_full_punch_starts_two_octaves_up_and_ends_on_the_note() {
        let crossings = |s: &[f32]| s.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        let patch =
            SynthPatch { osc_gain: [1.0, 0.0, 0.0], pitch_punch: 1.0, osc_freq: [1000.0; 3], ..SynthPatch::default() };
        let mut voice = SynthVoice::default();
        let bypass = SynthBypass { filter: true, env: true, ..SynthBypass::default() };
        voice.start(&SynthPatch { bypass, ..patch }, 0, 0.0, 1);
        let out = render(&mut voice, 7680);
        // 2 ms at nearly 4 kHz, then 20 ms at 1 kHz once the punch is over.
        assert!(crossings(&out[..96]) >= 6, "{}", crossings(&out[..96]));
        assert_eq!(crossings(&out[2880..3840]), 20);
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
}
