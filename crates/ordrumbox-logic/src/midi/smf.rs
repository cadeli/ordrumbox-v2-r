//! Reading Standard MIDI Files (`logic/midi/midi_parser.js`), with its
//! leniency: chunks are walked by their declared lengths, reads past the end
//! of the file give "nothing" (`undefined` in JS), and only meta, program and
//! note events are known (any other status is read as a two-byte event).

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

/// A note-on with a velocity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteOn {
    pub tick: i64,
    pub channel: u8,
    pub note: Option<u8>,
    pub velocity: u8,
    pub track: usize,
}

fn byte(bytes: &[u8], at: usize) -> Option<u8> {
    bytes.get(at).copied()
}

/// `decodeVLQ`: value and bytes read (a missing byte reads as 0 and ends it).
pub fn decode_vlq(bytes: &[u8], offset: usize) -> (i32, usize) {
    let (mut value, mut read) = (0i32, 0usize);
    loop {
        let b = byte(bytes, offset + read).unwrap_or(0);
        value = value.wrapping_shl(7) | i32::from(b & 0x7f);
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
            // `bytes.slice(from, from + len)` (a negative length takes nothing)
            let to = if len < 0 { from } else { from + len as usize };
            let data = bytes.get(from.min(bytes.len())..to.min(bytes.len())).unwrap_or_default().to_vec();
            events.push(Event { tick, kind: EventKind::Meta { meta_type, data } });
            // A negative length would step back (the JS can loop forever there): always move on.
            let next = pos as i64 + 1 + read as i64 + i64::from(len);
            pos = usize::try_from(next).unwrap_or(0).max(pos + 1);
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
                    EventKind::Meta { meta_type: Some(0x03), data } => {
                        Some(data.iter().map(|&b| char::from(b)).collect())
                    }
                    _ => None,
                })
                .unwrap_or_default()
        })
        .collect();
    Midi { header, tracks, track_names }
}

impl Midi {
    /// `findAllNotes`: the note-ons with a velocity, track 0 skipped in a
    /// format 1 file (its tempo track).
    pub fn note_ons(&self) -> Vec<NoteOn> {
        let first = usize::from(self.header.format == 1);
        let mut notes = Vec::new();
        for (track, events) in self.tracks.iter().enumerate().skip(first) {
            for event in events {
                if let EventKind::Note { status: 0x90, channel, note, velocity } = event.kind
                    && velocity > 0
                {
                    notes.push(NoteOn { tick: event.tick, channel, note, velocity, track });
                }
            }
        }
        notes
    }

    /// `extractProgramChanges`: the last program of each channel, in the
    /// order channels first got one; read from track 1 on, whatever the format.
    pub fn program_changes(&self) -> Vec<(u8, Option<u8>)> {
        let mut programs: Vec<(u8, Option<u8>)> = Vec::new();
        for events in self.tracks.iter().skip(1) {
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
    ordrumbox_core::js::round(f64::from(velocity) / 127.0 * 100.0) / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vlq_round_trip() {
        for v in [0, 1, 127, 128, 0x3fff, 0x4000, 0x0fff_ffff] {
            let bytes = encode_vlq(v);
            assert_eq!(decode_vlq(&bytes, 0), (v as i32, bytes.len()));
        }
        assert_eq!(encode_vlq(0x80), [0x81, 0x00]);
        assert_eq!(decode_vlq(&[0x81], 0), (0x80, 2)); // the missing byte reads as 0
    }
}
