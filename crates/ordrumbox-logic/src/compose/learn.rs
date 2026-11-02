//! Styles learned from the library: the patterns tagged with a style's name
//! (or with any other tag) tell how often each drum part plays each step of
//! the measure.
//!
//! A tag of a written style blends into it, in proportion to the measures
//! seen (`LEARNING_MEASURES` of them count as much as the written profile); a
//! tag of no style becomes a style of its own. Fewer than `MIN_MEASURES`
//! measures teach nothing.

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::{Pattern, Style, StylePart, Styles, Track};

/// Measures a learned style needs at least.
pub const MIN_MEASURES: usize = 4;
/// Measures after which the learned weights count as much as the written ones.
const LEARNING_MEASURES: f64 = 8.0;
/// A step played in this share of the measures is required.
const REQUIRED_SHARE: f64 = 0.9;
/// Below this velocity a hit counts as a ghost note.
const GHOST_VELOCITY: f64 = 0.5;

/// What the tracks of one part taught.
#[derive(Debug, Clone, Default)]
struct PartStats {
    /// Measures in which each step was played.
    counts: Vec<usize>,
    /// Hits of each measure seen.
    per_measure: Vec<usize>,
    hits: usize,
    ghosts: usize,
}

impl PartStats {
    fn measures(&self) -> usize {
        self.per_measure.len()
    }

    fn weights(&self) -> Vec<f64> {
        let m = self.measures().max(1) as f64;
        self.counts.iter().map(|&c| (c as f64 / m).min(1.0)).collect()
    }

    fn median_hits(&self) -> f64 {
        let mut hits = self.per_measure.clone();
        hits.sort_unstable();
        hits.get(hits.len() / 2).map_or(0.0, |&h| h as f64)
    }

    /// The part this teaches alone (the forbidden steps those never played).
    fn part(&self) -> StylePart {
        let weights = self.weights();
        let required = steps_where(&weights, |w| w >= REQUIRED_SHARE);
        StylePart {
            forbidden: steps_where(&weights, |w| w == 0.0),
            accents: required.clone(),
            required,
            weights,
            hits_per_measure: self.median_hits(),
            ghost: if self.hits == 0 { 0.0 } else { self.ghosts as f64 / self.hits as f64 },
            ..StylePart::default()
        }
    }
}

fn steps_where(weights: &[f64], test: impl Fn(f64) -> bool) -> Vec<usize> {
    weights.iter().enumerate().filter(|(_, w)| test(**w)).map(|(i, _)| i).collect()
}

/// Read the drum tracks of `patterns` on a grid of `steps_per_beat` and
/// measures of `beat_count` beats; parts by track name.
fn stats(patterns: &[&Pattern], steps_per_beat: usize, beat_count: usize) -> Vec<(String, PartStats)> {
    let measure = steps_per_beat * beat_count;
    let mut parts: Vec<(String, PartStats)> = Vec::new();
    for track in patterns.iter().flat_map(|p| &p.tracks).filter(|t| instrument_family(&t.name).is_drum()) {
        let tspb = track.steps_per_beat.max(1) as usize;
        let measures = track.beat_count.max(0) as usize / beat_count;
        if measures == 0 {
            continue;
        }
        let name = track.name.trim().to_uppercase();
        let slot = match parts.iter().position(|(n, _)| *n == name) {
            Some(i) => i,
            None => {
                parts.push((name, PartStats { counts: vec![0; measure], ..PartStats::default() }));
                parts.len() - 1
            }
        };
        let stats = &mut parts[slot].1;
        let mut played = vec![vec![false; measure]; measures];
        for note in &track.notes {
            let at = note.beat.max(0) as usize * tspb + note.step.max(0) as usize;
            // On the learning grid, or not counted (a ternary hit on a binary grid).
            if (at * steps_per_beat) % tspb != 0 {
                continue;
            }
            let reference = at * steps_per_beat / tspb;
            let m = reference / measure;
            if m < measures && !played[m][reference % measure] {
                played[m][reference % measure] = true;
                stats.hits += 1;
                if note.velocity < GHOST_VELOCITY {
                    stats.ghosts += 1;
                }
            }
        }
        for measure_played in played {
            for (s, &p) in measure_played.iter().enumerate() {
                stats.counts[s] += usize::from(p);
            }
            stats.per_measure.push(measure_played.iter().filter(|&&p| p).count());
        }
    }
    parts
}

/// The usual value: the most frequent grid of the drum tracks.
fn usual_steps_per_beat(patterns: &[&Pattern]) -> i32 {
    let mut counts = [0usize; 9];
    for track in patterns.iter().flat_map(|p| &p.tracks).filter(|t| instrument_family(&t.name).is_drum()) {
        counts[track.steps_per_beat.clamp(1, 8) as usize] += 1;
    }
    (1..=8).max_by_key(|&s| (counts[s], s == 4)).unwrap_or(4) as i32
}

fn drum_tracks<'a>(patterns: &'a [&Pattern]) -> impl Iterator<Item = &'a Track> {
    patterns.iter().flat_map(|p| &p.tracks).filter(|t| instrument_family(&t.name).is_drum())
}

/// Order of the parts in a learned style's track list: kick, snare, the others.
fn rank(name: &str) -> u8 {
    match instrument_family(name) {
        InstrumentFamily::Kick => 0,
        InstrumentFamily::Snare => 1,
        InstrumentFamily::Clap => 2,
        InstrumentFamily::Hat => 3,
        _ => 4,
    }
}

/// `written` with the library's lessons: each tag of `patterns` (its style
/// through the aliases) blended into its written style, or a style of its own.
pub fn learn(written: &Styles, patterns: &[Pattern]) -> Styles {
    let mut learned = written.clone();
    let mut tags: Vec<String> = Vec::new();
    for tag in patterns.iter().flat_map(|p| &p.tags) {
        let tag = tag.trim().to_lowercase();
        let name = written.get(&tag).map_or(tag, |s| s.name.clone());
        if !name.is_empty() && !tags.contains(&name) {
            tags.push(name);
        }
    }
    for name in tags {
        let tagged: Vec<&Pattern> = patterns
            .iter()
            .filter(|p| {
                p.tags.iter().any(|t| written.get(t.trim()).map_or(t.trim().to_lowercase(), |s| s.name.clone()) == name)
            })
            .collect();
        match learned.list.iter_mut().find(|s| s.name == name) {
            Some(style) => blend(style, &tagged),
            None => {
                if let Some(style) = new_style(&name, &tagged) {
                    learned.list.push(style);
                }
            }
        }
    }
    learned
}

/// A style learned from `patterns` alone, if they hold enough measures.
fn new_style(name: &str, patterns: &[&Pattern]) -> Option<Style> {
    let spb = usual_steps_per_beat(patterns);
    let mut style = Style::neutral(name);
    style.steps_per_beat = spb;
    let parts = stats(patterns, spb as usize, style.beat_count as usize);
    let measures = parts.iter().map(|(_, s)| s.measures()).max().unwrap_or(0);
    if measures < MIN_MEASURES {
        return None;
    }
    let mut parts: Vec<(String, StylePart)> = parts.into_iter().map(|(n, s)| (n, s.part())).collect();
    parts.sort_by_key(|(n, _)| rank(n));
    style.tracks = parts.iter().map(|(n, _)| n.clone()).collect();
    if patterns.iter().flat_map(|p| &p.tracks).any(|t| instrument_family(&t.name) == InstrumentFamily::Bass) {
        style.tracks.push("BASS".to_owned());
    }
    style.parts = parts;
    let drums: Vec<&Track> = drum_tracks(patterns).collect();
    style.swing_amount = drums.iter().map(|t| t.swing_amount).sum::<f64>() / drums.len().max(1) as f64;
    if let Some(t) = drums.first() {
        style.swing_resolution = t.swing_resolution;
    }
    let tempos = patterns.iter().map(|p| p.bpm).filter(|b| b.is_finite() && *b > 0.0);
    let (low, high) = tempos.fold((f64::INFINITY, 0.0_f64), |(l, h), b| (l.min(b), h.max(b)));
    if low <= high {
        style.tempo = (low, high);
    }
    style.fill.part = style.parts.iter().find(|(n, _)| rank(n) == 1).map_or(style.fill.part, |(n, _)| n.clone());
    style.learned = Some((patterns.len(), measures));
    Some(style)
}

/// `style` with what `patterns` teach on its own grid: the weights and hits
/// of each part moved toward the library's by the share of the measures seen.
fn blend(style: &mut Style, patterns: &[&Pattern]) {
    let parts = stats(patterns, style.steps_per_beat.max(1) as usize, style.beat_count.max(1) as usize);
    let measures = parts.iter().map(|(_, s)| s.measures()).max().unwrap_or(0);
    if measures < MIN_MEASURES {
        return;
    }
    for (name, stats) in parts {
        let lesson = stats.part();
        let share = stats.measures() as f64 / (stats.measures() as f64 + LEARNING_MEASURES);
        match style.parts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, part)) => {
                let written =
                    if part.weights.is_empty() { vec![0.5; lesson.weights.len()] } else { part.weights.clone() };
                part.weights = written.iter().zip(&lesson.weights).map(|(w, l)| w + share * (l - w)).collect();
                part.hits_per_measure =
                    (part.hits_per_measure + share * (lesson.hits_per_measure - part.hits_per_measure)).round();
                if share >= 0.5 {
                    part.required = lesson.required;
                }
            }
            None => {
                // A part the written style lacks: learned as is, but nothing forbidden.
                style.parts.push((name.clone(), StylePart { forbidden: Vec::new(), ..lesson }));
                style.tracks.push(name);
            }
        }
    }
    let swing =
        drum_tracks(patterns).map(|t| t.swing_amount).sum::<f64>() / drum_tracks(patterns).count().max(1) as f64;
    let share = measures as f64 / (measures as f64 + LEARNING_MEASURES);
    style.swing_amount += share * (swing - style.swing_amount);
    style.learned = Some((patterns.len(), measures));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Note;

    fn pattern(tag: &str, kick_steps: &[i32], beats: i32) -> Pattern {
        let mut kick = Track::new("KICK", beats, 4);
        for m in 0..beats / 4 {
            for &s in kick_steps {
                kick.notes.push(Note { beat: m * 4 + s / 4, step: s % 4, velocity: 0.9, ..Note::default() });
            }
        }
        Pattern { tags: vec![tag.to_owned()], tracks: vec![kick], beat_count: beats, bpm: 100.0, ..Pattern::default() }
    }

    #[test]
    fn a_tag_of_no_style_teaches_its_steps() {
        let patterns = vec![pattern("mine", &[0, 6, 10], 8), pattern("Mine", &[0, 6, 10], 8)];
        let styles = learn(&Styles::default(), &patterns);
        let mine = styles.get("mine").unwrap();
        assert_eq!(mine.learned, Some((2, 4)));
        let (_, kick) = mine.part_for("KICK").unwrap();
        assert_eq!(kick.required, [0, 6, 10]);
        assert_eq!(kick.hits_per_measure, 3.0);
        assert_eq!(kick.forbidden.len(), 13);
    }

    #[test]
    fn under_four_measures_nothing_is_learned() {
        let patterns = vec![pattern("mine", &[0], 8)];
        assert!(learn(&Styles::default(), &patterns).get("mine").is_none());
        let mut written = Styles::default();
        let mut style = Style::neutral("house");
        style.parts = vec![("KICK".into(), StylePart { weights: vec![0.2; 16], ..StylePart::default() })];
        written.list.push(style);
        let patterns = vec![pattern("house", &[3], 8)];
        assert_eq!(learn(&written, &patterns), written);
    }

    #[test]
    fn a_written_style_moves_toward_its_tagged_patterns() {
        let mut written = Styles::default();
        let mut style = Style::neutral("house");
        style.parts = vec![("KICK".into(), StylePart { weights: vec![0.0; 16], ..StylePart::default() })];
        written.list.push(style);
        written.aliases.push(("deep".into(), "house".into()));
        let patterns = vec![pattern("deep", &[3], 16), pattern("other", &[5], 4)];
        let styles = learn(&written, &patterns);
        let house = styles.get("house").unwrap();
        assert_eq!(house.learned, Some((1, 4)));
        let (_, kick) = house.part_for("KICK").unwrap();
        // 4 measures against 8: a third of the way.
        assert!((kick.weights[3] - 1.0 / 3.0).abs() < 1e-9, "{}", kick.weights[3]);
        assert_eq!(kick.weights[5], 0.0);
    }
}
