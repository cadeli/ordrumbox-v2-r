//! The kit editor: its view, its layout and its drawing.

use super::{ACCENT, ENVELOPE, focus_row_color};
use crate::canvas::{Canvas, Direction, chars_in, text_width};
use crate::frame::{body_top, footer_top, margin};
use crate::panel::hardware::{self, LCD_DIM, LCD_TEXT};
use crate::panel::{Rect, row_at};
use crate::sample_overlay::Overlay;
use crate::scale::Scale;
use crate::scroll_thumb;
use crate::theme;

/// What the kit editor shows.
#[derive(Debug)]
pub struct KitView<'a> {
    pub kit: &'a str,
    /// The folder the kit's samples are read from, `-` when not known.
    pub folder: String,
    /// File name of each sample (its instrument is a select of the panel).
    pub samples: Vec<&'a str>,
    /// The pattern's tracks that play each sample, by name (empty: none).
    pub users: Vec<Vec<String>>,
    pub selected: usize,
    /// The first sample of the table shown.
    pub first: usize,
    /// Top of the sample's details, under its controls (selects and knobs).
    pub details_top: i32,
    /// The selected sample's measurements: label and value, always the same
    /// labels ([`KIT_FACTS`]), the value `-` when unknown.
    pub facts: Vec<(&'static str, String)>,
    /// Peak of each slice, 0–1.
    pub envelope: &'a [f64],
    /// How the sample plays, drawn over its waveform.
    pub overlay: Option<Overlay>,
}

/// How many measurements the kit editor shows of a sample (note, length,
/// peak, RMS, detected instrument, file): a fixed count, so that nothing
/// under them moves.
pub const KIT_FACTS: usize = 6;
/// Lines of the kit's description.
pub const KIT_DESC_LINES: usize = 2;

/// Where the kit editor's parts are, on three cards over the metal of the
/// synth editor's hardware look: the kit (its name and counts in the title
/// band; folder, title, description), the table of its samples (the column
/// titles in the band; number, name, instrument, the tracks that play it),
/// then the selected sample: its controls on the left, its measurements on a
/// screen on the right, its waveform on a screen under both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitLayout {
    pub(super) scale: Scale,
    pub(super) top: i32,
    pub(super) bottom: i32,
    pub(super) left: i32,
    pub(super) width: i32,
    /// The kit's name, folder, title and description, after their labels.
    pub(super) head_x: i32,
    /// The kit's card, its folder line, title line and first description line.
    pub(super) kit_y: i32,
    pub(super) folder_y: i32,
    pub(super) title_y: i32,
    pub(super) desc_y: i32,
    /// The table: its card, its column titles, first row, row height, row count.
    pub(super) table_y: i32,
    pub(super) columns_y: i32,
    pub(super) list_top: i32,
    pub(super) list_width: i32,
    pub(super) row: i32,
    pub(super) list_rows: i32,
    /// Columns: sample name, instrument select (its auto-assign button
    /// after it), the tracks playing it.
    pub(super) name_x: i32,
    pub(super) instrument_x: i32,
    pub(super) users_x: i32,
    /// The selected sample's card, and its controls' top.
    pub(super) sample_y: i32,
    pub(super) detail_y: i32,
    /// The measurements' column.
    pub(super) facts_x: i32,
}

/// The kit editor's table row height, per scale unit (a select fits in it).
pub(super) const KIT_ROW: i32 = 14;
/// The height of the selected sample's controls (the kit select, the two rows
/// of knobs, the two switches), per scale unit: a test checks it.
pub(super) const KIT_CONTROLS_HEIGHT: i32 = 182;
/// A text line of the kit editor's header and measurements, per scale unit.
pub(super) const KIT_LINE: i32 = 11;
/// Width of the instrument column, per scale unit.
pub(super) const KIT_INSTRUMENT_W: i32 = 112;
/// Width of a row's auto-assign button, per scale unit ("AUTO").
pub(super) const KIT_AUTO_W: i32 = 40;
/// Width of the table's scroll bar, per scale unit.
pub(super) const KIT_BAR: i32 = 8;
/// Shortest thumb of the table's scroll bar, per scale unit.
pub(super) const KIT_MIN_THUMB: i32 = 12;
/// The widest a sample name gets, in characters.
pub(super) const KIT_NAME_CHARS: usize = 24;
/// Labels of the measurements, in characters (their values after).
pub(super) const KIT_FACT_LABEL: usize = 9;
/// Labels of the kit's header ("Notes"), in characters.
pub(super) const KIT_HEAD_LABEL: usize = 7;
/// A card's title band, its padding around what it holds, and the gap
/// between two cards, per scale unit.
const KIT_CARD_HEAD: i32 = 14;
const KIT_CARD_PAD: i32 = 6;
const KIT_CARD_GAP: i32 = 6;
/// The text of a title band, under its top, per scale unit.
const KIT_HEAD_TEXT: i32 = 3;

impl KitLayout {
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize) -> Self {
        let s = scale;
        let margin = margin(scale);
        let top = body_top(scale);
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let (head, pad, gap) = (KIT_CARD_HEAD * s, KIT_CARD_PAD * s, KIT_CARD_GAP * s);
        let left = margin + pad;
        let list_width = (width - 2 * left).max(40 * s);
        let line = KIT_LINE * s;
        let kit_y = top + 6 * s;
        let folder_y = kit_y + head + 4 * s;
        let title_y = folder_y + line;
        let desc_y = title_y + line;
        let table_y = desc_y + KIT_DESC_LINES as i32 * line + gap;
        let columns_y = table_y + KIT_HEAD_TEXT * s;
        let row = KIT_ROW * s;
        let list_top = table_y + head + 4 * s;
        // The details take what they need (their card's band, the controls,
        // the measurements, a short waveform); the rows the rest, a window
        // size giving one count.
        let details = 4 * s + gap + head + 6 * s + (KIT_FACTS as i32 * line).max(KIT_CONTROLS_HEIGHT * s) + 40 * s;
        let list_rows = ((bottom - list_top - details) / row).clamp(3, 16);
        let sample_y = list_top + list_rows * row + 4 * s + gap;
        let detail_y = sample_y + head + 6 * s;
        // Columns: the number ("12"), the name, the instrument, the tracks.
        let name_x = left + text_width("000", s);
        let room = list_width - (name_x - left) - (KIT_INSTRUMENT_W + KIT_AUTO_W) * s - 20 * s;
        let name_w = text_width(&"m".repeat(KIT_NAME_CHARS), s).min(room / 2).max(0);
        let instrument_x = name_x + name_w + 8 * s;
        let users_x = instrument_x + (KIT_INSTRUMENT_W + KIT_AUTO_W) * s + 12 * s;
        let controls_w = (list_width / 2).min(360 * s);
        Self {
            scale,
            top,
            bottom,
            left,
            width,
            head_x: left + text_width(&"m".repeat(KIT_HEAD_LABEL), s),
            kit_y,
            folder_y,
            title_y,
            desc_y,
            table_y,
            columns_y,
            list_top,
            list_width,
            row,
            list_rows,
            name_x,
            instrument_x,
            users_x,
            sample_y,
            detail_y,
            facts_x: left + controls_w + 16 * s,
        }
    }

    /// The whole editor, painted as one panel of metal.
    pub(super) fn panel(&self) -> Rect {
        let footer = self.bottom + margin(self.scale) / 2;
        Rect::new(0, self.top, self.width.max(0), (footer - self.top).max(0))
    }

    /// A card from `y` to `bottom`, across the editor.
    fn card(&self, y: i32, bottom: i32) -> Rect {
        let pad = KIT_CARD_PAD * self.scale;
        Rect::new(self.left - pad, y, self.list_width + 2 * pad, bottom - y)
    }

    /// The kit's card: its name in the band, its folder, title and description.
    pub(super) fn kit_card(&self) -> Rect {
        self.card(self.kit_y, self.table_y - KIT_CARD_GAP * self.scale)
    }

    /// The table's card: the column titles in the band, the rows.
    pub(super) fn table_card(&self) -> Rect {
        self.card(self.table_y, self.list_top + self.list_rows * self.row + 4 * self.scale)
    }

    /// The selected sample's card: its controls (which end at
    /// `details_top`), its measurements and its waveform.
    pub(super) fn sample_card(&self, details_top: i32) -> Rect {
        let wave = self.envelope_rect(details_top);
        let content = if wave.h > 0 { wave.y + wave.h } else { details_top.max(self.facts_bottom()) };
        self.card(self.sample_y, (content + KIT_CARD_PAD * self.scale).min(self.bottom))
    }

    /// The text of the title band of a card at `card_y`.
    pub(super) fn head_text_y(&self, card_y: i32) -> i32 {
        card_y + KIT_HEAD_TEXT * self.scale
    }

    /// The title band's height, per [`Scale`].
    pub(super) fn head_h(&self) -> i32 {
        KIT_CARD_HEAD * self.scale
    }

    /// The measurements' screen.
    pub(super) fn facts_screen(&self) -> Rect {
        let s = self.scale;
        let x = self.facts_x - 6 * s;
        Rect::new(
            x,
            self.detail_y - 4 * s,
            self.left + self.list_width - x,
            self.facts_bottom() - self.detail_y + 4 * s,
        )
    }

    /// Where the selected sample's selects and knobs go.
    pub fn controls_area(&self) -> Rect {
        Rect::new(self.left, self.detail_y, (self.facts_x - self.left - 16 * self.scale).max(0), 0)
    }

    /// The kit's title, a click edits it.
    pub fn title_rect(&self) -> Rect {
        let x = self.head_x;
        Rect::new(x, self.title_y, self.left + self.list_width - x, 8 * self.scale)
    }

    /// Line `line` of the kit's description, a click edits it.
    pub fn desc_rect(&self, line: usize) -> Rect {
        let x = self.head_x;
        let y = self.desc_y + line as i32 * KIT_LINE * self.scale;
        Rect::new(x, y, self.left + self.list_width - x, 8 * self.scale)
    }

    /// The title of the column of tracks playing each sample.
    pub fn played_by_rect(&self) -> Rect {
        let x = self.users_x;
        Rect::new(x, self.columns_y, (self.left + self.list_width - x).max(0), 8 * self.scale)
    }

    /// The instrument select of the table's row `row` (from the first shown).
    pub fn instrument_rect(&self, row: usize) -> Rect {
        let s = self.scale;
        let y = self.list_top + row as i32 * self.row;
        Rect::new(self.instrument_x, y + s.px(), KIT_INSTRUMENT_W * s, self.row - 2 * s)
    }

    /// The auto-assign button of the table's row `row`, after its select.
    pub fn auto_rect(&self, row: usize) -> Rect {
        let select = self.instrument_rect(row);
        Rect { x: select.x + select.w + 4 * self.scale, w: KIT_AUTO_W * self.scale, ..select }
    }

    pub fn shown(&self) -> usize {
        self.list_rows as usize
    }

    /// The largest first row of a list of `count` samples: the last ones at the bottom.
    pub fn last_first(&self, count: usize) -> usize {
        count.saturating_sub(self.shown())
    }

    /// The table's scroll bar, right of the rows.
    pub fn scroll_bar(&self) -> Rect {
        let w = KIT_BAR * self.scale;
        Rect::new(self.left + self.list_width - w, self.list_top, w, self.shown() as i32 * self.row)
    }

    /// Start and length of the scroll bar's thumb: the `shown` rows of `count` from `first`.
    pub fn thumb(&self, first: usize, count: usize) -> (i32, i32) {
        scroll_thumb::thumb(self.bar_track(), first, self.shown(), count, KIT_MIN_THUMB * self.scale)
    }

    /// The first row that puts the thumb centred on height `y`.
    pub fn first_at(&self, y: i32, count: usize) -> usize {
        scroll_thumb::first_at(self.bar_track(), y, self.shown(), count, KIT_MIN_THUMB * self.scale)
    }

    fn bar_track(&self) -> (i32, i32) {
        let bar = self.scroll_bar();
        (bar.y, bar.h)
    }

    /// Whether `(x, y)` is over the table's rows, its bar included.
    pub fn table_contains(&self, x: i32, y: i32) -> bool {
        Rect::new(self.left, self.list_top, self.list_width, self.shown() as i32 * self.row).contains(x, y)
    }

    /// Right edge of the table's text: the scroll bar's left side when `count` samples need one.
    pub fn text_right(&self, count: usize) -> i32 {
        let right = self.left + self.list_width;
        if count > self.shown() { right - (KIT_BAR + 4) * self.scale } else { right }
    }

    /// The sample of the list under `(x, y)`, the table shown from `first`.
    pub fn sample_at(&self, first: usize, count: usize, x: i32, y: i32) -> Option<usize> {
        let list =
            Rect::new(self.left, self.list_top, self.text_right(count) - self.left, self.shown() as i32 * self.row);
        if !list.contains(x, y) {
            return None;
        }
        let i = first + row_at(y, self.list_top, self.row)?;
        (i < count).then_some(i)
    }

    /// Where the waveform goes: under the measurements and the sample's
    /// controls (which end at `details_top`), down to the bottom; empty when
    /// there is no room.
    pub fn envelope_rect(&self, details_top: i32) -> Rect {
        let s = self.scale;
        let y = details_top.max(self.facts_bottom()) + 4 * s;
        Rect::new(self.left, y, self.list_width, (self.bottom - y - KIT_CARD_PAD * s).min(80 * s))
    }

    /// Bottom of the measurements' screen: the envelope starts under it.
    pub(super) fn facts_bottom(&self) -> i32 {
        self.detail_y + KIT_FACTS as i32 * KIT_LINE * self.scale + 2 * self.scale
    }
}

/// The start of `text` that fits `width`.
fn fit_start(text: &str, width: i32, scale: Scale) -> String {
    text.chars().take(chars_in(width, scale)).collect()
}

/// `text` fitted to `width` by its end: a path too long keeps the file and
/// the folders nearest to it.
fn fit_end(text: &str, width: i32, scale: Scale) -> String {
    let room = chars_in(width, scale);
    let count = text.chars().count();
    if count <= room {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - room.saturating_sub(3)).collect();
    format!("...{tail}")
}

/// The kit editor's table, on its card: column titles in the band, then a
/// row per sample shown, its number, file and the tracks that play it; the
/// scroll bar when they do not fit.
fn draw_kit_samples(canvas: &mut Canvas, kit: &KitView, layout: &KitLayout) {
    let scale = layout.scale;
    hardware::draw_card(canvas, layout.table_card(), Some(layout.head_h()), false, scale);
    let columns_y = layout.columns_y;
    canvas.text(layout.left, columns_y, "#", scale, theme::text());
    canvas.text(layout.name_x, columns_y, "File", scale, theme::text());
    canvas.text(layout.instrument_x, columns_y, "Instrument", scale, theme::text());
    // "Played by track" is an item of the panel: it has a tooltip.
    if kit.samples.is_empty() {
        canvas.text(layout.name_x, layout.list_top + 3 * scale, "No samples in this kit", scale, theme::muted());
    }
    let first = kit.first;
    let count = kit.samples.len();
    let name_w = layout.instrument_x - layout.name_x - 8 * scale;
    for (row, (i, name)) in kit.samples.iter().enumerate().skip(first).take(layout.shown()).enumerate() {
        let y = layout.list_top + row as i32 * layout.row;
        let text_y = y + (layout.row - 8 * scale) / 2;
        let selected = i == kit.selected;
        if selected {
            canvas.fill_rect(layout.left - 2 * scale, y, layout.list_width + 4 * scale, layout.row, focus_row_color());
        }
        canvas.text(layout.left, text_y, &(i + 1).to_string(), scale, theme::muted());
        canvas.text(
            layout.name_x,
            text_y,
            &fit_start(name, name_w, scale),
            scale,
            if selected { ACCENT } else { theme::text() },
        );
        let (users, color) = match kit.users.get(i).filter(|u| !u.is_empty()) {
            Some(users) => (users.join(", "), theme::text()),
            None => ("-".to_owned(), theme::muted()),
        };
        canvas.text(
            layout.users_x,
            text_y,
            &fit_start(&users, layout.text_right(count) - layout.users_x, scale),
            scale,
            color,
        );
    }

    if count > layout.shown() {
        scroll_thumb::draw(canvas, layout.scroll_bar(), layout.thumb(first, count), scale);
    }
}

/// The kit editor, in the synth editor's hardware look: the metal, the kit's
/// card, the table's, then the selected sample's (its controls are panel
/// items; its measurements and its waveform on screens).
pub(super) fn draw_kit(canvas: &mut Canvas, kit: &KitView, layout: &KitLayout) {
    let scale = layout.scale;
    let fitted = |text: &str, width: i32| fit_start(text, width, scale);
    let fitted_end = |text: &str, width: i32| fit_end(text, width, scale);
    let right = layout.left + layout.list_width;
    hardware::draw_metal(canvas, layout.panel());

    // The kit: its name and counts in the band, then the labels of its title
    // and description (their texts are the panel's, a click edits them).
    hardware::draw_card(canvas, layout.kit_card(), Some(layout.head_h()), false, scale);
    let used = kit.users.iter().filter(|u| !u.is_empty()).count();
    let counts = format!("{} samples, {used} played by the pattern's tracks", kit.samples.len());
    let counts_x = right - text_width(&counts, scale);
    let head_y = layout.head_text_y(layout.kit_y);
    canvas.text(layout.left, head_y, "KIT", scale, theme::text());
    let name = fitted(kit.kit, counts_x - layout.head_x - 8 * scale);
    canvas.text(layout.head_x, head_y, &name, scale, ACCENT);
    canvas.text(counts_x.max(layout.head_x), head_y, &counts, scale, theme::text());
    for (label, y) in [("Folder", layout.folder_y), ("Title", layout.title_y), ("Notes", layout.desc_y)] {
        canvas.text(layout.left, y, label, scale, theme::muted());
    }
    let folder = fitted_end(&kit.folder, right - layout.head_x);
    canvas.text(layout.head_x, layout.folder_y, &folder, scale, theme::text());

    draw_kit_samples(canvas, kit, layout);

    // The selected sample: its number and file in the band.
    hardware::draw_card(canvas, layout.sample_card(kit.details_top), Some(layout.head_h()), false, scale);
    let head_y = layout.head_text_y(layout.sample_y);
    canvas.text(layout.left, head_y, "SAMPLE", scale, theme::text());
    if let Some(name) = kit.samples.get(kit.selected) {
        let shown = format!("{}  {name}", kit.selected + 1);
        canvas.text(layout.head_x, head_y, &fitted(&shown, right - layout.head_x), scale, theme::text());
    }

    // Its measurements, on a screen right of its controls.
    let screen = layout.facts_screen();
    hardware::draw_screen(canvas, screen, scale);
    let label_w = text_width(&"m".repeat(KIT_FACT_LABEL), scale);
    let value_x = layout.facts_x + label_w;
    for (line, (label, value)) in kit.facts.iter().enumerate() {
        let y = layout.detail_y + 2 * scale + line as i32 * KIT_LINE * scale;
        canvas.text(layout.facts_x, y, &fitted(label, label_w - 4 * scale), scale, LCD_DIM);
        canvas.text(value_x, y, &fitted_end(value, right - 4 * scale - value_x), scale, LCD_TEXT);
    }

    // Its waveform, on a screen: smoothed from the bottom, how it plays over it.
    let Rect { x, y, w: width, h: height } = layout.envelope_rect(kit.details_top);
    if kit.envelope.is_empty() || width <= 0 || height <= 4 * scale {
        return;
    }
    hardware::draw_screen(canvas, Rect::new(x, y, width, height), scale);
    let t = scale.px();
    let (x, y, width, height) = (x + 2 * t, y + 2 * t, width - 4 * t, height - 4 * t);
    canvas.draw_profile((x, y + height, width), height, (kit.envelope, 1.0), Direction::Up, ENVELOPE);
    if let Some(overlay) = &kit.overlay {
        overlay.draw(canvas, (x, y, width, height), scale);
    }
}
