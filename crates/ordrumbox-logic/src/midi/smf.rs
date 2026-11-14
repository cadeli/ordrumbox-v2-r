//! Reading Standard MIDI Files (`logic/midi/midi_parser.js`), with its
//! leniency: chunks are walked by their declared lengths, reads past the end
//! of the file give `None`, sysex events are skipped by their length, and only
//! meta, program and note events are known (any other status is read as a
//! two-byte event).

/// Bytes read past the end of a track the file does not hold: beyond this,
/// a declared length is taken as garbage (the JS would read on until it).
const MAX_PHANTOM_BYTES: usize = 64 * 1024;

/// The header chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub format: u16,
    pub track_count: u16,
    /// Ticks per quarter note.
    pub division: u16,
}

/// What an event is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    /// `FF type length data` (`type` missing at the end of the file).
    Meta { meta_type: Option<u8>, data: Vec<u8> },
    /// A one-data-byte event (`0xC0` program change, `0xD0` channel pressure).
    Program { status: u8, channel: u8, program: Option<u8> },
    /// Any other status, read as two data bytes (note on/off, controllers…).
    Note { status: u8, channel: u8, note: Option<u8>, velocity: u8 },
}

/// One event of a track, at its absolute tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub tick: i64,
    pub kind: EventKind,
}

/// A parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Midi {
    pub header: Header,
    pub tracks: Vec<Vec<Event>>,
    /// Each track's first name event (`FF 03`), as Latin-1, or empty.
    pub track_names: Vec<String>,
}

/// A note-on with a velocity and, when the file pairs it with a note-off,
/// how long it lasts in ticks (0: no note-off found).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteOn {
    pub tick: i64,
    pub channel: u8,
    pub note: Option<u8>,
    pub velocity: u8,
    pub track: usize,
    /// Ticks from the note-on to the matching note-off, 0 without one.
    pub duration: i64,
}

fn byte(bytes: &[u8], at: usize) -> Option<u8> {
    bytes.get(at).copied()
}

/// `decodeVLQ`: value and bytes read (a missing byte reads as 0 and ends it).
pub fn decode_vlq(bytes: &[u8], offset: usize) -> (u32, usize) {
    let (mut value, mut read) = (0u32, 0usize);
    loop {
        let b = byte(bytes, offset + read).unwrap_or(0);
        // Past 4 bytes (28 bits, the most a file may use) the high bits drop out.
        value = (value << 7) | u32::from(b & 0x7f);
        read += 1;
        if b & 0x80 == 0 {
            return (value, read);
        }
    }
}

/// `encodeVLQ` (non-negative values).
pub fn encode_vlq(mut value: u32) -> Vec<u8> {
    let mut bytes = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        bytes.insert(0, (value & 0x7f) as u8 | 0x80);
        value >>= 7;
    }
    bytes
}

fn read_u32(bytes: &[u8], at: usize) -> u32 {
    (0..4).fold(0u32, |v, i| (v << 8) | u32::from(byte(bytes, at + i).unwrap_or(0)))
}

fn read_u16(bytes: &[u8], at: usize) -> u16 {
    (u16::from(byte(bytes, at).unwrap_or(0)) << 8) | u16::from(byte(bytes, at + 1).unwrap_or(0))
}

/// `parseMTrkEvents`: the events of a track chunk's `length` bytes.
fn parse_track(bytes: &[u8], start: usize, length: usize) -> Vec<Event> {
    let end = (start + length).min(bytes.len().max(start) + MAX_PHANTOM_BYTES);
    let mut events = Vec::new();
    let (mut pos, mut tick, mut running) = (start, 0i64, 0u8);
    while pos < end {
        let (delta, read) = decode_vlq(bytes, pos);
        pos += read;
        tick += i64::from(delta);
        let mut status = byte(bytes, pos);
        if status.is_some_and(|b| b & 0x80 != 0) {
            running = status.unwrap_or(0);
            pos += 1;
        } else {
            status = Some(running);
        }
        let status = status.unwrap_or(0);
        if status == 0xff {
            let meta_type = byte(bytes, pos);
            let (len, read) = decode_vlq(bytes, pos + 1);
            let from = pos + 1 + read;
            let to = from.saturating_add(len as usize);
            let data = bytes.get(from.min(bytes.len())..to.min(bytes.len())).unwrap_or_default().to_vec();
            events.push(Event { tick, kind: EventKind::Meta { meta_type, data } });
            pos = to;
            running = 0;
        } else if status == 0xf0 || status == 0xf7 {
            // A sysex: `F0 length data` (or `F7 length data`, a continued or
            // escaped packet), skipped by its length. Like a meta event, it
            // cancels running status.
            let (len, read) = decode_vlq(bytes, pos);
            pos = (pos + read).saturating_add(len as usize);
            running = 0;
        } else {
            let (hi, channel) = (status & 0xf0, status & 0x0f);
            if hi == 0xc0 || hi == 0xd0 {
                let program = byte(bytes, pos);
                events.push(Event { tick, kind: EventKind::Program { status: hi, channel, program } });
                pos += 1;
            } else {
                let note = byte(bytes, pos);
                let velocity = byte(bytes, pos + 1).unwrap_or(0);
                events.push(Event { tick, kind: EventKind::Note { status: hi, channel, note, velocity } });
                pos += 2;
            }
        }
    }
    events
}

/// `parseMidi`: the header and the tracks, chunk by chunk.
pub fn parse(bytes: &[u8]) -> Midi {
    let mut header = Header { format: 0, track_count: 0, division: 96 };
    let mut tracks = Vec::new();
    let mut i = 0usize;
    while i + 8 <= bytes.len() {
        let tag = &bytes[i..i + 4];
        let length = read_u32(bytes, i + 4) as usize;
        if tag == b"MThd" {
            header = Header {
                format: read_u16(bytes, i + 8),
                track_count: read_u16(bytes, i + 10),
                division: read_u16(bytes, i + 12),
            };
        } else if tag == b"MTrk" {
            tracks.push(parse_track(bytes, i + 8, length));
        }
        i = i.saturating_add(8).saturating_add(length);
    }
    let track_names = tracks
        .iter()
        .map(|events: &Vec<Event>| {
            events
                .iter()
                .find_map(|e| match &e.kind {
                    EventKind::Meta { meta_type: Some(0x03), data } => Some(text(data)),
                    _ => None,
                })
                .unwrap_or_default()
        })
        .collect();
    Midi { header, tracks, track_names }
}

/// A text event: UTF-8, as this app writes it, else Latin-1, as older files
/// often are.
fn text(data: &[u8]) -> String {
    match std::str::from_utf8(data) {
        Ok(text) => text.to_owned(),
        Err(_) => data.iter().map(|&b| char::from(b)).collect(),
    }
}

impl Midi {
    /// The tracks that hold the music: all of them, but track 0 in a format 1
    /// file (its tempo track).
    fn music_tracks(&self) -> impl Iterator<Item = (usize, &Vec<Event>)> {
        self.tracks.iter().enumerate().skip(usize::from(self.header.format == 1))
    }

    /// The note-ons with a velocity, of the music tracks, each with the
    /// duration its matching note-off gives (first in first out on its key:
    /// an off ends the note struck longest ago; no off, no duration).
    pub fn note_ons(&self) -> Vec<NoteOn> {
        let mut notes = Vec::new();
        for (track, events) in self.music_tracks() {
            // The note-ons of each key still waiting for their note-off.
            let mut open: Vec<Vec<(usize, i64)>> = vec![Vec::new(); 16 * 128];
            for event in events {
                let EventKind::Note { status, channel, note, velocity } = event.kind else { continue };
                let Some(note) = note else { continue };
                let slot = usize::from(channel) * 128 + usize::from(note & 0x7f);
                let is_off = status == 0x80 || (status == 0x90 && velocity == 0);
                if status == 0x90 && velocity > 0 {
                    notes.push(NoteOn { tick: event.tick, channel, note: Some(note), velocity, track, duration: 0 });
                    open[slot].push((notes.len() - 1, event.tick));
                } else if is_off && let Some(&(index, on_tick)) = open[slot].first() {
                    open[slot].remove(0);
                    notes[index].duration = (event.tick - on_tick).max(0);
                }
            }
        }
        notes
    }

    /// The last program of each channel, in the order channels first got
    /// one, read from the music tracks.
    pub fn program_changes(&self) -> Vec<(u8, Option<u8>)> {
        let mut programs: Vec<(u8, Option<u8>)> = Vec::new();
        for (_, events) in self.music_tracks() {
            for event in events {
                if let EventKind::Program { status: 0xc0, channel, program } = event.kind {
                    match programs.iter_mut().find(|(c, _)| *c == channel) {
                        Some(slot) => slot.1 = program,
                        None => programs.push((channel, program)),
                    }
                }
            }
        }
        programs
    }
}

/// `midiVelocityToNormalized`: 0–127 to 0–1, 2 decimals.
pub fn velocity_to_unit(velocity: u8) -> f64 {
    ordrumbox_core::num::round2(f64::from(velocity) / 127.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_round_trip() {
        for v in [0, 1, 127, 128, 0x3fff, 0x4000, 0x0fff_ffff] {
            let bytes = encode_vlq(v);
            assert_eq!(decode_vlq(&bytes, 0), (v, bytes.len()));
        }
        assert_eq!(encode_vlq(0x80), [0x81, 0x00]);
        assert_eq!(decode_vlq(&[0x81], 0), (0x80, 2)); // the missing byte reads as 0
    }

    /// A file of one track of `events`, in `format`.
    fn file(format: u8, events: &[u8]) -> Vec<u8> {
        let mut bytes = b"MThd\0\0\0\x06\0".to_vec();
        bytes.extend([format, 0, 1, 0, 96]);
        bytes.extend(b"MTrk");
        bytes.extend(u32::try_from(events.len()).unwrap().to_be_bytes());
        bytes.extend(events);
        bytes
    }

    #[test]
    fn a_sysex_is_skipped_by_its_length() {
        // GM reset (F0 05 7E 7F 09 01 F7), then a kick at tick 0 and an
        // escaped packet (F7 02 xx xx) before a snare at tick 96.
        let bytes = file(
            0,
            &[
                0x00, 0xf0, 0x05, 0x7e, 0x7f, 0x09, 0x01, 0xf7, //
                0x00, 0x99, 36, 100, //
                0x00, 0xf7, 0x02, 0x12, 0x34, //
                0x60, 0x99, 38, 90, //
                0x00, 0xff, 0x2f, 0x00,
            ],
        );
        let notes: Vec<_> = parse(&bytes).note_ons().iter().map(|n| (n.tick, n.note)).collect();
        assert_eq!(notes, [(0, Some(36)), (96, Some(38))]);
    }

    #[test]
    fn the_programs_of_a_format_0_file_are_read() {
        let events = [0x00, 0xc1, 33, 0x00, 0x91, 40, 100, 0x00, 0xff, 0x2f, 0x00];
        assert_eq!(parse(&file(0, &events)).program_changes(), [(1, Some(33))]);
    }
}
