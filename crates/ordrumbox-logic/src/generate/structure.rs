//! Genres and the song form the generation follows (`song_structure.js`):
//! a genre's track template, its bass variants, swing and chord progression,
//! and the section (intro, chorus, verse, break…) a loop falls in.

use ordrumbox_core::drum_taxonomy::instrument_family;
use ordrumbox_core::json::Value;

use super::{num, pick, table, text};

fn structure_table(name: &str) -> Option<&'static Value> {
    table("structure").get(name)
}

/// The string items of a structure list (`GENRES`, `SCALES`…).
fn names(name: &str) -> Vec<&'static str> {
    structure_table(name).and_then(Value::as_array).unwrap_or_default().iter().filter_map(Value::as_str).collect()
}

/// `getRandomGenre`.
pub(crate) fn random_genre(rnd: &mut dyn FnMut() -> f64) -> String {
    pick(rnd, &names("genres")).map_or_else(String::new, |g| (*g).to_owned())
}

/// `randomKeyOffset`: a tonal centre, 0 more often than the others.
pub(crate) fn random_key_offset(rnd: &mut dyn FnMut() -> f64) -> f64 {
    let offsets: Vec<f64> = structure_table("keyOffsets")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_f64)
        .collect();
    pick(rnd, &offsets).copied().unwrap_or(0.0)
}

/// `randomScale`.
pub(crate) fn random_scale(rnd: &mut dyn FnMut() -> f64) -> String {
    pick(rnd, &names("scales")).map_or_else(String::new, |s| (*s).to_owned())
}

/// `randomBassVariant`: one of the genre's bass variants (`basic` for an
/// unknown genre, still one draw).
pub(crate) fn random_bass_variant(genre: Option<&str>, rnd: &mut dyn FnMut() -> f64) -> String {
    let variants: Vec<&str> = genre
        .and_then(|g| structure_table("bassVariantsByGenre")?.get(g))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let variants = if variants.is_empty() { vec!["basic"] } else { variants };
    pick(rnd, &variants).map_or_else(|| "basic".to_owned(), |v| (*v).to_owned())
}

/// `generateStructure`: the genre's template, track name → variant (techno
/// for an unknown genre), in table order.
pub(crate) fn generate_structure(genre: &str) -> Vec<(String, String)> {
    let structures = structure_table("structures").and_then(Value::as_object);
    let template = structures.and_then(|s| s.get(genre).or_else(|| s.get("techno"))).and_then(Value::as_object);
    template
        .map(|t| t.iter().map(|(k, v)| (k.to_owned(), v.as_str().unwrap_or_default().to_owned())).collect())
        .unwrap_or_default()
}

/// `poolFor`: the variants a track may swap to (by name, else by type).
fn pool_for(name: &str) -> Option<&'static [Value]> {
    let by_name = structure_table("variantPools").and_then(|p| p.get(&name.to_uppercase()));
    let by_type = || structure_table("variantPoolsByType").and_then(|p| p.get(instrument_family(name).name()));
    by_name.or_else(by_type).and_then(Value::as_array)
}

fn pick_name(rnd: &mut dyn FnMut() -> f64, pool: &[Value]) -> String {
    pick(rnd, pool).map(ordrumbox_core::js::to_string).unwrap_or_default()
}

/// `randomizeStructure`: drop some optional tracks, swap variants within
/// their pools, maybe add an extra instrument. KICK, SNARE and BASS stay.
pub(crate) fn randomize_structure(base: &[(String, String)], rnd: &mut dyn FnMut() -> f64) -> Vec<(String, String)> {
    let mut result = base.to_vec();
    let core = names("coreTracks");
    let drop = structure_table("dropChance");
    for (name, _) in base {
        if core.contains(&name.as_str()) {
            continue;
        }
        let chance = drop.and_then(|d| num(d, name).or_else(|| num(d, "default"))).unwrap_or(f64::NAN);
        if rnd() < chance {
            result.retain(|(n, _)| n != name);
            continue;
        }
        if rnd() < 0.55
            && let Some(pool) = pool_for(name)
        {
            let variant = pick_name(rnd, pool);
            if let Some(slot) = result.iter_mut().find(|(n, _)| n == name) {
                slot.1 = variant;
            }
        }
    }
    if rnd() < 0.3 {
        let missing: Vec<&str> =
            names("optionalExtras").into_iter().filter(|n| !result.iter().any(|(r, _)| r == n)).collect();
        if let Some(name) = pick(rnd, &missing).map(|n| (*n).to_owned())
            && let Some(pool) = pool_for(&name)
        {
            let variant = pick_name(rnd, pool);
            result.push((name, variant));
        }
    }
    result
}

/// `getGenreSwing`: swing amount and resolution of a genre.
pub(crate) fn genre_swing(genre: &str) -> (f64, f64) {
    let swing = structure_table("swingByGenre").and_then(|s| s.get(genre));
    match swing {
        Some(s) => (num(s, "swingAmount").unwrap_or(0.0), num(s, "swingResolution").unwrap_or(4.0)),
        None => (0.0, 4.0),
    }
}

/// `SongStructure#resolveHarmony`: the section's root plus the genre's chord
/// progression step (none on breaks and outros), and its scale.
pub(crate) fn section_harmony(genre: &str, section: &str, loop_in_element: u64) -> (f64, String) {
    let table = structure_table("harmonicTable").and_then(|t| t.get(section));
    let root = table.and_then(|t| num(t, "root")).unwrap_or(0.0);
    let scale = table.and_then(|t| text(t, "scale")).unwrap_or("natural minor").to_owned();
    let progression: Vec<f64> = structure_table("chordProgressions")
        .and_then(|p| p.get(genre))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|v| v.as_f64().unwrap_or(f64::NAN))
        .collect();
    let offset = if !progression.is_empty() && section != "break" && section != "outro" {
        let step = progression[(loop_in_element % progression.len() as u64) as usize];
        if step.is_nan() { 0.0 } else { step }
    } else {
        0.0
    };
    (root + offset, scale)
}

/// Where a loop sits in the song form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionElement {
    /// `intro`, `chorus`, `verse`, `break`, `bridge`, `outro`.
    pub name: String,
    /// Which occurrence of that section, from 1.
    pub number: u32,
    pub loop_in_element: u64,
    /// The last loop before the next section.
    pub is_last_loop_before_change: bool,
    pub element_loops: u64,
}

/// The song form: sections and their lengths in loops, repeating.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Structure {
    sections: Vec<(String, u64)>,
}

impl Default for Structure {
    /// The JS form: intro 4, chorus 8, verse 8, break 1, chorus 8, verse 8,
    /// break 1, bridge 4, verse 8, outro 4 (54 loops).
    fn default() -> Self {
        let sections = structure_table("sections")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|s| (text(s, "name").unwrap_or_default().to_owned(), num(s, "loops").unwrap_or(0.0) as u64))
            .collect();
        Self { sections }
    }
}

impl Structure {
    pub fn total_loops(&self) -> u64 {
        self.sections.iter().map(|(_, loops)| loops).sum()
    }

    /// The section loop `index` falls in (the form repeats).
    pub fn element(&self, index: u64) -> SectionElement {
        let total = self.total_loops();
        let in_song = if total > 0 { index % total } else { 0 };
        let mut cursor = 0;
        let mut counters: Vec<(&str, u32)> = Vec::new();
        for (name, loops) in &self.sections {
            let number = match counters.iter_mut().find(|(n, _)| n == name) {
                Some((_, c)) => {
                    *c += 1;
                    *c
                }
                None => {
                    counters.push((name, 1));
                    1
                }
            };
            if in_song < cursor + loops {
                let loop_in_element = in_song - cursor;
                return SectionElement {
                    name: name.clone(),
                    number,
                    loop_in_element,
                    is_last_loop_before_change: loop_in_element + 1 == *loops,
                    element_loops: *loops,
                };
            }
            cursor += loops;
        }
        SectionElement {
            name: "unknown".into(),
            number: 0,
            loop_in_element: 0,
            is_last_loop_before_change: false,
            element_loops: 0,
        }
    }
}
