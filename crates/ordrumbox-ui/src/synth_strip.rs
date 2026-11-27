//! An oscillator of the synth editor as a strip, the three of them one
//! under the other in one card (`synth_family.rs`): its power LED and title on
//! the left, then its controls in one row, each over its label — the wave
//! on a spin that draws it, the octave and the semitones on spins, the knobs (their values
//! said in their tooltip, not on the card), oscillator
//! 2's sync LED. A control an oscillator lacks leaves no gap.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_engine::synth_preset::{SEMITONE_RANGE, WAVES};

use crate::panel::{Id, Item, Kind};
use crate::scale::Scale;
use crate::synth_cells::{Cell, Place, QUIET_H, push_head, push_row, value_tip};
use crate::synth_view::knob_kind;

/// The width of the title, left of the controls (at scale 1).
pub(crate) const TITLE_W: i32 = 48;
/// The space between two strips of the oscillators' card (at scale 1).
pub const STRIP_GAP: i32 = 4;
/// The waves' names in the list a spin opens.
const WAVE_LABELS: [&str; 5] = ["sin", "tri", "saw", "sqr", "rnd"];
/// The points of a wave's picture.
const WAVE_POINTS: usize = 48;

/// A strip's controls in order: the key of the field, the label under it,
/// its name in its tooltip.
const CELLS: [(&str, &str, &str); 8] = [
    ("wave", "Wave", "Wave"),
    ("octave", "Oct", "Octave"),
    ("semitone", "Semi", "Semitone"),
    ("detune", "Cent", "Detune"),
    ("width", "Width", "Width"),
    ("gain", "Gain", "Gain"),
    ("sync", "Sync", "Sync"),
    ("ring", "Ring", "Ring"),
];

/// Whether `group` is an oscillator, drawn as a strip of its own controls.
fn is_strip(group: &str) -> bool {
    crate::synth_family::Family::of(group) == Some(crate::synth_family::Family::Oscillators)
}

/// The strip of `group` (index `gi` of the groups) from `(x, top)`;
/// `fields`: every field, as [`crate::synth_view::all_fields`] names them.
/// The x right of its last control.
pub(crate) fn push_strip(
    items: &mut Vec<Item>,
    draft: &Object,
    (group, gi): (&str, usize),
    fields: &[String],
    (x, top): (i32, i32),
    s: Scale,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> i32 {
    push_head(items, draft, (group, gi), (x, top), s);
    let cells = CELLS.iter().filter_map(|(key, label, _)| {
        let path = format!("{group}.{key}");
        let fi = fields.iter().position(|f| *f == path)?;
        let id = Id::SynthField(fi as u16);
        let kind = match *key {
            "wave" => wave_spin(draft, &path),
            "octave" => {
                let range = model::find_meta(&path).map_or((-4.0, 4.0), |m| (m.min, m.max));
                whole_spin(draft, &path, range)
            }
            "semitone" => whole_spin(draft, &path, SEMITONE_RANGE),
            "sync" => Kind::Led { on: model::is_osc2_synced(draft) },
            _ => knob_kind(draft, &path, lfo),
        };
        Some(Cell { id, kind, label: (*label).to_owned(), place: Place::Beside })
    });
    // The fields of other groups the strip shows (VCO1's sub-oscillator), last.
    let borrowed = model::borrowed_fields(group).filter_map(|path| {
        let fi = fields.iter().position(|f| f == path)?;
        let label = model::field_label(path).to_owned();
        Some(Cell { id: Id::SynthField(fi as u16), kind: knob_kind(draft, path, lfo), label, place: Place::Beside })
    });
    push_row(items, cells.chain(borrowed), (x + TITLE_W * s, top), QUIET_H, s)
}

/// The whole number at `path` (an octave, semitones) on a spin, ▲▼ through
/// `min` to `max`, signed.
fn whole_spin(draft: &Object, path: &str, (min, max): (f64, f64)) -> Kind {
    // Whole numbers, the range of a preset's value: saturating is meant.
    let (low, high) = (min.round() as i32, max.round() as i32);
    let current = model::number(draft, path).unwrap_or(0.0).round().clamp(min, max) as i32;
    let values = (low..=high).map(|o| o.to_string()).collect();
    let labels = (low..=high).map(|o| if o > 0 { format!("+{o}") } else { o.to_string() }).collect();
    Kind::spin(values, labels, usize::try_from(current - low).unwrap_or(0))
}

/// The tooltip of a knob or a spin of numbers of field `path` of a strip:
/// its card and its full name, then its value (the wave and the sync: the
/// tooltip of every control, [`crate::synth_cells::field_tip`]).
#[must_use]
pub fn tip(draft: &Object, path: &str) -> Option<[String; 2]> {
    if let Some(card) = model::borrowing_card(path) {
        let name = model::find_meta(path).and_then(|m| m.label)?;
        return is_strip(card).then(|| value_tip(draft, (card, path), name)).flatten();
    }
    let (group, key) = path.split_once('.')?;
    if !is_strip(group) || matches!(key, "wave" | "sync") {
        return None;
    }
    let (.., name) = CELLS.iter().find(|(k, ..)| *k == key)?;
    value_tip(draft, (group, path), name)
}

/// The wave of `path` (an oscillator's or an LFO's) on a spin: its
/// picture, ▲▼ through the waves.
pub(crate) fn wave_spin(draft: &Object, path: &str) -> Kind {
    let current = model::get(draft, path).and_then(Value::to_text).unwrap_or_default();
    let selected = WAVES.iter().position(|w| *w == current).unwrap_or(0);
    Kind::Spin {
        values: WAVES.iter().map(|w| (*w).to_owned()).collect(),
        labels: WAVE_LABELS.iter().map(|l| (*l).to_owned()).collect(),
        selected,
        curve: model::wave_shape(WAVES[selected], WAVE_POINTS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::synth_view::{SynthLayout, build};

    fn draft() -> Object {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        d
    }

    #[test]
    fn the_strips_stay_put_and_inside_their_card() {
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let built = |d: &Object| build(d, (&[], ""), &layout, 0, &|_| None);
        let plain = built(&draft());
        let mut set = draft();
        model::set(&mut set, "vco2.gain", Value::from(0.7));
        model::set(&mut set, "vco2.wave", Value::from("square"));
        model::set(&mut set, "vco2.octave", Value::from(-2.0));
        model::set(&mut set, "vco2.sync", model::osc2_sync_value(true));
        model::toggle_card(&mut set, "vco1");
        let set = built(&set);
        let rects = |b: &crate::synth_view::Built| b.items.iter().map(|i| i.rect).collect::<Vec<_>>();
        assert_eq!(rects(&plain), rects(&set), "a value or a power changes no place");
        let card = plain
            .cards
            .iter()
            .find(|c| c.group == crate::synth_family::Family::Oscillators.name())
            .expect("one card")
            .rect;
        let strips: Vec<_> = plain.cards.iter().filter(|c| is_strip(&c.group)).collect();
        assert_eq!(strips.len(), 3);
        for pair in strips.windows(2) {
            assert!(pair[0].rect.y + pair[0].rect.h <= pair[1].rect.y, "one under the other");
        }
        for strip in strips {
            let r = strip.rect;
            assert!(r.x >= card.x && r.x + r.w <= card.x + card.w && r.y >= card.y && r.y + r.h <= card.y + card.h);
            let in_card = |i: crate::panel::Rect| i.x >= card.x && i.x < card.x + card.w;
            let inside = plain.items.iter().filter(|i| in_card(i.rect) && i.rect.y >= r.y && i.rect.y < r.y + r.h);
            for item in inside {
                let i = item.rect;
                assert!(i.x >= r.x && i.x + i.w <= r.x + r.w && i.y + i.h <= r.y + r.h, "{item:?} in {}", strip.group);
            }
        }
    }

    #[test]
    fn the_octave_is_a_spin_and_the_knobs_say_their_value_in_their_tooltip() {
        let mut d = draft();
        model::set(&mut d, "vco2.octave", Value::from(-2.0));
        let Kind::Spin { values, labels, selected, .. } = whole_spin(&d, "vco2.octave", (-4.0, 4.0)) else {
            panic!("a spin")
        };
        assert_eq!(values.first().map(String::as_str), Some("-4"));
        assert_eq!(labels.last().map(String::as_str), Some("+4"));
        assert_eq!(values[selected], "-2");
        assert_eq!(model::choice_value("vco2.octave", "3"), Value::from(3.0), "a number, as a preset holds it");
        let kind = knob_kind(&d, "vco1.detune", &|_| None);
        assert!(matches!(&kind, Kind::Value { text, .. } if text.is_empty()), "{kind:?}");
        let [title, _] = tip(&d, "vco1.detune").expect("a tip");
        assert!(title.ends_with("Detune"), "{title}");
        assert_eq!(tip(&d, "vco2.octave"), Some(["VCO2 Octave".to_owned(), "-2 oct".to_owned()]));
        assert_eq!(tip(&d, "vco2.wave"), None, "the tooltip of every control says it");
        assert_eq!(tip(&d, "filter.freq"), None);
    }
}
