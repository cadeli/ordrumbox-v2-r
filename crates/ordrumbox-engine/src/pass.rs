//! One pass of a pattern, ready for the audio thread: every note as a voice
//! start at an exact frame, plus the strip settings of the tracks.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Pattern, Rng, Scales, recompute_flat_notes};
use ordrumbox_dsp::{StripParams, VoiceStart};

use crate::kit::SampleBank;

/// A voice to start at `frame` (from the pass start).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub frame: u64,
    pub voice: VoiceStart,
    /// The track plays one note at a time: starting this one cuts the others.
    pub mono: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Pass {
    /// Pass index since play started (selects the `every`/`pos` notes).
    pub number: u64,
    /// Length in frames.
    pub frames: u64,
    /// Length in ticks (a beat is [`TICK`]).
    pub ticks: u32,
    /// Sorted by frame.
    pub events: Vec<Event>,
    /// One per track, by track index.
    pub strips: Vec<StripParams>,
}

/// What a pass is built from.
#[derive(Debug, Clone, Copy)]
pub struct PassContext<'a> {
    pub bank: &'a SampleBank,
    /// Sample of each track (see `kit::assign_samples`).
    pub samples: &'a [Option<usize>],
    pub scales: &'a Scales,
    pub sample_rate: f64,
}

/// Frames in one tick at `bpm`.
pub fn frames_per_tick(bpm: f64, sample_rate: f64) -> f64 {
    sample_rate * 60.0 / (bpm * f64::from(TICK))
}

/// Build pass `number` of `pattern` at the pattern's own tempo.
pub fn build_pass(pattern: &Pattern, number: u64, rng: &mut Rng, context: PassContext<'_>) -> Pass {
    let per_tick = frames_per_tick(pattern.bpm.clamp(20.0, 300.0), context.sample_rate);
    let ticks = u32::try_from(ordrumbox_core::pattern_engine::pattern_ticks(pattern).max(1)).unwrap_or(u32::MAX);
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let pass_index = u32::try_from(number).unwrap_or(u32::MAX);
    let flat = recompute_flat_notes(pattern, pass_index, rng, context.scales);

    let mut events = Vec::new();
    for (&tick, notes) in &flat {
        for note in notes {
            let track = &pattern.tracks[note.track];
            let Some(sample) = context.samples.get(note.track).copied().flatten() else { continue };
            let Some(bank_sample) = context.bank.get(sample) else { continue };
            let swing_ticks = note.swing_beats(track) * f64::from(TICK);
            let frame = ((f64::from(tick) + swing_ticks) * per_tick).round() as u64;
            events.push(Event {
                // A note swung past the end still plays, on the last frame.
                frame: frame.min(frames - 1),
                voice: VoiceStart {
                    sample,
                    track: note.track,
                    ratio: note.playback_ratio(track),
                    peak: note.velocity as f32,
                    decay: (bank_sample.instrument.decay_ms / 1000.0) as f32,
                    pan: note.playback_pan(track) as f32,
                    delay: 0,
                },
                mono: track.mono,
            });
        }
    }
    events.sort_by_key(|e| e.frame); // stable: keeps the engine order within a frame

    let any_solo = pattern.tracks.iter().any(|t| t.solo);
    let strips = pattern
        .tracks
        .iter()
        .map(|t| StripParams { volume: t.velocity as f32, pan: t.pan as f32, silent: t.mute || (any_solo && !t.solo) })
        .collect();
    Pass { number, frames, ticks, events, strips }
}
