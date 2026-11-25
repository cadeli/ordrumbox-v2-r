//! One pass of a song: every clip sounding during a measure (or the part of
//! one a song loop plays), its notes shaped by the song's automation lanes,
//! one strip per track name.

use std::collections::HashMap;

use ordrumbox_core::automation;
use ordrumbox_core::song::clamp_bpm;
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Hit, Hits, Pattern, Rng, Scales, Song, expand_hits};
use ordrumbox_dsp::SynthPatch;

use super::keymap::key_strips;
use super::{
    Event, Pass, PassContext, SONG_PASS_TICKS, Voice, event_for, frames_per_tick, midi_note, note_frame, root_key,
    song_notes, sound_for, swing_ticks,
};
use crate::assign::TrackSound;
use crate::kit::SampleBank;
use crate::midi::{Target, track_targets};
use crate::synth_preset::SynthPresets;

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
}

/// One strip per track name over the song's patterns, in song order (the
/// mixer's strips are keyed by track name): `(pattern, track)` of the first
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
    let span = (measure * u64::from(SONG_PASS_TICKS), SONG_PASS_TICKS);
    build_song_pass_at(song, span, rng, cache, context, clamp_bpm(song.bpm))
}

/// The ticks `(first_tick, ticks)` of `song` as a pass, at tempo `bpm`: a
/// measure, or the part of one that a song loop plays.
pub fn build_song_pass_at(
    song: &Song,
    (first_tick, ticks): (u64, u32),
    rng: &mut Rng,
    cache: &mut HitsCache,
    context: SongContext<'_>,
    bpm: f64,
) -> Pass {
    let per_tick = frames_per_tick(bpm, context.sample_rate);
    let frames = (f64::from(ticks) * per_tick).round().max(1.0) as u64;
    let strips = song_strips(song, context.patterns);
    let targets = track_targets(strips.iter().map(|&(p, t)| {
        let sounds = context.sounds.get(p).map_or(&[][..], Vec::as_slice);
        (context.patterns[p].tracks[t].name.as_str(), root_key(sounds.get(t), context.bank))
    }));
    let shared = SongPassParts { song, context, strips: &strips, targets: &targets, per_tick, frames, first_tick, bpm };

    let mut events = Vec::new();
    let mut patches = Vec::new();
    for t in 0..ticks {
        let tick = first_tick + u64::from(t);
        for source in song.active_clips_at(context.patterns, tick, TICK) {
            let p = source.pattern;
            let pattern = &context.patterns[p];
            let flat = cache.entry((p, source.loop_index)).or_insert_with(|| {
                let pattern = song_notes::lane_pattern(&song.automations, pattern);
                expand_hits(&pattern, u32::try_from(source.loop_index).unwrap_or(u32::MAX), rng, context.scales)
            });
            let Some(notes) = u32::try_from(source.local_tick).ok().and_then(|s| flat.get(&s)) else { continue };
            shared.push_events(p, notes, t, rng, (&mut events, &mut patches));
        }
    }
    events.sort_by_key(|e| e.frame);
    // Repetitions this pass is past are not needed again.
    cache.retain(|&(p, loop_index), _| {
        song.active_clips_at(context.patterns, first_tick + u64::from(ticks), TICK)
            .iter()
            .any(|s| s.pattern == p && s.loop_index <= loop_index)
    });

    let span = super::song_strips::Span { first_tick, ticks, bpm };
    let (strip_settings, lfos) = super::song_strips::settings_and_lfos(song, context.patterns, &strips, span);
    Pass {
        number: first_tick / u64::from(SONG_PASS_TICKS),
        first_tick,
        frames,
        ticks,
        events,
        strips: strip_settings,
        keys: key_strips(strips.iter().map(|&(p, t)| &context.patterns[p].tracks[t])),
        lfos,
        patches,
        ..Pass::default()
    }
}

/// What the notes of a song pass share: the context, the strips (a pattern
/// and a track each), their MIDI targets, and the pass's timing.
struct SongPassParts<'a> {
    /// Its automation lanes silence notes step by step.
    song: &'a Song,
    context: SongContext<'a>,
    strips: &'a [(usize, usize)],
    targets: &'a [Option<Target>],
    per_tick: f64,
    frames: u64,
    first_tick: u64,
    bpm: f64,
}

impl SongPassParts<'_> {
    /// The strip that track `t` of pattern `p` plays on: the first one with its name.
    fn strip_of(&self, p: usize, t: usize) -> Option<usize> {
        let name = &self.context.patterns[p].tracks[t].name;
        self.strips.iter().position(|&(sp, st)| self.context.patterns[sp].tracks[st].name == *name)
    }

    /// The events of `notes`, which pattern `p` plays on tick `t` of the
    /// pass, and the synth patches they need; `rng` draws what the song's
    /// probability lanes leave to chance.
    fn push_events(
        &self,
        p: usize,
        notes: &[Hit],
        t: u32,
        rng: &mut Rng,
        (events, patches): (&mut Vec<Event>, &mut Vec<SynthPatch>),
    ) {
        let context = &self.context;
        let pattern = &context.patterns[p];
        let sounds = context.sounds.get(p).map_or(&[][..], Vec::as_slice);
        let pass_context = PassContext {
            bank: context.bank,
            sounds,
            scales: context.scales,
            synths: context.synths,
            sample_rate: context.sample_rate,
            first_tick: self.first_tick,
        };
        let tick = self.first_tick + u64::from(t);
        let step = self.song.automation_step_at(tick, TICK);
        for note in notes {
            if automation::is_silent(&pattern.tracks, note.track, &self.song.automations, step) {
                continue;
            }
            let (Some(strip), Some(assigned)) = (self.strip_of(p, note.track), sounds.get(note.track)) else {
                continue;
            };
            let track = &pattern.tracks[note.track];
            let lanes = song_notes::NoteLanes::at(&self.song.automations, track, step);
            if !lanes.plays(rng) {
                continue;
            }
            let note = &lanes.apply(note, track);
            let swing = swing_ticks(lanes.swing_beats(note, track));
            let frame = note_frame(t, swing, self.per_tick, self.frames);
            let voice = Voice { strip, lfo_tick: tick, bpm: self.bpm };
            if let Some(sound) = sound_for(pattern, note, assigned, voice, &pass_context, patches) {
                let midi = midi_note(self.targets.get(strip).copied().flatten(), note, track, tick);
                events.push(event_for(frame, sound, note, track, midi, self.per_tick));
            }
        }
    }
}
