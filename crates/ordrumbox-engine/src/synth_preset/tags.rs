//! A synth preset's tags: the instrument it stands for (an id of the
//! instrument catalog: BASS, SYNTHLEAD, KICK…) and its style (dub,
//! chiptune…), which group the presets in the menus that choose one. They
//! are two keys of the preset, `instrument` and `style`; the sound ignores them.

use super::SynthPresets;
use ordrumbox_core::instruments::catalog;
use ordrumbox_core::json::{Object, Value};

/// The key of a preset's instrument.
pub const INSTRUMENT_KEY: &str = "instrument";
/// The key of a preset's style.
pub const STYLE_KEY: &str = "style";

/// The keys of a preset that are tags, not settings of its sound.
pub const TAG_KEYS: [&str; 2] = [INSTRUMENT_KEY, STYLE_KEY];

/// What presets are grouped by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Facet {
    Instrument,
    Style,
}

impl Facet {
    /// Every facet, the instrument first.
    pub const ALL: [Self; 2] = [Self::Instrument, Self::Style];

    /// The preset key the facet is read from.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Instrument => INSTRUMENT_KEY,
            Self::Style => STYLE_KEY,
        }
    }

    /// The tag of `preset` for this facet; `None` when it has none.
    #[must_use]
    pub fn of(self, preset: &Value) -> Option<&str> {
        self.in_object(preset.as_object()?)
    }

    /// [`Facet::of`] on the preset's object.
    #[must_use]
    pub fn in_object(self, preset: &Object) -> Option<&str> {
        preset.get(self.key())?.as_str().filter(|tag| !tag.trim().is_empty())
    }

    /// `text` as this facet's tag is kept: an instrument as its catalog id
    /// (any case), a style trimmed in lower case; empty is no tag (`Ok(None)`).
    /// An instrument the catalog does not hold is refused.
    pub fn parse(self, text: &str) -> Result<Option<String>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        match self {
            Self::Instrument => catalog()
                .by_exact_id(&text.to_uppercase())
                .map(|instrument| Some(instrument.id.clone()))
                .ok_or_else(|| format!("\"{text}\" is no instrument (BASS, SYNTHLEAD, KICK...)")),
            Self::Style => Ok(Some(text.to_lowercase())),
        }
    }
}

/// `preset` with the tag of `facet` set to `tag` (`None`: removed).
pub fn set_tag(preset: &mut Object, facet: Facet, tag: Option<&str>) {
    match tag {
        Some(tag) => {
            preset.insert(facet.key(), Value::from(tag));
        }
        None => {
            preset.remove(facet.key());
        }
    }
}

/// The tags `preset` lacks taken from `from` (the factory preset of the same
/// key, for a copy saved before presets had tags).
pub fn inherit_tags(preset: &mut Value, from: &Value) {
    let Value::Object(object) = preset else { return };
    for facet in Facet::ALL {
        if facet.in_object(object).is_none()
            && let Some(tag) = facet.of(from)
        {
            object.insert(facet.key(), Value::from(tag));
        }
    }
}

/// A group of presets: its tag (`None`: the presets without one) and their keys.
pub type Group<'a> = (Option<&'a str>, Vec<&'a str>);

impl SynthPresets {
    /// The presets grouped by `facet`: the tags in alphabetical order (any
    /// case), each with its presets' keys sorted the same way; the presets
    /// without a tag last, under `None`.
    #[must_use]
    pub fn groups(&self, facet: Facet) -> Vec<Group<'_>> {
        let mut groups: Vec<Group<'_>> = Vec::new();
        for key in self.keys() {
            let tag = self.get(key).and_then(|preset| facet.of(preset));
            match groups.iter_mut().find(|(t, _)| *t == tag) {
                Some((_, keys)) => keys.push(key),
                None => groups.push((tag, vec![key])),
            }
        }
        for (_, keys) in &mut groups {
            keys.sort_by(|a, b| by_name(a, b));
        }
        groups.sort_by(|(a, _), (b, _)| match (a, b) {
            (Some(a), Some(b)) => by_name(a, b),
            _ => a.is_none().cmp(&b.is_none()),
        });
        groups
    }
}

/// Names in alphabetical order, any case (then exactly, so the order is total).
fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    fn presets(text: &str) -> SynthPresets {
        SynthPresets::from_json(&json::parse(text).unwrap()).unwrap()
    }

    #[test]
    fn presets_group_by_instrument_and_by_style() {
        let p = presets(
            r#"{"b2":{"instrument":"BASS","style":"dub"},"lead":{"instrument":"SYNTHLEAD"},
                "B1":{"instrument":"BASS","style":"Acid"},"x":{"style":" "}}"#,
        );
        assert_eq!(
            p.groups(Facet::Instrument),
            [(Some("BASS"), vec!["B1", "b2"]), (Some("SYNTHLEAD"), vec!["lead"]), (None, vec!["x"])]
        );
        assert_eq!(
            p.groups(Facet::Style),
            [(Some("Acid"), vec!["B1"]), (Some("dub"), vec!["b2"]), (None, vec!["lead", "x"])]
        );
    }

    #[test]
    fn a_tag_is_parsed_as_it_is_kept() {
        assert_eq!(Facet::Instrument.parse(" synthLead "), Ok(Some("SYNTHLEAD".to_owned())));
        assert!(Facet::Instrument.parse("kazoo").is_err());
        assert_eq!(Facet::Style.parse(" Dub "), Ok(Some("dub".to_owned())));
        assert_eq!(Facet::Style.parse("  "), Ok(None));
    }

    #[test]
    fn a_preset_without_tags_takes_those_of_the_factory_one() {
        let factory = json::parse(r#"{"instrument":"BASS","style":"dub"}"#).unwrap();
        let mut own = json::parse(r#"{"style":"acid","masterVolume":0.5}"#).unwrap();
        inherit_tags(&mut own, &factory);
        assert_eq!(own, json::parse(r#"{"style":"acid","masterVolume":0.5,"instrument":"BASS"}"#).unwrap());
    }

    #[test]
    fn the_factory_presets_are_all_tagged() {
        let text = include_str!("../../../../assets/data/synth_presets.json");
        let p = SynthPresets::from_json(&json::parse(text).unwrap()).unwrap();
        for facet in Facet::ALL {
            let untagged = p.groups(facet).into_iter().find(|(tag, _)| tag.is_none());
            assert_eq!(untagged, None, "{facet:?}");
        }
        for (instrument, _) in p.groups(Facet::Instrument) {
            let instrument = instrument.unwrap_or_default();
            assert_eq!(Facet::Instrument.parse(instrument), Ok(Some(instrument.to_owned())));
        }
    }
}
