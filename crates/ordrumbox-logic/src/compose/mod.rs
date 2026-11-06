//! Drum lines from a style (`ordrumbox_core::style`).
//!
//! Each step of the measure gets a weight: the style's weight for the part,
//! mixed with the plain metrical weight (`composeStyleWeight`), flattened by
//! the complexity, tilted toward the weak steps by the syncopation, and
//! raised or lowered where the coupled part plays. Then exactly the asked
//! number of hits is drawn per measure, required steps first (weighted
//! sampling without replacement), for several candidate lines; the one whose
//! syncopation is closest to the asked one is kept.

mod learn;
mod meter;
mod mutate;
mod sample;
mod score;

pub use learn::learn;
pub use meter::metric_weight;
pub use mutate::{fill, vary_line};
pub use score::syncopation;

use ordrumbox_core::num::round2;
use ordrumbox_core::{Macros, Rng, Style, StylePart, Track};

use crate::generate::add_note;

/// Candidate lines drawn for one line.
const CANDIDATES: usize = 16;
/// Weight of a step the style never plays: it can still be drawn when the
/// asked hits outnumber the style's steps.
const STYLE_FLOOR: f64 = 0.01;
/// Weight left to a step where the part to avoid plays: taken only when no
/// other step is free.
const AVOIDED: f64 = 0.0;
/// Rolls: the hits of a rolled note, and their spacing (half a step).
const ROLL_COUNTS: [i32; 3] = [2, 3, 4];
const ROLL_RATE: f64 = 4.0;

/// A track's compose settings as they apply: its own, else its part's (in
/// the units of the sliders: hits, then 0–100, the coupling -100–100).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub hits_per_measure: f64,
    pub complexity: f64,
    pub syncopation: f64,
    pub coupling: f64,
    pub variation: f64,
    pub style_weight: f64,
}

impl Settings {
    /// The settings `track` plays part `part` with, in a measure of
    /// `measure_steps` steps, the pattern's `macros` applied over them.
    pub fn of(track: &Track, part: &StylePart, measure_steps: usize, macros: Macros) -> Self {
        let percent = |value: f64| value.clamp(0.0, 100.0);
        let shift = macros.complexity_shift();
        let hits = track.compose_hits_per_measure.unwrap_or(part.hits_per_measure) * macros.energy_factor();
        Self {
            hits_per_measure: hits.round().clamp(0.0, measure_steps as f64),
            complexity: percent(track.compose_complexity.unwrap_or(part.complexity) + shift),
            syncopation: percent(track.compose_syncopation.unwrap_or(part.syncopation) + shift),
            coupling: track.compose_coupling.unwrap_or(part.coupling).clamp(-100.0, 100.0),
            variation: percent(track.compose_variation.unwrap_or(part.variation) * macros.variation_factor()),
            style_weight: track.compose_style_weight.clamp(0.0, 100.0),
        }
    }
}

/// What a track plays of `style`, as the compose sliders show it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrackCompose {
    /// The part's name.
    pub part: String,
    /// The settings as they apply (the pattern's macros included).
    pub used: Settings,
    /// The track's settings, else the part's (without the macros).
    pub base: Settings,
    /// The hits a measure can hold at most (its steps less the forbidden ones).
    pub open_hits: usize,
}

/// What `track` plays of `style` under `macros`; `None` when the style has no part for it.
pub fn track_settings(track: &Track, style: &Style, macros: Macros) -> Option<TrackCompose> {
    let (name, part) = style.part_for(&track.name)?;
    let steps = steps(style, part, Settings::of(track, part, usize::MAX, macros), track.steps_per_beat);
    Some(TrackCompose {
        part: name.to_owned(),
        used: Settings::of(track, part, steps.len(), macros),
        base: Settings::of(track, part, steps.len(), Macros::default()),
        open_hits: steps.iter().filter(|s| !s.forbidden).count(),
    })
}

/// One step of the measure, on the track's grid.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Step {
    /// The weight sampling uses (before the coupling).
    pub weight: f64,
    pub metric: f64,
    pub required: bool,
    pub forbidden: bool,
    pub accent: bool,
}

/// The steps of a measure of `style` on a grid of `steps_per_beat`.
pub(crate) fn steps(style: &Style, part: &StylePart, settings: Settings, steps_per_beat: i32) -> Vec<Step> {
    let (spb, beats) = (steps_per_beat.max(1) as usize, style.beat_count.max(1) as usize);
    let n = spb * beats;
    let grid = if part.weights.is_empty() { style.measure_steps() } else { part.weights.len() };
    let style_weight = settings.style_weight / 100.0;
    let flatten = 1.0 - 0.9 * settings.complexity / 100.0;
    let tilt = settings.syncopation / 100.0;
    (0..n)
        .map(|s| {
            let metric = metric_weight(s, spb, beats);
            let read = part.read(s, n, grid);
            let style_w = match read {
                Some(_) if part.weights.is_empty() => metric,
                Some(r) => r.weight.max(STYLE_FLOOR),
                None => STYLE_FLOOR,
            };
            let mut weight = style_w.powf(style_weight) * metric.powf(1.0 - style_weight);
            weight = weight.powf(flatten);
            weight *= if metric >= 0.7 { 1.5 - tilt } else { 0.5 + tilt };
            let forbidden = read.is_some_and(|r| r.forbidden);
            Step {
                weight: if forbidden { 0.0 } else { weight },
                metric,
                required: read.is_some_and(|r| r.required) && !forbidden,
                forbidden,
                accent: read.is_some_and(|r| r.accent),
            }
        })
        .collect()
}

/// Whether `partner` plays at step `a` of a track of `steps_per_beat`
/// (the partner's own grid and length, looping).
fn partner_plays(partner: &Track, a: usize, steps_per_beat: usize) -> bool {
    let pspb = partner.steps_per_beat.max(1) as usize;
    let numerator = a * pspb;
    if !numerator.is_multiple_of(steps_per_beat) {
        return false;
    }
    let total = (partner.beat_count.max(1) as usize) * pspb;
    let k = (numerator / steps_per_beat) % total;
    partner.notes.iter().any(|n| (n.beat.max(0) as usize) * pspb + n.step.max(0) as usize == k)
}

/// The weight of each step of the track's length, the coupling applied; and
/// the weights of the steps the coupling blocked (used only to make up the count).
fn line_weights(
    steps: &[Step],
    total: usize,
    spb: usize,
    coupling: f64,
    partner: Option<&Track>,
) -> (Vec<f64>, Vec<f64>) {
    let c = coupling / 100.0;
    let mut weights = Vec::with_capacity(total);
    let mut blocked = vec![0.0; total];
    for a in 0..total {
        let base = steps[a % steps.len()].weight;
        let plays = partner.is_some_and(|p| partner_plays(p, a, spb));
        let w = match plays {
            false => base,
            true if c >= 0.0 => base * (1.0 + 4.0 * c),
            true => {
                let w = base * (1.0 + c);
                if w <= 0.0 || c <= -1.0 {
                    blocked[a] = base;
                    AVOIDED
                } else {
                    w
                }
            }
        };
        weights.push(w);
    }
    (weights, blocked)
}

/// The track's length in steps.
pub(crate) fn track_steps(track: &Track) -> usize {
    (track.beat_count.max(1) * track.steps_per_beat.max(1)) as usize
}

/// One candidate line: per measure, the required steps then `hits` drawn.
fn candidate(steps: &[Step], weights: &[f64], blocked: &[f64], hits: f64, rng: &mut Rng) -> Vec<usize> {
    let (n, total) = (steps.len(), weights.len());
    let mut onsets = Vec::new();
    let mut start = 0;
    while start < total {
        let len = n.min(total - start);
        let required: Vec<usize> = (start..start + len).filter(|&a| steps[a % n].required).collect();
        let wanted = ((hits * len as f64 / n as f64).round() as usize).max(required.len());
        let free = |w: &[f64]| -> Vec<f64> {
            (start..start + len).map(|a| if required.contains(&a) { 0.0 } else { w[a] }).collect()
        };
        let mut drawn: Vec<usize> = sample::take(&free(weights), wanted - required.len(), rng);
        if drawn.len() + required.len() < wanted {
            let mut rest = free(blocked);
            drawn.iter().for_each(|&i| rest[i] = 0.0);
            drawn.extend(sample::take(&rest, wanted - required.len() - drawn.len(), rng));
        }
        onsets.extend(required);
        onsets.extend(drawn.into_iter().map(|i| start + i));
        start += len;
    }
    onsets.sort_unstable();
    onsets
}

/// The velocity of a hit on `step`: accented on the style's accents (on the
/// beats when it names none), a ghost note on a weak step at the part's
/// ghost rate, a little spread.
pub(crate) fn velocity(step: &Step, part: &StylePart, rng: &mut Rng) -> f64 {
    let accent = if part.accents.is_empty() { step.metric >= 0.7 } else { step.accent };
    let ghost = !accent && step.metric < 0.5 && rng.next_f64() < part.ghost;
    let base = if accent {
        0.95
    } else if ghost {
        0.4
    } else {
        0.78
    };
    round2((base + (rng.next_f64() * 2.0 - 1.0) * 0.05).clamp(0.25, 1.0))
}

/// Add a hit at step `a` of `track`, a roll at the part's roll rate.
pub(crate) fn add_hit(track: &mut Track, a: usize, step: &Step, part: &StylePart, rng: &mut Rng) {
    let spb = track.steps_per_beat.max(1) as usize;
    let velocity = velocity(step, part, rng);
    let roll = rng.next_f64() < part.rolls;
    let note = add_note(track, (a / spb) as i32, (a % spb) as i32, 0.0, velocity);
    if roll {
        note.retrigger_count = ROLL_COUNTS[(a + note.beat as usize) % ROLL_COUNTS.len()];
        note.retrigger_rate = ROLL_RATE;
    }
}

/// The style's grid and swing on `track`: its notes cleared when the grid changes.
pub(crate) fn set_meter(track: &mut Track, style: &Style) {
    if track.steps_per_beat != style.steps_per_beat {
        track.steps_per_beat = style.steps_per_beat;
        track.notes.clear();
    }
    track.loop_length = Some(track.beat_count * track.steps_per_beat);
    track.swing_amount = style.swing_amount;
    track.swing_resolution = style.swing_resolution;
}

/// A new line on `track` for its part of `style` under the pattern's
/// `macros` (nothing when the style has no part for it), coupled to
/// `partner`, its hits scaled by `factor` (a section's fullness). The track
/// takes the style's grid and swing.
pub fn drum_line(
    track: &mut Track,
    style: &Style,
    macros: Macros,
    partner: Option<&Track>,
    factor: f64,
    rng: &mut Rng,
) {
    let Some((_, part)) = style.part_for(&track.name) else { return };
    let part = part.clone();
    set_meter(track, style);
    let spb = track.steps_per_beat.max(1) as usize;
    let probe = Settings::of(track, &part, usize::MAX, macros);
    let steps = steps(style, &part, probe, track.steps_per_beat);
    let settings = Settings::of(track, &part, steps.len(), macros);
    let total = track_steps(track);
    let (weights, blocked) = line_weights(&steps, total, spb, settings.coupling, partner);
    let hits = settings.hits_per_measure * factor;
    let metric: Vec<f64> = (0..total).map(|a| steps[a % steps.len()].metric).collect();
    let lines: Vec<Vec<usize>> = (0..CANDIDATES).map(|_| candidate(&steps, &weights, &blocked, hits, rng)).collect();
    let syncopations: Vec<f64> = lines.iter().map(|l| score::syncopation(l, &metric)).collect();
    let typical: Vec<f64> = lines.iter().map(|l| score::typicality(l, &weights)).collect();
    let best = score::closest(&syncopations, &typical, settings.syncopation / 100.0);
    track.notes.clear();
    for &a in &lines[best] {
        add_hit(track, a, &steps[a % steps.len()], &part, rng);
    }
}

/// The random source of a line: the track's seed (with `salt`, the loop),
/// else a draw of `rnd`.
pub fn line_rng(track: &Track, salt: u64, rnd: &mut dyn FnMut() -> f64) -> Rng {
    if track.compose_seed >= 1.0 {
        Rng::new((track.compose_seed as u64).wrapping_mul(1_000_003).wrapping_add(salt))
    } else {
        Rng::new((rnd() * 9_007_199_254_740_992.0) as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::style::StylePart;

    fn style() -> Style {
        let mut style = Style::neutral("test");
        let kick = StylePart {
            weights: (0..16).map(|s| if s % 4 == 0 { 1.0 } else { 0.1 }).collect(),
            required: vec![0],
            forbidden: vec![2],
            hits_per_measure: 4.0,
            ..StylePart::default()
        };
        let snare = StylePart {
            weights: (0..16).map(|s| if s == 4 || s == 12 { 1.0 } else { 0.2 }).collect(),
            hits_per_measure: 5.0,
            coupling: -100.0,
            coupling_to: Some("KICK".into()),
            ..StylePart::default()
        };
        style.parts = vec![("KICK".into(), kick), ("SNARE".into(), snare)];
        style
    }

    fn steps_of(track: &Track) -> Vec<usize> {
        let spb = track.steps_per_beat as usize;
        track.notes.iter().map(|n| n.beat as usize * spb + n.step as usize).collect()
    }

    #[test]
    fn exactly_the_asked_hits_in_every_measure_required_in_forbidden_out() {
        let style = style();
        for seed in 0..100 {
            for hits in [0.0, 1.0, 4.0, 9.0, 15.0] {
                let mut track = Track::new("KICK", 8, 4);
                track.compose_hits_per_measure = Some(hits);
                drum_line(&mut track, &style, Macros::default(), None, 1.0, &mut Rng::new(seed));
                let steps = steps_of(&track);
                for m in 0..2 {
                    let in_measure: Vec<usize> = steps.iter().filter(|&&a| a / 16 == m).map(|a| a % 16).collect();
                    assert_eq!(in_measure.len(), (hits as usize).max(1), "seed {seed} hits {hits}: {in_measure:?}");
                    assert!(in_measure.contains(&0) && !in_measure.contains(&2));
                }
            }
        }
    }

    #[test]
    fn the_same_seed_makes_the_same_line() {
        let style = style();
        let mut a = Track::new("KICK", 4, 4);
        a.compose_seed = 42.0;
        let mut b = a.clone();
        let (mut rng_a, mut rng_b) = (line_rng(&a, 0, &mut || 0.5), line_rng(&b, 0, &mut || 0.9));
        drum_line(&mut a, &style, Macros::default(), None, 1.0, &mut rng_a);
        drum_line(&mut b, &style, Macros::default(), None, 1.0, &mut rng_b);
        assert_eq!(a.notes, b.notes);
    }

    #[test]
    fn a_part_that_avoids_never_lands_on_its_partner_while_steps_are_free() {
        let style = style();
        for seed in 0..100 {
            let mut kick = Track::new("KICK", 4, 4);
            drum_line(&mut kick, &style, Macros::default(), None, 1.0, &mut Rng::new(seed));
            let mut snare = Track::new("SNARE", 4, 4);
            drum_line(&mut snare, &style, Macros::default(), Some(&kick), 1.0, &mut Rng::new(seed + 1000));
            let (k, s) = (steps_of(&kick), steps_of(&snare));
            assert_eq!(s.len(), 5);
            assert!(s.iter().all(|a| !k.contains(a)), "kick {k:?} snare {s:?}");
        }
        // With no free step left, the count still holds.
        let mut kick = Track::new("KICK", 4, 4);
        kick.compose_hits_per_measure = Some(15.0);
        drum_line(&mut kick, &style, Macros::default(), None, 1.0, &mut Rng::new(1));
        let mut snare = Track::new("SNARE", 4, 4);
        snare.compose_hits_per_measure = Some(4.0);
        drum_line(&mut snare, &style, Macros::default(), Some(&kick), 1.0, &mut Rng::new(2));
        assert_eq!(snare.notes.len(), 4);
    }

    #[test]
    fn more_syncopation_asked_more_syncopation_played() {
        let style = style();
        let metric: Vec<f64> = (0..16).map(|s| metric_weight(s, 4, 4)).collect();
        let mean = |target: f64| {
            let mut sum = 0.0;
            for seed in 0..60 {
                let mut track = Track::new("SNARE", 4, 4);
                track.compose_syncopation = Some(target);
                track.compose_complexity = Some(60.0);
                drum_line(&mut track, &style, Macros::default(), None, 1.0, &mut Rng::new(seed));
                sum += syncopation(&steps_of(&track), &metric);
            }
            sum / 60.0
        };
        let (low, mid, high) = (mean(0.0), mean(50.0), mean(100.0));
        assert!(low < mid && mid < high, "{low} {mid} {high}");
    }

    #[test]
    fn the_settings_shown_are_the_style_ones_until_set() {
        let style = style();
        let mut track = Track::new("808 Kick", 4, 4);
        let shown = track_settings(&track, &style, Macros::default()).unwrap();
        assert_eq!((shown.part.as_str(), shown.used.hits_per_measure, shown.open_hits), ("KICK", 4.0, 15));
        track.compose_hits_per_measure = Some(40.0);
        assert_eq!(track_settings(&track, &style, Macros::default()).unwrap().used.hits_per_measure, 16.0);
        assert!(track_settings(&Track::new("CHH", 4, 4), &style, Macros::default()).is_none());
        // The pattern's macros over the track's settings.
        track.compose_hits_per_measure = Some(4.0);
        let macros = Macros { energy: 100.0, complexity: 0.0, variation: 0.0 };
        let shown = track_settings(&track, &style, macros).unwrap();
        assert_eq!((shown.base.hits_per_measure, shown.used.hits_per_measure), (4.0, 6.0));
        assert_eq!((shown.used.complexity, shown.used.variation), (0.0, 0.0));
    }
}
