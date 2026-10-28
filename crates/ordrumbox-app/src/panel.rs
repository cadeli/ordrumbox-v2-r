//! The widgets of the editors (track editor, note editor, kit editor, the
//! grid's volume sliders, the song panel's column): a flat list of [`Item`]s, each with its place, an
//! [`Id`] when it reacts to the mouse or the keyboard, and what it shows.
//! The list is rebuilt from the state at every frame; drawing and
//! hit-testing both use it. The open drop-down list and the number entry
//! are drawn over everything.

use crate::canvas::{Canvas, Rgb, text_width};
use crate::control::{Spec, Style};
use crate::kits::Field;

pub const BACKGROUND: Rgb = 0x001C_1C22;
pub const BORDER: Rgb = 0x0038_3845;
pub const FIELD: Rgb = 0x002A_2A33;
pub const TEXT: Rgb = 0x00E6_E6EA;
pub const DIM: Rgb = 0x0080_8090;
pub const ACCENT: Rgb = 0x00F2_A93B;
pub const ON: Rgb = 0x005C_D67A;
const FOCUS: Rgb = 0x006A_B7FF;
const LFO: Rgb = 0x00B0_7CFF;
const DISC: Rgb = 0x0030_3040;
const SELECTED_ROW: Rgb = 0x0026_2632;
const ENVELOPE: Rgb = 0x00F2_A93B;
const MARKER: Rgb = 0x00FF_FFFF;

/// A rectangle of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        (self.x..self.x + self.w).contains(&x) && (self.y..self.y + self.h).contains(&y)
    }
}

/// An LFO parameter of the modulation tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LfoParam {
    Freq,
    Min,
    Max,
    Phase,
}

impl LfoParam {
    pub fn key(self) -> &'static str {
        match self {
            Self::Freq => "freq",
            Self::Min => "min",
            Self::Max => "max",
            Self::Phase => "phase",
        }
    }
}

/// What a widget is (and does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Id {
    // Track editor.
    /// Main tab: fx, sound, mod, loop, gen.
    Tab(u8),
    /// Effect sub-tab (Rev, Dly, Sat, fltr), and its on/off LED.
    FxTab(u8),
    FxLed(u8),
    /// Groove / Engine sub-tab.
    GenTab(u8),
    /// LFO target sub-tab, and its on/off LED.
    LfoTab(u8),
    LfoLed(u8),
    /// The knob bar (velocity, pan, pitch).
    KnobBar(&'static str),
    /// The knob bar's decay (the sample's, not the track's).
    Decay,
    /// An effect knob.
    Fx(&'static str),
    /// A generation slider.
    Gen(&'static str),
    /// A loop slider.
    Loop(&'static str),
    Lfo(LfoParam),
    LfoType,
    /// A select of a track setting (effect types, delay time, variant).
    TrackSelect(&'static str),
    /// An ON/OFF button of a track setting (mono, auto, scale lock).
    Toggle(&'static str),
    /// Filter type button: lowpass, highpass, bandpass.
    FilterIcon(u8),
    /// The sound tab's auto-assign LED.
    AutoAssign,
    Instrument,
    Sample,
    Synth,
    /// The sample bar's load button.
    LoadSample,
    /// A row's label: selects the row.
    Row(&'static str),
    // Note editor.
    NoteTab(u8),
    NoteKnob(&'static str),
    NoteSlider(&'static str),
    /// Arp scale / direction select.
    NoteSelect(&'static str),
    // Kit editor.
    KitKit,
    KitInstrument,
    KitKnob(Field),
    // Header.
    PatternList,
    KitList,
    /// Play / stop, the tempo knob, the beats list, the generation
    /// buttons (drums, bass, chords).
    Transport,
    Bpm,
    Beats,
    Generate(u8),
    // Song panel.
    /// The arrangement select.
    SongList,
    /// Save, load, export, import, new, delete.
    SongButton(&'static str),
    SongName,
    SongDescription,
    /// A pattern of the song panel's list (its rename entry).
    SongPattern(usize),
    /// The selected pattern's description entry.
    PatternDescription,
    /// The right-click menu of the song panel.
    SongMenu,
    // Output panel.
    OutTab(u8),
    /// A master bus setting, by its `settings.master` key.
    Master(&'static str),
    // Piano roll.
    /// The track it shows.
    RollTrack,
    /// Its right-click menus.
    RollMenu,
    // Grid.
    TrackVolume(usize),
    MasterVolume,
}

/// What a widget shows.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// A knob or a slider (by its spec); `text` is its value as shown. A
    /// tall knob shows its label under it, a row knob none.
    Value {
        spec: Spec,
        value: f64,
        label: Option<String>,
        text: String,
        lfo: bool,
    },
    /// A drop-down list: its options (values and labels) and the one shown.
    Select {
        values: Vec<String>,
        labels: Vec<String>,
        selected: usize,
    },
    /// An ON/OFF button.
    Toggle {
        on: bool,
    },
    Led {
        on: bool,
    },
    Tab {
        label: String,
        active: bool,
    },
    Button {
        label: String,
        on: bool,
    },
    Text {
        text: String,
        color: Rgb,
    },
    /// A sample's envelope (peaks 0–1) and where its decay ends (0–1).
    Envelope {
        points: Vec<f64>,
        marker: Option<f64>,
    },
    /// A highlighted row background (the selected setting).
    Highlight,
    Line,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: Option<Id>,
    pub rect: Rect,
    pub kind: Kind,
}

impl Item {
    pub fn new(id: Option<Id>, rect: Rect, kind: Kind) -> Self {
        Self { id, rect, kind }
    }

    pub fn text(x: i32, y: i32, text: impl Into<String>, color: Rgb, scale: i32) -> Self {
        let text = text.into();
        let rect = Rect::new(x, y, text_width(&text, scale), 8 * scale);
        Self { id: None, rect, kind: Kind::Text { text, color } }
    }

    /// Takes the keyboard focus (Tab cycles through these).
    pub fn focusable(&self) -> bool {
        self.id.is_some()
            && !matches!(self.kind, Kind::Text { .. } | Kind::Envelope { .. } | Kind::Highlight | Kind::Line)
            && !matches!(self.id, Some(Id::Row(_)))
    }
}

/// The topmost item with an id under `(x, y)`.
pub fn hit(items: &[Item], x: i32, y: i32) -> Option<&Item> {
    items.iter().rev().find(|i| i.id.is_some() && i.rect.contains(x, y))
}

pub fn find(items: &[Item], id: Id) -> Option<&Item> {
    items.iter().find(|i| i.id == Some(id))
}

/// The focusable item `delta` places from `from` (wrapping).
pub fn next_focus(items: &[Item], from: Option<Id>, delta: isize) -> Option<Id> {
    let ids: Vec<Id> = items.iter().filter(|i| i.focusable()).filter_map(|i| i.id).collect();
    if ids.is_empty() {
        return None;
    }
    let n = ids.len() as isize;
    let at = from.and_then(|f| ids.iter().position(|&i| i == f)).map_or(if delta > 0 { -1 } else { n }, |p| p as isize);
    Some(ids[(at + delta).rem_euclid(n) as usize])
}

// ─── Geometry shared by drawing and the mouse ────────────────────────────────

/// Width of a slider row's label.
pub fn label_width(scale: i32) -> i32 {
    88 * scale
}

/// Width of a slider row's value text.
fn value_width(scale: i32) -> i32 {
    64 * scale
}

/// The bar of a slider item (with a label, with a value text): left edge and width.
pub fn slider_bar(rect: Rect, labelled: bool, valued: bool, scale: i32) -> (i32, i32) {
    let left = rect.x + if labelled { label_width(scale) } else { 0 } + 4 * scale;
    let right = rect.x + rect.w - if valued { value_width(scale) + 6 * scale } else { 2 * scale };
    (left, (right - left).max(8 * scale))
}

/// Diameter of a knob in an item of height `h`.
fn knob_size(rect: Rect, scale: i32) -> i32 {
    if rect.h >= 40 * scale { 26 * scale } else { (rect.h - 2 * scale).max(8 * scale) }
}

// ─── Drawing ─────────────────────────────────────────────────────────────────

fn stroke(canvas: &mut Canvas, r: Rect, t: i32, color: Rgb) {
    canvas.fill_rect(r.x, r.y, r.w, t, color);
    canvas.fill_rect(r.x, r.y + r.h - t, r.w, t, color);
    canvas.fill_rect(r.x, r.y, t, r.h, color);
    canvas.fill_rect(r.x + r.w - t, r.y, t, r.h, color);
}

fn fill_circle(canvas: &mut Canvas, cx: i32, cy: i32, r: i32, color: Rgb) {
    for dy in -r..=r {
        let half = f64::from(r * r - dy * dy).sqrt() as i32;
        canvas.fill_rect(cx - half, cy + dy, 2 * half + 1, 1, color);
    }
}

/// Text cut to `width` pixels.
fn fit(text: &str, width: i32, scale: i32) -> String {
    let chars = usize::try_from(width / (8 * scale).max(1)).unwrap_or(0);
    text.chars().take(chars).collect()
}

/// A knob: a 270° arc from bottom left, lit up to `fraction`, a disc and its pointer.
fn draw_knob(canvas: &mut Canvas, cx: i32, cy: i32, size: i32, fraction: f64, color: Rgb, scale: i32) {
    let r = f64::from(size) / 2.0;
    let dot = scale.max(1) * if size >= 20 * scale { 2 } else { 1 };
    let start = 135f64.to_radians();
    let sweep = 270f64.to_radians();
    let steps = (r * 6.0) as i32 + 24;
    for i in 0..=steps {
        let f = f64::from(i) / f64::from(steps);
        let a = start + sweep * f;
        let x = cx + (a.cos() * (r - f64::from(dot))) as i32;
        let y = cy + (a.sin() * (r - f64::from(dot))) as i32;
        canvas.fill_rect(x - dot / 2, y - dot / 2, dot, dot, if f <= fraction { color } else { BORDER });
    }
    let disc = (r * 0.62) as i32;
    fill_circle(canvas, cx, cy, disc, DISC);
    let a = start + sweep * fraction;
    for k in 0..=disc {
        let x = cx + (a.cos() * f64::from(k)) as i32;
        let y = cy + (a.sin() * f64::from(k)) as i32;
        canvas.fill_rect(x - scale.max(1) / 2, y - scale.max(1) / 2, scale.max(1), scale.max(1), TEXT);
    }
}

/// Draw `items`; `focus` gets an outline.
pub fn draw(canvas: &mut Canvas, items: &[Item], focus: Option<Id>, scale: i32) {
    let t = scale.max(1);
    for item in items {
        let r = item.rect;
        let focused = item.id.is_some() && item.id == focus;
        match &item.kind {
            Kind::Highlight => canvas.fill_rect(r.x, r.y, r.w, r.h, SELECTED_ROW),
            Kind::Line => canvas.fill_rect(r.x, r.y, r.w, t, BORDER),
            Kind::Text { text, color } => {
                canvas.text(r.x, r.y, &fit(text, r.w.max(text_width(text, scale)), scale), scale, *color);
            }
            Kind::Value { spec, value, label, text, lfo } => {
                let fraction = spec.fraction(*value);
                let color = if *lfo { LFO } else { ACCENT };
                match spec.style {
                    Style::Knob => {
                        let size = knob_size(r, scale);
                        if r.h >= 40 * scale {
                            let cx = r.x + r.w / 2;
                            draw_knob(canvas, cx, r.y + size / 2, size, fraction, color, scale);
                            let mut y = r.y + size + 3 * scale;
                            if let Some(label) = label {
                                let label = fit(label, r.w, scale);
                                canvas.text(cx - text_width(&label, scale) / 2, y, &label, scale, DIM);
                                y += 10 * scale;
                            }
                            let text = fit(text, r.w, scale);
                            canvas.text(cx - text_width(&text, scale) / 2, y, &text, scale, TEXT);
                        } else {
                            draw_knob(canvas, r.x + size / 2, r.y + r.h / 2, size, fraction, color, scale);
                            let x = r.x + size + 6 * scale;
                            canvas.text(x, r.y + (r.h - 8 * scale) / 2, &fit(text, r.x + r.w - x, scale), scale, TEXT);
                        }
                    }
                    Style::Slider => {
                        let ty = r.y + (r.h - 8 * scale) / 2;
                        if let Some(label) = label {
                            canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, DIM);
                        }
                        let (left, width) = slider_bar(r, label.is_some(), !text.is_empty(), scale);
                        let bar_y = r.y + r.h / 2 - 2 * scale;
                        canvas.fill_rect(left, bar_y, width, 4 * scale, FIELD);
                        let filled = (fraction * f64::from(width)) as i32;
                        canvas.fill_rect(left, bar_y, filled, 4 * scale, color);
                        let thumb = left + filled - 2 * scale;
                        canvas.fill_rect(thumb, r.y + r.h / 2 - 5 * scale, 4 * scale, 10 * scale, TEXT);
                        let x = left + width + 6 * scale;
                        canvas.text(x, ty, &fit(text, r.x + r.w - x, scale), scale, TEXT);
                    }
                }
            }
            Kind::Select { labels, selected, .. } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, FIELD);
                let label = labels.get(*selected).map_or("", String::as_str);
                let ty = r.y + (r.h - 8 * scale) / 2;
                canvas.text(r.x + 4 * scale, ty, &fit(label, r.w - 16 * scale, scale), scale, TEXT);
                canvas.text(r.x + r.w - 10 * scale, ty, "v", scale, DIM);
            }
            Kind::Toggle { on } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ON } else { FIELD });
                let text = if *on { "ON" } else { "OFF" };
                let color = if *on { BACKGROUND } else { DIM };
                canvas.text(r.x + (r.w - text_width(text, scale)) / 2, r.y + (r.h - 8 * scale) / 2, text, scale, color);
            }
            Kind::Led { on } => {
                let s = 6 * scale;
                let (x, y) = (r.x + (r.w - s) / 2, r.y + (r.h - s) / 2);
                canvas.fill_rect(x, y, s, s, if *on { ON } else { BORDER });
            }
            Kind::Tab { label, active } => {
                if *active {
                    canvas.fill_rect(r.x, r.y, r.w, r.h, FIELD);
                    canvas.fill_rect(r.x, r.y + r.h - 2 * t, r.w, 2 * t, ACCENT);
                }
                let color = if *active { ACCENT } else { DIM };
                let x = r.x + (r.w - text_width(label, scale)) / 2;
                canvas.text(x, r.y + (r.h - 8 * scale) / 2, label, scale, color);
            }
            Kind::Button { label, on } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ACCENT } else { FIELD });
                let color = if *on { BACKGROUND } else { TEXT };
                let x = r.x + (r.w - text_width(label, scale)) / 2;
                canvas.text(x, r.y + (r.h - 8 * scale) / 2, label, scale, color);
            }
            Kind::Envelope { points, marker } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, FIELD);
                let n = points.len() as i32;
                for (i, v) in points.iter().enumerate() {
                    let i = i as i32;
                    let x = r.x + i * r.w / n.max(1);
                    let w = ((i + 1) * r.w / n.max(1) - i * r.w / n.max(1)).max(1);
                    let h = (v.clamp(0.0, 1.0) * f64::from(r.h)).round() as i32;
                    canvas.fill_rect(x, r.y + r.h - h, w, h, ENVELOPE);
                }
                if let Some(m) = marker {
                    let x = r.x + (m.clamp(0.0, 1.0) * f64::from(r.w - 1)) as i32;
                    let mut y = r.y;
                    while y < r.y + r.h {
                        canvas.fill_rect(x, y, t, (4 * scale).min(r.y + r.h - y), MARKER);
                        y += 8 * scale;
                    }
                }
            }
        }
        if focused {
            stroke(canvas, Rect::new(r.x - 2 * t, r.y - 2 * t, r.w + 4 * t, r.h + 4 * t), t, FOCUS);
        }
    }
}

// ─── Drop-down list ──────────────────────────────────────────────────────────

/// Rows a drop-down list shows at once.
const LIST_ROWS: usize = 14;

/// An open drop-down list (or a right-click menu, with a title).
#[derive(Debug, Clone, PartialEq)]
pub struct Dropdown {
    pub id: Id,
    /// A line over the options (`ContextMenu`'s title).
    pub title: Option<String>,
    /// The select's place: the list opens under it (or over it near the bottom).
    pub anchor: Rect,
    pub values: Vec<String>,
    pub labels: Vec<String>,
    /// Highlighted option.
    pub at: usize,
    /// First option shown.
    pub first: usize,
}

impl Dropdown {
    pub fn new(id: Id, anchor: Rect, values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        let mut d = Self { id, title: None, anchor, values, labels, at: selected, first: 0 };
        d.show_at();
        d
    }

    /// A menu of actions at `(x, y)`, under `title`.
    pub fn menu(id: Id, x: i32, y: i32, title: String, values: Vec<String>, labels: Vec<String>) -> Self {
        Self { title: Some(title), ..Self::new(id, Rect::new(x, y, 0, 0), values, labels, 0) }
    }

    /// Rows over the options (the title).
    fn head(&self) -> usize {
        usize::from(self.title.is_some())
    }

    fn shown(&self) -> usize {
        self.labels.len().min(LIST_ROWS)
    }

    fn show_at(&mut self) {
        if self.at < self.first {
            self.first = self.at;
        } else if self.at >= self.first + self.shown() {
            self.first = self.at + 1 - self.shown();
        }
    }

    pub fn step(&mut self, delta: isize) {
        if self.labels.is_empty() {
            return;
        }
        self.at = (self.at as isize + delta).clamp(0, self.labels.len() as isize - 1) as usize;
        self.show_at();
    }

    /// Scroll by `delta` rows (the wheel), the highlight kept in view.
    pub fn scroll(&mut self, delta: isize) {
        let max = self.labels.len().saturating_sub(self.shown());
        self.first = (self.first as isize + delta).clamp(0, max as isize) as usize;
        self.at = self.at.clamp(self.first, self.first + self.shown().saturating_sub(1));
    }

    /// The next option starting with `letter` (case-insensitive), after the highlighted one.
    pub fn jump(&mut self, letter: char) {
        let n = self.labels.len();
        let letter = letter.to_ascii_lowercase();
        if let Some(i) = (1..=n)
            .map(|k| (self.at + k) % n)
            .find(|&i| self.labels[i].chars().next().is_some_and(|c| c.to_ascii_lowercase() == letter))
        {
            self.at = i;
            self.show_at();
        }
    }

    /// The list's place in a `width` × `height` window.
    pub fn rect(&self, width: i32, height: i32, scale: i32) -> Rect {
        let row = row_height(scale);
        let longest = self.labels.iter().chain(&self.title).map(|l| text_width(l, scale)).max().unwrap_or(0);
        let w = (longest + 20 * scale).max(self.anchor.w).min(width);
        let h = (self.shown() + self.head()) as i32 * row + 4 * scale;
        let x = self.anchor.x.min(width - w).max(0);
        let below = self.anchor.y + self.anchor.h;
        let y = if below + h <= height { below } else { (self.anchor.y - h).max(0) };
        Rect::new(x, y, w, h)
    }

    /// The option under `(x, y)`.
    pub fn hit(&self, width: i32, height: i32, scale: i32, x: i32, y: i32) -> Option<usize> {
        let r = self.rect(width, height, scale);
        if !r.contains(x, y) {
            return None;
        }
        let row = usize::try_from((y - r.y - 2 * scale) / row_height(scale)).ok()?.checked_sub(self.head())?;
        let i = self.first + row;
        (i < self.labels.len()).then_some(i)
    }

    pub fn draw(&self, canvas: &mut Canvas, scale: i32) {
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x, r.y, r.w, r.h, BORDER);
        canvas.fill_rect(r.x + scale, r.y + scale, r.w - 2 * scale, r.h - 2 * scale, BACKGROUND);
        let row = row_height(scale);
        if let Some(title) = &self.title {
            let y = r.y + 2 * scale + (row - 8 * scale) / 2;
            canvas.text(r.x + 6 * scale, y, &fit(title, r.w - 12 * scale, scale), scale, DIM);
        }
        let top = r.y + 2 * scale + self.head() as i32 * row;
        for (k, label) in self.labels.iter().enumerate().skip(self.first).take(self.shown()) {
            let y = top + (k - self.first) as i32 * row;
            if k == self.at {
                canvas.fill_rect(r.x + scale, y, r.w - 2 * scale, row, SELECTED_ROW);
            }
            let color = if k == self.at { ACCENT } else { TEXT };
            canvas.text(r.x + 6 * scale, y + (row - 8 * scale) / 2, &fit(label, r.w - 12 * scale, scale), scale, color);
        }
        if self.labels.len() > self.shown() {
            // Scroll bar.
            let total = self.labels.len() as i32;
            let track = r.h - 4 * scale - self.head() as i32 * row;
            let top = top + track * self.first as i32 / total;
            let len = (track * self.shown() as i32 / total).max(4 * scale);
            canvas.fill_rect(r.x + r.w - 4 * scale, top, 2 * scale, len, DIM);
        }
    }
}

fn row_height(scale: i32) -> i32 {
    12 * scale
}

// ─── Number entry ────────────────────────────────────────────────────────────

/// The number typed for a control (`promptNumericInput`, right-click or
/// Enter), or a text (a name, a description: `spec` is `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub spec: Option<Spec>,
    pub title: String,
    pub text: String,
}

impl Entry {
    pub fn rect(&self, width: i32, height: i32, scale: i32) -> Rect {
        let w = (text_width(&self.title, scale) + 32 * scale).max(220 * scale).min(width);
        let h = 52 * scale;
        Rect::new((width - w) / 2, (height - h) / 2, w, h)
    }

    pub fn draw(&self, canvas: &mut Canvas, scale: i32) {
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x - 4 * scale, r.y - 4 * scale, r.w + 8 * scale, r.h + 8 * scale, 0x0010_1014);
        canvas.fill_rect(r.x, r.y, r.w, r.h, BORDER);
        canvas.fill_rect(r.x + scale, r.y + scale, r.w - 2 * scale, r.h - 2 * scale, BACKGROUND);
        canvas.text(r.x + 12 * scale, r.y + 8 * scale, &self.title, scale, ACCENT);
        let field = Rect::new(r.x + 12 * scale, r.y + 22 * scale, r.w - 24 * scale, 12 * scale);
        canvas.fill_rect(field.x, field.y, field.w, field.h, FIELD);
        let x = canvas.text(field.x + 4 * scale, field.y + 2 * scale, &self.text, scale, TEXT);
        canvas.fill_rect(x + scale, field.y + 2 * scale, scale.max(1), 8 * scale, ACCENT);
        canvas.text(r.x + 12 * scale, r.y + 40 * scale, "ENTER: set   ESC: cancel", scale, DIM);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_cycles_over_focusable_items() {
        let r = Rect::new(0, 0, 10, 10);
        let items = vec![
            Item::new(Some(Id::Tab(0)), r, Kind::Tab { label: "fx".into(), active: true }),
            Item::new(None, r, Kind::Line),
            Item::new(Some(Id::Row("x")), r, Kind::Highlight),
            Item::new(Some(Id::Toggle("mono")), r, Kind::Toggle { on: false }),
        ];
        assert_eq!(next_focus(&items, None, 1), Some(Id::Tab(0)));
        assert_eq!(next_focus(&items, Some(Id::Tab(0)), 1), Some(Id::Toggle("mono")));
        assert_eq!(next_focus(&items, Some(Id::Tab(0)), -1), Some(Id::Toggle("mono")));
        assert_eq!(hit(&items, 5, 5).and_then(|i| i.id), Some(Id::Toggle("mono")));
    }

    #[test]
    fn dropdown_scrolls_and_hits() {
        let labels: Vec<String> = (0..40).map(|i| format!("item{i}")).collect();
        let mut d = Dropdown::new(Id::Sample, Rect::new(10, 10, 100, 12), labels.clone(), labels, 30);
        assert!(d.first <= 30 && 30 < d.first + LIST_ROWS);
        d.step(100);
        assert_eq!(d.at, 39);
        d.scroll(-100);
        assert_eq!(d.first, 0);
        assert_eq!(d.at, LIST_ROWS - 1);
        let r = d.rect(800, 600, 1);
        assert_eq!(d.hit(800, 600, 1, r.x + 5, r.y + 2 + 12 * 3 + 1), Some(3));
        d.jump('i');
        assert_eq!(d.at, LIST_ROWS);
        // A menu: its title row takes no click.
        let m = Dropdown::menu(
            Id::SongMenu,
            10,
            10,
            "title".into(),
            vec!["a".into(), "b".into()],
            vec!["A".into(), "B".into()],
        );
        let r = m.rect(800, 600, 1);
        assert_eq!(m.hit(800, 600, 1, r.x + 5, r.y + 3), None);
        assert_eq!(m.hit(800, 600, 1, r.x + 5, r.y + 2 + 12 + 1), Some(0));
        assert_eq!(m.hit(800, 600, 1, r.x + 5, r.y + 2 + 24 + 1), Some(1));
    }

    #[test]
    fn draws_every_kind() {
        let mut pixels = vec![0; 400 * 300];
        let mut canvas = Canvas::new(&mut pixels, 400, 300);
        let spec = Spec::knob(0.0, 1.0, 0.01, 0.0);
        let items = vec![
            Item::new(
                Some(Id::KnobBar("velocity")),
                Rect::new(0, 0, 60, 50),
                Kind::Value { spec, value: 0.5, label: Some("Vel".into()), text: "50 %".into(), lfo: false },
            ),
            Item::new(
                Some(Id::Fx("reverbAmount")),
                Rect::new(0, 60, 160, 16),
                Kind::Value { spec, value: 0.2, label: None, text: "0.2".into(), lfo: true },
            ),
            Item::new(
                Some(Id::Gen("variation")),
                Rect::new(0, 80, 300, 16),
                Kind::Value {
                    spec: Spec::slider(0.0, 100.0, 1.0, 0.0),
                    value: 30.0,
                    label: Some("Var Pos".into()),
                    text: "30".into(),
                    lfo: false,
                },
            ),
            Item::new(
                Some(Id::Sample),
                Rect::new(0, 100, 120, 12),
                Kind::Select { values: vec!["a".into()], labels: vec!["a".into()], selected: 0 },
            ),
            Item::new(None, Rect::new(0, 120, 300, 40), Kind::Envelope { points: vec![0.0, 1.0], marker: Some(0.5) }),
        ];
        draw(&mut canvas, &items, Some(Id::Sample), 1);
        Dropdown::new(Id::Sample, Rect::new(390, 290, 20, 12), vec!["x".into(); 3], vec!["x".into(); 3], 0)
            .draw(&mut canvas, 1);
        let spec = Spec::knob(20.0, 5000.0, 10.0, 500.0);
        Entry { id: Id::Decay, spec: Some(spec), title: "Enter value".into(), text: "12".into() }.draw(&mut canvas, 1);
        Dropdown::menu(Id::SongMenu, 390, 290, "rock".into(), vec!["a".into()], vec!["Add".into()])
            .draw(&mut canvas, 1);
    }
}
