//! The synth editor's controls that are faders standing up, each named by
//! its letter under it: the attack, decay, sustain and release of the
//! envelopes (the filter's included), the filters' frequency and resonance,
//! the first one's drive. Their values are
//! said in their tooltip, not on the card.

use ordrumbox_core::json::Object;

use crate::control::Style;
use crate::panel::Kind;
use crate::synth_cells::value_tip;
use crate::synth_view::knob_kind;

/// Each fader: its card, its key, its letter, its name.
const FADERS: [(&str, &str, &str, &str); 18] = [
    ("envelope", "attack", "A", "Attack"),
    ("envelope", "decay", "D", "Decay"),
    ("envelope", "sustain", "S", "Sustain"),
    ("envelope", "release", "R", "Release"),
    ("modEnvelope", "attack", "A", "Attack"),
    ("modEnvelope", "decay", "D", "Decay"),
    ("modEnvelope", "sustain", "S", "Sustain"),
    ("modEnvelope", "release", "R", "Release"),
    ("filter", "freq", "F", "Frequency"),
    ("filter", "Q", "Q", "Resonance"),
    ("filter", "drive", "D", "Drive"),
    ("filterEnv", "attack", "A", "Attack"),
    ("filterEnv", "decay", "D", "Decay"),
    ("filterEnv", "sustain", "S", "Sustain"),
    ("filterEnv", "release", "R", "Release"),
    ("filter2", "freq", "F", "Frequency"),
    ("filter2", "Q", "Q", "Resonance"),
    ("filter2", "balance", "B", "Balance"),
];

/// The fader field `path` is, if it is one: its letter and its name.
fn fader(path: &str) -> Option<(&'static str, &'static str)> {
    let (group, key) = path.split_once('.')?;
    FADERS.iter().find(|(g, k, ..)| *g == group && *k == key).map(|(.., letter, name)| (*letter, *name))
}

/// The letter under the fader of field `path`, if it is one.
#[must_use]
pub fn letter(path: &str) -> Option<&'static str> {
    fader(path).map(|(letter, _)| letter)
}

/// The fader of field `path`, at the value an LFO gives it now if one moves
/// it, with no value written on it (as its knob); logarithmic when its knob would be.
pub(crate) fn fader_kind(draft: &Object, path: &str, lfo: &dyn Fn(&str) -> Option<f64>) -> Kind {
    match knob_kind(draft, path, lfo) {
        Kind::Value { mut spec, value, label, text, lfo } => {
            spec.style = Style::Fader;
            Kind::Value { spec, value, label, text, lfo }
        }
        Kind::Disabled { .. } => {
            Kind::Disabled { label: letter(path).unwrap_or_default().to_owned(), style: Style::Fader }
        }
        other => other,
    }
}

/// The tooltip of the fader of field `path` of `draft`: the card and the
/// fader's name, then the value.
#[must_use]
pub fn tip(draft: &Object, path: &str) -> Option<[String; 2]> {
    let (_, name) = fader(path)?;
    let group = path.split('.').next().unwrap_or(path);
    value_tip(draft, (group, path), name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_engine::synth_model as model;

    fn draft() -> Object {
        let mut draft = model::defaults();
        model::hydrate(&mut draft);
        draft
    }

    #[test]
    fn the_envelopes_parts_are_faders_named_by_a_letter() {
        assert_eq!(letter("envelope.attack"), Some("A"));
        assert_eq!(letter("modEnvelope.release"), Some("R"));
        assert_eq!(letter("pitchEnv.decay"), None, "two knobs, not an ADSR");
        let draft = draft();
        let kind = fader_kind(&draft, "envelope.sustain", &|_| None);
        assert!(
            matches!(&kind, Kind::Value { spec, text, .. } if spec.style == Style::Fader && text.is_empty()),
            "{kind:?}"
        );
        assert_eq!(tip(&draft, "envelope.decay"), Some(["Envelope Decay".to_owned(), "0.12 s".to_owned()]));
    }

    #[test]
    fn the_filters_frequency_resonance_and_drive_are_faders() {
        assert_eq!(["filter.freq", "filter.Q", "filter.drive"].map(letter), [Some("F"), Some("Q"), Some("D")]);
        assert_eq!(letter("noise.filterFreq"), None, "the noise's filter keeps its knobs");
        let draft = draft();
        let Kind::Value { spec, .. } = fader_kind(&draft, "filter.freq", &|_| None) else { panic!("a fader") };
        // Logarithmic, as its knob was: half of the travel is the middle decade.
        assert!((spec.fraction(632.456) - 0.5).abs() < 1e-5);
        let [title, value] = tip(&draft, "filter.Q").unwrap();
        assert!(title.ends_with("Resonance"), "{title}");
        assert_eq!(value, "1");
        assert!(tip(&draft, "filter.freq").unwrap()[1].contains("Hz"));
    }
}
