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

/// The catalog's instrument ids by category, in the order the "add track"
/// menus list them. Every id of `data/instruments.json` is in one category.
pub const CATEGORIES: [(&str, &[&str]); 9] = [
    ("Kick & snare", &["KICK", "SNARE", "RIMSHOT", "CLAP"]),
    ("Hats & cymbals", &["CHH", "CHHK", "OHH", "CYM", "CRASH", "RIDE"]),
    ("Toms", &["TOM", "HI_TOM", "MTOM", "LO_TOM"]),
    (
        "Latin",
        &[
            "BONGOS",
            "HI_BONGOS",
            "LO_BONGOS",
            "CONGAS",
            "HI_CONGAS",
            "LO_CONGAS",
            "OHI_CONGAS",
            "CHI_CONGAS",
            "TIMBAL",
            "HI_TIMBAL",
            "LO_TIMBAL",
            "CUICA",
            "GUIRO",
        ],
    ),
    (
        "Percussion",
        &[
            "COWBELL",
            "TAMB",
            "TAMBOURINE",
            "SHAKER",
            "MARACAS",
            "CABA",
            "CLAVES",
            "CASTENET",
            "TRIANGLE",
            "HI_WOODBLOCK",
            "LO_WOODBLOCK",
            "WHISTLE",
            "VIBRA",
            "PERC",
            "PERCUSSIVE",
            "CROMAPERC",
        ],
    ),
    ("Bass & keys", &["BASS", "PIANO", "ORGAN", "GUITAR", "MELO"]),
    ("Strings & winds", &["STRINGS", "ENSEMBLE", "BRASS", "LONGBRASS", "SHORTBRASS", "SAX", "REED", "PIPE", "ETHNIC"]),
    ("Synth", &["SYNTH", "SYNTHLEAD", "SYNTHPAD", "SYNTHEFFECT"]),
    ("FX & loops", &["HIT", "RAZOR", "SCRATCH", "SOUNDEFFECT", "LOOP"]),
];

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
#[expect(clippy::expect_used, reason = "a file built in, read by the tests")]
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

/// Words two strings share.
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

/// Words that pick a variant of an instrument: a tom's pitch, a sound's length.
const QUALIFIER_WORDS: [(&str, &str); 11] = [
    ("FLOOR", "LO"),
    ("HI", "HI"),
    ("HIGH", "HI"),
    ("LO", "LO"),
    ("LOW", "LO"),
    ("MID", "MID"),
    ("MED", "MID"),
    ("MIDDLE", "MID"),
    ("SHORT", "SHORT"),
    ("LONG", "LONG"),
    ("LNG", "LONG"),
];

/// Words read as another one, an id: the agogo plays where a cowbell does, a Rhodes is a piano.
/// They name an instrument, so they count like the id itself.
const WORD_ALIASES: [(&str, &str); 15] = [
    ("AGOGO", "COWBELL"),
    ("PEDAL", "CHHK"),
    ("RHODES", "PIANO"),
    ("WURLY", "PIANO"),
    ("WURLITZER", "PIANO"),
    ("KEYS", "PIANO"),
    ("CLAV", "PIANO"),
    ("EPIANO", "PIANO"),
    ("SUPERSAW", "SYNTHLEAD"),
    ("HOOVER", "SYNTHLEAD"),
    ("REESE", "BASS"),
    ("CHOIR", "ENSEMBLE"),
    ("VOX", "ENSEMBLE"),
    ("VOCAL", "ENSEMBLE"),
    ("VOICE", "ENSEMBLE"),
];

/// Words that say how a sound is played more than what plays it ("Organ Stab" is an organ):
/// they give their id only when no other word of the name finds an instrument.
const WEAK_WORDS: [(&str, &str); 17] = [
    ("SUB", "BASS"),
    ("ACID", "BASS"),
    ("TB", "BASS"),
    ("WOBBLE", "BASS"),
    ("PLUCK", "SYNTHLEAD"),
    ("STAB", "SYNTHLEAD"),
    ("ARP", "SYNTHLEAD"),
    ("SYNC", "SYNTHLEAD"),
    ("SCREAMER", "SYNTHLEAD"),
    ("CHORD", "SYNTHPAD"),
    ("ATMOS", "SYNTHPAD"),
    ("DRONE", "SYNTHPAD"),
    ("SWEEP", "SOUNDEFFECT"),
    ("RISER", "SOUNDEFFECT"),
    ("NOISE", "SOUNDEFFECT"),
    ("IMPACT", "SOUNDEFFECT"),
    ("DOWNLIFTER", "SOUNDEFFECT"),
];

/// Category prefixes of synth patch names ("LD Supersaw", "BA Acid"), and the id they mean.
/// One that is also a catalog word or pattern (`CH`, `FX`, `BELL`) is not a prefix.
const PATCH_PREFIXES: [(&str, &str); 22] = [
    ("BA", "BASS"),
    ("BS", "BASS"),
    ("BAS", "BASS"),
    ("LD", "SYNTHLEAD"),
    ("PL", "SYNTHLEAD"),
    ("PLK", "SYNTHLEAD"),
    ("ARP", "SYNTHLEAD"),
    ("SQ", "SYNTHLEAD"),
    ("SEQ", "SYNTHLEAD"),
    ("CHD", "SYNTHLEAD"),
    ("PD", "SYNTHPAD"),
    ("SY", "SYNTH"),
    ("SYN", "SYNTH"),
    ("KY", "PIANO"),
    ("KEY", "PIANO"),
    ("KB", "PIANO"),
    ("EP", "PIANO"),
    ("PN", "PIANO"),
    ("PNO", "PIANO"),
    ("ORG", "ORGAN"),
    ("STR", "STRINGS"),
    ("GT", "GUITAR"),
];

/// Other spellings of an instrument word: vowels dropped ("shkr"), plurals, names the
/// catalog's patterns miss. None is an id word of another instrument or a word of its patterns
/// (`abbreviations_do_not_collide`): CL stays the rimshot's, though a 808's CL is its claves.
const ABBREVIATIONS: [(&str, &str); 22] = [
    ("BASSDRUM", "KICK"),
    ("CRSH", "CRASH"),
    ("CRS", "CRASH"),
    ("SPLASH", "CRASH"),
    ("CHINESE", "CRASH"),
    ("RD", "RIDE"),
    ("RDE", "RIDE"),
    ("SHKR", "SHAKER"),
    ("SHK", "SHAKER"),
    ("TMB", "TAMB"),
    ("TMBR", "TAMB"),
    ("CWB", "COWBELL"),
    ("CWBL", "COWBELL"),
    ("PRC", "PERC"),
    ("TMBL", "TIMBAL"),
    ("TRNGL", "TRIANGLE"),
    ("WSTL", "WHISTLE"),
    ("MRCS", "MARACAS"),
    ("CASTANET", "CASTENET"),
    ("CASTANETS", "CASTENET"),
    ("VIBRASLAP", "VIBRA"),
    ("WDBLK", "LO_WOODBLOCK"),
];

/// Words that name a kind of sound more than an instrument ("Perc Bongo" is a bongo,
/// "Tamb_Loop" a tambourine): they count only when no other word of the name finds one.
const GENERIC_WORDS: [&str; 6] = ["PERC", "HIT", "LOOP", "DRUM", "CYMBAL", "SYNTH"];

/// The word that names a 808 bass when nothing else in the name does.
const BASS_808: &str = "808";

/// The words of a name, digits removed ("sn01" is SN), aliases and abbreviations
/// applied, and BASS DRUM read as KICK.
fn name_words(s: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for w in words(s) {
        let w: String = w.chars().filter(|c| !c.is_ascii_digit()).collect();
        if w.is_empty() {
            continue;
        }
        let w = WORD_ALIASES
            .iter()
            .chain(ABBREVIATIONS.iter())
            .find(|(word, _)| *word == w)
            .map_or(w, |(_, alias)| (*alias).to_owned());
        // A bass drum is a kick, not a bass.
        match found.last_mut() {
            Some(last) if w == "DRUM" && last == "BASS" => *last = "KICK".to_owned(),
            _ => found.push(w),
        }
    }
    found
}

/// Ids with the qualifier glued to the name: id, its core word, its qualifier.
const GLUED_IDS: [(&str, &str, &str); 3] =
    [("MTOM", "TOM", "MID"), ("LONGBRASS", "BRASS", "LONG"), ("SHORTBRASS", "BRASS", "SHORT")];

/// The qualifier a name states: MID if any ("Low-Mid Tom" is the mid tom), else its first one.
fn qualifier(s: &str) -> Option<&'static str> {
    let found: Vec<&'static str> = name_words(s)
        .iter()
        .filter_map(|w| QUALIFIER_WORDS.iter().find(|(word, _)| word == w).map(|(_, q)| *q))
        .collect();
    if found.contains(&"MID") { Some("MID") } else { found.first().copied() }
}

/// An id's core words and qualifier (`HI_TOM` is TOM, HI; `MTOM` is TOM, MID).
fn id_parts(id: &str) -> (Vec<String>, Option<&'static str>) {
    match GLUED_IDS.iter().find(|(glued, ..)| *glued == id) {
        Some(&(_, core, q)) => (vec![core.to_owned()], Some(q)),
        None => (core_words(id), qualifier(id)),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Openness {
    Open,
    Closed,
}

/// Words that say whether a sound is open or closed (a hi-hat, a conga).
const OPENNESS_WORDS: [(&str, Openness); 4] =
    [("OPEN", Openness::Open), ("OPENED", Openness::Open), ("CLOSE", Openness::Closed), ("CLOSED", Openness::Closed)];

/// Instruments with an open and a closed sound: the ids found, the open one,
/// the closed one, and the one a name that states neither plays.
const OPEN_CLOSED: [(&[&str], &str, &str, Option<&str>); 2] = [
    (&["OHH", "CHH", "CHHK"], "OHH", "CHH", Some("CHH")),
    (&["HI_CONGAS", "OHI_CONGAS", "CHI_CONGAS"], "OHI_CONGAS", "CHI_CONGAS", None),
];

/// The openness a name states, from its first openness word.
fn openness(s: &str) -> Option<Openness> {
    name_words(s).iter().find_map(|w| OPENNESS_WORDS.iter().find(|(word, _)| word == w).map(|(_, o)| *o))
}

/// The words of `s` but the qualifier and openness words.
fn core_words(s: &str) -> Vec<String> {
    name_words(s)
        .into_iter()
        .filter(|w| {
            !QUALIFIER_WORDS.iter().any(|(word, _)| word == w) && !OPENNESS_WORDS.iter().any(|(word, _)| word == w)
        })
        .collect()
}

/// Whether `text` has one of `words`, qualifier and openness words left out.
fn shares_word(text: &str, words: &[String]) -> bool {
    core_words(text).iter().any(|w| words.contains(w))
}

/// The literal parts of a pattern of two words or more (`.*TUBULAR.*BELL.*` is
/// TUBULAR, BELL); none for a pattern with a part of one letter or a qualifier
/// (`.*TOM.*HI.*`), which the qualifier pass handles.
fn compound_parts(pattern: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = pattern.split(['.', '*', '?', ' ']).filter(|p| !p.is_empty()).collect();
    let is_qualifier = |p: &str| {
        let p = p.to_uppercase();
        QUALIFIER_WORDS.iter().any(|(w, _)| *w == p) || OPENNESS_WORDS.iter().any(|(w, _)| *w == p)
    };
    let usable = parts.len() >= 2 && parts.iter().all(|p| p.len() >= 2 && !is_qualifier(p));
    usable.then_some(parts)
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

    /// The first instrument with a pattern matching the whole `name`.
    pub fn by_name(&self, name: &str) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.synonyms.iter().any(|s| pattern_matches(s, name)))
    }

    /// The instrument a file or patch name means. In order: the id itself; a patch
    /// category prefix ("LD Supersaw"); an instrument whose id or GM name shares a
    /// word with the name, then a weak word ("Stab"), then the most specific pattern;
    /// and a lone 808 is a bass. Digits do not count, CamelCase is cut, and the
    /// qualifiers (HI, LOW, MID, LONG...) and OPEN/CLOSED only pick the variant
    /// of what the other words found. This departs from the JS app (`README.md`,
    /// "Golden fixtures").
    pub fn from_file_name(&self, file_name: &str) -> Option<&Instrument> {
        if let Some(found) = self.by_id(file_name.trim()) {
            return Some(found);
        }
        if let Some(found) = self.by_patch_prefix(file_name) {
            return Some(found);
        }
        self.by_words(file_name)
            .or_else(|| words(file_name).iter().any(|w| w == BASS_808).then(|| self.by_exact_id("BASS")).flatten())
    }

    /// The id a patch name's category prefix means, when the prefix is no catalog word.
    fn by_patch_prefix(&self, file_name: &str) -> Option<&Instrument> {
        let all = words(file_name);
        let [first, _, ..] = all.as_slice() else { return None };
        let &(prefix, id) = PATCH_PREFIXES.iter().find(|(prefix, _)| prefix == first)?;
        let collides =
            self.by_name(prefix).is_some() || self.instruments.iter().any(|i| words(&i.id).iter().any(|w| w == prefix));
        if collides { None } else { self.by_exact_id(id) }
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
    /// (`.*ORCH.*HIT.*` for "Orchestra Hit"), the one with the most letters.
    fn by_compound_pattern(&self, file_name: &str) -> Option<&Instrument> {
        let text = name_words(file_name).join(" ");
        let mut best: Option<(&Instrument, usize)> = None;
        for i in &self.instruments {
            for pattern in &i.synonyms {
                let Some(parts) = compound_parts(pattern) else { continue };
                let count = parts.iter().map(|p| p.len()).sum();
                if pattern_matches(pattern, &text) && best.is_none_or(|(_, b)| count > b) {
                    best = Some((i, count));
                }
            }
        }
        best.map(|(i, _)| i)
    }

    /// What `words` find, in order: an id word, a GM name word, a weak word, a pattern.
    fn by_word_list(&self, words: &[String]) -> Option<Base<'_>> {
        if words.is_empty() {
            return None;
        }
        if let Some(found) = self.instruments.iter().find(|i| shares_word(&i.id, words)) {
            return Some(Base::IdWord(found));
        }
        if let Some(found) = self.instruments.iter().find(|i| i.midi.iter().any(|m| shares_word(&m.name, words))) {
            return Some(Base::GmWord(found));
        }
        if let Some(&(_, id)) = words.iter().find_map(|w| WEAK_WORDS.iter().find(|(weak, _)| weak == w)) {
            return self.by_exact_id(id).map(Base::Weak);
        }
        words.iter().find_map(|w| self.by_specific_name(w)).map(Base::Pattern)
    }

    /// The instrument a name's words point to: a pattern of two words, then the
    /// words but the generic ones, then the generic ones; the qualifiers and the
    /// openness pick the variant after.
    fn by_words(&self, file_name: &str) -> Option<&Instrument> {
        // First pass: the qualifiers (HI, LOW, MID...) do not count.
        let wanted = qualifier(file_name);
        let (specific, generic): (Vec<String>, Vec<String>) =
            core_words(file_name).into_iter().partition(|w| !GENERIC_WORDS.contains(&w.as_str()));
        let base = self
            .by_compound_pattern(file_name)
            .map(Base::Pattern)
            .or_else(|| self.by_word_list(&specific))
            .or_else(|| self.by_word_list(&generic))?;
        // Found by a GM name ("Open Hi-Hat"), the instrument says nothing of the name's openness.
        // A pattern can state the qualifier itself (`LT.*` is LO_TOM).
        let (base, level_known, from_gm_name) = match base {
            Base::Weak(found) => return Some(found),
            Base::IdWord(found) => (found, true, false),
            Base::GmWord(found) => (found, true, true),
            Base::Pattern(found) => (found, wanted.is_some(), false),
        };
        let found = if level_known {
            // Second pass: the qualifier picks the variant of the instrument found.
            let (core, _) = id_parts(&base.id);
            self.instruments.iter().find(|i| id_parts(&i.id) == (core.clone(), wanted)).unwrap_or(base)
        } else {
            base
        };
        // Then the openness, for the instruments that have an open and a closed sound.
        let Some(&(_, open, closed, neutral)) =
            OPEN_CLOSED.iter().find(|(members, ..)| members.contains(&found.id.as_str()))
        else {
            return Some(found);
        };
        let id = match openness(file_name) {
            Some(Openness::Open) => Some(open),
            Some(Openness::Closed) => Some(closed),
            None if from_gm_name => neutral,
            None => None,
        };
        Some(id.and_then(|id| self.by_exact_id(id)).unwrap_or(found))
    }

    /// The instrument mapped to `key` on MIDI `channel`.
    pub fn from_midi(&self, channel: i64, key: i64) -> Option<&Instrument> {
        self.instruments.iter().find(|i| i.midi.iter().any(|m| m.channel == channel && m.key == Some(key)))
    }

    /// The instrument for GM `program`: a
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

    /// The instrument's id then its substitutes.
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
    fn abbreviations_do_not_collide() {
        let c = catalog();
        for (abbreviation, id) in ABBREVIATIONS {
            assert!(c.by_exact_id(id).is_some(), "{id}");
            for i in c.instruments.iter().filter(|i| i.id != id) {
                let named = words(&i.id).iter().any(|w| w == abbreviation)
                    || i.synonyms.iter().any(|p| p.split(['.', '*', '?', ' ']).any(|part| part == abbreviation));
                assert!(!named, "{abbreviation} names {}", i.id);
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
        assert_eq!(detected("Tamb_Loop.wav"), "TAMB");
        assert_eq!(detected("Bassline_Loop_124.wav"), "BASS");
        assert_eq!(detected("Perc Bongo.wav"), "BONGOS");
        assert_eq!(detected("Perc_03.wav"), "PERC");
        assert_eq!(detected("Drum Loop"), "LOOP");
        assert_eq!(detected("Tubular Bells"), "CROMAPERC");
        assert_eq!(detected("Orchestra Hit"), "ENSEMBLE");
        assert_eq!(detected("Synth Drum"), "PERCUSSIVE");
    }

    #[test]
    fn catalog_loads() {
        let c = catalog();
        assert_eq!(c.instruments.len(), 66);
        assert_eq!(c.by_id("ohh").map(|i| i.id.as_str()), Some("OHH"));
        assert_eq!(c.program_names.len(), 128);
        assert_eq!(count_common_words("HI_TOM", "tom hi"), 2);
    }

    #[test]
    fn every_instrument_is_in_one_category() {
        let listed: Vec<&str> = CATEGORIES.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
        for instrument in &catalog().instruments {
            let count = listed.iter().filter(|id| **id == instrument.id).count();
            assert_eq!(count, 1, "{}", instrument.id);
        }
        assert_eq!(listed.len(), catalog().instruments.len(), "no id outside the catalog");
    }
}
