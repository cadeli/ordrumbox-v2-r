//! One synth note (`worklets/processors/synth_voice_source.js`): three
//! oscillators with FM, a sub-oscillator, filtered noise, drive, a TPT
//! state-variable filter with its envelope, an ADSR, a modulation envelope and
//! two LFOs routed to any of 33 targets, then equal-power pan.
//!
//! The JS processor is driven by messages (`trigger`, `release`, `update`);
//! here [`SynthVoice::start`] is the trigger with the update's values (a
//! [`SynthPatch`]) and [`SynthVoice::release_at`] the release. Starting,
//! releasing and rendering never allocate.
//!
//! It departs from the JS processor on purpose where that one did not do what
//! its controls say: the sub-oscillator sounds an octave below oscillator 1,
//! the pitch punch reaches two octaves, FM follows the note and goes through
//! zero, and each note has its own noise.

use std::f64::consts::{PI, TAU};
use std::sync::OnceLock;

use crate::{js_round, lfo};

/// `Math.LN2 / 1200`: cents to a natural-log ratio.
const LN2_OVER_1200: f64 = 0.000_577_622_650_466_621_1;
const SINE_TABLE_SIZE: usize = 4096;
/// Initial state of the noise generator.
const NOISE_SEED: i32 = 54_321;
const FLUSH: f64 = 1e-15;
/// Pitch rise of a full pitch punch at the start of a note, cents.
const PITCH_PUNCH_CENTS: f64 = 2400.0;
/// Frequency deviation of a full FM amount, as a multiple of the carrier's
/// frequency: the timbre stays the same over the keyboard.
pub const FM_DEVIATION: f64 = 2.0;

/// Initial noise state of the note numbered `serial`: every note has its own,
/// the same from one render to the next. A xorshift never leaves a state of 0.
fn noise_seed(serial: u64) -> i32 {
    let seed = NOISE_SEED ^ serial.wrapping_sub(1).wrapping_mul(0x9E37_79B1) as i32;
    if seed == 0 { NOISE_SEED } else { seed }
}

/// Frequencies of oscillators 1 and 2 under FM of `amount`, routed by
/// `algorithm` from the raw `o1`, `o2` and `o3`.
#[inline]
fn fm_frequencies(amount: f64, algorithm: i64, (f1, f2): (f64, f64), [o1, o2, o3]: [f64; 3]) -> (f64, f64) {
    let (d1, d2) = (f1 * amount * FM_DEVIATION, f2 * amount * FM_DEVIATION);
    match algorithm {
        0 => (f1 + o2 * d1, f2),
        1 => (f1 + o3 * d1, f2),
        2 => (f1 + o2 * d1, f2 + o3 * d2),
        3 => (f1 + (o2 + o3) * d1, f2),
        4 => (f1 + o2 * d1, f2 + o1 * d2),
        _ => (f1, f2),
    }
}

fn sine_table() -> &'static [f32; SINE_TABLE_SIZE] {
    static TABLE: OnceLock<[f32; SINE_TABLE_SIZE]> = OnceLock::new();
    TABLE.get_or_init(|| std::array::from_fn(|i| (TAU * i as f64 / SINE_TABLE_SIZE as f64).sin() as f32))
}

/// Sine of `phase` cycles from the 4096-entry table, linearly interpolated.
#[inline]
fn sin_lookup(table: &[f32; SINE_TABLE_SIZE], phase: f64) -> f64 {
    let p = ((phase % 1.0) + 1.0) % 1.0;
    let index = p * SINE_TABLE_SIZE as f64;
    let i = index as usize;
    let fraction = index - i as f64;
    f64::from(table[i & (SINE_TABLE_SIZE - 1)]) * (1.0 - fraction)
        + f64::from(table[(i + 1) & (SINE_TABLE_SIZE - 1)]) * fraction
}

/// PolyBLEP correction at phase `t` for a phase increment `dt`.
#[inline]
fn poly_blep(t: f64, dt: f64) -> f64 {
    if dt <= 0.00001 || dt >= 0.5 {
        return 0.0;
    }
    if t < dt {
        let n = t / dt;
        return n + n - n * n - 1.0;
    }
    if t > 1.0 - dt {
        let n = (t - 1.0) / dt;
        return n * n + n + n + 1.0;
    }
    0.0
}

#[inline]
fn xorshift32(mut state: i32) -> i32 {
    state ^= state << 13;
    state ^= state >> 17;
    state ^= state << 5;
    state
}

/// Oscillator `shape` (0 sine, 1 triangle, 2 saw, 3 square, 4 random per cycle).
#[inline]
fn oscillator(table: &[f32; SINE_TABLE_SIZE], shape: f64, phase: f64, dt: f64, cycle: f64) -> f64 {
    if shape < 0.5 {
        sin_lookup(table, phase)
    } else if shape < 1.5 {
        if phase < 0.25 {
            phase * 4.0
        } else if phase < 0.75 {
            2.0 - phase * 4.0
        } else {
            phase * 4.0 - 4.0
        }
    } else if shape < 2.5 {
        phase * 2.0 - 1.0 - poly_blep(phase, dt)
    } else if shape < 3.5 {
        let square = if phase < 0.5 { 1.0 } else { -1.0 };
        square + poly_blep(phase, dt) - poly_blep((phase + 0.5) % 1.0, dt)
    } else {
        lfo::sample_and_hold(cycle)
    }
}

/// One LFO of a patch.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SynthLfo {
    /// Routing code, 0 (off) to 33 (see `LFO_TARGET_TO_INT` in worklet_synth_voice.js).
    pub target: f64,
    /// Waveform code (see [`lfo::wave`]).
    pub wave: f64,
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

/// Everything one note plays with: the processor's `update` message.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthPatch {
    /// Hz, before detune.
    pub osc_freq: [f64; 3],
    pub osc_gain: [f64; 3],
    /// Cents.
    pub osc_detune: [f64; 3],
    pub osc_wave: [f64; 3],
    pub noise_mix: f64,
    /// 0 lowpass, 1 highpass, 2 bandpass, 3 notch.
    pub noise_filter_type: f64,
    pub noise_filter_freq: f64,
    pub noise_filter_q: f64,
    /// 0 lowpass, 1 highpass, 2 bandpass, 3 notch.
    pub filter_type: f64,
    pub filter_freq: f64,
    pub filter_q: f64,
    pub drive: f64,
    pub pitch_punch: f64,
    pub sub_gain: f64,
    pub envelope: Adsr,
    pub volume: f64,
    pub pan: f64,
    /// Peak level of the note (the `velocity` parameter).
    pub velocity: f64,
    pub lfo1: SynthLfo,
    pub lfo2: SynthLfo,
    pub filter_env_amount: f64,
    pub fm_amount: f64,
    /// 0–4.
    pub fm_algorithm: f64,
    pub mod_envelope: Adsr,
    /// 0 off, 1 filter, 2 pitch, 3 FM, 4 shape.
    pub mod_env_target: f64,
    pub mod_env_depth: f64,
    pub bypass: SynthBypass,
}

impl Default for SynthPatch {
    /// The processor's parameter defaults.
    fn default() -> Self {
        let lfo = SynthLfo { target: 0.0, wave: 0.0, freq: 1.0, depth: 0.0 };
        Self {
            osc_freq: [440.0; 3],
            osc_gain: [0.5; 3],
            osc_detune: [0.0; 3],
            osc_wave: [0.0; 3],
            noise_mix: 0.0,
            noise_filter_type: 0.0,
            noise_filter_freq: 1000.0,
            noise_filter_q: 0.7,
            filter_type: 0.0,
            filter_freq: 1000.0,
            filter_q: 0.7,
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
            mod_env_target: 0.0,
            mod_env_depth: 0.0,
            bypass: SynthBypass::default(),
        }
    }
}

/// What an LFO adds this sample: detune per oscillator,
/// gain per oscillator, and the other targets.
#[derive(Debug, Clone, Copy, Default)]
struct LfoMods {
    detune: [f64; 3],
    gain: [f64; 3],
    filter: f64,
    master: f64,
    q: f64,
    noise: f64,
    filter_env_amount: f64,
    sub_gain: f64,
    pitch_punch: f64,
    drive: f64,
    noise_freq: f64,
    noise_q: f64,
    fm_amount: f64,
    fm_algorithm: f64,
    env: [f64; 4],
    mod_env: [f64; 4],
}

impl LfoMods {
    fn new(target: i64, depth: f64, phase: f64, wave: f64) -> Self {
        let mut m = Self::default();
        if target == 0 {
            return m;
        }
        let raw = lfo::wave(phase, wave) * depth;
        match target {
            1 | 15 => m.filter = raw * 1000.0,
            2..=4 => m.detune[(target - 2) as usize] = raw * 1000.0,
            5 => m.master = raw,
            6 | 9 | 12 => m.gain[((target - 6) / 3) as usize] = raw,
            7 | 10 | 13 => m.detune[((target - 7) / 3) as usize] = raw * 100.0,
            8 | 11 | 14 => m.detune[((target - 8) / 3) as usize] = raw * 1200.0,
            16 => m.filter_env_amount = raw,
            17 => m.q = raw * 24.0,
            18 => m.noise = raw,
            19 => m.sub_gain = raw,
            20 => m.pitch_punch = raw,
            21 => m.drive = raw,
            22 => m.noise_freq = raw * 10_000.0,
            23 => m.noise_q = raw * 24.0,
            24 => m.fm_amount = raw,
            25 => m.fm_algorithm = raw,
            26 | 29 => m.env[(target - 26) as usize] = raw * 0.25,
            27 | 28 => m.env[(target - 26) as usize] = raw * 0.5,
            30 | 33 => m.mod_env[(target - 30) as usize] = raw * 0.25,
            31 | 32 => m.mod_env[(target - 30) as usize] = raw * 0.5,
            _ => {}
        }
        m
    }
}

/// TPT state-variable filter state.
#[derive(Debug, Clone, Copy, Default)]
struct Tpt {
    z1: f64,
    z2: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Segment {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
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
    release_start_level: f64,
    phase: [f64; 3],
    osc_cycle: [f64; 3],
    rng: i32,
    lfo_phase: [f64; 2],
    lfo_seeded: bool,
    filter: Tpt,
    noise_filter: Tpt,
    env_level: f64,
    env_segment: Segment,
    env_segment_start: f64,
    filter_env_level: f64,
    /// 0 off, 1 attack, 2 decay.
    filter_env_segment: u8,
    mod_env_level: f64,
    mod_env_segment: Segment,
    mod_env_segment_start: f64,
    mod_env_release_start_level: f64,
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
            release_start_level: 0.0,
            phase: [0.0; 3],
            osc_cycle: [0.0; 3],
            rng: NOISE_SEED,
            lfo_phase: [0.0; 2],
            lfo_seeded: false,
            filter: Tpt::default(),
            noise_filter: Tpt::default(),
            env_level: 0.0,
            env_segment: Segment::Idle,
            env_segment_start: 0.0,
            filter_env_level: 0.0,
            filter_env_segment: 0,
            mod_env_level: 0.0,
            mod_env_segment: Segment::Idle,
            mod_env_segment_start: 0.0,
            mod_env_release_start_level: 0.0,
        }
    }
}

/// `Math.max(lo, Math.min(hi, x))`.
#[inline]
fn clamp(x: f64, lo: f64, hi: f64) -> f64 {
    x.min(hi).max(lo)
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
    /// is given), on a fresh voice (a pooled JS node is reset before reuse).
    pub fn start(&mut self, patch: &SynthPatch, track: usize, start_time: f64, serial: u64) {
        *self =
            Self { active: true, track, serial, patch: *patch, start_time, rng: noise_seed(serial), ..Self::default() };
        self.env_segment = Segment::Attack;
        self.filter_env_segment = 1;
        self.mod_env_segment = Segment::Attack;
    }

    /// Release the note at `time` (seconds, same clock), replacing any release
    /// already set.
    pub fn release_at(&mut self, time: f64) {
        if self.active {
            self.release_time = time;
        }
    }

    /// ADSR step, `peak` the note's peak level.
    fn envelope_step(&mut self, t: f64, env: Adsr, peak: f64) -> f64 {
        if t < 0.0 || self.env_segment == Segment::Idle {
            return 0.0;
        }
        let Adsr { attack, decay, sustain, release } = env;
        if self.env_segment == Segment::Attack {
            let dt = t - self.env_segment_start;
            if attack <= 0.0001 || dt >= attack {
                self.env_level = peak;
                self.env_segment = Segment::Decay;
                self.env_segment_start = t;
            } else {
                let norm = (dt / attack).max(0.0);
                self.env_level = peak * norm * norm;
            }
        }
        if self.env_segment == Segment::Decay {
            let dt = t - self.env_segment_start;
            if decay <= 0.0001 || dt >= decay {
                self.env_level = peak * sustain;
                self.env_segment = Segment::Sustain;
            } else {
                let norm = clamp(1.0 - dt / decay, 0.0, 1.0);
                let curve = norm * norm * norm;
                self.env_level = peak * (sustain + (1.0 - sustain) * curve);
            }
        }
        if self.env_segment == Segment::Sustain {
            self.env_level = peak * sustain;
        }
        if self.env_segment == Segment::Release {
            let rt = t - self.env_segment_start;
            if release <= 0.0001 || rt >= release {
                self.env_level = 0.0;
                self.env_segment = Segment::Idle;
            } else {
                let norm = clamp(1.0 - rt / release, 0.0, 1.0);
                self.env_level = self.release_start_level * (norm * norm * norm);
            }
        }
        if self.env_level.is_finite() && self.env_level > 0.0 { self.env_level } else { 0.0 }
    }

    /// Modulation envelope step: linear attack, levels 0–1.
    fn mod_env_step(&mut self, t: f64, env: Adsr) -> f64 {
        if t < 0.0 || self.mod_env_segment == Segment::Idle {
            return 0.0;
        }
        let Adsr { attack, decay, sustain, release } = env;
        if self.mod_env_segment == Segment::Attack {
            let dt = t - self.mod_env_segment_start;
            if attack <= 0.0001 || dt >= attack {
                self.mod_env_level = 1.0;
                self.mod_env_segment = Segment::Decay;
                self.mod_env_segment_start = t;
            } else {
                self.mod_env_level = (dt / attack).max(0.0);
            }
        }
        if self.mod_env_segment == Segment::Decay {
            let dt = t - self.mod_env_segment_start;
            if decay <= 0.0001 || dt >= decay {
                self.mod_env_level = sustain;
                self.mod_env_segment = Segment::Sustain;
            } else {
                let norm = clamp(1.0 - dt / decay, 0.0, 1.0);
                self.mod_env_level = sustain + (1.0 - sustain) * (norm * norm * norm);
            }
        }
        if self.mod_env_segment == Segment::Sustain {
            self.mod_env_level = sustain;
        }
        if self.mod_env_segment == Segment::Release {
            let rt = t - self.mod_env_segment_start;
            if release <= 0.0001 || rt >= release {
                self.mod_env_level = 0.0;
                self.mod_env_segment = Segment::Idle;
            } else {
                let norm = clamp(1.0 - rt / release, 0.0, 1.0);
                self.mod_env_level = self.mod_env_release_start_level * (norm * norm * norm);
            }
        }
        if self.mod_env_level.is_finite() && self.mod_env_level > 0.0 { self.mod_env_level } else { 0.0 }
    }

    /// Main filter step: (lowpass, highpass, bandpass).
    #[inline]
    fn filter_step(&mut self, x: f64, g: f64, k: f64) -> (f64, f64, f64) {
        let f = &mut self.filter;
        if !f.z1.is_finite() {
            f.z1 = 0.0;
        }
        if !f.z2.is_finite() {
            f.z2 = 0.0;
        }
        let g = clamp(g, 0.0001, 10.0);
        let k = clamp(k, 0.05, 10.0);
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let v3 = x - f.z2;
        let v1 = a1 * f.z1 + a2 * v3;
        let v2 = f.z2 + a2 * f.z1 + a3 * v3;
        f.z1 = 2.0 * v1 - f.z1;
        f.z2 = 2.0 * v2 - f.z2;
        if f.z1.abs() < FLUSH || !f.z1.is_finite() {
            f.z1 = 0.0;
        }
        if f.z2.abs() < FLUSH || !f.z2.is_finite() {
            f.z2 = 0.0;
        }
        (v2, v3 - v1 * k, v1)
    }

    /// ADD the next `left.len()` frames to `left`/`right`; `first_frame` is
    /// the clock position of the first one (in frames at `sample_rate`).
    pub fn render(&mut self, sample_rate: f32, first_frame: u64, left: &mut [f32], right: &mut [f32]) {
        if !self.active {
            return;
        }
        let table = sine_table();
        let sr = f64::from(sample_rate);
        let p = self.patch;
        let [f1, f2, f3] = p.osc_freq;
        let [g1, g2, g3] = p.osc_gain;
        let [d1, d2, d3] = p.osc_detune;
        let [w1, w2, w3] = p.osc_wave;
        let noise_mix = p.noise_mix;
        let nf_type = p.noise_filter_type;
        let nf_freq = p.noise_filter_freq;
        let nf_q = p.noise_filter_q;
        let f_type = p.filter_type;
        let f_freq = p.filter_freq;
        let f_q = p.filter_q;
        let lfo1_target = js_round(p.lfo1.target) as i64;
        let lfo1_wave = js_round(p.lfo1.wave);
        let lfo1_freq = p.lfo1.freq;
        let lfo1_depth = p.lfo1.depth;
        let lfo2_target = js_round(p.lfo2.target) as i64;
        let lfo2_wave = js_round(p.lfo2.wave);
        let lfo2_freq = p.lfo2.freq;
        let lfo2_depth = p.lfo2.depth;
        let fm_algorithm = js_round(p.fm_algorithm);
        let m_target = js_round(p.mod_env_target) as i64;
        let m_depth = p.mod_env_depth;
        let bypass = p.bypass;
        let osc_mix = 1.0 - noise_mix;

        let pan = clamp(p.pan, -1.0, 1.0);
        let pan_l = ((pan + 1.0) * PI / 4.0).cos();
        let pan_r = ((pan + 1.0) * PI / 4.0).sin();
        let mode = |t: f64| {
            if (0.5..1.5).contains(&t) {
                1
            } else if (1.5..2.5).contains(&t) {
                2
            } else if t >= 2.5 {
                3
            } else {
                0
            }
        };
        let filter_mode = mode(f_type);
        let Adsr { attack: a, decay: d, sustain: s, release: r } = p.envelope;
        let v = p.velocity;

        let f_freq_mod = clamp(f_freq, 20.0, 20_000.0);
        let f_q_val = clamp(f_q, 0.1, 20.0);
        let g_coeff = (PI * f_freq_mod.min(sr * 0.25) / sr).tan();
        let k_coeff = 1.0 / f_q_val;

        let lfo1_inc = lfo1_freq / sr;
        let lfo2_inc = lfo2_freq / sr;
        let lfo1_on = !bypass.lfo1 && lfo1_freq > 0.0;
        let lfo2_on = !bypass.lfo2 && lfo2_freq > 0.0;

        let nf_mode = mode(nf_type);
        let nf_g = (PI * clamp(nf_freq, 20.0, 20_000.0).min(sr * 0.25) / sr).tan();
        let nf_k = 1.0 / clamp(nf_q, 0.1, 20.0);

        let frames = left.len().min(right.len());
        for i in 0..frames {
            let current_time = (first_frame + i as u64) as f64 / sr;
            let t = current_time - self.start_time;
            if t < 0.0 {
                continue;
            }

            if self.lfo_seeded {
                self.lfo_phase[0] += lfo1_inc;
                self.lfo_phase[1] += lfo2_inc;
            } else {
                self.lfo_phase = [current_time * lfo1_freq, current_time * lfo2_freq];
                self.lfo_seeded = true;
            }
            let l1 = LfoMods::new(if lfo1_on { lfo1_target } else { 0 }, lfo1_depth, self.lfo_phase[0], lfo1_wave);
            let l2 = LfoMods::new(if lfo2_on { lfo2_target } else { 0 }, lfo2_depth, self.lfo_phase[1], lfo2_wave);

            let mut f_freq_sample = f_freq + l1.filter + l2.filter;
            let filter_env_amt = (p.filter_env_amount + l1.filter_env_amount + l2.filter_env_amount).max(0.0);
            let sub_gain = clamp(p.sub_gain + l1.sub_gain + l2.sub_gain, 0.0, 1.0);
            let pitch_punch = clamp(p.pitch_punch + l1.pitch_punch + l2.pitch_punch, 0.0, 1.0);
            let drive = clamp(p.drive + l1.drive + l2.drive, 0.0, 1.0);
            let fm_amount = clamp(p.fm_amount + l1.fm_amount + l2.fm_amount, 0.0, 1.0);
            let fm_algorithm = clamp(js_round(fm_algorithm + l1.fm_algorithm + l2.fm_algorithm), 0.0, 4.0) as i64;
            let env = Adsr {
                attack: (a + l1.env[0] + l2.env[0]).max(0.0),
                decay: (d + l1.env[1] + l2.env[1]).max(0.0),
                sustain: clamp(s + l1.env[2] + l2.env[2], 0.0, 1.0),
                release: (r + l1.env[3] + l2.env[3]).max(0.0),
            };
            let me = p.mod_envelope;
            let mod_env = Adsr {
                attack: (me.attack + l1.mod_env[0] + l2.mod_env[0]).max(0.0),
                decay: (me.decay + l1.mod_env[1] + l2.mod_env[1]).max(0.0),
                sustain: clamp(me.sustain + l1.mod_env[2] + l2.mod_env[2], 0.0, 1.0),
                release: (me.release + l1.mod_env[3] + l2.mod_env[3]).max(0.0),
            };

            // Modulation envelope.
            let mut m_env = 0.0;
            if !bypass.mod_env && m_target > 0 && m_depth > 0.001 {
                if matches!(self.mod_env_segment, Segment::Attack | Segment::Decay | Segment::Sustain)
                    && self.release_time > 0.0
                    && current_time >= self.release_time
                {
                    self.mod_env_release_start_level = self.mod_env_level;
                    self.mod_env_segment = Segment::Release;
                    self.mod_env_segment_start = t;
                }
                m_env = self.mod_env_step(t, mod_env);
            }

            // Filter envelope.
            if !bypass.filter_env && filter_env_amt > 0.001 && self.filter_env_segment > 0 {
                if self.filter_env_segment == 1 {
                    if env.attack > 0.0001 && t < env.attack {
                        self.filter_env_level = t / env.attack;
                    } else {
                        self.filter_env_level = 1.0;
                        self.filter_env_segment = 2;
                    }
                } else {
                    let dt = t - env.attack;
                    if env.decay > 0.0001 && dt < env.decay {
                        self.filter_env_level = 1.0 - dt / env.decay;
                    } else {
                        self.filter_env_level = 0.0;
                        self.filter_env_segment = 0;
                    }
                }
                f_freq_sample += (20_000.0 - f_freq_sample) * filter_env_amt * self.filter_env_level;
            }
            if m_target == 1 && m_env > 0.001 {
                f_freq_sample += (20_000.0 - f_freq_sample) * m_depth * m_env;
            }

            let detune = |base: f64, i: usize| base + l1.detune[i] + l2.detune[i];
            let (d1m, d2m, d3m) = (detune(d1, 0), detune(d2, 1), detune(d3, 2));
            let gain = |base: f64, i: usize| clamp(base + l1.gain[i] + l2.gain[i], 0.0, 1.0);
            let (mut g1c, mut g2c, mut g3c) = (gain(g1, 0), gain(g2, 1), gain(g3, 2));
            let master = (p.volume + l1.master + l2.master).max(0.0);

            let ratio = |cents: f64| if cents == 0.0 { 1.0 } else { (cents * LN2_OVER_1200).exp() };
            let mut f1d = f1 * ratio(d1m);
            let mut f2d = f2 * ratio(d2m);
            let mut f3d = f3 * ratio(d3m);

            if pitch_punch > 0.001 && t < 0.06 {
                let punch = (((1.0 - t / 0.06) * pitch_punch * PITCH_PUNCH_CENTS) * LN2_OVER_1200).exp();
                f1d *= punch;
                f2d *= punch;
                f3d *= punch;
            }

            // FM.
            let (phase, cycle) = (self.phase, self.osc_cycle);
            let raw =
                |shape: f64, n: usize, freq: f64| oscillator(table, shape, phase[n], (freq / sr).min(0.49), cycle[n]);
            let raw_oscillators = || [raw(w1, 0, f1d), raw(w2, 1, f2d), raw(w3, 2, f3d)];
            let (mut f1fm, mut f2fm) = (f1d, f2d);
            if !bypass.fm && fm_amount > 0.001 {
                (f1fm, f2fm) = fm_frequencies(fm_amount, fm_algorithm, (f1d, f2d), raw_oscillators());
            }

            if m_target > 0 && m_env > 0.001 {
                match m_target {
                    2 => {
                        let pitch = ((m_depth * m_env * 1200.0) * LN2_OVER_1200).exp();
                        f1fm *= pitch;
                        f2fm *= pitch;
                        f3d *= pitch;
                    }
                    3 if !bypass.fm => {
                        let amount = fm_amount * m_depth * m_env;
                        if amount > 0.001 {
                            let (a, b) = fm_frequencies(amount, fm_algorithm, (f1d, f2d), raw_oscillators());
                            f1fm += a - f1d;
                            f2fm += b - f2d;
                        }
                    }
                    4 => {
                        let shape = |g: f64| clamp(g + m_depth * m_env * (1.0 - g), 0.0, 1.0);
                        g1c = shape(g1c);
                        g2c = shape(g2c);
                        g3c = shape(g3c);
                    }
                    _ => {}
                }
            }

            // Oscillators.
            // A negative frequency runs the phase backwards: through-zero FM
            // keeps the note's pitch, where stopping at zero would raise it.
            let dts = [clamp(f1fm / sr, -0.49, 0.49), clamp(f2fm / sr, -0.49, 0.49), clamp(f3d / sr, -0.49, 0.49)];
            for (n, dt) in dts.iter().enumerate() {
                let advanced = self.phase[n] + dt;
                self.osc_cycle[n] += advanced.floor();
                self.phase[n] = advanced.rem_euclid(1.0);
            }
            let o1 = oscillator(table, w1, self.phase[0], dts[0].abs(), self.osc_cycle[0]) * g1c;
            let o2 = oscillator(table, w2, self.phase[1], dts[1].abs(), self.osc_cycle[1]) * g2c;
            let o3 = oscillator(table, w3, self.phase[2], dts[2].abs(), self.osc_cycle[2]) * g3c;
            // One cycle over two of oscillator 1: an octave below it.
            let sub = if sub_gain > 0.001 {
                sin_lookup(table, (self.osc_cycle[0].rem_euclid(2.0) + self.phase[0]) * 0.5) * sub_gain
            } else {
                0.0
            };
            let osc_sum = (o1 + o2 + o3 + sub) * osc_mix;

            // Noise.
            self.rng = xorshift32(self.rng);
            let mut noise = 0.0;
            if !bypass.noise {
                let level = clamp(noise_mix + l1.noise + l2.noise, 0.0, 1.0);
                let noise_raw = (f64::from(self.rng) / 2_147_483_648.0) * level;
                let (mut g, mut k) = (nf_g, nf_k);
                let freq = nf_freq + l1.noise_freq + l2.noise_freq;
                let q = clamp(nf_q + l1.noise_q + l2.noise_q, 0.1, 20.0);
                if freq != nf_freq || q != nf_q {
                    g = (PI * clamp(freq, 20.0, 20_000.0).min(sr * 0.25) / sr).tan();
                    k = 1.0 / q;
                }
                let a1 = 1.0 / (1.0 + g * (g + k));
                let a2 = g * a1;
                let a3 = g * a2;
                let nf = &mut self.noise_filter;
                let v3 = noise_raw - nf.z2;
                let v1 = a1 * nf.z1 + a2 * v3;
                let v2 = nf.z2 + a2 * nf.z1 + a3 * v3;
                nf.z1 = 2.0 * v1 - nf.z1;
                nf.z2 = 2.0 * v2 - nf.z2;
                if nf.z1.abs() < FLUSH || !nf.z1.is_finite() {
                    nf.z1 = 0.0;
                }
                if nf.z2.abs() < FLUSH || !nf.z2.is_finite() {
                    nf.z2 = 0.0;
                }
                noise = match nf_mode {
                    0 => v2,
                    1 => noise_raw - k * v1 - v2,
                    2 => v1,
                    _ => (noise_raw - k * v1 - v2) + v2,
                };
            }

            let mut dry = osc_sum + noise;
            if drive > 0.001 {
                let driven = dry * (1.0 + drive * 3.5);
                dry = driven / (1.0 + driven.abs() * 0.6);
            }

            // Filter.
            let q_mod = f_q_val + l1.q + l2.q;
            let k_mod = 1.0 / clamp(q_mod, 0.1, 20.0);
            let recompute = l1.filter != 0.0
                || l2.filter != 0.0
                || (filter_env_amt > 0.001 && self.filter_env_segment > 0)
                || l1.q != 0.0
                || l2.q != 0.0
                || (m_target == 1 && m_env > 0.001);
            let mut y = if bypass.filter {
                dry
            } else {
                let (lp, hp, bp) = if recompute {
                    let f = if f_freq_sample < 20.0 {
                        20.0
                    } else if f_freq_sample > sr * 0.25 {
                        sr * 0.25
                    } else {
                        f_freq_sample
                    };
                    self.filter_step(dry, (PI * f / sr).tan(), k_mod)
                } else {
                    self.filter_step(dry, g_coeff, k_coeff)
                };
                match filter_mode {
                    0 => lp,
                    1 => hp,
                    2 => bp,
                    _ => lp + hp,
                }
            };

            // Amplitude envelope.
            if matches!(self.env_segment, Segment::Attack | Segment::Decay | Segment::Sustain)
                && self.release_time > 0.0
                && current_time >= self.release_time
            {
                self.release_start_level = self.env_level;
                self.env_segment = Segment::Release;
                self.env_segment_start = t;
            }
            let level = self.envelope_step(t, env, v);
            y *= (if bypass.env { 1.0 } else { level }) * master;

            left[i] += (y * pan_l) as f32;
            right[i] += (y * pan_r) as f32;
        }
        if self.env_segment == Segment::Idle {
            self.active = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(voice: &mut SynthVoice, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        voice.render(48_000.0, 0, &mut l, &mut r);
        l
    }

    #[test]
    fn sine_table_lookup() {
        let table = sine_table();
        assert!((sin_lookup(table, 0.25) - 1.0).abs() < 1e-6);
        assert!((sin_lookup(table, -0.25) + 1.0).abs() < 1e-6);
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
