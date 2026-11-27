//! The cards of the synth editor that hold a family of groups with the same
//! controls, in bands one under the other so that their controls line up: the
//! oscillators (the scope right of VCO1's controls), the two LFOs, and the
//! three envelopes. The main envelope's curve sits right of its faders; the
//! filter and the mod envelopes share the band below it, side by side. Each
//! strip has its power LED and its title.

use crate::panel::{Item, Rect};
use crate::scale::Scale;
use crate::synth_cells::{QUIET_H, push_head, push_row};
use crate::synth_strip::{self, STRIP_GAP, TITLE_W};
use crate::synth_view::{Block, Card, PAD_X, PAD_Y, PLOT_GAP, Plot, SCOPE_MIN_W, Source, field_cells, is_bypassed};

/// The width a curve takes right of its strip's controls, at least (at scale 1).
const CURVE_MIN_W: i32 = 90;

/// A strip of a band, before the rows place its card: its group, left and right edge.
type PlacedStrip<'a> = (&'a str, i32, i32);
/// A band: its top and its strips, from left to right.
type Band<'a> = (i32, Vec<PlacedStrip<'a>>);

/// A family of groups drawn as strips of one card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    Oscillators,
    Lfos,
    Envelopes,
}

impl Family {
    /// Every family, in no particular order (each card goes where its first strip's group is).
    pub const ALL: [Self; 3] = [Self::Oscillators, Self::Lfos, Self::Envelopes];

    /// The family `group` is a strip of, if any.
    #[must_use]
    pub fn of(group: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|family| family.groups().contains(&group))
    }

    /// The groups of the family, in the order of its strips.
    #[must_use]
    pub const fn groups(self) -> &'static [&'static str] {
        match self {
            Self::Oscillators => &["vco1", "vco2", "vco3"],
            Self::Lfos => &["lfo", "lfo2"],
            Self::Envelopes => &["envelope", "filterEnv", "modEnvelope"],
        }
    }

    /// The name of the family's card, as [`Card::group`] holds it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Oscillators => "oscillators",
            Self::Lfos => "lfos",
            Self::Envelopes => "envelopes",
        }
    }

    /// The plot right of strip `group`'s controls: the scope right of the
    /// first oscillator's when the preset has it, the main envelope's curve.
    fn plot(self, group: &str, first: bool, scope: bool) -> Option<Plot> {
        match self {
            Self::Oscillators => (first && scope).then_some(Plot::Scope),
            Self::Envelopes => (first && group == "envelope").then_some(Plot::Envelope),
            Self::Lfos => None,
        }
    }
}

/// The card of `family`: its `strips` (group, index of the groups) in bands one
/// under the other — the filter and the mod envelopes share one, side by side;
/// `scope`: whether the preset has the scope, which then sits right of the
/// first oscillator's controls. The card is wide enough for its plot.
pub(crate) fn family_block(source: &Source, family: Family, strips: &[(&str, usize)], scope: bool) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, top) = (PAD_X * s, PAD_Y * s);
    // Every strip is a quiet row, the envelopes' faders included.
    let strip_h = QUIET_H * s;
    let title_w = title_width(strips, s);
    let half_gap = STRIP_GAP * s / 2;
    let bands = bands_of(family, strips);
    // Each band: its y and its strips' group, left and right edge.
    let mut rows: Vec<Band<'_>> = Vec::new();
    for (band, strips) in bands.into_iter().enumerate() {
        let y = top + band as i32 * (strip_h + STRIP_GAP * s);
        let mut next = x;
        let mut placed = Vec::new();
        for &(group, gi) in strips {
            let right = push_strip(&mut items, source, family, (group, gi), (next, y), title_w);
            placed.push((group, next, right));
            next = right + STRIP_GAP * s;
        }
        rows.push((y, placed));
    }
    let min_w = |plot: Plot| if plot == Plot::Scope { SCOPE_MIN_W } else { CURVE_MIN_W } * s;
    let mut right = x;
    let mut plots = Vec::new();
    for (bi, (y, strips)) in rows.iter().enumerate() {
        for (si, &(group, _, controls_right)) in strips.iter().enumerate() {
            right = right.max(controls_right);
            if let Some(plot) = family.plot(group, bi == 0 && si == 0, scope) {
                let left = controls_right + PLOT_GAP * s;
                right = right.max(left + min_w(plot));
                plots.push((plot, left, *y));
            }
        }
    }
    let (w, h) = (right + PAD_X * s, rows.last().map_or(top, |(y, _)| y + strip_h) + PAD_Y * s);
    let inner_right = w - PAD_X * s;
    let frame =
        Card { group: family.name().into(), rect: Rect::new(0, 0, w, h), bypassed: false, plot: None, framed: true };
    let framed = rows.iter().flat_map(|(y, strips)| {
        strips.iter().map(move |&(group, left, right)| Card {
            group: group.into(),
            rect: Rect::new(left, *y - half_gap, right - left, strip_h + 2 * half_gap),
            bypassed: is_bypassed(source.draft, group),
            plot: None,
            framed: false,
        })
    });
    let plotted = plots.into_iter().map(|(plot, left, y)| {
        let rect = Rect::new(left, y, inner_right - left, strip_h);
        let group = if plot == Plot::Scope { "scope" } else { family.name() };
        Card { group: group.into(), rect, bypassed: false, plot: Some((plot, rect)), framed: false }
    });
    let cards: Vec<Card> = std::iter::once(frame).chain(framed).chain(plotted).collect();
    // A plot takes the width; without one, the controls spread across it, right of the titles.
    let has_plot = cards.iter().any(|c| c.plot.is_some());
    let controls = (!has_plot).then_some((x + title_w, 0));
    Block { items, size: (w, h), cards, matrix: None, controls }
}

/// The strips grouped into bands: the main envelope on its own, the filter
/// and the mod envelopes side by side under it, each other strip on its own.
fn bands_of<'a, 'b>(family: Family, strips: &'a [(&'b str, usize)]) -> Vec<&'a [(&'b str, usize)]> {
    let split = if family == Family::Envelopes {
        strips.iter().position(|(group, _)| *group == "envelope").map(|at| at + 1)
    } else {
        None
    };
    match split {
        Some(at) if at < strips.len() => vec![&strips[..at], &strips[at..]],
        _ => strips.iter().map(std::slice::from_ref).collect(),
    }
}

/// The width the strips' titles take, left of their controls: the
/// oscillators', or the longest title's, its power LED included.
fn title_width(strips: &[(&str, usize)], s: Scale) -> i32 {
    let widest = strips.iter().map(|(group, _)| crate::synth_cells::head_width(group, s)).max().unwrap_or(0);
    (TITLE_W * s).max(widest + PAD_X * s)
}

/// Strip `group` (index `gi` of the groups) of `family` at `(x, y)`, its
/// controls `title_w` right of its head; the x right of its last control.
fn push_strip(
    items: &mut Vec<Item>,
    source: &Source,
    family: Family,
    (group, gi): (&str, usize),
    (x, y): (i32, i32),
    title_w: i32,
) -> i32 {
    let s = source.s;
    if family == Family::Oscillators {
        return synth_strip::push_strip(items, source.draft, (group, gi), source.fields, (x, y), s, source.lfo);
    }
    push_head(items, source.draft, (group, gi), (x, y), s);
    let mut cells = field_cells(source.draft, group, source.fields, source.lfo);
    // The stages (faders) first, so that they line up from strip to strip.
    cells.sort_by_key(|cell| !crate::synth_cells::is_fader(cell));
    push_row(items, cells, (x + title_w, y), QUIET_H, s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::panel::{Id, Kind};
    use crate::synth_view::{Built, SynthLayout, all_fields, build};
    use ordrumbox_core::json::Object;
    use ordrumbox_engine::synth_model as model;

    fn built() -> (Object, Built) {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        (d, built)
    }

    #[test]
    fn the_main_envelope_keeps_its_curve_and_the_other_two_share_its_card() {
        let (d, built) = built();
        let fields = all_fields(&d);
        let rect = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect
        };
        let card = |group: &str| built.cards.iter().find(|c| c.group == group).unwrap_or_else(|| panic!("{group}"));
        let frame = card(Family::Envelopes.name()).rect;
        let curves: Vec<_> = built.cards.iter().filter_map(|c| c.plot).filter(|(p, _)| *p == Plot::Envelope).collect();
        assert_eq!(curves.len(), 1, "only the main envelope's curve");
        let main = card("envelope").rect;
        let (_, curve) = curves[0];
        assert!(curve.y >= main.y && curve.y + curve.h <= main.y + main.h, "in the main band");
        assert!(curve.x + curve.w <= frame.x + frame.w, "in the card");
        let controls = built.items.iter().filter(|i| {
            matches!(i.id, Some(Id::SynthField(_)))
                && i.rect.x >= main.x
                && i.rect.x < main.x + main.w
                && i.rect.y >= main.y
                && i.rect.y < main.y + main.h
        });
        assert!(controls.into_iter().all(|i| i.rect.x + i.rect.w <= curve.x), "right of its faders");
        let filter = card("filterEnv").rect;
        let mod_env = card("modEnvelope").rect;
        assert_eq!(filter.y, mod_env.y, "one band");
        assert!(filter.x + filter.w <= mod_env.x, "side by side");
        assert!(
            rect("filterEnv.attack").y >= rect("envelope.attack").y + QUIET_H * Scale::ONE,
            "under the main envelope"
        );
        for group in ["filterEnv", "modEnvelope"] {
            let band = card(group).rect;
            assert!(band.y >= frame.y && band.y + band.h <= frame.y + frame.h);
            let no_curve = built
                .cards
                .iter()
                .filter_map(|c| c.plot)
                .all(|(p, r)| p != Plot::Envelope || r.y + r.h <= band.y || r.y >= band.y + band.h);
            assert!(no_curve, "{group}: no curve in its band");
        }
        for stage in ["attack", "decay", "sustain", "release"] {
            let filter_x = rect(&format!("filterEnv.{stage}")).x;
            let mod_x = rect(&format!("modEnvelope.{stage}")).x;
            assert_eq!(mod_x - filter_x, mod_env.x - filter.x, "{stage}: their faders line up");
        }
        assert!(matches!(&built.items.iter().find(|i| i.rect == rect("envelope.attack")).unwrap().kind,
            Kind::Value { spec, .. } if spec.style == crate::control::Style::Fader));
    }

    #[test]
    fn the_lfos_share_a_card_their_controls_in_columns() {
        let (d, built) = built();
        let fields = all_fields(&d);
        let x = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect.x
        };
        for key in ["wave", "freq", "sync", "delay", "retrigger"] {
            assert_eq!(x(&format!("lfo.{key}")), x(&format!("lfo2.{key}")), "{key}");
        }
        let frame = built.cards.iter().find(|c| c.group == Family::Lfos.name()).expect("the LFOs' card").rect;
        let bands: Vec<_> = built.cards.iter().filter(|c| c.group == "lfo" || c.group == "lfo2").collect();
        assert_eq!(bands.len(), 2);
        assert!(bands.iter().all(|b| !b.framed && b.rect.y >= frame.y && b.rect.y + b.rect.h <= frame.y + frame.h));
        assert!(bands[0].rect.y + bands[0].rect.h <= bands[1].rect.y, "one under the other");
        assert_eq!(bands[0].rect.h, bands[1].rect.h, "the last band keeps its height");
    }

    #[test]
    fn a_group_is_in_one_family_at_most() {
        assert_eq!(Family::of("vco2"), Some(Family::Oscillators));
        assert_eq!(Family::of("lfo2"), Some(Family::Lfos));
        assert_eq!(Family::of("filterEnv"), Some(Family::Envelopes));
        assert_eq!(Family::of("pitchEnv"), None, "not made of stages");
        assert_eq!(Family::of("filter"), None);
        let all: Vec<_> = Family::ALL.iter().flat_map(|f| f.groups()).collect();
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(all.len(), unique.len(), "a group in two families");
    }
}
