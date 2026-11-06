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
//! drops the later notes the catalog's mapping resolves to it; the tempo meta event is never
//! read (120 BPM); every pattern is 32 beats long, up to 16 patterns.
//!
//! Three departures from the JS: a file timed in SMPTE frames or with a 0
//! division is refused (the JS makes 16 patterns of misplaced notes), the
//! message says when notes past the 16th pattern are left out, and a drum key
//! found by its GM name, whose instrument another key plays, gets a numbered
//! track (`KICK 2`) instead of losing its notes.

use ordrumbox_core::instruments::{Catalog, Instrument, catalog};
use ordrumbox_core::js;
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

/// `#resolveDrumTrack`: one track per instrument the notes resolve to.
fn drum_tracks(c: &Catalog, notes: &[NoteOn], channel: u8) -> Option<Vec<TrackDef>> {
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
        let name = if *mapped {
            instrument.id.clone()
        } else {
            // A key named after an instrument another key plays ("Acoustic Bass Drum"
            // beside "Bass Drum 1") is another sound of it: a track of its own, numbered.
            let taken = |name: &str| {
                defs.iter().any(|d| d.name == name) || resolved.iter().any(|(_, _, i, m)| *m && i.id == name)
            };
            let numbered = (2..).map(|n| format!("{} {n}", instrument.id));
            std::iter::once(instrument.id.clone()).chain(numbered).find(|name| !taken(name)).unwrap_or_default()
        };
        if !defs.iter().any(|d| d.name == name) {
            defs.push(TrackDef { name, notes: group.clone(), base_note: f64::from(*key) });
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
            if !defs.iter().any(|d| d.name == instrument.id) {
                let base_note = root_or_c3(&instrument.id);
                defs.push(TrackDef { name: instrument.id.clone(), notes: list.clone(), base_note });
            }
            continue;
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
    let (patterns, cut) = patterns_from(&defs, base_name, f64::from(division));
    let mut message = if patterns.len() > 1 {
        format!("MIDI imported: {} track(s) into {} patterns", defs.len(), patterns.len())
    } else {
        format!("MIDI imported: {} track(s)", defs.len())
    };
    if cut {
        message.push_str(&format!(", notes past beat {} left out", MAX_PATTERNS * i64::from(PATTERN_BEATS)));
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
    let step_of = |tick: i64| js::round(tick as f64 * f64::from(STEPS_PER_BEAT) / division) as i64;
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
                        let step_percent = js::round(f64::from(beat_step) * 100.0 / f64::from(STEPS_PER_BEAT));
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
    fn notes_past_the_last_pattern_are_cut() {
        let last_beat = MAX_PATTERNS * i64::from(PATTERN_BEATS);
        let (patterns, cut) = patterns_from(&[def(&[(last_beat - 1) * 96, last_beat * 96])], "f", 96.0);
        assert!(cut);
        assert_eq!(patterns.len(), 16);
        assert_eq!(places(&patterns[15]), [(31, 0)]);
    }
}
