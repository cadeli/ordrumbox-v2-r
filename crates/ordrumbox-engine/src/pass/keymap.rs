//! A pattern's MIDI key map: which strip each track takes its sidechain key
//! from, the MIDI target of each track, and what each key plays live.

use ordrumbox_core::{Hit, Pattern, Track};

use super::{Event, Pass, PassContext, Voice, heard_strips, root_key, sound_for};
use crate::assign::TrackSound;
use crate::kit::SampleBank;
use crate::midi::{KeyMap, Target, track_targets};


/// The strip each of `tracks` takes its sidechain key from: the one named by
/// its `sidechain_source`, none when the name is empty, unknown, or its own.
/// A track is named by its instrument, so a key follows its track through a
/// reorder, a removal or another pattern of a song.
pub fn key_strips<'a>(tracks: impl Iterator<Item = &'a Track>) -> Vec<Option<usize>> {
    let tracks: Vec<&Track> = tracks.collect();
    tracks
        .iter()
        .enumerate()
        .map(|(i, track)| {
            if track.sidechain_source.is_empty() {
                return None;
            }
            tracks.iter().position(|key| key.name == track.sidechain_source).filter(|&key| key != i)
        })
        .collect()
}

/// The MIDI target of each track of `pattern` with the sounds `sounds`: where
/// its notes go out, and which keys play it live. The events of a key map play
/// pitch 0 with the track's transpose: so does their key.
pub fn keymap_targets(pattern: &Pattern, sounds: &[TrackSound], bank: &SampleBank) -> Vec<Option<Target>> {
    track_targets(
        pattern.tracks.iter().enumerate().map(|(t, track)| (track.name.as_str(), root_key(sounds.get(t), bank))),
    )
    .into_iter()
    .zip(&pattern.tracks)
    .map(|(target, track)| target.map(|target| target.transposed(track.transpose)))
    .collect()
}

/// What each MIDI key plays live in `pattern` at tempo `bpm`: every track
/// at full velocity and its own pan, through the pattern's strips (silent
/// where the track is).
pub fn build_keymap(pattern: &Pattern, context: PassContext<'_>, bpm: f64) -> KeyMap {
    let mut patches = Vec::new();
    let mut events = Vec::new();
    for (t, track) in pattern.tracks.iter().enumerate() {
        let hit = Hit {
            track: t,
            source: None,
            beat: 0,
            step: 0,
            pitch: 0.0,
            velocity: 1.0,
            pan: track.pan,
            length_steps: 1.0,
        };
        let voice = Voice { strip: t, lfo_tick: context.first_tick, bpm };
        let sound = context.sounds.get(t).and_then(|a| sound_for(pattern, &hit, a, voice, &context, &mut patches));
        if let Some(sound) = sound {
            events.push(Event {
                frame: 0,
                sound,
                mono: track.mono,
                choke_group: track.choke(),
                velocity: 1.0,
                note: None,
                note_frames: 0,
                lock: None,
            });
        }
    }
    let strips = heard_strips(pattern, bpm);
    let targets = keymap_targets(pattern, context.sounds, context.bank);
    let keys = key_strips(pattern.tracks.iter());
    KeyMap::new(Pass { frames: 1, ticks: 1, events, strips, keys, patches, ..Pass::default() }, &targets)
}
