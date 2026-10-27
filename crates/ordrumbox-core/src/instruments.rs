//! The instrument catalog (`logic/services/instruments_manager/`): canonical
//! instrument ids (`KICK`, `OHH`, `CONGAS`…) with their name patterns,
//! substitutes, GM MIDI mapping and default pan, and the lookups the app makes
//! with them (sound auto-assignment, MIDI files).
//!
//! The data is `data/instruments.json`, exported from the JS catalog by
//! `tools/export_assign_fixtures.mjs` (GM keys and programs already filled in).

use std::sync::OnceLock;

use crate::json::{self, Value};

/// Id of "no instrument" (`NOT_FOUND`).
pub const NOT_FOUND: &str = "NOT_FOUND";

/// One MIDI mapping of an instrument.
#[derive(Debug, Clone, PartialEq)]
pub struct MidiMapping {
    pub channel: i64,
    /// GM name ("Closed Hi-Hat", "Acoustic Bass").
    pub name: String,
    /// Drum note (key-based mapping).
    pub key: Option<i64>,
    /// GM program, 0-based (melodic mapping).
    pub program: Option<i64>,
    pub key_based: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instrument {
    pub id: String,
    pub drum: bool,
    /// As written in the catalog ("-4", "0"…).
    pub pan: String,
    /// Name patterns (a small regex dialect: `.`, `*`, `?`), whole name, any case.
    pub synonyms: Vec<String>,
    /// Ids to fall back on, in order.
    pub subst: Vec<String>,
    pub midi: Vec<MidiMapping>,
}

/// The catalog.
#[derive(Debug, Clone, PartialEq)]
pub struct Catalog {
    /// In catalog order (the JS `Map` order).
    pub instruments: Vec<Instrument>,
    /// GM program names, by program number.
    pub program_names: Vec<(i64, String)>,
    /// GM drum names, by note.
    pub drum_names: Vec<(i64, String)>,
}

/// The catalog of the app (parsed once).
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let value = json::parse(include_str!("../data/instruments.json")).expect("data/instruments.json parses");
        Catalog::from_json(&value)
    })
}

fn names(value: Option<&Value>) -> Vec<(i64, String)> {
    let Some(object) = value.and_then(Value::as_object) else { return Vec::new() };
    object.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_owned()))).collect()
}

/// Upper-case alphanumeric words of `s` (`split(/[^a-zA-Z0-9]+/)` after `toUpperCase()`).
fn words(s: &str) -> Vec<String> {
    s.to_uppercase().split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_owned).collect()
}

/// Words two strings share (`countCommonWords`).
pub fn count_common_words(a: &str, b: &str) -> usize {
    if a.is_empty() || b.is_empty() {
        return 0;
    }
    let mut first = words(a);
    first.sort();
    first.dedup();
    let second = words(b);
    first.iter().filter(|w| second.contains(w)).count()
}

/// `new RegExp('^' + pattern + '$', 'i').test(text)` for the catalog's
/// patterns: literals, `.` (any character), and `*` / `?` after an atom.
pub fn pattern_matches(pattern: &str, text: &str) -> bool {
    fn fold(c: char) -> char {
        c.to_uppercase().next().unwrap_or(c)
    }
    fn matches(p: &[char], t: &[char]) -> bool {
        let Some(&atom) = p.first() else { return t.is_empty() };
        let fits = |c: char| atom == '.' || fold(atom) == fold(c);
        match p.get(1) {
            Some('*') => {
                // Zero or more `atom`, longest first.
                let mut n = 0;
                while n < t.len() && fits(t[n]) {
                    n += 1;
                }
                (0..=n).rev().any(|k| matches(&p[2..], &t[k..]))
            }
            Some('?') => (!t.is_empty() && fits(t[0]) && matches(&p[2..], &t[1..])) || matches(&p[2..], t),
            _ => !t.is_empty() && fits(t[0]) && matches(&p[1..], &t[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    matches(&p, &t)
}

impl Catalog {
    pub fn from_json(value: &Value) -> Self {
        let strings = |v: Option<&Value>| -> Vec<String> {
            v.and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        };
        let instruments = value
            .get("instruments")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|i| Instrument {
                id: i.get("id").and_then(Value::as_str).unwrap_or(NOT_FOUND).to_owned(),
                drum: i.get("drum").and_then(Value::as_bool) == Some(true),
                pan: i.get("pan").and_then(Value::as_str).unwrap_or("0").to_owned(),
                synonyms: strings(i.get("synonyms")),
                subst: strings(i.get("subst")),
                midi: i
                    .get("midi")
                    .and_then(Value::as_array)
                    .unwrap_or_default()
                    .iter()
                    .map(|m| MidiMapping {
                        channel: m.get("channel").and_then(Value::as_f64).map_or(9, |c| c as i64),
                        name: m.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
                        key: m.get("key").and_then(Value::as_f64).map(|k| k as i64),
                        program: m.get("program").and_then(Value::as_f64).map(|p| p as i64),
                        key_based: m.get("keyBased").and_then(Value::as_bool),
                    })
                    .collect(),
            })
            .collect();
        Self {
            instruments,
            program_names: names(value.get("gmProgramNames")),
            drum_names: names(value.get("gmDrumNames")),
        }
    }

    /// The instrument with this id (any case), `findById`.
    pub fn by_id(&self, id: &str) -> Option<&Instrument> {
        let id = id.to_uppercase();
        self.instruments.iter().find(|i| i.id.to_uppercase() == id)
    }

    /// The instrument with exactly this id (case-sensitive), as the
    /// auto-assignment checks `DATA.instruments`.
    pub fn by_exact_id(&self, id: &str) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.id == id)
    }

    /// The first instrument with a pattern matching the whole `name` (`findByName`).
    pub fn by_name(&self, name: &str) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.synonyms.iter().any(|s| pattern_matches(s, name)))
    }

    /// `findInstrumentFromFileName`: the id itself, else an instrument whose id
    /// or a GM name shares a word with `file_name`, else the first word that
    /// matches a pattern.
    pub fn from_file_name(&self, file_name: &str) -> Option<&Instrument> {
        if let Some(found) = self.by_id(file_name.trim()) {
            return Some(found);
        }
        self.instruments
            .iter()
            .find(|i| count_common_words(&i.id, file_name) > 0)
            .or_else(|| {
                self.instruments.iter().find(|i| i.midi.iter().any(|m| count_common_words(&m.name, file_name) > 0))
            })
            .or_else(|| words(file_name).iter().find_map(|w| self.by_name(w)))
    }

    /// The instrument mapped to `key` on MIDI `channel` (`findInstrumentFromMidi`).
    pub fn from_midi(&self, channel: i64, key: i64) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.midi.iter().any(|m| m.channel == channel && m.key == Some(key)))
    }

    /// The instrument for GM `program` (`findInstrumentFromMidiProgram`): a
    /// direct mapping, else the program's GM name matched by pattern or by
    /// mapping name.
    pub fn from_midi_program(&self, program: i64) -> Option<&Instrument> {
        if let Some(found) = self.instruments.iter().find(|i| i.midi.iter().any(|m| m.program == Some(program))) {
            return Some(found);
        }
        let gm = &self.program_names.iter().find(|(p, _)| *p == program)?.1;
        self.by_name(gm).or_else(|| {
            self.instruments.iter().find(|i| i.midi.iter().any(|m| m.name.to_lowercase() == gm.to_lowercase()))
        })
    }

    /// The instrument's id then its substitutes (`getTrackCandidatesFromInstrument`).
    pub fn track_candidates(instrument: &Instrument) -> Vec<&str> {
        let mut candidates = vec![instrument.id.as_str()];
        for s in &instrument.subst {
            if !s.is_empty() && !candidates.contains(&s.as_str()) {
                candidates.push(s);
            }
        }
        candidates
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns() {
        assert!(pattern_matches(".*HAT.*", "open hat 2"));
        assert!(pattern_matches("OH", "oh"));
        assert!(!pattern_matches("OH", "OHH"));
        assert!(pattern_matches(".*MARACAS?.*", "xMARACAy"));
        assert!(pattern_matches("D.*BASS", "DRUM BASS"));
        assert!(pattern_matches("E.GUIT", "E GUIT"));
        assert!(!pattern_matches("LT.*", "ALT"));
    }

    #[test]
    fn catalog_loads() {
        let c = catalog();
        assert_eq!(c.instruments.len(), 66);
        assert_eq!(c.by_id("ohh").map(|i| i.id.as_str()), Some("OHH"));
        assert_eq!(c.program_names.len(), 128);
        assert_eq!(count_common_words("HI_TOM", "tom hi"), 2);
    }
}
