//! The synth editor, in the place of
//! the grid: the preset bar (the preset list, delete, new, duplicate,
//! revert, tags, export, import), the tabs (OSC, FLT, MOD, ENV), and the cards of
//! the tab, each as small as what it holds, side by side — its power LED
//! and title, then its knobs, spins and switches in a row
//! (`synth_cells.rs`) — with the scope (four cycles of the oscillators),
//! the filter curve and the envelope, and the module pills under them. The
//! oscillators are strips, each on a row of its own (`synth_strip.rs`).
//!
//! [`SynthLayout`] and [`build`] place everything as panel items (the ids
//! name groups and fields by their index in [`model::groups`] and
//! [`all_fields`]); [`draw`] paints the cards and the plots under them.

use crate::scale::Scale;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model::{self as model, FILTER_SLOPES, LFO_SYNCS, TABS};
use ordrumbox_engine::synth_preset::{FILTER_TYPES, NOISE_MODES};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::Spec;
use crate::editors::FILTER_ICONS;
use crate::frame;
use crate::panel::{Id, Item, Kind, Rect, SynthTab};
use crate::synth_cells::{Cell, KNOB_H, push_head, push_row};
use crate::synth_strip;
use crate::theme;

/// The preset bar's buttons: id, label, width (scale units).
pub const BUTTONS: [(&str, &str, i32); 7] = [
    ("delete", "x", 20),
    ("new", "+", 20),
    ("duplicate", "Duplicate", 60),
    ("revert", "Revert", 56),
    ("tags", "Tags", 44),
    ("export", "Export", 56),
    ("import", "Import", 56),
];

/// The space inside a card around what it holds, across and down (at scale 1).
const PAD_X: i32 = 6;
const PAD_Y: i32 = 4;
/// The space between two cards (at scale 1).
const CARD_GAP: i32 = 10;
/// The height of a card's head, the space under it included (at scale 1).
const HEAD_H: i32 = 18;
/// The narrowest a plot is (at scale 1): four knobs.
const PLOT_MIN_W: i32 = 280;

fn card_color() -> Rgb {
    theme::panel()
}
fn card_off_color() -> Rgb {
    theme::bg()
}
fn border_color() -> Rgb {
    theme::line()
}
fn surface_color() -> Rgb {
    theme::screen()
}
fn grid_color() -> Rgb {
    theme::panel_2()
}
/// The sound's curves and the switches that are on.
const ACCENT: Rgb = theme::LIVE;
/// The same, a step back: an on switch's ground, the filter's side curves.
fn accent_dim() -> Rgb {
    theme::mix(theme::LIVE, theme::panel(), 0.55)
}

/// Every field of the draft's cards, card after card: the index a
/// [`Id::SynthField`] names.
pub fn all_fields(draft: &Object) -> Vec<String> {
    model::groups(draft).iter().flat_map(|g| model::fields(draft, g)).collect()
}

/// What a field shows as: a knob, a list (values and labels, on a spin), a
/// list on a stepped knob (the LFOs' sync), a wave (on a spin that draws
/// it), a switch, a LED (oscillator 2's sync), or nothing (an object).
enum Field {
    Knob,
    List(Vec<(String, String)>),
    Steps(Vec<(String, String)>),
    Wave,
    Switch,
    Led,
    None,
}

/// The knob of a numeric control.
fn knob_spec(meta: &model::Meta) -> Spec {
    let spec = Spec::knob(meta.min, meta.max, meta.step, meta.default);
    if meta.log { spec.log_full_travel() } else { spec }
}

/// The values, labels and selected index of list field `path` with `options`.
fn list_choice(draft: &Object, path: &str, options: Vec<(String, String)>) -> (Vec<String>, Vec<String>, usize) {
    let current = model::get(draft, path).and_then(Value::to_text).unwrap_or_default();
    let selected = options.iter().position(|(v, _)| *v == current).unwrap_or(0);
    let (values, labels) = options.into_iter().unzip();
    (values, labels, selected)
}

fn field_kind(path: &str, value: Option<&Value>) -> Field {
    let key = path.rsplit('.').next().unwrap_or(path);
    let group = path.split('.').next().unwrap_or(path);
    let is_lfo = matches!(group, "lfo" | "lfo2");
    if key == "wave" {
        return Field::Wave;
    }
    if path == "filter.type" || path == "noise.filterType" {
        return Field::List(
            FILTER_TYPES.iter().zip(FILTER_ICONS).map(|(v, l)| ((*v).to_owned(), l.to_owned())).collect(),
        );
    }
    if path == "fm.algo" {
        return Field::List((0..FM_ALGORITHMS).map(|i| (i.to_string(), (i + 1).to_string())).collect());
    }
    if path == "filter.slope" {
        return Field::List(FILTER_SLOPES.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect());
    }
    if path == "noise.mode" {
        let labels = model::NOISE_MODE_LABELS;
        return Field::List(NOISE_MODES.iter().zip(labels).map(|(v, l)| ((*v).to_owned(), l.to_owned())).collect());
    }
    if path == "vco2.sync" {
        return Field::Led;
    }
    if model::extra_default(path).is_some() {
        return Field::Knob;
    }
    if is_lfo && key == "sync" {
        return Field::Steps(LFO_SYNCS.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect());
    }
    match value {
        Some(Value::Number(_)) => Field::Knob,
        Some(Value::Bool(_)) => Field::Switch,
        _ => Field::None,
    }
}

/// The FM algorithms, offered on a spin as 1 to 5 (a preset holds 0 to 4).
const FM_ALGORITHMS: usize = 5;

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
    scale: Scale,
    pub preset: Rect,
    pub buttons: [Rect; 7],
    pub tabs: [Rect; SynthTab::ALL.len()],
    pub area: Rect,
}

impl SynthLayout {
    /// The editor on a `width` × `height` window (`width` without the track editor).
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = frame::body_top(scale);
        let preset = Rect::new(margin, top, 180 * s, 14 * s);
        let mut x = preset.x + preset.w + 8 * s;
        let buttons = BUTTONS.map(|(_, _, w)| {
            let r = Rect::new(x, top, w * s, 14 * s);
            x += (w + 4) * s;
            r
        });
        let tabs = [0, 1, 2, 3].map(|i| Rect::new(margin + i * 56 * s, top + 20 * s, 52 * s, 14 * s));
        let area_top = top + 42 * s;
        let bottom = frame::footer_top(height, scale, status_lines) - 20 * s;
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

// A synth tab for each of the engine's tabs.
const _: () = assert!(TABS.len() == SynthTab::ALL.len());

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
    tab: SynthTab,
    layout: &SynthLayout,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> Built {
    let s = layout.scale;
    let mut built = Built::default();
    push_header(&mut built.items, presets, current, tab, layout);

    let groups = model::groups(draft);
    let fields = all_fields(draft);
    let mut flow = Flow::new(layout.area, s);
    for (gi, group) in groups.iter().enumerate() {
        if SynthTab::ALL.get(model::group_tab(group)) != Some(&tab) {
            continue;
        }
        // Built from (0, 0), then moved to its place.
        let mut items = Vec::new();
        let origin = (PAD_X * s, PAD_Y * s);
        let is_strip = synth_strip::is_strip(group);
        let (right, bottom, plot) = if is_strip {
            let right = synth_strip::push_strip(&mut items, draft, (group, gi), &fields, origin, s, lfo);
            (right, origin.1 + KNOB_H * s, None)
        } else {
            push_card(&mut items, draft, (group, gi), &fields, origin, s, lfo)
        };
        let (w, h) = (right + PAD_X * s, bottom + PAD_Y * s);
        let (x, y) = flow.place(w, h, is_strip);
        built.items.extend(items.into_iter().map(|item| Item { rect: item.rect.moved(x, y), ..item }));
        built.cards.push(Card {
            group: group.clone(),
            rect: Rect::new(x, y, w, h),
            bypassed: model::has_power(group) && model::bypassed(draft, group),
            plot: plot.map(|(plot, rect): (Plot, Rect)| (plot, rect.moved(x, y))),
        });
    }
    built
}

/// Where the next card goes: left to right, then on the next row.
struct Flow {
    area: Rect,
    gap: i32,
    x: i32,
    top: i32,
    row_h: i32,
}

impl Flow {
    fn new(area: Rect, s: Scale) -> Self {
        Self { area, gap: CARD_GAP * s, x: area.x, top: area.y, row_h: 0 }
    }

    /// The place of a `w` × `h` card; `alone`: on a row of its own.
    fn place(&mut self, w: i32, h: i32, alone: bool) -> (i32, i32) {
        if self.x > self.area.x && (alone || self.x + w > self.area.x + self.area.w) {
            self.next_row();
        }
        let at = (self.x, self.top);
        self.row_h = self.row_h.max(h);
        self.x += w + self.gap;
        if alone {
            self.next_row();
        }
        at
    }

    fn next_row(&mut self) {
        self.top += self.row_h + self.gap;
        self.x = self.area.x;
        self.row_h = 0;
    }
}

/// Card `group` (index `gi` of the groups) from `(x, y)`: its head, its
/// plot, its controls in a row (the matrix: its lines). The x right of what
/// it holds, the y under it, and its plot.
fn push_card(
    items: &mut Vec<Item>,
    draft: &Object,
    (group, gi): (&str, usize),
    fields: &[String],
    (x, y): (i32, i32),
    s: Scale,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> (i32, i32, Option<(Plot, Rect)>) {
    let head_right = push_head(items, draft, (group, gi), (x, y), s);
    let plot_top = y + HEAD_H * s;
    let plot_h = card_plot(group).map_or(0, |(_, h)| (h + 6) * s);
    let top = plot_top + plot_h;
    let (right, bottom) = if group == "matrix" {
        crate::synth_matrix::push_lines(items, draft, fields, (x, top), s)
    } else {
        let cells = field_cells(draft, group, fields, lfo);
        let bottom = if cells.is_empty() { top } else { top + KNOB_H * s };
        (push_row(items, cells, x, top, s), bottom)
    };
    let plot = card_plot(group).map(|(plot, h)| {
        let w = (right - x).max(PLOT_MIN_W * s);
        (plot, Rect::new(x, plot_top, w, h * s))
    });
    let right = plot.map_or(right, |(_, r)| right.max(r.x + r.w)).max(head_right);
    (right, bottom.max(plot_top), plot)
}

/// The preset's select, the buttons and the tabs, above the cards.
fn push_header(items: &mut Vec<Item>, presets: &[String], current: &str, tab: SynthTab, layout: &SynthLayout) {
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
    for ((rect, label), each) in layout.tabs.iter().zip(TABS).zip(SynthTab::ALL) {
        items.push(Item::new(Some(Id::SynthTab(each)), *rect, Kind::Tab { label: label.into(), active: tab == each }));
    }
}

/// The plot a card shows under its title, and its height (unscaled).
fn card_plot(group: &str) -> Option<(Plot, i32)> {
    match group {
        "scope" => Some((Plot::Scope, 60)),
        "filter" => Some((Plot::Filter, 50)),
        "envelope" => Some((Plot::Envelope, 40)),
        _ => None,
    }
}

/// The knob of numeric field `path`, at the value an LFO gives it now if
/// one moves it.
pub(crate) fn knob_kind(draft: &Object, path: &str, lfo: &dyn Fn(&str) -> Option<f64>) -> Kind {
    let meta = model::find_meta(path);
    let key = path.rsplit('.').next().unwrap_or(path);
    let label = meta.and_then(|m| m.label).unwrap_or(key).to_owned();
    let base = model::number(draft, path).or_else(|| model::extra_default(path)).unwrap_or(0.0);
    let spec = meta.map_or_else(|| Spec::knob(0.0, 1.0, 0.01, 0.0), knob_spec);
    let value = lfo(path).map_or(base, |m| m.clamp(spec.min, spec.max));
    let text = crate::control::Format::Fmt.value(value, meta.map_or("", |m| m.unit));
    let lfo_on = model::lfo_targeted(draft, path);
    Kind::Value { spec, value, label: Some(label), text, lfo: lfo_on }
}

/// List field `path` on a knob: a notch per option of `options` (value,
/// label), the option's label as its value.
fn steps_kind(draft: &Object, path: &str, options: Vec<(String, String)>) -> Kind {
    let (_, labels, selected) = list_choice(draft, path, options);
    let last = labels.len().saturating_sub(1).max(1);
    let spec = Spec::knob(0.0, last as f64, 1.0, 0.0);
    let text = labels.get(selected).cloned().unwrap_or_default();
    Kind::Value { spec, value: selected as f64, label: None, text, lfo: false }
}

/// The controls of `group`, in its fields' order, as cells of a row.
fn field_cells(draft: &Object, group: &str, fields: &[String], lfo: &dyn Fn(&str) -> Option<f64>) -> Vec<Cell> {
    let mut cells = Vec::new();
    for path in model::fields(draft, group) {
        let Some(fi) = fields.iter().position(|f| *f == path) else { continue };
        let key = path.rsplit('.').next().unwrap_or(&path);
        let label = model::find_meta(&path).and_then(|m| m.label).unwrap_or(key).to_owned();
        let kind = match field_kind(&path, model::get(draft, &path)) {
            Field::Knob => knob_kind(draft, &path, lfo),
            Field::Wave => synth_strip::wave_spin(draft, &path),
            Field::Steps(options) => steps_kind(draft, &path, options),
            Field::List(options) => {
                let (values, labels, selected) = list_choice(draft, &path, options);
                Kind::spin(values, labels, selected)
            }
            Field::Switch => Kind::Toggle { on: model::get(draft, &path).and_then(Value::as_bool).unwrap_or(false) },
            Field::Led => Kind::Led { on: model::is_osc2_synced(draft) },
            Field::None => continue,
        };
        cells.push(Cell { id: Id::SynthField(fi as u16), kind, label });
    }
    cells
}

/// Paint the cards (under their items), their plots and the module pills.
pub fn draw(canvas: &mut Canvas, draft: &Object, built: &Built, layout: &SynthLayout, time: f64, bpm: f64) {
    let s = layout.scale;
    for card in &built.cards {
        let r = card.rect;
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(
            r.x + s.px(),
            r.y + s.px(),
            r.w - 2 * s,
            r.h - 2 * s,
            if card.bypassed { card_off_color() } else { card_color() },
        );
        if let Some((plot, rect)) = card.plot {
            canvas.fill_rect(rect.x, rect.y, rect.w, rect.h, surface_color());
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
        canvas.fill_rect(x, y, w, 12 * s, if on { accent_dim() } else { card_off_color() });
        canvas.text(x + 4 * s, y + 2 * s, label, s, if on { ACCENT } else { theme::muted() });
        x += w + 4 * s;
    }
}

/// The scope: the samples across, -1 to 1 around the middle.
fn draw_scope(canvas: &mut Canvas, samples: &[f64], r: Rect, s: Scale) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.px(), grid_color());
    let n = samples.len().max(1);
    let points = samples
        .iter()
        .enumerate()
        .map(|(i, v)| (r.x + (i as i32 * r.w) / n as i32, mid - (v * f64::from(r.h / 2 - 4 * s)) as i32));
    canvas.polyline(points, (s * 3 / 2).max(1), ACCENT);
}

/// The envelope: attack, decay, sustain, release.
fn draw_envelope(canvas: &mut Canvas, draft: &Object, r: Rect, s: Scale) {
    let mid = r.y + r.h / 2;
    canvas.fill_rect(r.x, mid, r.w, s.px(), grid_color());
    let (points, total) = model::envelope(draft);
    let point = |(t, v): (f64, f64)| {
        (r.x + ((t / total) * f64::from(r.w - 1)) as i32, mid - (v * f64::from(r.h / 2 - 4 * s)) as i32)
    };
    canvas.polyline(points.into_iter().map(point), (s * 3 / 2).max(1), ACCENT);
}

/// The filter's response, 20 Hz to 20 kHz across, -30 to +12 dB, and the
/// band an LFO on the frequency sweeps.
fn draw_filter(canvas: &mut Canvas, draft: &Object, r: Rect, s: Scale, time: f64, bpm: f64) {
    let to_x = |f: f64| r.x + (((f.log10() - 20f64.log10()) / 3.0) * f64::from(r.w)) as i32;
    let to_y = |db: f64| r.y + r.h - 1 - (((db + 30.0) / 42.0) * f64::from(r.h - 1)) as i32;
    for f in [100.0, 1000.0, 10_000.0] {
        canvas.fill_rect(to_x(f), r.y, s.px(), r.h, grid_color());
    }
    for db in [0.0, -20.0] {
        canvas.fill_rect(r.x, to_y(db), r.w, s.px(), grid_color());
    }
    const N: usize = 200;
    let (kind, fc, q, slope_db, band) = model::filter_now(draft, time, bpm);
    let curve = |canvas: &mut Canvas, fc: f64, color: Rgb, t: i32| {
        let response = model::filter_response(&kind, fc, q, slope_db, N);
        canvas.polyline(
            response.iter().enumerate().map(|(i, db)| (r.x + (i as i32 * (r.w - 1)) / N as i32, to_y(*db))),
            t,
            color,
        );
    };
    if let Some((low, high)) = band {
        curve(canvas, low, accent_dim(), s.px());
        curve(canvas, high, accent_dim(), s.px());
    }
    curve(canvas, fc, ACCENT, (s * 3 / 2).max(1));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_freq_knob_is_logarithmic() {
        let spec = knob_spec(model::find_meta("filter.freq").unwrap());
        // Half of the knob's travel is the middle of the three decades, not 10 kHz.
        assert_eq!(spec.drag(20.0, 100.0, false), 632.0);
        assert_eq!(spec.drag(20.0, 200.0, false), 20_000.0);
        assert!((spec.fraction(632.456) - 0.5).abs() < 1e-5);
    }

    fn draft() -> Object {
        let mut d = model::defaults();
        model::hydrate(&mut d);
        d
    }

    #[test]
    fn builds_every_tab_and_names_fields_by_index() {
        let d = draft();
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let fields = all_fields(&d);
        let mut knobs = 0;
        for tab in SynthTab::ALL {
            let built = build(&d, &["a".into(), "b".into()], "b", tab, &layout, &|_| None);
            for item in &built.items {
                if let (Some(Id::SynthField(fi)), Kind::Value { .. }) = (item.id, &item.kind) {
                    let path = &fields[usize::from(fi)];
                    // A knob is a numeric control, or a list on notches (the LFOs' sync).
                    if model::choices(path).is_none() {
                        assert!(model::find_meta(path).is_some(), "{path}");
                        knobs += 1;
                    }
                }
                assert!(item.rect.x + item.rect.w <= layout.area.x + layout.area.w + 1, "{item:?}");
            }
            assert!(!built.cards.is_empty());
        }
        // Every numeric control but the algorithm (a spin) is a knob.
        assert_eq!(knobs, model::metas().count() - 1);
        let built = build(&d, &["a".into(), "b".into()], "b", SynthTab::Oscillators, &layout, &|_| None);
        assert!(matches!(built.items[0].kind, Kind::Select { selected: 1, .. }));
        // OSC: the scope, the oscillators (vco2, vco3 off: gain 0), the unison, FM (its flag unset).
        let groups: Vec<_> = built.cards.iter().map(|c| (c.group.as_str(), c.bypassed)).collect();
        assert_eq!(
            groups,
            [("scope", false), ("vco1", false), ("vco2", true), ("vco3", true), ("unison", false), ("fm", false)]
        );
    }

    #[test]
    fn the_matrix_card_has_a_line_per_route_and_it_stays_put() {
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let names = ["presets".to_owned()];
        let rects = |d: &Object| {
            let built = build(d, &names, "presets", SynthTab::Modulation, &layout, &|_| None);
            let card = built.cards.iter().find(|c| c.group == "matrix").expect("the matrix card").rect;
            let fields: Vec<_> = built
                .items
                .iter()
                .filter(|i| matches!(i.id, Some(Id::SynthField(fi)) if all_fields(d)[usize::from(fi)].starts_with("matrix.")))
                .map(|i| i.rect)
                .collect();
            (card, fields)
        };
        let plain = draft();
        let (card, fields) = rects(&plain);
        assert_eq!(fields.len(), 3 * ordrumbox_engine::MATRIX_SIZE);
        assert!(card.y + card.h <= layout.area.y + layout.area.h, "{card:?} below {:?}", layout.area);
        assert!(fields.iter().all(|r| r.x >= card.x && r.x + r.w <= card.x + card.w && r.y + r.h <= card.y + card.h));
        // A line set changes no place.
        let mut set = draft();
        model::set(&mut set, "matrix.source1", Value::from("lfo1"));
        model::set(&mut set, "matrix.target1", Value::from("filter.freq"));
        model::set(&mut set, "matrix.amount1", Value::from(-0.7));
        assert_eq!(rects(&set), (card, fields));
    }

    #[test]
    fn draws_at_any_size() {
        let d = draft();
        for (w, h) in [(1, 1), (40, 30), (740, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SynthLayout::new(w as i32, h as i32, Scale::ONE, 0);
            for tab in SynthTab::ALL {
                let built = build(&d, &[], "", tab, &layout, &|_| None);
                draw(&mut canvas, &d, &built, &layout, 0.3, 120.0);
            }
        }
    }
}
