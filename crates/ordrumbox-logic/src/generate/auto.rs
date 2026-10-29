//! Whole patterns and auto mode (`auto_generator.js`, the loop start of
//! `audio/player.js`, the ↻ Drum / Bass / Chords buttons of
//! `pattern_auto_gen.js`, and `random_generator.js`).
//!
//! A pattern remembers its generation in three saved keys: `_autoGenGenre`,
//! `_autoGenKeyOffset` (semitones) and `_autoGenScale`. A track in auto mode
//! (`auto`) gets a new line at every loop start; a pattern with `autoGen`
//! regenerates its melodic tracks every loop and all of them at section
//! boundaries.

use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Note, Pattern, Scales, Track, js};

use super::structure::{
    Structure, generate_structure, genre_swing, random_bass_variant, random_genre, random_key_offset, random_scale,
    randomize_structure, section_harmony,
};
use super::{clamp, drums, melodic, table};

const GENRE: &str = "_autoGenGenre";
const KEY_OFFSET: &str = "_autoGenKeyOffset";
const SCALE: &str = "_autoGenScale";
/// Legacy auto flag of the toolbar, read alongside `auto` and written with it.
const TOOLBAR_AUTO: &str = "_toolbarAuto";

/// Tonal centre and scale a melodic line plays in.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Harmony {
    /// Semitones.
    pub root: f64,
    /// A scale name; `None`: the variant's own.
    pub scale: Option<String>,
}

/// What generates: the scales and the random source.
#[allow(missing_debug_implementations)]
pub struct AutoGenerator<'a> {
    pub scales: &'a Scales,
    pub rnd: &'a mut dyn FnMut() -> f64,
    structure: Structure,
}

impl<'a> AutoGenerator<'a> {
    pub fn new(scales: &'a Scales, rnd: &'a mut dyn FnMut() -> f64) -> Self {
        Self { scales, rnd, structure: Structure::default() }
    }

    fn random(&mut self) -> f64 {
        (self.rnd)()
    }
}

/// A pattern-level key, as the JS `??` sees it (null counts as missing).
fn pattern_value<'p>(pattern: &'p Pattern, key: &str) -> Option<&'p Value> {
    pattern.extra.get(key).filter(|v| !v.is_null())
}

fn genre_of(pattern: &Pattern) -> Option<String> {
    pattern_value(pattern, GENRE).map(js::to_string)
}

/// `#resolveHarmony`: the section harmony moved to the pattern's key, in its scale.
fn pattern_harmony(pattern: &Pattern, genre: &str, section: &str, loop_in_element: u64) -> Harmony {
    let (root, scale) = section_harmony(genre, section, loop_in_element);
    let offset = pattern_value(pattern, KEY_OFFSET).map_or(0.0, js::to_number);
    let scale = pattern_value(pattern, SCALE).map_or(scale, js::to_string);
    Harmony { root: ((root + offset) % 12.0 + 12.0) % 12.0, scale: Some(scale) }
}

/// `#randomDensity`: kick, snare and bass near full, the others vary more.
fn random_density(g: &mut AutoGenerator<'_>, track: &Track) -> f64 {
    let core = matches!(
        instrument_family(&track.name),
        InstrumentFamily::Kick | InstrumentFamily::Snare | InstrumentFamily::Bass
    );
    let (min, max) = if core { (0.85, 1.05) } else { (0.6, 1.2) };
    js::to_fixed(min + g.random() * (max - min), 2)
}

/// `#applyGenreSwing`: the genre's swing, ±0.08.
fn apply_genre_swing(g: &mut AutoGenerator<'_>, track: &mut Track, genre: Option<&str>) {
    let genre = match genre {
        Some(genre) => genre.to_owned(),
        None => random_genre(g.rnd),
    };
    let (amount, resolution) = genre_swing(&genre);
    let jittered = amount + (g.random() * 2.0 - 1.0) * 0.08;
    track.swing_amount = js::to_fixed(clamp(jittered, 0.0, 0.45), 2);
    track.swing_resolution = resolution as i32;
}

/// `#applyVariation`: the variation budget of a generated track, when it has none yet.
fn apply_variation(g: &mut AutoGenerator<'_>, track: &mut Track, kind: InstrumentFamily) {
    let (min, max) = if kind.is_melodic() { (50.0, 85.0) } else { (30.0, 60.0) };
    let pick_in = |g: &mut AutoGenerator<'_>, min: f64, max: f64| min + (g.random() * (max - min + 1.0)).floor();
    if track.auto_hit_variation.is_none() {
        track.auto_hit_variation = Some(pick_in(g, min, max));
    }
    if track.auto_note_variation.is_none() {
        track.auto_note_variation = Some(pick_in(g, js::round(min / 2.0), max));
    }
}

/// `generateTrack`: a new line for track `t` of `pattern` with variant
/// `variant` (a bass track keeps its own `auto_variant`, else draws one of
/// its genre's), after its swing and variation are picked.
pub fn generate_track(
    g: &mut AutoGenerator<'_>,
    pattern: &mut Pattern,
    t: usize,
    variant: &str,
    density: f64,
    harmony: &Harmony,
) {
    let genre = genre_of(pattern);
    let Some(track) = pattern.tracks.get_mut(t) else { return };
    let kind = instrument_family(&track.name);
    apply_genre_swing(g, track, genre.as_deref());
    apply_variation(g, track, kind);
    let variant = if kind == InstrumentFamily::Bass {
        if track.auto_variant.is_empty() {
            random_bass_variant(genre.as_deref(), g.rnd)
        } else {
            track.auto_variant.clone()
        }
    } else {
        variant.to_owned()
    };
    instrument_line(g, pattern, t, kind, Some(&variant), density, harmony);
}

/// The line of instrument `kind` (its generator: `generateNewKick`…,
/// PIANO and ORGAN `generateNewMelody`) for track `t` of `pattern`, variant
/// `variant` (random when `None` or unknown).
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
    if matches!(kind, InstrumentFamily::Piano | InstrumentFamily::Organ) {
        melodic::melody(pattern, t, variant, density, harmony, scales, g.rnd);
        return;
    }
    let Some(track) = pattern.tracks.get_mut(t) else { return };
    match kind {
        InstrumentFamily::Kick => drums::kick(track, variant, density, scales, g.rnd),
        InstrumentFamily::Snare => drums::snare(track, variant, density, scales, g.rnd),
        InstrumentFamily::Hat => drums::hat(track, variant, density, scales, g.rnd),
        InstrumentFamily::Clap => drums::clap(track, variant, density, scales, g.rnd),
        InstrumentFamily::Perc => drums::perc(track, variant, density, scales, g.rnd),
        InstrumentFamily::Cowbell => drums::cowbell(track, variant, density, scales, g.rnd),
        InstrumentFamily::Bass => melodic::bass(track, variant, density, harmony, scales, g.rnd),
        InstrumentFamily::Piano | InstrumentFamily::Organ => {}
    }
}

/// `#findTrackConfig`: the variant of the template entry of the track's
/// type — the one whose name the track name contains, else the last.
fn find_track_config(structure: &[(String, String)], track: &Track) -> Option<String> {
    let kind = instrument_family(&track.name);
    let upper = track.name.to_uppercase();
    let mut config = None;
    for (name, variant) in structure {
        if instrument_family(name) == kind {
            config = Some(variant.clone()).filter(|v| !v.is_empty());
            if upper.contains(&name.to_uppercase()) {
                break;
            }
        }
    }
    config
}

/// Choices `generate_pattern` may be given instead of drawing them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    pub genre: Option<String>,
    pub key_offset: Option<f64>,
    pub scale: Option<String>,
}

/// `generatePattern`: a genre (drawn), a variation of its template, a key
/// and scale (kept when the pattern has them); an empty pattern gets the
/// template's tracks, an existing one new lines for the tracks the template
/// covers; a BASS track (empty) is added when there is none.
pub fn generate_pattern(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, options: &Options) {
    // `resolveGenreFromTags` never finds a genre in a tag list.
    let genre = match &options.genre {
        Some(genre) => genre.clone(),
        None => random_genre(g.rnd),
    };
    let structure = randomize_structure(&generate_structure(&genre), g.rnd);
    pattern.extra.insert(GENRE, Value::from(genre.as_str()));
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
    let harmony = pattern_harmony(pattern, &genre, &first.name, first.loop_in_element);

    if pattern.tracks.is_empty() {
        for (name, variant) in &structure {
            pattern.tracks.push(Track::new(name, pattern.beat_count, 4));
            let t = pattern.tracks.len() - 1;
            let density = random_density(g, &pattern.tracks[t]);
            generate_track(g, pattern, t, variant, density, &harmony);
        }
    } else {
        for t in 0..pattern.tracks.len() {
            if let Some(variant) = find_track_config(&structure, &pattern.tracks[t]) {
                let density = random_density(g, &pattern.tracks[t]);
                generate_track(g, pattern, t, &variant, density, &harmony);
            }
        }
    }

    if !pattern.tracks.iter().any(|t| instrument_family(&t.name) == InstrumentFamily::Bass) {
        let mut bass = Track::new("BASS", pattern.beat_count, 4);
        bass.use_synth = false;
        bass.auto_assign = true;
        bass.synth_preset = Some("BASS1".into());
        bass.volume = 0.5;
        pattern.tracks.push(bass);
    }
}

/// `SECTION_DENSITY`: how full a section plays.
fn section_density(section: &str) -> f64 {
    match section {
        "intro" => 0.4,
        "verse" => 0.7,
        "chorus" => 1.0,
        "break" => 0.2,
        "bridge" => 0.6,
        "outro" => 0.3,
        _ => 0.7,
    }
}

/// `#resolveSectionEndVariant`: hats roll and percussion fills into the next section.
fn section_end_variant(track: &Track, kind: InstrumentFamily) -> Option<&'static str> {
    match kind {
        InstrumentFamily::Hat => {
            let name = track.name.to_uppercase();
            Some(if name.contains("OHH") || name.contains("OPEN") { "ohhRoll" } else { "chhRoll" })
        }
        InstrumentFamily::Perc => Some("fill"),
        _ => None,
    }
}

/// `changeTrack`: the line of track `t` for loop `index` of the song form —
/// a snare crescendo on breaks, a fill merged into the old line on the last
/// loop of a section, a new line otherwise, at the section's density.
pub fn change_track(g: &mut AutoGenerator<'_>, index: u64, pattern: &mut Pattern, t: usize) {
    let Some(track) = pattern.tracks.get(t) else { return };
    let genre = match genre_of(pattern) {
        Some(genre) => genre,
        None => random_genre(g.rnd),
    };
    let element = g.structure.element(index);
    let section_end = element.is_last_loop_before_change;
    let density = if track.auto_density >= 0.0 {
        track.auto_density
    } else if section_end {
        0.2
    } else {
        section_density(&element.name)
    };
    let harmony = pattern_harmony(pattern, &genre, &element.name, element.loop_in_element);
    let kind = instrument_family(&track.name);
    let config = if track.auto_variant.is_empty() {
        find_track_config(&generate_structure(&genre), track)
    } else {
        Some(track.auto_variant.clone())
    };
    let Some(config) = config else { return };

    if element.name == "break" && kind == InstrumentFamily::Snare {
        drums::snare(&mut pattern.tracks[t], Some("breakCrescendo"), density, g.scales, g.rnd);
    } else if section_end {
        let variant = section_end_variant(track, kind).map_or(config, str::to_owned);
        let mut notes: Vec<Note> = pattern.tracks[t].notes.clone();
        generate_track(g, pattern, t, &variant, density, &harmony);
        let seen: Vec<(i32, i32)> = notes.iter().map(|n| (n.beat, n.step)).collect();
        let track = &mut pattern.tracks[t];
        notes.extend(track.notes.iter().filter(|n| !seen.contains(&(n.beat, n.step))).cloned());
        track.notes = notes;
    } else {
        pattern.tracks[t].notes.clear();
        generate_track(g, pattern, t, &config, density, &harmony);
    }
}

/// The player's loop start (`#handleLoopStart`) for loop `index`: an
/// `autoGen` pattern regenerates its melodic tracks, and every track on a
/// section's first or last loop; otherwise the tracks in auto mode are
/// regenerated. Whether anything was.
pub fn run_loop(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, index: u64) -> bool {
    let mut changed = false;
    if pattern.extra.get("autoGen").is_some_and(js::truthy) {
        let element = g.structure.element(index);
        let boundary = element.loop_in_element == 0 || element.is_last_loop_before_change;
        for t in 0..pattern.tracks.len() {
            if boundary || instrument_family(&pattern.tracks[t].name).is_melodic() {
                change_track(g, index, pattern, t);
                changed = true;
            }
        }
    } else {
        for t in 0..pattern.tracks.len() {
            if pattern.tracks[t].auto_mode {
                change_track(g, index, pattern, t);
                changed = true;
            }
        }
    }
    changed
}

/// Whether a pattern's tracks regenerate as it plays.
pub fn plays_auto(pattern: &Pattern) -> bool {
    pattern.extra.get("autoGen").is_some_and(js::truthy) || pattern.tracks.iter().any(|t| t.auto_mode)
}

// ─── The ↻ buttons ───────────────────────────────────────────────────────────

/// Auto mode as the toolbar shows it (`auto || _toolbarAuto`).
pub fn is_auto_on(track: &Track) -> bool {
    track.auto_mode || track.extra.get(TOOLBAR_AUTO).is_some_and(js::truthy)
}

fn set_auto(track: &mut Track, on: bool) {
    track.auto_mode = on;
    track.extra.insert(TOOLBAR_AUTO, Value::Bool(on));
}

/// `toggleAutoGeneration`: when a track of `kinds` is in auto mode, all of
/// them leave it (`false`); otherwise `generate` runs (`true`).
fn toggle(
    pattern: &mut Pattern,
    kinds: impl Fn(InstrumentFamily) -> bool,
    generate: impl FnOnce(&mut Pattern),
) -> bool {
    let of_kind = |t: &Track| kinds(instrument_family(&t.name));
    if pattern.tracks.iter().any(|t| is_auto_on(t) && of_kind(t)) {
        for track in pattern.tracks.iter_mut().filter(|t| of_kind(t)) {
            set_auto(track, false);
        }
        false
    } else {
        generate(pattern);
        true
    }
}

/// ↻ Drum (`toggleDrums`): new drum lines over the existing tracks (and a new
/// bass line if there is a bass track), the drum tracks in auto mode, empty
/// melodic tracks removed; or, when drums are in auto mode, auto mode off.
/// Whether generation is now on.
pub fn toggle_drums(g: &mut AutoGenerator<'_>, pattern: &mut Pattern) -> bool {
    toggle(pattern, InstrumentFamily::is_drum, |pattern| {
        generate_pattern(g, pattern, &Options::default());
        pattern.tracks.retain(|t| !instrument_family(&t.name).is_melodic() || !t.notes.is_empty());
        for track in pattern.tracks.iter_mut().filter(|t| instrument_family(&t.name).is_drum()) {
            set_auto(track, true);
        }
    })
}

/// The melodic parts a ↻ button drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MelodicKind {
    /// ↻ Bass.
    Bass,
    /// ↻ Chords.
    Chords,
}

impl MelodicKind {
    fn track_type(self) -> InstrumentFamily {
        match self {
            Self::Bass => InstrumentFamily::Bass,
            Self::Chords => InstrumentFamily::Piano,
        }
    }

    /// `MELODIC_GEN_PRESETS`: the synth preset and the fallback variant.
    fn preset(self) -> (&'static str, &'static str) {
        match self {
            Self::Bass => ("BASS1", "basic"),
            Self::Chords => ("PIANO", "chordStab"),
        }
    }
}

/// ↻ Bass / ↻ Chords (`toggleMelodic`): the first BASS (PIANO) track in auto
/// mode — created on the pattern's synth, with a first line from the genre
/// template, when there is none; or auto mode off. Whether generation is now on.
pub fn toggle_melodic(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, kind: MelodicKind) -> bool {
    let wanted = kind.track_type();
    toggle(
        pattern,
        |k| k == wanted,
        |pattern| {
            let found = pattern.tracks.iter().position(|t| instrument_family(&t.name) == wanted);
            let t = match found {
                Some(t) => t,
                None => {
                    if !pattern.extra.get(GENRE).is_some_and(js::truthy) {
                        let genre = random_genre(g.rnd);
                        pattern.extra.insert(GENRE, Value::from(genre.as_str()));
                    }
                    let genre = genre_of(pattern).unwrap_or_default();
                    let first = g.structure.element(0);
                    let (root, scale) = section_harmony(&genre, &first.name, first.loop_in_element);
                    let structure = generate_structure(&genre);
                    let (synth, fallback) = kind.preset();
                    let variant = structure
                        .iter()
                        .find(|(name, _)| name == wanted.name())
                        .map_or(fallback.to_owned(), |(_, v)| v.clone());
                    let mut track = Track::new(wanted.name(), pattern.beat_count, 4);
                    track.use_synth = true;
                    track.auto_assign = false;
                    track.synth_preset = Some(synth.to_owned());
                    track.volume = 0.8;
                    pattern.tracks.push(track);
                    let t = pattern.tracks.len() - 1;
                    generate_track(g, pattern, t, &variant, 1.0, &Harmony { root, scale: Some(scale) });
                    t
                }
            };
            set_auto(&mut pattern.tracks[t], true);
        },
    )
}

// ─── Random track ────────────────────────────────────────────────────────────

/// `RandomGenerator.generateRandom`: 15–35 % of the steps, at random
/// pitches (±6) and velocities (0.5–1), the track looping over its length.
pub fn random_track(track: &mut Track, rnd: &mut dyn FnMut() -> f64) {
    let config = table("random");
    let get = |key: &str| config.get(key).and_then(Value::as_f64).unwrap_or(f64::NAN);
    track.notes.clear();
    let spb = track.steps_per_beat;
    track.loop_length = Some(track.beat_count * spb);
    let total = track.beat_count * spb;
    let count = ((f64::from(total) * (get("densityMin") + rnd() * get("densitySpread"))).floor() as i32).max(1);
    let mut used: Vec<i32> = Vec::new();
    for _ in 0..count.min(total) {
        let step = loop {
            let step = (rnd() * f64::from(total)).floor() as i32;
            if !used.contains(&step) {
                break step;
            }
        };
        used.push(step);
        let pitch = (rnd() * get("pitchRange")).floor() + get("pitchMin");
        let velocity = get("velocityMin") + rnd() * get("velocitySpread");
        track.notes.push(Note { beat: step / spb, step: step % spb, pitch, velocity, ..Note::default() });
    }
}
