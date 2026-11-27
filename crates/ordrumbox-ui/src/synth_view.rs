//! The synth editor, in the place of
//! the grid: the preset bar (`synth_bar.rs`), the line of the track it is
//! for, and every card on one screen, always in the same order — what makes the sound (the sources, the
//! filter, the envelopes), then what moves it (the LFOs, the modulation
//! envelope, the matrix), then the master — in rows filled to the full width, each card as tall as its
//! row (`synth_rows.rs`), FM in a column at the right edge beside the rows of the
//! noise and the filters and of the envelopes — its power LED
//! and title, then its plot (the filter curve left of its controls, the main
//! envelope's curve right of its own) and its knobs, faders, spins and
//! switches in a row (`synth_cells.rs`; the filter's slope under its type).
//! The oscillators, the LFOs and the
//! envelopes are strips in bands of a card of their family
//! (`synth_family.rs`): the scope (four cycles of the oscillators), with no
//! title, in the room VCO1's strip leaves right of its controls; the three
//! envelopes side by side, each title over its faders, the main envelope's
//! curve right of its own. Filter 2 shares Filter's card, beside it
//! (`synth_filters.rs`); the voice, the unison and the noise are narrow cards
//! of columns (`synth_column.rs`).
//!
//! [`SynthLayout`] and [`build`] place everything as panel items (the ids
//! name groups and fields by their index in [`model::groups`] and
//! [`all_fields`]), each in the hardware skin (`panel/hardware.rs`); `synth_paint.rs`
//! paints the metal, the cards and the plots under them.

use crate::scale::Scale;
use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model::{self as model, FILTER_SLOPES, LFO_SYNCS, VOICE_MODE_LABELS};
use ordrumbox_engine::synth_preset::{FILTER_TYPES, NOISE_MODES, VOICE_MODES};

use crate::control::Spec;
use crate::editors::FILTER_ICONS;
use crate::frame;
use crate::panel::{Id, Item, Kind, Rect, Skin};
use crate::synth_bar::{self, Bar};
use crate::synth_cells::{Cell, KNOB_H, Place, is_fader, push_head, push_row};
use crate::synth_column::{COLUMN_GROUPS, column_block};
use crate::synth_faders;
use crate::synth_family::{Family, family_block};
use crate::synth_filters::{MERGED_GROUP, filter_block};
use crate::synth_matrix::MatrixList;
use crate::synth_rows::{self, Column, Size};
use crate::synth_spread;
use crate::synth_strip;

/// The space inside a card around what it holds, across and down (at scale 1).
pub(crate) const PAD_X: i32 = 6;
pub(crate) const PAD_Y: i32 = 4;
/// The space between two cards (at scale 1).
const CARD_GAP: i32 = 8;
/// The card in a column at the right edge, beside the rows of the cards
/// after it (the noise and the filters, the envelopes), and how many.
const COLUMN_GROUP: &str = "fm";
/// The card of the orDrumbox emblem, the last, which is no group of the preset.
pub(crate) const EMBLEM_GROUP: &str = "emblem";
const COLUMN_ROW_COUNT: usize = 2;
/// The height of a card's head, the space under it included (at scale 1).
pub(crate) const HEAD_H: i32 = 18;
/// A plot's width left of its card's controls, and the space right of it (at scale 1).
pub(crate) const PLOT_W: i32 = 120;
pub(crate) const PLOT_GAP: i32 = 8;
/// The scope in the oscillators' card: its narrowest (at scale 1), else
/// it has a card of its own, its plot this high.
pub(crate) const SCOPE_MIN_W: i32 = 60;
const SCOPE_H: i32 = 60;

/// Every field of the draft's cards, card after card: the index a
/// [`Id::SynthField`] names.
pub fn all_fields(draft: &Object) -> Vec<String> {
    model::groups(draft).iter().flat_map(|g| model::fields(draft, g)).collect()
}

/// What a field shows as: a knob, a list (values and labels, on a spin: the
/// unison's layers among them), a wave (on a spin that draws
/// it), a switch, a LED (oscillator 2's sync), or nothing (an object).
enum Field {
    Knob,
    List(Vec<(String, String)>),
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
    if model::SWITCHES.contains(&path) {
        return Field::Switch;
    }
    if path == "voice.mode" {
        let modes = VOICE_MODES.iter().zip(VOICE_MODE_LABELS);
        return Field::List(modes.map(|(v, l)| ((*v).to_owned(), l.to_owned())).collect());
    }
    if matches!(path, "filter.type" | "noise.filterType" | "filter2.type") {
        return Field::List(
            FILTER_TYPES.iter().zip(FILTER_ICONS).map(|(v, l)| ((*v).to_owned(), l.to_owned())).collect(),
        );
    }
    if path == "fm.algo" {
        return Field::List(
            FM_ALGORITHMS.iter().enumerate().map(|(i, (l, _))| (i.to_string(), (*l).to_owned())).collect(),
        );
    }
    if path == "unison.count" {
        // Whole layers, 1 to the most a preset holds: saturating is meant.
        let most = model::find_meta(path).map_or(1.0, |meta| meta.max).round() as i32;
        return Field::List((1..=most).map(|n| (n.to_string(), n.to_string())).collect());
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
        return Field::List(LFO_SYNCS.iter().map(|(v, l)| ((*v).to_owned(), (*l).to_owned())).collect());
    }
    match value {
        Some(Value::Number(_)) => Field::Knob,
        Some(Value::Bool(_)) => Field::Switch,
        _ => Field::None,
    }
}

/// The FM algorithms (a preset holds 0 to 4), offered on a spin: which
/// oscillators modulate which, short on the spin, in full in its tooltip.
const FM_ALGORITHMS: [(&str, &str); 5] =
    [("2→1", "2 → 1"), ("3→1", "3 → 1"), ("3→2", "3 → 2 → 1"), ("2+3", "2 + 3 → 1"), ("2↔1", "2 ↔ 1")];

/// The tooltip of the FM algorithm's spin (`path` `fm.algo`): its routing in full.
#[must_use]
pub fn fm_tip(draft: &Object, path: &str) -> Option<[String; 2]> {
    if path != "fm.algo" {
        return None;
    }
    // A whole algorithm, 0 to 4: the cast saturates.
    let algorithm = model::number(draft, path).unwrap_or(0.0).round().max(0.0) as usize;
    let (_, routing) = FM_ALGORITHMS.get(algorithm)?;
    Some(["FM Algorithm".to_owned(), (*routing).to_owned()])
}

/// A plot of a card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plot {
    Scope,
    Filter,
    /// The curve of the main envelope (the note's volume).
    Envelope,
}

/// The space above the preset bar, under the menu bar and the toolbar (at scale 1).
const HEAD_TOP: i32 = 8;
/// The top of the cards' area, under the preset bar and the track's line (at scale 1).
const AREA_TOP: i32 = HEAD_TOP + 42;
/// The height the cards take, every one shown, in the narrowest editor: five
/// rows (at scale 1, rounding included). The smallest window leaves them this
/// (`every_card_fits_the_smallest_window`).
const CARDS_H: i32 = 552;
/// The height the editor needs under the header, at scale 1: the smallest window gives it.
pub const MIN_HEIGHT: i32 = AREA_TOP + CARDS_H;

/// Where the editor's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthLayout {
    scale: Scale,
    /// The whole editor, and its head: the preset bar, the track's line, the logo.
    panel: Rect,
    head: Rect,
    pub bar: Bar,
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
        let head_top = top + HEAD_TOP * s;
        let track_line = Rect::new(margin, head_top + 20 * s, (width - 2 * margin).max(0), 14 * s);
        let area_top = top + AREA_TOP * s;
        let footer = frame::footer_top(height, scale, status_lines);
        let bottom = footer - margin / 2;
        Self {
            scale,
            panel: Rect::new(0, top, width.max(0), (footer - top).max(0)),
            head: Rect::new(margin, head_top, (width - 2 * margin).max(0), (AREA_TOP - HEAD_TOP - 6) * s),
            bar: Bar::new(margin, head_top, s),
            track_line,
            area: Rect::new(margin, area_top, (width - 2 * margin).max(0), (bottom - area_top).max(0)),
        }
    }

    #[must_use]
    pub fn scale(&self) -> Scale {
        self.scale
    }

    /// The whole editor, painted as one panel.
    #[must_use]
    pub fn panel(&self) -> Rect {
        self.panel
    }

    /// The editor's head, above the cards.
    #[must_use]
    pub fn head(&self) -> Rect {
        self.head
    }
}

/// A card as built: its place, whether it is bypassed, its plot; `framed`:
/// as tall as its row, with a border except for the emblem, which is painted
/// on the metal (an oscillator's strip is a band of the oscillators' card,
/// shaded only when bypassed).
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

/// What the editor shows, as built: the panel items, the cards, the
/// matrix's list.
#[derive(Debug, Default)]
pub struct Built {
    pub items: Vec<Item>,
    pub cards: Vec<Card>,
    pub matrix: Option<MatrixList>,
}

/// A card (or a family's card) built from (0, 0), before the rows place it.
pub(crate) struct Block {
    pub(crate) items: Vec<Item>,
    pub(crate) size: (i32, i32),
    pub(crate) cards: Vec<Card>,
    pub(crate) matrix: Option<MatrixList>,
    /// The top-left corner of its controls, spread when it is widened; none:
    /// what reaches its right edge takes the width (the scope, the matrix).
    pub(crate) controls: Option<(i32, i32)>,
    /// Whether its faders, its plot and its bands grow down with it when
    /// its row is taller than it (the filters' card).
    pub(crate) grows_down: bool,
}

impl Block {
    /// The block made `w` × `h` (no smaller than it is): its frame, and what
    /// reaches its right or bottom edge (a card's plot), grown with it, its
    /// controls spread across the width. A family's bands and their plots
    /// (unframed) grow across only: a strip keeps its height.
    fn stretch(&mut self, w: i32, h: i32, s: Scale) {
        let (old_w, old_h) = self.size;
        let (dw, dh) = ((w - old_w).max(0), (h - old_h).max(0));
        let grow = |r: Rect, framed: bool| {
            let wide = r.x + r.w >= old_w - PAD_X * s;
            let tall = framed && r.y + r.h >= old_h - PAD_Y * s;
            Rect::new(r.x, r.y, r.w + if wide { dw } else { 0 }, r.h + if tall { dh } else { 0 })
        };
        for card in &mut self.cards {
            card.rect = grow(card.rect, card.framed);
            card.plot = card.plot.map(|(plot, r)| (plot, grow(r, card.framed)));
        }
        if let Some(origin) = self.controls {
            synth_spread::spread(&mut self.items, origin, dw);
        }
        if self.grows_down {
            let taller = |r: Rect| Rect::new(r.x, r.y, r.w, r.h + dh);
            for item in self.items.iter_mut().filter(|item| is_fader(&item.kind)) {
                item.rect = taller(item.rect);
            }
            for card in self.cards.iter_mut().filter(|card| !card.framed) {
                card.rect = taller(card.rect);
                card.plot = card.plot.map(|(plot, r)| (plot, taller(r)));
            }
        }
        self.size = (old_w + dw, old_h + dh);
    }
}

impl Built {
    /// `block` at `rect`, stretched to it.
    fn place(&mut self, mut block: Block, rect: Rect, s: Scale) {
        block.stretch(rect.w, rect.h, s);
        let (x, y) = (rect.x, rect.y);
        self.items.extend(block.items.into_iter().map(|item| Item { rect: item.rect.moved(x, y), ..item }));
        self.cards.extend(block.cards.into_iter().map(|card| card.moved(x, y)));
        if let Some(list) = block.matrix {
            self.matrix = Some(list.moved(x, y));
        }
    }
}

/// What a card is built from: the draft, its fields ([`all_fields`]), the
/// scale, the LFOs' values now, the first line of the matrix shown.
pub(crate) struct Source<'a> {
    pub(crate) draft: &'a Object,
    pub(crate) fields: &'a [String],
    pub(crate) s: Scale,
    pub(crate) lfo: &'a dyn Fn(&str) -> Option<f64>,
    matrix_first: usize,
}

/// The editor for `draft` (preset `current` of `presets`), its matrix
/// scrolled to line `matrix_first` of those listed;
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
    synth_bar::push_bar(&mut built.items, (presets, current), &layout.bar);
    let bar_count = built.items.len();

    let groups = model::groups(draft);
    let fields = all_fields(draft);
    let source = Source { draft, fields: &fields, s, lfo, matrix_first };
    let shown: Vec<(usize, &str)> = groups.iter().enumerate().map(|(gi, group)| (gi, group.as_str())).collect();
    let scope = shown.iter().any(|(_, group)| *group == "scope");
    // Each family's card, in the place of its first strip.
    let mut families: Vec<(&str, Block)> = Family::ALL
        .into_iter()
        .filter_map(|family| {
            let strips: Vec<(&str, usize)> = family
                .groups()
                .iter()
                .filter_map(|g| shown.iter().find(|(_, group)| group == g).map(|&(gi, group)| (group, gi)))
                .collect();
            let first = strips.first()?.0;
            Some((first, family_block(&source, family, &strips, scope)))
        })
        .collect();
    let scope_inside = families.iter().any(|(_, block)| block.cards.iter().any(|c| c.group == "scope"));
    let mut blocks = Vec::with_capacity(shown.len());
    for &(gi, group) in &shown {
        let block = if Family::of(group).is_some() {
            let Some(at) = families.iter().position(|(first, _)| *first == group) else { continue };
            families.swap_remove(at).1
        } else if group == "scope" {
            if scope_inside {
                continue;
            }
            scope_block(&source, (group, gi))
        } else if group == MERGED_GROUP {
            continue;
        } else if group == "filter" {
            match shown.iter().find(|(_, g)| *g == MERGED_GROUP) {
                Some(&(f2i, _)) => filter_block(&source, (gi, f2i)),
                None => card_block(&source, (group, gi)),
            }
        } else if let Some(&(_, columns)) = COLUMN_GROUPS.iter().find(|(g, _)| *g == group) {
            column_block(&source, (group, gi), columns)
        } else {
            card_block(&source, (group, gi))
        };
        blocks.push(block);
    }
    blocks.push(emblem_block(s));
    let sizes = sizes_of(&blocks, CARD_GAP * s);
    let column = blocks
        .iter()
        .position(|block| block.cards.first().is_some_and(|card| card.group == COLUMN_GROUP))
        .map(|index| Column { index, row_count: COLUMN_ROW_COUNT });
    for (block, rect) in blocks.into_iter().zip(synth_rows::place(&sizes, layout.area, CARD_GAP * s, column)) {
        built.place(block, rect, s);
    }
    for (i, item) in built.items.iter_mut().enumerate() {
        item.skin = if i < bar_count { Skin::Metal } else { Skin::Card };
    }
    built
}

/// The sizes the rows place `blocks` at: the cards at the right edge (the
/// voice and the unison side by side, FM's column, the emblem) one width,
/// the widest's, which they keep.
fn sizes_of(blocks: &[Block], gap: i32) -> Vec<Size> {
    let mut sizes: Vec<Size> = blocks.iter().map(|block| Size::from(block.size)).collect();
    let at = |group: &str| blocks.iter().position(|block| block.cards.first().is_some_and(|card| card.group == group));
    let pair = at("voice").zip(at("unison"));
    let pair_w = pair.map_or(0, |(v, u)| sizes[v].w + gap + sizes[u].w);
    let singles: Vec<usize> = [at(COLUMN_GROUP), at(EMBLEM_GROUP)].into_iter().flatten().collect();
    let right_w = singles.iter().map(|&i| sizes[i].w).fold(pair_w, i32::max);
    if let Some((v, u)) = pair {
        let extra = right_w - pair_w;
        sizes[v] = Size { w: sizes[v].w + extra / 2, h: sizes[v].h, is_fixed: true };
        sizes[u] = Size { w: sizes[u].w + extra - extra / 2, h: sizes[u].h, is_fixed: true };
    }
    for i in singles {
        sizes[i] = Size { w: right_w, h: sizes[i].h, is_fixed: true };
    }
    sizes
}

/// The emblem's place, last, at the right edge: no controls, no painted card,
/// as wide as FM ([`sizes_of`]), as tall as its row, a knob's height at least.
fn emblem_block(s: Scale) -> Block {
    let size = (0, (HEAD_H + KNOB_H) * s);
    let card = Card {
        group: EMBLEM_GROUP.into(),
        rect: Rect::new(0, 0, 0, size.1),
        bypassed: false,
        plot: None,
        framed: true,
    };
    Block { items: Vec::new(), size, cards: vec![card], matrix: None, controls: None, grows_down: false }
}

/// Whether the card of `group` is bypassed.
pub(crate) fn is_bypassed(draft: &Object, group: &str) -> bool {
    model::has_power(group) && model::bypassed(draft, group)
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
    Block { items, size: (w, h), cards: vec![card], matrix: None, controls: None, grows_down: false }
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
    let mut controls = None;
    let (right, bottom) = if group == "matrix" {
        let (right, bottom, list) =
            crate::synth_matrix::push_lines(&mut items, source.draft, source.fields, (x, top, source.matrix_first), s);
        matrix = Some(list);
        (right, bottom)
    } else {
        let left = plot.map_or(x, |(_, r)| r.x + r.w + PLOT_GAP * s);
        controls = Some((left, top));
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
    Block { items, size: (w, h), cards: vec![card], matrix, controls, grows_down: false }
}

/// The plot a card shows left of its controls, as high as a knob (the
/// scope has a card of its own: [`scope_block`]).
fn card_plot(group: &str) -> Option<Plot> {
    match group {
        "filter" => Some(Plot::Filter),
        _ => None,
    }
}

/// The knob of numeric field `path`, at the value an LFO gives it now if
/// one moves it; greyed out where it does nothing ([`model::is_inert`]). It
/// says its value in its tooltip ([`crate::synth_cells::field_tip`]), not
/// under it.
pub(crate) fn knob_kind(draft: &Object, path: &str, lfo: &dyn Fn(&str) -> Option<f64>) -> Kind {
    let meta = model::find_meta(path);
    let label = model::field_label(path).to_owned();
    if model::is_inert(draft, path) {
        return Kind::Disabled { label };
    }
    let base = model::number(draft, path).or_else(|| model::extra_value(draft, path)).unwrap_or(0.0);
    let spec = meta.map_or_else(|| Spec::knob(0.0, 1.0, 0.01, 0.0), knob_spec);
    let value = lfo(path).map_or(base, |m| m.clamp(spec.min, spec.max));
    let lfo_on = model::lfo_targeted(draft, path);
    Kind::Value { spec, value, label: Some(label), text: String::new(), lfo: lfo_on }
}

/// The fields whose control goes under another's, in its column: the
/// field, the one it goes under.
const STACKED: [(&str, &str); 1] = [("filter.slope", "filter.type")];

/// The controls of `group`, in its fields' order, as cells of a row; a
/// field of [`STACKED`] right after the one it goes under.
pub(crate) fn field_cells(
    draft: &Object,
    group: &str,
    fields: &[String],
    lfo: &dyn Fn(&str) -> Option<f64>,
) -> Vec<Cell> {
    let mut cells = Vec::new();
    for path in model::fields(draft, group) {
        let Some(fi) = fields.iter().position(|f| *f == path) else { continue };
        let mut label = model::field_label(&path).to_owned();
        let kind = match field_kind(&path, model::get(draft, &path)) {
            Field::Knob => match synth_faders::letter(&path) {
                Some(letter) => {
                    letter.clone_into(&mut label);
                    synth_faders::fader_kind(draft, &path, lfo)
                }
                None => knob_kind(draft, &path, lfo),
            },
            Field::Wave => synth_strip::wave_spin(draft, &path),
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
        // Every numeric control but the algorithm, the unison's layers and the
        // three octaves (spins) is a knob or a fader, greyed out where it does nothing (the
        // widths of sine waves, the clock of white noise); the matrix lists
        // its free line only.
        let inert = fields.iter().filter(|path| model::is_inert(&d, path)).count();
        assert_eq!(inert, 4);
        // The bar has no knob.
        assert_eq!(knobs, model::metas().count() - 5 - inert - (ordrumbox_engine::MATRIX_SIZE - 1));
        assert!(matches!(built.items[0].kind, Kind::Select { selected: 1, .. }));
        // The cards in their one order: the oscillators' card, its strips
        // (vco2, vco3 off: gain 0) and the scope, the voice, the unison, FM
        // (its flag unset), the noise, the filter with the second filter beside
        // it (off until the preset has one), the envelopes' card (the three side
        // by side, the main envelope's curve after its own), the pitch envelope,
        // the LFOs' card (off: at 0 Hz), the matrix, the master and the emblem.
        let groups: Vec<_> = built.cards.iter().map(|c| (c.group.as_str(), c.bypassed)).collect();
        assert_eq!(
            groups,
            [
                (Family::Oscillators.name(), false),
                ("vco1", false),
                ("vco2", true),
                ("vco3", true),
                ("scope", false),
                ("voice", false),
                ("unison", false),
                ("fm", false),
                ("noise", false),
                ("filter", false),
                ("filter2", true),
                ("filter", false),
                (Family::Envelopes.name(), false),
                ("envelope", false),
                ("filterEnv", false),
                ("modEnvelope", false),
                (Family::Envelopes.name(), false),
                ("pitchEnv", false),
                (Family::Lfos.name(), false),
                ("lfo", true),
                ("lfo2", true),
                ("matrix", false),
                ("master", false),
                (EMBLEM_GROUP, false)
            ]
        );
        // The scope, with no title, in VCO1's band right of its controls; the
        // filter's plot left of its controls.
        let [oscillators, vco1, scope] = [&built.cards[0], &built.cards[1], &built.cards[4]].map(|c| c.rect);
        assert!(!scope_card_has_title(&built), "no title");
        assert!(scope.y >= vco1.y && scope.y + scope.h <= vco1.y + vco1.h, "in VCO1's band");
        assert!(scope.x + scope.w <= oscillators.x + oscillators.w);
        let in_band = |r: Rect| r.x < oscillators.x + oscillators.w && r.y >= vco1.y && r.y < vco1.y + vco1.h;
        let vco1_controls = built.items.iter().filter(|i| in_band(i.rect));
        assert!(vco1_controls.into_iter().all(|i| i.rect.x + i.rect.w <= scope.x), "right of VCO1's controls");
        let group = "filter";
        let frame = built.cards.iter().find(|c| c.group == group && c.framed).unwrap();
        let card = built.cards.iter().find(|c| c.group == group && c.plot.is_some()).unwrap();
        let (_, plot) = card.plot.unwrap();
        assert_eq!(frame.rect.h, (HEAD_H + KNOB_H + 2 * PAD_Y) * Scale::ONE, "{group}: one row");
        let inside = |r: Rect| {
            r.x >= frame.rect.x
                && r.x < frame.rect.x + frame.rect.w
                && r.y >= plot.y
                && r.y < frame.rect.y + frame.rect.h
        };
        let controls = built.items.iter().filter(|i| matches!(i.id, Some(Id::SynthField(_))) && inside(i.rect));
        assert!(controls.into_iter().all(|i| i.rect.x > plot.x + plot.w), "{group}");
    }

    #[test]
    fn fm_stands_in_a_column_beside_the_filters_and_the_envelopes() {
        let built = build(&draft(), (&[], ""), &SynthLayout::new(912, 680, Scale::ONE, 0), 0, &|_| None);
        let card = |group: &str| {
            built.cards.iter().find(|c| c.framed && c.group == group).unwrap_or_else(|| panic!("{group}")).rect
        };
        let (fm, filter, envelopes, pitch) = (card("fm"), card("filter"), card("envelopes"), card("pitchEnv"));
        assert!(card("unison").y < fm.y, "out of the first row");
        assert_eq!((fm.y, fm.y + fm.h), (filter.y, envelopes.y + envelopes.h), "as tall as both rows");
        assert_eq!(fm.x + fm.w, card("unison").x + card("unison").w, "at the right edge");
        for r in [filter, pitch] {
            assert_eq!(r.x + r.w + CARD_GAP, fm.x, "the rows end before it");
        }
        assert!(card("lfos").y > fm.y + fm.h, "the next row under it");
    }

    #[test]
    fn the_right_edge_is_one_width_and_the_envelopes_faders_are_the_filters_height() {
        let d = draft();
        let built = build(&d, (&[], ""), &SynthLayout::new(912, 680, Scale::ONE, 0), 0, &|_| None);
        let card = |group: &str| {
            built.cards.iter().find(|c| c.framed && c.group == group).unwrap_or_else(|| panic!("{group}")).rect
        };
        let (voice, fm, emblem, master) = (card("voice"), card("fm"), card(EMBLEM_GROUP), card("master"));
        for r in [fm, emblem] {
            assert_eq!((r.x, r.x + r.w), (voice.x, card("unison").x + card("unison").w), "{r:?}");
        }
        assert_eq!((emblem.y, emblem.h), (master.y, master.h), "the emblem ends the master's row");
        assert_eq!(master.x + master.w + CARD_GAP, emblem.x, "the master left of it");
        let fields = all_fields(&d);
        let height = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}")).rect.h
        };
        for path in ["envelope.attack", "filterEnv.release", "filter.freq", "filter2.Q"] {
            assert_eq!(height(path), crate::synth_cells::ENVELOPE_H, "{path}");
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
        // As wide as the smallest window leaves the editor, and as high as it
        // leaves the cards (`CARDS_H`): every card on one screen.
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let bottom = layout.area.y + CARDS_H * Scale::ONE;
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
        assert!(card(&few).y + card(&few).h <= bottom);
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
    fn the_knobs_keep_their_value_to_their_tooltip_and_the_noise_mode_is_beside_its_filter_type() {
        let d = draft();
        let layout = SynthLayout::new(912, 680, Scale::ONE, 0);
        let built = build(&d, (&["p".to_owned()], "p"), &layout, 0, &|_| None);
        let fields = all_fields(&d);
        let item = |path: &str| {
            let fi = fields.iter().position(|f| f == path).unwrap_or_else(|| panic!("{path}"));
            crate::panel::find(&built.items, Id::SynthField(fi as u16)).unwrap_or_else(|| panic!("{path}"))
        };
        let text = |path: &str| match &item(path).kind {
            Kind::Value { text, .. } => text.clone(),
            other => panic!("{path}: {other:?}"),
        };
        for path in ["fm.amount", "unison.detune", "noise.mix", "lfo.freq", "vco1.detune", "masterVolume"] {
            assert_eq!(text(path), "", "{path}: continuous");
        }
        assert!(
            matches!(&item("unison.count").kind, Kind::Spin { labels, selected: 0, .. } if labels.first().is_some_and(|l| l == "1")),
            "the unison's layers on a spin"
        );
        assert_eq!(model::choice_value("unison.count", "3"), Value::from(3.0), "a number, as a preset holds it");
        let (kind, mode) = (item("noise.filterType").rect, item("noise.mode").rect);
        assert_eq!(mode.y, kind.y, "in its line");
        assert!(mode.x >= kind.x + kind.w, "beside it");
    }
}
