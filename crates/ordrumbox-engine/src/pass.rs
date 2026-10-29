//! One pass of a pattern, ready for the audio thread: every note as a voice
//! start at an exact frame, the strip settings of the tracks and their LFO
//! values tick by tick.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::steps::TICK;
use std::collections::HashMap;

use ordrumbox_core::{FlatNote, FlatNotes, Note, Pattern, Rng, Scales, Song, lfo, recompute_flat_notes};
use ordrumbox_dsp::{StripSettings, SynthPatch, VoiceStart};

use crate::assign::TrackSound;
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
    /// Pass index since play started (selects the notes by `cycle`/`cycle_offset`).
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
    /// What each track plays (see [`crate::assign`]).
    pub sounds: &'a [TrackSound],
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
            let Some(assigned) = context.sounds.get(note.track) else { continue };
            let voice = Voice { strip: note.track, lfo_tick, bpm };
            if let Some(sound) = sound_for(pattern, note, assigned, voice, &context, &mut patches) {
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

/// Where and how a voice plays.
#[derive(Debug, Clone, Copy)]
struct Voice {
    /// Strip the voice plays into.
    strip: usize,
    /// Transport tick, for the pitch LFO.
    lfo_tick: u64,
    bpm: f64,
}

/// The voice `note` starts with `assigned` (`None`: nothing to play), its
/// synth patch pushed to `patches`.
fn sound_for(
    pattern: &Pattern,
    note: &FlatNote,
    assigned: &TrackSound,
    voice: Voice,
    context: &PassContext<'_>,
    patches: &mut Vec<SynthPatch>,
) -> Option<Sound> {
    let Voice { strip, lfo_tick, bpm } = voice;
    let track = &pattern.tracks[note.track];
    let sample = match assigned {
        TrackSound::None => return None,
        TrackSound::Synth(key) => {
            let preset = context.synths.get(key)?;
            let synth_note = SynthNote {
                ratio: note.playback_ratio(track),
                velocity: note.velocity,
                pan: note.playback_pan(track),
                bpm,
            };
            patches.push(synth_patch(preset, synth_note));
            return Some(Sound::Synth(SynthStart {
                track: strip,
                patch: patches.len() - 1,
                length: note_seconds(bpm, track.steps_per_beat),
            }));
        }
        TrackSound::Sample(sample) => *sample,
    };
    let bank_sample = context.bank.get(sample)?;
    // The sample's tune and gain trim (`sample.tune`, `sample.gainDb`).
    let mut ratio = note.playback_ratio(track) * (bank_sample.sample.transpose / 12.0).exp2();
    if let Some(pitch_lfo) = &track.transpose_lfo {
        ratio *= (lfo::value(pitch_lfo, lfo_tick as f64) / 12.0).exp2();
    }
    Some(Sound::Sample(VoiceStart {
        sample,
        track: strip,
        ratio,
        peak: (note.velocity * bank_sample.sample.gain()) as f32,
        decay: (bank_sample.sample.decay_ms() / 1000.0) as f32,
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
        step: 0,
        pitch: note.map_or(0.0, |n| n.pitch),
        velocity: note.map_or(track.volume, |n| n.velocity),
        pan: note.map_or(track.pan, |n| n.pan),
    };
    let mut patches = Vec::new();
    let voice = Voice { strip: t, lfo_tick: context.first_tick, bpm };
    let events = context
        .sounds
        .get(t)
        .and_then(|assigned| sound_for(pattern, &flat, assigned, voice, &context, &mut patches))
        .map(|sound| Event { frame: 0, sound, mono: track.mono })
        .into_iter()
        .collect();
    let strips = pattern.tracks.iter().map(|t| strip_settings(t, bpm)).collect();
    Pass { frames: 1, ticks: 1, events, strips, patches, ..Pass::default() }
}

/// Flat notes of a pattern repetition, kept between song passes so a
/// pattern longer than a measure is drawn once per repetition.
pub type FlatNotesCache = HashMap<(usize, u64), FlatNotes>;

/// What a song pass is built from.
#[derive(Debug, Clone, Copy)]
pub struct SongContext<'a> {
    pub bank: &'a SampleBank,
    pub patterns: &'a [Pattern],
    /// What each track of each pattern plays, by pattern index (empty for
    /// patterns the song does not use).
    pub sounds: &'a [Vec<TrackSound>],
    pub scales: &'a Scales,
    pub synths: &'a SynthPresets,
    pub sample_rate: f64,
}

/// One strip per track name over the song's patterns, in song order
/// (`Mixer.strips` is keyed by track name): `(pattern, track)` of the first
/// track with each name.
pub fn song_strips(song: &Song, patterns: &[Pattern]) -> Vec<(usize, usize)> {
    let mut strips: Vec<(usize, usize)> = Vec::new();
    let mut seen = Vec::new();
    for clip in &song.clips {
        let Some(p) = patterns.iter().rposition(|p| p.id == clip.pattern) else { continue };
        if seen.contains(&p) {
            continue;
        }
        seen.push(p);
        for (t, track) in patterns[p].tracks.iter().enumerate() {
            if !strips.iter().any(|&(sp, st)| patterns[sp].tracks[st].name == track.name) {
                strips.push((p, t));
            }
        }
    }
    strips
}

/// Pass `measure` (measures since play started) of `song`: every clip
/// sounding during that measure at the song's tempo (`#playSongNotes`).
pub fn build_song_pass(
    song: &Song,
    measure: u64,
    rng: &mut Rng,
    cache: &mut FlatNotesCache,
    context: SongContext<'_>,
) -> Pass {
    let bpm = song.bpm.clamp(20.0, 300.0);
    let per_tick = frames_per_tick(bpm, context.sample_rate);
    let ticks = TICK * 4;
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let first_tick = measure * u64::from(ticks);
    let strips = song_strips(song, context.patterns);
    let strip_of = |p: usize, t: usize| {
        let name = &context.patterns[p].tracks[t].name;
        strips.iter().position(|&(sp, st)| context.patterns[sp].tracks[st].name == *name)
    };
    let silent = |p: usize, t: usize| {
        let tracks = &context.patterns[p].tracks;
        if tracks.iter().any(|t| t.solo) { !tracks[t].solo } else { tracks[t].mute }
    };

    let mut events = Vec::new();
    let mut patches = Vec::new();
    for t in 0..ticks {
        let tick = first_tick + u64::from(t);
        for source in song.active_clips_at(context.patterns, tick, TICK) {
            let p = source.pattern;
            let pattern = &context.patterns[p];
            let flat = cache.entry((p, source.loop_index)).or_insert_with(|| {
                recompute_flat_notes(pattern, u32::try_from(source.loop_index).unwrap_or(u32::MAX), rng, context.scales)
            });
            let Some(notes) = u32::try_from(source.local_tick).ok().and_then(|s| flat.get(&s)) else { continue };
            let sounds = context.sounds.get(p).map_or(&[][..], Vec::as_slice);
            let pass_context = PassContext {
                bank: context.bank,
                sounds,
                scales: context.scales,
                synths: context.synths,
                sample_rate: context.sample_rate,
                first_tick,
            };
            for note in notes {
                if silent(p, note.track) {
                    continue;
                }
                let (Some(strip), Some(assigned)) = (strip_of(p, note.track), sounds.get(note.track)) else { continue };
                let track = &pattern.tracks[note.track];
                let swing_ticks = note.swing_beats(track) * f64::from(TICK);
                let frame = (((f64::from(t) + swing_ticks) * per_tick).round() as u64).min(frames - 1);
                let voice = Voice { strip, lfo_tick: tick, bpm };
                if let Some(sound) = sound_for(pattern, note, assigned, voice, &pass_context, &mut patches) {
                    events.push(Event { frame, sound, mono: track.mono });
                }
            }
        }
    }
    events.sort_by_key(|e| e.frame);
    // Repetitions this pass is past are not needed again.
    cache.retain(|&(p, loop_index), _| {
        song.active_clips_at(context.patterns, first_tick + u64::from(ticks), TICK)
            .iter()
            .any(|s| s.pattern == p && s.loop_index <= loop_index)
    });

    let strip_settings_of = |&(p, t): &(usize, usize)| {
        let mut settings = strip_settings(&context.patterns[p].tracks[t], bpm);
        if silent(p, t) {
            settings.volume = 0.0;
        }
        settings
    };
    let lfos = strips
        .iter()
        .enumerate()
        .filter_map(|(i, &(p, t))| strip_lfo(i, &context.patterns[p].tracks[t], silent(p, t), first_tick, ticks))
        .collect();
    Pass {
        number: measure,
        first_tick,
        frames,
        ticks,
        events,
        strips: strips.iter().map(strip_settings_of).collect(),
        lfos,
        patches,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{BankSample, KitSample};
    use ordrumbox_core::Library;
    use ordrumbox_dsp::SampleBuffer;

    #[test]
    fn song_pass_layers_the_clips_of_each_measure() {
        let (library, _) = Library::parse(
            r#"{"patterns":[
                {"id":"a","name":"a","tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":2}]},{"name":"HAT","notes":[{"beat":1}]}]},
                {"id":"b","name":"b","beatCount":8,"tracks":[{"name":"KICK","notes":[{"beat":5}]},{"name":"SNARE","notes":[{"beat":6}]}]}],
              "songs":[{"id":"s","name":"s","bpm":120,"clips":[
                {"pattern":"a","startMeasure":0,"measureCount":2},{"pattern":"b","startMeasure":1,"measureCount":2}]}]}"#,
        )
        .unwrap();
        let instrument = KitSample {
            key: "X".into(),
            display_name: "x".into(),
            path: "x".into(),
            decay: Some(100.0),
            ..Default::default()
        };
        let bank = SampleBank {
            samples: vec![BankSample {
                kit: "k".into(),
                sample: instrument,
                buffer: SampleBuffer::mono(vec![1.0], 48_000).into(),
            }],
        };
        let sounds = vec![vec![TrackSound::Sample(0); 2]; 2];
        let (scales, synths) = (Scales::default(), SynthPresets::default());
        let context = SongContext {
            bank: &bank,
            patterns: &library.patterns,
            sounds: &sounds,
            scales: &scales,
            synths: &synths,
            sample_rate: 48_000.0,
        };
        let song = &library.songs[0];
        let strips = song_strips(song, &library.patterns);
        assert_eq!(strips, [(0, 0), (0, 1), (1, 1)], "KICK, HAT, SNARE: one strip per name");

        let mut cache = FlatNotesCache::new();
        let beat = 24_000; // frames per beat at 120 bpm
        let events = |measure: u64, cache: &mut FlatNotesCache| -> Vec<(u64, usize)> {
            let pass = build_song_pass(song, measure, &mut Rng::new(1), cache, context);
            assert_eq!((pass.frames, pass.first_tick, pass.strips.len()), (4 * beat, measure * 128, 3));
            pass.events.iter().map(|e| (e.frame / beat, e.sound.track())).collect()
        };
        assert_eq!(events(0, &mut cache), [(0, 0), (1, 1), (2, 0)]);
        // Measure 1: a again; b's first measure (beats 0–3) has nothing.
        assert_eq!(events(1, &mut cache), [(0, 0), (1, 1), (2, 0)]);
        // Measure 2: b's second measure: KICK on beat 5 → 1, SNARE on 6 → 2.
        assert_eq!(events(2, &mut cache), [(1, 0), (2, 2)]);
        // The song is 3 measures long: measure 3 is measure 0 again.
        assert_eq!(events(3, &mut cache), [(0, 0), (1, 1), (2, 0)]);
    }
}
