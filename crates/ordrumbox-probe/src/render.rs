//! One note of a synth preset, rendered by the voice the app plays it with.

use ordrumbox_core::json::Value;
use ordrumbox_dsp::{SampleBuffer, SynthVoice};
use ordrumbox_engine::synth_preset::{SynthNote, note_seconds, synth_patch};

/// Frames rendered at a time, the engine's block size.
const BLOCK_FRAMES: usize = 128;
/// Longest tail after the release, seconds; every preset ends well before it.
const MAX_TAIL_SECONDS: f64 = 10.0;
/// Clock position of the second note of [`repeat_difference_db`]: a time that
/// falls on no simple cycle of an LFO.
const REPEAT_START_FRAME: u64 = 65_761;

/// The note to render and the context the app would play it in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteSettings {
    /// Pitch ratio (1 = C3).
    pub ratio: f64,
    pub velocity: f64,
    pub pan: f64,
    pub bpm: f64,
    pub steps_per_beat: i32,
    /// Seconds before the release; `None` is what the app plays, one step.
    pub gate_seconds: Option<f64>,
    pub sample_rate: u32,
}

impl Default for NoteSettings {
    fn default() -> Self {
        Self {
            ratio: 1.0,
            velocity: 0.8,
            pan: 0.0,
            bpm: 120.0,
            steps_per_beat: 4,
            gate_seconds: None,
            sample_rate: 48_000,
        }
    }
}

impl NoteSettings {
    /// Seconds the note is held.
    pub fn gate(&self) -> f64 {
        self.gate_seconds.unwrap_or_else(|| note_seconds(self.bpm, self.steps_per_beat))
    }
}

/// Render the note from its start to the end of its release, the voice's
/// clock at `start_frame`.
fn render_from(preset: &Value, settings: &NoteSettings, start_frame: u64) -> Result<SampleBuffer, String> {
    let note = SynthNote { ratio: settings.ratio, velocity: settings.velocity, pan: settings.pan, bpm: settings.bpm };
    let rate = f64::from(settings.sample_rate);
    let start = start_frame as f64 / rate;
    let mut voice = SynthVoice::default();
    // Numbered as the engine numbers its notes: each has its own noise.
    voice.start(&synth_patch(preset, note), 0, start, 1 + u64::from(start_frame > 0));
    voice.release_at(start + settings.gate());

    let max_frames = ((settings.gate() + MAX_TAIL_SECONDS) * rate) as usize;
    let (mut left, mut right) = (Vec::new(), Vec::new());
    while voice.is_active() {
        if left.len() >= max_frames {
            return Err(format!("the note still plays {MAX_TAIL_SECONDS} s after its release"));
        }
        let done = left.len();
        left.resize(done + BLOCK_FRAMES, 0.0);
        right.resize(done + BLOCK_FRAMES, 0.0);
        voice.render(settings.sample_rate as f32, start_frame + done as u64, &mut left[done..], &mut right[done..]);
    }
    Ok(SampleBuffer { left, right: Some(right), sample_rate: settings.sample_rate })
}

/// The note of `preset` for `settings`.
pub fn render_note(preset: &Value, settings: &NoteSettings) -> Result<SampleBuffer, String> {
    render_from(preset, settings, 0)
}

/// How far a second, later note differs from `first`, dB relative to it:
/// `None` when they are the same to the last bit (the same noise and the same
/// LFO position on every hit).
pub fn repeat_difference_db(
    preset: &Value,
    settings: &NoteSettings,
    first: &SampleBuffer,
) -> Result<Option<f64>, String> {
    let second = render_from(preset, settings, REPEAT_START_FRAME)?;
    let frames = first.left.len().min(second.left.len());
    let (mut difference, mut energy) = (0.0, 0.0);
    for (a, b) in first.left[..frames].iter().zip(&second.left[..frames]) {
        let (a, b) = (f64::from(*a), f64::from(*b));
        difference += (a - b) * (a - b);
        energy += a * a;
    }
    if difference == 0.0 || energy == 0.0 {
        return Ok(None);
    }
    Ok(Some(10.0 * (difference / energy).log10()))
}
