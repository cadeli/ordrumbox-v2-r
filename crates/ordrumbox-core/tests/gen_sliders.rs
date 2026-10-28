//! `e2e/gen-sliders.spec.js` of ordrumbox-v2: each generation slider of an
//! auto-mode track has its own effect on the notes the pattern engine plays.
//!
//! As in the JS test, every operation gate but the one under test is closed
//! (so the whole variation budget goes to that effect), the projection is
//! rolled many times, and a one-sided invariant is checked against the
//! written notes: at 0 the effect never appears, at 100 it appears at least once.

use ordrumbox_core::{FlatNote, Library, Note, Pattern, Rng, Scales, Track, recompute_flat_notes};

const GATES: [&str; 9] = [
    "prob_pitch",
    "prob_velocity",
    "prob_silence",
    "prob_fill",
    "prob_ghost",
    "prob_retrig",
    "prob_euclid",
    "prob_note",
    "prob_arp",
];

/// One note every 4 steps: each has a free step beside it (fill, ghost).
fn sparse_grid() -> Vec<Note> {
    (0..4).map(|beat| Note { beat, ..Note::default() }).collect()
}

/// One note every step: every inner note has a neighbour on both sides (silence).
fn dense_grid() -> Vec<Note> {
    (0..4).flat_map(|beat| (0..4).map(move |beat_step| Note { beat, beat_step, ..Note::default() })).collect()
}

/// A 4-beat pattern with one BASS track in auto mode playing `notes`, every
/// gate closed but `keep`, both variation amounts at 100.
fn pattern(notes: Vec<Note>, keep: &str) -> Pattern {
    let (mut library, _) =
        Library::parse(r#"{"patterns":[{"name":"gen","beatCount":4,"tracks":[{"name":"BASS","auto":true}]}]}"#)
            .unwrap();
    let mut pattern = library.patterns.remove(0);
    let track = &mut pattern.tracks[0];
    track.notes = notes;
    track.auto_variation = Some(100.0);
    track.auto_variation2 = Some(100.0);
    for gate in GATES {
        set(track, gate, if gate == keep { 100.0 } else { 0.0 });
    }
    pattern
}

/// A slider moved (a track setting, 0–100 or a range).
fn set(track: &mut Track, key: &str, value: f64) {
    let field = match key {
        "prob_pitch" => &mut track.prob_pitch,
        "prob_velocity" => &mut track.prob_velocity,
        "prob_silence" => &mut track.prob_silence,
        "prob_fill" => &mut track.prob_fill,
        "prob_ghost" => &mut track.prob_ghost,
        "prob_retrig" => &mut track.prob_retrig,
        "prob_euclid" => &mut track.prob_euclid,
        "prob_note" => &mut track.prob_note,
        "prob_note_depth" => &mut track.prob_note_depth,
        "prob_arp" => &mut track.prob_arp,
        "pitch_range" => &mut track.pitch_range,
        _ => panic!("no slider {key}"),
    };
    *field = value;
}

/// The notes of `rolls` projections, pooled.
fn roll(pattern: &Pattern, rolls: u32, rng: &mut Rng) -> Vec<FlatNote> {
    (0..rolls)
        .flat_map(|pass| recompute_flat_notes(pattern, pass, rng, &Scales::default()).into_values().flatten())
        .collect()
}

/// `slider` at 0, then at 100: the notes of each.
fn at_0_and_100(mut pattern: Pattern, slider: &str, rolls: u32) -> (Vec<FlatNote>, Vec<FlatNote>) {
    let mut rng = Rng::new(11);
    set(&mut pattern.tracks[0], slider, 0.0);
    let base = roll(&pattern, rolls, &mut rng);
    set(&mut pattern.tracks[0], slider, 100.0);
    let varied = roll(&pattern, rolls, &mut rng);
    (base, varied)
}

// ─── Groove panel ───────────────────────────────────────────────────────────

#[test]
fn prob_pitch_never_moves_a_pitch_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_pitch"), "prob_pitch", 8);
    assert!(!base.is_empty());
    assert!(base.iter().all(|n| n.pitch == 0.0));
    assert!(varied.iter().any(|n| n.pitch != 0.0));
}

#[test]
fn prob_velocity_never_moves_a_velocity_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_velocity"), "prob_velocity", 8);
    assert!(base.iter().all(|n| n.velocity == 0.8));
    assert!(varied.iter().any(|n| n.velocity != 0.8));
}

#[test]
fn prob_silence_never_removes_a_note_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(dense_grid(), "prob_silence"), "prob_silence", 4);
    assert_eq!(base.len(), 4 * 16);
    assert!(varied.len() < base.len(), "a Silence of 100 must drop notes");
}

#[test]
fn prob_fill_never_adds_a_note_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_fill"), "prob_fill", 4);
    assert_eq!(base.len(), 4 * 4);
    assert!(varied.len() > base.len(), "a Fill of 100 must add notes");
}

#[test]
fn prob_ghost_never_adds_a_soft_note_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_ghost"), "prob_ghost", 8);
    assert!(base.iter().all(|n| n.velocity == 0.8 && n.source.is_some()));
    assert!(varied.iter().any(|n| n.source.is_none() && n.velocity < 0.8));
}

#[test]
fn pitch_range_bounds_how_far_a_pitch_can_travel() {
    let mut pattern = pattern(sparse_grid(), "prob_pitch");
    let mut rng = Rng::new(5);
    set(&mut pattern.tracks[0], "pitch_range", 1.0);
    let tight = roll(&pattern, 8, &mut rng);
    assert!(!tight.is_empty());
    assert!(tight.iter().all(|n| n.pitch.abs() <= 1.0), "a Pitch Rng of 1 keeps every pitch inside ±1");
    set(&mut pattern.tracks[0], "pitch_range", 24.0);
    let wide = roll(&pattern, 8, &mut rng);
    assert!(wide.iter().any(|n| n.pitch.abs() > 12.0), "a Pitch Rng of 24 throws pitches past an octave");
}

#[test]
fn pitch_scale_lock_keeps_every_varied_pitch_inside_the_scale() {
    let mut pattern = pattern(sparse_grid(), "prob_pitch");
    set(&mut pattern.tracks[0], "pitch_range", 24.0);
    // No scale named: the major scale, as `pickPitch` falls back to.
    let on_scale = |pitch: f64| [0.0, 2.0, 4.0, 5.0, 7.0, 9.0, 11.0].contains(&(pitch.abs() % 12.0));
    let mut rng = Rng::new(9);
    pattern.tracks[0].pitch_scale_lock = true;
    let locked = roll(&pattern, 12, &mut rng);
    assert!(!locked.is_empty());
    assert!(locked.iter().all(|n| on_scale(n.pitch)), "Scale Lock never leaves the scale");
    pattern.tracks[0].pitch_scale_lock = false;
    let free = roll(&pattern, 12, &mut rng);
    assert!(free.iter().any(|n| !on_scale(n.pitch)), "without Scale Lock a wide range leaves the scale");
}

// ─── Engine panel ───────────────────────────────────────────────────────────

#[test]
fn prob_retrig_never_retriggers_at_0_always_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_retrig"), "prob_retrig", 8);
    assert_eq!(base.len(), 8 * 4, "each written note once");
    // More notes, all from the written ones: repeats.
    assert!(varied.len() > base.len());
    assert!(varied.iter().all(|n| n.source.is_some()));
}

#[test]
fn prob_euclid_never_fills_a_beat_at_0_always_one_at_100() {
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_euclid"), "prob_euclid", 8);
    assert_eq!(base.len(), 8 * 4);
    assert!(varied.len() > base.len(), "an Euclid of 100 adds fill pulses");
}

#[test]
fn prob_note_never_randomizes_a_play_probability_at_0_always_at_100() {
    // A probability below 1 shows as notes left out of a pass.
    let (base, varied) = at_0_and_100(pattern(sparse_grid(), "prob_note"), "prob_note", 8);
    assert_eq!(base.len(), 8 * 4);
    assert!(varied.len() < base.len());
}

#[test]
fn prob_note_depth_stretches_how_far_the_draw_can_fall() {
    let mut pattern = pattern(sparse_grid(), "prob_note");
    let mut rng = Rng::new(4);
    set(&mut pattern.tracks[0], "prob_note_depth", 0.0);
    assert_eq!(roll(&pattern, 8, &mut rng).len(), 8 * 4, "a Depth of 0 leaves the notes alone");
    set(&mut pattern.tracks[0], "prob_note_depth", 100.0);
    assert!(roll(&pattern, 8, &mut rng).len() < 8 * 4);
}
