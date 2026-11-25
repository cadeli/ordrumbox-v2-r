//! What a strip works out once per block from its settings (coefficients,
//! which stages are on), so that the sample loop only reads it. The
//! compressor and the ducker carry their own step for one frame.

use std::f64::consts::PI;

use super::curves::Saturation;
use super::{
    CLIP_CEILING, CLIP_DRIVE_DB, CRUSH_BITS_MAX, CRUSH_BITS_MIN, CRUSH_OCTAVES, CRUSH_RATE_HZ, GATE_CLOSE_SECONDS,
    GATE_OPEN_SECONDS, StripSettings,
};
use crate::denormal::flush;
use crate::slope::{self, FilterMode};
use crate::svf::SvfCoeffs;

/// The filter's stages: one, or two in series for 24 dB/oct.
#[derive(Debug, Clone, Copy)]
pub(super) struct FilterBlock {
    pub on: bool,
    pub mode: FilterMode,
    pub stage_1: SvfCoeffs,
    pub stage_2: Option<SvfCoeffs>,
    pub gain_2: f64,
}

impl FilterBlock {
    pub fn new(s: &StripSettings, sample_rate: f64) -> Self {
        let (cutoff, q) = (f64::from(s.cutoff), f64::from(s.q));
        let mode = FilterMode::from_setting(f64::from(s.filter_mode));
        let g = (PI * cutoff / sample_rate).tan();
        let (stage_1, stage_2, gain_2) = if slope::is_steep(f64::from(s.filter_slope)) {
            let (k1, k2, gain_2) = slope::stages(1.0 / q, mode);
            (SvfCoeffs::new(g, k1), Some(SvfCoeffs::new(g, k2)), gain_2)
        } else {
            (SvfCoeffs::new(g, 1.0 / q), None, 1.0)
        };
        Self { on: s.filter_on > 0.5, mode, stage_1, stage_2, gain_2 }
    }
}

/// The saturation, when it is mixed in.
#[derive(Debug, Clone, Copy)]
pub(super) struct SaturationBlock {
    pub curve: Saturation,
    pub drive: f64,
    pub mix: f64,
    pub output: f64,
}

impl SaturationBlock {
    pub fn new(s: &StripSettings) -> Option<Self> {
        let mix = f64::from(s.saturation_mix);
        (mix > 0.0).then(|| Self {
            curve: Saturation::from_setting(f64::from(s.saturation_type)),
            drive: f64::from(s.saturation_drive),
            mix,
            output: f64::from(s.saturation_output),
        })
    }
}

/// The bitcrusher, when it is on.
#[derive(Debug, Clone, Copy)]
pub(super) struct CrushBlock {
    /// Quantization levels on each side of 0.
    pub levels: f64,
    /// The hold's phase step.
    pub step: f64,
}

impl CrushBlock {
    pub fn new(s: &StripSettings, sample_rate: f64) -> Option<Self> {
        let crush = f64::from(s.crush);
        (crush > 0.0).then(|| Self {
            levels: (CRUSH_BITS_MAX - crush * (CRUSH_BITS_MAX - CRUSH_BITS_MIN) - 1.0).exp2(),
            step: (CRUSH_RATE_HZ * (-CRUSH_OCTAVES * crush).exp2() / sample_rate).min(1.0),
        })
    }
}

/// The hard clipper, when it is on.
#[derive(Debug, Clone, Copy)]
pub(super) struct ClipBlock {
    /// Drive into the ceiling, as a gain over the ceiling (the clip is at ±1).
    pub gain: f64,
}

impl ClipBlock {
    pub fn new(s: &StripSettings) -> Option<Self> {
        (s.clip > 0.0).then(|| Self { gain: 10_f64.powf(f64::from(s.clip) * CLIP_DRIVE_DB / 20.0) / CLIP_CEILING })
    }
}

/// The reverb, when it is on and mixed in.
#[derive(Debug, Clone, Copy)]
pub(super) struct ReverbBlock {
    pub predelay_frames: f64,
    pub dispersion: f64,
    pub room: f64,
    pub damp1: f64,
    pub damp2: f64,
    pub width1: f64,
    pub width2: f64,
    pub mix: f64,
    pub gate_frames: f64,
    pub gate_open: f64,
    pub gate_close: f64,
}

impl ReverbBlock {
    pub fn new(s: &StripSettings, sample_rate: f64) -> Option<Self> {
        let mix = f64::from(s.reverb_mix);
        if s.reverb_on <= 0.5 || mix <= 0.001 {
            return None;
        }
        let damp1 = f64::from(s.reverb_damping) * 0.4;
        let width = f64::from(s.reverb_width);
        Some(Self {
            predelay_frames: f64::from(s.reverb_predelay) * sample_rate,
            dispersion: f64::from(s.reverb_dispersion),
            room: f64::from(s.reverb_room) * 0.28 + 0.7,
            damp1,
            damp2: 1.0 - damp1,
            width1: width * 0.5 + 0.5,
            width2: (1.0 - width) * 0.5,
            mix,
            gate_frames: f64::from(s.reverb_gate) * sample_rate,
            gate_open: 1.0 - (-1.0 / (GATE_OPEN_SECONDS * sample_rate)).exp(),
            gate_close: 1.0 - (-1.0 / (GATE_CLOSE_SECONDS * sample_rate)).exp(),
        })
    }
}

/// The delay. Off, it still takes the input in, so that it starts from what
/// was played when it is turned on.
#[derive(Debug, Clone, Copy)]
pub(super) struct DelayBlock {
    pub on: bool,
    pub mono_in: bool,
    pub ping_pong: bool,
    pub sample_rate: f64,
    pub mix: f64,
    pub feedback: f64,
    pub tone: f64,
    pub time_l_frames: f64,
    pub time_r_frames: f64,
    /// Depth of the tape wow, frames.
    pub wow_frames: f64,
}

impl DelayBlock {
    pub fn new(s: &StripSettings, sample_rate: f64) -> Self {
        let mix = f64::from(s.delay_mix);
        Self {
            on: s.delay_on > 0.5 && mix > 0.001,
            mono_in: s.delay_mono_in > 0.5,
            ping_pong: f64::from(s.delay_mode) > 1.5,
            sample_rate,
            mix,
            feedback: f64::from(s.delay_feedback),
            tone: f64::from(s.delay_tone),
            time_l_frames: f64::from(s.delay_time_l) * sample_rate,
            time_r_frames: f64::from(s.delay_time_r) * sample_rate,
            wow_frames: f64::from(s.delay_wow) * sample_rate,
        }
    }
}

/// Compensated equal-power pan.
#[derive(Debug, Clone, Copy)]
pub(super) struct PanBlock {
    pub left: f64,
    pub right: f64,
    pub compensation: f64,
}

impl PanBlock {
    pub fn new(pan: f64) -> Self {
        let left = ((pan + 1.0) * PI / 4.0).cos();
        let right = ((pan + 1.0) * PI / 4.0).sin();
        let max = if left > right { left } else { right };
        Self { left, right, compensation: if max > 0.001 { 1.0 / max } else { 1.0 } }
    }
}

/// Width of the compressor's soft knee, dB around the threshold.
const COMP_KNEE_DB: f64 = 6.0;
/// Share of the gain reduction at 0 dBFS the compressor makes up.
pub(super) const COMP_MAKEUP_SHARE: f64 = 0.5;
/// Level the compressor's detector reads as silence, dB.
const COMP_FLOOR_DB: f64 = -120.0;
/// Seconds the sidechain's ducking takes to close on a key.
const DUCK_ATTACK_SECONDS: f64 = 0.002;
/// Level of the key, linear, at which the ducking is at its full amount.
const DUCK_FULL_LEVEL: f64 = 0.5;
/// dB per neper: `20 / ln 10`.
const DB_PER_NEPER: f64 = 8.685_889_638_065_037;

/// The compressor's settings for one block: a feed-forward, stereo-linked
/// peak compressor with a soft knee, an automatic make-up gain and a dry/wet
/// mix.
#[derive(Debug, Clone, Copy)]
pub(super) struct Compressor {
    threshold_db: f64,
    /// `1 - 1 / ratio`: the share of the excess over the threshold taken off.
    slope: f64,
    attack: f64,
    release: f64,
    makeup: f64,
    mix: f64,
}

impl Compressor {
    /// The compressor of `s` at `sr`, `None` when it is off.
    pub(super) fn new(s: &StripSettings, sr: f64) -> Option<Self> {
        let mix = f64::from(s.comp_mix);
        if mix <= 0.0 {
            return None;
        }
        let threshold_db = f64::from(s.comp_threshold_db);
        let slope = 1.0 - 1.0 / f64::from(s.comp_ratio);
        // One-pole coefficients: the share of the way to the target per sample.
        let coefficient = |seconds: f32| 1.0 - (-1.0 / (f64::from(seconds) * sr)).exp();
        let makeup_db = -threshold_db * slope * COMP_MAKEUP_SHARE;
        Some(Self {
            threshold_db,
            slope,
            attack: coefficient(s.comp_attack_seconds),
            release: coefficient(s.comp_release_seconds),
            makeup: (makeup_db / DB_PER_NEPER).exp(),
            mix,
        })
    }

    /// The gain reduction, dB, the gain computer asks for at `level_db`.
    #[inline]
    pub(super) fn target_db(&self, level_db: f64) -> f64 {
        let over = level_db - self.threshold_db;
        if over <= -COMP_KNEE_DB / 2.0 {
            0.0
        } else if over >= COMP_KNEE_DB / 2.0 {
            -self.slope * over
        } else {
            let x = over + COMP_KNEE_DB / 2.0;
            -self.slope * x * x / (2.0 * COMP_KNEE_DB)
        }
    }

    /// Compress one frame, moving the gain reduction `reduction_db` toward
    /// what the level `peak` asks for: the frame's own, or a sidechain key's.
    #[inline]
    pub(super) fn process(&self, reduction_db: &mut f64, peak: f64, l: f64, r: f64) -> (f64, f64) {
        let level_db = if peak > 0.0 { (peak.ln() * DB_PER_NEPER).max(COMP_FLOOR_DB) } else { COMP_FLOOR_DB };
        let target = self.target_db(level_db);
        // Deeper reduction is the attack, less is the release.
        let speed = if target < *reduction_db { self.attack } else { self.release };
        *reduction_db += speed * (target - *reduction_db);
        flush(reduction_db);
        let gain = (*reduction_db / DB_PER_NEPER).exp() * self.makeup;
        let wet = self.mix * gain + (1.0 - self.mix);
        (l * wet, r * wet)
    }
}

/// The sidechain ducking's settings for one block: an envelope follower on
/// the key, and the gain it takes off.
#[derive(Debug, Clone, Copy)]
pub(super) struct Ducker {
    amount: f64,
    attack: f64,
    release: f64,
}

impl Ducker {
    /// The ducker of `s` at `sr`, `None` when it is off.
    pub(super) fn new(s: &StripSettings, sr: f64) -> Option<Self> {
        let amount = f64::from(s.duck_amount);
        if amount <= 0.0 {
            return None;
        }
        let coefficient = |seconds: f64| 1.0 - (-1.0 / (seconds * sr)).exp();
        Some(Self {
            amount,
            attack: coefficient(DUCK_ATTACK_SECONDS),
            release: coefficient(f64::from(s.duck_release_seconds)),
        })
    }

    /// The gain for one frame, `envelope` following the key's level `peak`.
    #[inline]
    pub(super) fn gain(&self, envelope: &mut f64, peak: f64) -> f64 {
        let speed = if peak > *envelope { self.attack } else { self.release };
        *envelope += speed * (peak - *envelope);
        flush(envelope);
        1.0 - self.amount * (*envelope / DUCK_FULL_LEVEL).min(1.0)
    }
}
