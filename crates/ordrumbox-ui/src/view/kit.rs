//! The kit editor: its view, its layout and its drawing.

use super::{ACCENT, ENVELOPE, cell_color, focus_row_color};
use crate::canvas::{Canvas, chars_in, text_width};
use crate::frame::{body_top, footer_top, margin};
use crate::panel::{Rect, row_at};
use crate::sample_overlay::Overlay;
use crate::scale::Scale;
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

/// Where the kit editor's parts are: the kit's header (name, folder, title,
/// description), the table of its samples (number, name, instrument, the
/// tracks that play it), then the selected sample: its controls on the left,
/// its measurements on the right, its envelope under both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitLayout {
    pub(super) scale: Scale,
    pub(super) top: i32,
    pub(super) bottom: i32,
    pub(super) left: i32,
    pub(super) width: i32,
    /// The kit's name, folder, title and description, after their labels.
    pub(super) head_x: i32,
    /// The kit's folder line, title line and first description line.
    pub(super) folder_y: i32,
    pub(super) title_y: i32,
    pub(super) desc_y: i32,
    /// The table: its column titles, first row, row height, row count.
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
/// The widest a sample name gets, in characters.
pub(super) const KIT_NAME_CHARS: usize = 24;
/// Labels of the measurements, in characters (their values after).
pub(super) const KIT_FACT_LABEL: usize = 9;
/// Labels of the kit's header ("Notes"), in characters.
pub(super) const KIT_HEAD_LABEL: usize = 7;

impl KitLayout {
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize) -> Self {
        let s = scale;
        let margin = margin(scale);
        let top = body_top(scale);
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let left = margin;
        let list_width = (width - 2 * margin).max(40 * scale);
        let line = KIT_LINE * s;
        let folder_y = top + line + 2 * s;
        let title_y = folder_y + line;
        let desc_y = title_y + line;
        let columns_y = desc_y + KIT_DESC_LINES as i32 * line + 6 * s;
        let row = KIT_ROW * s;
        let list_top = columns_y + line;
        // The details take what they need (controls, measurements, a short
        // envelope); the rows the rest, a window size giving one count.
        let details = 8 * s + (KIT_FACTS as i32 * line).max(KIT_CONTROLS_HEIGHT * s) + 40 * s;
        let list_rows = ((bottom - list_top - details) / row).clamp(3, 16);
        let detail_y = list_top + list_rows * row + 8 * s;
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
            folder_y,
            title_y,
            desc_y,
            columns_y,
            list_top,
            list_width,
            row,
            list_rows,
            name_x,
            instrument_x,
            users_x,
            detail_y,
            facts_x: left + controls_w + 16 * s,
        }
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

    /// First list row shown, so that `selected` is visible.
    pub fn first(&self, selected: usize) -> usize {
        selected.saturating_sub(self.shown().saturating_sub(1))
    }

    /// The sample of the list under `(x, y)`.
    pub fn hit(&self, view_selected: usize, count: usize, x: i32, y: i32) -> Option<usize> {
        let list_bottom = self.list_top + self.shown() as i32 * self.row;
        if y < self.list_top || y >= list_bottom || !(self.left..self.left + self.list_width).contains(&x) {
            return None;
        }
        let i = self.first(view_selected) + row_at(y, self.list_top, self.row)?;
        (i < count).then_some(i)
    }

    /// Where the waveform goes: under the measurements and the sample's
    /// controls (which end at `details_top`), down to the bottom; empty when
    /// there is no room.
    pub fn envelope_rect(&self, details_top: i32) -> Rect {
        let s = self.scale;
        let y = details_top.max(self.facts_bottom()) + 4 * s;
        Rect::new(self.left, y, self.list_width, (self.bottom - y - 4 * s).min(80 * s))
    }

    /// Bottom of the measurements: the envelope starts under them.
    pub(super) fn facts_bottom(&self) -> i32 {
        self.detail_y + KIT_FACTS as i32 * KIT_LINE * self.scale
    }
}

/// The kit editor: the kit's samples on the left, the selected one on the
/// right (fields, analysis, envelope with the decay marked).
pub(super) fn draw_kit(canvas: &mut Canvas, kit: &KitView, layout: &KitLayout) {
    let scale = layout.scale;
    let fitted = |text: &str, width: i32| -> String { text.chars().take(chars_in(width, scale)).collect() };
    let right = layout.left + layout.list_width;
    // A path too long keeps its end, the file and the folders nearest to it.
    let fitted_end = |text: &str, width: i32| -> String {
        let room = chars_in(width, scale);
        let count = text.chars().count();
        if count <= room {
            return text.to_owned();
        }
        let tail: String = text.chars().skip(count - room.saturating_sub(3)).collect();
        format!("...{tail}")
    };

    // The kit: its name and counts, then the labels of its title and
    // description (their texts are the panel's, a click edits them).
    let used = kit.users.iter().filter(|u| !u.is_empty()).count();
    let counts = format!("{} samples, {used} played by the pattern's tracks", kit.samples.len());
    let counts_x = right - text_width(&counts, scale);
    canvas.text(layout.left, layout.top + 2 * scale, "KIT", scale, theme::muted());
    let name = fitted(kit.kit, counts_x - layout.head_x - 8 * scale);
    canvas.text(layout.head_x, layout.top + 2 * scale, &name, scale, ACCENT);
    canvas.text(counts_x.max(layout.head_x), layout.top + 2 * scale, &counts, scale, theme::muted());
    for (label, y) in [("Folder", layout.folder_y), ("Title", layout.title_y), ("Notes", layout.desc_y)] {
        canvas.text(layout.left, y, label, scale, theme::muted());
    }
    let folder = fitted_end(&kit.folder, right - layout.head_x);
    canvas.text(layout.head_x, layout.folder_y, &folder, scale, theme::text());

    // The table: column titles, then a row per sample.
    let columns_y = layout.columns_y;
    canvas.text(layout.left, columns_y, "#", scale, theme::muted());
    canvas.text(layout.name_x, columns_y, "File", scale, theme::muted());
    canvas.text(layout.instrument_x, columns_y, "Instrument", scale, theme::muted());
    // "Played by track" is an item of the panel: it has a tooltip.
    canvas.fill_rect(layout.left, layout.list_top - 3 * scale, layout.list_width, scale.px(), theme::line());
    if kit.samples.is_empty() {
        canvas.text(layout.name_x, layout.list_top + 3 * scale, "No samples in this kit", scale, theme::muted());
    }
    let first = layout.first(kit.selected);
    let name_w = layout.instrument_x - layout.name_x - 8 * scale;
    for (row, (i, name)) in kit.samples.iter().enumerate().skip(first).take(layout.shown()).enumerate() {
        let y = layout.list_top + row as i32 * layout.row;
        let text_y = y + (layout.row - 8 * scale) / 2;
        let selected = i == kit.selected;
        if selected {
            canvas.fill_rect(layout.left - 2 * scale, y, layout.list_width + 4 * scale, layout.row, focus_row_color());
        }
        canvas.text(layout.left, text_y, &(i + 1).to_string(), scale, theme::muted());
        canvas.text(layout.name_x, text_y, &fitted(name, name_w), scale, if selected { ACCENT } else { theme::text() });
        let (users, color) = match kit.users.get(i).filter(|u| !u.is_empty()) {
            Some(users) => (users.join(", "), theme::text()),
            None => ("-".to_owned(), theme::muted()),
        };
        canvas.text(layout.users_x, text_y, &fitted(&users, right - layout.users_x), scale, color);
    }

    // The selected sample's measurements, right of its controls.
    let label_w = text_width(&"m".repeat(KIT_FACT_LABEL), scale);
    let value_x = layout.facts_x + label_w;
    for (line, (label, value)) in kit.facts.iter().enumerate() {
        let y = layout.detail_y + 2 * scale + line as i32 * KIT_LINE * scale;
        canvas.text(layout.facts_x, y, &fitted(label, label_w - 4 * scale), scale, theme::muted());
        canvas.text(value_x, y, &fitted_end(value, right - value_x), scale, theme::text());
    }

    // Envelope: one bar per point, the decay end as a line.
    let Rect { x, y, w: width, h: height } = layout.envelope_rect(kit.details_top);
    if kit.envelope.is_empty() || width <= 0 || height <= 4 * scale {
        return;
    }
    canvas.fill_rect(x, y, width, height, cell_color());
    let n = kit.envelope.len() as i32;
    for (i, v) in kit.envelope.iter().enumerate() {
        let bar_x = x + i as i32 * width / n;
        let bar_w = ((i as i32 + 1) * width / n - i as i32 * width / n).max(1);
        let bar_h = (v.clamp(0.0, 1.0) * f64::from(height)).round() as i32;
        canvas.fill_rect(bar_x, y + height - bar_h, bar_w, bar_h, ENVELOPE);
    }
    if let Some(overlay) = &kit.overlay {
        overlay.draw(canvas, (x, y, width, height), scale);
    }
}
