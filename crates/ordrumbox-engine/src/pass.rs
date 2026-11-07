//! One pass of a pattern, ready for the audio thread: every note as a voice
//! start at an exact frame, the strip settings of the tracks and their LFO
//! values tick by tick.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::song::clamp_bpm;
use ordrumbox_core::steps::TICK;
use std::collections::HashMap;

use ordrumbox_core::{Hit, Hits, Note, Pattern, Rng, Scales, Song, Track, expand_hits, lfo};
use ordrumbox_dsp::{StripSettings, SynthPatch, VoiceStart};

use crate::assign::TrackSound;
use crate::kit::SampleBank;
use crate::midi::{C3_KEY, KeyMap, MidiNote, Target, track_targets};
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
    /// The note's velocity (0–1), for the MIDI note sent with it.
    pub velocity: f32,
    /// The MIDI note sent with it, if any ([`crate::midi`]).
    pub note: Option<MidiNote>,
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
    /// How long a MIDI note sent lasts, in frames: a 16th.
    pub note_frames: u64,
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
    /// A swing on every track, on top of their own: where the second step
    /// of each pair starts ([`ordrumbox_core::track::STRAIGHT_SWING`]: none;
    /// a plugin host's).
    pub swing: f64,
}

/// How late `note` of `track` plays, in ticks: its track's swing, and the
/// pair swing `swing` set for every track.
fn swing_ticks(note: &Hit, track: &Track, swing: f64) -> f64 {
    (note.swing_beats(track) + note.pair_swing_beats(track, swing)) * f64::from(TICK)
}

/// Frames in one tick at `bpm`.
pub fn frames_per_tick(bpm: f64, sample_rate: f64) -> f64 {
    sample_rate * 60.0 / (bpm * f64::from(TICK))
}

/// Frames a MIDI note sent lasts at `per_tick` frames per tick: a 16th, at least one frame.
fn note_frames(per_tick: f64) -> u64 {
    (f64::from(TICK / 4) * per_tick).round().max(1.0) as u64
}

/// The key of a track's pitch 0 with `assigned`: its sample's root note, else C3.
fn root_key(assigned: Option<&TrackSound>, bank: &SampleBank) -> f64 {
    match assigned {
        Some(TrackSound::Sample(sample)) => bank.get(*sample).and_then(|s| s.sample.root_midi).unwrap_or(C3_KEY),
        _ => C3_KEY,
    }
}

/// The MIDI note `note` of `track` sends at transport tick `lfo_tick`, to
/// `target`: its pitch with the track's transpose and pitch LFO.
fn midi_note(target: Option<Target>, note: &Hit, track: &Track, lfo_tick: u64) -> Option<MidiNote> {
    let lfo = track.transpose_lfo.as_ref().map_or(0.0, |l| lfo::value(l, lfo_tick as f64));
    Some(target?.note(note.pitch + track.transpose + lfo))
}

/// Whether track `t` of `pattern` is silent: muted, or silenced by another
/// track's solo (`shouldTrackPlay()`).
fn is_silent(pattern: &Pattern, t: usize) -> bool {
    let tracks = &pattern.tracks;
    if tracks.iter().any(|t| t.solo) { !tracks[t].solo } else { tracks[t].mute }
}

/// Ticks in one song pass: a measure of four beats.
pub const SONG_PASS_TICKS: u32 = TICK * 4;

/// Ticks in one pass of `pattern`: the whole pattern, at least one.
pub fn pattern_pass_ticks(pattern: &Pattern) -> u32 {
    u32::try_from(ordrumbox_core::pattern_engine::pattern_ticks(pattern).max(1)).unwrap_or(u32::MAX)
}

/// Build pass `number` of `pattern` at tempo `bpm` (the library's, or a
/// plugin host's). Notes of muted tracks (or of tracks another track's solo
/// silences) are left out, as `shouldTrackPlay()` does.
pub fn build_pass(pattern: &Pattern, number: u64, rng: &mut Rng, context: PassContext<'_>, bpm: f64) -> Pass {
    let per_tick = frames_per_tick(bpm, context.sample_rate);
    let ticks = pattern_pass_ticks(pattern);
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let pass_index = u32::try_from(number).unwrap_or(u32::MAX);
    let flat = expand_hits(pattern, pass_index, rng, context.scales);
    let silent = |i: usize| is_silent(pattern, i);
    let targets = track_targets(
        pattern
            .tracks
            .iter()
            .enumerate()
            .map(|(t, track)| (track.name.as_str(), root_key(context.sounds.get(t), context.bank))),
    );

    let mut events = Vec::new();
    let mut patches = Vec::new();
    for (&tick, notes) in &flat {
        for note in notes {
            if silent(note.track) {
                continue;
            }
            let track = &pattern.tracks[note.track];
            // A note swung past the end still plays, on the last frame.
            let frame = (((f64::from(tick) + swing_ticks(note, track, context.swing)) * per_tick).round() as u64)
                .min(frames - 1);
            let lfo_tick = context.first_tick + u64::from(tick);
            let Some(assigned) = context.sounds.get(note.track) else { continue };
            let voice = Voice { strip: note.track, lfo_tick, bpm };
            if let Some(sound) = sound_for(pattern, note, assigned, voice, &context, &mut patches) {
                let midi = midi_note(targets.get(note.track).copied().flatten(), note, track, lfo_tick);
                events.push(Event { frame, sound, mono: track.mono, velocity: note.velocity as f32, note: midi });
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
    Pass {
        number,
        first_tick: context.first_tick,
        frames,
        ticks,
        events,
        strips,
        lfos,
        patches,
        note_frames: note_frames(per_tick),
    }
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
    note: &Hit,
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

/// A one-note pass to audition track `t` at tempo `bpm`: `note` as written,
/// or, without one, a note at the track's velocity and pan. Its strips are
/// the pattern's.
pub fn build_preview(pattern: &Pattern, t: usize, note: Option<&Note>, context: PassContext<'_>, bpm: f64) -> Pass {
    let Some(track) = pattern.tracks.get(t) else { return Pass::default() };
    let flat = Hit {
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
        .map(|sound| Event { frame: 0, sound, mono: track.mono, velocity: 1.0, note: None })
        .into_iter()
        .collect();
    let strips = pattern.tracks.iter().map(|t| strip_settings(t, bpm)).collect();
    Pass { frames: 1, ticks: 1, events, strips, patches, ..Pass::default() }
}

/// What each MIDI key plays live in `pattern` at tempo `bpm`: every track
/// at full velocity and its own pan, through the pattern's strips (silent
/// where the track is).
pub fn build_keymap(pattern: &Pattern, context: PassContext<'_>, bpm: f64) -> KeyMap {
    let mut patches = Vec::new();
    let mut events = Vec::new();
    for (t, track) in pattern.tracks.iter().enumerate() {
        let hit = Hit { track: t, source: None, beat: 0, step: 0, pitch: 0.0, velocity: 1.0, pan: track.pan };
        let voice = Voice { strip: t, lfo_tick: context.first_tick, bpm };
        let sound = context.sounds.get(t).and_then(|a| sound_for(pattern, &hit, a, voice, &context, &mut patches));
        if let Some(sound) = sound {
            events.push(Event { frame: 0, sound, mono: track.mono, velocity: 1.0, note: None });
        }
    }
    let strips = (0..pattern.tracks.len())
        .map(|t| {
            let mut settings = strip_settings(&pattern.tracks[t], bpm);
            if is_silent(pattern, t) {
                settings.volume = 0.0;
            }
            settings
        })
        .collect();
    // The events play pitch 0 with the track's transpose: so does their key.
    let targets: Vec<Option<Target>> = track_targets(
        pattern
            .tracks
            .iter()
            .enumerate()
            .map(|(t, track)| (track.name.as_str(), root_key(context.sounds.get(t), context.bank))),
    )
    .into_iter()
    .zip(&pattern.tracks)
    .map(|(target, track)| target.map(|target| target.transposed(track.transpose)))
    .collect();
    KeyMap::new(Pass { frames: 1, ticks: 1, events, strips, patches, ..Pass::default() }, &targets)
}

/// Hits of a pattern repetition, kept between song passes so a
/// pattern longer than a measure is drawn once per repetition.
pub type HitsCache = HashMap<(usize, u64), Hits>;

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
    /// A swing on every track ([`PassContext::swing`]).
    pub swing: f64,
}

/// One strip per track name over the song's patterns, in song order
/// (`Mixer.strips` is keyed by track name): `(pattern, track)` of the first
/// track with each name.
pub fn song_strips(song: &Song, patterns: &[Pattern]) -> Vec<(usize, usize)> {
    let mut strips: Vec<(usize, usize)> = Vec::new();
    let mut seen = Vec::new();
    for clip in &song.clips {
        let Some(p) = patterns.iter().rposition(|p| p.id == clip.pattern_id) else { continue };
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
/// sounding during that measure at the song's tempo.
pub fn build_song_pass(
    song: &Song,
    measure: u64,
    rng: &mut Rng,
    cache: &mut HitsCache,
    context: SongContext<'_>,
) -> Pass {
    build_song_pass_at(song, measure, rng, cache, context, clamp_bpm(song.bpm))
}

/// [`build_song_pass`] at tempo `bpm` instead of the song's (a plugin host's tempo).
pub fn build_song_pass_at(
    song: &Song,
    measure: u64,
    rng: &mut Rng,
    cache: &mut HitsCache,
    context: SongContext<'_>,
    bpm: f64,
) -> Pass {
    let per_tick = frames_per_tick(bpm, context.sample_rate);
    let ticks = SONG_PASS_TICKS;
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let first_tick = measure * u64::from(ticks);
    let strips = song_strips(song, context.patterns);
    let strip_of = |p: usize, t: usize| {
        let name = &context.patterns[p].tracks[t].name;
        strips.iter().position(|&(sp, st)| context.patterns[sp].tracks[st].name == *name)
    };
    let silent = |p: usize, t: usize| is_silent(&context.patterns[p], t);
    let targets = track_targets(strips.iter().map(|&(p, t)| {
        let sounds = context.sounds.get(p).map_or(&[][..], Vec::as_slice);
        (context.patterns[p].tracks[t].name.as_str(), root_key(sounds.get(t), context.bank))
    }));

    let mut events = Vec::new();
    let mut patches = Vec::new();
    for t in 0..ticks {
        let tick = first_tick + u64::from(t);
        for source in song.active_clips_at(context.patterns, tick, TICK) {
            let p = source.pattern;
            let pattern = &context.patterns[p];
            let flat = cache.entry((p, source.loop_index)).or_insert_with(|| {
                expand_hits(pattern, u32::try_from(source.loop_index).unwrap_or(u32::MAX), rng, context.scales)
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
                swing: context.swing,
            };
            for note in notes {
                if silent(p, note.track) {
                    continue;
                }
                let (Some(strip), Some(assigned)) = (strip_of(p, note.track), sounds.get(note.track)) else { continue };
                let track = &pattern.tracks[note.track];
                let frame = (((f64::from(t) + swing_ticks(note, track, context.swing)) * per_tick).round() as u64)
                    .min(frames - 1);
                let voice = Voice { strip, lfo_tick: tick, bpm };
                if let Some(sound) = sound_for(pattern, note, assigned, voice, &pass_context, &mut patches) {
                    let midi = midi_note(targets.get(strip).copied().flatten(), note, track, tick);
                    events.push(Event { frame, sound, mono: track.mono, velocity: note.velocity as f32, note: midi });
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
        note_frames: note_frames(per_tick),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::{BankSample, KitSample};
    use ordrumbox_core::Library;
    use ordrumbox_core::track::STRAIGHT_SWING;
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
            swing: STRAIGHT_SWING,
        };
        let song = &library.songs[0];
        let strips = song_strips(song, &library.patterns);
        assert_eq!(strips, [(0, 0), (0, 1), (1, 1)], "KICK, HAT, SNARE: one strip per name");

        let mut cache = HitsCache::new();
        let beat = 24_000; // frames per beat at 120 bpm
        let events = |measure: u64, cache: &mut HitsCache| -> Vec<(u64, usize)> {
            let pass = build_song_pass(song, measure, &mut Rng::new(1), cache, context);
            assert_eq!(
                (pass.frames, pass.first_tick, pass.strips.len()),
                (4 * beat, measure * u64::from(SONG_PASS_TICKS), 3)
            );
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
