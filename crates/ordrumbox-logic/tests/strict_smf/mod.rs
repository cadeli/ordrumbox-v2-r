//! A Standard MIDI File validator written from the spec (MIDI 1.0, "Standard
//! MIDI Files 1.0", RP-001) alone, apart from `smf::parse`: it shares no code
//! with the crate, so a misreading of the format there is not repeated here.
//!
//! It refuses anything the spec does not allow, and goes past its letter
//! where a file would still be ambiguous to play:
//! - the header: `MThd` of length 6, format 0, 1 or 2, one track for format
//!   0, at least one track, a division in ticks per quarter note above 0 or a
//!   SMPTE rate of 24, 25, 29 or 30 frames with ticks per frame above 0;
//! - the chunks: as many `MTrk` as the header states, each ending exactly
//!   where its length says, nothing after the last one; an alien chunk is
//!   refused rather than skipped;
//! - the events: deltas of at most 4 bytes, running status only after a
//!   channel message (sysex and meta events cancel it), data bytes under
//!   0x80, no system common or real-time status, sysex packets closed by
//!   0xf7 or continued by 0xf7 packets, with no other event between (so
//!   none open at the end of track);
//! - channel mode messages (controllers 120 to 127) with the values the spec
//!   gives them;
//! - the meta events: a type under 0x80, a length of at most 4 bytes, the
//!   types the spec defines only, each with its length and its range (tempo
//!   above 0, key signature in -7..=7, time signature parts above 0, a
//!   SMPTE offset that is a time), the sequence number and the SMPTE offset
//!   at tick 0 before any MIDI event, tempo, time signature and SMPTE offset
//!   on the first track of a format 1 file, one end of track, last;
//! - the notes (past the letter): on each track, a note-off of a key not
//!   sounding on its channel, and a key still sounding at the end of the
//!   track, are refused. A key struck again while it sounds is allowed, as
//!   MIDI 1.0 allows it: notes are counted per key, each note-off ending one.

/// An event: its absolute tick and what it is, its status made explicit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub tick: u64,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// A channel message: status (0x80..=0xef), then its 1 or 2 data bytes.
    Channel(Vec<u8>),
    /// A sysex packet (0xf0) or an escape or continuation (0xf7), its data.
    Sysex {
        status: u8,
        data: Vec<u8>,
    },
    Meta {
        meta_type: u8,
        data: Vec<u8>,
    },
}

/// A file the validator accepts.
#[derive(Debug)]
pub struct File {
    pub format: u16,
    pub division: u16,
    pub tracks: Vec<Vec<Event>>,
}

/// Bytes read in order, every read checked against the end.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(n).filter(|&end| end <= self.bytes.len());
        let end = end.ok_or_else(|| format!("cut in {what} at byte {}", self.at))?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn byte(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.at).copied()
    }

    fn u16(&mut self, what: &str) -> Result<u16, String> {
        let b = self.take(2, what)?;
        Ok(u16::from(b[0]) << 8 | u16::from(b[1]))
    }

    fn u32(&mut self, what: &str) -> Result<u32, String> {
        let b = self.take(4, what)?;
        Ok(b.iter().fold(0, |n, &b| n << 8 | u32::from(b)))
    }

    /// A variable-length quantity: at most 4 bytes, so at most 0x0fffffff.
    fn vlq(&mut self, what: &str) -> Result<u32, String> {
        let start = self.at;
        let mut value = 0u32;
        for _ in 0..4 {
            let b = self.byte(what)?;
            value = value << 7 | u32::from(b & 0x7f);
            if b < 0x80 {
                return Ok(value);
            }
        }
        Err(format!("{what} longer than 4 bytes at byte {start}"))
    }

    fn done(&self) -> bool {
        self.at == self.bytes.len()
    }
}

/// The data byte count of a channel message.
fn data_count(status: u8) -> usize {
    match status & 0xf0 {
        0xc0 | 0xd0 => 1,
        _ => 2,
    }
}

/// Checks a whole file; the first fault found is the error, with its place.
pub fn validate(bytes: &[u8]) -> Result<File, String> {
    let mut r = Reader { bytes, at: 0 };
    if r.take(4, "the header chunk type")? != b"MThd" {
        return Err("the file does not start with MThd".into());
    }
    let length = r.u32("the header length")?;
    if length != 6 {
        return Err(format!("a header of {length} bytes, not 6"));
    }
    let format = r.u16("the format")?;
    let track_count = r.u16("the track count")?;
    let division = r.u16("the division")?;
    if format > 2 {
        return Err(format!("format {format}"));
    }
    if track_count == 0 || (format == 0 && track_count != 1) {
        return Err(format!("format {format} with {track_count} tracks"));
    }
    if division & 0x8000 != 0 {
        let fps = -i16::from(division.to_be_bytes()[0] as i8);
        if ![24, 25, 29, 30].contains(&fps) || division & 0xff == 0 {
            return Err(format!("SMPTE division {division:#06x}"));
        }
    } else if division == 0 {
        return Err("a division of 0 ticks per quarter note".into());
    }
    let mut tracks = Vec::new();
    while !r.done() {
        let start = r.at;
        let chunk_type = r.take(4, "a chunk type")?;
        let length = r.u32("a chunk length")? as usize;
        if chunk_type != b"MTrk" {
            return Err(format!("chunk {chunk_type:x?} at byte {start}, not MTrk"));
        }
        let body = r.take(length, "a track chunk (its length runs past the file)")?;
        let index = tracks.len();
        let events = track(body, index, format).map_err(|e| format!("track {index} (byte {start}): {e}"))?;
        tracks.push(events);
    }
    if tracks.len() != usize::from(track_count) {
        return Err(format!("{} track chunks, {track_count} in the header", tracks.len()));
    }
    Ok(File { format, division, tracks })
}

/// The events of one track chunk.
fn track(body: &[u8], index: usize, format: u16) -> Result<Vec<Event>, String> {
    let mut r = Reader { bytes: body, at: 0 };
    let mut events = Vec::new();
    let mut tick = 0u64;
    let mut running: Option<u8> = None;
    let mut sysex_open = false;
    let mut ended = false;
    // A MIDI event or a sysex came: the sequence number and SMPTE offset are too late.
    let mut transmitted = false;
    // Per channel and key, the note-ons not yet ended.
    let mut sounding = [[0u32; 128]; 16];
    while !r.done() {
        if ended {
            return Err(format!("bytes after the end of track, at {}", r.at));
        }
        tick += u64::from(r.vlq("a delta time")?);
        let at = r.at;
        let first = r.peek().ok_or("cut before a status")?;
        let status = if first < 0x80 {
            running.ok_or_else(|| format!("data byte {first:#04x} at {at} with no running status"))?
        } else {
            r.at += 1;
            first
        };
        let kind = match status {
            0x80..=0xef => {
                if sysex_open {
                    return Err(format!("a channel message at {at} inside an open sysex"));
                }
                running = Some(status);
                transmitted = true;
                let mut message = vec![status];
                for _ in 0..data_count(status) {
                    let b = r.byte("a channel message")?;
                    if b >= 0x80 {
                        return Err(format!("data byte {b:#04x} in the message at {at}"));
                    }
                    message.push(b);
                }
                channel_message(&message, &mut sounding).map_err(|e| format!("{e}, at {at} (tick {tick})"))?;
                Kind::Channel(message)
            }
            0xf0 | 0xf7 => {
                running = None;
                transmitted = true;
                let length = r.vlq("a sysex length")? as usize;
                let data = r.take(length, "a sysex")?.to_vec();
                sysex_open = sysex(status, &data, sysex_open).map_err(|e| format!("{e}, at {at}"))?;
                Kind::Sysex { status, data }
            }
            0xff => {
                running = None;
                if sysex_open {
                    return Err(format!("a meta event at {at} inside an open sysex"));
                }
                let meta_type = r.byte("a meta type")?;
                let length = r.vlq("a meta length")? as usize;
                let data = r.take(length, "a meta event")?.to_vec();
                meta(meta_type, &data).map_err(|e| format!("{e}, at {at}"))?;
                let early = tick == 0 && !transmitted;
                if matches!(meta_type, 0x00 | 0x54) && !early {
                    return Err(format!("meta {meta_type:#04x} at {at} past tick 0 or after a MIDI event"));
                }
                if format == 1 && index > 0 && matches!(meta_type, 0x00 | 0x51 | 0x54 | 0x58) {
                    return Err(format!("meta {meta_type:#04x} at {at} off the first track of a format 1 file"));
                }
                ended = meta_type == 0x2f;
                Kind::Meta { meta_type, data }
            }
            _ => return Err(format!("status {status:#04x} at {at}: system common or real-time, not in a file")),
        };
        events.push(Event { tick, kind });
    }
    if !ended {
        return Err("no end of track".into());
    }
    if let Some((channel, key)) = (0..16).flat_map(|c| (0..128).map(move |k| (c, k))).find(|&(c, k)| sounding[c][k] > 0)
    {
        return Err(format!("key {key} still sounding on channel {channel} at the end of track"));
    }
    Ok(events)
}

/// A channel message's values, and the notes it starts or ends.
fn channel_message(message: &[u8], sounding: &mut [[u32; 128]; 16]) -> Result<(), String> {
    let channel = usize::from(message[0] & 0x0f);
    match message[0] & 0xf0 {
        0x90 if message[2] > 0 => sounding[channel][usize::from(message[1])] += 1,
        0x80 | 0x90 => {
            let key = &mut sounding[channel][usize::from(message[1])];
            if *key == 0 {
                return Err(format!("a note-off of key {} not sounding on channel {channel}", message[1]));
            }
            *key -= 1;
        }
        0xb0 => {
            let (controller, value) = (message[1], message[2]);
            let allowed = match controller {
                120 | 121 | 123 | 124 | 125 | 127 => value == 0,
                122 => value == 0 || value == 127,
                126 => value <= 16,
                _ => true,
            };
            if !allowed {
                return Err(format!("channel mode message {controller} with value {value}"));
            }
        }
        _ => {}
    }
    Ok(())
}

/// A sysex packet; whether a sysex is open after it.
fn sysex(status: u8, data: &[u8], open: bool) -> Result<bool, String> {
    if status == 0xf7 && !open {
        // An escape: any bytes, sent as they are.
        return Ok(false);
    }
    if status == 0xf0 && open {
        return Err("a sysex starting inside an open one".into());
    }
    let Some((&last, body)) = data.split_last() else {
        return Err("an empty sysex packet".into());
    };
    if let Some(b) = body.iter().find(|&&b| b >= 0x80) {
        return Err(format!("byte {b:#04x} inside a sysex"));
    }
    match last {
        0xf7 => Ok(false),
        0x00..=0x7f => Ok(true),
        _ => Err(format!("byte {last:#04x} ending a sysex packet")),
    }
}

/// A meta event's length and values, by its type.
fn meta(meta_type: u8, data: &[u8]) -> Result<(), String> {
    let length = |want: &[usize]| {
        if want.contains(&data.len()) {
            Ok(())
        } else {
            Err(format!("meta {meta_type:#04x} of {} bytes, not {want:?}", data.len()))
        }
    };
    match meta_type {
        0x00 => length(&[0, 2]),
        0x01..=0x0f | 0x7f => Ok(()),
        0x20 | 0x21 => {
            length(&[1])?;
            // A channel prefix names a channel, a port a data byte.
            let max = if meta_type == 0x20 { 15 } else { 127 };
            if data[0] <= max { Ok(()) } else { Err(format!("meta {meta_type:#04x} of value {}", data[0])) }
        }
        0x2f => length(&[0]),
        0x51 => {
            length(&[3])?;
            if data == [0, 0, 0] { Err("a tempo of 0 µs per quarter note".into()) } else { Ok(()) }
        }
        0x54 => {
            length(&[5])?;
            let fps = [24, 25, 29, 30][usize::from(data[0] >> 5 & 0x03)];
            let hours = data[0] & 0x1f;
            if data[0] & 0x80 != 0 || hours > 23 || data[1] > 59 || data[2] > 59 || data[3] >= fps || data[4] > 99 {
                Err(format!("SMPTE offset {data:x?}"))
            } else {
                Ok(())
            }
        }
        0x58 => {
            length(&[4])?;
            if data[0] == 0 || data[2] == 0 || data[3] == 0 { Err(format!("time signature {data:?}")) } else { Ok(()) }
        }
        0x59 => {
            length(&[2])?;
            if (-7..=7).contains(&(data[0] as i8)) && data[1] <= 1 {
                Ok(())
            } else {
                Err(format!("key signature {data:?}"))
            }
        }
        _ => Err(format!("meta type {meta_type:#04x}, undefined by SMF 1.0")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A file of `format` at 96 PPQN, of these track bodies; `track_count`
    /// in the header.
    fn smf(format: u16, track_count: u16, bodies: &[&[u8]]) -> Vec<u8> {
        let mut bytes = b"MThd\0\0\0\x06".to_vec();
        for n in [format, track_count, 96] {
            bytes.extend(n.to_be_bytes());
        }
        for body in bodies {
            bytes.extend(b"MTrk");
            bytes.extend((body.len() as u32).to_be_bytes());
            bytes.extend(*body);
        }
        bytes
    }

    const END: &[u8] = &[0x00, 0xff, 0x2f, 0x00];

    fn one(body: &[u8]) -> Vec<u8> {
        let mut body = body.to_vec();
        body.extend(END);
        smf(0, 1, &[&body])
    }

    fn refused(bytes: &[u8], wants: &str) {
        let error = validate(bytes).expect_err(wants);
        assert!(error.contains(wants), "{error:?} does not say {wants:?}");
    }

    #[test]
    fn a_well_formed_file_is_read() {
        // Running status, a velocity-0 note-off, a full sysex, a split one, an escape, meta events.
        let body = [
            0x00, 0xff, 0x00, 0x02, 0x00, 0x01, // sequence number
            0x00, 0xff, 0x03, 0x02, b'h', b'i', // name
            0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, // tempo
            0x00, 0xff, 0x58, 0x04, 0x04, 0x02, 0x18, 0x08, // time signature
            0x00, 0xff, 0x59, 0x02, 0xfd, 0x01, // key signature
            0x00, 0x99, 36, 100, 0x00, 38, 90, // two note-ons, running status
            0x81, 0x00, 36, 0, 0x00, 38, 0, // note-offs as note-ons at 0
            0x00, 0xf0, 0x03, 0x7e, 0x7f, 0xf7, // a sysex
            0x00, 0xf0, 0x02, 0x43, 0x12, 0x10, 0xf7, 0x02, 0x00, 0xf7, // in two packets
            0x00, 0xf7, 0x02, 0xf3, 0x01, // an escape
            0x00, 0xb0, 121, 0, 0x00, 0xc0, 5, 0x00, 0xe0, 0x00, 0x40,
        ];
        let file = validate(&one(&body)).unwrap();
        assert_eq!((file.format, file.division, file.tracks.len()), (0, 96, 1));
        let notes: Vec<_> =
            file.tracks[0].iter().filter(|e| matches!(&e.kind, Kind::Channel(m) if m[0] == 0x99)).collect();
        assert_eq!(notes.len(), 4);
        assert_eq!((notes[3].tick, &notes[3].kind), (128, &Kind::Channel(vec![0x99, 38, 0])));
        let sysex: Vec<(u8, usize)> = file.tracks[0]
            .iter()
            .filter_map(|e| match &e.kind {
                Kind::Sysex { status, data } => Some((*status, data.len())),
                _ => None,
            })
            .collect();
        assert_eq!(sysex, [(0xf0, 3), (0xf0, 2), (0xf7, 2), (0xf7, 2)]);
    }

    #[test]
    fn every_fault_is_refused() {
        let note = [0x00, 0x90, 60, 100, 0x18, 0x80, 60, 0];
        refused(b"MThd\0\0\0\x06\0\0\0\x01", "cut in the division");
        refused(&smf(0, 1, &[])[..13], "cut in the division");
        let mut long_header = smf(0, 1, &[END]);
        long_header[7] = 8;
        refused(&long_header, "header of 8 bytes");
        refused(&smf(3, 1, &[END]), "format 3");
        refused(&smf(0, 2, &[END, END]), "format 0 with 2 tracks");
        refused(&smf(1, 0, &[]), "format 1 with 0 tracks");
        refused(&smf(1, 2, &[END]), "1 track chunks, 2 in the header");
        refused(&smf(1, 1, &[END, END]), "2 track chunks, 1 in the header");
        let mut zero_division = smf(0, 1, &[END]);
        zero_division[12..14].copy_from_slice(&[0, 0]);
        refused(&zero_division, "division of 0");
        zero_division[12..14].copy_from_slice(&[0xe5, 40]);
        refused(&zero_division, "SMPTE division");
        let mut alien = smf(0, 1, &[END]);
        alien[14..18].copy_from_slice(b"MTrx");
        refused(&alien, "not MTrk");
        let mut past = smf(0, 1, &[END]);
        past[21] = 5;
        refused(&past, "runs past the file");
        refused(&smf(0, 1, &[&[0x00, 0x90, 60, 100]]), "no end of track");
        refused(&smf(0, 1, &[&[0x00, 0xff, 0x2f, 0x00, 0x00]]), "bytes after the end of track");
        refused(&one(&[0x80, 0x80, 0x80, 0x80, 0x00]), "longer than 4 bytes");
        refused(&one(&[0x00, 60, 100]), "no running status");
        refused(&one(&[0x00, 0x90, 60, 0x80]), "data byte 0x80");
        refused(&one(&[0x00, 0xf8]), "real-time");
        refused(&one(&[0x00, 0xf2, 0x00, 0x00]), "system common");
        refused(&one(&[0x00, 0xff, 0x51, 0x02, 0x07, 0xa1]), "of 2 bytes");
        refused(&one(&[0x00, 0xff, 0x51, 0x03, 0, 0, 0]), "tempo of 0");
        refused(&one(&[0x00, 0xff, 0x2f, 0x01, 0x00]), "of 1 bytes");
        refused(&one(&[0x00, 0xff, 0x59, 0x02, 0x08, 0x00]), "key signature");
        refused(&one(&[0x00, 0xff, 0x58, 0x04, 0x00, 0x02, 0x18, 0x08]), "time signature");
        refused(&one(&[0x00, 0xff, 0x54, 0x05, 0x18, 0, 0, 0, 0]), "SMPTE offset");
        refused(&one(&[0x00, 0xff, 0x60, 0x00]), "undefined");
        refused(&one(&[0x00, 0xff, 0x80, 0x00]), "undefined");
        refused(&one(&[0x00, 0xff, 0x01, 0x80, 0x80, 0x80, 0x80, 0x00]), "longer than 4 bytes");
        refused(&smf(0, 1, &[&[0x00, 0xff, 0x03, 0x05, b'a']]), "cut in a meta event");
        refused(&one(&[0x00, 0xff, 0x20, 0x01, 16]), "meta 0x20 of value 16");
        refused(&one(&[0x01, 0xff, 0x00, 0x00]), "past tick 0");
        refused(&one(&[0x00, 0xc0, 0x01, 0x00, 0xff, 0x54, 0x05, 0, 0, 0, 0, 0]), "after a MIDI event");
        let tempo = [0x00, 0xff, 0x51, 0x03, 0x07, 0xa1, 0x20, 0x00, 0xff, 0x2f, 0x00];
        refused(&smf(1, 2, &[END, &tempo]), "off the first track");
        assert!(validate(&smf(2, 2, &[END, &tempo])).is_ok(), "format 2 tracks have their own tempo");
        refused(&one(&[0x00, 0xb0, 123, 1]), "channel mode message 123");
        refused(&one(&[0x00, 0xf0, 0x02, 0x7e, 0x7f]), "a meta event at 6 inside an open sysex");
        refused(&one(&[0x00, 0xf0, 0x01, 0x7e, 0x00, 0x90, 60, 100]), "inside an open sysex");
        refused(&one(&[0x00, 0xf0, 0x03, 0x7e, 0x80, 0xf7]), "inside a sysex");
        refused(&one(&[0x00, 0xf0, 0x00]), "empty sysex");
        refused(&one(&[0x00, 0x90, 60, 100]), "still sounding");
        refused(&one(&[0x00, 0x80, 60, 0]), "not sounding");
        let twice = [0x00, 0x90, 60, 100, 0x0c, 0x90, 60, 90, 0x0c, 0x80, 60, 0, 0x0c, 0x80, 60, 0];
        assert!(validate(&one(&twice)).is_ok(), "a key struck again while it sounds");
        refused(&one(&twice[..12]), "still sounding");
        refused(&one(&[&twice[..], &[0x00, 0x80, 60, 0]].concat()), "not sounding");
        assert!(validate(&one(&note)).is_ok());
    }
}
