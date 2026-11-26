//! An oscillator of the synth editor as a strip: its power LED and title on
//! the left, then its controls in one row, each over its label — the wave
//! on a spin that draws it, the knobs (the octave and the detune turn in
//! whole steps), oscillator 2's sync LED. A control an oscillator lacks
//! leaves no gap.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_engine::synth_preset::WAVES;

use crate::panel::{Id, Item, Kind};
use crate::scale::Scale;
use crate::synth_cells::{Cell, push_head, push_row};
use crate::synth_view::knob_kind;

/// The width of the title, left of the controls (at scale 1).
const TITLE_W: i32 = 64;
/// The waves' names in the list a spin opens.
const WAVE_LABELS: [&str; 5] = ["sin", "tri", "saw", "sqr", "rnd"];
/// The points of a wave's picture.
const WAVE_POINTS: usize = 48;

/// A strip's controls in order: the key of the field, the label under it.
const CELLS: [(&str, &str); 7] = [
    ("wave", "Wave"),
    ("octave", "Oct"),
    ("detune", "Cent"),
    ("width", "Width"),
    ("gain", "Gain"),
    ("sync", "Sync"),
    ("ring", "Ring"),
];

/// Whether `group` is drawn as a strip: the oscillators.
#[must_use]
pub fn is_strip(group: &str) -> bool {
    matches!(group, "vco1" | "vco2" | "vco3")
}

/// The strip of `group` (index `gi` of the groups) from `(x, top)`;
/// `fields`: every field, as [`crate::synth_view::all_fields`] names them.
/// The x right of its last control.
pub fn push_strip(
    items: &mut Vec<Item>,
    draft: &Object,
    (group, gi): (&str, usize),
    fields: &[String],
    (x, top): (i32, i32),
    s: Scale,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> i32 {
    push_head(items, draft, (group, gi), (x, top), s);
    let cells = CELLS.iter().filter_map(|(key, label)| {
        let path = format!("{group}.{key}");
        let fi = fields.iter().position(|f| *f == path)?;
        let id = Id::SynthField(fi as u16);
        let kind = match *key {
            "wave" => wave_spin(draft, &path),
            "sync" => Kind::Led { on: model::is_osc2_synced(draft) },
            _ => knob_kind(draft, &path, lfo),
        };
        Some(Cell { id, kind, label: (*label).to_owned() })
    });
    push_row(items, cells, x + TITLE_W * s, top, s)
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
    use crate::panel::SynthTab;
    use crate::synth_view::{SynthLayout, build};

    fn draft() -> Object {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        d
    }

    #[test]
    fn the_strips_stay_put_and_inside_their_card() {
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let built = |d: &Object| build(d, &[], "", SynthTab::Oscillators, &layout, &|_| None);
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
        let strips: Vec<_> = plain.cards.iter().filter(|c| is_strip(&c.group)).collect();
        assert_eq!(strips.len(), 3);
        // As small as its controls: VCO1 has no sync and no ring.
        assert!(strips[0].rect.w < strips[1].rect.w, "{:?} vs {:?}", strips[0].rect, strips[1].rect);
        for card in strips {
            let inside = plain.items.iter().filter(|i| i.rect.y >= card.rect.y && i.rect.y < card.rect.y + card.rect.h);
            for item in inside {
                let r = item.rect;
                assert!(r.x >= card.rect.x && r.x + r.w <= card.rect.x + card.rect.w, "{item:?} in {}", card.group);
                assert!(r.y + r.h <= card.rect.y + card.rect.h, "{item:?} in {}", card.group);
            }
        }
    }
}
