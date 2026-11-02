//! The song form the generation follows (`song_structure.js`): the section
//! (intro, chorus, verse, break…) a loop falls in and its harmony, the keys
//! and scales a pattern draws. Styles are in `assets/data/styles.json`.

use ordrumbox_core::json::Value;

use super::{num, pick, table, text};

fn structure_table(name: &str) -> Option<&'static Value> {
    table("structure").get(name)
}

/// The string items of a structure list (`GENRES`, `SCALES`…).
fn names(name: &str) -> Vec<&'static str> {
    structure_table(name).and_then(Value::as_array).unwrap_or_default().iter().filter_map(Value::as_str).collect()
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

/// `SongStructure#resolveHarmony`: the section's root plus the style's chord
/// progression step (none on breaks and outros), and its scale.
pub(crate) fn section_harmony(progression: &[f64], section: &str, loop_in_element: u64) -> (f64, String) {
    let table = structure_table("harmonicTable").and_then(|t| t.get(section));
    let root = table.and_then(|t| num(t, "root")).unwrap_or(0.0);
    let scale = table.and_then(|t| text(t, "scale")).unwrap_or("natural minor").to_owned();
    let offset = if !progression.is_empty() && section != "break" && section != "outro" {
        progression[(loop_in_element % progression.len() as u64) as usize]
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
