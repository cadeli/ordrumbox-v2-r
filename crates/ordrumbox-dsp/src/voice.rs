//! One playing sample (`audio/voices/sample_voice.js`): read at a pitch
//! ratio with cubic interpolation, shaped by a 5 ms linear attack and an
//! exponential decay to -60 dB (a [`SampleShape`] moves the start, the end,
//! the direction, the loop, the attack and a hold at full level), placed in the stereo field like a Web Audio
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
/// Overlap of a loop's two ends, so that it does not click: the end fades
/// into the start (at most a quarter of the loop).
const LOOP_CROSSFADE_SECONDS: f64 = 0.01;
/// Shortest decay a sample may have.
const MIN_DECAY_SECONDS: f32 = 0.02;

/// How a sample is read and shaped, besides its pitch and decay. The default
/// plays the whole sample forward, once, with the 5 ms attack.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SampleShape {
    /// Where reading starts, seconds into the sample.
    pub start: f32,
    /// Where reading ends, seconds into the sample; `None`: the sample's end.
    pub end: Option<f32>,
    /// Read from the end of the region back to its start.
    pub reverse: bool,
    /// Read the region over and over, until the decay ends the voice.
    pub looping: bool,
    /// Where the loop starts and ends, seconds into the sample, within the
    /// region; `None`: the region's own start or end. The voice plays from
    /// the region's start (its end, reversed) into the loop, then loops it.
    pub loop_start: Option<f32>,
    pub loop_end: Option<f32>,
    /// Linear rise to the peak, seconds.
    pub attack: f32,
    /// Time kept at the peak before the decay, seconds.
    pub hold: f32,
    /// Keep the peak until the note ends (`VoiceStart::length`): the decay is then its release.
    pub gated: bool,
}

impl Default for SampleShape {
    fn default() -> Self {
        Self {
            start: 0.0,
            end: None,
            reverse: false,
            looping: false,
            loop_start: None,
            loop_end: None,
            attack: ATTACK_SECONDS,
            hold: 0.0,
            gated: false,
        }
    }
}

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
    /// How long the note lasts, seconds (only a gated shape reads it).
    pub length: f32,
    pub shape: SampleShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Hold,
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
    /// Sample frames advanced per output frame (negative when reversed).
    step: f64,
    /// Where the voice stops (or loops) on its way, in the sample's own
    /// frames: below `low` or at `high`. Looping, the loop's edge ahead and
    /// the region's edge behind (which a voice never crosses going away from it).
    low: f64,
    high: f64,
    /// The loop, `[loop_low, loop_high)`, inside the region.
    loop_low: f64,
    loop_high: f64,
    looping: bool,
    /// Frames of the loop's crossfade (0: none).
    fade: f64,
    gain: f32,
    peak: f32,
    /// Output frames left in the current stage.
    stage_left: u32,
    attack_step: f32,
    decay_factor: f32,
    hold_frames: u32,
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
            low: 0.0,
            high: 0.0,
            loop_low: 0.0,
            loop_high: 0.0,
            looping: false,
            fade: 0.0,
            gain: 0.0,
            peak: 0.0,
            stage_left: 0,
            attack_step: 0.0,
            decay_factor: 1.0,
            hold_frames: 0,
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
        let shape = &params.shape;
        let attack_frames = (shape.attack.max(0.0) * sample_rate).round().max(1.0) as u32;
        // A gated voice holds until its note ends, then decays.
        let held = if shape.gated { shape.hold.max(params.length - shape.attack) } else { shape.hold };
        let hold_frames = (held.max(0.0) * sample_rate).round() as u32;
        let frames = buffer.frames() as f64;
        let per_second = f64::from(buffer.sample_rate);
        let low = (f64::from(shape.start.max(0.0)) * per_second).min(frames - 1.0).floor();
        let high = shape.end.map_or(frames, |end| (f64::from(end) * per_second).min(frames).ceil()).max(low + 1.0);
        let step = params.ratio * per_second / f64::from(sample_rate);
        let frame_of = |seconds: Option<f32>| seconds.map(|s| f64::from(s.max(0.0)) * per_second);
        let loop_low = frame_of(shape.loop_start).map_or(low, |f| f.floor().clamp(low, high - 1.0));
        let loop_high = frame_of(shape.loop_end).map_or(high, |f| f.ceil().clamp(loop_low + 1.0, high));
        // Looping, the voice wraps at the loop's edge ahead of it.
        let (stop_low, stop_high) = match (shape.looping, shape.reverse) {
            (false, _) => (low, high),
            (true, false) => (low, loop_high),
            (true, true) => (loop_low, high),
        };
        let decay_frames = (params.decay.max(MIN_DECAY_SECONDS) * sample_rate).round().max(1.0) as u32;
        *self = Self {
            stage: Stage::Attack,
            sample: params.sample,
            track: params.track,
            position: if shape.reverse { high - 1.0 } else { low },
            step: if shape.reverse { -step } else { step },
            low: stop_low,
            high: stop_high,
            loop_low,
            loop_high,
            looping: shape.looping,
            fade: loop_fade(shape.looping, loop_high - loop_low, per_second),
            gain: 0.0,
            peak: params.peak,
            stage_left: attack_frames,
            attack_step: params.peak / attack_frames as f32,
            decay_factor: (MIN_GAIN / params.peak).powf(1.0 / decay_frames as f32),
            hold_frames,
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
        let (low, high) = (self.low, self.high);
        let span = LoopSpan { low: self.loop_low, high: self.loop_high, fade: self.fade };

        let outputs = left[start..frames].iter_mut().zip(&mut right[start..frames]);
        for (out_l, out_r) in outputs {
            if self.stage == Stage::Idle {
                return;
            }
            if self.position >= high || self.position < low {
                if !self.looping {
                    self.stage = Stage::Idle;
                    return;
                }
                self.position = span.wrap(self.position, self.step > 0.0);
            }
            let (mut in_l, mut in_r) =
                (SampleBuffer::read(&buffer.left, self.position), SampleBuffer::read(right_channel, self.position));
            if let Some((partner, mix)) = span.crossfade(self.position, self.step > 0.0) {
                in_l += (SampleBuffer::read(&buffer.left, partner) - in_l) * mix;
                in_r += (SampleBuffer::read(right_channel, partner) - in_r) * mix;
            }
            let (in_l, in_r) = (in_l * self.gain, in_r * self.gain);
            *out_l += ll * in_l + rl * in_r;
            *out_r += lr * in_l + rr * in_r;
            self.position += self.step;
            self.advance_envelope();
        }
    }

    #[inline]
    fn advance_envelope(&mut self) {
        match self.stage {
            Stage::Attack => self.gain += self.attack_step,
            Stage::Hold => {}
            Stage::Decay => self.gain *= self.decay_factor,
            Stage::Release => self.gain *= self.release_factor,
            Stage::Idle => return,
        }
        self.stage_left = self.stage_left.saturating_sub(1);
        if self.stage_left == 0 {
            match self.stage {
                Stage::Attack if self.hold_frames > 0 => {
                    self.stage = Stage::Hold;
                    self.stage_left = self.hold_frames;
                    self.gain = self.peak;
                }
                Stage::Attack | Stage::Hold => {
                    self.stage = Stage::Decay;
                    self.stage_left = self.decay_frames;
                }
                _ => self.stage = Stage::Idle,
            }
        }
    }
}

/// The loop a voice plays, in the sample's own frames: `[low, high)`, and
/// the frames its two ends overlap (0: none).
#[derive(Debug, Clone, Copy)]
struct LoopSpan {
    low: f64,
    high: f64,
    fade: f64,
}

impl LoopSpan {
    /// Where a voice past the loop's edge goes on, `forward` or not: back
    /// into it, `fade` frames in, as its first frames were already heard in the crossfade.
    #[inline]
    fn wrap(self, position: f64, forward: bool) -> f64 {
        let length = self.high - self.low - self.fade;
        if forward {
            self.low + self.fade + (position - self.low - self.fade).rem_euclid(length)
        } else {
            self.high - self.fade - (self.high - self.fade - position).rem_euclid(length)
        }
    }

    /// Near the edge the voice is heading to, the loop's other end fading
    /// in: where to read it, and how much of it.
    #[inline]
    fn crossfade(self, position: f64, forward: bool) -> Option<(f64, f32)> {
        if self.fade <= 0.0 {
            return None;
        }
        let distance = if forward { self.high - position } else { position - self.low };
        if distance >= self.fade {
            return None;
        }
        let partner = if forward { self.low + self.fade - distance } else { self.high - self.fade + distance };
        Some((partner, (1.0 - distance / self.fade) as f32))
    }
}

/// Frames of a loop's crossfade: its length in seconds at `per_second`, at most a quarter of the loop.
fn loop_fade(looping: bool, length: f64, per_second: f64) -> f64 {
    let fade = (LOOP_CROSSFADE_SECONDS * per_second).min(length / 4.0).floor();
    if looping && fade >= 2.0 { fade } else { 0.0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 1000.0;

    fn ones(frames: usize) -> SampleBuffer {
        SampleBuffer::mono(vec![1.0; frames], 1000)
    }

    fn params() -> VoiceStart {
        VoiceStart {
            sample: 0,
            track: 0,
            ratio: 1.0,
            peak: 1.0,
            decay: 0.1,
            pan: 0.0,
            delay: 0,
            length: 0.0,
            shape: SampleShape::default(),
        }
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

    /// A ramp of `frames` frames, 0, 1, 2…: each frame says where it was read.
    fn ramp(frames: usize) -> SampleBuffer {
        SampleBuffer::mono((0..frames).map(|i| i as f32).collect(), 1000)
    }

    fn shaped(shape: SampleShape) -> VoiceStart {
        VoiceStart { decay: 10.0, shape: SampleShape { attack: 0.001, ..shape }, ..params() }
    }

    #[test]
    fn start_and_end_cut_the_region() {
        let buffer = ramp(100);
        let mut voice = SampleVoice::default();
        voice.start(&shaped(SampleShape { start: 0.02, end: Some(0.04), ..SampleShape::default() }), &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 60);
        assert!(l[0] == 0.0 && l[1] > 0.0, "the first read frame is frame 20, not 0");
        assert_eq!(l[21], 0.0, "stops at the end, after 20 frames");
        assert!(!voice.is_active());
    }

    #[test]
    fn reverse_reads_the_region_backwards() {
        let buffer = ramp(100);
        let mut voice = SampleVoice::default();
        voice.start(
            &shaped(SampleShape { end: Some(0.05), reverse: true, ..SampleShape::default() }),
            &buffer,
            RATE,
            1,
        );
        let (l, _) = render(&mut voice, &buffer, 80);
        // Frame 49 first, then 48…: the ramp falls while the gain is steady.
        assert!(l[20] > l[30] && l[30] > l[40], "{} {} {}", l[20], l[30], l[40]);
        assert_eq!(l[60], 0.0, "ends after the 50 frames of the region");
    }

    #[test]
    fn a_loop_plays_until_the_decay_ends() {
        let buffer = ones(10);
        let mut voice = SampleVoice::default();
        voice.start(&shaped(SampleShape { looping: true, ..SampleShape::default() }), &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 5000);
        assert!(l[500] > 0.0 && l[900] > 0.0, "still playing long after the 10 frames");
        assert!(l[4900] > 0.0 && voice.is_active(), "the 10 s decay is far from over");
    }

    /// Where a voice of `shape` reads, frame by frame, over `frames` frames
    /// of a 100-frame sample: a ramp's level over a flat one's.
    fn read_positions(shape: SampleShape, frames: usize) -> Vec<f32> {
        let (ramp, flat) = (ramp(100), ones(100));
        let (mut a, mut b) = (SampleVoice::default(), SampleVoice::default());
        a.start(&shaped(shape), &ramp, RATE, 1);
        b.start(&shaped(shape), &flat, RATE, 1);
        let (l, _) = render(&mut a, &ramp, frames);
        let (gain, _) = render(&mut b, &flat, frames);
        l.iter().zip(&gain).map(|(v, g)| if *g > 0.0 { v / g } else { -1.0 }).collect()
    }

    #[test]
    fn a_loop_inside_the_region_plays_its_way_in_then_loops() {
        let (loop_start, loop_end) = (Some(0.06), Some(0.08));
        let shape = SampleShape { looping: true, loop_start, loop_end, ..SampleShape::default() };
        let read = read_positions(shape, 600);
        assert!((read[10] - 10.0).abs() < 1e-3 && (read[50] - 50.0).abs() < 1e-3, "from the start: {read:?}");
        let looped = &read[100..];
        assert!(looped.iter().all(|&p| (60.0..80.0).contains(&p)), "then frames 60 to 80 only");
        // Backwards: from the region's end down into the loop, then the loop.
        let read = read_positions(SampleShape { reverse: true, ..shape }, 600);
        assert!((read[5] - 94.0).abs() < 1e-3, "from the end: {}", read[5]);
        assert!(read[100..].iter().all(|&p| (60.0..80.0).contains(&p)));
        // Without loop points, the whole region loops, as before.
        let whole = read_positions(SampleShape { looping: true, ..SampleShape::default() }, 600);
        assert!(whole[150..].iter().any(|&p| p < 20.0) && whole[150..].iter().any(|&p| p > 80.0));
    }

    #[test]
    fn loop_points_are_kept_inside_the_region() {
        // A loop end past the region's end, a loop start before its start.
        let shape = SampleShape {
            start: 0.02,
            end: Some(0.05),
            looping: true,
            loop_start: Some(0.0),
            loop_end: Some(0.09),
            ..SampleShape::default()
        };
        let read = read_positions(shape, 400);
        let outside: Vec<_> =
            read.iter().enumerate().skip(1).filter(|(_, p)| !(20.0..50.0).contains(*p)).take(5).collect();
        assert!(outside.is_empty(), "within frames 20 to 50: {outside:?}");
    }

    #[test]
    fn hold_keeps_the_peak_before_the_decay() {
        let buffer = ones(1000);
        let mut voice = SampleVoice::default();
        let shape = SampleShape { hold: 0.1, ..SampleShape::default() };
        voice.start(&VoiceStart { decay: 0.1, shape, ..params() }, &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 300);
        let center = (std::f32::consts::FRAC_PI_4).cos();
        assert!((l[50] / center - 1.0).abs() < 1e-3 && (l[100] / center - 1.0).abs() < 1e-3, "{} {}", l[50], l[100]);
        assert!(l[150] < l[100] * 0.5, "the decay has begun: {}", l[150]);
    }

    #[test]
    fn a_gated_voice_holds_until_the_note_ends() {
        let buffer = ones(100);
        let mut voice = SampleVoice::default();
        let shape = SampleShape { gated: true, looping: true, ..SampleShape::default() };
        voice.start(&VoiceStart { decay: 0.1, length: 0.3, shape, ..params() }, &buffer, RATE, 1);
        let (l, _) = render(&mut voice, &buffer, 600);
        let center = (std::f32::consts::FRAC_PI_4).cos();
        assert!((l[290] / center - 1.0).abs() < 1e-3, "still at the peak just before the note ends: {}", l[290]);
        assert!(l[350] < l[290] * 0.5, "the decay follows the note end: {}", l[350]);
        assert!(!voice.is_active());
    }

    #[test]
    fn a_loop_crossfades_its_ends_without_a_jump() {
        // A ramp's ends are far apart: a hard loop jumps by the whole ramp.
        let buffer = ramp(100);
        for reverse in [false, true] {
            let mut voice = SampleVoice::default();
            let shape = SampleShape { looping: true, reverse, ..SampleShape::default() };
            voice.start(&shaped(shape), &buffer, RATE, 1);
            let (l, _) = render(&mut voice, &buffer, 600);
            let center = (std::f32::consts::FRAC_PI_4).cos();
            let jump = l[20..500].windows(2).map(|w| (w[1] - w[0]).abs() / center).fold(0.0, f32::max);
            assert!(jump < 10.0, "reverse {reverse}: the loop seam jumps by {jump}");
        }
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
