//! The widgets of the editors (track editor, note editor, kit editor, the
//! grid's volume sliders, the song panel's column): a flat list of [`Item`]s, each with its place, an
//! [`Id`] when it reacts to the mouse or the keyboard, and what it shows.
//! The list is rebuilt from the state at every frame; drawing and
//! finding what is under the mouse both use it. The open drop-down list and the number entry
//! are drawn over everything.

use crate::canvas::{Canvas, Direction, Rgb, char_width, fit, text_width};
use crate::control::{Spec, Style};
use crate::menu::Command;
use crate::sample_overlay::Overlay;
use crate::scale::Scale;
use crate::theme;
use ordrumbox_engine::kit::edit::SampleField;

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

/// The row of a list under `y`, rows `row_height` high from `top`; `None`
/// above the list (a division alone would put the pixels just above in row 0).
pub fn row_at(y: i32, top: i32, row_height: i32) -> Option<usize> {
    if y < top || row_height <= 0 {
        return None;
    }
    usize::try_from((y - top) / row_height).ok()
}

/// The first index after `from`, wrapping round the `count` of a list, that
/// satisfies `matches` (a typed letter jumps to the next name that starts with it).
pub fn next_matching(count: usize, from: usize, matches: impl Fn(usize) -> bool) -> Option<usize> {
    (1..=count).map(|k| (from + k) % count).find(|&i| matches(i))
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

    /// The same rectangle, `dx` to the right and `dy` down.
    #[must_use]
    pub const fn moved(self, dx: i32, dy: i32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
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

/// Which way an arrow goes through a list: to the item before, or after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Step {
    Previous,
    Next,
}

impl Step {
    /// The index one step from `from` in a list of `count`, wrapping round
    /// (`None` in an empty list).
    #[must_use]
    pub fn target_index(self, from: usize, count: usize) -> Option<usize> {
        let last = count.checked_sub(1)?;
        Some(match self {
            Self::Previous => from.min(last).checked_sub(1).unwrap_or(last),
            Self::Next if from >= last => 0,
            Self::Next => from + 1,
        })
    }
}

/// What a widget is (and does).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Id {
    // Track editor.
    /// A main tab of the track editor.
    Tab(EditorTab),
    /// An effect's on / off button, named after it.
    FxPower(u8),
    /// A sub-tab of the Generate tab.
    GenerationTab(GenerationTab),
    /// A compose setting of a drum track (its style's value when unset).
    Compose(&'static str),
    /// ♫: a new line on the edited track.
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
    /// An arrow of an LFO's wave screen.
    LfoTypeStep(u8, Step),
    /// A select of a track setting (effect types, delay time, variant).
    TrackSelect(&'static str),
    /// An arrow of a track setting's screen (an effect's type).
    TrackSelectStep(&'static str, Step),
    /// An ON/OFF button of a track setting (mono, auto, scale lock).
    Toggle(&'static str),
    /// The sound tab's auto-assign LED.
    AutoAssign,
    Instrument,
    Sample,
    /// The sound tab's synth cartridge: its screen (the preset), its LED (the
    /// synth on or off), its arrows (the preset before, the one after).
    Synth,
    SynthOn,
    SynthStep(Step),
    /// The sample bar's load button.
    LoadSample,
    /// A row's label: selects the row.
    Row(&'static str),
    // Note editor.
    NoteKnob(&'static str),
    NoteFader(&'static str),
    /// A spin of whole numbers (a cycle, a count, the arpeggio's range).
    NoteSpin(&'static str),
    /// Arp scale / direction spin.
    NoteSelect(&'static str),
    // Kit editor.
    /// The kit to move the selected sample to.
    KitKit,
    /// The instrument of a sample, by its index in the kit (its table row).
    KitSampleInstrument(usize),
    /// The auto-assign button of a sample's row: its instrument from its file name.
    KitAutoInstrument(usize),
    KitKnob(SampleField),
    /// A sample's reverse or loop switch in the kit editor.
    KitToggle(SampleField),
    /// The same settings in the track editor, on the track's sample.
    SampleKnob(SampleField),
    SampleToggle(SampleField),
    /// The kit's title or description, by its `kits.json` key.
    KitText(&'static str),
    /// The title of the table's column of tracks playing each sample.
    KitPlayedBy,
    /// The sample's File measurement: its hover shows the full path.
    KitSampleFile,
    // Header.
    PatternList,
    KitList,
    /// Play / stop, the tempo knob, the beats list, the generation
    /// buttons (drums, bass, chords).
    Transport,
    /// REC: the mixer writes the song's automation while it plays.
    RecordAutomation,
    Bpm,
    Beats,
    /// The style list, ♫ (the pattern made again in it), AUTO (every track in
    /// auto mode), and a generation macro of the pattern.
    StyleList,
    GeneratePattern,
    AutoAll,
    Macro(&'static str),
    /// A button that runs a command of the menus (the pattern's new,
    /// duplicate, rename…, quantize…, the drum, bass and chord lines).
    Command(Command),
    /// The scale and the key the pattern's lines are generated in.
    PatternScale,
    PatternKey,
    /// A view button (the song's slot has no button).
    View(ViewButton),
    /// The mode switch: 0 the pattern, 1 the song.
    Mode(u8),
    // Song panel.
    /// The song select.
    SongList,
    /// Save, load, export, import, new, delete.
    SongButton(&'static str),
    ProjectName,
    ProjectDescription,
    /// A pattern of the song panel's list (its rename entry).
    SongPattern(usize),
    /// The selected pattern's description entry.
    PatternDescription,
    /// The selected pattern's tags, on the song screen (their entry).
    PatternTags,
    /// The selected pattern's name, description and tags, in one form.
    PatternInfo,
    /// The header's PATTERN list, opened as styles and their patterns.
    PatternMenu,
    /// The right-click menu of the song panel.
    SongMenu,
    // Mixer.
    /// A master bus setting, by its `settings.master` key.
    Master(&'static str),
    /// A setting of the shared returns, by its `settings.returns` key.
    Returns(&'static str),
    /// A track strip's name: the track editor edits that track.
    MixerName(usize),
    /// A track strip's knob (a send, the pan), by its track key.
    MixerKnob(usize, &'static str),
    /// A track strip's mute or solo button, by its track key.
    MixerToggle(usize, &'static str),
    /// An arrow of the scroll row: the track strips before, after.
    // Synth editor.
    SynthPreset,
    /// The menus that choose a synth preset: its instruments and styles,
    /// then a group's presets; the select they are for is the dropdown's `control`.
    SynthPresetMenu,
    /// The preset edited's tags (their entry).
    SynthTags,
    SynthButton(&'static str),
    /// A card's power button: by the card's index in
    /// `ordrumbox_engine::synth_model::groups`.
    SynthPower(u8),
    /// A control, by its index in `synth_view::all_fields`.
    SynthField(u16),
    // Piano roll.
    /// The track it shows.
    RollTrack,
    /// Its right-click menus.
    RollMenu,
    // Grid.
    TrackVolume(usize),
    /// The right-click menu of the lane under the grid.
    LaneMenu,
    /// The list that picks the lane under the grid: a value is an index in
    /// `editors::LANES`.
    LaneList,
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
    /// The song panel's automation menus: the instruments of the song, then
    /// the controls of one; or a lane's menu.
    AutomationMenu,
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
    /// A two-state button: its two options side by side, the selected one
    /// lit; a click picks the half under it.
    TwoState {
        values: [String; 2],
        labels: [String; 2],
        selected: usize,
    },
    /// A one-line display of a choice (the synth cartridge's screen): it
    /// opens its list as a [`Kind::Select`] does; dimmed when `lit` is off.
    Screen {
        values: Vec<String>,
        labels: Vec<String>,
        selected: usize,
        lit: bool,
    },
    /// A choice on a screen with ▲ and ▼ beside it, which step through it;
    /// a click on the screen opens its list. `curve` (values from -1 to 1
    /// across) is drawn on the screen in place of the label when it has any.
    Spin {
        values: Vec<String>,
        labels: Vec<String>,
        selected: usize,
        curve: Vec<f64>,
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
    /// The transport's button: PLAY, or STOP while `playing`, in a colour
    /// of its own (green) apart from the other buttons.
    Transport {
        playing: bool,
    },
    /// A control greyed out: shown, not used (a slap's echo feedback), in
    /// the style of the control it stands for (a knob or a fader).
    Disabled {
        label: String,
        style: Style,
    },
    Text {
        text: String,
        color: Rgb,
    },
    /// A panel's title, at [`Scale::title`].
    Title {
        text: String,
    },
    /// A sample's waveform (peaks 0–1) and how it plays over it.
    Envelope {
        points: Vec<f64>,
        overlay: Option<Overlay>,
    },
    /// A highlighted row background (the selected setting).
    Highlight,
    /// A panel's background (the track editor's column, under its cards).
    Panel,
    /// A card: a raised block with a border that groups one editor (the
    /// track's, the note's) apart from its neighbour.
    Card,
    /// A small square over a control's corner: a state of the control (taken
    /// back from its automation lane), in `color`.
    Mark {
        color: Rgb,
    },
}

/// The transport button's label: PLAY, or STOP while playing.
#[must_use]
pub const fn transport_label(playing: bool) -> &'static str {
    if playing { "STOP" } else { "PLAY" }
}

impl Kind {
    /// A choice as a [`Kind::Spin`] showing its label.
    #[must_use]
    pub fn spin(values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        Self::Spin { values, labels, selected, curve: Vec::new() }
    }

    /// A choice of two options as a [`Kind::TwoState`]; any other count of
    /// options as a [`Kind::Select`].
    #[must_use]
    pub fn two_state(values: Vec<String>, labels: Vec<String>, selected: usize) -> Self {
        match (<[String; 2]>::try_from(values), <[String; 2]>::try_from(labels)) {
            (Ok(values), Ok(labels)) => Self::TwoState { values, labels, selected },
            (values, labels) => Self::Select {
                values: values.map_or_else(|v| v, Vec::from),
                labels: labels.map_or_else(|l| l, Vec::from),
                selected,
            },
        }
    }
}

/// How an item is drawn: in the app's flat look, or in the synth editor's
/// hardware panel (`panel/hardware.rs`), on one of its cards or on the
/// metal around them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Skin {
    #[default]
    Flat,
    Card,
    Metal,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: Option<Id>,
    pub rect: Rect,
    pub kind: Kind,
    pub skin: Skin,
}

impl Item {
    pub fn new(id: Option<Id>, rect: Rect, kind: Kind) -> Self {
        Self { id, rect, kind, skin: Skin::Flat }
    }

    pub fn text(x: i32, y: i32, text: impl Into<String>, color: Rgb, scale: Scale) -> Self {
        let text = text.into();
        let rect = Rect::new(x, y, text_width(&text, scale), 8 * scale);
        Self::new(None, rect, Kind::Text { text, color })
    }

    /// A panel's title at `(x, y)`, in a row of text at `scale`.
    pub fn title(x: i32, y: i32, text: impl Into<String>, scale: Scale) -> Self {
        let text = text.into();
        let rect = Rect::new(x, y, text_width(&text, scale.title()), 8 * scale.title());
        Self::new(None, rect, Kind::Title { text })
    }

    /// The item drawn in `skin`.
    #[must_use]
    pub fn skinned(self, skin: Skin) -> Self {
        Self { skin, ..self }
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
                    | Kind::Mark { .. }
            )
            && !matches!(self.id, Some(Id::Row(_)))
    }
}

/// The topmost item with an id under `(x, y)`.
pub fn item_at(items: &[Item], x: i32, y: i32) -> Option<&Item> {
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

/// The groove of a fader item: its top and its height, over the value text
/// at the item's foot.
pub fn fader_bar(rect: Rect, scale: Scale) -> (i32, i32) {
    let top = rect.y + 6 * scale;
    (top, (rect.y + rect.h - 18 * scale - top).max(8 * scale))
}

/// The parts of a stepper item (with a label): its minus button, its value,
/// its plus button. Square buttons the row's height around a value as wide
/// as a slider's; the same width whatever the value.
/// The two halves of a [`Kind::TwoState`] button.
#[must_use]
pub fn two_state_halves(rect: Rect) -> [Rect; 2] {
    let left = rect.w / 2;
    [Rect::new(rect.x, rect.y, left, rect.h), Rect::new(rect.x + left, rect.y, rect.w - left, rect.h)]
}

/// Which half of a [`Kind::TwoState`] button `x` is over.
#[must_use]
pub fn two_state_half_at(rect: Rect, x: i32) -> usize {
    usize::from(x >= two_state_halves(rect)[1].x)
}

pub fn stepper_parts(rect: Rect, labelled: bool, scale: Scale) -> [Rect; 3] {
    let left = rect.x + if labelled { label_width(scale) } else { 0 } + 4 * scale;
    let button = rect.h;
    let value = Rect::new(left + button, rect.y, value_width(scale), rect.h);
    [Rect::new(left, rect.y, button, rect.h), value, Rect::new(value.x + value.w, rect.y, button, rect.h)]
}

/// The height of a spin's screen (at scale 1).
pub const SPIN_SCREEN_H: i32 = 20;
/// The width of a spin's ▲ and ▼ (at scale 1).
const SPIN_ARROW_W: i32 = 12;

/// The width of a [`Kind::Spin`] whose screen holds `chars` characters,
/// its ▲ and ▼ included.
#[must_use]
pub fn spin_width(chars: i32, scale: Scale) -> i32 {
    chars * char_width(scale) + 8 * scale + SPIN_ARROW_W * scale
}

/// The parts of a [`Kind::Spin`]: its ▼, its screen, its ▲. The screen at
/// the left of `rect` (at most [`SPIN_SCREEN_H`] high), the ▲ over the ▼
/// on its right.
#[must_use]
pub fn spin_parts(rect: Rect, scale: Scale) -> [Rect; 3] {
    let (arrow_w, screen_h) = (SPIN_ARROW_W * scale, rect.h.min(SPIN_SCREEN_H * scale));
    let screen = Rect::new(rect.x, rect.y, (rect.w - arrow_w).max(0), screen_h);
    let x = screen.x + screen.w;
    let half = screen_h / 2;
    [Rect::new(x, rect.y + half, arrow_w, screen_h - half), screen, Rect::new(x, rect.y, arrow_w, half)]
}

/// Diameter of a knob in a row of knobs (at scale 1).
pub const KNOB_SIZE: i32 = 26;

/// Diameter of a knob in an item of height `h`.
fn knob_size(rect: Rect, scale: Scale) -> i32 {
    if rect.h >= 40 * scale { KNOB_SIZE * scale } else { (rect.h - 2 * scale).max(8 * scale) }
}

/// Rectangle outline, `t` wide, inside `r`.
pub fn stroke(canvas: &mut Canvas, r: Rect, t: i32, color: Rgb) {
    canvas.fill_rect(r.x, r.y, r.w, t, color);
    canvas.fill_rect(r.x, r.y + r.h - t, r.w, t, color);
    canvas.fill_rect(r.x, r.y, t, r.h, color);
    canvas.fill_rect(r.x + r.w - t, r.y, t, r.h, color);
}

#[path = "panel/draw.rs"]
mod draw;
#[path = "panel/dropdown.rs"]
mod dropdown;
#[path = "panel/entry.rs"]
mod entry;
#[path = "panel/hardware.rs"]
pub(crate) mod hardware;
#[path = "panel/tabs.rs"]
mod tabs;
pub use draw::draw;
pub use dropdown::{Choices, Dropdown, MENU_MARK, SUBMENU_MARK};
pub use entry::{Entry, Field};
pub use tabs::{EditorTab, GenerationTab, ViewButton};

#[cfg(test)]
mod tests {
    use super::dropdown::LIST_ROWS;
    use super::*;
    use crate::menu_action::LoopAction;

    #[test]
    fn a_step_wraps_round_the_list() {
        assert_eq!(Step::Next.target_index(0, 3), Some(1));
        assert_eq!(Step::Next.target_index(2, 3), Some(0));
        assert_eq!(Step::Previous.target_index(0, 3), Some(2));
        assert_eq!(Step::Previous.target_index(2, 3), Some(1));
        assert_eq!(Step::Previous.target_index(9, 3), Some(1));
        assert_eq!(Step::Next.target_index(0, 1), Some(0));
        assert_eq!(Step::Next.target_index(0, 0), None);
    }

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
            Item::new(Some(Id::Tab(EditorTab::Fx)), r, Kind::Tab { label: "FX".into(), active: true }),
            Item::new(None, r, Kind::Card),
            Item::new(Some(Id::Row("x")), r, Kind::Highlight),
            Item::new(Some(Id::Toggle("mono")), r, Kind::Toggle { on: false }),
        ];
        assert_eq!(next_focus(&items, None, 1), Some(Id::Tab(EditorTab::Fx)));
        assert_eq!(next_focus(&items, Some(Id::Tab(EditorTab::Fx)), 1), Some(Id::Toggle("mono")));
        assert_eq!(next_focus(&items, Some(Id::Tab(EditorTab::Fx)), -1), Some(Id::Toggle("mono")));
        assert_eq!(item_at(&items, 5, 5).and_then(|i| i.id), Some(Id::Toggle("mono")));
    }

    #[test]
    fn dropdown_scrolls_and_finds_options() {
        let labels: Vec<String> = (0..40).map(|i| format!("item{i}")).collect();
        let mut d = Dropdown::new(Id::Sample, Rect::new(10, 10, 100, 12), labels.clone(), labels, 30);
        assert!(d.first <= 30 && 30 < d.first + LIST_ROWS);
        d.step(100);
        assert_eq!(d.at, 39);
        d.scroll(-100);
        assert_eq!(d.first, 0);
        assert_eq!(d.at, LIST_ROWS - 1);
        let r = d.rect(800, 600, Scale::ONE);
        assert_eq!(d.option_at(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 12 * 3 + 1), Some(3));
        d.jump('i');
        assert_eq!(d.at, LIST_ROWS);
        // A menu: its title row takes no click.
        let lines = [LoopAction::Toggle, LoopAction::Clear].map(|a| (a, format!("{a:?}")));
        let m = Dropdown::menu(Id::SongMenu, 10, 10, "title".into(), lines);
        let r = m.rect(800, 600, Scale::ONE);
        assert_eq!(m.option_at(800, 600, Scale::ONE, r.x + 5, r.y + 3), None);
        assert_eq!(m.option_at(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 12 + 1), Some(0));
        assert_eq!(m.option_at(800, 600, Scale::ONE, r.x + 5, r.y + 2 + 24 + 1), Some(1));
    }

    #[test]
    fn a_long_list_has_a_bar_whose_thumb_scrolls_it() {
        let labels: Vec<String> = (0..40).map(|i| format!("item{i}")).collect();
        let mut d = Dropdown::new(Id::PatternList, Rect::new(10, 10, 100, 12), labels.clone(), labels, 0);
        let bar = d.scroll_bar(800, 600, Scale::ONE).expect("40 options: a bar");
        assert_eq!(bar.w, 8);
        let r = d.rect(800, 600, Scale::ONE);
        assert_eq!(bar.x + bar.w, r.x + r.w - 1, "at the right of the rows");
        assert_eq!(d.option_at(800, 600, Scale::ONE, bar.x + 2, bar.y + 30), None, "the bar picks no option");
        assert!(d.bar_to((800, 600, Scale::ONE), bar.y + bar.h));
        assert_eq!(d.first, 40 - LIST_ROWS, "the thumb at the bottom: the last options");
        assert!((d.first..d.first + LIST_ROWS).contains(&d.at), "the highlight kept in view");
        let (start, len) = d.thumb(bar, Scale::ONE);
        assert_eq!(start + len, bar.y + bar.h);
        let short = Dropdown::new(Id::KitList, Rect::new(10, 10, 100, 12), vec!["x".into(); 3], vec!["x".into(); 3], 0);
        assert_eq!(short.scroll_bar(800, 600, Scale::ONE), None);
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
                    label: Some("Spot Variation".into()),
                    text: "30".into(),
                    lfo: false,
                },
            ),
            Item::new(
                Some(Id::Sample),
                Rect::new(0, 100, 120, 12),
                Kind::Select { values: vec!["a".into()], labels: vec!["a".into()], selected: 0 },
            ),
            Item::new(
                Some(Id::Synth),
                Rect::new(130, 100, 120, 12),
                Kind::Screen { values: vec!["bass".into()], labels: vec!["bass".into()], selected: 0, lit: true },
            ),
            Item::new(None, Rect::new(0, 120, 300, 40), Kind::Envelope { points: vec![0.0, 1.0], overlay: None }),
        ];
        draw(&mut canvas, &items, Some(Id::Sample), Scale::ONE);
        Dropdown::new(Id::Sample, Rect::new(390, 290, 20, 12), vec!["x".into(); 3], vec!["x".into(); 3], 0)
            .draw(&mut canvas, Scale::ONE);
        let spec = Spec::knob(20.0, 5000.0, 10.0, 500.0);
        Entry::new(Id::Decay, Some(spec), "Enter value".into(), "12".into(), (390, 290)).draw(&mut canvas, Scale::ONE);
        Dropdown::menu(Id::SongMenu, 390, 290, "Loop".into(), [(LoopAction::Clear, "Clear".into())])
            .draw(&mut canvas, Scale::ONE);
    }
}
