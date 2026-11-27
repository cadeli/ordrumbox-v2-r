//! One synth note: three oscillators with FM and unison, a sub-oscillator, filtered
//! noise, drive, a TPT state-variable filter with its envelope (an ADSR of
//! its own, which may close it instead of opening it), a second filter, an
//! ADSR, a modulation envelope, a pitch envelope, ring modulation and hard
//! sync of oscillator 2 by oscillator 1, and two LFOs (each with a fade-in
//! and, if asked, restarted by each note) routed to any of their targets,
//! then equal-power pan.
//!
//! [`SynthVoice::start`] starts a note of a [`SynthPatch`],
//! [`SynthVoice::retrigger`] plays a note over one still sounding (mono and
//! legato: it may glide from its pitch) and [`SynthVoice::release_at`]
//! releases it. Starting, releasing and rendering never allocate.
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
mod filter2;
mod modulation;
mod oscillator;
pub mod patch;
mod pitch_env;
mod retrigger;
mod sources;
mod unison;

pub use cross_mod::ring;
use cross_mod::{Moved, cycle_starts, restart};
use envelope::{Envelope, FilterEnvelope, Segment};
use filter::{coefficients, cutoff_coefficient};
use filter2::Filter2Stage;
pub use filter2::balance_gains;
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
use patch::{Adsr, Filter2Routing, LfoTarget, MatrixSource, NoiseMode, SynthLfo, SynthPatch};
pub use pitch_env::MAX_PITCH_ENV_SEMITONES;
use pitch_env::{PitchFall, fall_per_sample};
pub use retrigger::Retrigger;
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
    /// Seconds each LFO takes to fade in from the note's start (0: none), and whether one does.
    lfo_delays: [f64; 2],
    has_lfo_delay: bool,
    /// The share of a glide's offset a sample keeps.
    glide_fall: f64,
    filter2: Option<Filter2Stage>,
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
        let lfo_delays = [p.lfo1.delay_seconds.max(0.0), p.lfo2.delay_seconds.max(0.0)];
        Self {
            tables: WaveTables { sine: sine_table(), source },
            sample_rate,
            lfo_steps: [p.lfo1.freq / sample_rate, p.lfo2.freq / sample_rate],
            lfo_delays,
            has_lfo_delay: lfo_delays.iter().any(|delay| *delay > 0.0),
            glide_fall: fall_per_sample(p.glide_seconds, sample_rate),
            filter2: p.filter2.map(|filter| Filter2Stage::new(&filter, sample_rate)),
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
    /// When the note playing started, seconds on the engine clock: the
    /// voice's start, or a later note played over it (mono).
    note_start: f64,
    /// The glide from the pitch of the note before: what is left of it, and its offset at the start, semitones.
    glide: PitchFall,
    glide_semitones: f64,
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
    /// The filter envelope when it has an ADSR of its own.
    filter_adsr: Envelope,
    /// The second filter, left and right.
    filter2: Svf,
    filter2_right: Svf,
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
            note_start: -1.0,
            glide: PitchFall::START,
            glide_semitones: 0.0,
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
            filter_adsr: Envelope::new(Segment::Idle),
            filter2: Svf::default(),
            filter2_right: Svf::default(),
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
            note_start: start_time,
            rng: noise_seed(serial),
            sources: SourceValues {
                velocity: clamp(patch.note_velocity, 0.0, 1.0),
                key: clamp(patch.note_key, -1.0, 1.0),
                random: noise_sample(xorshift32(noise_seed(serial))),
                ..SourceValues::default()
            },
            env: Envelope::new(Segment::Attack),
            filter_env: FilterEnvelope::new(Segment::Attack),
            filter_adsr: Envelope::new(Segment::Attack),
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

    /// Move the LFOs on by a sample and set the values the matrix reads of
    /// them, faded in over their delay.
    #[inline]
    fn step_lfos(&mut self, p: &SynthPatch, b: &Block, time: f64) {
        if self.lfo_seeded {
            self.lfo_phase[0] += b.lfo_steps[0];
            self.lfo_phase[1] += b.lfo_steps[1];
        } else {
            // On the engine clock, unless the LFO starts with each note.
            let since_note = time - self.note_start;
            let seed = |lfo: &SynthLfo| if lfo.is_retriggered { since_note * lfo.freq } else { time * lfo.freq };
            self.lfo_phase = [seed(&p.lfo1), seed(&p.lfo2)];
            self.lfo_seeded = true;
        }
        for ((wave, phase), value) in b.matrix_lfo_waves.iter().zip(self.lfo_phase).zip(&mut self.sources.lfo) {
            *value = wave.map_or(0.0, |wave| lfo::wave(phase, wave));
        }
        if b.has_lfo_delay {
            let since_note = time - self.note_start;
            for (value, delay) in self.sources.lfo.iter_mut().zip(b.lfo_delays) {
                if delay > 0.0 {
                    *value *= clamp(since_note / delay, 0.0, 1.0);
                }
            }
        }
    }

    /// Whether the voice plays a note not yet released at `time` (seconds on the engine clock).
    #[must_use]
    pub fn is_held(&self, time: f64) -> bool {
        self.active && !self.is_released(time)
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
        let cutoff = self.cutoff(p, mods, &env, (filter_env_amount, filter_open), (time, t));

        let mut gains = [0.0; 3];
        for ((gain, base), slot) in gains.iter_mut().zip(p.osc_gain).zip(GAIN_SLOTS) {
            *gain = clamp(modulated(mods, base, slot), 0.0, 1.0);
        }
        self.width = pulse_widths(b, mods, p.osc_width);
        let mut bend = if b.has_pitch_env { self.pitch_env_ratio(p, b, mods) } else { 1.0 };
        if self.glide_semitones != 0.0 {
            bend *= self.glide.step(self.glide_semitones, b.glide_fall);
            if self.glide.left() == 0.0 {
                self.glide_semitones = 0.0;
            }
        }
        let frequencies = self.frequencies(p, b, mods, &mut gains, (&late, bend));
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

        let is_swept = (filter_env_amount > SILENT && self.is_filter_env_running(p)) || filter_open.abs() > SILENT;
        let filtered = if p.bypass.filter { dry } else { self.filter(b, mods, dry, cutoff, is_swept) };
        let filtered = match &b.filter2 {
            Some(stage) => self.filter_2nd(b, mods, stage, filtered, dry),
            None => filtered,
        };

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
    /// opened (or closed, inverted) by the filter envelope at `filter_env_amount`,
    /// and opened by `filter_open` (a share of the way to the top); `time` on
    /// the engine clock, `t` from the note's start.
    #[inline]
    fn cutoff(
        &mut self,
        p: &SynthPatch,
        mods: &Mods,
        env: &Adsr,
        (filter_env_amount, filter_open): (f64, f64),
        (time, t): (f64, f64),
    ) -> f64 {
        let mut cutoff = modulated(mods, p.filter_freq, Slot::Filter);
        if !p.bypass.filter_env
            && filter_env_amount > SILENT
            && let Some(level) = self.filter_env_level(p, env, time, t)
        {
            let depth = filter_env_amount * level;
            if p.filter_env.is_inverted {
                cutoff -= (cutoff - crate::MIN_FILTER_FREQ).max(0.0) * depth;
            } else {
                cutoff += (crate::MAX_FILTER_FREQ - cutoff) * depth;
            }
        }
        if filter_open.abs() > SILENT {
            cutoff += (crate::MAX_FILTER_FREQ - cutoff) * filter_open;
        }
        cutoff
    }

    /// The filter envelope's level (0–1) at `t`, moved on by a sample; `None` once it is over.
    #[inline]
    fn filter_env_level(&mut self, p: &SynthPatch, env: &Adsr, time: f64, t: f64) -> Option<f64> {
        match p.filter_env.adsr {
            None => (!self.filter_env.is_idle()).then(|| self.filter_env.step(t, env.attack, env.decay)),
            Some(adsr) => {
                if self.is_released(time) {
                    self.filter_adsr.release(t);
                }
                (!self.filter_adsr.is_idle()).then(|| self.filter_adsr.step(t, adsr, 1.0, |norm| norm))
            }
        }
    }

    /// Whether the filter envelope still moves the cutoff.
    #[inline]
    fn is_filter_env_running(&self, p: &SynthPatch) -> bool {
        if p.filter_env.adsr.is_some() { !self.filter_adsr.is_idle() } else { !self.filter_env.is_idle() }
    }

    /// The second filter's output (left, right): in series, `first` (the
    /// first filter's output) through it; in parallel, `dry` (the first
    /// filter's input) through it, mixed with `first` at the balance. At the
    /// block's coefficient and gains unless the matrix moves them.
    #[inline]
    fn filter_2nd(&mut self, b: &Block, mods: &Mods, stage: &Filter2Stage, first: [f64; 2], dry: [f64; 2]) -> [f64; 2] {
        let g = if is_moved(mods, Slot::Filter2) {
            cutoff_coefficient(modulated(mods, stage.freq, Slot::Filter2), b.sample_rate)
        } else {
            stage.g
        };
        let coefficients = coefficients(g, stage.k);
        let x = match stage.routing {
            Filter2Routing::Serial => first,
            Filter2Routing::Parallel => dry,
        };
        let left = tap(self.filter2.step(x[0], &coefficients), stage.weights);
        let right = if b.is_stereo { tap(self.filter2_right.step(x[1], &coefficients), stage.weights) } else { left };
        if stage.routing == Filter2Routing::Serial {
            return [left, right];
        }
        let [first_gain, second_gain] = if is_moved(mods, Slot::Filter2Balance) {
            balance_gains(modulated(mods, stage.balance, Slot::Filter2Balance))
        } else {
            stage.gains
        };
        [first[0] * first_gain + left * second_gain, first[1] * first_gain + right * second_gain]
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
        let lfo = SynthLfo::free(Wave::Square, 2.0);
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

    /// A sawtooth at 100 Hz through a lowpass at 200 Hz, opened all the way
    /// by its envelope at `filter_env`, held for 0.5 s then released: the
    /// level of its 2 kHz partial every 50 ms.
    fn swept(filter_env: super::patch::FilterEnv, filter_freq: f64) -> Vec<f64> {
        let mut voice = SynthVoice::default();
        let patch = SynthPatch {
            osc_freq: [100.0; 3],
            osc_gain: [1.0, 0.0, 0.0],
            osc_wave: [Wave::Sawtooth; 3],
            filter_freq,
            filter_env_amount: 1.0,
            filter_env,
            volume: 1.0,
            bypass: SynthBypass { env: true, ..SynthBypass::default() },
            ..SynthPatch::default()
        };
        voice.start(&patch, 0, 0.0, 1);
        voice.release_at(0.5);
        let out = render(&mut voice, 48_000);
        out.as_chunks::<2_400>().0.iter().map(|chunk| tone_level(chunk, 2_000.0)).collect()
    }

    #[test]
    fn a_filter_adsr_holds_its_sustain_until_the_release() {
        use super::patch::FilterEnv;
        let adsr = Adsr { attack: 0.001, decay: 0.1, sustain: 0.5, release: 0.1 };
        let held = swept(FilterEnv { adsr: Some(adsr), is_inverted: false }, 200.0);
        let legacy = swept(FilterEnv { adsr: None, is_inverted: false }, 200.0);
        // Half open from 0.1 s to the release at 0.5 s; the older one is shut by then.
        assert!(held[5] > 10.0 * legacy[5], "{held:?} {legacy:?}");
        assert!((held[4] - held[8]).abs() < 0.1 * held[4], "{held:?}");
        assert!(held[14] < 0.1 * held[8], "closed after its release: {held:?}");
    }

    #[test]
    fn an_inverted_filter_envelope_closes_the_filter() {
        use super::patch::FilterEnv;
        let adsr = Adsr { attack: 0.001, decay: 0.1, sustain: 1.0, release: 0.1 };
        let closed = swept(FilterEnv { adsr: Some(adsr), is_inverted: true }, 8_000.0);
        // Shut while held, open again once released.
        assert!(closed[5] < 0.05 * closed[14], "{closed:?}");
    }

    #[test]
    fn the_second_filter_follows_the_first() {
        let saw = |filter2| {
            let patch = SynthPatch {
                osc_gain: [1.0, 0.0, 0.0],
                osc_wave: [Wave::Sawtooth; 3],
                filter2,
                ..SynthPatch::default()
            };
            let out = held(patch, 24_000);
            [200.0, 3_000.0].map(|f| tone_level(&out[12_000..], f))
        };
        let open = saw(None);
        let cut = saw(Some(lowpass_500(Filter2Routing::Serial, 0.5)));
        assert!((cut[0] - open[0]).abs() < 0.2 * open[0], "{open:?} {cut:?}");
        assert!(cut[1] < 0.05 * open[1], "{open:?} {cut:?}");
    }

    /// A second filter: a lowpass at 500 Hz, `routing` with the first at `balance`.
    fn lowpass_500(routing: Filter2Routing, balance: f64) -> patch::Filter2 {
        patch::Filter2 { mode: FilterMode::Lowpass, freq: 500.0, q: 0.7, routing, balance }
    }

    #[test]
    fn the_second_filter_in_parallel_is_mixed_with_the_first_at_the_balance() {
        // The first filter is bypassed: in parallel, its side is the dry sound.
        let saw = |balance| {
            let patch = SynthPatch {
                osc_gain: [1.0, 0.0, 0.0],
                osc_wave: [Wave::Sawtooth; 3],
                filter2: Some(lowpass_500(Filter2Routing::Parallel, balance)),
                ..SynthPatch::default()
            };
            let out = held(patch, 24_000);
            [200.0, 3_000.0].map(|f| tone_level(&out[12_000..], f))
        };
        let [first, both, second] = [saw(0.0), saw(0.5), saw(1.0)];
        // The first alone: the dry sound; the second alone: the lowpass.
        assert!(second[1] < 0.05 * first[1], "{first:?} {second:?}");
        // Both at full level: the lows of the two add up, the highs of the first stay.
        assert!((both[0] - 2.0 * first[0]).abs() < 0.3 * first[0], "{first:?} {both:?}");
        assert!((both[1] - first[1]).abs() < 0.1 * first[1], "{first:?} {both:?}");
    }

    #[test]
    fn a_delayed_lfo_fades_in_and_a_retriggered_one_starts_with_the_note() {
        let route = MatrixRoute { source: MatrixSource::Lfo1, target: LfoTarget::Volume, amount: -1.0 };
        let base = routed(route);
        let delayed = SynthLfo { delay_seconds: 0.5, ..SynthLfo::free(Wave::Square, 4.0) };
        let out = held(SynthPatch { lfo1: delayed, ..base }, 48_000);
        // How far the level of each 10 ms moves: barely over the first tenth
        // of a second, all the way over the last half.
        let swing = |out: &[f32]| {
            let levels = out.as_chunks::<480>().0.iter().map(|chunk| peak(chunk));
            levels.clone().fold(0.0, f32::max) - levels.fold(f32::MAX, f32::min)
        };
        let (first, last) = (swing(&out[..4_800]), swing(&out[24_000..]));
        assert!(first < 0.25 * last, "{first} {last}");
        // A note starting later hears the same start of the cycle when it restarts.
        let note = |is_retriggered, start: f64| {
            let lfo1 = SynthLfo { is_retriggered, ..SynthLfo::free(Wave::Sine, 3.0) };
            let mut voice = SynthVoice::default();
            let bypass = SynthBypass { filter: true, env: true, ..SynthBypass::default() };
            voice.start(&SynthPatch { osc_freq: [100.0; 3], volume: 1.0, bypass, lfo1, ..base }, 0, start, 1);
            let start_frame = (start * 48_000.0) as usize;
            let out = render(&mut voice, start_frame + 4_800);
            out[start_frame..].to_vec()
        };
        assert_eq!(note(true, 0.0), note(true, 0.25));
        assert_ne!(note(false, 0.0), note(false, 0.25));
    }

    #[test]
    fn a_note_played_over_glides_from_the_pitch_before() {
        let play = |how: Retrigger, glide_seconds: f64| {
            let mut voice = SynthVoice::default();
            let bypass = SynthBypass { filter: true, env: true, ..SynthBypass::default() };
            let low = SynthPatch {
                osc_freq: [100.0; 3],
                osc_gain: [1.0, 0.0, 0.0],
                volume: 1.0,
                bypass,
                glide_seconds,
                ..SynthPatch::default()
            };
            voice.start(&low, 0, 0.0, 1);
            let (mut l, mut r) = (vec![0.0; 4_800], vec![0.0; 4_800]);
            voice.render(48_000.0, 0, &[], &mut l, &mut r);
            voice.retrigger(&SynthPatch { osc_freq: [400.0; 3], ..low }, 0.1, 2, how);
            let (mut l, mut r) = (vec![0.0; 9_600], vec![0.0; 9_600]);
            voice.render(48_000.0, 4_800, &[], &mut l, &mut r);
            // Cycles in the first and the last 20 ms after the new note.
            (cycles(&l[..960]), cycles(&l[8_640..]))
        };
        let (start, end) = play(Retrigger::Glide, 0.1);
        assert!(start < 5 && end == 8, "glides up to 400 Hz: {start} {end}");
        assert_eq!(play(Retrigger::Legato, 0.1).1, 8);
        let (start, _) = play(Retrigger::Restart, 0.1);
        assert!(start >= 7, "no glide: {start}");
        let (start, _) = play(Retrigger::Glide, 0.0);
        assert!(start >= 7, "no glide time, no glide: {start}");
    }

    #[test]
    fn the_noise_register_holds_its_value_between_two_steps() {
        // A clock at a sixth of the sample rate: each value lasts six samples.
        let slow = console_noise(NoiseMode::Long, 8_000.0, 600);
        assert!(slow[6..].chunks(6).all(|run| run.iter().all(|x| *x == run[0])));
    }
}
