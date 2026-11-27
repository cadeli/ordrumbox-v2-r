//! Auto mode: turned on and off (AUTO on a pattern, the ♫ Drum / Bass /
//! Chords buttons, ♫ on one track, a random line for a track), and played.
//! A track in auto mode (`auto`), or any track of a pattern with `autoGen`,
//! moves through a section of the song form: a new line at its start, the
//! drums varying loop after loop and filling at its end.

use super::auto::{
    AutoGenerator, BASS_PRESET, Harmony, Options, TOOLBAR_AUTO, apply_variation, drum_tracks, generate_pattern,
    generate_track, melodic_variant, partner_of, pattern_harmony, pattern_style, random_density, style_name_of,
};
use super::structure::{SectionElement, section_harmony};
use super::table;
use crate::compose;
use ordrumbox_core::drum_taxonomy::{InstrumentFamily, instrument_family};
use ordrumbox_core::json::Value;
use ordrumbox_core::{Macros, Note, Pattern, Style, Track};

/// AUTO: every unlocked track of `pattern` in auto mode, or, when one is,
/// all of them out of it. Whether auto mode is now on.
pub fn toggle_auto(pattern: &mut Pattern) -> bool {
    let on = !pattern.tracks.iter().any(|t| !t.compose_lock && is_auto_on(t));
    for track in pattern.tracks.iter_mut().filter(|t| !t.compose_lock) {
        set_auto(track, on);
    }
    on
}

/// ♫ on one track: a new line for track `t` of `pattern` (in its style; a
/// new seed first when it has one); nothing on a locked track. A melodic
/// track plays one of its style's variants.
pub fn regenerate_track(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, t: usize) {
    let Some(seeded) = pattern.tracks.get(t).filter(|t| !t.compose_lock).map(|track| track.compose_seed >= 1.0) else {
        return;
    };
    if seeded {
        let draw = g.random();
        pattern.tracks[t].compose_seed = 1.0 + (draw * 9998.0).floor();
    }
    let style = pattern_style(g, pattern);
    if instrument_family(&pattern.tracks[t].name).is_drum() {
        drum_tracks(g, pattern, &style, &[t], 1.0);
    } else {
        let first = g.structure().element(0);
        let harmony = pattern_harmony(pattern, &style, &first.name, first.loop_in_element);
        let variant = melodic_variant(g, &style, &pattern.tracks[t], true);
        let density = random_density(g, &pattern.tracks[t]);
        generate_track(g, pattern, t, &variant, density, &harmony);
    }
}

// ─── The ♫ buttons ───────────────────────────────────────────────────────────

/// Auto mode as the ♫ buttons show it (`auto_mode` or their own flag).
pub fn is_auto_on(track: &Track) -> bool {
    track.auto_mode || track.extra.get(TOOLBAR_AUTO).is_some_and(Value::is_true)
}

fn set_auto(track: &mut Track, on: bool) {
    track.auto_mode = on;
    track.extra.insert(TOOLBAR_AUTO, Value::Bool(on));
}

/// When a track of `kinds` is in auto mode, all of
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

/// ♫ Drum: new lines over the existing tracks in the style of `options`
/// (else the pattern's, else a drawn one), the drum tracks the style plays
/// in auto mode, empty melodic tracks removed; or, when drums are in auto
/// mode, auto mode off. Whether generation is now on.
pub fn toggle_drums(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, options: &Options) -> bool {
    toggle(pattern, InstrumentFamily::is_drum, |pattern| {
        generate_pattern(g, pattern, options);
        pattern.tracks.retain(|t| !instrument_family(&t.name).is_melodic() || !t.notes.is_empty());
        let style = g.style(&style_name_of(pattern).unwrap_or_default());
        for track in pattern.tracks.iter_mut().filter(|t| instrument_family(&t.name).is_drum()) {
            let plays = style.part_for(&track.name).is_some();
            set_auto(track, plays);
        }
    })
}

/// The melodic parts a ♫ button drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MelodicKind {
    /// ♫ Bass.
    Bass,
    /// ♫ Chords.
    Chords,
}

impl MelodicKind {
    fn track_type(self) -> InstrumentFamily {
        match self {
            Self::Bass => InstrumentFamily::Bass,
            Self::Chords => InstrumentFamily::Piano,
        }
    }

    /// `MELODIC_GEN_PRESETS`: the synth preset.
    fn preset(self) -> &'static str {
        match self {
            Self::Bass => BASS_PRESET,
            Self::Chords => "PIANO",
        }
    }
}

/// ♫ Bass / ♫ Chords: the first BASS (PIANO) track in auto mode — created
/// on the pattern's synth, with a first line in the pattern's style, when
/// there is none; or auto mode off. Whether generation is now on.
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
                    let style = pattern_style(g, pattern);
                    let first = g.structure().element(0);
                    let (root, scale) = section_harmony(&style.progression, &first.name, first.loop_in_element);
                    let mut track = Track::new(wanted.name(), pattern.beat_count, 4);
                    track.use_synth = true;
                    track.auto_assign = false;
                    track.synth_preset = Some(kind.preset().to_owned());
                    track.volume = 0.8;
                    let variant = melodic_variant(g, &style, &track, false);
                    pattern.tracks.push(track);
                    let t = pattern.tracks.len() - 1;
                    generate_track(g, pattern, t, &variant, 1.0, &Harmony { root, scale: Some(scale) });
                    t
                }
            };
            let track = &mut pattern.tracks[t];
            set_auto(track, true);
            // A line holds as written unless the track's amounts say otherwise.
            apply_variation(g, track, wanted);
        },
    )
}

// ─── Random track ────────────────────────────────────────────────────────────

/// `RandomGenerator.generateRandom`: 15–35 % of the steps, at random
/// pitches (±6) and velocities (0.5–1), the track looping over its length.
pub fn random_track(track: &mut Track, rnd: &mut dyn FnMut() -> f64) {
    let config = table("random");
    // Without the table's value, the one this function's doc gives.
    let get = |key: &str| {
        let default = match key {
            "densityMin" => 0.15,
            "densitySpread" => 0.2,
            "pitchMin" => -6.0,
            "pitchRange" => 13.0,
            "velocityMin" | "velocitySpread" => 0.5,
            _ => 0.0,
        };
        config.get(key).and_then(Value::as_f64).unwrap_or(default)
    };
    track.notes.clear();
    let spb = track.steps_per_beat;
    track.loop_steps = Some(track.beat_count * spb);
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

/// `SECTION_DENSITY`: how full a section plays (the melodic density, and
/// the share of its hits a drum line keeps).
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

/// The share of its hits a drum line keeps in `section`: the section's
/// fullness, never under half (a drum line thins, it does not vanish).
fn section_hits(section: &str) -> f64 {
    0.5 + 0.5 * section_density(section)
}

/// The variation a drum track plays with (its own, else its part's, under
/// the macros): 0 when the style has no part for it.
fn drum_variation(track: &Track, style: &Style, macros: Macros) -> f64 {
    compose::track_settings(track, style, macros).map_or(0.0, |c| c.used.variation)
}

/// The line of track `t` for loop `index` of the song form.
/// A drum line: new at the start of a section, the style's fill on its last
/// loop, varied otherwise. A melodic line: new at the section's density.
/// A locked track stays as it is.
pub fn change_track(g: &mut AutoGenerator<'_>, index: u64, pattern: &mut Pattern, t: usize) {
    let Some(kind) = pattern.tracks.get(t).filter(|t| !t.compose_lock).map(|track| instrument_family(&track.name))
    else {
        return;
    };
    let style = pattern_style(g, pattern);
    let macros = Macros::of(pattern);
    let element = g.structure().element(index);
    if kind.is_drum() {
        let track = &pattern.tracks[t];
        if element.loop_in_element == 0 || track.notes.is_empty() {
            drum_tracks(g, pattern, &style, &[t], section_hits(&element.name));
            return;
        }
        let partner = partner_of(pattern, &style, t).map(|p| pattern.tracks[p].clone());
        let mut rng = g.line_rng(&pattern.tracks[t], index);
        let track = &mut pattern.tracks[t];
        if !(element.is_last_loop_before_change && compose::fill(track, &style, macros, &mut rng)) {
            compose::vary_line(track, &style, macros, partner.as_ref(), &mut rng);
        }
        return;
    }
    let track = &pattern.tracks[t];
    let density = if track.auto_density >= 0.0 { track.auto_density } else { section_density(&element.name) };
    let density = density * macros.energy_factor();
    let harmony = pattern_harmony(pattern, &style, &element.name, element.loop_in_element);
    let variant = melodic_variant(g, &style, &pattern.tracks[t], false);
    pattern.tracks[t].notes.clear();
    generate_track(g, pattern, t, &variant, density, &harmony);
}

/// Whether a track gets a new line on this loop. Bass and chords hold
/// theirs through a section and change at its start; a drum line also moves
/// every loop it varies, and fills at the section's end; a track without a
/// line gets one. A locked track never moves.
fn line_moves(track: &Track, style: &Style, macros: Macros, element: &SectionElement) -> bool {
    let drum = instrument_family(&track.name).is_drum();
    let moving = drum && (element.is_last_loop_before_change || drum_variation(track, style, macros) > 0.0);
    !track.compose_lock && (element.loop_in_element == 0 || moving || track.notes.is_empty())
}

/// The player's loop start for loop `index`: the tracks in auto mode, or all
/// those of an `autoGen` pattern, whose line moves on this loop get a new one.
/// Whether any did.
pub fn run_loop(g: &mut AutoGenerator<'_>, pattern: &mut Pattern, index: u64) -> bool {
    let all = pattern.extra.get("autoGen").is_some_and(Value::is_true);
    let style = g.style(&style_name_of(pattern).unwrap_or_default());
    let element = g.structure().element(index);
    let macros = Macros::of(pattern);
    let mut changed = false;
    for t in 0..pattern.tracks.len() {
        let track = &pattern.tracks[t];
        if (all || track.auto_mode) && line_moves(track, &style, macros, &element) {
            let before = pattern.tracks[t].notes.clone();
            change_track(g, index, pattern, t);
            changed |= pattern.tracks[t].notes != before;
        }
    }
    changed
}

/// Whether a pattern's tracks regenerate as it plays.
pub fn plays_auto(pattern: &Pattern) -> bool {
    pattern.extra.get("autoGen").is_some_and(Value::is_true) || pattern.tracks.iter().any(|t| t.auto_mode)
}
