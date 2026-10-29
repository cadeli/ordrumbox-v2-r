//! Patterns from a MIDI file (`logic/services/midi_import_service.js`).
//!
//! Notes are grouped by channel, and each channel becomes tracks:
//! 1. a melodic channel whose program names a melodic instrument: one track
//!    of it, pitches relative to the sample's root note (C3 by default);
//! 2. otherwise, per note number, the drum instrument of that note (the
//!    catalog's MIDI mapping, else its GM drum name): one track each, pitch 0;
//! 3. otherwise the melodic instrument the track name names;
//! 4. otherwise the first catalog instrument not used yet.
//!
//! The JS rules are kept as they are: an instrument already made a track
//! drops the later notes that resolve to it; the tempo meta event is never
//! read (120 BPM); every pattern is 32 beats long, up to 16 patterns.

use ordrumbox_core::instruments::{Catalog, Instrument, catalog};
use ordrumbox_core::js;
use ordrumbox_core::{Note, Pattern, Track};

use super::smf::{self, NoteOn};

/// `C3_MIDI_NOTE`: the MIDI note of pitch 0.
pub const C3_MIDI_NOTE: f64 = 48.0;
/// `MIDI_MAX_BEATS`, `MIDI_MAX_PATTERNS`.
const PATTERN_BEATS: i32 = 32;
const MAX_PATTERNS: f64 = 16.0;
const TICK: f64 = ordrumbox_core::steps::TICK as f64;

/// What an import made.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    /// New patterns (no id yet), named after the file.
    pub patterns: Vec<Pattern>,
    pub track_count: usize,
    /// "MIDI imported: …".
    pub message: String,
}

/// Root note of the first sample whose key the track name contains.
fn root_note(roots: &[(String, f64)], name: &str) -> Option<f64> {
    let upper = name.trim().to_uppercase();
    roots.iter().find(|(key, _)| upper.contains(&key.to_uppercase())).map(|(_, root)| *root)
}

#[derive(Debug, Clone)]
struct TrackDef {
    name: String,
    notes: Vec<NoteOn>,
    base_note: f64,
}

fn found(instrument: Option<&Instrument>) -> Option<&Instrument> {
    instrument.filter(|i| i.id != ordrumbox_core::instruments::NOT_FOUND)
}

/// `#resolveDrumTrack`: one track per instrument the notes resolve to.
fn drum_tracks(c: &Catalog, notes: &[NoteOn], channel: u8) -> Option<Vec<TrackDef>> {
    let mut groups: Vec<(Option<u8>, Vec<NoteOn>)> = Vec::new();
    for note in notes {
        match groups.iter_mut().find(|(n, _)| *n == note.note) {
            Some((_, group)) => group.push(*note),
            None => groups.push((note.note, vec![*note])),
        }
    }
    let mut defs: Vec<TrackDef> = Vec::new();
    for (key, group) in groups {
        let Some(key) = key else { continue };
        let by_mapping = found(c.from_midi(i64::from(channel), i64::from(key)));
        let instrument = by_mapping.or_else(|| {
            let (_, gm) = c.drum_names.iter().find(|(k, _)| *k == i64::from(key))?;
            found(c.from_file_name(gm))
        });
        let Some(instrument) = instrument else { continue };
        if !defs.iter().any(|d| d.name == instrument.id) {
            defs.push(TrackDef { name: instrument.id.clone(), notes: group, base_note: f64::from(key) });
        }
    }
    (!defs.is_empty()).then_some(defs)
}

/// `importFile`: the patterns of `bytes`, a MIDI file named `file_name`;
/// `Err` with the reason when nothing can be imported.
pub fn import(file_name: &str, bytes: &[u8], roots: &[(String, f64)]) -> Result<Imported, String> {
    let midi = smf::parse(bytes);
    let notes = midi.note_ons();
    if notes.is_empty() {
        return Err("No MIDI notes found in file".into());
    }
    let programs = midi.program_changes();
    let c = catalog();

    // Notes by channel, in the order channels first play; a channel's name is
    // the name of the track its first note is in.
    let mut channels: Vec<(u8, Vec<NoteOn>, String)> = Vec::new();
    for note in &notes {
        match channels.iter_mut().find(|(ch, _, _)| *ch == note.channel) {
            Some((_, list, _)) => list.push(*note),
            None => {
                let name = midi.track_names.get(note.track).cloned().unwrap_or_default();
                channels.push((note.channel, vec![*note], name));
            }
        }
    }

    let root_or_c3 = |name: &str| root_note(roots, name).unwrap_or(C3_MIDI_NOTE);
    let mut defs: Vec<TrackDef> = Vec::new();
    let mut skipped: Vec<u8> = Vec::new();
    for (channel, list, track_name) in &channels {
        let program = programs.iter().find(|(ch, _)| ch == channel).and_then(|(_, p)| *p).unwrap_or(0);
        if *channel != 9 {
            if let Some(instrument) = found(c.from_midi_program(i64::from(program))).filter(|i| !i.drum) {
                if !defs.iter().any(|d| d.name == instrument.id) {
                    let base_note = root_or_c3(&instrument.id);
                    defs.push(TrackDef { name: instrument.id.clone(), notes: list.clone(), base_note });
                }
                continue;
            }
        }
        if let Some(drums) = drum_tracks(c, list, *channel) {
            defs.extend(drums);
            continue;
        }
        if !track_name.is_empty()
            && let Some(instrument) = found(c.by_name(track_name)).filter(|i| !i.drum)
        {
            if !defs.iter().any(|d| d.name == instrument.id) {
                let base_note = root_or_c3(&instrument.id);
                defs.push(TrackDef { name: instrument.id.clone(), notes: list.clone(), base_note });
            }
            continue;
        }
        skipped.push(*channel);
    }
    let mut ids: Vec<&str> = c.instruments.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    for channel in skipped {
        let Some(id) = ids.iter().find(|id| !defs.iter().any(|d| d.name == **id)) else { continue };
        let list = channels.iter().find(|(ch, _, _)| *ch == channel).map(|(_, l, _)| l.clone()).unwrap_or_default();
        defs.push(TrackDef { name: (*id).to_owned(), notes: list, base_note: root_or_c3(id) });
    }
    if defs.is_empty() {
        return Err("No matching instruments found in MIDI file".into());
    }

    let base_name = strip_midi_extension(file_name);
    let patterns = patterns_from(&defs, base_name, f64::from(midi.header.division));
    let message = if patterns.len() > 1 {
        format!("MIDI imported: {} track(s) into {} patterns", defs.len(), patterns.len())
    } else {
        format!("MIDI imported: {} track(s)", defs.len())
    };
    Ok(Imported { patterns, track_count: defs.len(), message })
}

/// `file.name.replace(/\.midi?$/i, '')`.
fn strip_midi_extension(name: &str) -> &str {
    let lower = name.to_ascii_lowercase();
    for ext in [".midi", ".mid"] {
        if lower.ends_with(ext) && name.is_char_boundary(name.len() - ext.len()) {
            return &name[..name.len() - ext.len()];
        }
    }
    name
}

/// `#createPatternsFromTrackDefs`: 32-beat patterns covering the notes, every
/// track in each of them.
fn patterns_from(defs: &[TrackDef], base_name: &str, division: f64) -> Vec<Pattern> {
    let ratio = division / TICK;
    let max_tick = defs.iter().flat_map(|d| &d.notes).map(|n| n.tick).fold(0, i64::max);
    let total_ticks = js::round(max_tick as f64 / ratio);
    // `Math.max`/`Math.min` keep a NaN (a 0 division): then no pattern.
    let total_beats = (total_ticks / TICK).ceil();
    let wanted = (total_beats.max(1.0) / f64::from(PATTERN_BEATS)).ceil();
    let count = if total_beats.is_nan() { 0 } else { MAX_PATTERNS.min(wanted) as usize };
    (0..count)
        .map(|p| {
            let start_beat = (p as i32) * PATTERN_BEATS;
            let (start_tick, end_tick) = (f64::from(start_beat) * TICK, f64::from(start_beat + PATTERN_BEATS) * TICK);
            let suffix = if count > 1 { format!(" {}/{count}", p + 1) } else { String::new() };
            let tracks = defs
                .iter()
                .map(|def| {
                    let mut track = Track::new(&def.name, PATTERN_BEATS, 4);
                    let ticks_per_step = TICK / f64::from(track.steps_per_beat);
                    for note in &def.notes {
                        let ticks = js::round(note.tick as f64 / ratio);
                        if ticks < start_tick || ticks >= end_tick {
                            continue;
                        }
                        let beat = (ticks / TICK).floor() as i32 - start_beat;
                        let beat_step = js::round((ticks % TICK) / ticks_per_step) as i32;
                        let pitch = note.note.map_or(f64::NAN, f64::from) - def.base_note;
                        let step_percent = js::round(f64::from(beat_step) * 100.0 / f64::from(track.steps_per_beat));
                        track.notes.push(Note {
                            velocity: smf::velocity_to_unit(note.velocity),
                            beat,
                            step: beat_step,
                            pitch,
                            step_percent: Some(step_percent),
                            ..Note::default()
                        });
                    }
                    track
                })
                .collect();
            Pattern {
                name: Some(format!("{base_name}{suffix}")),
                beat_count: PATTERN_BEATS,
                bpm: 120.0,
                tracks,
                ..Pattern::default()
            }
        })
        .collect()
}
