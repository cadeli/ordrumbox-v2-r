//! The cases the processors are rendered on (`processors.rs`): strip and
//! master bus settings over noise bursts, and synth patches. They are the
//! inputs the JS worklets were first checked on, which the snapshots of
//! `tests/fixtures/` matched when written (`README.md`, "Snapshots").

use ordrumbox_dsp::synth::{Adsr, SynthBypass, SynthLfo};
use ordrumbox_dsp::{MasterSettings, StripSettings, SynthPatch};

/// Settings changed from a quantum on: `(quantum, change)`.
pub type StripChange = (usize, fn(&mut StripSettings));

/// A strip case: its changes, over the bursts of `input` (`(left seed, right
/// seed, gain)`).
#[derive(Debug)]
pub struct StripCase {
    pub name: &'static str,
    pub sample_rate: f32,
    pub frames: usize,
    pub input: (u32, u32, f64),
    pub changes: &'static [StripChange],
}

/// A master bus case: settings set before the first quantum.
#[derive(Debug)]
pub struct MasterCase {
    pub name: &'static str,
    pub sample_rate: f32,
    pub frames: usize,
    pub input: (u32, u32, f64),
    pub settings: fn(&mut MasterSettings),
}

/// A synth case: one note of `patch`, started and released at these times
/// (seconds), rendered over `frames`.
#[derive(Debug)]
pub struct SynthCase {
    pub name: &'static str,
    pub sample_rate: f32,
    pub frames: usize,
    pub start_time: f64,
    pub release_time: f64,
    pub patch: SynthPatch,
}

pub const STRIP_CASES: &[StripCase] = &[
    StripCase { name: "dry", sample_rate: 48000.0, frames: 8192, input: (1, 2, 1.0), changes: &[(0, |_| {})] },
    StripCase {
        name: "lowpass",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.cutoff = 800.0;
            s.q = 2.0;
            s.filter_mode = 0.0;
        })],
    },
    StripCase {
        name: "highpass",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.cutoff = 2000.0;
            s.q = 0.707;
            s.filter_mode = 1.0;
        })],
    },
    StripCase {
        name: "bandpass",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.cutoff = 1000.0;
            s.q = 5.0;
            s.filter_mode = 2.0;
        })],
    },
    StripCase {
        name: "notch",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.cutoff = 1000.0;
            s.q = 1.0;
            s.filter_mode = 3.0;
        })],
    },
    StripCase {
        name: "sat_soft",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.saturation_type = 0.0;
            s.saturation_drive = 4.0;
            s.saturation_output = 0.925;
            s.saturation_mix = 1.0;
        })],
    },
    StripCase {
        name: "sat_hard",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.saturation_type = 1.0;
            s.saturation_drive = 7.0;
            s.saturation_output = 0.85;
            s.saturation_mix = 1.0;
        })],
    },
    StripCase {
        name: "sat_tape",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.saturation_type = 2.0;
            s.saturation_drive = 2.2;
            s.saturation_output = 0.97;
            s.saturation_mix = 1.0;
        })],
    },
    StripCase {
        name: "reverb_room",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.reverb_room = 0.5;
            s.reverb_damping = 0.5;
            s.reverb_width = 0.8;
            s.reverb_mix = 0.4;
        })],
    },
    StripCase {
        name: "reverb_hall",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.reverb_room = 0.85;
            s.reverb_damping = 0.3;
            s.reverb_width = 1.0;
            s.reverb_mix = 0.6;
        })],
    },
    StripCase {
        name: "delay_slap",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.delay_time_l = 0.03;
            s.delay_time_r = 0.03;
            s.delay_mode = 0.0;
            s.delay_mix = 0.5;
        })],
    },
    StripCase {
        name: "delay_pingpong",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.delay_time_l = 0.02001;
            s.delay_time_r = 0.03;
            s.delay_mode = 2.0;
            s.delay_mix = 0.7;
        })],
    },
    StripCase {
        name: "everything",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.cutoff = 3000.0;
            s.q = 4.0;
            s.filter_mode = 0.0;
            s.saturation_type = 0.0;
            s.saturation_drive = 2.5;
            s.saturation_output = 0.96;
            s.saturation_mix = 1.0;
            s.reverb_room = 0.7;
            s.reverb_damping = 0.4;
            s.reverb_width = 0.9;
            s.reverb_mix = 0.5;
            s.delay_time_l = 0.025;
            s.delay_time_r = 0.0375;
            s.delay_mode = 1.0;
            s.delay_mix = 0.4;
            s.volume = 1.5;
            s.pan = 0.5;
        })],
    },
    StripCase {
        name: "pan_left_quiet",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[(0, |s| {
            s.pan = -1.0;
            s.volume = 0.5;
        })],
    },
    StripCase {
        name: "changes",
        sample_rate: 48000.0,
        frames: 8192,
        input: (1, 2, 1.0),
        changes: &[
            (0, |s| {
                s.reverb_room = 0.5;
                s.reverb_mix = 0.3;
            }),
            (24, |s| {
                s.cutoff = 500.0;
                s.q = 8.0;
                s.reverb_mix = 0.0;
                s.delay_time_l = 0.01;
                s.delay_time_r = 0.01;
                s.delay_mix = 0.6;
            }),
            (40, |s| {
                s.filter_mode = 1.0;
                s.cutoff = 4000.0;
                s.pan = 0.3;
                s.delay_mix = 0.0;
            }),
        ],
    },
    StripCase {
        name: "rate_44100",
        sample_rate: 44100.0,
        frames: 8192,
        input: (3, 4, 1.0),
        changes: &[(0, |s| {
            s.reverb_room = 0.85;
            s.reverb_damping = 0.3;
            s.reverb_mix = 0.5;
            s.delay_time_l = 0.02;
            s.delay_time_r = 0.02;
            s.delay_mix = 0.5;
            s.cutoff = 15000.0;
        })],
    },
];

pub const MASTER_CASES: &[MasterCase] = &[
    MasterCase { name: "defaults", sample_rate: 48000.0, frames: 8192, input: (5, 6, 2.0), settings: |_| {} },
    MasterCase {
        name: "bypass",
        sample_rate: 48000.0,
        frames: 8192,
        input: (5, 6, 2.0),
        settings: |m| {
            m.bypass = 1.0;
            m.master = 0.7;
        },
    },
    MasterCase {
        name: "hard_knee_strong",
        sample_rate: 48000.0,
        frames: 8192,
        input: (5, 6, 2.0),
        settings: |m| {
            m.threshold = -30.0;
            m.ratio = 20.0;
            m.knee = 0.0;
            m.pre_gain = 6.0;
        },
    },
    MasterCase {
        name: "soft_knee_slow",
        sample_rate: 48000.0,
        frames: 8192,
        input: (5, 6, 2.0),
        settings: |m| {
            m.threshold = -12.0;
            m.ratio = 3.0;
            m.knee = 12.0;
            m.attack = 0.03;
            m.release = 0.4;
            m.makeup = 2.0;
        },
    },
    MasterCase {
        name: "band",
        sample_rate: 48000.0,
        frames: 8192,
        input: (5, 6, 2.0),
        settings: |m| {
            m.lowcut = 200.0;
            m.hicut = 3000.0;
            m.master = 0.5;
            m.pre_gain = -6.0;
        },
    },
];

/// The synth cases: every web preset, two note variants, busy patches
/// (every LFO target and wave, FM algorithm, modulation target) and bypasses.
#[rustfmt::skip]
pub fn synth_cases() -> Vec<SynthCase> {
    vec![
        SynthCase {
            name: "BASS0", sample_rate: 48000.0, frames: 9088, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 65.40639132514966, 32.70319566257483],
                osc_gain: [0.75, 0.35, 0.0], osc_detune: [0.0, 5.0, 0.0], osc_wave: [2.0, 0.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 480.0, filter_q: 2.5, filter_slope: 12.0, filter_env_amount: 0.35,
                drive: 0.15, pitch_punch: 0.15, sub_gain: 0.4, volume: 1.0, pan: 0.0,
                velocity: 0.7727272727272727,
                envelope: Adsr { attack: 0.005, decay: 0.28, sustain: 0.4, release: 0.12 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BASS1", sample_rate: 48000.0, frames: 8192, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 32.70319566257483, 32.70319566257483],
                osc_gain: [0.67, 0.63, 0.0], osc_detune: [0.0, 8.0, 0.0], osc_wave: [3.0, 2.0, 0.0],
                noise_mix: 0.02, noise_filter_type: 1.0, noise_filter_freq: 1500.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.0,
                velocity: 0.23846153846153847,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 15.0, wave: 0.0, freq: 0.5, depth: 0.1 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BASS2", sample_rate: 48000.0, frames: 10624, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 65.40639132514966, 32.70319566257483],
                osc_gain: [0.7, 0.4, 0.0], osc_detune: [0.0, -7.0, 0.0], osc_wave: [1.0, 2.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 520.0, filter_q: 3.0, filter_slope: 12.0, filter_env_amount: 0.35,
                drive: 0.15, pitch_punch: 0.1, sub_gain: 0.3, volume: 1.0, pan: 0.0,
                velocity: 0.7727272727272727,
                envelope: Adsr { attack: 0.005, decay: 0.32, sustain: 0.5, release: 0.15 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BASS3", sample_rate: 48000.0, frames: 7168, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 65.40639132514966, 32.70319566257483],
                osc_gain: [0.75, 0.4, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [3.0, 1.0, 0.0],
                noise_mix: 0.08, noise_filter_type: 1.0, noise_filter_freq: 2500.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 750.0, filter_q: 6.0, filter_slope: 12.0, filter_env_amount: 0.5,
                drive: 0.2, pitch_punch: 0.5, sub_gain: 0.35, volume: 1.0, pan: 0.0,
                velocity: 0.7391304347826088,
                envelope: Adsr { attack: 0.003, decay: 0.16, sustain: 0.2, release: 0.08 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BASS4", sample_rate: 48000.0, frames: 8192, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 130.8127826502993, 32.70319566257483],
                osc_gain: [0.95, 0.0, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [2.0, 3.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 340.0, filter_q: 14.0, filter_slope: 12.0, filter_env_amount: 0.65,
                drive: 0.35, pitch_punch: 0.2, sub_gain: 0.15, volume: 1.0, pan: 0.0,
                velocity: 0.894736842105263,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.0, release: 0.1 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BASS5", sample_rate: 48000.0, frames: 9088, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 130.8127826502993, 32.70319566257483],
                osc_gain: [0.85, 0.5, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [0.0, 2.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 600.0, filter_q: 6.5, filter_slope: 12.0, filter_env_amount: 0.4,
                drive: 0.3, pitch_punch: 0.2, sub_gain: 0.35, volume: 1.0, pan: 0.0,
                velocity: 0.6296296296296295,
                envelope: Adsr { attack: 0.004, decay: 0.25, sustain: 0.35, release: 0.12 },
                lfo1: SynthLfo { target: 24.0, wave: 2.0, freq: 2.0, depth: 0.3 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.45, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "SYNTH1", sample_rate: 48000.0, frames: 12928, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 130.8127826502993, 261.6255653005986],
                osc_gain: [0.6, 0.6, 0.0], osc_detune: [-6.0, 6.0, 0.0], osc_wave: [2.0, 3.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 2200.0, filter_q: 2.0, filter_slope: 12.0, filter_env_amount: 0.3,
                drive: 0.05, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 0.6666666666666667,
                envelope: Adsr { attack: 0.01, decay: 0.3, sustain: 0.6, release: 0.2 },
                lfo1: SynthLfo { target: 15.0, wave: 0.0, freq: 3.0, depth: 0.15 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "SYNTH2", sample_rate: 48000.0, frames: 10624, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 261.6255653005986, 130.8127826502993],
                osc_gain: [0.7, 0.5, 0.0], osc_detune: [0.0, 4.0, 0.0], osc_wave: [2.0, 2.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 4500.0, filter_q: 1.5, filter_slope: 12.0, filter_env_amount: 0.2,
                drive: 0.1, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 0.6666666666666667,
                envelope: Adsr { attack: 0.005, decay: 0.2, sustain: 0.8, release: 0.15 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "SYNTH3", sample_rate: 48000.0, frames: 7168, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 261.6255653005986, 130.8127826502993],
                osc_gain: [0.8, 0.4, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [3.0, 0.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 1800.0, filter_q: 4.0, filter_slope: 12.0, filter_env_amount: 0.6,
                drive: 0.1, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 0.7083333333333333,
                envelope: Adsr { attack: 0.003, decay: 0.18, sustain: 0.0, release: 0.08 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.1, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "PIANO", sample_rate: 48000.0, frames: 15360, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 261.6255653005986, 523.2511306011972],
                osc_gain: [0.7, 0.0, 0.0], osc_detune: [0.0, 0.0, -4.0], osc_wave: [0.0, 0.0, 0.0],
                noise_mix: 0.01, noise_filter_type: 1.0, noise_filter_freq: 2000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 2700.0, filter_q: 3.4000000000000004, filter_slope: 12.0, filter_env_amount: 0.1,
                drive: 0.0, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 1.2142857142857144,
                envelope: Adsr { attack: 0.003, decay: 0.66, sustain: 0.18, release: 0.338 },
                lfo1: SynthLfo { target: 0.0, wave: 3.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 3.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.45, sustain: 0.52, release: 0.248 },
                bypass: SynthBypass { noise: true, filter: true, lfo1: true, lfo2: true, fm: true, mod_env: true, filter_env: true, ..SynthBypass::default() },
            },
        },
        SynthCase {
            name: "SN", sample_rate: 48000.0, frames: 7168, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [130.8127826502993, 130.8127826502993, 130.8127826502993],
                osc_gain: [0.6, 0.0, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [1.0, 0.0, 0.0],
                noise_mix: 0.75, noise_filter_type: 1.0, noise_filter_freq: 1500.0, noise_filter_q: 1.0,
                filter_type: 1.0, filter_freq: 800.0, filter_q: 2.0, filter_slope: 12.0, filter_env_amount: 0.3,
                drive: 0.2, pitch_punch: 0.8, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 1.416666666666667,
                envelope: Adsr { attack: 0.003, decay: 0.18, sustain: 0.0, release: 0.08 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "TOM", sample_rate: 48000.0, frames: 10624, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 65.40639132514966, 130.8127826502993],
                osc_gain: [0.9, 0.2, 0.0], osc_detune: [0.0, 0.0, 0.0], osc_wave: [0.0, 1.0, 0.0],
                noise_mix: 0.05, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 1200.0, filter_q: 3.0, filter_slope: 12.0, filter_env_amount: 0.4,
                drive: 0.15, pitch_punch: 0.7, sub_gain: 0.2, volume: 1.0, pan: 0.0,
                velocity: 0.7727272727272727,
                envelope: Adsr { attack: 0.003, decay: 0.35, sustain: 0.0, release: 0.15 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "CHH_SYNTH", sample_rate: 48000.0, frames: 4352, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [523.2511306011972, 523.2511306011972, 130.8127826502993],
                osc_gain: [0.2, 0.2, 0.0], osc_detune: [0.0, 350.0, 0.0], osc_wave: [3.0, 3.0, 0.0],
                noise_mix: 0.8, noise_filter_type: 1.0, noise_filter_freq: 5000.0, noise_filter_q: 1.0,
                filter_type: 1.0, filter_freq: 6500.0, filter_q: 4.0, filter_slope: 12.0, filter_env_amount: 0.1,
                drive: 0.1, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 1.875,
                envelope: Adsr { attack: 0.003, decay: 0.04, sustain: 0.0, release: 0.02 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.2, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "OHH_SYNTH", sample_rate: 48000.0, frames: 10624, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [523.2511306011972, 523.2511306011972, 130.8127826502993],
                osc_gain: [0.2, 0.2, 0.0], osc_detune: [0.0, 350.0, 0.0], osc_wave: [3.0, 3.0, 0.0],
                noise_mix: 0.8, noise_filter_type: 1.0, noise_filter_freq: 4500.0, noise_filter_q: 1.0,
                filter_type: 1.0, filter_freq: 5500.0, filter_q: 3.0, filter_slope: 12.0, filter_env_amount: 0.1,
                drive: 0.1, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 1.875,
                envelope: Adsr { attack: 0.003, decay: 0.25, sustain: 0.1, release: 0.15 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.2, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BELL1", sample_rate: 48000.0, frames: 15360, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [261.6255653005986, 523.2511306011972, 130.8127826502993],
                osc_gain: [0.8, 0.5, 0.0], osc_detune: [0.0, 3.0, 0.0], osc_wave: [0.0, 0.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 4000.0, filter_q: 2.5, filter_slope: 12.0, filter_env_amount: 0.3,
                drive: 0.0, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 0.6153846153846154,
                envelope: Adsr { attack: 0.003, decay: 0.7, sustain: 0.0, release: 0.4 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.6, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "BELL2", sample_rate: 48000.0, frames: 15360, start_time: 0.00625, release_time: 0.06875,
            patch: SynthPatch {
                osc_freq: [261.6255653005986, 523.2511306011972, 1046.5022612023945],
                osc_gain: [0.7, 0.5, 0.2], osc_detune: [0.0, 7.0, -3.0], osc_wave: [0.0, 0.0, 0.0],
                noise_mix: 0.0, noise_filter_type: 1.0, noise_filter_freq: 1000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 5000.0, filter_q: 3.0, filter_slope: 12.0, filter_env_amount: 0.35,
                drive: 0.0, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.0,
                velocity: 0.5714285714285715,
                envelope: Adsr { attack: 0.003, decay: 0.9, sustain: 0.05, release: 0.5 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.4, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "PIANO_variant", sample_rate: 48000.0, frames: 15360, start_time: 0.00625, release_time: 0.11736111111111111,
            patch: SynthPatch {
                osc_freq: [196.21917397544897, 392.43834795089793, 784.8766959017959],
                osc_gain: [0.7, 0.0, 0.0], osc_detune: [0.0, 0.0, -4.0], osc_wave: [0.0, 0.0, 0.0],
                noise_mix: 0.01, noise_filter_type: 1.0, noise_filter_freq: 2000.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 2700.0, filter_q: 3.4000000000000004, filter_slope: 12.0, filter_env_amount: 0.1,
                drive: 0.0, pitch_punch: 0.0, sub_gain: 0.0, volume: 1.0, pan: 0.5,
                velocity: 0.6071428571428572,
                envelope: Adsr { attack: 0.003, decay: 0.66, sustain: 0.18, release: 0.338 },
                lfo1: SynthLfo { target: 0.0, wave: 3.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 3.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.45, sustain: 0.52, release: 0.248 },
                bypass: SynthBypass { noise: true, filter: true, lfo1: true, lfo2: true, fm: true, mod_env: true, filter_env: true, ..SynthBypass::default() },
            },
        },
        SynthCase {
            name: "BASS1_low", sample_rate: 48000.0, frames: 7552, start_time: 0.00625, release_time: 0.05625,
            patch: SynthPatch {
                osc_freq: [65.40639132514966, 16.351597831287414, 16.351597831287414],
                osc_gain: [0.67, 0.63, 0.0], osc_detune: [0.0, 8.0, 0.0], osc_wave: [3.0, 2.0, 0.0],
                noise_mix: 0.02, noise_filter_type: 1.0, noise_filter_freq: 1500.0, noise_filter_q: 1.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: -0.7,
                velocity: 0.2980769230769231,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 15.0, wave: 0.0, freq: 0.5, depth: 0.1 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.005, decay: 0.12, sustain: 0.0, release: 0.1 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_0", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 1.0, wave: 0.0, freq: 3.0, depth: 0.3 },
                lfo2: SynthLfo { target: 8.0, wave: 2.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_1", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 2.0, wave: 1.0, freq: 4.0, depth: 0.3 },
                lfo2: SynthLfo { target: 9.0, wave: 3.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_2", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 3.0, wave: 2.0, freq: 5.0, depth: 0.3 },
                lfo2: SynthLfo { target: 10.0, wave: 4.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_3", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 4.0, wave: 3.0, freq: 6.0, depth: 0.3 },
                lfo2: SynthLfo { target: 11.0, wave: 0.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_4", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 5.0, wave: 4.0, freq: 7.0, depth: 0.3 },
                lfo2: SynthLfo { target: 12.0, wave: 1.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_5", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 6.0, wave: 0.0, freq: 8.0, depth: 0.3 },
                lfo2: SynthLfo { target: 13.0, wave: 2.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_6", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 7.0, wave: 1.0, freq: 9.0, depth: 0.3 },
                lfo2: SynthLfo { target: 14.0, wave: 3.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_7", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 8.0, wave: 2.0, freq: 10.0, depth: 0.3 },
                lfo2: SynthLfo { target: 15.0, wave: 4.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_8", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 9.0, wave: 3.0, freq: 11.0, depth: 0.3 },
                lfo2: SynthLfo { target: 16.0, wave: 0.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_9", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 10.0, wave: 4.0, freq: 12.0, depth: 0.3 },
                lfo2: SynthLfo { target: 17.0, wave: 1.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_10", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 11.0, wave: 0.0, freq: 13.0, depth: 0.3 },
                lfo2: SynthLfo { target: 18.0, wave: 2.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_11", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 12.0, wave: 1.0, freq: 14.0, depth: 0.3 },
                lfo2: SynthLfo { target: 19.0, wave: 3.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_12", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 13.0, wave: 2.0, freq: 15.0, depth: 0.3 },
                lfo2: SynthLfo { target: 20.0, wave: 4.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_13", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 14.0, wave: 3.0, freq: 16.0, depth: 0.3 },
                lfo2: SynthLfo { target: 21.0, wave: 0.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_14", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 15.0, wave: 4.0, freq: 17.0, depth: 0.3 },
                lfo2: SynthLfo { target: 22.0, wave: 1.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_15", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 16.0, wave: 0.0, freq: 18.0, depth: 0.3 },
                lfo2: SynthLfo { target: 23.0, wave: 2.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_16", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 17.0, wave: 1.0, freq: 19.0, depth: 0.3 },
                lfo2: SynthLfo { target: 24.0, wave: 3.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_17", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 18.0, wave: 2.0, freq: 20.0, depth: 0.3 },
                lfo2: SynthLfo { target: 25.0, wave: 4.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_18", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 19.0, wave: 3.0, freq: 21.0, depth: 0.3 },
                lfo2: SynthLfo { target: 26.0, wave: 0.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_19", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 20.0, wave: 4.0, freq: 22.0, depth: 0.3 },
                lfo2: SynthLfo { target: 27.0, wave: 1.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_20", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 21.0, wave: 0.0, freq: 23.0, depth: 0.3 },
                lfo2: SynthLfo { target: 28.0, wave: 2.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_21", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 22.0, wave: 1.0, freq: 24.0, depth: 0.3 },
                lfo2: SynthLfo { target: 29.0, wave: 3.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_22", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 23.0, wave: 2.0, freq: 25.0, depth: 0.3 },
                lfo2: SynthLfo { target: 30.0, wave: 4.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_23", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 24.0, wave: 3.0, freq: 26.0, depth: 0.3 },
                lfo2: SynthLfo { target: 31.0, wave: 0.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_24", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 25.0, wave: 4.0, freq: 27.0, depth: 0.3 },
                lfo2: SynthLfo { target: 32.0, wave: 1.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_25", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 26.0, wave: 0.0, freq: 28.0, depth: 0.3 },
                lfo2: SynthLfo { target: 33.0, wave: 2.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_26", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 27.0, wave: 1.0, freq: 29.0, depth: 0.3 },
                lfo2: SynthLfo { target: 1.0, wave: 3.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_27", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 28.0, wave: 2.0, freq: 30.0, depth: 0.3 },
                lfo2: SynthLfo { target: 2.0, wave: 4.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_28", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 29.0, wave: 3.0, freq: 31.0, depth: 0.3 },
                lfo2: SynthLfo { target: 3.0, wave: 0.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 3.0, mod_env_target: 3.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_29", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 30.0, wave: 4.0, freq: 32.0, depth: 0.3 },
                lfo2: SynthLfo { target: 4.0, wave: 1.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.1, fm_algorithm: 4.0, mod_env_target: 4.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_30", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 31.0, wave: 0.0, freq: 33.0, depth: 0.3 },
                lfo2: SynthLfo { target: 5.0, wave: 2.0, freq: 13.333333333333334, depth: 0.5 },
                fm_amount: 0.2, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_31", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 32.0, wave: 1.0, freq: 34.0, depth: 0.3 },
                lfo2: SynthLfo { target: 6.0, wave: 3.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.30000000000000004, fm_algorithm: 1.0, mod_env_target: 1.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "busy_32", sample_rate: 48000.0, frames: 3072, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 33.0, wave: 2.0, freq: 35.0, depth: 0.3 },
                lfo2: SynthLfo { target: 7.0, wave: 4.0, freq: 0.0, depth: 0.5 },
                fm_amount: 0.0, fm_algorithm: 2.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass::default(),
            },
        },
        SynthCase {
            name: "bypassed", sample_rate: 48000.0, frames: 8704, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 0.0, filter_freq: 380.0, filter_q: 5.0, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.25, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 1.0, wave: 0.0, freq: 2.0, depth: 0.5 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.3, fm_algorithm: 1.0, mod_env_target: 2.0, mod_env_depth: 1.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass { noise: true, filter: true, lfo1: true, fm: true, mod_env: true, filter_env: true, ..SynthBypass::default() },
            },
        },
        SynthCase {
            name: "no_env", sample_rate: 48000.0, frames: 8704, start_time: 0.00625, release_time: 0.08125,
            patch: SynthPatch {
                osc_freq: [163.51597831287415, 40.87899457821854, 327.0319566257483],
                osc_gain: [0.67, 0.63, 0.3], osc_detune: [0.0, 8.0, -7.0], osc_wave: [3.0, 2.0, 1.0],
                noise_mix: 0.2, noise_filter_type: 2.0, noise_filter_freq: 3000.0, noise_filter_q: 2.0,
                filter_type: 3.0, filter_freq: 9000.0, filter_q: 0.5, filter_slope: 12.0, filter_env_amount: 0.17,
                drive: 0.0, pitch_punch: 0.3, sub_gain: 0.35, volume: 1.0, pan: 0.2,
                velocity: 0.21796875,
                envelope: Adsr { attack: 0.003, decay: 0.22, sustain: 0.35, release: 0.1 },
                lfo1: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                lfo2: SynthLfo { target: 0.0, wave: 0.0, freq: 0.0, depth: 0.0 },
                fm_amount: 0.0, fm_algorithm: 0.0, mod_env_target: 0.0, mod_env_depth: 0.0,
                mod_envelope: Adsr { attack: 0.01, decay: 0.05, sustain: 0.3, release: 0.05 },
                bypass: SynthBypass { env: true, ..SynthBypass::default() },
            },
        },
    ]
}
