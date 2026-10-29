//! Chord progressions (`logic/composition.js`): what the piano roll's
//! keyboard menu writes with "Add sequence", one chord per 4/4 measure.

/// Beats in one measure: a chord per measure.
const BEATS_PER_MEASURE: i32 = 4;

/// The intervals of a chord quality, semitones from its root (`CHORD_QUALITIES`).
fn intervals(quality: &str) -> &'static [f64] {
    match quality {
        "min" => &[0.0, 3.0, 7.0],
        "dim" => &[0.0, 3.0, 6.0],
        "maj7" => &[0.0, 4.0, 7.0, 11.0],
        "min7" => &[0.0, 3.0, 7.0, 10.0],
        "sus4" => &[0.0, 5.0, 7.0],
        _ => &[0.0, 4.0, 7.0],
    }
}

/// A progression: its chords as (quality, root in semitones from the tonic).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progression {
    pub id: &'static str,
    pub name: &'static str,
    pub chords: &'static [(&'static str, i32)],
}

/// The standard progressions (`SEQUENCES`).
pub const PROGRESSIONS: [Progression; 6] = [
    Progression { id: "pop", name: "I-V-vi-IV", chords: &[("maj", 0), ("maj", 7), ("min", 9), ("maj", 5)] },
    Progression { id: "doo-wop", name: "I-vi-IV-V", chords: &[("maj", 0), ("min", 9), ("maj", 5), ("maj", 7)] },
    Progression {
        id: "minor-natural",
        name: "i-VI-III-VII",
        chords: &[("min", 0), ("maj", 8), ("maj", 3), ("maj", 10)],
    },
    Progression { id: "andalusian", name: "i-VII-VI-V", chords: &[("min", 0), ("maj", 10), ("maj", 8), ("maj", 7)] },
    Progression { id: "canon", name: "I-V-vi-iii", chords: &[("maj", 0), ("maj", 7), ("min", 9), ("min", 4)] },
    Progression { id: "ii-V-I", name: "ii-V-I", chords: &[("min", 2), ("maj", 7), ("maj", 0)] },
];

/// Progression `index`, wrapping both ways.
pub fn progression(index: i64) -> &'static Progression {
    &PROGRESSIONS[index.rem_euclid(PROGRESSIONS.len() as i64) as usize]
}

/// The notes of `progression` from `tonic` (a pitch relative to the track)
/// over `beat_count` beats: each chord's notes on the first step of its
/// measure, as (beat, pitch).
pub fn progression_notes(progression: &Progression, tonic: f64, beat_count: i32) -> Vec<(i32, f64)> {
    let mut notes = Vec::new();
    for (measure, beat) in (0..beat_count.max(0)).step_by(BEATS_PER_MEASURE as usize).enumerate() {
        let (quality, root) = progression.chords[measure % progression.chords.len()];
        let root = tonic + f64::from(root);
        notes.extend(intervals(quality).iter().map(|interval| (beat, root + interval)));
    }
    notes
}
