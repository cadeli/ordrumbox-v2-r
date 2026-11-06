//! How a drum line moves in auto mode: from one loop to the next a few hits
//! move to steps of comparable weight, so the line changes and stays in its
//! style; on the last loop of a section the fill part plays a fill.

use ordrumbox_core::num::round2;
use ordrumbox_core::{Macros, Rng, Style, Track};

use super::{Settings, add_hit, sample, steps, track_steps, velocity};

/// How much a move prefers a step as strong as the one it leaves.
const SAME_STRENGTH: f64 = 4.0;

fn onset(track: &Track, beat: i32, step: i32) -> usize {
    (beat.max(0) * track.steps_per_beat + step.max(0)) as usize
}

/// Move `variation`% of the hits of `track` (its part of `style`, coupled to
/// `partner`) to free steps of comparable weight; the count stays. Whether
/// any moved.
pub fn vary_line(track: &mut Track, style: &Style, macros: Macros, partner: Option<&Track>, rng: &mut Rng) -> bool {
    let Some((_, part)) = style.part_for(&track.name) else { return false };
    let part = part.clone();
    let probe = Settings::of(track, &part, usize::MAX, macros);
    let steps = steps(style, &part, probe, track.steps_per_beat);
    let settings = Settings::of(track, &part, steps.len(), macros);
    let moves = (settings.variation / 100.0 * track.notes.len() as f64).round() as usize;
    if moves == 0 || steps.is_empty() {
        return false;
    }
    let (spb, total, n) = (track.steps_per_beat.max(1) as usize, track_steps(track), steps.len());
    let (weights, _) = super::line_weights(&steps, total, spb, settings.coupling, partner);
    let mut moved = false;
    for _ in 0..moves {
        let movable: Vec<usize> = (0..track.notes.len())
            .filter(|&i| {
                let a = onset(track, track.notes[i].beat, track.notes[i].step);
                a < total && !steps[a % n].required
            })
            .collect();
        let Some(&i) = movable.get((rng.next_f64() * movable.len() as f64) as usize) else { break };
        let from = onset(track, track.notes[i].beat, track.notes[i].step);
        let taken: Vec<usize> = track.notes.iter().map(|n| onset(track, n.beat, n.step)).collect();
        let strength = steps[from % n].metric;
        let choices: Vec<f64> = (0..total)
            .map(|a| {
                if taken.contains(&a) {
                    0.0
                } else {
                    weights[a] * (-SAME_STRENGTH * (steps[a % n].metric - strength).abs()).exp()
                }
            })
            .collect();
        let Some(to) = sample::pick(&choices, rng) else { continue };
        let note = &mut track.notes[i];
        note.beat = (to / spb) as i32;
        note.step = (to % spb) as i32;
        note.step_percent = Some((note.step as f64 * 100.0 / spb as f64).round());
        note.velocity = velocity(&steps[to % n], &part, rng);
        moved = true;
    }
    moved
}

/// The fill of `style` on `track` when it is the style's fill part: over the
/// last beats of the line, its usual hits times the fill's boost (4 at least
/// a measure), louder toward the end. Whether any hit was added.
pub fn fill(track: &mut Track, style: &Style, macros: Macros, rng: &mut Rng) -> bool {
    let Some((name, part)) = style.part_for(&track.name) else { return false };
    if name != style.fill.part || style.fill.beats <= 0.0 {
        return false;
    }
    let part = part.clone();
    let probe = Settings::of(track, &part, usize::MAX, macros);
    let steps = steps(style, &part, probe, track.steps_per_beat);
    let settings = Settings::of(track, &part, steps.len(), macros);
    let (spb, total, n) = (track.steps_per_beat.max(1) as usize, track_steps(track), steps.len());
    let len = ((style.fill.beats * spb as f64).round() as usize).min(total);
    let start = total - len;
    let taken: Vec<usize> = track.notes.iter().map(|n| onset(track, n.beat, n.step)).collect();
    let present = taken.iter().filter(|&&a| a >= start && a < total).count();
    let target = (style.fill.boost * settings.hits_per_measure.max(4.0) * len as f64 / n as f64).round() as usize;
    let extra = target.saturating_sub(present);
    let weights: Vec<f64> = (start..total)
        .map(|a| if taken.contains(&a) || steps[a % n].forbidden { 0.0 } else { steps[a % n].metric.powf(0.3) })
        .collect();
    let added = sample::take(&weights, extra, rng);
    for &i in &added {
        let a = start + i;
        add_hit(track, a, &steps[a % n], &part, rng);
        if let Some(note) = track.notes.last_mut() {
            // A crescendo over the fill.
            note.velocity = round2(0.6 + 0.4 * (i + 1) as f64 / len as f64);
        }
    }
    !added.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::drum_line;
    use ordrumbox_core::StylePart;

    fn style() -> Style {
        let mut style = Style::neutral("test");
        let snare = StylePart {
            weights: (0..16).map(|s| if s == 4 || s == 12 { 1.0 } else { 0.3 }).collect(),
            required: vec![4, 12],
            hits_per_measure: 5.0,
            ..StylePart::default()
        };
        style.parts = vec![("SNARE".into(), snare)];
        style
    }

    #[test]
    fn no_variation_no_move_and_a_move_keeps_the_count_and_the_required_steps() {
        let style = style();
        for seed in 0..50 {
            let mut track = Track::new("SNARE", 4, 4);
            drum_line(&mut track, &style, Macros::default(), None, 1.0, &mut Rng::new(seed));
            let line = track.notes.clone();
            track.compose_variation = Some(0.0);
            assert!(!vary_line(&mut track, &style, Macros::default(), None, &mut Rng::new(seed)));
            assert_eq!(track.notes, line);
            track.compose_variation = Some(100.0);
            vary_line(&mut track, &style, Macros::default(), None, &mut Rng::new(seed));
            assert_eq!(track.notes.len(), 5);
            let steps: Vec<i32> = track.notes.iter().map(|n| n.beat * 4 + n.step).collect();
            assert!(steps.contains(&4) && steps.contains(&12), "{steps:?}");
            let mut unique = steps.clone();
            unique.sort_unstable();
            unique.dedup();
            assert_eq!(unique.len(), 5);
        }
    }

    #[test]
    fn the_fill_part_fills_its_last_beat() {
        let style = style();
        let mut track = Track::new("SNARE", 4, 4);
        drum_line(&mut track, &style, Macros::default(), None, 1.0, &mut Rng::new(3));
        assert!(fill(&mut track, &style, Macros::default(), &mut Rng::new(4)));
        let last_beat = track.notes.iter().filter(|n| n.beat == 3).count();
        assert!(last_beat >= 3, "{last_beat}");
        let mut hat = Track::new("CHH", 4, 4);
        assert!(!fill(&mut hat, &style, Macros::default(), &mut Rng::new(4)));
    }
}
