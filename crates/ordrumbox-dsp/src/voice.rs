//! One playing sample (`audio/voices/sample_voice.js`): read at a pitch
//! ratio with cubic interpolation, shaped by a 5 ms linear attack and an
//! exponential decay to -60 dB, placed in the stereo field like a Web Audio
//! `StereoPannerNode`.
//!
//! A voice is a plain value: starting, releasing and rendering it never allocates.

use std::f32::consts::FRAC_PI_2;

use crate::sample::SampleBuffer;

/// Attack ramp (`GAIN_ATTACK_RAMP`).
const ATTACK_SECONDS: f32 = 0.005;
/// Fade used when a voice is cut (stolen, mono track): `STOP_BUFFER`.
const RELEASE_SECONDS: f32 = 0.015;
/// Level the decay ends at (`MIN_GAIN_VALUE`, -60 dB): the voice stops there.
const MIN_GAIN: f32 = 0.001;
/// Shortest decay a sample may have.
const MIN_DECAY_SECONDS: f32 = 0.02;

/// What to play.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VoiceStart {
    /// Index of the sample in the caller's bank (the voice keeps only the index).
    pub sample: usize,
    /// Track (strip) the voice plays into.
    pub track: usize,
    /// Playback-rate ratio from the pitch (1 = as recorded).
    pub ratio: f64,
    /// Peak level: velocity × sample gain.
    pub peak: f32,
    /// Decay to -60 dB, in seconds.
    pub decay: f32,
    /// -1 (left) to 1 (right).
    pub pan: f32,
    /// Frames to wait, within the next rendered block, before the first sample.
    pub delay: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

#[derive(Debug, Clone)]
pub struct SampleVoice {
    stage: Stage,
    sample: usize,
    track: usize,
    /// Read position in the sample, in its own frames.
    position: f64,
    /// Sample frames advanced per output frame.
    step: f64,
    gain: f32,
    /// Output frames left in the current stage.
    stage_left: u32,
    attack_step: f32,
    decay_factor: f32,
    decay_frames: u32,
    release_factor: f32,
    /// `[in_left → out_left, in_right → out_left, in_left → out_right, in_right → out_right]`.
    pan: [f32; 4],
    delay: usize,
    /// Start order, to steal the oldest voice first.
    serial: u64,
}

impl Default for SampleVoice {
    fn default() -> Self {
        Self {
            stage: Stage::Idle,
            sample: 0,
            track: 0,
            position: 0.0,
            step: 1.0,
            gain: 0.0,
            stage_left: 0,
            attack_step: 0.0,
            decay_factor: 1.0,
            decay_frames: 0,
            release_factor: 1.0,
            pan: [0.0; 4],
            delay: 0,
            serial: 0,
        }
    }
}

/// Gains of a Web Audio `StereoPannerNode`: `[in_left → out_left, in_right → out_left, in_left → out_right, in_right → out_right]`.
pub fn stereo_panner(pan: f32, stereo_input: bool) -> [f32; 4] {
    let pan = pan.clamp(-1.0, 1.0);
    if !stereo_input {
        let x = (pan + 1.0) / 2.0 * FRAC_PI_2;
        return [x.cos(), 0.0, x.sin(), 0.0];
    }
    if pan <= 0.0 {
        let x = (pan + 1.0) * FRAC_PI_2;
        [1.0, x.cos(), 0.0, x.sin()]
    } else {
        let x = pan * FRAC_PI_2;
        [x.cos(), 0.0, x.sin(), 1.0]
    }
}

impl SampleVoice {
    pub fn is_active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn track(&self) -> usize {
        self.track
    }

    pub fn sample(&self) -> usize {
        self.sample
    }

    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Start `params` on `buffer`, rendered at `sample_rate`. A peak at or below
    /// -60 dB plays nothing.
    pub fn start(&mut self, params: &VoiceStart, buffer: &SampleBuffer, sample_rate: f32, serial: u64) {
        if params.peak <= MIN_GAIN || buffer.frames() == 0 || !params.ratio.is_finite() || params.ratio <= 0.0 {
            self.stage = Stage::Idle;
            return;
        }
        let attack_frames = (ATTACK_SECONDS * sample_rate).round().max(1.0) as u32;
        let decay_frames = (params.decay.max(MIN_DECAY_SECONDS) * sample_rate).round().max(1.0) as u32;
        *self = Self {
            stage: Stage::Attack,
            sample: params.sample,
            track: params.track,
            position: 0.0,
            step: params.ratio * f64::from(buffer.sample_rate) / f64::from(sample_rate),
            gain: 0.0,
            stage_left: attack_frames,
            attack_step: params.peak / attack_frames as f32,
            decay_factor: (MIN_GAIN / params.peak).powf(1.0 / decay_frames as f32),
            decay_frames,
            release_factor: 1.0,
            pan: stereo_panner(params.pan, buffer.is_stereo()),
            delay: params.delay,
            serial,
        };
    }

    /// Fade out quickly (15 ms to -60 dB) and stop.
    pub fn release(&mut self, sample_rate: f32) {
        if !self.is_active() || self.stage == Stage::Release {
            return;
        }
        let frames = (RELEASE_SECONDS * sample_rate).round().max(1.0) as u32;
        let from = self.gain.max(MIN_GAIN);
        self.release_factor = (MIN_GAIN / from).powf(1.0 / frames as f32);
        self.gain = from;
        self.stage = Stage::Release;
        self.stage_left = frames;
    }

    /// Add the next `left.len()` frames into `left`/`right`.
    pub fn render(&mut self, buffer: &SampleBuffer, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        let start = self.delay.min(frames);
        self.delay -= start;
        let right_channel = buffer.right.as_deref().unwrap_or(&buffer.left);
        let [ll, rl, lr, rr] = self.pan;
        let end_position = buffer.frames() as f64;

        for i in start..frames {
            if self.stage == Stage::Idle {
                return;
            }
            if self.position >= end_position {
                self.stage = Stage::Idle;
                return;
            }
            let in_l = SampleBuffer::read(&buffer.left, self.position) * self.gain;
            let in_r = SampleBuffer::read(right_channel, self.position) * self.gain;
            left[i] += ll * in_l + rl * in_r;
            right[i] += lr * in_l + rr * in_r;
            self.position += self.step;
            self.advance_envelope();
        }
    }

    #[inline]
    fn advance_envelope(&mut self) {
        match self.stage {
            Stage::Attack => self.gain += self.attack_step,
            Stage::Decay => self.gain *= self.decay_factor,
            Stage::Release => self.gain *= self.release_factor,
            Stage::Idle => return,
        }
        self.stage_left = self.stage_left.saturating_sub(1);
        if self.stage_left == 0 {
            match self.stage {
                Stage::Attack => {
                    self.stage = Stage::Decay;
                    self.stage_left = self.decay_frames;
                }
                _ => self.stage = Stage::Idle,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 1000.0;

    fn ones(frames: usize) -> SampleBuffer {
        SampleBuffer::mono(vec![1.0; frames], 1000)
    }

    fn params() -> VoiceStart {
        VoiceStart { sample: 0, track: 0, ratio: 1.0, peak: 1.0, decay: 0.1, pan: 0.0, delay: 0 }
    }

    fn render(voice: &mut SampleVoice, buffer: &SampleBuffer, frames: usize) -> (Vec<f32>, Vec<f32>) {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        voice.render(buffer, &mut l, &mut r);
        (l, r)
    }

    #[test]
    fn envelope_rises_then_decays_to_minus_60_db() {
        let buffer = ones(1000);
        let mut voice = SampleVoice::default();
        voice.start(&params(), &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 200);
        let center = (std::f32::consts::FRAC_PI_4).cos();
        assert_eq!(l[0], 0.0); // attack starts from silence
        assert!((l[5] / center - 1.0).abs() < 1e-3, "peak after 5 ms: {}", l[5]);
        // Last frame of the 100 ms decay, one step above -60 dB.
        assert!((l[104] / center / MIN_GAIN - 1.0).abs() < 0.1, "-60 dB after the decay: {}", l[104]);
        assert!(!voice.is_active());
        assert_eq!(l[150], 0.0);
    }

    #[test]
    fn delay_and_ratio() {
        let buffer = ones(10);
        let mut voice = SampleVoice::default();
        voice.start(&VoiceStart { ratio: 2.0, delay: 3, decay: 10.0, ..params() }, &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 20);
        assert_eq!(&l[..4], &[0.0; 4]);
        assert!(l[4] > 0.0);
        assert_eq!(l[9], 0.0, "10 frames read at ratio 2 last 5 output frames");
    }

    #[test]
    fn stereo_panner_matches_web_audio() {
        let [l, _, r, _] = stereo_panner(0.0, false);
        assert!((l - r).abs() < 1e-6 && (l - 0.70710677).abs() < 1e-6);
        assert!(stereo_panner(-1.0, false)[2].abs() < 1e-6);
        assert_eq!(stereo_panner(0.0, true), [1.0, (FRAC_PI_2).cos(), 0.0, 1.0]);
        let [ll, rl, lr, rr] = stereo_panner(1.0, true);
        assert!(ll.abs() < 1e-6 && rl == 0.0 && (lr - 1.0).abs() < 1e-6 && rr == 1.0);
    }

    #[test]
    fn release_fades_in_15_ms() {
        let buffer = ones(1000);
        let mut voice = SampleVoice::default();
        voice.start(&VoiceStart { decay: 10.0, ..params() }, &buffer, RATE, 1);
        render(&mut voice, &buffer, 10);
        voice.release(RATE);
        let (l, _) = render(&mut voice, &buffer, 30);
        assert!(l[14] < 0.002 && !voice.is_active());
    }

    #[test]
    fn inaudible_or_empty_starts_nothing() {
        let mut voice = SampleVoice::default();
        voice.start(&VoiceStart { peak: 0.0, ..params() }, &ones(10), RATE, 1);
        assert!(!voice.is_active());
        voice.start(&params(), &SampleBuffer::default(), RATE, 1);
        assert!(!voice.is_active());
    }
}
