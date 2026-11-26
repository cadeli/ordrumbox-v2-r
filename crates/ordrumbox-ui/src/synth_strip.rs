//! An oscillator of the synth editor as a strip across it: its power LED and
//! title on the left, then its controls in one row, each over its label —
//! the wave on a screen with ▲ and ▼, the knobs (the octave and the
//! detune turn in whole steps), oscillator 2's sync LED. A control an oscillator lacks leaves no gap.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_engine::synth_preset::WAVES;

use crate::canvas::text_width;
use crate::panel::{Id, Item, Kind, Rect, SPIN_SCREEN_H};
use crate::scale::Scale;
use crate::synth_view::{KNOB_H, KNOB_W, knob_item};
use crate::theme;

/// The width of the title, left of the controls (at scale 1).
const TITLE_W: i32 = 64;
/// The space between two controls (at scale 1).
const GAP: i32 = 8;
/// The waves' names in the list a spin opens.
const WAVE_LABELS: [&str; 5] = ["sin", "tri", "saw", "sqr", "rnd"];
/// How far a screen sits below the top of a knob, so that their labels line up.
const SCREEN_TOP: i32 = 5;
/// The points of a wave's picture.
const WAVE_POINTS: usize = 48;

/// How a control of a strip shows.
#[derive(Debug, Clone, Copy)]
enum Cell {
    /// The wave, drawn on a screen.
    Wave,
    /// A knob, under this label.
    Knob(&'static str),
    /// A LED (the sync), under this label.
    Led(&'static str),
}

/// A strip's controls in order: the key of the field, how it shows, its
/// width (at scale 1).
const CELLS: [(&str, Cell, i32); 7] = [
    ("wave", Cell::Wave, 64),
    ("octave", Cell::Knob("Oct"), KNOB_W),
    ("detune", Cell::Knob("Cent"), KNOB_W),
    ("width", Cell::Knob("Width"), KNOB_W),
    ("gain", Cell::Knob("Gain"), KNOB_W),
    ("sync", Cell::Led("Sync"), 40),
    ("ring", Cell::Knob("Ring"), KNOB_W),
];

/// Whether `group` is drawn as a strip: the oscillators.
#[must_use]
pub fn is_strip(group: &str) -> bool {
    matches!(group, "vco1" | "vco2" | "vco3")
}

/// The strip of `group` (index `gi` of the groups) at the top of `inner`;
/// `fields`: every field, as [`crate::synth_view::all_fields`] names them.
/// The y under it.
pub fn push_strip(
    items: &mut Vec<Item>,
    draft: &Object,
    (group, gi): (&str, usize),
    fields: &[String],
    inner: Rect,
    s: Scale,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> i32 {
    let top = inner.y;
    let power = Rect::new(inner.x, top, 14 * s, 13 * s);
    items.push(Item::new(Some(Id::SynthPower(gi as u8)), power, Kind::Led { on: !model::bypassed(draft, group) }));
    items.push(Item::text(inner.x + 18 * s, top + 3 * s, model::group_label(group), theme::text(), s));
    let mut x = inner.x + TITLE_W * s;
    for (key, cell, width) in CELLS {
        let path = format!("{group}.{key}");
        let Some(fi) = fields.iter().position(|f| *f == path) else { continue };
        let w = width * s;
        let screen = Rect::new(x, top + SCREEN_TOP * s, w, SPIN_SCREEN_H * s);
        match cell {
            Cell::Wave => {
                items.push(Item::new(Some(Id::SynthField(fi as u16)), screen, wave_spin(draft, &path)));
                push_label(items, screen, "Wave", s);
            }
            Cell::Knob(label) => {
                let mut knob = knob_item(draft, &path, fi, Rect::new(x, top, w, KNOB_H * s), lfo);
                if let Kind::Value { label: shown, .. } = &mut knob.kind {
                    *shown = Some(label.to_owned());
                }
                items.push(knob);
            }
            Cell::Led(label) => {
                let on = model::is_osc2_synced(draft);
                items.push(Item::new(Some(Id::SynthField(fi as u16)), screen, Kind::Led { on }));
                push_label(items, screen, label, s);
            }
        }
        x += w + GAP * s;
    }
    top + KNOB_H * s
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

/// `label` centred under `screen`, where a knob has its own.
fn push_label(items: &mut Vec<Item>, screen: Rect, label: &str, s: Scale) {
    let x = screen.x + (screen.w - text_width(label, s)) / 2;
    items.push(Item::text(x, screen.y + screen.h + 4 * s, label, theme::muted(), s));
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
        for card in strips {
            assert_eq!(card.rect.w, layout.area.w, "{} across the editor", card.group);
            let inside = plain.items.iter().filter(|i| i.rect.y >= card.rect.y && i.rect.y < card.rect.y + card.rect.h);
            for item in inside {
                let r = item.rect;
                assert!(r.x >= card.rect.x && r.x + r.w <= card.rect.x + card.rect.w, "{item:?} in {}", card.group);
                assert!(r.y + r.h <= card.rect.y + card.rect.h, "{item:?} in {}", card.group);
            }
        }
    }
}
