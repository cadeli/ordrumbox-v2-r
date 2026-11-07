//! Live MIDI notes, in and out, one place per track: where a track's notes
//! are sent is where they play it from. A drum track is on the drum channel,
//! at its instrument's GM key, whatever the note's pitch. Any other track
//! (melodic, unknown names, a second track of one drum) has a channel of its
//! own, its notes on the key of their pitch: the track's pitch 0 is the
//! sample's root note (C3 without one, and for a synth). Past the 15 channels,
//! the tracks left take spare keys of the drum channel, from
//! [`FIRST_SPARE_KEY`] up.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::steps::semitone_ratio;

use crate::pass::Pass;

/// Keys a MIDI note can have.
pub const KEY_COUNT: usize = 128;
/// Channels a MIDI note can be on.
pub const CHANNEL_COUNT: usize = 16;
/// The GM drum channel (channel 10).
pub const DRUM_CHANNEL: u8 = 9;
/// The channels given to tracks that are not drums: all 16 but the drums'.
const MELODIC_CHANNELS: [u8; 15] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 11, 12, 13, 14, 15];
/// The first drum-channel key given to a track with no GM drum key and no
/// channel left: above the GM drum keys (35–81), so that none takes the place of a drum.
pub const FIRST_SPARE_KEY: u8 = 84;
/// The key of pitch 0 without a root note: C3, as MIDI files are read and written.
pub const C3_KEY: f64 = 48.0;

/// Where a track's notes go out, and come in from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Target {
    /// One key of the drum channel, whatever the pitch.
    Drum(u8),
    /// A channel of its own, a note on the key of its pitch above `root`
    /// (the key of pitch 0).
    Melodic { channel: u8, root: f64 },
}

/// A MIDI note's place: its channel and key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MidiNote {
    pub channel: u8,
    pub key: u8,
}

impl Target {
    /// Where a note `pitch` semitones above the track's pitch 0 goes: the
    /// nearest key, kept in the MIDI range.
    #[must_use]
    pub fn note(self, pitch: f64) -> MidiNote {
        match self {
            Self::Drum(key) => MidiNote { channel: DRUM_CHANNEL, key },
            Self::Melodic { channel, root } => MidiNote { channel, key: nearest_key(root + pitch) },
        }
    }

    /// The same target with its pitch 0 `semitones` higher (a track's transpose).
    #[must_use]
    pub fn transposed(self, semitones: f64) -> Self {
        match self {
            Self::Drum(_) => self,
            Self::Melodic { channel, root } => Self::Melodic { channel, root: root + semitones },
        }
    }
}

/// The key nearest to `key`, in 0–127 (NaN is 0).
fn nearest_key(key: f64) -> u8 {
    key.round().clamp(0.0, 127.0) as u8
}

/// The GM drum key of a track named `name`, if its instrument is a drum.
fn drum_key(name: &str) -> Option<u8> {
    let instrument = catalog().by_name(&name.trim().to_uppercase()).filter(|i| i.drum)?;
    instrument.midi.iter().find_map(|m| u8::try_from(m.key?).ok()).filter(|&k| usize::from(k) < KEY_COUNT)
}

/// The target of each track, given as its name and root note, in order;
/// `None` once every channel and key is taken.
pub fn track_targets<'a>(tracks: impl IntoIterator<Item = (&'a str, f64)>) -> Vec<Option<Target>> {
    let mut taken = [false; KEY_COUNT];
    let tracks: Vec<(Option<u8>, f64)> = tracks
        .into_iter()
        .map(|(name, root)| {
            let key = drum_key(name).filter(|&k| !taken[usize::from(k)]);
            if let Some(key) = key {
                taken[usize::from(key)] = true;
            }
            (key, root)
        })
        .collect();
    let mut channels = MELODIC_CHANNELS.iter();
    let mut spare = (FIRST_SPARE_KEY..=127).chain(0..FIRST_SPARE_KEY).filter(|&k| !taken[usize::from(k)]);
    tracks
        .into_iter()
        .map(|(key, root)| match key {
            Some(key) => Some(Target::Drum(key)),
            None => match channels.next() {
                Some(&channel) => Some(Target::Melodic { channel, root }),
                None => spare.next().map(Target::Drum),
            },
        })
        .collect()
}

/// What each MIDI note plays live: the tracks of a pattern, one event each
/// at full velocity and pitch 0 (the note's velocity scales it, its key
/// pitches it), through the pattern's strips.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyMap {
    /// One event per track that plays something; its strips are the pattern's.
    pub pass: Pass,
    /// The event each key of the drum channel starts.
    drums: Vec<Option<usize>>,
    /// The event each channel's notes start, and the key of its pitch.
    channels: [Option<(usize, f64)>; CHANNEL_COUNT],
}

impl KeyMap {
    /// The keymap of `pass`'s events, for tracks going to `targets` (their
    /// pitch 0 that of the events).
    pub fn new(pass: Pass, targets: &[Option<Target>]) -> Self {
        let mut drums = vec![None; KEY_COUNT];
        let mut channels = [None; CHANNEL_COUNT];
        for (index, event) in pass.events.iter().enumerate() {
            match targets.get(event.sound.track()).copied().flatten() {
                Some(Target::Drum(key)) => drums[usize::from(key)] = Some(index),
                Some(Target::Melodic { channel, root }) => channels[usize::from(channel)] = Some((index, root)),
                None => {}
            }
        }
        Self { pass, drums, channels }
    }

    /// The event key `key` on channel `channel` starts, and the pitch ratio
    /// it plays at. A channel no track has plays the drum keys.
    pub fn event(&self, channel: u8, key: u8) -> Option<(usize, f64)> {
        if let Some(&Some((event, root))) = self.channels.get(usize::from(channel)) {
            return Some((event, semitone_ratio(f64::from(key) - root)));
        }
        self.drums.get(usize::from(key)).copied().flatten().map(|event| (event, 1.0))
    }
}

/// A note sent out, at an engine clock frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOut {
    /// Frames rendered since the engine was made, when the note starts or ends.
    pub clock: u64,
    pub note: MidiNote,
    /// The velocity (0–1) of a note-on; `None` for a note-off.
    pub velocity: Option<f32>,
}

/// A note sent out, timed in the block it falls in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockNote {
    /// Frames from the start of the block.
    pub timing: u32,
    pub channel: u8,
    pub key: u8,
    /// The velocity (0–1) of a note-on; `None` for a note-off.
    pub velocity: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(names: &[&'static str]) -> Vec<Option<Target>> {
        track_targets(names.iter().map(|&n| (n, C3_KEY)))
    }

    #[test]
    fn drums_have_their_gm_key_and_the_others_a_channel() {
        let targets = track_targets([("KICK", 48.0), ("SNARE", 48.0), ("BASS", 60.0), ("KICK", 36.0), ("?", 48.0)]);
        assert_eq!(targets[0], Some(Target::Drum(36)), "Bass Drum 1");
        assert_eq!(targets[1], Some(Target::Drum(38)), "Acoustic Snare");
        assert_eq!(targets[2], Some(Target::Melodic { channel: 0, root: 60.0 }));
        assert_eq!(targets[3], Some(Target::Melodic { channel: 1, root: 36.0 }), "a second KICK too");
        assert_eq!(targets[4], Some(Target::Melodic { channel: 2, root: 48.0 }));
    }

    #[test]
    fn past_the_channels_tracks_take_spare_drum_keys_until_none_is_left() {
        let targets = named(&vec!["BASS"; MELODIC_CHANNELS.len() + KEY_COUNT + 1]);
        let (melodic, rest) = targets.split_at(MELODIC_CHANNELS.len());
        let channels: Vec<u8> = melodic
            .iter()
            .map(|t| match t {
                Some(Target::Melodic { channel, .. }) => *channel,
                _ => panic!("{t:?}"),
            })
            .collect();
        assert_eq!(channels, MELODIC_CHANNELS, "every channel but the drums'");
        assert_eq!(rest[0], Some(Target::Drum(FIRST_SPARE_KEY)));
        let mut keys: Vec<u8> = rest[..KEY_COUNT]
            .iter()
            .map(|t| match t {
                Some(Target::Drum(key)) => *key,
                _ => panic!("{t:?}"),
            })
            .collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), KEY_COUNT, "each key once");
        assert_eq!(rest[KEY_COUNT], None);
    }

    #[test]
    fn a_note_goes_to_the_key_of_its_pitch_and_a_drum_note_to_its_key() {
        let melodic = Target::Melodic { channel: 3, root: 48.0 };
        assert_eq!(melodic.note(7.0), MidiNote { channel: 3, key: 55 });
        assert_eq!(melodic.note(-0.4).key, 48, "the nearest key");
        assert_eq!(melodic.note(200.0).key, 127);
        assert_eq!(melodic.transposed(12.0).note(0.0).key, 60);
        assert_eq!(Target::Drum(38).note(5.0), MidiNote { channel: DRUM_CHANNEL, key: 38 });
    }
}
