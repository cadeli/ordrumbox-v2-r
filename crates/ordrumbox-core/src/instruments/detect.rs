//! What a file or patch name means: the catalog's name rules (words, CamelCase,
//! parentheses, patterns, qualifiers, openness) applied to find its instrument,
//! alone or among the other files of its kit.

use super::{Catalog, Instrument, Openness};

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instruments::catalog;

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
        catalog().from_file_name(name).map_or("", |i| i.id.as_str())
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
        assert_eq!(detected("BA"), "");
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
            catalog().from_kit_file_names(names).iter().map(|i| i.map_or("", |i| i.id.as_str()).to_owned()).collect()
        };
        assert_eq!(ids(&["RS.wav", "CL.wav", "BD.wav"]), ["RIMSHOT", "CLAVES", "KICK"]);
        assert_eq!(ids(&["CL.wav", "BD.wav"]), ["RIMSHOT", "KICK"]);
        assert_eq!(ids(&["CL 2.wav", "Rim.wav"]), ["CLAVES", "RIMSHOT"]);
    }

    #[test]
    fn a_qualifier_picks_the_variant_of_a_group() {
        assert_eq!(catalog().variant("TOM", Some("HI")).map(|i| i.id.as_str()), Some("HI_TOM"));
    }
}
