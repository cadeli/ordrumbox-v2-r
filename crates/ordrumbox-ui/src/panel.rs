//! The widgets of the editors (track editor, note editor, kit editor, the
//! grid's volume sliders, the song panel's column): a flat list of [`Item`]s, each with its place, an
//! [`Id`] when it reacts to the mouse or the keyboard, and what it shows.
//! The list is rebuilt from the state at every frame; drawing and
//! hit-testing both use it. The open drop-down list and the number entry
//! are drawn over everything.

use crate::canvas::{Canvas, Rgb, fit, text_width};
use crate::control::{Spec, Style};
use crate::kits::Field;
use crate::scale::Scale;
use crate::theme;

pub fn background_color() -> Rgb {
    theme::panel()
}
pub fn border_color() -> Rgb {
    theme::line()
}
pub fn field_color() -> Rgb {
    theme::cell()
}
/// Labels and secondary text.
pub fn dim() -> Rgb {
    theme::muted()
}
/// A knob's or a slider's arc, its handle and its value: neutral, so the
/// accents keep their meaning ([`theme::control`], [`theme::text`]).
fn arc_color() -> Rgb {
    theme::control()
}
fn handle_color() -> Rgb {
    theme::text()
}
fn value_color() -> Rgb {
    theme::text()
}
/// A text field's caret.
const CARET: Rgb = theme::FOCUS;
pub const ON: Rgb = theme::LIVE;
const FOCUS: Rgb = theme::FOCUS;
/// The active tab and button.
const ACTIVE: Rgb = theme::VALUE;
const TITLE: Rgb = theme::ACCENT;
const LFO: Rgb = theme::CHANCE;
fn disc_color() -> Rgb {
    theme::panel_2()
}
fn selected_row_color() -> Rgb {
    theme::selected_row()
}
const ENVELOPE: Rgb = theme::LIVE;
const MARKER: Rgb = theme::ACCENT;

/// The row of a list under `y`, rows `row_height` high from `top`; `None`
/// above the list (a division alone would put the pixels just above in row 0).
pub fn row_at(y: i32, top: i32, row_height: i32) -> Option<usize> {
    if y < top || row_height <= 0 {
        return None;
    }
    usize::try_from((y - top) / row_height).ok()
}

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
    /// Its knob's label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Freq => "Rate",
            Self::Min => "Min",
            Self::Max => "Max",
            Self::Phase => "Phase",
        }
    }

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
    /// An effect's on / off button, named after it.
    FxPower(u8),
    /// Groove / Engine / Compose sub-tab.
    GenerationTab(u8),
    /// A compose setting of a drum track (its style's value when unset).
    Compose(&'static str),
    /// ↻: a new line on the edited track.
    Regenerate,
    /// The Generate tab's Advanced settings, shown or folded.
    GenerationAdvanced,
    /// The on / off button of an LFO target, named after it.
    LfoPower(u8),
    /// The knob bar (volume, pan, pitch).
    KnobBar(&'static str),
    /// The knob bar's decay (the sample's, not the track's).
    Decay,
    /// An effect knob.
    Fx(&'static str),
    /// A generation slider.
    Generation(&'static str),
    /// A loop slider.
    Loop(&'static str),
    /// A setting of the LFO of a target (an index in `LFO_TARGETS`), and its wave.
    Lfo(u8, LfoParam),
    LfoType(u8),
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
    /// The kit to move the selected sample to.
    KitKit,
    /// The instrument of a sample, by its index in the kit (its table row).
    KitSampleInstrument(usize),
    /// The auto-assign button of a sample's row: its instrument from its file name.
    KitAutoInstrument(usize),
    KitKnob(Field),
    /// The kit's title or description, by its `kits.json` key.
    KitText(&'static str),
    /// The title of the table's column of tracks playing each sample.
    KitPlayedBy,
    // Header.
    PatternList,
    KitList,
    /// Play / stop, the tempo knob, the beats list, the generation
    /// buttons (drums, bass, chords).
    Transport,
    Bpm,
    Beats,
    /// The style list, ⟳ (the pattern made again in it), AUTO (every track in
    /// auto mode), and a generation macro of the pattern.
    StyleList,
    GeneratePattern,
    AutoAll,
    Macro(&'static str),
    /// A view button: grid, piano roll, song, synth, output.
    View(u8),
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
    /// The list of sound devices File > Audio output opens.
    AudioMenu,
    // Output panel.
    /// A master bus setting, by its `settings.master` key.
    Master(&'static str),
    // Synth editor.
    SynthPreset,
    SynthButton(&'static str),
    SynthTab(u8),
    /// A card's power button, its icon choices: by the card's index in
    /// `ordrumbox_engine::synth_model::groups`.
    SynthPower(u8),
    SynthIcon(u8, u8),
    /// A control, by its index in `synth_view::all_fields`.
    SynthField(u16),
    // Piano roll.
    /// The track it shows.
    RollTrack,
    /// Its right-click menus.
    RollMenu,
    // Grid.
    TrackVolume(usize),
    /// A tab of the lane under the grid: an index in `editors::LANES`.
    Lane(u8),
    /// A track's rename entry.
    TrackName(usize),
    /// The grid's right-click menus.
    GridMenu,
    /// The instrument menus that name a track: the categories, then a
    /// category's instruments. `None`: a new track; `Some(t)`: track `t` renamed.
    TrackNameMenu(Option<usize>),
    /// The instrument menus of a kit's sample (its index): the categories,
    /// then a category's instruments.
    KitInstrumentMenu(usize),
    /// The instrument menus of the track editor's Sound tab: the categories,
    /// then a category's instruments, those that have samples only.
    InstrumentMenu,
    /// The right-click menu of a knob or a slider: the control it is for is
    /// the dropdown's `control`.
    ControlMenu,
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
    /// A tab of the track editor's main tab bar: a filled block, the active
    /// one lit, over a line that runs under the whole bar.
    MainTab {
        label: String,
        active: bool,
    },
    Button {
        label: String,
        on: bool,
    },
    /// A control greyed out: shown, not used (a slap's echo feedback).
    Disabled {
        label: String,
    },
    Text {
        text: String,
        color: Rgb,
    },
    /// A panel's title, at [`Scale::title`].
    Title {
        text: String,
    },
    /// A sample's envelope (peaks 0–1) and where its decay ends (0–1).
    Envelope {
        points: Vec<f64>,
        marker: Option<f64>,
    },
    /// A highlighted row background (the selected setting).
    Highlight,
    /// A panel's background (the track editor's column, under its cards).
    Panel,
    /// A card: a raised block with a border that groups one editor (the
    /// track's, the note's) apart from its neighbour.
    Card,
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

    pub fn text(x: i32, y: i32, text: impl Into<String>, color: Rgb, scale: Scale) -> Self {
        let text = text.into();
        let rect = Rect::new(x, y, text_width(&text, scale), 8 * scale);
        Self { id: None, rect, kind: Kind::Text { text, color } }
    }

    /// A panel's title at `(x, y)`, in a row of text at `scale`.
    pub fn title(x: i32, y: i32, text: impl Into<String>, scale: Scale) -> Self {
        let text = text.into();
        let rect = Rect::new(x, y, text_width(&text, scale.title()), 8 * scale.title());
        Self { id: None, rect, kind: Kind::Title { text } }
    }

    /// Takes the keyboard focus (Tab cycles through these).
    pub fn focusable(&self) -> bool {
        self.id.is_some()
            && !matches!(
                self.kind,
                Kind::Text { .. }
                    | Kind::Title { .. }
                    | Kind::Disabled { .. }
                    | Kind::Envelope { .. }
                    | Kind::Highlight
                    | Kind::Panel
                    | Kind::Card
            )
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
pub fn label_width(scale: Scale) -> i32 {
    88 * scale
}

/// Width of a slider row's value text.
fn value_width(scale: Scale) -> i32 {
    64 * scale
}

/// The bar of a slider item (with a label, with a value text): left edge and width.
pub fn slider_bar(rect: Rect, labelled: bool, valued: bool, scale: Scale) -> (i32, i32) {
    let left = rect.x + if labelled { label_width(scale) } else { 0 } + 4 * scale;
    let right = rect.x + rect.w - if valued { value_width(scale) + 6 * scale } else { 2 * scale };
    (left, (right - left).max(8 * scale))
}

/// The parts of a stepper item (with a label): its minus button, its value,
/// its plus button. Square buttons the row's height around a value as wide
/// as a slider's; the same width whatever the value.
pub fn stepper_parts(rect: Rect, labelled: bool, scale: Scale) -> [Rect; 3] {
    let left = rect.x + if labelled { label_width(scale) } else { 0 } + 4 * scale;
    let button = rect.h;
    let value = Rect::new(left + button, rect.y, value_width(scale), rect.h);
    [Rect::new(left, rect.y, button, rect.h), value, Rect::new(value.x + value.w, rect.y, button, rect.h)]
}

/// Diameter of a knob in a row of knobs (at scale 1).
pub const KNOB_SIZE: i32 = 26;

/// Diameter of a knob in an item of height `h`.
fn knob_size(rect: Rect, scale: Scale) -> i32 {
    if rect.h >= 40 * scale { KNOB_SIZE * scale } else { (rect.h - 2 * scale).max(8 * scale) }
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

/// A label centred in `r`, cut to its width so it never spills out.
fn centered_label(canvas: &mut Canvas, r: Rect, label: &str, scale: Scale, color: Rgb) {
    let label = fit(label, r.w - 2 * scale, scale);
    let x = r.x + (r.w - text_width(&label, scale)) / 2;
    canvas.text(x, r.y + (r.h - 8 * scale) / 2, &label, scale, color);
}

/// A knob: a 270° arc from bottom left, lit up to `fraction`, a disc and its pointer.
fn draw_knob(canvas: &mut Canvas, cx: i32, cy: i32, size: i32, fraction: f64, color: Rgb, scale: Scale) {
    let r = f64::from(size) / 2.0;
    let dot = scale.px() * if size >= 20 * scale { 2 } else { 1 };
    let start = 135f64.to_radians();
    let sweep = 270f64.to_radians();
    let steps = (r * 6.0) as i32 + 24;
    for i in 0..=steps {
        let f = f64::from(i) / f64::from(steps);
        let a = start + sweep * f;
        let x = cx + (a.cos() * (r - f64::from(dot))) as i32;
        let y = cy + (a.sin() * (r - f64::from(dot))) as i32;
        canvas.fill_rect(x - dot / 2, y - dot / 2, dot, dot, if f <= fraction { color } else { border_color() });
    }
    let disc = (r * 0.62) as i32;
    fill_circle(canvas, cx, cy, disc, disc_color());
    let a = start + sweep * fraction;
    for k in 0..=disc {
        let x = cx + (a.cos() * f64::from(k)) as i32;
        let y = cy + (a.sin() * f64::from(k)) as i32;
        canvas.fill_rect(x - scale.px() / 2, y - scale.px() / 2, scale.px(), scale.px(), theme::text());
    }
}

/// Draw `items`; `focus` gets an outline.
pub fn draw(canvas: &mut Canvas, items: &[Item], focus: Option<Id>, scale: Scale) {
    let t = scale.px();
    for item in items {
        let r = item.rect;
        let focused = item.id.is_some() && item.id == focus;
        match &item.kind {
            Kind::Highlight => canvas.fill_rect(r.x, r.y, r.w, r.h, selected_row_color()),
            Kind::Panel => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, theme::bg());
                canvas.fill_rect(r.x, r.y, 2 * t, r.h, theme::ACCENT);
            }
            Kind::Card => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
                canvas.fill_rect(r.x + t, r.y + t, r.w - 2 * t, r.h - 2 * t, background_color());
            }
            Kind::Title { text } => {
                canvas.text(r.x, r.y, text, scale.title(), TITLE);
            }
            Kind::Text { text, color } => {
                canvas.text(r.x, r.y, &fit(text, r.w.max(text_width(text, scale)), scale), scale, *color);
            }
            Kind::Value { spec, value, label, text, lfo } => {
                let fraction = spec.fraction(*value);
                let color = if *lfo { LFO } else { arc_color() };
                match spec.style {
                    Style::Knob => {
                        let size = knob_size(r, scale);
                        if r.h >= 40 * scale {
                            let cx = r.x + r.w / 2;
                            draw_knob(canvas, cx, r.y + size / 2, size, fraction, color, scale);
                            let mut y = r.y + size + 3 * scale;
                            if let Some(label) = label {
                                let label = fit(label, r.w, scale);
                                canvas.text(cx - text_width(&label, scale) / 2, y, &label, scale, dim());
                                y += 10 * scale;
                            }
                            let text = fit(text, r.w, scale);
                            canvas.text(cx - text_width(&text, scale) / 2, y, &text, scale, value_color());
                        } else {
                            draw_knob(canvas, r.x + size / 2, r.y + r.h / 2, size, fraction, color, scale);
                            let x = r.x + size + 6 * scale;
                            canvas.text(
                                x,
                                r.y + (r.h - 8 * scale) / 2,
                                &fit(text, r.x + r.w - x, scale),
                                scale,
                                value_color(),
                            );
                        }
                    }
                    Style::Slider => {
                        let ty = r.y + (r.h - 8 * scale) / 2;
                        if let Some(label) = label {
                            canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, dim());
                        }
                        let (left, width) = slider_bar(r, label.is_some(), !text.is_empty(), scale);
                        // A thin line, lit up to the value, and a square handle.
                        let bar_y = r.y + r.h / 2 - scale.px() / 2;
                        canvas.fill_rect(left, bar_y, width, 2 * t, border_color());
                        let filled = (fraction * f64::from(width)) as i32;
                        canvas.fill_rect(left, bar_y, filled, 2 * t, color);
                        let thumb = left + filled - 4 * scale;
                        canvas.fill_rect(thumb, r.y + r.h / 2 - 4 * scale, 8 * scale, 8 * scale, handle_color());
                        let x = left + width + 6 * scale;
                        canvas.text(x, ty, &fit(text, r.x + r.w - x, scale), scale, value_color());
                    }
                    Style::Stepper => {
                        let ty = r.y + (r.h - 8 * scale) / 2;
                        if let Some(label) = label {
                            canvas.text(r.x, ty, &fit(label, label_width(scale) - 4 * scale, scale), scale, dim());
                        }
                        let [down, field, up] = stepper_parts(r, label.is_some(), scale);
                        // A button is greyed out at its end of the range.
                        for (button, glyph, live) in [(down, "-", *value > spec.min), (up, "+", *value < spec.max)] {
                            canvas.fill_rect(
                                button.x + t,
                                button.y + t,
                                button.w - 2 * t,
                                button.h - 2 * t,
                                field_color(),
                            );
                            let x = button.x + (button.w - text_width(glyph, scale)) / 2;
                            canvas.text(x, ty, glyph, scale, if live { theme::text() } else { dim() });
                        }
                        let text = fit(text, field.w, scale);
                        canvas.text(
                            field.x + (field.w - text_width(&text, scale)) / 2,
                            ty,
                            &text,
                            scale,
                            value_color(),
                        );
                    }
                }
            }
            Kind::Select { labels, selected, .. } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
                let label = labels.get(*selected).map_or("", String::as_str);
                let ty = r.y + (r.h - 8 * scale) / 2;
                canvas.text(r.x + 4 * scale, ty, &fit(label, r.w - 16 * scale, scale), scale, theme::text());
                canvas.text(r.x + r.w - 10 * scale, ty, "v", scale, dim());
            }
            Kind::Toggle { on } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ON } else { field_color() });
                let text = if *on { "ON" } else { "OFF" };
                let color = if *on { background_color() } else { dim() };
                canvas.text(r.x + (r.w - text_width(text, scale)) / 2, r.y + (r.h - 8 * scale) / 2, text, scale, color);
            }
            Kind::Led { on } => {
                let s = 6 * scale;
                let (x, y) = (r.x + (r.w - s) / 2, r.y + (r.h - s) / 2);
                canvas.fill_rect(x, y, s, s, if *on { ON } else { border_color() });
            }
            Kind::Tab { label, active } => {
                if *active {
                    canvas.fill_rect(r.x, r.y + r.h - 2 * t, r.w, 2 * t, ACTIVE);
                }
                let color = if *active { ACTIVE } else { dim() };
                centered_label(canvas, r, label, scale, color);
            }
            Kind::MainTab { label, active } => {
                let (fill, color) = if *active { (ACTIVE, background_color()) } else { (field_color(), theme::text()) };
                canvas.fill_rect(r.x, r.y, r.w, r.h, fill);
                // The line under the bar, through the gap to the next tab.
                canvas.fill_rect(r.x, r.y + r.h - 2 * t, r.w + t, 2 * t, ACTIVE);
                centered_label(canvas, r, label, scale, color);
            }
            Kind::Button { label, on } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, if *on { ACTIVE } else { field_color() });
                let color = if *on { background_color() } else { theme::text() };
                centered_label(canvas, r, label, scale, color);
            }
            Kind::Disabled { label } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
                centered_label(canvas, r, label, scale, theme::disabled());
            }
            Kind::Envelope { points, marker } => {
                canvas.fill_rect(r.x, r.y, r.w, r.h, field_color());
                // The waveform, mirrored around the middle and scaled to its
                // own peak: a quiet sample shows its shape as well as a loud one.
                let peak = points.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                let gain = if peak > 0.0 { 1.0 / peak } else { 0.0 };
                let mid = r.y + r.h / 2;
                canvas.fill_rect(r.x, mid, r.w, t, theme::mix(field_color(), ENVELOPE, 0.4));
                let n = points.len() as i32;
                for (i, v) in points.iter().enumerate() {
                    let i = i as i32;
                    let x = r.x + i * r.w / n.max(1);
                    let w = ((i + 1) * r.w / n.max(1) - i * r.w / n.max(1)).max(1);
                    let half = ((v.abs() * gain).min(1.0) * f64::from(r.h / 2)).round() as i32;
                    if half > 0 {
                        canvas.fill_rect(x, mid - half, w, 2 * half, ENVELOPE);
                    }
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
    /// The menu this one is a submenu of, still shown, its line that opened
    /// this one highlighted.
    pub parent: Option<Box<Dropdown>>,
    /// The control a `ControlMenu` is for.
    pub control: Option<Id>,
}

/// The label end of a line that opens a submenu.
pub const SUBMENU_MARK: &str = " >";

impl Dropdown {
    pub fn new(id: Id, anchor: Rect, values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        let mut d =
            Self { id, title: None, anchor, values, labels, at: selected, first: 0, parent: None, control: None };
        d.show_at();
        d
    }

    /// A menu of actions at `(x, y)`, under `title`.
    pub fn menu(id: Id, x: i32, y: i32, title: String, values: Vec<String>, labels: Vec<String>) -> Self {
        Self { title: Some(title), ..Self::new(id, Rect::new(x, y, 0, 0), values, labels, 0) }
    }

    /// A menu opened by the highlighted line of `parent`, beside it (on its
    /// right, or on its left where the `width` × `height` window ends).
    pub fn submenu(
        parent: Dropdown,
        size: (i32, i32, Scale),
        id: Id,
        title: String,
        values: Vec<String>,
        labels: Vec<String>,
    ) -> Self {
        let (width, height, scale) = size;
        let mut menu = Self::menu(id, 0, 0, title, values, labels);
        let beside = parent.rect(width, height, scale);
        let w = menu.rect(width, height, scale).w;
        let row = parent.at.saturating_sub(parent.first) + parent.head();
        let y = beside.y + 2 * scale + row as i32 * row_height(scale) - 2 * scale;
        let right = beside.x + beside.w;
        let x = if right + w <= width { right } else { (beside.x - w).max(0) };
        menu.anchor = Rect::new(x, y, 0, 0);
        menu.parent = Some(Box::new(parent));
        menu
    }

    /// Whether the highlighted line opens a submenu.
    pub fn opens_submenu(&self) -> bool {
        self.labels.get(self.at).is_some_and(|l| l.ends_with(SUBMENU_MARK))
    }

    /// Back to the menu above, `None` when there is none.
    pub fn back(self) -> Option<Dropdown> {
        self.parent.map(|p| *p)
    }

    /// The menu above whose list is at `(x, y)` (the nearest first), and the
    /// option there: a click on it goes back to it.
    pub fn parent_at(self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> Option<(Dropdown, Option<usize>)> {
        let mut menu = self.back();
        while let Some(m) = menu {
            if m.rect(width, height, scale).contains(x, y) {
                let i = m.hit(width, height, scale, x, y);
                return Some((m, i));
            }
            menu = m.back();
        }
        None
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
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
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
    pub fn hit(&self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> Option<usize> {
        let r = self.rect(width, height, scale);
        if !r.contains(x, y) {
            return None;
        }
        let row = row_at(y, r.y + 2 * scale, row_height(scale))?.checked_sub(self.head())?;
        let i = self.first + row;
        (i < self.labels.len()).then_some(i)
    }

    /// The menus above first, then this one over them.
    pub fn draw(&self, canvas: &mut Canvas, scale: Scale) {
        if let Some(parent) = &self.parent {
            parent.draw(canvas, scale);
        }
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(r.x + scale.px(), r.y + scale.px(), r.w - 2 * scale, r.h - 2 * scale, background_color());
        let row = row_height(scale);
        if let Some(title) = &self.title {
            let y = r.y + 2 * scale + (row - 8 * scale) / 2;
            canvas.text(r.x + 6 * scale, y, &fit(title, r.w - 12 * scale, scale), scale, dim());
        }
        let top = r.y + 2 * scale + self.head() as i32 * row;
        for (k, label) in self.labels.iter().enumerate().skip(self.first).take(self.shown()) {
            let y = top + (k - self.first) as i32 * row;
            if k == self.at {
                canvas.fill_rect(r.x + scale.px(), y, r.w - 2 * scale, row, selected_row_color());
            }
            let color = if k == self.at { TITLE } else { theme::text() };
            canvas.text(r.x + 6 * scale, y + (row - 8 * scale) / 2, &fit(label, r.w - 12 * scale, scale), scale, color);
        }
        if self.labels.len() > self.shown() {
            // Scroll bar.
            let total = self.labels.len() as i32;
            let track = r.h - 4 * scale - self.head() as i32 * row;
            let top = top + track * self.first as i32 / total;
            let len = (track * self.shown() as i32 / total).max(4 * scale);
            canvas.fill_rect(r.x + r.w - 4 * scale, top, 2 * scale, len, dim());
        }
    }
}

fn row_height(scale: Scale) -> i32 {
    12 * scale
}

// ─── Number entry ────────────────────────────────────────────────────────────

/// Where a text entry's field is in it (unscaled): the mouse that opened it is there.
const ENTRY_FIELD: (i32, i32) = (12, 22);

/// The number typed for a control (`promptNumericInput`, right-click or
/// Enter), or a text (a name, a description: `spec` is `None`).
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub id: Id,
    pub spec: Option<Spec>,
    pub title: String,
    pub text: String,
    /// The mouse when it opened: its field starts there.
    pub at: (i32, i32),
}

impl Entry {
    /// The entry with its field at the mouse, inside the `width` × `height` window.
    pub fn rect(&self, width: i32, height: i32, scale: Scale) -> Rect {
        let w = (text_width(&self.title, scale) + 32 * scale).max(220 * scale).min(width);
        let h = 52 * scale;
        let (x, y) = (self.at.0 - ENTRY_FIELD.0 * scale, self.at.1 - ENTRY_FIELD.1 * scale);
        Rect::new(x.clamp(0, (width - w).max(0)), y.clamp(0, (height - h).max(0)), w, h)
    }

    pub fn draw(&self, canvas: &mut Canvas, scale: Scale) {
        let r = self.rect(canvas.width(), canvas.height(), scale);
        canvas.fill_rect(r.x - 4 * scale, r.y - 4 * scale, r.w + 8 * scale, r.h + 8 * scale, theme::shade());
        canvas.fill_rect(r.x, r.y, r.w, r.h, border_color());
        canvas.fill_rect(r.x + scale.px(), r.y + scale.px(), r.w - 2 * scale, r.h - 2 * scale, background_color());
        canvas.text(r.x + 12 * scale, r.y + 8 * scale, &self.title, scale, TITLE);
        let field = Rect::new(r.x + ENTRY_FIELD.0 * scale, r.y + ENTRY_FIELD.1 * scale, r.w - 24 * scale, 12 * scale);
        canvas.fill_rect(field.x, field.y, field.w, field.h, field_color());
        let x = canvas.text(field.x + 4 * scale, field.y + 2 * scale, &self.text, scale, theme::text());
        canvas.fill_rect(x + scale.px(), field.y + 2 * scale, scale.px(), 8 * scale, CARET);
        canvas.text(r.x + 12 * scale, r.y + 40 * scale, "ENTER: set   ESC: cancel", scale, dim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_start_at_the_top() {
        assert_eq!(row_at(9, 10, 16), None);
        assert_eq!(row_at(10, 10, 16), Some(0));
        assert_eq!(row_at(26, 10, 16), Some(1));
    }

    #[test]
    fn focus_cycles_over_focusable_items() {
        let r = Rect::new(0, 0, 10, 10);
        let items = vec![
            Item::new(Some(Id::Tab(0)), r, Kind::Tab { label: "FX".into(), active: true }),
            Item::new(None, r, Kind::Card),
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
        let r = d.rect(800, 600, Scale::ONE);
        assert_eq!(d.hit(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 12 * 3 + 1), Some(3));
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
        let r = m.rect(800, 600, Scale::ONE);
        assert_eq!(m.hit(800, 600, Scale::ONE, r.x + 5, r.y + 3), None);
        assert_eq!(m.hit(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 12 + 1), Some(0));
        assert_eq!(m.hit(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 24 + 1), Some(1));
    }

    #[test]
    fn draws_every_kind() {
        let mut pixels = vec![0; 400 * 300];
        let mut canvas = Canvas::new(&mut pixels, 400, 300);
        let spec = Spec::knob(0.0, 1.0, 0.01, 0.0);
        let items = vec![
            Item::new(
                Some(Id::KnobBar("volume")),
                Rect::new(0, 0, 60, 50),
                Kind::Value { spec, value: 0.5, label: Some("Vol".into()), text: "50 %".into(), lfo: false },
            ),
            Item::new(
                Some(Id::Fx("reverbMix")),
                Rect::new(0, 60, 160, 16),
                Kind::Value { spec, value: 0.2, label: None, text: "0.2".into(), lfo: true },
            ),
            Item::new(
                Some(Id::Generation("hitVariation")),
                Rect::new(0, 80, 300, 16),
                Kind::Value {
                    spec: Spec::slider(0.0, 100.0, 1.0, 0.0),
                    value: 30.0,
                    label: Some("Hit Variation".into()),
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
        draw(&mut canvas, &items, Some(Id::Sample), Scale::ONE);
        Dropdown::new(Id::Sample, Rect::new(390, 290, 20, 12), vec!["x".into(); 3], vec!["x".into(); 3], 0)
            .draw(&mut canvas, Scale::ONE);
        let spec = Spec::knob(20.0, 5000.0, 10.0, 500.0);
        Entry { id: Id::Decay, spec: Some(spec), title: "Enter value".into(), text: "12".into(), at: (390, 290) }
            .draw(&mut canvas, Scale::ONE);
        Dropdown::menu(Id::SongMenu, 390, 290, "rock".into(), vec!["a".into()], vec!["Add".into()])
            .draw(&mut canvas, Scale::ONE);
    }
}
