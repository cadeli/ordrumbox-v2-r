//! The shared returns as the engine plays them: what the strips send in a
//! quantum goes through the echo and the reverb ([`ReturnSettings`]), then
//! into the mix, ahead of the master bus. A send closed lets the return
//! play on: the echo of a hit sent for a moment keeps repeating.

use ordrumbox_core::steps::TICK;
use ordrumbox_dsp::{ReturnSettings, Strip, StripSettings};

use super::{IDLE_LEVEL, QUANTUM};
use crate::strip_settings::delay_beats_used;

/// A beat's length until a pass gives the tempo: 120 BPM.
const DEFAULT_BEAT_SECONDS: f64 = 0.5;

/// The echo and the reverb, with what the tracks sent them.
#[derive(Debug, Clone)]
pub(super) struct Returns {
    sample_rate: f64,
    target: ReturnSettings,
    echo: Return,
    reverb: Return,
    /// Seconds a beat lasts in the last pass played: the echo's time is in beats.
    beat_seconds: f64,
}

/// One return: a strip that plays only its effect, its input for the
/// quantum, its output.
#[derive(Debug, Clone)]
struct Return {
    strip: Strip,
    /// Values in use, moving toward the return's settings.
    current: StripSettings,
    input: [[f32; QUANTUM]; 2],
    output: [[f32; QUANTUM]; 2],
    /// Quanta in a row with no input and a quiet output.
    idle: u32,
}

impl Return {
    fn new(sample_rate: f32, settings: StripSettings) -> Self {
        Self {
            strip: Strip::new(sample_rate),
            current: settings,
            input: [[0.0; QUANTUM]; 2],
            output: [[0.0; QUANTUM]; 2],
            idle: 0,
        }
    }

    /// Add `level` times `left`, `right` to the input.
    #[inline]
    fn send(&mut self, level: f32, left: &[f32], right: &[f32]) {
        if level <= 0.0 {
            return;
        }
        let [input_left, input_right] = &mut self.input;
        for (input, s) in input_left.iter_mut().zip(left).chain(input_right.iter_mut().zip(right)) {
            *input += s * level;
        }
    }

    /// Play the first `n` frames of the input into the output, toward
    /// `target`; `false`, with the output left as it was, when the return
    /// has stayed silent for `idle_quanta`.
    fn play(&mut self, target: &StripSettings, keep: f32, n: usize, idle_quanta: u32) -> bool {
        self.current.approach(target, keep);
        let [in_l, in_r] = &self.input;
        let (in_l, in_r) = (&in_l[..n], &in_r[..n]);
        let silent_input = in_l.iter().chain(in_r).all(|&s| s == 0.0);
        if silent_input && self.idle > idle_quanta {
            return false;
        }
        let [out_l, out_r] = &mut self.output;
        let (out_l, out_r) = (&mut out_l[..n], &mut out_r[..n]);
        out_l.fill(0.0);
        out_r.fill(0.0);
        self.strip.process(&self.current, None, in_l, in_r, out_l, out_r);
        let peak = out_l.iter().chain(out_r.iter()).fold(0.0_f32, |peak, s| peak.max(s.abs()));
        self.idle = if silent_input && peak < IDLE_LEVEL { self.idle.saturating_add(1) } else { 0 };
        true
    }

    /// Add the first `n` frames of the output, times `level`, to `left`, `right`.
    #[inline]
    fn mix_into(&self, level: f32, n: usize, left: &mut [f32], right: &mut [f32]) {
        let [out_l, out_r] = &self.output;
        for (sum, s) in left.iter_mut().zip(&out_l[..n]).chain(right.iter_mut().zip(&out_r[..n])) {
            *sum += s * level;
        }
    }
}

impl Returns {
    /// Allocates both returns' memories.
    pub(super) fn new(sample_rate: f32) -> Self {
        let target = ReturnSettings::default();
        let echo_seconds = echo_seconds(&target, DEFAULT_BEAT_SECONDS) as f32;
        Self {
            sample_rate: f64::from(sample_rate),
            target,
            echo: Return::new(sample_rate, target.echo_strip(echo_seconds)),
            reverb: Return::new(sample_rate, target.reverb_strip()),
            beat_seconds: DEFAULT_BEAT_SECONDS,
        }
    }

    pub(super) fn settings(&self) -> ReturnSettings {
        self.target
    }

    /// The settings the returns move to (through the effects' ramp).
    pub(super) fn set_settings(&mut self, settings: ReturnSettings) {
        self.target = settings.clamped();
    }

    /// Take the tempo of a pass of `frames` frames and `ticks` ticks.
    pub(super) fn follow_tempo(&mut self, frames: u64, ticks: u32) {
        if frames > 0 && ticks > 0 {
            self.beat_seconds = frames as f64 * f64::from(TICK) / (f64::from(ticks) * self.sample_rate);
        }
    }

    /// Empty the inputs, for a new quantum.
    pub(super) fn clear(&mut self) {
        for input in [&mut self.echo.input, &mut self.reverb.input].into_iter().flatten() {
            input.fill(0.0);
        }
    }

    /// A strip's output `left`, `right` sent at `settings`' send levels.
    #[inline]
    pub(super) fn send(&mut self, settings: &StripSettings, left: &[f32], right: &[f32]) {
        self.echo.send(settings.echo_send, left, right);
        self.reverb.send(settings.reverb_send, left, right);
    }

    /// Play `out_left.len()` frames of both returns and add them to the mix;
    /// `feedback` is added to the echo's (a live knob's). The echo feeds the
    /// reverb by its `echo_reverb` share.
    pub(super) fn render(
        &mut self,
        out_left: &mut [f32],
        out_right: &mut [f32],
        feedback: f32,
        keep: f32,
        idle_quanta: u32,
    ) {
        let n = out_left.len().min(out_right.len()).min(QUANTUM);
        let target = ReturnSettings {
            echo_feedback: (self.target.echo_feedback + feedback)
                .clamp(ReturnSettings::MIN.echo_feedback, ReturnSettings::MAX.echo_feedback),
            ..self.target
        };
        let echo = target.echo_strip(echo_seconds(&target, self.beat_seconds) as f32);
        if self.echo.play(&echo, keep, n, idle_quanta) {
            self.echo.mix_into(1.0, n, out_left, out_right);
            let [reverb_l, reverb_r] = &mut self.reverb.input;
            self.echo.mix_into(target.echo_reverb, n, reverb_l, reverb_r);
        }
        if self.reverb.play(&target.reverb_strip(), keep, n, idle_quanta) {
            self.reverb.mix_into(1.0, n, out_left, out_right);
        }
    }
}

/// The echo's time, seconds, at beats of `beat_seconds`: its beats, halved
/// until they fit the delay line.
fn echo_seconds(settings: &ReturnSettings, beat_seconds: f64) -> f64 {
    beat_seconds * delay_beats_used(f64::from(settings.echo_beats), 60.0 / beat_seconds)
}
