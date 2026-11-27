//! The cards of the synth editor that hold a family of groups with the same
//! controls, in bands one under the other so that their controls line up: the
//! oscillators (the scope right of VCO1's controls), the two LFOs, and the
//! three envelopes. The envelopes share one band, side by side, each title
//! over its faders, the main envelope's curve right of its own. Each strip
//! has its power LED and its title.

use crate::panel::{Item, Rect};
use crate::scale::Scale;
use crate::synth_cells::{QUIET_H, push_head, push_row};
use crate::synth_strip::{self, STRIP_GAP, TITLE_W};
use crate::synth_view::{
    Block, Card, HEAD_H, PAD_X, PAD_Y, PLOT_GAP, Plot, SCOPE_MIN_W, Source, field_cells, is_bypassed,
};

/// The width a curve takes right of its strip's controls, at least (at scale 1).
const CURVE_MIN_W: i32 = 60;
/// The space between two strips side by side in a band (at scale 1).
pub(crate) const BESIDE_GAP: i32 = 16;

/// A strip of a band, before the rows place its card: its group, left and right edge.
type PlacedStrip<'a> = (&'a str, i32, i32);
/// A band: its top and its strips, from left to right.
type Band<'a> = (i32, Vec<PlacedStrip<'a>>);
/// A plot: what, its left and top, its width when it does not reach the card's edge.
type PlotPlace = (Plot, i32, i32, Option<i32>);

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

    /// The plot right of the first strip's controls: the scope right of the
    /// first oscillator's when the preset has it, the main envelope's curve
    /// right of the envelope's, before the others.
    fn plot(self, first: bool, scope: bool) -> Option<Plot> {
        match self {
            Self::Oscillators => (first && scope).then_some(Plot::Scope),
            Self::Envelopes => first.then_some(Plot::Envelope),
            Self::Lfos => None,
        }
    }

    /// The height of a strip: a quiet row, its title on its left — or over
    /// it, for the envelopes, which stand side by side.
    fn strip_height(self, s: Scale) -> i32 {
        match self {
            Self::Envelopes => (HEAD_H + QUIET_H) * s,
            Self::Oscillators | Self::Lfos => QUIET_H * s,
        }
    }
}

/// The card of `family`: its `strips` (group, index of the groups) in bands one
/// under the other — the envelopes share one, side by side, the main envelope's
/// curve between it and the others; `scope`: whether the preset has the scope,
/// which then sits right of the first oscillator's controls. The card is wide
/// enough for its plot.
pub(crate) fn family_block(source: &Source, family: Family, strips: &[(&str, usize)], scope: bool) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, top) = (PAD_X * s, PAD_Y * s);
    let strip_h = family.strip_height(s);
    let title_w = title_width(strips, s);
    let half_gap = STRIP_GAP * s / 2;
    let (rows, plots) = push_bands(&mut items, source, family, strips, title_w, scope);
    let min_w = |plot: Plot| if plot == Plot::Scope { SCOPE_MIN_W } else { CURVE_MIN_W } * s;
    let right = rows
        .iter()
        .flat_map(|(_, strips)| strips)
        .map(|&(_, _, right)| right)
        .chain(plots.iter().map(|&(plot, left, _, width)| left + width.unwrap_or_else(|| min_w(plot))))
        .max()
        .unwrap_or(x);
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
    let plotted = plots.into_iter().map(|(plot, left, y, width)| {
        let rect = Rect::new(left, y, width.map_or(inner_right, |w| left + w) - left, strip_h);
        let group = if plot == Plot::Scope { "scope" } else { family.name() };
        Card { group: group.into(), rect, bypassed: false, plot: Some((plot, rect)), framed: false }
    });
    let cards: Vec<Card> = std::iter::once(frame).chain(framed).chain(plotted).collect();
    // A plot takes the width; without one, the controls spread across it, right of the titles.
    let has_plot = cards.iter().any(|c| c.plot.is_some());
    let controls = (!has_plot).then_some((x + title_w, 0));
    Block { items, size: (w, h), cards, matrix: None, controls }
}

/// Place the strips of each band and the plot right of its strip: a plot with
/// strips after it keeps its own width; one at a band's end grows with the card.
fn push_bands<'b>(
    items: &mut Vec<Item>,
    source: &Source<'_>,
    family: Family,
    strips: &[(&'b str, usize)],
    title_w: i32,
    scope: bool,
) -> (Vec<Band<'b>>, Vec<PlotPlace>) {
    let s = source.s;
    let (x, top) = (PAD_X * s, PAD_Y * s);
    let strip_h = family.strip_height(s);
    let min_w = |plot: Plot| if plot == Plot::Scope { SCOPE_MIN_W } else { CURVE_MIN_W } * s;
    let bands = bands_of(family, strips);
    let mut rows = Vec::new();
    let mut plots = Vec::new();
    for (bi, strips) in bands.into_iter().enumerate() {
        let y = top + bi as i32 * (strip_h + STRIP_GAP * s);
        let mut next = x;
        let mut placed = Vec::new();
        for (si, &(group, gi)) in strips.iter().enumerate() {
            let right = push_strip(items, source, family, (group, gi), (next, y), title_w);
            placed.push((group, next, right));
            next = right + BESIDE_GAP * s;
            let Some(plot) = family.plot(bi == 0 && si == 0, scope) else { continue };
            let left = right + PLOT_GAP * s;
            let width = (si + 1 < strips.len()).then(|| min_w(plot));
            next = left + width.unwrap_or_else(|| min_w(plot)) + BESIDE_GAP * s;
            plots.push((plot, left, y, width));
        }
        rows.push((y, placed));
    }
    (rows, plots)
}

/// The strips grouped into bands: the envelopes side by side in one, each
/// other strip on its own.
fn bands_of<'a, 'b>(family: Family, strips: &'a [(&'b str, usize)]) -> Vec<&'a [(&'b str, usize)]> {
    if family == Family::Envelopes { vec![strips] } else { strips.iter().map(std::slice::from_ref).collect() }
}

/// The width the strips' titles take, left of their controls: the
/// oscillators', or the longest title's, its power LED included.
fn title_width(strips: &[(&str, usize)], s: Scale) -> i32 {
    let widest = strips.iter().map(|(group, _)| crate::synth_cells::head_width(group, s)).max().unwrap_or(0);
    (TITLE_W * s).max(widest + PAD_X * s)
}

/// Strip `group` (index `gi` of the groups) of `family` at `(x, y)`, its
/// controls `title_w` right of its head (an envelope's: under it); the x
/// right of its last control, or of its title.
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
    let head_right = push_head(items, source.draft, (group, gi), (x, y), s);
    let mut cells = field_cells(source.draft, group, source.fields, source.lfo);
    // The stages (faders) first, so that they line up from strip to strip.
    cells.sort_by_key(|cell| !crate::synth_cells::is_fader(cell));
    if family == Family::Envelopes {
        return push_row(items, cells, (x, y + HEAD_H * s), QUIET_H, s).max(head_right);
    }
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
    fn the_envelopes_share_one_band_titles_over_faders_the_curve_beside_the_main() {
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
        let (_, curve) = curves[0];
        assert!(curve.x + curve.w <= frame.x + frame.w, "in the card");
        let groups = Family::Envelopes.groups();
        let bands: Vec<Rect> = groups.iter().map(|g| card(g).rect).collect();
        for pair in bands.windows(2) {
            assert_eq!(pair[0].y, pair[1].y, "one band");
            assert!(pair[0].x + pair[0].w <= pair[1].x, "side by side");
        }
        let main = bands[0];
        assert!(curve.x >= main.x + main.w, "right of the main envelope");
        assert!(curve.x + curve.w <= bands[1].x, "before the filter envelope");
        assert!(curve.w >= CURVE_MIN_W * Scale::ONE, "wide enough for the curve");
        assert_eq!(
            (curve.y, curve.h),
            (main.y + STRIP_GAP * Scale::ONE / 2, main.h - STRIP_GAP * Scale::ONE),
            "as tall"
        );
        for (group, band) in groups.iter().zip(&bands) {
            assert!(band.y >= frame.y && band.y + band.h <= frame.y + frame.h, "{group}: in the card");
            let title = built
                .items
                .iter()
                .find(|i| matches!(&i.kind, Kind::Text { text, .. } if *text == model::group_label(group)))
                .unwrap_or_else(|| panic!("{group}: its title"))
                .rect;
            let attack = rect(&format!("{group}.attack"));
            assert!(attack.y >= title.y + title.h, "{group}: its title over its faders");
            assert_eq!(attack.x, band.x, "{group}: its faders under its title");
        }
        for stage in ["attack", "decay", "sustain", "release"] {
            assert_eq!(rect(&format!("envelope.{stage}")).y, rect(&format!("modEnvelope.{stage}")).y, "{stage}");
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
