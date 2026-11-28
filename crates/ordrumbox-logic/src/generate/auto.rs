//! Whole patterns and auto mode: a pattern generated, its lines drawn again
//! at each loop start, and the ♫ Drum / Bass / Chords buttons.
//!
//! A pattern remembers its generation in three saved keys: `autoGenre` (its
//! style, a key of `styles.json` or a tag the library taught), `autoKeyOffset`
//! (semitones) and `autoScale`. The drum lines come from the style
//! (`crate::compose`), the bass and chord lines from the melodic generators.
//! How a track in auto mode moves loop after loop is `auto_mode.rs`.

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::Value;
use ordrumbox_core::num::{clamp, round2};
use ordrumbox_core::{Groove, Macros, Pattern, Scales, Style, Styles, Track};

use super::melodic;
use super::structure::{Structure, random_key_offset, random_scale, section_harmony};
use crate::compose;

/// The pattern's style key (in `Pattern::extra`).
pub const STYLE_KEY: &str = "autoGenre";
/// The synth preset a generated bass track plays.
pub const BASS_PRESET: &str = "BASS_SQUARE";
const KEY_OFFSET: &str = "autoKeyOffset";
const SCALE: &str = "autoScale";
/// The seed a pattern's lines are drawn from (♫ draws a new one, the macros keep it).
const PATTERN_SEED: &str = "composeSeed";
/// Seeds are drawn below this (whole numbers a JSON file keeps exactly).
const SEED_RANGE: f64 = 4_294_967_296.0;
/// The ♫ buttons' own auto flag: it can stay on while `auto_mode` is off
/// (the button lit, the track not regenerating).
pub(super) const TOOLBAR_AUTO: &str = "_toolbarAuto";
/// The variant a melodic track plays when its style names none.
const BASS_VARIANT: &str = "basic";
const CHORD_VARIANT: &str = "chordStab";

/// Tonal centre and scale a melodic line plays in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Harmony {
    /// Semitones.
    pub root: f64,
    /// A scale name; `None`: the variant's own.
    pub scale: Option<String>,
}

/// What generates: the scales, the styles and the random source.
pub struct AutoGenerator<'a> {
    pub scales: &'a Scales,
    pub styles: &'a Styles,
    pub rnd: &'a mut dyn FnMut() -> f64,
    structure: Structure,
    /// The tempo drawn in the style of the last pattern made, for the
    /// library to play at: a pattern has none of its own.
    pub tempo: Option<f64>,
}

impl std::fmt::Debug for AutoGenerator<'_> {
    /// Everything but the random source, a closure.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AutoGenerator")
            .field("structure", &self.structure)
            .field("tempo", &self.tempo)
            .finish_non_exhaustive()
    }
}

impl<'a> AutoGenerator<'a> {
    pub fn new(scales: &'a Scales, styles: &'a Styles, rnd: &'a mut dyn FnMut() -> f64) -> Self {
        Self { scales, styles, rnd, structure: Structure::default(), tempo: None }
    }

    /// The song structure the generator plays through.
    pub(super) fn structure(&self) -> &Structure {
        &self.structure
    }

    pub(super) fn random(&mut self) -> f64 {
        (self.rnd)()
    }

    /// Draw a tempo in the range of `style`, whole BPM, into [`Self::tempo`].
    fn draw_tempo(&mut self, style: &Style) {
        let (low, high) = style.tempo;
        self.tempo = Some((low + self.random() * (high - low)).round());
    }

    /// The style `name` (through its aliases), or one with no part of that name.
    pub fn style(&self, name: &str) -> Style {
        self.styles.get(name).cloned().unwrap_or_else(|| Style::neutral(&name.to_lowercase()))
    }

    /// A style drawn among the loaded ones.
    fn random_style_name(&mut self) -> String {
        let names = self.styles.names();
        let i = (self.random() * names.len() as f64) as usize;
        names.get(i.min(names.len().saturating_sub(1))).map_or_else(String::new, |n| (*n).to_owned())
    }

    /// A random source for `compose`: the track's seed, else one draw.
    pub(super) fn line_rng(&mut self, track: &Track, salt: u64) -> ordrumbox_core::Rng {
        compose::line_rng(track, salt, self.rnd)
    }
}

/// A pattern-level key; null counts as missing.
fn pattern_value<'p>(pattern: &'p Pattern, key: &str) -> Option<&'p Value> {
    pattern.extra.get(key).filter(|v| !v.is_null())
}

pub(super) fn style_name_of(pattern: &Pattern) -> Option<String> {
    pattern_value(pattern, STYLE_KEY).and_then(Value::to_text).filter(|g| !g.is_empty())
}

/// The style of `pattern`: its own, else a drawn one (written to it).
pub(super) fn pattern_style(g: &mut AutoGenerator<'_>, pattern: &mut Pattern) -> Style {
    let name = match style_name_of(pattern) {
        Some(name) => name,
        None => {
            let name = g.random_style_name();
            pattern.extra.insert(STYLE_KEY, Value::from(name.as_str()));
            name
        }
    };
    g.style(&name)
}

/// The section harmony moved to the pattern's key, in its scale.
pub(super) fn pattern_harmony(pattern: &Pattern, style: &Style, section: &str, loop_in_element: u64) -> Harmony {
    let (root, scale) = section_harmony(&style.progression, section, loop_in_element);
    let offset = pattern_value(pattern, KEY_OFFSET).and_then(Value::to_number).unwrap_or(0.0);
    let scale = pattern_value(pattern, SCALE).and_then(Value::to_text).unwrap_or(scale);
    Harmony { root: ordrumbox_core::num::wrap(root + offset, 12.0), scale: Some(scale) }
}

/// A random density for a melodic line: bass near full, chords vary more.
pub(super) fn random_density(g: &mut AutoGenerator<'_>, track: &Track) -> f64 {
    let core = instrument_family(&track.name) == InstrumentFamily::Bass;
    let (min, max) = if core { (0.85, 1.05) } else { (0.6, 1.2) };
    round2(min + g.random() * (max - min))
}

/// The style's swing on a melodic track, ±0.08 when it swings at all.
fn apply_style_swing(g: &mut AutoGenerator<'_>, track: &mut Track, style: &Style) {
    if track.groove.swing_resolution().is_none() {
        return; // a groove of the user's own
    }
    let jitter = (g.random() * 2.0 - 1.0) * 0.08;
    let amount = if style.swing_amount > 0.0 { style.swing_amount + jitter } else { 0.0 };
    track.groove_amount = round2(clamp(amount, 0.0, 0.45));
    track.groove = if track.groove_amount > 0.0 { Groove::swing(style.swing_resolution) } else { Groove::default() };
}

/// The variation amounts of a generated track, when it has
/// none yet; once set they are the track's (Var Hits, Var Notes). A drum line
/// is a written rhythm, its variation an ornament; a bass or chord line plays
/// as written until its section ends.
pub(super) fn apply_variation(g: &mut AutoGenerator<'_>, track: &mut Track, kind: InstrumentFamily) {
    // A drum track draws its amount every time, so the lines after it draw
    // the same whether it had one or not.
    let draw = if kind.is_melodic() { 0.0 } else { g.random() };
    if track.auto_hit_variation.is_none() {
        let amount = if kind.is_melodic() { 0.0 } else { 12.0 + (draw * 14.0).floor() };
        track.auto_hit_variation = Some(amount);
    }
    if track.auto_note_variation.is_none() {
        track.auto_note_variation = Some(0.0);
    }
}

/// The variant a melodic track of `style` plays: its own `auto_variant`, else
/// one of the style's (the first when `draw` is off, so a line keeps its
/// character through the loops).
pub(super) fn melodic_variant(g: &mut AutoGenerator<'_>, style: &Style, track: &Track, draw: bool) -> String {
    if !track.auto_variant.is_empty() {
        return track.auto_variant.clone();
    }
    let kind = instrument_family(&track.name);
    let variants = style.melodic_variants(&track.name);
    let variants = if variants.is_empty() {
        let key = if kind == InstrumentFamily::Bass { "BASS" } else { "PIANO" };
        style.melodic_variants(key)
    } else {
        variants
    };
    let fallback = if kind == InstrumentFamily::Bass { BASS_VARIANT } else { CHORD_VARIANT };
    if variants.is_empty() {
        return fallback.to_owned();
    }
    let i = if draw { ((g.random() * variants.len() as f64) as usize).min(variants.len() - 1) } else { 0 };
    variants[i].clone()
}

/// A melodic track generated: a new line with variant `variant`,
/// after its swing and variation are picked.
pub fn generate_track(
    g: &mut AutoGenerator<'_>,
    pattern: &mut Pattern,
    t: usize,
    variant: &str,
    density: f64,
    harmony: &Harmony,
) {
    let style = g.style(&style_name_of(pattern).unwrap_or_default());
    let Some(kind) = pattern.tracks.get(t).map(|track| instrument_family(&track.name)) else { return };
    if kind.is_drum() {
        return drum_tracks(g, pattern, &style, &[t], 1.0);
    }
    apply_style_swing(g, &mut pattern.tracks[t], &style);
    apply_variation(g, &mut pattern.tracks[t], kind);
    instrument_line(g, pattern, t, kind, Some(variant), density, harmony);
}

/// The line of instrument `kind` for track `t` of `pattern`: a drum line
/// from the pattern's style, a bass or chord line from its generator
/// (PIANO and ORGAN as melodies), variant `variant`
/// (random when `None` or unknown) at `density`.
pub fn instrument_line(
    g: &mut AutoGenerator<'_>,
    pattern: &mut Pattern,
    t: usize,
    kind: InstrumentFamily,
    variant: Option<&str>,
    density: f64,
    harmony: &Harmony,
) {
    let scales = g.scales;
    match kind {
        InstrumentFamily::Piano | InstrumentFamily::Organ => {
            melodic::melody(pattern, t, variant, density, harmony, scales, g.rnd)
        }
        InstrumentFamily::Bass => {
            if let Some(track) = pattern.tracks.get_mut(t) {
                melodic::bass(track, variant, density, harmony, scales, g.rnd);
            }
        }
        _ => {
            let style = pattern_style(g, pattern);
            drum_tracks(g, pattern, &style, &[t], 1.0);
        }
    }
}

/// The track of `pattern` playing part `part` of `style`, other than `t`.
fn track_of_part(pattern: &Pattern, style: &Style, part: &str, t: usize) -> Option<usize> {
    let plays = |i: usize| style.part_for(&pattern.tracks[i].name).is_some_and(|(n, _)| n == part);
    (0..pattern.tracks.len()).find(|&i| i != t && plays(i))
}

/// The track a drum track of `style` couples to.
pub(super) fn partner_of(pattern: &Pattern, style: &Style, t: usize) -> Option<usize> {
    let (_, part) = style.part_for(&pattern.tracks[t].name)?;
    track_of_part(pattern, style, part.coupling_to.as_deref()?, t)
}

/// The order the drum tracks `tracks` are generated in: a track after the
/// one it couples to (kick and snare first otherwise).
fn generation_order(pattern: &Pattern, style: &Style, tracks: &[usize]) -> Vec<usize> {
    let rank = |t: usize| match instrument_family(&pattern.tracks[t].name) {
        InstrumentFamily::Kick => 0,
        InstrumentFamily::Snare => 1,
        _ => 2,
    };
    let mut left: Vec<usize> = tracks.to_vec();
    left.sort_by_key(|&t| rank(t));
    let mut order = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let ready =
            left.iter().position(|&t| partner_of(pattern, style, t).is_none_or(|p| !left.contains(&p))).unwrap_or(0);
        order.push(left.remove(ready));
    }
    order
}

/// New lines on the drum tracks `tracks` of `pattern` for `style` under the
/// pattern's macros, their hits scaled by `factor`; a track the style has no
/// part for is left empty, a locked track as it is.
pub(super) fn drum_tracks(
    g: &mut AutoGenerator<'_>,
    pattern: &mut Pattern,
    style: &Style,
    tracks: &[usize],
    factor: f64,
) {
    let macros = Macros::of(pattern);
    for t in generation_order(pattern, style, tracks) {
        if pattern.tracks[t].compose_lock {
            continue;
        }
        let partner = partner_of(pattern, style, t).map(|p| pattern.tracks[p].clone());
        let mut rng = g.line_rng(&pattern.tracks[t], 0);
        let track = &mut pattern.tracks[t];
        if style.part_for(&track.name).is_none() {
            track.notes.clear();
            continue;
        }
        compose::drum_line(track, style, macros, partner.as_ref(), factor, &mut rng);
        let kind = instrument_family(&track.name);
        apply_variation(g, &mut pattern.tracks[t], kind);
    }
}

/// Choices `generate_pattern` may be given instead of drawing them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    /// A style name; `None`: the pattern's own, else a drawn one.
    pub style: Option<String>,
    pub key_offset: Option<f64>,
    pub scale: Option<String>,
}

/// `generatePattern`: a style (given, the pattern's, or drawn), a key and
/// scale (kept when the pattern has them). An empty pattern gets the style's
/// tracks (a drum part sometimes left out, by its chance), meter and tempo
/// (in [`AutoGenerator::tempo`]);
/// an existing one new lines for its drum tracks (empty for those the style
/// has no part for) and for the melodic tracks the style names. A BASS
/// track (empty) is added when there is none.
pub fn generate_pattern(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, options: &Options) {
    let name = match options.style.clone().or_else(|| style_name_of(pattern)) {
        Some(name) => name,
        None => g.random_style_name(),
    };
    let style = g.style(&name);
    pattern.extra.insert(STYLE_KEY, Value::from(style.name.as_str()));
    match options.key_offset {
        Some(offset) => _ = pattern.extra.insert(KEY_OFFSET, Value::Number(offset)),
        None if pattern_value(pattern, KEY_OFFSET).is_some() => {}
        None => _ = pattern.extra.insert(KEY_OFFSET, Value::Number(random_key_offset(g.rnd))),
    }
    match &options.scale {
        Some(scale) => _ = pattern.extra.insert(SCALE, Value::from(scale.as_str())),
        None if pattern_value(pattern, SCALE).is_some() => {}
        None => _ = pattern.extra.insert(SCALE, Value::from(random_scale(g.rnd).as_str())),
    }
    let first = g.structure.element(0);
    let harmony = pattern_harmony(pattern, &style, &first.name, first.loop_in_element);

    let melodic: Vec<usize> = if pattern.tracks.is_empty() {
        pattern.beat_count = style.beat_count;
        g.draw_tempo(&style);
        for name in &style.tracks {
            if instrument_family(name).is_drum() {
                let chance = style.part_for(name).map_or(0.0, |(_, p)| p.chance);
                if chance < 1.0 && g.random() >= chance {
                    continue;
                }
                pattern.tracks.push(Track::new(name, style.beat_count, style.steps_per_beat));
            } else {
                pattern.tracks.push(Track::new(name, style.beat_count, 4));
            }
        }
        (0..pattern.tracks.len()).filter(|&t| instrument_family(&pattern.tracks[t].name).is_melodic()).collect()
    } else {
        let named = |t: &Track| !style.melodic_variants(&t.name).is_empty();
        (0..pattern.tracks.len())
            .filter(|&t| instrument_family(&pattern.tracks[t].name).is_melodic() && named(&pattern.tracks[t]))
            .collect()
    };
    let melodic: Vec<usize> = melodic.into_iter().filter(|&t| !pattern.tracks[t].compose_lock).collect();
    let energy = Macros::of(pattern).energy_factor();
    let drums: Vec<usize> =
        (0..pattern.tracks.len()).filter(|&t| instrument_family(&pattern.tracks[t].name).is_drum()).collect();
    drum_tracks(g, pattern, &style, &drums, 1.0);
    for t in melodic {
        let variant = melodic_variant(g, &style, &pattern.tracks[t], true);
        let density = random_density(g, &pattern.tracks[t]) * energy;
        generate_track(g, pattern, t, &variant, density, &harmony);
    }

    if !pattern.tracks.iter().any(|t| instrument_family(&t.name) == InstrumentFamily::Bass) {
        let mut bass = Track::new("BASS", pattern.beat_count, 4);
        bass.use_synth = false;
        bass.auto_assign = true;
        bass.synth_preset = Some(BASS_PRESET.into());
        bass.volume = 0.5;
        pattern.tracks.push(bass);
    }
}

/// ♫: the pattern made again in a style (given, the pattern's, or drawn) —
/// its meter, tempo (in [`AutoGenerator::tempo`]) and tracks: a track the style names is kept with its
/// sound and settings and gets a new line, the others go; a locked track
/// stays as it is, its line included.
pub fn restyle_pattern(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, options: &Options) {
    let name = match options.style.clone().or_else(|| style_name_of(pattern)) {
        Some(name) => name,
        None => g.random_style_name(),
    };
    let style = g.style(&name);
    let mut old = std::mem::take(&mut pattern.tracks);
    pattern.beat_count = style.beat_count;
    g.draw_tempo(&style);
    for name in &style.tracks {
        let drum = instrument_family(name).is_drum();
        let chance = if drum { style.part_for(name).map_or(0.0, |(_, p)| p.chance) } else { 1.0 };
        if chance < 1.0 && g.random() >= chance {
            continue;
        }
        let track = match old.iter().position(|t| t.name.eq_ignore_ascii_case(name)) {
            Some(i) => {
                let mut track = old.remove(i);
                if !track.compose_lock {
                    track.notes.clear();
                    track.beat_count = style.beat_count;
                }
                track
            }
            None => Track::new(name, style.beat_count, if drum { style.steps_per_beat } else { 4 }),
        };
        pattern.tracks.push(track);
    }
    pattern.tracks.extend(old.into_iter().filter(|t| t.compose_lock));
    // The key and scale drawn here: the seed then draws the lines only.
    if pattern_value(pattern, KEY_OFFSET).is_none() || options.key_offset.is_some() {
        let offset = options.key_offset.unwrap_or_else(|| random_key_offset(g.rnd));
        pattern.extra.insert(KEY_OFFSET, Value::Number(offset));
    }
    if pattern_value(pattern, SCALE).is_none() || options.scale.is_some() {
        let scale = options.scale.clone().unwrap_or_else(|| random_scale(g.rnd));
        pattern.extra.insert(SCALE, Value::from(scale.as_str()));
    }
    let seed = (g.random() * SEED_RANGE).floor();
    pattern.extra.insert(PATTERN_SEED, Value::Number(seed));
    let options = Options { style: Some(style.name.clone()), ..options.clone() };
    seeded_lines(g, pattern, &options);
}

/// The macros moved: the lines of `pattern` again from its seed, in its
/// style (a seed drawn first when it has none). The same settings make the
/// same lines.
pub fn recompose(g: &mut AutoGenerator<'_>, pattern: &mut Pattern) {
    if seed_of(pattern).is_none() {
        let seed = (g.random() * SEED_RANGE).floor();
        pattern.extra.insert(PATTERN_SEED, Value::Number(seed));
    }
    seeded_lines(g, pattern, &Options::default());
}

/// The key of `pattern`'s generated lines: semitones above the section's
/// root, 0 when it has none.
#[must_use]
pub fn pattern_key(pattern: &Pattern) -> f64 {
    pattern_value(pattern, KEY_OFFSET).and_then(Value::to_number).unwrap_or(0.0)
}

/// The scale of `pattern`'s generated lines, when it has one of its own (else
/// each section's).
#[must_use]
pub fn pattern_scale(pattern: &Pattern) -> Option<String> {
    pattern_value(pattern, SCALE).and_then(Value::to_text).filter(|s| !s.is_empty())
}

/// The key of `pattern` set to `offset` semitones, its lines made again
/// from its seed when it has generated ones.
pub fn set_key(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, offset: f64) {
    pattern.extra.insert(KEY_OFFSET, Value::Number(offset));
    redraw_seeded(g, pattern);
}

/// The scale of `pattern` set to `scale`, its lines made again from its
/// seed when it has generated ones.
pub fn set_scale(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, scale: &str) {
    pattern.extra.insert(SCALE, Value::from(scale));
    redraw_seeded(g, pattern);
}

/// The lines of a generated pattern (one with a seed) drawn again from it;
/// a pattern never generated keeps its notes.
fn redraw_seeded(g: &mut AutoGenerator<'_>, pattern: &mut Pattern) {
    if seed_of(pattern).is_some() {
        seeded_lines(g, pattern, &Options::default());
    }
}

/// The pattern's seed, if it has one: a whole number from 0.
fn seed_of(pattern: &Pattern) -> Option<u64> {
    pattern.extra.get(PATTERN_SEED).and_then(Value::as_f64).and_then(ordrumbox_core::num::whole)
}

/// `generate_pattern` drawing from the pattern's seed (its callers write
/// one first: 0 is never drawn from in practice).
fn seeded_lines(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, options: &Options) {
    let seed = seed_of(pattern).unwrap_or(0);
    let mut rng = ordrumbox_core::Rng::new(seed);
    let mut rnd = || rng.next_f64();
    generate_pattern(&mut AutoGenerator::new(g.scales, g.styles, &mut rnd), pattern, options);
}
