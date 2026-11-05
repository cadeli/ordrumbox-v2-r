//! Musical styles (`assets/data/styles.json`): what the drum generation of a
//! style plays — its meter, tempo, swing, tracks, and for each drum part a
//! weight per step of the measure with its required and forbidden steps and
//! the defaults of the track's compose settings.
//!
//! ```json
//! {"aliases": {"rap": "hiphop"},
//!  "styles": {"reggae": {"meter": {"stepsPerBeat": 4, "beatCount": 4}, "tempo": [70, 90],
//!    "parts": {"KICK": {"weights": [0, …], "required": [8], "hitsPerMeasure": 1}}}}}
//! ```

use crate::drum_taxonomy::instrument_family;
use crate::json::{Object, Value};
use crate::pattern::Pattern;
use crate::record::Issue;

/// The pattern keys of the generation macros, 0–100, 50 when unset: as the
/// style and the tracks say.
pub const ENERGY: &str = "composeEnergy";
pub const COMPLEXITY: &str = "composeComplexity";
pub const VARIATION: &str = "composeVariation";
/// The macro value that changes nothing.
pub const MACRO_NEUTRAL: f64 = 50.0;

/// The three generation macros of a pattern: they move every track at once,
/// over its own settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Macros {
    /// Hits and melodic density: ×0.5 at 0, ×1.5 at 100.
    pub energy: f64,
    /// Complexity and syncopation of every drum part: −100 at 0, +100 at 100.
    pub complexity: f64,
    /// Variation of the lines and of the play: none at 0, twice at 100.
    pub variation: f64,
}

impl Default for Macros {
    fn default() -> Self {
        Self { energy: MACRO_NEUTRAL, complexity: MACRO_NEUTRAL, variation: MACRO_NEUTRAL }
    }
}

impl Macros {
    /// The macros of `pattern` (a value that is not a number in 0–100 counts as unset).
    pub fn of(pattern: &Pattern) -> Self {
        let read = |key: &str| {
            pattern
                .extra
                .get(key)
                .and_then(Value::as_f64)
                .filter(|v| (0.0..=100.0).contains(v))
                .unwrap_or(MACRO_NEUTRAL)
        };
        Self { energy: read(ENERGY), complexity: read(COMPLEXITY), variation: read(VARIATION) }
    }

    /// What the hits and densities are multiplied by.
    pub fn energy_factor(self) -> f64 {
        0.5 + self.energy / 100.0
    }

    /// What is added to the complexity and syncopation (0–100 settings).
    pub fn complexity_shift(self) -> f64 {
        (self.complexity - MACRO_NEUTRAL) * 2.0
    }

    /// What the variation amounts are multiplied by.
    pub fn variation_factor(self) -> f64 {
        self.variation / MACRO_NEUTRAL
    }
}

/// A drum part of a style: how one track (KICK, SNARE, CHH…) plays.
#[derive(Debug, Clone, PartialEq)]
pub struct StylePart {
    /// One weight (0–1) per step of the style's measure: how typical a hit there is.
    pub weights: Vec<f64>,
    /// Steps of the measure always played.
    pub required: Vec<usize>,
    /// Steps of the measure never played.
    pub forbidden: Vec<usize>,
    /// Steps of the measure played accented.
    pub accents: Vec<usize>,
    /// Hits in a measure.
    pub hits_per_measure: f64,
    /// 0–100: how far the weights flatten toward the weak steps.
    pub complexity: f64,
    /// 0–100: how syncopated the line is, among the lines the weights allow.
    pub syncopation: f64,
    /// -100–100: avoids (−) or follows (+) the part `coupling_to`.
    pub coupling: f64,
    pub coupling_to: Option<String>,
    /// 0–100: hits moved from one loop to the next in auto mode.
    pub variation: f64,
    /// Share (0–1) of the weak-step hits played as ghost notes.
    pub ghost: f64,
    /// Chance (0–1) that a hit is a roll (a retriggered note).
    pub rolls: f64,
    /// Chance (0–1) that a new pattern of the style has this track.
    pub chance: f64,
}

impl Default for StylePart {
    fn default() -> Self {
        Self {
            weights: Vec::new(),
            required: Vec::new(),
            forbidden: Vec::new(),
            accents: Vec::new(),
            hits_per_measure: 4.0,
            complexity: 30.0,
            syncopation: 30.0,
            coupling: 0.0,
            coupling_to: None,
            variation: 15.0,
            ghost: 0.0,
            rolls: 0.0,
            chance: 1.0,
        }
    }
}

/// The fill a style plays on the last loop of a section.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    /// The part that fills (`SNARE`).
    pub part: String,
    /// Beats at the end of the measure the fill covers.
    pub beats: f64,
    /// How many times its usual hits the part plays in them.
    pub boost: f64,
}

impl Default for Fill {
    fn default() -> Self {
        Self { part: "SNARE".to_owned(), beats: 1.0, boost: 2.0 }
    }
}

/// A style: its meter and tempo, its tracks and drum parts.
#[derive(Debug, Clone, PartialEq)]
pub struct Style {
    /// The key in `styles.json`, or a tag of the library.
    pub name: String,
    pub steps_per_beat: i32,
    /// Beats in a measure.
    pub beat_count: i32,
    pub swing_amount: f64,
    pub swing_resolution: i32,
    /// Tempo range of a new pattern, BPM.
    pub tempo: (f64, f64),
    /// Track names of a new pattern, in order (drum parts and melodic tracks).
    pub tracks: Vec<String>,
    /// Drum parts by track name.
    pub parts: Vec<(String, StylePart)>,
    /// Variants of the melodic generators by track name (`BASS`, `PIANO`).
    pub melodic: Vec<(String, Vec<String>)>,
    /// Chord progression, semitones from the root, one step per loop.
    pub progression: Vec<f64>,
    pub fill: Fill,
    /// Learned from the library: the patterns and measures it rests on.
    pub learned: Option<(usize, usize)>,
}

/// Bounds of a style's meter: what a track's grid can hold.
const STEPS_PER_BEAT: (i32, i32) = (1, 8);
const BEAT_COUNT: (i32, i32) = (1, 16);

/// The parts of a style no file describes: hits on the plain meter.
fn neutral_parts() -> Vec<(String, StylePart)> {
    let part = |hits: f64, required: Vec<usize>, coupling_to: Option<&str>, coupling: f64| StylePart {
        required,
        hits_per_measure: hits,
        coupling_to: coupling_to.map(str::to_owned),
        coupling,
        ..StylePart::default()
    };
    vec![
        ("KICK".to_owned(), part(2.0, vec![0], None, 0.0)),
        ("SNARE".to_owned(), part(2.0, Vec::new(), Some("KICK"), -80.0)),
        ("CLAP".to_owned(), part(2.0, Vec::new(), Some("SNARE"), 100.0)),
        ("CHH".to_owned(), part(8.0, Vec::new(), None, 0.0)),
        ("COWBELL".to_owned(), part(2.0, Vec::new(), None, 0.0)),
        ("PERC".to_owned(), part(3.0, Vec::new(), Some("KICK"), -50.0)),
    ]
}

impl Style {
    /// A style no file describes (`styles.json` missing, or an unknown
    /// name): its parts follow the meter alone.
    pub fn neutral(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            steps_per_beat: 4,
            beat_count: 4,
            swing_amount: 0.0,
            swing_resolution: 4,
            tempo: (100.0, 130.0),
            tracks: ["KICK", "SNARE", "CHH", "BASS"].map(str::to_owned).to_vec(),
            parts: neutral_parts(),
            melodic: Vec::new(),
            progression: Vec::new(),
            fill: Fill::default(),
            learned: None,
        }
    }

    /// Steps in a measure of the style.
    pub fn measure_steps(&self) -> usize {
        (self.steps_per_beat * self.beat_count).max(1) as usize
    }

    /// The part track `name` plays: the part of that name, else the first of
    /// the same instrument family (`OPEN HAT` plays `OHH`, `808 KICK` `KICK`).
    pub fn part_for(&self, name: &str) -> Option<(&str, &StylePart)> {
        let upper = name.trim().to_uppercase();
        let exact = self.parts.iter().find(|(n, _)| *n == upper);
        let family = instrument_family(&upper);
        let same_family = || self.parts.iter().find(|(n, _)| instrument_family(n) == family);
        exact.or_else(same_family).map(|(n, p)| (n.as_str(), p))
    }

    /// The melodic variants of track `name` (empty when the style names none).
    pub fn melodic_variants(&self, name: &str) -> &[String] {
        let upper = name.to_uppercase();
        self.melodic.iter().find(|(n, _)| *n == upper).map_or(&[], |(_, v)| v.as_slice())
    }
}

/// The styles of `styles.json` and the names that stand for them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Styles {
    pub list: Vec<Style>,
    /// Another name → a style's name.
    pub aliases: Vec<(String, String)>,
}

impl Styles {
    /// Read `styles.json`. A value of the wrong type is reported with its path
    /// and takes its default; a style that is not an object is skipped.
    pub fn from_json(value: &Value) -> (Self, Vec<Issue>) {
        let mut issues = Vec::new();
        let mut styles = Self::default();
        let Some(root) = value.as_object() else {
            issues.push(Issue::new("", "not an object"));
            return (styles, issues);
        };
        if let Some(aliases) = object(root, "aliases", "", &mut issues) {
            for (alias, target) in aliases.iter() {
                match target.as_str() {
                    Some(target) => styles.aliases.push((alias.to_lowercase(), target.to_lowercase())),
                    None => issues.push(Issue::new(format!("aliases.{alias}"), "not a style name")),
                }
            }
        }
        if let Some(list) = object(root, "styles", "", &mut issues) {
            for (name, style) in list.iter() {
                let at = format!("styles.{name}");
                match style.as_object() {
                    Some(style) => styles.list.push(read_style(&name.to_lowercase(), style, &at, &mut issues)),
                    None => issues.push(Issue::new(at, "not an object, skipped")),
                }
            }
        }
        (styles, issues)
    }

    /// The style `name` (any case) or the one it is an alias of.
    pub fn get(&self, name: &str) -> Option<&Style> {
        let name = name.to_lowercase();
        let target = self.aliases.iter().find(|(a, _)| *a == name).map_or(name.as_str(), |(_, t)| t.as_str());
        self.list.iter().find(|s| s.name == target)
    }

    pub fn names(&self) -> Vec<&str> {
        self.list.iter().map(|s| s.name.as_str()).collect()
    }
}

fn object<'a>(parent: &'a Object, key: &str, at: &str, issues: &mut Vec<Issue>) -> Option<&'a Object> {
    let value = parent.get(key)?;
    let found = value.as_object();
    if found.is_none() {
        issues.push(Issue::new(join(at, key), "not an object"));
    }
    found
}

fn join(at: &str, key: &str) -> String {
    if at.is_empty() { key.to_owned() } else { format!("{at}.{key}") }
}

/// Number `key` of `parent` within `min..=max`, else `default` and an issue.
fn number(parent: &Object, key: &str, at: &str, range: (f64, f64), default: f64, issues: &mut Vec<Issue>) -> f64 {
    let Some(value) = parent.get(key) else { return default };
    match value.as_f64().filter(|n| n.is_finite()) {
        Some(n) if (range.0..=range.1).contains(&n) => n,
        Some(n) => {
            issues.push(Issue::new(join(at, key), format!("{n} is outside {}–{}, {default} used", range.0, range.1)));
            default
        }
        None => {
            issues.push(Issue::new(join(at, key), format!("not a number, {default} used")));
            default
        }
    }
}

fn integer(parent: &Object, key: &str, at: &str, range: (i32, i32), default: i32, issues: &mut Vec<Issue>) -> i32 {
    let n = number(parent, key, at, (f64::from(range.0), f64::from(range.1)), f64::from(default), issues);
    if n.fract() == 0.0 {
        n as i32
    } else {
        issues.push(Issue::new(join(at, key), format!("not a whole number, {default} used")));
        default
    }
}

fn text(parent: &Object, key: &str, at: &str, issues: &mut Vec<Issue>) -> Option<String> {
    let value = parent.get(key)?;
    let found = value.as_str().map(str::to_owned);
    if found.is_none() {
        issues.push(Issue::new(join(at, key), "not a string"));
    }
    found
}

/// The items of list `key` that `item` reads; the others are reported.
fn list<T>(
    parent: &Object,
    key: &str,
    at: &str,
    issues: &mut Vec<Issue>,
    item: impl Fn(&Value) -> Option<T>,
) -> Vec<T> {
    let Some(value) = parent.get(key) else { return Vec::new() };
    let Some(items) = value.as_array() else {
        issues.push(Issue::new(join(at, key), "not a list"));
        return Vec::new();
    };
    let mut out = Vec::with_capacity(items.len());
    for (i, v) in items.iter().enumerate() {
        match item(v) {
            Some(x) => out.push(x),
            None => issues.push(Issue::new(format!("{}[{i}]", join(at, key)), "invalid item, skipped")),
        }
    }
    out
}

fn step_index(value: &Value) -> Option<usize> {
    value.as_f64().filter(|n| n.fract() == 0.0 && *n >= 0.0).map(|n| n as usize)
}

/// Steps of `key` within the measure; the others are reported.
fn steps(parent: &Object, key: &str, at: &str, measure: usize, issues: &mut Vec<Issue>) -> Vec<usize> {
    let all = list(parent, key, at, issues, step_index);
    let (inside, outside): (Vec<usize>, Vec<usize>) = all.into_iter().partition(|&s| s < measure);
    if !outside.is_empty() {
        issues.push(Issue::new(join(at, key), format!("steps {outside:?} are past the measure ({measure}), skipped")));
    }
    inside
}

fn read_part(part: &Object, at: &str, measure: usize, issues: &mut Vec<Issue>) -> StylePart {
    let d = StylePart::default();
    let weight = |v: &Value| v.as_f64().filter(|n| (0.0..=1.0).contains(n));
    let mut weights = list(part, "weights", at, issues, weight);
    if !weights.is_empty() && weights.len() != measure {
        issues.push(Issue::new(
            join(at, "weights"),
            format!("{} weights for a measure of {measure} steps, cut or padded with 0", weights.len()),
        ));
    }
    if !weights.is_empty() {
        weights.resize(measure, 0.0);
    }
    StylePart {
        weights,
        required: steps(part, "required", at, measure, issues),
        forbidden: steps(part, "forbidden", at, measure, issues),
        accents: steps(part, "accents", at, measure, issues),
        hits_per_measure: number(part, "hitsPerMeasure", at, (0.0, measure as f64), d.hits_per_measure, issues),
        complexity: number(part, "complexity", at, (0.0, 100.0), d.complexity, issues),
        syncopation: number(part, "syncopation", at, (0.0, 100.0), d.syncopation, issues),
        coupling: number(part, "coupling", at, (-100.0, 100.0), d.coupling, issues),
        coupling_to: text(part, "couplingTo", at, issues).map(|t| t.to_uppercase()),
        variation: number(part, "variation", at, (0.0, 100.0), d.variation, issues),
        ghost: number(part, "ghost", at, (0.0, 1.0), d.ghost, issues),
        rolls: number(part, "rolls", at, (0.0, 1.0), d.rolls, issues),
        chance: number(part, "chance", at, (0.0, 1.0), d.chance, issues),
    }
}

fn read_style(name: &str, style: &Object, at: &str, issues: &mut Vec<Issue>) -> Style {
    let mut s = Style::neutral(name);
    if let Some(meter) = object(style, "meter", at, issues) {
        let at = join(at, "meter");
        s.steps_per_beat = integer(meter, "stepsPerBeat", &at, STEPS_PER_BEAT, s.steps_per_beat, issues);
        s.beat_count = integer(meter, "beatCount", &at, BEAT_COUNT, s.beat_count, issues);
    }
    if let Some(swing) = object(style, "swing", at, issues) {
        let at = join(at, "swing");
        s.swing_amount = number(swing, "amount", &at, (0.0, 1.0), s.swing_amount, issues);
        s.swing_resolution = integer(swing, "resolution", &at, STEPS_PER_BEAT, s.swing_resolution, issues);
    }
    let tempo = list(style, "tempo", at, issues, |v| v.as_f64().filter(|n| (20.0..=300.0).contains(n)));
    match tempo.as_slice() {
        [] => {}
        [low, high] if low <= high => s.tempo = (*low, *high),
        _ => issues.push(Issue::new(join(at, "tempo"), "not a [low, high] BPM range, 100–130 used")),
    }
    if style.get("tracks").is_some() {
        s.tracks = list(style, "tracks", at, issues, |v| v.as_str().map(str::to_uppercase));
    }
    let measure = s.measure_steps();
    // A written style plays its own parts only.
    s.parts.clear();
    if let Some(parts) = object(style, "parts", at, issues) {
        for (part_name, part) in parts.iter() {
            let at = format!("{}.{part_name}", join(at, "parts"));
            match part.as_object() {
                Some(part) => s.parts.push((part_name.to_uppercase(), read_part(part, &at, measure, issues))),
                None => issues.push(Issue::new(at, "not an object, skipped")),
            }
        }
    }
    if let Some(melodic) = object(style, "melodic", at, issues) {
        let at = join(at, "melodic");
        for (track, _) in melodic.iter() {
            let variants = list(melodic, track, &at, issues, |v| v.as_str().map(str::to_owned));
            s.melodic.push((track.to_uppercase(), variants));
        }
    }
    s.progression = list(style, "progression", at, issues, |v| v.as_f64().filter(|n| n.is_finite()));
    if let Some(fill) = object(style, "fill", at, issues) {
        let at = join(at, "fill");
        if let Some(part) = text(fill, "part", &at, issues) {
            s.fill.part = part.to_uppercase();
        }
        s.fill.beats = number(fill, "beats", &at, (0.0, f64::from(s.beat_count)), s.fill.beats, issues);
        s.fill.boost = number(fill, "boost", &at, (0.0, 8.0), s.fill.boost, issues);
    }
    s
}

/// The weight, required, forbidden and accent flags of step `step` of a
/// measure of `steps` steps, read on the style's grid (`None` off its grid:
/// a ternary style read on a binary track, say).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepRead {
    pub weight: f64,
    pub required: bool,
    pub forbidden: bool,
    pub accent: bool,
}

impl StylePart {
    /// Step `step` of a measure of `steps` steps on the part's grid of
    /// `grid` steps; `None` when it falls between two steps of that grid.
    pub fn read(&self, step: usize, steps: usize, grid: usize) -> Option<StepRead> {
        let numerator = step * grid;
        if steps == 0 || !numerator.is_multiple_of(steps) {
            return None;
        }
        let index = numerator / steps;
        Some(StepRead {
            weight: self.weights.get(index).copied().unwrap_or(0.0),
            required: self.required.contains(&index),
            forbidden: self.forbidden.contains(&index),
            accent: self.accents.contains(&index),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json;

    fn parse(text: &str) -> (Styles, Vec<Issue>) {
        Styles::from_json(&json::parse(text).unwrap())
    }

    #[test]
    fn a_style_reads_with_its_parts_and_aliases() {
        let (styles, issues) = parse(
            r#"{"aliases":{"Ragga":"reggae"},"styles":{"reggae":{"meter":{"stepsPerBeat":4,"beatCount":4},
            "tempo":[70,90],"parts":{"kick":{"weights":[0,0,0,0,0,0,0,0,1,0,0,0,0,0,0,0],"required":[8],
            "forbidden":[0],"hitsPerMeasure":1}},"melodic":{"bass":["groove"]}}}}"#,
        );
        assert_eq!(issues, []);
        let reggae = styles.get("RAGGA").unwrap();
        assert_eq!(reggae.tempo, (70.0, 90.0));
        let (name, kick) = reggae.part_for("808 Kick").unwrap();
        assert_eq!(name, "KICK");
        assert_eq!((kick.required.as_slice(), kick.forbidden.as_slice()), ([8].as_slice(), [0].as_slice()));
        assert_eq!(reggae.melodic_variants("BASS"), ["groove"]);
        assert!(reggae.part_for("SNARE").is_none());
    }

    #[test]
    fn a_wrong_value_is_reported_with_its_path_and_takes_its_default() {
        let (styles, issues) = parse(
            r#"{"styles":{"x":{"meter":{"stepsPerBeat":12},"parts":{"KICK":{"hitsPerMeasure":"four",
            "weights":[1,2],"required":[3,40]}}},"y":3}}"#,
        );
        let at: Vec<&str> = issues.iter().map(|i| i.at.as_str()).collect();
        assert_eq!(
            at,
            [
                "styles.x.meter.stepsPerBeat",
                "styles.x.parts.KICK.weights[1]",
                "styles.x.parts.KICK.weights",
                "styles.x.parts.KICK.required",
                "styles.x.parts.KICK.hitsPerMeasure",
                "styles.y"
            ]
        );
        let x = styles.get("x").unwrap();
        assert_eq!(x.steps_per_beat, 4);
        let kick = &x.parts[0].1;
        assert_eq!(kick.hits_per_measure, StylePart::default().hits_per_measure);
        assert_eq!(kick.weights.len(), 16);
        assert_eq!(kick.required, [3]);
        assert!(styles.get("y").is_none());
    }

    #[test]
    fn a_step_reads_on_the_part_grid() {
        let part = StylePart { weights: vec![1.0, 0.0, 0.5], required: vec![2], ..StylePart::default() };
        // A grid of 3 read on 6 steps: every other step is between two of it.
        assert_eq!(part.read(4, 6, 3).map(|r| (r.weight, r.required)), Some((0.5, true)));
        assert_eq!(part.read(1, 6, 3), None);
        assert_eq!(part.read(0, 3, 3).map(|r| r.weight), Some(1.0));
    }
}
