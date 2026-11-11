//! The instrument catalog: canonical instrument ids (`KICK`, `OHH`, `CONGAS`…)
//! with their name patterns, words, substitutes, GM MIDI mapping and default
//! pan, and the lookups the app makes with them (sound auto-assignment, MIDI
//! files, kit import).
//!
//! Everything the detection knows is data, in `data/instruments.json`, which is
//! edited by hand (`DEVELOPMENT.md`, "Instrument catalog"); this module only reads it
//! and applies its rules.

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

/// Upper-case alphanumeric words of `s`, CamelCase cut ("OpenHat" is OPEN HAT).
fn words(s: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(s.len() + 4);
    let mut previous_lower = false;
    for c in s.chars() {
        if previous_lower && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        previous_lower = c.is_ascii_lowercase();
        spaced.push(c);
    }
    spaced
        .to_uppercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A name without what it says in parentheses, and what it says there:
/// "Lead 8 (bass + lead)" is a lead, its parentheses only a detail.
fn split_parentheses(s: &str) -> (String, String) {
    let (mut outside, mut inside) = (String::new(), String::new());
    let mut depth = 0usize;
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth > 0 => inside.push(c),
            _ => outside.push(c),
        }
        if matches!(c, '(' | ')' | '[' | ']') {
            // Keep the words apart: "Pad(choir)" is PAD, CHOIR.
            outside.push(' ');
            inside.push(' ');
        }
    }
    (outside, inside)
}

/// How the words of a name found an instrument.
enum Base<'a> {
    IdWord(&'a Instrument),
    GmWord(&'a Instrument),
    Weak(&'a Instrument),
    Pattern(&'a Instrument),
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

/// The literal parts of a pattern (`.*TUBULAR.*BELL.*` is TUBULAR, BELL).
fn pattern_parts(pattern: &str) -> impl Iterator<Item = &str> {
    pattern.split(['.', '*', '?', ' ']).filter(|p| !p.is_empty())
}

impl Catalog {
    pub fn from_json(value: &Value) -> Self {
        let instruments = value
            .get("instruments")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|i| Instrument {
                id: str_of(i, "id").unwrap_or_else(|| NOT_FOUND.to_owned()),
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

    /// The instrument with this id (any case), `findById`.
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

    fn is_qualifier(&self, word: &str) -> bool {
        self.qualifiers.iter().any(|(w, _)| w == word)
    }

    fn is_generic(&self, word: &str) -> bool {
        self.generic_words.iter().any(|(w, _)| w == word)
    }

    /// The words of a name, digits removed ("sn01" is SN) but in a word the
    /// catalog lists ("808"), and the instruments' words read as their id
    /// ("Rhodes" is PIANO, "Bass Drum" KICK).
    fn name_words(&self, s: &str) -> Vec<String> {
        let listed = |w: &str| self.is_generic(w) || self.instruments.iter().any(|i| i.words.iter().any(|x| x == w));
        let raw: Vec<String> = words(s)
            .into_iter()
            .map(|w| if listed(&w) { w } else { w.chars().filter(|c| !c.is_ascii_digit()).collect() })
            .filter(|w| !w.is_empty())
            .collect();
        let mut found = Vec::new();
        let mut rest = raw.as_slice();
        while let Some(first) = rest.first() {
            // The longest word or phrase of an instrument that starts here.
            let phrase = self
                .instruments
                .iter()
                .flat_map(|i| i.words.iter().map(move |w| (w.split(' ').collect::<Vec<_>>(), &i.id)))
                .filter(|(parts, _)| rest.len() >= parts.len() && parts.iter().zip(rest).all(|(p, w)| p == w))
                .max_by_key(|(parts, _)| parts.len());
            match phrase {
                Some((parts, id)) => {
                    found.push(id.clone());
                    rest = &rest[parts.len()..];
                }
                None => {
                    found.push(first.clone());
                    rest = &rest[1..];
                }
            }
        }
        found
    }

    /// The qualifier a name states: MID if any ("Low-Mid Tom" is the mid tom), else its first one.
    fn qualifier(&self, s: &str) -> Option<&str> {
        let found: Vec<&str> = self
            .name_words(s)
            .iter()
            .filter_map(|w| self.qualifiers.iter().find(|(word, _)| word == w).map(|(_, q)| q.as_str()))
            .collect();
        if found.contains(&"MID") { Some("MID") } else { found.first().copied() }
    }

    /// The openness a name states, from its first openness word.
    fn name_openness(&self, s: &str) -> Option<Openness> {
        self.name_words(s).iter().find_map(|w| self.openness.iter().find(|(word, _)| word == w).map(|(_, o)| *o))
    }

    /// The words of `s` but the qualifier and openness words.
    fn core_words(&self, s: &str) -> Vec<String> {
        self.name_words(s)
            .into_iter()
            .filter(|w| !self.is_qualifier(w) && !self.openness.iter().any(|(word, _)| word == w))
            .collect()
    }

    /// Whether `text` has one of `words`, qualifier and openness words left out.
    fn shares_word(&self, text: &str, words: &[String]) -> bool {
        self.core_words(text).iter().any(|w| words.contains(w))
    }

    /// The instrument a file or patch name means. In order: the id itself; a patch
    /// category prefix ("LD Supersaw"); a pattern of two words ("Tubular Bells");
    /// the words of the name but the generic ones, outside the parentheses then
    /// inside: an articulation, an id or instrument word, a GM name, a weak word,
    /// the most specific pattern; then the generic words (PERC, LOOP, BELL…).
    /// Digits do not count and CamelCase is cut; the qualifiers (HI, LOW, MID…) and
    /// OPEN/CLOSED only pick the variant of what the other words found.
    pub fn from_file_name(&self, file_name: &str) -> Option<&Instrument> {
        if let Some(found) = self.by_id(file_name.trim()) {
            return Some(found);
        }
        if let Some(found) = self.by_patch_prefix(file_name) {
            return Some(found);
        }
        self.by_words(file_name)
    }

    /// The instruments the files of one kit mean, each read with the names of the
    /// others (`kitContext`): in a kit with an `RS.wav`, `CL.wav` is the claves.
    pub fn from_kit_file_names(&self, names: &[&str]) -> Vec<Option<&Instrument>> {
        let found: Vec<Option<&Instrument>> = names.iter().map(|n| self.from_file_name(n)).collect();
        let stem = |name: &str| -> Vec<String> {
            let stem =
                name.rsplit_once('.').filter(|(_, ext)| ext.eq_ignore_ascii_case("wav")).map_or(name, |(s, _)| s);
            self.name_words(stem)
        };
        names
            .iter()
            .enumerate()
            .map(|(k, name)| {
                let words = stem(name);
                let beside = |id: &str| found.iter().enumerate().any(|(j, f)| j != k && f.is_some_and(|f| f.id == id));
                let rule = self.kit_context.iter().find(|c| words == [c.word.as_str()] && beside(&c.beside));
                rule.map_or(found[k], |c| self.by_exact_id(&c.id))
            })
            .collect()
    }

    /// The id a patch name's category prefix means, when the prefix is no catalog word.
    fn by_patch_prefix(&self, file_name: &str) -> Option<&Instrument> {
        let all = words(file_name);
        let [first, _, ..] = all.as_slice() else { return None };
        let found = self.instruments.iter().find(|i| i.patch_prefixes.contains(first))?;
        let collides = self.by_name(first).is_some() || self.instruments.iter().any(|i| words(&i.id).contains(first));
        if collides { None } else { Some(found) }
    }

    /// The instrument whose most specific pattern matches `word`: `.*CLAVINET.*` wins over `.*CLAV.*`.
    fn by_specific_name(&self, word: &str) -> Option<&Instrument> {
        let literal_count = |pattern: &str| pattern.chars().filter(|c| !matches!(c, '.' | '*' | '?')).count();
        let mut best: Option<(&Instrument, usize)> = None;
        for i in &self.instruments {
            for pattern in i.synonyms.iter().filter(|p| pattern_matches(p, word)) {
                let count = literal_count(pattern);
                if best.is_none_or(|(_, b)| count > b) {
                    best = Some((i, count));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// The instrument whose pattern of two words or more matches the whole name
    /// (`.*ORCH.*HIT.*` for "Orchestra Hit"), the one with the most letters. A
    /// pattern with a part of one letter or a qualifier (`.*TOM.*HI.*`) is left
    /// to the qualifier pass.
    fn by_compound_pattern(&self, file_name: &str) -> Option<&Instrument> {
        let text = self.name_words(file_name).join(" ");
        let usable = |parts: &[&str]| {
            let qualifier = |p: &str| {
                let p = p.to_uppercase();
                self.is_qualifier(&p) || self.openness.iter().any(|(w, _)| *w == p)
            };
            parts.len() >= 2 && parts.iter().all(|p| p.len() >= 2 && !qualifier(p))
        };
        let mut best: Option<(&Instrument, usize)> = None;
        for i in &self.instruments {
            for pattern in &i.synonyms {
                let parts: Vec<&str> = pattern_parts(pattern).collect();
                let count = parts.iter().map(|p| p.len()).sum();
                if usable(&parts) && pattern_matches(pattern, &text) && best.is_none_or(|(_, b)| count > b) {
                    best = Some((i, count));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// What `words` find, in order: an articulation, an id word, the GM name with
    /// the fewest other words ("Low Conga" for "conga low", not "Mute Hi Conga"),
    /// a weak word, a pattern.
    fn by_word_list(&self, words: &[String]) -> Option<Base<'_>> {
        if words.is_empty() {
            return None;
        }
        let names_it = |list: &[String]| list.iter().any(|w| words.contains(w));
        if let Some(found) = self.instruments.iter().find(|i| names_it(&i.articulations)) {
            return Some(Base::IdWord(found));
        }
        if let Some(found) = self.instruments.iter().find(|i| self.shares_word(&i.id, words)) {
            return Some(Base::IdWord(found));
        }
        let extra_words = |name: &str| self.core_words(name).iter().filter(|w| !words.contains(w)).count();
        let by_gm_name = self
            .instruments
            .iter()
            .filter_map(|i| {
                let names = i.midi.iter().filter(|m| self.shares_word(&m.name, words));
                Some((i, names.map(|m| extra_words(&m.name)).min()?))
            })
            .min_by_key(|&(_, extra)| extra);
        if let Some((found, _)) = by_gm_name {
            return Some(Base::GmWord(found));
        }
        if let Some(found) = self.instruments.iter().find(|i| names_it(&i.weak_words)) {
            return Some(Base::Weak(found));
        }
        words.iter().find_map(|w| self.by_specific_name(w)).map(Base::Pattern)
    }

    /// What the generic words of a name find: an id ("Perc"), else the id one of
    /// them names ("Bell"), else what they share with the rest.
    fn by_generic_words(&self, words: &[String]) -> Option<Base<'_>> {
        if let Some(found) = self.instruments.iter().find(|i| self.shares_word(&i.id, words)) {
            return Some(Base::IdWord(found));
        }
        let named = words.iter().find_map(|w| self.generic_words.iter().find(|(g, _)| g == w)?.1.as_deref());
        match named.and_then(|id| self.by_exact_id(id)) {
            Some(found) => Some(Base::IdWord(found)),
            None => self.by_word_list(words),
        }
    }

    /// The instrument a variant group holds for `qualifier` (`TOM`, `HI`: `HI_TOM`).
    fn variant(&self, group: &str, qualifier: Option<&str>) -> Option<&Instrument> {
        self.instruments
            .iter()
            .find(|i| i.variant_of.as_deref().unwrap_or(&i.id) == group && i.qualifier.as_deref() == qualifier)
    }

    /// The instrument a name's words point to, then its variant and its openness.
    fn by_words(&self, file_name: &str) -> Option<&Instrument> {
        // First pass: the qualifiers (HI, LOW, MID...) do not count.
        let wanted = self.qualifier(file_name);
        let (outside, inside) = split_parentheses(file_name);
        let specific = |s: &str| self.core_words(s).into_iter().filter(|w| !self.is_generic(w)).collect::<Vec<_>>();
        let generic: Vec<String> = self.core_words(file_name).into_iter().filter(|w| self.is_generic(w)).collect();
        let base = self
            .by_compound_pattern(file_name)
            .map(Base::Pattern)
            .or_else(|| self.by_word_list(&specific(&outside)))
            .or_else(|| self.by_word_list(&specific(&inside)))
            .or_else(|| self.by_generic_words(&generic))?;
        // Found by a GM name ("Open Hi-Hat"), the instrument says nothing of the name's openness.
        // A pattern can state the qualifier itself (`LT.*` is LO_TOM).
        let (base, level_known, from_gm_name) = match base {
            Base::Weak(found) => return Some(found),
            Base::IdWord(found) => (found, true, false),
            Base::GmWord(found) => (found, true, true),
            Base::Pattern(found) => (found, wanted.is_some(), false),
        };
        // Second pass: the qualifier picks the variant of the instrument found.
        let group = base.variant_of.as_deref().unwrap_or(&base.id);
        let found = if level_known { self.variant(group, wanted).unwrap_or(base) } else { base };
        // Then the openness, for the instruments that have an open and a closed sound.
        let Some(pair) = self.open_closed.iter().find(|o| o.members.contains(&found.id)) else { return Some(found) };
        let id = match self.name_openness(file_name) {
            Some(Openness::Open) => Some(&pair.open),
            Some(Openness::Closed) => Some(&pair.closed),
            None if from_gm_name => pair.neutral.as_ref(),
            None => None,
        };
        Some(id.and_then(|id| self.by_exact_id(id)).unwrap_or(found))
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
    fn patterns() {
        assert!(pattern_matches(".*HAT.*", "open hat 2"));
        assert!(pattern_matches("OH", "oh"));
        assert!(!pattern_matches("OH", "OHH"));
        assert!(pattern_matches(".*MARACAS?.*", "xMARACAy"));
        assert!(pattern_matches("D.*BASS", "DRUM BASS"));
        assert!(pattern_matches("E.GUIT", "E GUIT"));
        assert!(!pattern_matches("LT.*", "ALT"));
    }

    fn detected(name: &str) -> &str {
        catalog().from_file_name(name).map_or(NOT_FOUND, |i| i.id.as_str())
    }

    #[test]
    fn qualifiers_pick_the_variant() {
        assert_eq!(detected("LOW TOM"), "LO_TOM");
        assert_eq!(detected("tom_hi"), "HI_TOM");
        assert_eq!(detected("Tom High.wav"), "HI_TOM");
        assert_eq!(detected("Tom_Floor.wav"), "LO_TOM");
        assert_eq!(detected("Low-Mid Tom"), "MTOM");
        assert_eq!(detected("Hi-Mid Tom"), "MTOM");
        assert_eq!(detected("ltom"), "LO_TOM");
        assert_eq!(detected("HT"), "HI_TOM");
        assert_eq!(detected("Tom_Rack_01.wav"), "TOM");
        assert_eq!(detected("conga low"), "LO_CONGAS");
    }

    #[test]
    fn openness_picks_the_hat() {
        assert_eq!(detected("hat"), "CHH");
        assert_eq!(detected("Hi-Hat 01.wav"), "CHH");
        assert_eq!(detected("Closed Hi-Hat"), "CHH");
        assert_eq!(detected("HH Open 2.wav"), "OHH");
        assert_eq!(detected("HH_Open_Long.wav"), "OHH");
        assert_eq!(detected("OpenHat_02.wav"), "OHH");
        assert_eq!(detected("OHH_909.wav"), "OHH");
        assert_eq!(detected("Pedal Hi-Hat"), "CHHK");
        assert_eq!(detected("Open Hi Conga"), "OHI_CONGAS");
    }

    #[test]
    fn digits_do_not_count() {
        assert_eq!(detected("sn01.wav"), "SNARE");
        assert_eq!(detected("cp01.wav"), "CLAP");
        assert_eq!(detected("HH Closed 1.wav"), "CHH");
        assert_eq!(detected("TR909_BD_Long.wav"), "KICK");
        assert_eq!(detected("Lead 1 (square)"), "SYNTHLEAD");
        assert_eq!(detected("808_C.wav"), "BASS");
        assert_eq!(detected("808 Kick.wav"), "KICK");
    }

    #[test]
    fn synth_and_keys_words() {
        assert_eq!(detected("Agogo Hi.wav"), "COWBELL");
        assert_eq!(detected("Rhodes_Chord.wav"), "PIANO");
        assert_eq!(detected("Organ_Stab.wav"), "ORGAN");
        assert_eq!(detected("Clavinet"), "PIANO");
        assert_eq!(detected("Supersaw Stack"), "SYNTHLEAD");
        assert_eq!(detected("Pluck_C.wav"), "SYNTHLEAD");
        assert_eq!(detected("sub.wav"), "BASS");
        assert_eq!(detected("TB303 Acid"), "BASS");
        assert_eq!(detected("Sweep_Up.wav"), "SOUNDEFFECT");
        assert_eq!(detected("Bassoon"), "REED");
    }

    #[test]
    fn patch_prefixes_name_the_category() {
        assert_eq!(detected("LD Supersaw"), "SYNTHLEAD");
        assert_eq!(detected("PD Warm Strings"), "SYNTHPAD");
        assert_eq!(detected("BA Acid Squelch"), "BASS");
        assert_eq!(detected("KY Rhodes"), "PIANO");
        assert_eq!(detected("ORG Jazz"), "ORGAN");
        // CH is the closed hat's pattern: no prefix.
        assert_eq!(detected("CH 909"), "CHH");
        // A prefix alone is a name, not a category.
        assert_eq!(detected("BA"), NOT_FOUND);
    }

    #[test]
    fn words_name_one_instrument() {
        let c = catalog();
        for i in &c.instruments {
            for word in i.words.iter().chain(&i.weak_words).chain(&i.articulations).chain(&i.patch_prefixes) {
                for other in c.instruments.iter().filter(|o| o.id != i.id) {
                    let named = words(&other.id).contains(word)
                        || other.words.contains(word)
                        || other.synonyms.iter().any(|p| p.eq_ignore_ascii_case(word));
                    assert!(!named, "{word} of {} names {}", i.id, other.id);
                }
            }
        }
    }

    #[test]
    fn abbreviations_and_generic_words() {
        assert_eq!(detected("crsh.wav"), "CRASH");
        assert_eq!(detected("rd01.wav"), "RIDE");
        assert_eq!(detected("shkr.wav"), "SHAKER");
        assert_eq!(detected("Castanets.wav"), "CASTENET");
        assert_eq!(detected("Splash_01.wav"), "CRASH");
        assert_eq!(detected("Acoustic Bass Drum"), "KICK");
        assert_eq!(detected("BassDrum_02.wav"), "KICK");
        assert_eq!(detected("Bass_C2.wav"), "BASS");
        assert_eq!(detected("Tamb_Loop.wav"), "TAMBOURINE");
        assert_eq!(detected("Bassline_Loop_124.wav"), "BASS");
        assert_eq!(detected("Perc Bongo.wav"), "BONGOS");
        assert_eq!(detected("Perc_03.wav"), "PERC");
        assert_eq!(detected("Drum Loop"), "LOOP");
        assert_eq!(detected("Tubular Bells"), "CROMAPERC");
        assert_eq!(detected("Orchestra Hit"), "ENSEMBLE");
        assert_eq!(detected("Synth Drum"), "PERCUSSIVE");
    }

    #[test]
    fn parentheses_articulations_and_bells() {
        assert_eq!(detected("Lead 8 (bass + lead)"), "SYNTHLEAD");
        assert_eq!(detected("Pad 4 (choir)"), "SYNTHPAD");
        assert_eq!(detected("Kick (soft)"), "KICK");
        assert_eq!(detected("Snare Rim.wav"), "RIMSHOT");
        assert_eq!(detected("Snare Side Stick"), "RIMSHOT");
        assert_eq!(detected("Snare Brush.wav"), "SNARE");
        assert_eq!(detected("Bell_C5.wav"), "CROMAPERC");
        assert_eq!(detected("Bell Glass"), "CROMAPERC");
        assert_eq!(detected("Ride_Bell.wav"), "RIDE");
        assert_eq!(detected("Agogo Bell"), "COWBELL");
        assert_eq!(detected("Cowbell"), "COWBELL");
    }

    #[test]
    fn a_kit_reads_cl_beside_rs_as_the_claves() {
        let ids = |names: &[&str]| -> Vec<String> {
            catalog()
                .from_kit_file_names(names)
                .iter()
                .map(|i| i.map_or(NOT_FOUND, |i| i.id.as_str()).to_owned())
                .collect()
        };
        assert_eq!(ids(&["RS.wav", "CL.wav", "BD.wav"]), ["RIMSHOT", "CLAVES", "KICK"]);
        assert_eq!(ids(&["CL.wav", "BD.wav"]), ["RIMSHOT", "KICK"]);
        assert_eq!(ids(&["CL 2.wav", "Rim.wav"]), ["CLAVES", "RIMSHOT"]);
    }

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
        assert_eq!(c.variant("TOM", Some("HI")).map(|i| i.id.as_str()), Some("HI_TOM"));
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
