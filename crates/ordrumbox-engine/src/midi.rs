//! Live MIDI notes, in and out, one key per track: the key a track's notes
//! are sent on is the key that plays it. A drum track has its instrument's
//! GM key; the other tracks (melodic, unknown names, a second track of one
//! drum) take the spare keys, from [`FIRST_SPARE_KEY`] up, in track order.
//!
//! Built on the UI side (it allocates); the audio thread only reads it.

use ordrumbox_core::instruments::catalog;

use crate::pass::Pass;

/// Keys a MIDI note can have.
pub const KEY_COUNT: usize = 128;
/// The channel notes are sent on (channel 10, the GM drum channel).
pub const DRUM_CHANNEL: u8 = 9;
/// The first key given to a track with no GM drum key: above the GM drum
/// keys (35–81), so that none takes the place of a drum.
pub const FIRST_SPARE_KEY: u8 = 84;

/// The GM drum key of a track named `name`, if its instrument is a drum.
fn drum_key(name: &str) -> Option<u8> {
    let instrument = catalog().by_name(&name.trim().to_uppercase()).filter(|i| i.drum)?;
    instrument.midi.iter().find_map(|m| u8::try_from(m.key?).ok()).filter(|&k| usize::from(k) < KEY_COUNT)
}

/// The key of each track named in `names`, in order; `None` once every key is taken.
pub fn track_keys<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<Option<u8>> {
    let mut taken = [false; KEY_COUNT];
    let mut keys: Vec<Option<u8>> = names
        .into_iter()
        .map(|name| {
            let key = drum_key(name).filter(|&k| !taken[usize::from(k)])?;
            taken[usize::from(key)] = true;
            Some(key)
        })
        .collect();
    let mut spare = (FIRST_SPARE_KEY..=127).chain(0..FIRST_SPARE_KEY).filter(|&k| !taken[usize::from(k)]);
    for key in keys.iter_mut().filter(|k| k.is_none()) {
        *key = spare.next();
    }
    keys
}

/// What each key plays live: the tracks of a pattern, one event each at
/// full velocity (the note's velocity scales it), through the pattern's strips.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct KeyMap {
    /// One event per track that plays something; its strips are the pattern's.
    pub pass: Pass,
    /// The event of [`Self::pass`] each key starts.
    pub events: Vec<Option<usize>>,
}

impl KeyMap {
    /// The keymap of `pass`'s events, for tracks keyed by `keys`.
    pub fn new(pass: Pass, keys: &[Option<u8>]) -> Self {
        let mut events = vec![None; KEY_COUNT];
        for (index, event) in pass.events.iter().enumerate() {
            if let Some(&Some(key)) = keys.get(event.sound.track()) {
                events[usize::from(key)] = Some(index);
            }
        }
        Self { pass, events }
    }

    /// The event key `key` starts.
    pub fn event(&self, key: u8) -> Option<usize> {
        self.events.get(usize::from(key)).copied().flatten()
    }
}

/// A note sent out, at an engine clock frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NoteOut {
    /// Frames rendered since the engine was made, when the note starts or ends.
    pub clock: u64,
    pub key: u8,
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

    #[test]
    fn drums_have_their_gm_key_and_the_others_spare_keys() {
        let keys = track_keys(["KICK", "SNARE", "BASS", "KICK", "nothing known"]);
        assert_eq!(keys[0], Some(36), "Bass Drum 1");
        assert_eq!(keys[1], Some(38), "Acoustic Snare");
        assert_eq!(&keys[2..], [Some(84), Some(85), Some(86)], "in track order, a second KICK too");
    }

    #[test]
    fn every_track_has_a_key_until_none_is_left() {
        let names = vec!["BASS"; KEY_COUNT + 1];
        let keys = track_keys(names.iter().copied());
        let mut given: Vec<u8> = keys[..KEY_COUNT].iter().map(|k| k.unwrap()).collect();
        given.sort_unstable();
        given.dedup();
        assert_eq!(given.len(), KEY_COUNT, "each key once");
        assert_eq!(keys[KEY_COUNT], None);
    }
}
