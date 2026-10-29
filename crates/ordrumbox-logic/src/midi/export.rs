//! A pattern as a Standard MIDI File (`logic/midi/midi_exporter.js`): type 1,
//! 96 ticks per quarter note, a tempo track then one track per instrument,
//! the notes the pattern engine plays (loops, every/pos, retriggers,
//! arpeggios, euclidean fills), loop after loop.
//!
//! Drums go on channel 10 at their GM key, melodic instruments each on the
//! next channel at the sample's root note (else their mapping's key, else
//! C3); a name the catalog does not know is a bass drum. The velocity LFO
//! scales the velocity, the pitch LFO adds semitones; notes last a 16th.

use ordrumbox_core::instruments::catalog;
use ordrumbox_core::pattern_engine::expand_hits;
use ordrumbox_core::steps::TICK;
use ordrumbox_core::{Pattern, Rng, Scales, js, lfo};

use super::import::C3_MIDI_NOTE;
use super::smf::encode_vlq;

/// Ticks per quarter note.
pub const PPQN: u32 = 96;
const MIDI_RATIO: f64 = PPQN as f64 / TICK as f64;
const DRUM_CHANNEL: u8 = 9;
/// A 16th at 96 PPQN.
const NOTE_LENGTH: f64 = 24.0;
/// Bass Drum 1.
const DEFAULT_NOTE: f64 = 36.0;

/// `new Uint8Array([x])[0]`: truncated, modulo 256 (NaN is 0).
fn uint8(x: f64) -> u8 {
    if x.is_finite() { x.trunc().rem_euclid(256.0) as u8 } else { 0 }
}

/// `Math.min(max, Math.max(min, value))` (NaN stays NaN).
fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value.is_nan() { value } else { value.max(min).min(max) }
}

fn meta(delta: u32, kind: u8, data: &[u8]) -> Vec<u8> {
    let mut out = encode_vlq(delta);
    out.extend([0xff, kind]);
    out.extend(encode_vlq(data.len() as u32));
    out.extend(data);
    out
}

fn chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend((body.len() as u32).to_be_bytes());
    out.extend(body);
    out
}

/// `buildMTrk`: the events, then end of track.
fn track_chunk(mut events: Vec<u8>) -> Vec<u8> {
    events.extend([0x00, 0xff, 0x2f, 0x00]);
    chunk(b"MTrk", &events)
}

/// The bytes JS writes for a string: one per code point (its first UTF-16
/// unit, truncated to a byte).
fn text_bytes(text: &str) -> Vec<u8> {
    text.chars().map(|c| c.encode_utf16(&mut [0; 2])[0] as u8).collect()
}

/// Where a track name plays.
#[derive(Debug, Clone, Copy)]
struct Target {
    note: f64,
    channel: u8,
    program: Option<i64>,
}

/// Root note of the first sample whose key the track name contains.
fn root_note(roots: &[(String, f64)], name: &str) -> Option<f64> {
    let upper = name.trim().to_uppercase();
    roots.iter().find(|(key, _)| upper.contains(&key.to_uppercase())).map(|(_, root)| *root)
}

/// `assignChannels`: by name, in track order (a repeated melodic name takes
/// a channel each time and keeps the last).
fn assign(names: &[&str], roots: &[(String, f64)]) -> Vec<(String, Target)> {
    let c = catalog();
    let mut targets: Vec<(String, Target)> = Vec::new();
    let mut next_melodic = 0u8;
    for &name in names {
        let instrument = c.by_name(&name.trim().to_uppercase()).filter(|i| !i.midi.is_empty());
        let target = match instrument {
            Some(instrument) => {
                let mapping = &instrument.midi[0];
                let channel = if instrument.drum {
                    DRUM_CHANNEL
                } else {
                    let channel = next_melodic;
                    next_melodic = next_melodic.wrapping_add(1);
                    channel
                };
                let mut note = mapping.key.map_or(C3_MIDI_NOTE, |k| k as f64);
                if !instrument.drum
                    && let Some(root) = root_note(roots, name)
                {
                    note = root;
                }
                Target { note, channel, program: mapping.program }
            }
            None => Target { note: DEFAULT_NOTE, channel: DRUM_CHANNEL, program: None },
        };
        match targets.iter_mut().find(|(n, _)| n == name) {
            Some(slot) => slot.1 = target,
            None => targets.push((name.to_owned(), target)),
        }
    }
    targets
}

#[derive(Debug, Clone, Copy)]
struct NoteEvent {
    tick: f64,
    key: f64,
    velocity: f64,
}

/// `buildInstrumentTrackFromEvents`: name, program, then note on/off pairs.
fn instrument_track(name: &str, target: Target, mut events: Vec<NoteEvent>) -> Vec<u8> {
    js::sort_by(&mut events, |a, b| {
        let by_tick = a.tick - b.tick;
        if by_tick != 0.0 && !by_tick.is_nan() { by_tick } else { a.key - b.key }
    });
    // (tick, on, key, velocity)
    let mut raw: Vec<(f64, bool, f64, f64)> = Vec::with_capacity(events.len() * 2);
    for e in &events {
        raw.push((e.tick, true, e.key, e.velocity));
        raw.push((e.tick + NOTE_LENGTH, false, e.key, 0.0));
    }
    // The JS order: by tick, a note-off before anything (inconsistent for two
    // ons or two offs: the order is TimSort's own, see `js::sort_by`).
    js::sort_by(&mut raw, |a, b| {
        let by_tick = a.0 - b.0;
        if by_tick != 0.0 && !by_tick.is_nan() {
            by_tick
        } else if a.1 {
            1.0
        } else {
            -1.0
        }
    });
    let mut bytes = meta(0, 0x03, &text_bytes(name));
    if let Some(program) = target.program.filter(|p| (0..=127).contains(p)) {
        bytes.extend([0x00, 0xc0 | (target.channel & 0x0f), program as u8]);
    }
    let (on, off) = (0x90 | (target.channel & 0x0f), 0x80 | (target.channel & 0x0f));
    let mut cursor = 0.0;
    for (tick, is_on, key, velocity) in raw {
        bytes.extend(encode_vlq((tick - cursor) as u32));
        cursor = tick;
        if is_on {
            bytes.extend([on, uint8(key), uint8(velocity)]);
        } else {
            bytes.extend([off, uint8(key), 0]);
        }
    }
    track_chunk(bytes)
}

/// `MidiExporter.export`: pattern `pattern` played `loops` times. `roots`
/// are the samples' root notes (key, MIDI note); `rng` and `scales` feed
/// the pattern engine as when it plays.
pub fn export(pattern: &Pattern, loops: u32, roots: &[(String, f64)], rng: &mut Rng, scales: &Scales) -> Vec<u8> {
    let pattern_ticks = f64::from(TICK) * f64::from(pattern.beat_count);
    let any_solo = pattern.tracks.iter().any(|t| t.solo);
    let plays = |t: &ordrumbox_core::Track| if any_solo { t.solo } else { !t.mute };
    let names: Vec<&str> = pattern.tracks.iter().filter(|t| plays(t)).map(|t| t.name.as_str()).collect();
    let targets = assign(&names, roots);
    // One entry per name, in the order the names first come.
    let mut tracks: Vec<(String, Target, Vec<NoteEvent>)> = Vec::new();
    for name in &names {
        if !tracks.iter().any(|(n, _, _)| n == name) {
            let target = targets.iter().find(|(n, _)| n == name).map(|(_, t)| *t);
            let target = target.unwrap_or(Target { note: DEFAULT_NOTE, channel: DRUM_CHANNEL, program: None });
            tracks.push(((*name).to_owned(), target, Vec::new()));
        }
    }

    for pass in 0..loops {
        let flat = expand_hits(pattern, pass, rng, scales);
        let offset = f64::from(pass) * pattern_ticks * MIDI_RATIO;
        for (&tick, notes) in &flat {
            let midi_tick = f64::from(tick) * MIDI_RATIO + offset;
            for note in notes {
                let track = &pattern.tracks[note.track];
                let Some((_, target, events)) = tracks.iter_mut().find(|(n, _, _)| *n == track.name) else {
                    continue;
                };
                let mut velocity = note.velocity;
                if let Some(l) = &track.volume_lfo {
                    velocity = clamp(velocity * lfo::value(l, f64::from(tick)), 0.0, 1.0);
                }
                let mut pitch = note.pitch + track.transpose;
                if let Some(l) = &track.transpose_lfo {
                    pitch += lfo::value(l, f64::from(tick));
                }
                let key = clamp(target.note + pitch, 0.0, 127.0);
                events.push(NoteEvent { tick: midi_tick, key, velocity: js::round(velocity * 127.0) });
            }
        }
    }

    let chunks: Vec<Vec<u8>> = tracks
        .into_iter()
        .filter(|(_, _, events)| !events.is_empty())
        .map(|(name, target, events)| instrument_track(&name, target, events))
        .collect();

    // Format 1, the track count, the PPQN.
    let mut header = vec![0x00, 0x01];
    header.extend(((1 + chunks.len()) as u16).to_be_bytes());
    header.extend((PPQN as u16).to_be_bytes());
    let mut bytes = chunk(b"MThd", &header);
    let us = js::round(60_000_000.0 / pattern.bpm);
    let us = if us.is_finite() { us as i64 as u32 } else { 0 };
    let mut tempo = meta(0, 0x58, &[0x04, 0x02, 0x18, 0x08]);
    tempo.extend(meta(0, 0x51, &[(us >> 16) as u8, (us >> 8) as u8, us as u8]));
    tempo.extend(meta(0, 0x03, b"orDrumbox Pattern"));
    bytes.extend(track_chunk(tempo));
    for chunk in chunks {
        bytes.extend(chunk);
    }
    bytes
}
