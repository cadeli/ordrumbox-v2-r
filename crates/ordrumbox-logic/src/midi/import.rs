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
//! An instrument that already has a track gets another one, numbered
//! (`KICK 2`, `BASS 2`): no note is dropped for sharing an instrument. A
//! channel left with no instrument at all is left out, and the message says
//! so, as it says when notes fall past the 16th pattern. The tempo meta event
//! is never read (the library's tempo stays); every pattern is 32 beats long,
//! up to 16 patterns. A file timed in SMPTE frames or with a 0 division is
//! refused.

use ordrumbox_core::instruments::{Catalog, Instrument, catalog};
use ordrumbox_core::{Note, Pattern, Track};

use super::smf::{self, NoteOn};

/// `C3_MIDI_NOTE`: the MIDI note of pitch 0.
pub const C3_MIDI_NOTE: f64 = 48.0;
/// `MIDI_MAX_BEATS`, `MIDI_MAX_PATTERNS`.
const PATTERN_BEATS: i32 = 32;
const MAX_PATTERNS: i64 = 16;
/// Grid of the imported tracks.
const STEPS_PER_BEAT: i32 = 4;

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

/// The track name of instrument `id`: `id`, else the first of `id 2`,
/// `id 3`… that `taken` leaves free.
fn free_name(id: &str, taken: impl Fn(&str) -> bool) -> String {
    let numbered = (2..).map(|n| format!("{id} {n}"));
    std::iter::once(id.to_owned()).chain(numbered).find(|name| !taken(name)).unwrap_or_default()
}

fn has_track(defs: &[TrackDef], name: &str) -> bool {
    defs.iter().any(|d| d.name == name)
}

/// One track per drum key the notes of `channel` resolve to an instrument,
/// named apart from the tracks of `before`.
fn drum_tracks(c: &Catalog, notes: &[NoteOn], channel: u8, before: &[TrackDef]) -> Option<Vec<TrackDef>> {
    let mut groups: Vec<(Option<u8>, Vec<NoteOn>)> = Vec::new();
    for note in notes {
        match groups.iter_mut().find(|(n, _)| *n == note.note) {
            Some((_, group)) => group.push(*note),
            None => groups.push((note.note, vec![*note])),
        }
    }
    // The catalog's mapping first: a key it maps claims its instrument's name.
    let resolved: Vec<(u8, Vec<NoteOn>, &Instrument, bool)> = groups
        .into_iter()
        .filter_map(|(key, group)| {
            let key = key?;
            if let Some(mapped) = found(c.from_midi(i64::from(channel), i64::from(key))) {
                return Some((key, group, mapped, true));
            }
            let (_, gm) = c.drum_names.iter().find(|(k, _)| *k == i64::from(key))?;
            Some((key, group, found(c.from_file_name(gm))?, false))
        })
        .collect();
    let mut defs: Vec<TrackDef> = Vec::new();
    for (key, group, instrument, mapped) in &resolved {
        // A key the mapping names claims its instrument's plain name; a key
        // named after an instrument another key plays ("Acoustic Bass Drum"
        // beside "Bass Drum 1") is another sound of it: a track of its own, numbered.
        let name = free_name(&instrument.id, |name| {
            has_track(before, name)
                || has_track(&defs, name)
                || (!mapped && resolved.iter().any(|(_, _, i, m)| *m && i.id == name))
        });
        defs.push(TrackDef { name, notes: group.clone(), base_note: f64::from(*key) });
    }
    (!defs.is_empty()).then_some(defs)
}

/// The patterns of `bytes`, a MIDI file named `file_name`;
/// `Err` with the reason when nothing can be imported.
pub fn import(file_name: &str, bytes: &[u8], roots: &[(String, f64)]) -> Result<Imported, String> {
    let midi = smf::parse(bytes);
    let notes = midi.note_ons();
    if notes.is_empty() {
        return Err("No MIDI notes found in file".into());
    }
    // A set top bit means SMPTE frames, which this import does not convert.
    let division = midi.header.division;
    if division == 0 || division & 0x8000 != 0 {
        return Err(format!("MIDI timing not supported (division {division:#06x}): only ticks per beat are read"));
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
        if *channel != 9
            && let Some(instrument) = found(c.from_midi_program(i64::from(program))).filter(|i| !i.drum)
        {
            let name = free_name(&instrument.id, |name| has_track(&defs, name));
            defs.push(TrackDef { name, notes: list.clone(), base_note: root_or_c3(&instrument.id) });
            continue;
        }
        if let Some(drums) = drum_tracks(c, list, *channel, &defs) {
            defs.extend(drums);
            continue;
        }
        if !track_name.is_empty()
            && let Some(instrument) = found(c.by_name(track_name)).filter(|i| !i.drum)
        {
            let name = free_name(&instrument.id, |name| has_track(&defs, name));
            defs.push(TrackDef { name, notes: list.clone(), base_note: root_or_c3(&instrument.id) });
            continue;
        }
        skipped.push(*channel);
    }
    let mut ids: Vec<&str> = c.instruments.iter().map(|i| i.id.as_str()).collect();
    ids.sort_unstable();
    // Notes of the channels no instrument was left for.
    let mut unplaced = 0;
    for channel in skipped {
        let list = channels.iter().find(|(ch, _, _)| *ch == channel).map(|(_, l, _)| l.clone()).unwrap_or_default();
        let Some(id) = ids.iter().find(|id| !has_track(&defs, id)) else {
            unplaced += list.len();
            continue;
        };
        defs.push(TrackDef { name: (*id).to_owned(), notes: list, base_note: root_or_c3(id) });
    }
    if defs.is_empty() {
        return Err("No matching instruments found in MIDI file".into());
    }

    let base_name = strip_midi_extension(file_name);
    let (patterns, cut) = patterns_from(&defs, base_name, f64::from(division));
    let mut message = if patterns.len() > 1 {
        format!("MIDI imported: {} track(s) into {} patterns", defs.len(), patterns.len())
    } else {
        format!("MIDI imported: {} track(s)", defs.len())
    };
    if cut {
        message.push_str(&format!(", notes past beat {} left out", MAX_PATTERNS * i64::from(PATTERN_BEATS)));
    }
    if unplaced > 0 {
        message.push_str(&format!(", {unplaced} note(s) left out: no instrument left for their channel"));
    }
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
/// track in each of them, and whether notes fall past the last one.
fn patterns_from(defs: &[TrackDef], base_name: &str, division: f64) -> (Vec<Pattern>, bool) {
    // Each note goes to the step nearest its MIDI tick, rounded once: a note
    // just before a beat rounds to that beat's first step, never to a step
    // past the last one of the beat before.
    let step_of = |tick: i64| (tick as f64 * f64::from(STEPS_PER_BEAT) / division).round() as i64;
    let pattern_steps = i64::from(PATTERN_BEATS * STEPS_PER_BEAT);
    let last_step = defs.iter().flat_map(|d| &d.notes).map(|n| step_of(n.tick)).fold(0, i64::max);
    let wanted = last_step / pattern_steps + 1;
    let count = wanted.min(MAX_PATTERNS) as usize;
    let cut = wanted > MAX_PATTERNS;
    let patterns = (0..count)
        .map(|p| {
            let first_step = p as i64 * pattern_steps;
            let suffix = if count > 1 { format!(" {}/{count}", p + 1) } else { String::new() };
            let tracks = defs
                .iter()
                .map(|def| {
                    let mut track = Track::new(&def.name, PATTERN_BEATS, STEPS_PER_BEAT);
                    for note in &def.notes {
                        let step = step_of(note.tick) - first_step;
                        if !(0..pattern_steps).contains(&step) {
                            continue;
                        }
                        let spb = i64::from(STEPS_PER_BEAT);
                        let (beat, beat_step) = ((step / spb) as i32, (step % spb) as i32);
                        let pitch = note.note.map_or(f64::NAN, f64::from) - def.base_note;
                        let step_percent = (f64::from(beat_step) * 100.0 / f64::from(STEPS_PER_BEAT)).round();
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
                tracks,
                ..Pattern::default()
            }
        })
        .collect();
    (patterns, cut)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(ticks: &[i64]) -> TrackDef {
        let notes = ticks.iter().map(|&tick| NoteOn { tick, channel: 9, note: Some(36), velocity: 100, track: 0 });
        TrackDef { name: "KICK".into(), notes: notes.collect(), base_note: 36.0 }
    }

    fn places(pattern: &Pattern) -> Vec<(i32, i32)> {
        pattern.tracks[0].notes.iter().map(|n| (n.beat, n.step)).collect()
    }

    #[test]
    fn a_note_just_before_a_beat_starts_that_beat() {
        // 96 ticks a beat: tick 90 is 3.75 steps, the first step of beat 1,
        // never step 4 of beat 0.
        let (patterns, cut) = patterns_from(&[def(&[0, 30, 90])], "f", 96.0);
        assert!(!cut);
        assert_eq!(places(&patterns[0]), [(0, 0), (0, 1), (1, 0)]);
    }

    #[test]
    fn a_note_rounded_past_the_last_step_opens_the_next_pattern() {
        // 3.95 steps before the end of beat 31: it rounds to beat 32, the first
        // of a second pattern, where it would have been lost.
        let (patterns, cut) = patterns_from(&[def(&[0, 31 * 96 + 95])], "f", 96.0);
        assert!(!cut);
        assert_eq!(patterns.len(), 2);
        assert_eq!(places(&patterns[0]), [(0, 0)]);
        assert_eq!(places(&patterns[1]), [(0, 0)]);
    }

    #[test]
    fn two_channels_of_one_instrument_make_two_tracks() {
        // Channels 0 and 1, both program 33 (a bass), one note each.
        let events = [
            0x00, 0xc0, 33, 0x00, 0xc1, 33, //
            0x00, 0x90, 40, 100, 0x00, 0x91, 43, 100, //
            0x00, 0xff, 0x2f, 0x00,
        ];
        let mut bytes = b"MThd\0\0\0\x06\0\0\0\x01\0\x60MTrk".to_vec();
        bytes.extend(u32::try_from(events.len()).unwrap().to_be_bytes());
        bytes.extend(events);
        let imported = import("two.mid", &bytes, &[]).unwrap();
        let names: Vec<&str> = imported.patterns[0].tracks.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{names:?}");
        assert_eq!(names[1], format!("{} 2", names[0]));
        assert!(imported.patterns[0].tracks.iter().all(|t| t.notes.len() == 1));
    }

    #[test]
    fn notes_past_the_last_pattern_are_cut() {
        let last_beat = MAX_PATTERNS * i64::from(PATTERN_BEATS);
        let (patterns, cut) = patterns_from(&[def(&[(last_beat - 1) * 96, last_beat * 96])], "f", 96.0);
        assert!(cut);
        assert_eq!(patterns.len(), 16);
        assert_eq!(places(&patterns[15]), [(31, 0)]);
    }
}
