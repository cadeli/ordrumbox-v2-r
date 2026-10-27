//! One pass of a pattern, ready for the audio thread: every note as a voice
//! start at an exact frame, the strip settings of the tracks and their LFO
//! values tick by tick.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::steps::TICK;
use ordrumbox_core::{FlatNote, Note, Pattern, Rng, Scales, lfo, recompute_flat_notes};
use ordrumbox_dsp::{StripSettings, SynthPatch, VoiceStart};

use crate::kit::SampleBank;
use crate::strip_settings::{StripLfo, strip_lfo, strip_settings};
use crate::synth_preset::{SynthNote, SynthPresets, note_seconds, synth_patch};

/// A synth note to start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthStart {
    pub track: usize,
    /// Index in [`Pass::patches`].
    pub patch: usize,
    /// Seconds until the note is released.
    pub length: f64,
}

/// What an event starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Sound {
    Sample(VoiceStart),
    Synth(SynthStart),
}

impl Sound {
    pub fn track(&self) -> usize {
        match self {
            Self::Sample(v) => v.track,
            Self::Synth(s) => s.track,
        }
    }
}

/// A voice to start at `frame` (from the pass start).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub frame: u64,
    pub sound: Sound,
    /// The track plays one note at a time: starting this one releases the others.
    pub mono: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Pass {
    /// Pass index since play started (selects the `every`/`pos` notes).
    pub number: u64,
    /// Transport tick of the first tick (LFOs run on the transport tick).
    pub first_tick: u64,
    /// Length in frames.
    pub frames: u64,
    /// Length in ticks (a beat is [`TICK`]).
    pub ticks: u32,
    /// Sorted by frame.
    pub events: Vec<Event>,
    /// Strip parameters, by track index (a silent track has volume 0).
    pub strips: Vec<StripSettings>,
    /// Strip LFOs of the tracks that have some.
    pub lfos: Vec<StripLfo>,
    /// Synth patches the events refer to.
    pub patches: Vec<SynthPatch>,
}

impl Pass {
    /// Tick playing at `frame`.
    pub fn tick_at(&self, frame: u64) -> usize {
        if self.frames == 0 {
            0
        } else {
            (u128::from(frame) * u128::from(self.ticks) / u128::from(self.frames)) as usize
        }
    }
}

/// What a pass is built from.
#[derive(Debug, Clone, Copy)]
pub struct PassContext<'a> {
    pub bank: &'a SampleBank,
    /// Sample of each track (see `kit::assign_samples`).
    pub samples: &'a [Option<usize>],
    pub scales: &'a Scales,
    pub synths: &'a SynthPresets,
    pub sample_rate: f64,
    /// Transport tick the pass starts at.
    pub first_tick: u64,
}

/// Frames in one tick at `bpm`.
pub fn frames_per_tick(bpm: f64, sample_rate: f64) -> f64 {
    sample_rate * 60.0 / (bpm * f64::from(TICK))
}

/// Build pass `number` of `pattern` at the pattern's own tempo. Notes of
/// muted tracks (or of tracks another track's solo silences) are left out,
/// as `shouldTrackPlay()` does.
pub fn build_pass(pattern: &Pattern, number: u64, rng: &mut Rng, context: PassContext<'_>) -> Pass {
    let bpm = pattern.bpm.clamp(20.0, 300.0);
    let per_tick = frames_per_tick(bpm, context.sample_rate);
    let ticks = u32::try_from(ordrumbox_core::pattern_engine::pattern_ticks(pattern).max(1)).unwrap_or(u32::MAX);
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let pass_index = u32::try_from(number).unwrap_or(u32::MAX);
    let flat = recompute_flat_notes(pattern, pass_index, rng, context.scales);
    let any_solo = pattern.tracks.iter().any(|t| t.solo);
    let silent = |i: usize| {
        let t = &pattern.tracks[i];
        if any_solo { !t.solo } else { t.mute }
    };

    let mut events = Vec::new();
    let mut patches = Vec::new();
    for (&tick, notes) in &flat {
        for note in notes {
            if silent(note.track) {
                continue;
            }
            let track = &pattern.tracks[note.track];
            let swing_ticks = note.swing_beats(track) * f64::from(TICK);
            // A note swung past the end still plays, on the last frame.
            let frame = (((f64::from(tick) + swing_ticks) * per_tick).round() as u64).min(frames - 1);
            let lfo_tick = context.first_tick + u64::from(tick);
            if let Some(sound) = sound_for(pattern, note, bpm, lfo_tick, &context, &mut patches) {
                events.push(Event { frame, sound, mono: track.mono });
            }
        }
    }
    events.sort_by_key(|e| e.frame); // stable: keeps the engine order within a frame

    let strips = pattern
        .tracks
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let mut settings = strip_settings(t, bpm);
            if silent(i) {
                settings.volume = 0.0;
            }
            settings
        })
        .collect();
    let lfos = pattern
        .tracks
        .iter()
        .enumerate()
        .filter_map(|(i, t)| strip_lfo(i, t, silent(i), context.first_tick, ticks))
        .collect();
    Pass { number, first_tick: context.first_tick, frames, ticks, events, strips, lfos, patches }
}

/// The voice `note` starts (`None`: nothing to play), its synth patch pushed
/// to `patches`. `lfo_tick` is the transport tick for the pitch LFO.
fn sound_for(
    pattern: &Pattern,
    note: &FlatNote,
    bpm: f64,
    lfo_tick: u64,
    context: &PassContext<'_>,
    patches: &mut Vec<SynthPatch>,
) -> Option<Sound> {
    let track = &pattern.tracks[note.track];
    if track.use_soft_synth {
        let preset = track.synth_sound_key.as_deref().and_then(|key| context.synths.get(key))?;
        let synth_note = SynthNote {
            ratio: note.playback_ratio(track),
            velocity: note.velocity,
            pan: note.playback_pan(track),
            bpm,
        };
        patches.push(synth_patch(preset, synth_note));
        return Some(Sound::Synth(SynthStart {
            track: note.track,
            patch: patches.len() - 1,
            length: note_seconds(bpm, track.steps_per_beat),
        }));
    }
    let sample = context.samples.get(note.track).copied().flatten()?;
    let bank_sample = context.bank.get(sample)?;
    let mut ratio = note.playback_ratio(track);
    if let Some(pitch_lfo) = &track.pitch_lfo {
        ratio *= (lfo::value(pitch_lfo, lfo_tick as f64) / 12.0).exp2();
    }
    Some(Sound::Sample(VoiceStart {
        sample,
        track: note.track,
        ratio,
        peak: note.velocity as f32,
        decay: (bank_sample.instrument.decay_ms / 1000.0) as f32,
        pan: note.playback_pan(track) as f32,
        delay: 0,
    }))
}

/// A one-note pass to audition track `t` (`simpleBeep()`): `note` as written,
/// or, without one, a note at the track's velocity and pan. Its strips are
/// the pattern's.
pub fn build_preview(pattern: &Pattern, t: usize, note: Option<&Note>, context: PassContext<'_>) -> Pass {
    let Some(track) = pattern.tracks.get(t) else { return Pass::default() };
    let bpm = pattern.bpm.clamp(20.0, 300.0);
    let flat = FlatNote {
        track: t,
        source: None,
        beat: 0,
        beat_step: 0,
        pitch: note.map_or(0.0, |n| n.pitch),
        velocity: note.map_or(track.velocity, |n| n.velocity),
        pan: note.map_or(track.pan, |n| n.pan),
    };
    let mut patches = Vec::new();
    let events = sound_for(pattern, &flat, bpm, context.first_tick, &context, &mut patches)
        .map(|sound| Event { frame: 0, sound, mono: track.mono })
        .into_iter()
        .collect();
    let strips = pattern.tracks.iter().map(|t| strip_settings(t, bpm)).collect();
    Pass { frames: 1, ticks: 1, events, strips, patches, ..Pass::default() }
}
