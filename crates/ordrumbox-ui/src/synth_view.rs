//! The synth editor, in the place of
//! the grid: the preset bar (the preset list, delete, new, duplicate,
//! revert, tags, export, import), the line of the track it is for, and
//! every card on one screen — what makes the sound (the sources, the
//! filter, the envelopes, the master), then what moves it (the LFOs, the
//! modulation envelope, the matrix) — each as small as what it holds, side by side — its power LED
//! and title, then its plot (the filter curve, the envelope) left of its
//! knobs, faders, spins and switches in a row (`synth_cells.rs`; the
//! filter's slope under its type) — and the
//! module pills under them. The oscillators are strips one under the other
//! in one card (`synth_strip.rs`), the scope (four cycles of them), with no
//! title, in the room VCO1's strip leaves right of its controls.
//!
//! [`SynthLayout`] and [`build`] place everything as panel items (the ids
//! name groups and fields by their index in [`model::groups`] and
//! [`all_fields`]); [`draw`] paints the cards and the plots under them.

use crate::scale::Scale;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model::{self as model, FILTER_SLOPES, LFO_SYNCS};
use ordrumbox_engine::synth_preset::{FILTER_TYPES, NOISE_MODES};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::Spec;
use crate::editors::FILTER_ICONS;
use crate::frame;
use crate::panel::{Id, Item, Kind, Rect};
use crate::synth_cells::{Cell, KNOB_H, Place, QUIET_H, push_head, push_row};
use crate::synth_faders;
use crate::synth_matrix::MatrixList;
use crate::synth_strip;
use crate::{scroll_thumb, theme};

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
const CARD_GAP: i32 = 8;
/// The height of a card's head, the space under it included (at scale 1).
const HEAD_H: i32 = 18;
/// A plot's width left of its card's controls, and the space right of it (at scale 1).
const PLOT_W: i32 = 120;
const PLOT_GAP: i32 = 8;
/// The scope in the oscillators' card: its narrowest (at scale 1), else
/// it has a card of its own, its plot this high.
const SCOPE_MIN_W: i32 = 60;
const SCOPE_H: i32 = 60;

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
    /// The line under the preset bar: what the track edited plays.
    pub track_line: Rect,
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
        let track_line = Rect::new(margin, top + 20 * s, (width - 2 * margin).max(0), 14 * s);
        let area_top = top + 42 * s;
        let bottom = frame::footer_top(height, scale, status_lines) - 20 * s;
        Self {
            scale,
            preset,
            buttons,
            track_line,
            area: Rect::new(margin, area_top, (width - 2 * margin).max(0), (bottom - area_top).max(0)),
        }
    }
}

/// A card as built: its place, whether it is bypassed, its plot; `framed`:
/// drawn with its border (an oscillator's strip is a band of the
/// oscillators' card, shaded only when bypassed).
#[derive(Debug, Clone, PartialEq)]
pub struct Card {
    pub group: String,
    pub rect: Rect,
    pub bypassed: bool,
    pub plot: Option<(Plot, Rect)>,
    pub framed: bool,
}

impl Card {
    fn moved(self, x: i32, y: i32) -> Self {
        Self { rect: self.rect.moved(x, y), plot: self.plot.map(|(plot, r)| (plot, r.moved(x, y))), ..self }
    }
}

/// The group name of the card that holds the oscillators' strips.
pub const OSCILLATORS: &str = "oscillators";

/// What the editor shows, as built: the panel items, the cards, the
/// matrix's list.
#[derive(Debug, Default)]
pub struct Built {
    pub items: Vec<Item>,
    pub cards: Vec<Card>,
    pub matrix: Option<MatrixList>,
}

/// A card (or the oscillators' card) built from (0, 0), before the flow places it.
struct Block {
    items: Vec<Item>,
    size: (i32, i32),
    cards: Vec<Card>,
    matrix: Option<MatrixList>,
}

impl Built {
    /// `block` at its place in `flow`.
    fn place(&mut self, flow: &mut Flow, block: Block) {
        let (x, y) = flow.place(block.size.0, block.size.1);
        self.items.extend(block.items.into_iter().map(|item| Item { rect: item.rect.moved(x, y), ..item }));
        self.cards.extend(block.cards.into_iter().map(|card| card.moved(x, y)));
        if let Some(list) = block.matrix {
            self.matrix = Some(list.moved(x, y));
        }
    }
}

/// What a card is built from: the draft, its fields ([`all_fields`]), the
/// scale, the LFOs' values now, the first line of the matrix shown.
struct Source<'a> {
    draft: &'a Object,
    fields: &'a [String],
    s: Scale,
    lfo: &'a dyn Fn(&str) -> Option<f64>,
    matrix_first: usize,
}

/// The editor for `draft` (preset `current` of `presets`), its matrix scrolled to line `matrix_first` of those listed;
/// `lfo`: the value an LFO gives a control now, if it moves it.
pub fn build(
    draft: &Object,
    (presets, current): (&[String], &str),
    layout: &SynthLayout,
    matrix_first: usize,
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> Built {
    let s = layout.scale;
    let mut built = Built::default();
    push_header(&mut built.items, presets, current, layout);

    let groups = model::groups(draft);
    let fields = all_fields(draft);
    let source = Source { draft, fields: &fields, s, lfo, matrix_first };
    let shown: Vec<(usize, &str)> = groups.iter().enumerate().map(|(gi, group)| (gi, group.as_str())).collect();
    let strips: Vec<(&str, usize)> =
        shown.iter().filter(|(_, group)| synth_strip::is_strip(group)).map(|&(gi, group)| (group, gi)).collect();
    let scope = shown.iter().any(|(_, group)| *group == "scope");
    let mut oscillators = (!strips.is_empty()).then(|| oscillators_block(&source, &strips, scope));
    let scope_inside = oscillators.as_ref().is_some_and(|block| block.cards.iter().any(|c| c.group == "scope"));
    let mut flow = Flow::new(layout.area, s);
    for &(gi, group) in &shown {
        let block = if synth_strip::is_strip(group) {
            // The oscillators' card, in the place of the first one.
            let Some(block) = oscillators.take() else { continue };
            block
        } else if group == "scope" {
            if scope_inside {
                continue;
            }
            scope_block(&source, (group, gi))
        } else {
            card_block(&source, (group, gi))
        };
        built.place(&mut flow, block);
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

    /// The place of a `w` × `h` card.
    fn place(&mut self, w: i32, h: i32) -> (i32, i32) {
        if self.x > self.area.x && self.x + w > self.area.x + self.area.w {
            self.top += self.row_h + self.gap;
            self.x = self.area.x;
            self.row_h = 0;
        }
        let at = (self.x, self.top);
        self.row_h = self.row_h.max(h);
        self.x += w + self.gap;
        at
    }
}

/// Whether the card of `group` is bypassed.
fn is_bypassed(draft: &Object, group: &str) -> bool {
    model::has_power(group) && model::bypassed(draft, group)
}

/// The oscillators' strips one under the other in one card, each strip a
/// band of it; with `scope`, the scope's plot right of the first strip's
/// controls, when it leaves room for it.
fn oscillators_block(source: &Source, strips: &[(&str, usize)], scope: bool) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let origin = (PAD_X * s, PAD_Y * s);
    let rows = synth_strip::push_strips(&mut items, source.draft, strips, source.fields, origin, s, source.lfo);
    let right = rows.iter().map(|&(_, right)| right).max().unwrap_or(origin.0);
    let (w, h) = (right + PAD_X * s, rows.last().map_or(origin.1, |(top, _)| top + QUIET_H * s) + PAD_Y * s);
    let half_gap = synth_strip::STRIP_GAP * s / 2;
    let frame =
        Card { group: OSCILLATORS.into(), rect: Rect::new(0, 0, w, h), bypassed: false, plot: None, framed: true };
    let bands = strips.iter().zip(&rows).map(|(&(group, _), &(top, _))| Card {
        group: group.into(),
        rect: Rect::new(s.px(), top - half_gap, w - 2 * s.px(), QUIET_H * s + 2 * half_gap),
        bypassed: is_bypassed(source.draft, group),
        plot: None,
        framed: false,
    });
    let scope = rows
        .first()
        .filter(|_| scope)
        .map(|&(top, first_right)| {
            let x = first_right + PLOT_GAP * s;
            Rect::new(x, top, right - x, QUIET_H * s)
        })
        .filter(|plot| plot.w >= SCOPE_MIN_W * s)
        .map(|plot| Card {
            group: "scope".into(),
            rect: plot,
            bypassed: false,
            plot: Some((Plot::Scope, plot)),
            framed: false,
        });
    let cards = std::iter::once(frame).chain(bands).chain(scope).collect();
    Block { items, size: (w, h), cards, matrix: None }
}

/// The scope's card of its own, when the oscillators' card has no room for it.
fn scope_block(source: &Source, (group, gi): (&str, usize)) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, y) = (PAD_X * s, PAD_Y * s);
    let head_right = push_head(&mut items, source.draft, (group, gi), (x, y), s);
    let (w, h) = ((PLOT_W * s + 2 * x).max(head_right + x), (HEAD_H + SCOPE_H) * s + 2 * y);
    let plot = Rect::new(x, y + HEAD_H * s, w - 2 * x, SCOPE_H * s);
    let card = Card {
        group: group.into(),
        rect: Rect::new(0, 0, w, h),
        bypassed: is_bypassed(source.draft, group),
        plot: Some((Plot::Scope, plot)),
        framed: true,
    };
    Block { items, size: (w, h), cards: vec![card], matrix: None }
}

/// Card `group` (index `gi` of the groups): its head, then its plot left of
/// its controls in a row (the matrix: its lines).
fn card_block(source: &Source, (group, gi): (&str, usize)) -> Block {
    let s = source.s;
    let mut items = Vec::new();
    let (x, y) = (PAD_X * s, PAD_Y * s);
    let head_right = push_head(&mut items, source.draft, (group, gi), (x, y), s);
    let top = y + HEAD_H * s;
    let mut matrix = None;
    let plot = card_plot(group).map(|plot| (plot, Rect::new(x, top, PLOT_W * s, KNOB_H * s)));
    let (right, bottom) = if group == "matrix" {
        let (right, bottom, list) =
            crate::synth_matrix::push_lines(&mut items, source.draft, source.fields, (x, top, source.matrix_first), s);
        matrix = Some(list);
        (right, bottom)
    } else {
        let left = plot.map_or(x, |(_, r)| r.x + r.w + PLOT_GAP * s);
        let cells = field_cells(source.draft, group, source.fields, source.lfo);
        let bottom = if cells.is_empty() && plot.is_none() { top } else { top + KNOB_H * s };
        (push_row(&mut items, cells, (left, top), KNOB_H, s), bottom)
    };
    let (w, h) = (right.max(head_right) + PAD_X * s, bottom + PAD_Y * s);
    let card = Card {
        group: group.into(),
        rect: Rect::new(0, 0, w, h),
        bypassed: is_bypassed(source.draft, group),
        plot,
        framed: true,
    };
    Block { items, size: (w, h), cards: vec![card], matrix }
}

/// The preset's select and the buttons, above the cards.
fn push_header(items: &mut Vec<Item>, presets: &[String], current: &str, layout: &SynthLayout) {
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
}

/// The plot a card shows left of its controls, as high as a knob (the
/// scope has a card of its own: [`scope_block`]).
fn card_plot(group: &str) -> Option<Plot> {
    match group {
        "filter" => Some(Plot::Filter),
        "envelope" => Some(Plot::Envelope),
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

/// The fields whose control goes under another's, in its column: the
/// field, the one it goes under.
const STACKED: [(&str, &str); 1] = [("filter.slope", "filter.type")];

/// The controls of `group`, in its fields' order, as cells of a row; a
/// field of [`STACKED`] right after the one it goes under.
fn field_cells(draft: &Object, group: &str, fields: &[String], lfo: &dyn Fn(&str) -> Option<f64>) -> Vec<Cell> {
    let mut cells = Vec::new();
    for path in model::fields(draft, group) {
        let Some(fi) = fields.iter().position(|f| *f == path) else { continue };
        let key = path.rsplit('.').next().unwrap_or(&path);
        let mut label = model::find_meta(&path).and_then(|m| m.label).unwrap_or(key).to_owned();
        let kind = match field_kind(&path, model::get(draft, &path)) {
            Field::Knob => match synth_faders::letter(&path) {
                Some(letter) => {
                    letter.clone_into(&mut label);
                    synth_faders::fader_kind(draft, &path, lfo)
                }
                None => knob_kind(draft, &path, lfo),
            },
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
        let id = Id::SynthField(fi as u16);
        let above = STACKED.iter().find(|(field, _)| *field == path).map(|(_, above)| *above);
        let above = above.and_then(|above| fields.iter().position(|f| f == above));
        match above.and_then(|fa| cells.iter().position(|c: &Cell| c.id == Id::SynthField(fa as u16))) {
            Some(at) => cells.insert(at + 1, Cell { id, kind, label, place: Place::Under }),
            None => cells.push(Cell { id, kind, label, place: Place::Beside }),
        }
    }
    cells
}

/// Paint the cards (under their items), their plots and the module pills.
pub fn draw(canvas: &mut Canvas, draft: &Object, built: &Built, layout: &SynthLayout, time: f64, bpm: f64) {
    let s = layout.scale;
    for card in &built.cards {
        let r = card.rect;
        let fill = if card.bypassed { card_off_color() } else { card_color() };
        if card.framed {
            canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
            canvas.fill_rect(r.x + s.px(), r.y + s.px(), r.w - 2 * s, r.h - 2 * s, fill);
        } else if card.bypassed {
            canvas.fill_rect(r.x, r.y, r.w, r.h, fill);
        }
        if let Some((plot, rect)) = card.plot {
            canvas.fill_rect(rect.x, rect.y, rect.w, rect.h, surface_color());
            match plot {
                Plot::Scope => draw_scope(canvas, &model::scope(draft, time, bpm), rect, s),
                Plot::Envelope => draw_envelope(canvas, draft, rect, s),
                Plot::Filter => draw_filter(canvas, draft, rect, s, time, bpm),
            }
        }
    }
    if let Some(list) = built.matrix.filter(MatrixList::scrolls) {
        scroll_thumb::draw(canvas, list.bar, list.thumb(), s);
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
    fn builds_every_card_and_names_fields_by_index() {
        let d = draft();
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let fields = all_fields(&d);
        let names = ["a".to_owned(), "b".to_owned()];
        let mut knobs = 0;
        let built = build(&d, (&names, "b"), &layout, 0, &|_| None);
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
        // Every numeric control but the algorithm and the three octaves
        // (spins) is a knob or a fader; the matrix lists its free line only.
        assert_eq!(knobs, model::metas().count() - 4 - (ordrumbox_engine::MATRIX_SIZE - 1));
        assert!(matches!(built.items[0].kind, Kind::Select { selected: 1, .. }));
        // The oscillators' card, its strips (vco2, vco3 off: gain 0) and the
        // scope, the unison, FM (its flag unset), the noise, the filter and
        // its envelope, the envelopes and the master, then the LFOs, the mod
        // envelope and the matrix.
        let groups: Vec<_> = built.cards.iter().map(|c| (c.group.as_str(), c.bypassed)).collect();
        assert_eq!(
            groups,
            [
                (OSCILLATORS, false),
                ("vco1", false),
                ("vco2", true),
                ("vco3", true),
                ("scope", false),
                ("unison", false),
                ("fm", false),
                ("noise", false),
                ("filter", false),
                ("filterEnv", false),
                ("envelope", false),
                ("pitchEnv", false),
                ("master", false),
                ("lfo", false),
                ("lfo2", false),
                ("modEnvelope", false),
                ("matrix", false)
            ]
        );
        // The scope, with no title, in VCO1's band right of its controls; the
        // filter's and the envelope's plots left of their controls.
        let [oscillators, vco1, scope] = [&built.cards[0], &built.cards[1], &built.cards[4]].map(|c| c.rect);
        assert!(!scope_card_has_title(&built), "no title");
        assert!(scope.y >= vco1.y && scope.y + scope.h <= vco1.y + vco1.h, "in VCO1's band");
        assert!(scope.x + scope.w <= oscillators.x + oscillators.w);
        let vco1_controls = built.items.iter().filter(|i| i.rect.y >= vco1.y && i.rect.y < vco1.y + vco1.h);
        assert!(vco1_controls.into_iter().all(|i| i.rect.x + i.rect.w <= scope.x), "right of VCO1's controls");
        for group in ["filter", "envelope"] {
            let card = built.cards.iter().find(|c| c.group == group).unwrap();
            let (_, plot) = card.plot.unwrap();
            assert_eq!(card.rect.h, (HEAD_H + KNOB_H + 2 * PAD_Y) * Scale::ONE, "{group}: one row");
            let inside = |r: Rect| {
                r.x >= card.rect.x
                    && r.x < card.rect.x + card.rect.w
                    && r.y >= plot.y
                    && r.y < card.rect.y + card.rect.h
            };
            let controls = built.items.iter().filter(|i| matches!(i.id, Some(Id::SynthField(_))) && inside(i.rect));
            assert!(controls.into_iter().all(|i| i.rect.x > plot.x + plot.w), "{group}");
        }
    }

    /// Whether an item says "Scope".
    fn scope_card_has_title(built: &Built) -> bool {
        built.items.iter().any(|i| matches!(&i.kind, Kind::Text { text, .. } if text == "Scope"))
    }

    #[test]
    fn the_filters_slope_is_under_its_type_and_its_knobs_are_faders() {
        let d = draft();
        let layout = SynthLayout::new(740, 680, Scale::ONE, 0);
        let built = build(&d, (&[], ""), &layout, 0, &|_| None);
        let fields = all_fields(&d);
        let rect = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap() as u16;
            built.items.iter().find(|i| i.id == Some(Id::SynthField(fi))).unwrap()
        };
        let (kind, slope) = (rect("filter.type").rect, rect("filter.slope").rect);
        assert_eq!(kind.x, slope.x, "one column");
        assert!(slope.y >= kind.y + kind.h, "the slope under the type");
        let card = built.cards.iter().find(|c| c.group == "filter").unwrap().rect;
        assert!(slope.y + slope.h <= card.y + card.h);
        for (path, letter) in [("filter.freq", "F"), ("filter.Q", "Q"), ("filter.drive", "D")] {
            let item = rect(path);
            assert!(
                matches!(&item.kind, Kind::Value { spec, label: Some(l), .. }
                    if spec.style == crate::control::Style::Fader && l == letter),
                "{path}: {:?}",
                item.kind
            );
            assert!(item.rect.x > slope.x + slope.w, "{path}: right of the spins");
        }
    }

    #[test]
    fn the_matrix_lists_its_routes_and_a_free_line_and_scrolls_and_stays_put() {
        // As wide as the smallest window leaves the editor: every card on one screen.
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let names = ["presets".to_owned()];
        let built = |d: &Object, first: usize| build(d, (&names, "presets"), &layout, first, &|_| None);
        let lines = |d: &Object, built: &Built| {
            let fields = all_fields(d);
            let paths: Vec<_> = built
                .items
                .iter()
                .filter_map(|i| match i.id {
                    Some(Id::SynthField(fi)) => {
                        fields[usize::from(fi)].strip_prefix("matrix.source").map(str::to_owned)
                    }
                    _ => None,
                })
                .collect();
            paths
        };
        let card = |built: &Built| built.cards.iter().find(|c| c.group == "matrix").expect("the matrix card").rect;
        let plain = draft();
        let few = built(&plain, 0);
        assert_eq!(lines(&plain, &few), ["1"], "the free line only");
        let list = few.matrix.unwrap();
        assert!(!list.scrolls());
        assert!(card(&few).y + card(&few).h <= layout.area.y + layout.area.h);
        let mut set = draft();
        for n in [2, 3, 4, 5, 7, 8, 9] {
            model::set(&mut set, &format!("matrix.source{n}"), Value::from("lfo1"));
        }
        let many = built(&set, 0);
        assert_eq!(lines(&set, &many), ["2", "3", "4", "5", "7", "8"]);
        assert_eq!(card(&many), card(&few), "the card stays put");
        assert_eq!(many.matrix.unwrap().bar, list.bar, "and so does its bar");
        assert!(many.matrix.unwrap().scrolls());
        // Scrolled to the end, held there: the last routes and the free line.
        let end = built(&set, 99);
        assert_eq!(lines(&set, &end), ["4", "5", "7", "8", "9", "1"]);
        assert_eq!(end.matrix.unwrap().first, 2);
        let fields = all_fields(&set);
        let is_matrix =
            |i: &&Item| matches!(i.id, Some(Id::SynthField(fi)) if fields[usize::from(fi)].starts_with("matrix."));
        for item in end.items.iter().filter(is_matrix) {
            let (r, c) = (item.rect, card(&end));
            assert!(r.x >= c.x && r.x + r.w <= c.x + c.w && r.y + r.h <= c.y + c.h, "{item:?}");
        }
    }

    #[test]
    fn draws_at_any_size() {
        let d = draft();
        for (w, h) in [(1, 1), (40, 30), (740, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = SynthLayout::new(w as i32, h as i32, Scale::ONE, 0);
            let built = build(&d, (&[], ""), &layout, 0, &|_| None);
            draw(&mut canvas, &d, &built, &layout, 0.3, 120.0);
        }
    }
}
