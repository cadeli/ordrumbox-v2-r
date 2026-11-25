//! The instrument catalog: canonical instrument ids (`KICK`, `OHH`, `CONGAS`…)
//! with their name patterns, words, substitutes, GM MIDI mapping and default
//! pan, and the lookups the app makes with them (sound auto-assignment, MIDI
//! files, kit import).
//!
//! Everything the detection knows is data, in `data/instruments.json`, which is
//! edited by hand (`DEVELOPMENT.md`, "Instrument catalog"); this module only reads it
//! and applies its rules.
// audit: allow(function size): `from_json` reads the catalog's fields one after the other.

use std::sync::OnceLock;

use crate::json::{self, Value};

pub use detect::pattern_matches;

mod detect;

/// What a kit file writes as the instrument (`key`) of a sample of none
/// the catalog knows; read back as no instrument.
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

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Instrument {
    pub id: String,
    pub drum: bool,
    /// Plays notes (a bass, a lead, a pitched bell): with no sample of its own, a synth preset.
    pub melodic: bool,
    /// As written in the catalog ("-4", "0"…).
    pub pan: String,
    /// The instrument this one is a variant of (`TOM` for `HI_TOM`), and the
    /// qualifier that picks it (`HI`).
    pub variant_of: Option<String>,
    pub qualifier: Option<String>,
    /// Name patterns (a small regex dialect: `.`, `*`, `?`), whole name, any case.
    pub synonyms: Vec<String>,
    /// Words that name it like its id: other names ("Rhodes"), short spellings
    /// ("shkr"), phrases ("BASS DRUM").
    pub words: Vec<String>,
    /// Words that name it only when no other word of the name names anything ("Stab").
    pub weak_words: Vec<String>,
    /// Ways of playing another instrument that name this one ("Snare Rim" is a rimshot).
    pub articulations: Vec<String>,
    /// Category prefixes of synth patch names ("LD Supersaw").
    pub patch_prefixes: Vec<String>,
    /// Ids to fall back on, in order.
    pub subst: Vec<String>,
    pub midi: Vec<MidiMapping>,
}

/// Instruments with an open and a closed sound.
#[derive(Debug, Clone, PartialEq)]
pub struct OpenClosed {
    pub members: Vec<String>,
    pub open: String,
    pub closed: String,
    /// What a GM name that states neither plays (a hi-hat is closed).
    pub neutral: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Openness {
    Open,
    Closed,
}

/// A kit's file named `word` alone is `id` when another file of the kit is `beside`.
#[derive(Debug, Clone, PartialEq)]
pub struct KitContext {
    pub word: String,
    pub beside: String,
    pub id: String,
}

/// The catalog.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Catalog {
    /// In catalog order: the first one that fits wins.
    pub instruments: Vec<Instrument>,
    /// Category names and their ids, in the order the "add track" menus list them.
    pub categories: Vec<(String, Vec<String>)>,
    /// Words that pick a variant (`LOW` is `LO`).
    pub qualifiers: Vec<(String, String)>,
    pub openness: Vec<(String, Openness)>,
    pub open_closed: Vec<OpenClosed>,
    /// Words that count last, and the id one of them names then (`BELL` is `CROMAPERC`).
    pub generic_words: Vec<(String, Option<String>)>,
    pub kit_context: Vec<KitContext>,
    /// GM program names, by program number.
    pub program_names: Vec<(i64, String)>,
    /// GM drum names, by note.
    pub drum_names: Vec<(i64, String)>,
}

/// The catalog of the app (parsed once).
#[expect(clippy::expect_used, reason = "a file built in, read by the tests")]
pub fn catalog() -> &'static Catalog {
    static CATALOG: OnceLock<Catalog> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let value = json::parse(include_str!("../data/instruments.json")).expect("data/instruments.json parses");
        Catalog::from_json(&value)
    })
}

fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array).unwrap_or_default().iter().filter_map(Value::as_str).map(str::to_owned).collect()
}

fn entries(v: Option<&Value>) -> Vec<(String, &Value)> {
    let Some(object) = v.and_then(Value::as_object) else { return Vec::new() };
    object.iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

fn names(value: Option<&Value>) -> Vec<(i64, String)> {
    entries(value).into_iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_owned()))).collect()
}

impl Catalog {
    pub fn from_json(value: &Value) -> Self {
        let instruments = value
            .get("instruments")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            // An entry without an id names no instrument: left out.
            .filter_map(|i| {
                Some(Instrument {
                    id: str_of(i, "id")?,
                    drum: i.get("drum").and_then(Value::as_bool) == Some(true),
                    melodic: i.get("melodic").and_then(Value::as_bool) == Some(true),
                    pan: str_of(i, "pan").unwrap_or_else(|| "0".to_owned()),
                    variant_of: str_of(i, "variantOf"),
                    qualifier: str_of(i, "qualifier"),
                    synonyms: strings(i.get("synonyms")),
                    words: strings(i.get("words")),
                    weak_words: strings(i.get("weakWords")),
                    articulations: strings(i.get("articulations")),
                    patch_prefixes: strings(i.get("patchPrefixes")),
                    subst: strings(i.get("subst")),
                    midi: i
                        .get("midi")
                        .and_then(Value::as_array)
                        .unwrap_or_default()
                        .iter()
                        .map(|m| MidiMapping {
                            channel: m.get("channel").and_then(Value::as_f64).map_or(9, |c| c as i64),
                            name: str_of(m, "name").unwrap_or_default(),
                            key: m.get("key").and_then(Value::as_f64).map(|k| k as i64),
                            program: m.get("program").and_then(Value::as_f64).map(|p| p as i64),
                            key_based: m.get("keyBased").and_then(Value::as_bool),
                        })
                        .collect(),
                })
            })
            .collect();
        let list = |key: &str| value.get(key).and_then(Value::as_array).unwrap_or_default();
        Self {
            instruments,
            categories: list("categories")
                .iter()
                .map(|c| (str_of(c, "name").unwrap_or_default(), strings(c.get("ids"))))
                .collect(),
            qualifiers: entries(value.get("qualifiers"))
                .into_iter()
                .filter_map(|(w, q)| Some((w, q.as_str()?.to_owned())))
                .collect(),
            openness: entries(value.get("openness"))
                .into_iter()
                .filter_map(|(w, o)| match o.as_str()? {
                    "open" => Some((w, Openness::Open)),
                    "closed" => Some((w, Openness::Closed)),
                    _ => None,
                })
                .collect(),
            open_closed: list("openClosed")
                .iter()
                .map(|o| OpenClosed {
                    members: strings(o.get("members")),
                    open: str_of(o, "open").unwrap_or_default(),
                    closed: str_of(o, "closed").unwrap_or_default(),
                    neutral: str_of(o, "neutral"),
                })
                .collect(),
            generic_words: entries(value.get("genericWords"))
                .into_iter()
                .map(|(w, id)| (w, id.as_str().map(str::to_owned)))
                .collect(),
            kit_context: list("kitContext")
                .iter()
                .map(|k| KitContext {
                    word: str_of(k, "word").unwrap_or_default(),
                    beside: str_of(k, "beside").unwrap_or_default(),
                    id: str_of(k, "is").unwrap_or_default(),
                })
                .collect(),
            program_names: names(value.get("gmProgramNames")),
            drum_names: names(value.get("gmDrumNames")),
        }
    }

    /// The instrument with this id (any case).
    pub fn by_id(&self, id: &str) -> Option<&Instrument> {
        let id = id.to_uppercase();
        self.instruments.iter().find(|i| i.id.to_uppercase() == id)
    }

    /// The instrument with exactly this id (case-sensitive).
    pub fn by_exact_id(&self, id: &str) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.id == id)
    }

    /// The first instrument with a pattern matching the whole `name`.
    pub fn by_name(&self, name: &str) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.synonyms.iter().any(|s| pattern_matches(s, name)))
    }

    /// The category an id is in.
    pub fn category_of(&self, id: &str) -> Option<&str> {
        self.categories.iter().find(|(_, ids)| ids.iter().any(|i| i == id)).map(|(name, _)| name.as_str())
    }

    /// The instrument mapped to `key` on MIDI `channel`.
    pub fn from_midi(&self, channel: i64, key: i64) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.midi.iter().any(|m| m.channel == channel && m.key == Some(key)))
    }

    /// The instrument for GM `program`: a direct mapping, else the program's GM
    /// name matched by pattern or by mapping name.
    pub fn from_midi_program(&self, program: i64) -> Option<&Instrument> {
        if let Some(found) = self.instruments.iter().find(|i| i.midi.iter().any(|m| m.program == Some(program))) {
            return Some(found);
        }
        let gm = &self.program_names.iter().find(|(p, _)| *p == program)?.1;
        self.by_name(gm).or_else(|| {
            self.instruments.iter().find(|i| i.midi.iter().any(|m| m.name.to_lowercase() == gm.to_lowercase()))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_the_catalog_names_exists() {
        let c = catalog();
        let exists = |id: &str| assert!(c.by_exact_id(id).is_some(), "{id}");
        for i in &c.instruments {
            i.subst.iter().for_each(|s| exists(s));
            if let Some(q) = &i.qualifier {
                assert!(c.qualifiers.iter().any(|(_, v)| v == q), "{}: {q}", i.id);
            }
        }
        for pair in &c.open_closed {
            pair.members.iter().chain([&pair.open, &pair.closed]).chain(&pair.neutral).for_each(|s| exists(s));
        }
        c.generic_words.iter().filter_map(|(_, id)| id.as_deref()).for_each(exists);
        for k in &c.kit_context {
            exists(&k.beside);
            exists(&k.id);
        }
        assert_eq!(c.category_of("HI_TOM"), Some("Toms"));
    }

    #[test]
    fn catalog_loads() {
        let c = catalog();
        assert_eq!(c.instruments.len(), 65);
        assert_eq!(c.by_id("ohh").map(|i| i.id.as_str()), Some("OHH"));
        assert_eq!(c.program_names.len(), 128);
    }

    #[test]
    fn every_instrument_is_in_one_category() {
        let listed: Vec<&str> =
            catalog().categories.iter().flat_map(|(_, ids)| ids.iter().map(String::as_str)).collect();
        for instrument in &catalog().instruments {
            let count = listed.iter().filter(|id| **id == instrument.id).count();
            assert_eq!(count, 1, "{}", instrument.id);
        }
        assert_eq!(listed.len(), catalog().instruments.len(), "no id outside the catalog");
    }
}
