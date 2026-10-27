//! The two variation layers of a track (`src/patterns/track_variation.js`).
//!
//! - **Notes** (`variation2`): before expansion, some written notes get new
//!   repeats, euclidean fill, probability or arpeggio span ([`vary_notes`]).
//! - **Flat notes** (`variation`): after expansion, notes are silenced,
//!   re-velocitied, re-pitched, or anticipations, doubles and ghosts are added
//!   ([`apply`]).
//!
//! Each candidate operation first passes its own chance (a `prob_*` slider in
//! auto mode, 50 % otherwise); the survivors are shuffled and applied while the
//! budget bought by the amount slider lasts.

use std::collections::HashMap;

use crate::js;
use crate::json::Value;
use crate::note::Note;
use crate::pattern_engine::{FlatNote, FlatNotes};
use crate::rng::Rng;
use crate::scales::Scales;
use crate::steps::{step_to_beat, step_to_tick};
use crate::track::Track;

/// Amounts an auto track uses until the generator has picked its own.
const AUTO_VARIATION_FALLBACK: f64 = 60.0;
const AUTO_VARIATION2_FALLBACK: f64 = 40.0;

/// Chance (0–100) every operation gets outside auto mode, where the per-kind
/// sliders are hidden.
const DEFAULT_CHANCE: f64 = 50.0;

/// Lowest play probability the note-probability operation can draw (at depth 100).
const NOTE_PROB_FLOOR: f64 = 0.2;

/// Budget (cost units, 16 at 100) bought by an amount slider (0–100). Any
/// non-zero amount buys at least one unit.
fn budget_for(amount: f64) -> i32 {
    if amount <= 0.0 { 0 } else { (js::round(amount * 16.0 / 100.0) as i32).max(1) }
}

/// Keep each candidate with its chance (0–100), then shuffle the survivors.
fn select<T>(candidates: Vec<T>, chance: impl Fn(&T) -> f64, rng: &mut Rng) -> Vec<T> {
    let mut kept: Vec<T> = candidates.into_iter().filter(|c| rng.next_f64() * 100.0 < chance(c)).collect();
    rng.shuffle(&mut kept);
    kept
}

fn round2(value: f64) -> f64 {
    js::round(value * 100.0) / 100.0
}

// ── Notes layer (variation2) ────────────────────────────────────────────

enum NoteOp {
    RetrigRate { retrigger_count: i32, rate: f64 },
    EuclideanFill(i32),
    Prob(f64),
    ArpSpan(f64),
}

impl NoteOp {
    fn cost(&self, budget: i32) -> i32 {
        match self {
            Self::RetrigRate { .. } => 2,
            Self::EuclideanFill(fill) => (*fill).min(budget),
            Self::Prob(_) | Self::ArpSpan(_) => 1,
        }
    }

    fn chance(&self, track: &Track) -> f64 {
        if !track.auto {
            return DEFAULT_CHANCE;
        }
        match self {
            Self::RetrigRate { .. } => track.prob_retrig,
            Self::EuclideanFill(_) => track.prob_euclid,
            Self::Prob(_) => track.prob_note,
            Self::ArpSpan(_) => track.prob_arp,
        }
    }
}

/// The varied copies of a track's notes, by note index; notes not varied are
/// absent, and the track itself is never modified (`computeNoteVariation`).
pub(crate) fn vary_notes(track: &Track, rng: &mut Rng) -> HashMap<usize, Note> {
    let amount = if track.auto { track.auto_variation2.unwrap_or(AUTO_VARIATION2_FALLBACK) } else { track.variation2 };
    let budget = budget_for(amount);
    let mut varied = HashMap::new();
    if budget <= 0 || track.notes.is_empty() {
        return varied;
    }
    let depth = track.prob_note_depth;
    let mut candidates = Vec::new();
    for (idx, note) in track.notes.iter().enumerate() {
        let retrigger_count = rng.below(4) as i32 + 1;
        let max_rate = (4 - retrigger_count).max(1);
        let rate = (rng.below(max_rate as u64) + 1) as f64;
        candidates.push((idx, NoteOp::RetrigRate { retrigger_count, rate }));
        candidates.push((idx, NoteOp::EuclideanFill(rng.below(2) as i32 + 1)));
        // Depth 0 cannot move the probability: not offered, its cost stays free.
        if depth > 0.0 {
            let floor = 1.0 - (1.0 - NOTE_PROB_FLOOR) * depth / 100.0;
            candidates.push((idx, NoteOp::Prob(round2(floor + rng.next_f64() * (1.0 - floor)))));
        }
        if note.arp.as_array().is_some_and(|intervals| intervals.len() >= 2) {
            candidates.push((idx, NoteOp::ArpSpan((rng.below(7) + 6) as f64)));
        }
    }

    let mut remaining = budget;
    for (idx, op) in select(candidates, |(_, op)| op.chance(track), rng) {
        let cost = op.cost(budget);
        if cost > remaining {
            continue;
        }
        remaining -= cost;
        let note = varied.entry(idx).or_insert_with(|| track.notes[idx].clone());
        match op {
            NoteOp::RetrigRate { retrigger_count, rate } => {
                note.retrigger_count = retrigger_count;
                note.rate = rate;
            }
            NoteOp::EuclideanFill(fill) => note.euclidean_fill = fill,
            NoteOp::Prob(prob) => note.prob = prob,
            NoteOp::ArpSpan(span) => {
                if let Value::Array(intervals) = &mut note.arp {
                    intervals[0] = Value::Number(span);
                }
            }
        }
    }
    varied
}

// ── Flat-notes layer (variation) ────────────────────────────────────────

/// A flat note found by the scan: its tick and its index in that tick's list.
/// Removals are deferred to the end of the loop, and additions are appended,
/// so these stay valid while operations apply.
type Slot = (u32, usize);

/// Where an added note goes.
#[derive(Clone, Copy)]
struct Target {
    tick: u32,
    beat: i32,
    beat_step: i32,
}

enum FlatOp {
    Silence(Slot),
    Velocity(Slot),
    Pitch(Slot),
    /// A softer copy on the next step, before a note that has no neighbour.
    Anticipation(Slot, Target),
    /// A copy on the next step, after a note that has no neighbour.
    Double(Slot, Target),
    /// A quiet copy in the middle of a gap of 3 steps or more.
    Ghost(Slot, Target),
}

impl FlatOp {
    fn cost(&self) -> i32 {
        match self {
            Self::Velocity(_) | Self::Pitch(_) => 1,
            Self::Silence(_) | Self::Anticipation(..) | Self::Double(..) | Self::Ghost(..) => 3,
        }
    }

    fn chance(&self, track: &Track) -> f64 {
        if !track.auto {
            return DEFAULT_CHANCE;
        }
        match self {
            Self::Silence(_) => track.prob_silence,
            Self::Velocity(_) => track.prob_velocity,
            Self::Pitch(_) => track.prob_pitch,
            Self::Anticipation(..) | Self::Double(..) => track.prob_fill,
            Self::Ghost(..) => track.prob_ghost,
        }
    }
}

/// Apply the flat-notes layer of one track, loop by loop (`TrackVariation.apply`).
pub(crate) fn apply(
    flat: &mut FlatNotes,
    track_idx: usize,
    track: &Track,
    loop_ticks: i64,
    pattern_ticks: i64,
    rng: &mut Rng,
    scales: &Scales,
) {
    let amount = if track.auto { track.auto_variation.unwrap_or(AUTO_VARIATION_FALLBACK) } else { track.variation };
    let budget = budget_for(amount);
    if budget <= 0 || loop_ticks <= 0 {
        return;
    }
    let spb = track.steps_per_beat;
    let steps_in_loop = js::round(loop_ticks as f64 * f64::from(spb) / f64::from(crate::steps::TICK)) as i64;
    let loop_count = ((pattern_ticks as f64 / loop_ticks as f64).ceil() as i64).max(1);

    for lp in 0..loop_count {
        let tick_of = |step: i64| lp * loop_ticks + step_to_tick(step, spb);
        let target = |step: i64| {
            let (beat, beat_step) = step_to_beat(step, spb);
            u32::try_from(tick_of(step)).ok().map(|tick| Target {
                tick,
                beat: beat as i32,
                beat_step: beat_step as i32,
            })
        };

        // Steps of this loop where the track plays, with its first note there.
        let mut played: Vec<(i64, Slot)> = Vec::new();
        for step in 0..steps_in_loop {
            let Ok(tick) = u32::try_from(tick_of(step)) else { continue };
            if i64::from(tick) >= pattern_ticks {
                continue;
            }
            if let Some(idx) = flat.get(&tick).and_then(|notes| notes.iter().position(|n| n.track == track_idx)) {
                played.push((step, (tick, idx)));
            }
        }
        if played.is_empty() {
            continue;
        }
        let is_played = |step: i64| played.iter().any(|&(s, _)| s == step);

        let mut ops = Vec::new();
        for &(step, slot) in &played {
            let next = (step < steps_in_loop - 1).then_some(step + 1);
            let has_prev = step > 0 && is_played(step - 1);
            let next_free = next.filter(|&n| !is_played(n));
            if let (false, Some(next)) = (has_prev, next_free) {
                if let Some(target) = target(next) {
                    ops.push(FlatOp::Anticipation(slot, target));
                }
            }
            if has_prev && next.is_some_and(is_played) {
                ops.push(FlatOp::Silence(slot));
            }
            ops.push(FlatOp::Velocity(slot));
            ops.push(FlatOp::Pitch(slot));
            if let Some(next) = next_free {
                if let Some(target) = target(next) {
                    ops.push(FlatOp::Double(slot, target));
                }
            }
        }
        for pair in played.windows(2) {
            let ((step, slot), (next_step, _)) = (pair[0], pair[1]);
            let gap = next_step - step;
            if gap >= 3 {
                if let Some(target) = target(step + gap / 2) {
                    ops.push(FlatOp::Ghost(slot, target));
                }
            }
        }

        let mut silenced = Vec::new();
        let mut remaining = budget;
        for op in select(ops, |op| op.chance(track), rng) {
            if op.cost() > remaining {
                continue;
            }
            remaining -= op.cost();
            let note_at = |flat: &FlatNotes, (tick, idx): Slot| flat[&tick][idx].clone();
            match op {
                FlatOp::Silence(slot) => silenced.push(slot),
                FlatOp::Velocity((tick, idx)) => {
                    let velocity = round2(rng.next_f64());
                    flat.get_mut(&tick).expect("scanned tick")[idx].velocity = velocity;
                }
                FlatOp::Pitch((tick, idx)) => {
                    let pitch = pick_pitch(track, rng, scales);
                    flat.get_mut(&tick).expect("scanned tick")[idx].pitch = pitch;
                }
                FlatOp::Anticipation(slot, target) => {
                    let from = note_at(flat, slot);
                    add(flat, track_idx, &from, target, round2((from.velocity * 0.7).max(0.2)));
                }
                FlatOp::Double(slot, target) => {
                    let from = note_at(flat, slot);
                    add(flat, track_idx, &from, target, round2(from.velocity * 0.8));
                }
                FlatOp::Ghost(slot, target) => {
                    let from = note_at(flat, slot);
                    add(flat, track_idx, &from, target, round2(from.velocity * 0.5));
                }
            }
        }

        silenced.sort_unstable_by(|a, b| b.cmp(a)); // highest index first in each tick
        for (tick, idx) in silenced {
            if let Some(notes) = flat.get_mut(&tick) {
                notes.remove(idx);
                if notes.is_empty() {
                    flat.remove(&tick);
                }
            }
        }
    }
}

/// A note added by the variation: pitch and pan of `from`, default everything else.
fn add(flat: &mut FlatNotes, track_idx: usize, from: &FlatNote, target: Target, velocity: f64) {
    flat.entry(target.tick).or_default().push(FlatNote {
        track: track_idx,
        source: None,
        beat: target.beat,
        beat_step: target.beat_step,
        pitch: from.pitch,
        velocity,
        pan: from.pan,
    });
}

/// A random transposition within ±`pitch_range` (±12 outside auto mode),
/// snapped to the track scale when `pitch_scale_lock` is on in auto mode.
fn pick_pitch(track: &Track, rng: &mut Rng, scales: &Scales) -> f64 {
    let range = if track.auto { track.pitch_range } else { 12.0 };
    let raw = (rng.next_f64() * (2.0 * range + 1.0)).floor() - range;
    if !track.auto || !track.pitch_scale_lock {
        return raw;
    }
    let degrees = scales.steps_or_major(track.scale_name.as_deref());
    let (sign, abs) = (if raw < 0.0 { -1.0 } else { 1.0 }, raw.abs());
    let degree = abs % 12.0;
    let nearest = degrees.iter().map(|&d| f64::from(d)).fold((0.0, f64::INFINITY), |best, d| {
        let distance = (degree - d).abs();
        if distance < best.1 { (d, distance) } else { best }
    });
    sign * ((abs / 12.0).floor() * 12.0 + nearest.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_buys_at_least_one_unit() {
        assert_eq!(budget_for(0.0), 0);
        assert_eq!(budget_for(1.0), 1);
        assert_eq!(budget_for(50.0), 8);
        assert_eq!(budget_for(100.0), 16);
    }

    #[test]
    fn locked_pitch_stays_in_the_scale() {
        let track = Track {
            auto: true,
            pitch_scale_lock: true,
            pitch_range: 24.0,
            scale_name: Some("dorian".into()),
            ..Track::default()
        };
        let scales = Scales::from_json(&crate::json::parse(r#"{"dorian":{"scaleSteps":[0,2,3,5,7,9,10]}}"#).unwrap());
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let pitch = pick_pitch(&track, &mut rng, &scales);
            assert!((-24.0..=24.0).contains(&pitch));
            assert!([0.0, 2.0, 3.0, 5.0, 7.0, 9.0, 10.0].contains(&(pitch.abs() % 12.0)), "{pitch}");
        }
    }

    #[test]
    fn no_amount_no_draw() {
        let track = Track { notes: vec![Note::default()], ..Track::default() };
        let mut rng = Rng::new(1);
        let before = rng.clone().next_u64();
        assert!(vary_notes(&track, &mut rng).is_empty());
        let mut flat = FlatNotes::new();
        apply(&mut flat, 0, &track, 128, 128, &mut rng, &Scales::default());
        assert_eq!(rng.next_u64(), before);
    }
}
