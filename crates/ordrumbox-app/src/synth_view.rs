//! The synth editor (`synth_editor.js` and its sections), in the place of
//! the grid: the preset bar (the preset list, delete, new, duplicate,
//! revert, export, import), the tabs (OSC, FLT, MOD, ENV), and the cards of
//! the tab — each with its title, its wave, filter type or algorithm
//! choices, its power button, its knobs, lists and switches — with the scope
//! (four cycles of the oscillators), the filter curve and the envelope, and
//! the module pills under them.
//!
//! [`SynthLayout`] and [`build`] place everything as panel items (the ids
//! name groups and fields by their index in [`synth_model::groups`] and
//! [`all_fields`]); [`draw`] paints the cards and the plots under them.

use ordrumbox_core::json::{Object, Value};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::Spec;
use crate::panel::{Id, Item, Kind, Rect};
use crate::synth_model::{self as model, FILTER_TYPES, LFO_SYNCS, MOD_ENV_TARGETS, TABS, WAVES};
use crate::view;

/// The preset bar's buttons: id, label, width (scale units).
pub const BUTTONS: [(&str, &str, i32); 6] = [
    ("delete", "x", 20),
    ("new", "+", 20),
    ("duplicate", "Dup", 36),
    ("revert", "Revert", 56),
    ("export", "Export", 56),
    ("import", "Import", 56),
];
/// The waves and filter types as their icons are labelled here (the font has no curves).
const WAVE_LABELS: [&str; 5] = ["sin", "tri", "saw", "sqr", "rnd"];
const FILTER_LABELS: [&str; 3] = ["LP", "HP", "BP"];

const CARD_WIDTH: i32 = 330;
const KNOB_W: i32 = 64;
const KNOB_H: i32 = 56;
const ROW_H: i32 = 16;

const CARD: Rgb = 0x001E_1E26;
const CARD_OFF: Rgb = 0x0018_181C;
const BORDER: Rgb = 0x0030_3040;
const SURFACE: Rgb = 0x0024_242E;
const GRID: Rgb = 0x0038_3845;
const ACCENT: Rgb = 0x00F2_A93B;
const ACCENT_DIM: Rgb = 0x0060_4520;
const TEXT: Rgb = 0x00E6_E6EA;
const DIM: Rgb = 0x0080_8090;

/// Every field of the draft's cards, card after card: the index a
/// [`Id::SynthField`] names.
pub fn all_fields(draft: &Object) -> Vec<String> {
    model::groups(draft).iter().flat_map(|g| model::fields(draft, g)).collect()
}

/// What a field shows as: a knob, a list (values and labels), a switch, or
/// nothing (an icon row in the card's title, an object).
enum Field {
    Knob,
    List(Vec<(String, String)>),
    Switch,
    None,
}

fn field_kind(path: &str, value: Option<&Value>) -> Field {
    let key = path.rsplit('.').next().unwrap_or(path);
    let group = path.split('.').next().unwrap_or(path);
    let is_lfo = matches!(group, "lfo" | "lfo2");
    let list =
        |items: &[(&str, &str)]| Field::List(items.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect());
    if key == "wave" || path == "filter.type" || path == "noise.filterType" || path == "fm.algo" {
        return Field::None;
    }
    if path == "modEnvelope.target" {
        return list(&MOD_ENV_TARGETS);
    }
    if is_lfo && key == "target" {
        let targets = model::lfo_targets();
        return Field::List(
            targets
                .iter()
                .map(|t| ((*t).to_owned(), if *t == "NOT" { "off".to_owned() } else { (*t).to_owned() }))
                .collect(),
        );
    }
    if is_lfo && key == "sync" {
        return list(&LFO_SYNCS);
    }
    match value {
        Some(Value::Number(_)) => Field::Knob,
        Some(Value::Bool(_)) => Field::Switch,
        _ => Field::None,
    }
}

/// The choices in a card's title: the field and its options (value, label).
fn icon_choices(group: &str) -> Option<(String, Vec<(Value, &'static str)>)> {
    let vco_or_lfo = group.starts_with("vco") || group.starts_with("lfo");
    if vco_or_lfo {
        Some((format!("{group}.wave"), WAVES.iter().zip(WAVE_LABELS).map(|(w, l)| (Value::from(*w), l)).collect()))
    } else if group == "filter" || group == "noise" {
        let key = if group == "filter" { "type" } else { "filterType" };
        Some((
            format!("{group}.{key}"),
            FILTER_TYPES.iter().zip(FILTER_LABELS).map(|(t, l)| (Value::from(*t), l)).collect(),
        ))
    } else if group == "fm" {
        Some((
            "fm.algo".into(),
            (0..5).map(|i| (Value::from(f64::from(i)), ["1", "2", "3", "4", "5"][i as usize])).collect(),
        ))
    } else {
        None
    }
}

/// A plot of a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plot {
    Scope,
    Filter,
    Envelope,
}

/// Where the editor's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthLayout {
    scale: i32,
    pub preset: Rect,
    pub buttons: [Rect; 6],
    pub tabs: [Rect; 4],
    pub area: Rect,
}

impl SynthLayout {
    /// The editor on a `width` × `height` window (`width` without the track editor).
    pub fn new(width: i32, height: i32, scale: i32, status_lines: usize) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = view::body_top(scale);
        let preset = Rect::new(margin, top, 180 * s, 14 * s);
        let mut x = preset.x + preset.w + 8 * s;
        let buttons = BUTTONS.map(|(_, _, w)| {
            let r = Rect::new(x, top, w * s, 14 * s);
            x += (w + 4) * s;
            r
        });
        let tabs = [0, 1, 2, 3].map(|i| Rect::new(margin + i * 56 * s, top + 20 * s, 52 * s, 14 * s));
        let area_top = top + 42 * s;
        let bottom = view::footer_top(height, scale, status_lines) - 20 * s;
        Self {
            scale,
            preset,
            buttons,
            tabs,
            area: Rect::new(margin, area_top, (width - 2 * margin).max(0), (bottom - area_top).max(0)),
        }
    }
}

/// A card as built: its place, whether it is bypassed, its plot.
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub group: String,
    pub rect: Rect,
    pub bypassed: bool,
    pub plot: Option<(Plot, Rect)>,
}

/// What the editor shows, as built: the panel items, the cards.
#[derive(Debug, Default)]
pub struct Built {
    pub items: Vec<Item>,
    pub cards: Vec<Card>,
}

/// The editor for `draft` (preset `current` of `presets`) on tab `tab`;
/// `lfo`: the value an LFO gives a control now, if it moves it.
pub fn build(
    draft: &Object,
    presets: &[String],
    current: &str,
    tab: u8,
    layout: &SynthLayout,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> Built {
    let s = layout.scale;
    let mut built = Built::default();
    let items = &mut built.items;
    let values = presets.to_vec();
    let selected = presets.iter().position(|p| p == current).unwrap_or(0);
    items.push(Item::new(
        Some(Id::SynthPreset),
        layout.preset,
        Kind::Select { values: values.clone(), labels: values, selected },
    ));
    for ((id, label, _), rect) in BUTTONS.iter().zip(layout.buttons) {
        items.push(Item::new(Some(Id::SynthButton(id)), rect, Kind::Button { label: (*label).into(), on: false }));
    }
    for (i, (rect, label)) in layout.tabs.iter().zip(TABS).enumerate() {
        items.push(Item::new(
            Some(Id::SynthTab(i as u8)),
            *rect,
            Kind::Tab { label: label.into(), active: usize::from(tab) == i },
        ));
    }

    let groups = model::groups(draft);
    let fields = all_fields(draft);
    let card_w = (CARD_WIDTH * s).min(layout.area.w.max(1));
    let columns = ((layout.area.w + 10 * s) / (card_w + 10 * s)).max(1);
    let (mut column, mut row_top, mut row_h) = (0, layout.area.y, 0);
    for (gi, group) in groups.iter().enumerate() {
        if model::group_tab(group) != usize::from(tab) {
            continue;
        }
        if column == columns {
            column = 0;
            row_top += row_h + 10 * s;
            row_h = 0;
        }
        let x = layout.area.x + column * (card_w + 10 * s);
        let inner = Rect::new(x + 6 * s, row_top + 4 * s, card_w - 12 * s, 0);
        let mut y = inner.y;
        // Title: the label, the choices, the power button.
        let label = model::group_label(group);
        items.push(Item::text(inner.x, y + 3 * s, label, TEXT, s));
        if let Some((path, options)) = icon_choices(group) {
            let current = model::get(draft, &path);
            for (k, (value, label)) in options.iter().enumerate() {
                let rect = Rect::new(inner.x + 64 * s + k as i32 * 30 * s, y, 28 * s, 13 * s);
                let on = current.is_some_and(|c| c == value);
                items.push(Item::new(
                    Some(Id::SynthIcon(gi as u8, k as u8)),
                    rect,
                    Kind::Button { label: (*label).into(), on },
                ));
            }
        }
        if model::has_power(group) {
            let rect = Rect::new(inner.x + inner.w - 14 * s, y, 14 * s, 13 * s);
            items.push(Item::new(
                Some(Id::SynthPower(gi as u8)),
                rect,
                Kind::Led { on: !model::bypassed(draft, group) },
            ));
        }
        y += 18 * s;
        let plot = match group.as_str() {
            "scope" => Some((Plot::Scope, 60)),
            "filter" => Some((Plot::Filter, 50)),
            "envelope" => Some((Plot::Envelope, 40)),
            _ => None,
        }
        .map(|(plot, h)| {
            let rect = Rect::new(inner.x, y, inner.w, h * s);
            y += (h + 6) * s;
            (plot, rect)
        });
        // Knobs, in rows.
        let per_row = (inner.w / (KNOB_W * s)).max(1);
        let mut knobs = 0;
        let mut rows = Vec::new();
        for path in model::fields(draft, group) {
            let Some(fi) = fields.iter().position(|f| *f == path) else { continue };
            let id = Some(Id::SynthField(fi as u16));
            let key = path.rsplit('.').next().unwrap_or(&path);
            let meta = model::find_meta(&path);
            let label = meta.and_then(|m| m.label).unwrap_or(key).to_owned();
            match field_kind(&path, model::get(draft, &path)) {
                Field::Knob => {
                    let base = model::number(draft, &path).unwrap_or(0.0);
                    let moved = lfo(&path);
                    let spec = meta.map_or_else(|| Spec::knob(0.0, 1.0, 0.01, 0.0), |m| m.spec());
                    let value = moved.map_or(base, |m| m.clamp(spec.min, spec.max));
                    let unit = meta.map_or("", |m| m.unit);
                    let text = crate::control::Format::Fmt.value(value, unit);
                    let lfo_on = model::lfo_targeted(draft, &path);
                    let rect = Rect::new(
                        inner.x + (knobs % per_row) * KNOB_W * s,
                        y + (knobs / per_row) * (KNOB_H + 4) * s,
                        KNOB_W * s,
                        KNOB_H * s,
                    );
                    items.push(Item::new(id, rect, Kind::Value { spec, value, label: Some(label), text, lfo: lfo_on }));
                    knobs += 1;
                }
                Field::List(options) => rows.push((id, label, Some(options), path)),
                Field::Switch => rows.push((id, label, None, path)),
                Field::None => {}
            }
        }
        if knobs > 0 {
            y += ((knobs + per_row - 1) / per_row) * (KNOB_H + 4) * s;
        }
        for (id, label, options, path) in rows {
            items.push(Item::text(inner.x, y + 3 * s, label, DIM, s));
            let rect = Rect::new(inner.x + 80 * s, y, (inner.w - 80 * s).min(200 * s), 13 * s);
            let kind = match options {
                Some(options) => {
                    let current = model::get(draft, &path).map(ordrumbox_core::js::to_string).unwrap_or_default();
                    let selected = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
                    let (values, labels) = options.into_iter().unzip();
                    Kind::Select { values, labels, selected }
                }
                None => Kind::Toggle { on: model::get(draft, &path).and_then(Value::as_bool).unwrap_or(false) },
            };
            items.push(Item::new(id, rect, kind));
            y += (ROW_H + 2) * s;
        }
        let height = y - row_top + 4 * s;
        row_h = row_h.max(height);
        built.cards.push(Card {
            group: group.clone(),
            rect: Rect::new(x, row_top, card_w, height),
            bypassed: model::has_power(group) && model::bypassed(draft, group),
            plot,
        });
        column += 1;
    }
    built
}

/// Paint the cards (under their items), their plots and the module pills.
pub fn draw(canvas: &mut Canvas, draft: &Object, built: &Built, layout: &SynthLayout, time: f64, bpm: f64) {
    let s = layout.scale;
    for card in &built.cards {
        let r = card.rect;
        canvas.fill_rect(r.x, r.y, r.w, r.h, BORDER);
        canvas.fill_rect(r.x + s, r.y + s, r.w - 2 * s, r.h - 2 * s, if card.bypassed { CARD_OFF } else { CARD });
        if let Some((plot, rect)) = card.plot {
            canvas.fill_rect(rect.x, rect.y, rect.w, rect.h, SURFACE);
            match plot {
                Plot::Scope => draw_scope(canvas, &model::scope(draft, time, bpm), rect, s),
                Plot::Envelope => draw_envelope(canvas, draft, rect, s),
                Plot::Filter => draw_filter(canvas, draft, rect, s, time, bpm),
            }
        }
    }
    // The modules in use, under the cards.
    let mut x = layout.area.x;
    let y = layout.area.y + layout.area.h + 4 * s;
    for (label, on) in model::modules(draft) {
        let w = text_width(label, s) + 8 * s;
        canvas.fill_rect(x, y, w, 12 * s, if on { ACCENT_DIM } else { CARD_OFF });
        canvas.text(x + 4 * s, y + 2 * s, label, s, if on { ACCENT } else { DIM });
        x += w + 4 * s;
    }
}

/// A line from `a` to `b`, `t` thick.
fn line(canvas: &mut Canvas, (x0, y0): (i32, i32), (x1, y1): (i32, i32), t: i32, color: Rgb) {
    let steps = (x1 - x0).abs().max((y1 - y0).abs()).max(1);
    for i in 0..=steps {
        canvas.fill_rect(x0 + (x1 - x0) * i / steps - t / 2, y0 + (y1 - y0) * i / steps - t / 2, t, t, color);
    }
}

fn polyline(canvas: &mut Canvas, points: impl Iterator<Item = (i32, i32)>, t: i32, color: Rgb) {
    let mut previous = None;
    for p in points {
        if let Some(q) = previous {
            line(canvas, q, p, t, color);
        }
        previous = Some(p);
    }
}

/// The scope: the samples across, -1 to 1 around the middle.
fn draw_scope(canvas: &mut Canvas, samples: &[f64], r: Rect, s: i32) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.max(1), GRID);
    let n = samples.len().max(1);
    let points = samples
        .iter()
        .enumerate()
        .map(|(i, v)| (r.x + (i as i32 * r.w) / n as i32, mid - (v * f64::from(r.h / 2 - 4 * s)) as i32));
    polyline(canvas, points, (s * 3 / 2).max(1), ACCENT);
}

/// The envelope: attack, decay, sustain, release.
fn draw_envelope(canvas: &mut Canvas, draft: &Object, r: Rect, s: i32) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.max(1), GRID);
    let (points, total) = model::envelope(draft);
    let point = |(t, v): (f64, f64)| {
        (r.x + ((t / total) * f64::from(r.w - 1)) as i32, mid - (v * f64::from(r.h / 2 - 4 * s)) as i32)
    };
    polyline(canvas, points.into_iter().map(point), (s * 3 / 2).max(1), ACCENT);
}

/// The filter's response, 20 Hz to 20 kHz across, -30 to +12 dB, and the
/// band an LFO on the frequency sweeps.
fn draw_filter(canvas: &mut Canvas, draft: &Object, r: Rect, s: i32, time: f64, bpm: f64) {
    let to_x = |f: f64| r.x + (((f.log10() - 20f64.log10()) / 3.0) * f64::from(r.w)) as i32;
    let to_y = |db: f64| r.y + r.h - 1 - (((db + 30.0) / 42.0) * f64::from(r.h - 1)) as i32;
    for f in [100.0, 1000.0, 10_000.0] {
        canvas.fill_rect(to_x(f), r.y, s.max(1), r.h, GRID);
    }
    for db in [0.0, -20.0] {
        canvas.fill_rect(r.x, to_y(db), r.w, s.max(1), GRID);
    }
    const N: usize = 200;
    let (kind, fc, q, band) = model::filter_now(draft, time, bpm);
    let curve = |canvas: &mut Canvas, fc: f64, color: Rgb, t: i32| {
        let response = model::filter_response(&kind, fc, q, N);
        polyline(
            canvas,
            response.iter().enumerate().map(|(i, db)| (r.x + (i as i32 * (r.w - 1)) / N as i32, to_y(*db))),
            t,
            color,
        );
    };
    if let Some((low, high)) = band {
        curve(canvas, low, ACCENT_DIM, s.max(1));
        curve(canvas, high, ACCENT_DIM, s.max(1));
    }
    curve(canvas, fc, ACCENT, (s * 3 / 2).max(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> Object {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        d
    }

    #[test]
    fn builds_every_tab_and_names_fields_by_index() {
        let d = draft();
        let layout = SynthLayout::new(740, 680, 1, 0);
        let fields = all_fields(&d);
        let mut knobs = 0;
        for tab in 0..4 {
            let built = build(&d, &["a".into(), "b".into()], "b", tab, &layout, &|_| None);
            for item in &built.items {
                if let (Some(Id::SynthField(fi)), Kind::Value { .. }) = (item.id, &item.kind) {
                    assert!(model::find_meta(&fields[usize::from(fi)]).is_some(), "{}", fields[usize::from(fi)]);
                    knobs += 1;
                }
                assert!(item.rect.x + item.rect.w <= layout.area.x + layout.area.w + 1, "{item:?}");
            }
            assert!(!built.cards.is_empty());
        }
        // Every numeric control but the algorithm (an icon row) is a knob.
        assert_eq!(knobs, model::metas().count() - 1);
        let built = build(&d, &["a".into(), "b".into()], "b", 0, &layout, &|_| None);
        assert!(matches!(built.items[0].kind, Kind::Select { selected: 1, .. }));
        // OSC: the scope, the oscillators (vco2, vco3 off: gain 0), FM (its flag unset).
        let groups: Vec<_> = built.cards.iter().map(|c| (c.group.as_str(), c.bypassed)).collect();
        assert_eq!(groups, [("scope", false), ("vco1", false), ("vco2", true), ("vco3", true), ("fm", false)]);
    }

    #[test]
    fn draws_at_any_size() {
        let d = draft();
        for (w, h) in [(1, 1), (40, 30), (740, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SynthLayout::new(w as i32, h as i32, 1, 0);
            for tab in 0..4 {
                let built = build(&d, &[], "", tab, &layout, &|_| None);
                draw(&mut canvas, &d, &built, &layout, 0.3, 120.0);
            }
        }
    }
}
