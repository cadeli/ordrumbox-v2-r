//! What the window shows around the library, and nothing else reads: the
//! open menu and overlay, the widgets the mouse and keyboard hold, the grid
//! page, the panels shown, the VU levels. Undo, saving and playback never
//! touch it.

use crate::scale::Scale;
use std::path::PathBuf;

use super::session_overlay::Overlay;
use super::{menu, session_grid, session_lane, session_zone, session_zoom};
use crate::panel::{Dropdown, Entry, Id};
use std::time::Instant;

/// A drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Drag {
    pub(super) id: Id,
    pub(super) spec: crate::control::Spec,
    pub(super) start: f64,
    pub(super) x: i32,
    pub(super) y: i32,
    /// A slider's bar (left, width), or a fader's groove (top, height),
    /// when it follows the mouse.
    pub(super) bar: Option<(i32, i32)>,
    /// A slider dragged finely: from where.
    pub(super) fine: bool,
}

/// What the mouse and the keyboard hold among the widgets of every panel.
#[derive(Debug, Default)]
pub(super) struct Widgets {
    /// The widget with the keyboard focus.
    pub(super) focus: Option<Id>,
    /// A knob or slider being dragged.
    pub(super) drag: Option<Drag>,
    /// The drop-down list or the text entry open over the panels.
    popup: Option<Popup>,
    /// The mouse, at its last move or press: where a text entry opens.
    pub(super) pointer: (i32, i32),
    /// Last click on a widget, for double-clicks.
    pub(super) last_click: Option<(Id, Instant)>,
    /// A control's value copied from its right-click menu, to paste on
    /// another whose range holds it.
    pub(super) copied_value: Option<f64>,
}

/// What opens over the panels, one at a time: a drop-down list (a menu
/// among them) or a text entry.
#[derive(Debug)]
enum Popup {
    Dropdown(Dropdown),
    Entry(Entry),
}

impl Widgets {
    /// The drop-down list open.
    pub(super) const fn dropdown(&self) -> Option<&Dropdown> {
        match &self.popup {
            Some(Popup::Dropdown(dropdown)) => Some(dropdown),
            _ => None,
        }
    }

    pub(super) const fn dropdown_mut(&mut self) -> Option<&mut Dropdown> {
        match &mut self.popup {
            Some(Popup::Dropdown(dropdown)) => Some(dropdown),
            _ => None,
        }
    }

    /// The text entry open.
    pub(super) const fn entry(&self) -> Option<&Entry> {
        match &self.popup {
            Some(Popup::Entry(entry)) => Some(entry),
            _ => None,
        }
    }

    pub(super) const fn entry_mut(&mut self) -> Option<&mut Entry> {
        match &mut self.popup {
            Some(Popup::Entry(entry)) => Some(entry),
            _ => None,
        }
    }

    /// `dropdown` open, in place of any other popup.
    pub(super) fn open_dropdown(&mut self, dropdown: Dropdown) {
        self.popup = Some(Popup::Dropdown(dropdown));
    }

    /// `entry` open, in place of any other popup.
    pub(super) fn open_entry(&mut self, entry: Entry) {
        self.popup = Some(Popup::Entry(entry));
    }

    /// The drop-down list, closed; an entry stays open.
    pub(super) fn take_dropdown(&mut self) -> Option<Dropdown> {
        match self.popup.take() {
            Some(Popup::Dropdown(dropdown)) => Some(dropdown),
            other => {
                self.popup = other;
                None
            }
        }
    }

    /// The text entry, closed; a drop-down list stays open.
    pub(super) fn take_entry(&mut self) -> Option<Entry> {
        match self.popup.take() {
            Some(Popup::Entry(entry)) => Some(entry),
            other => {
                self.popup = other;
                None
            }
        }
    }

    /// The drop-down list closed; an entry stays open.
    pub(super) fn close_dropdown(&mut self) {
        if matches!(self.popup, Some(Popup::Dropdown(_))) {
            self.popup = None;
        }
    }

    /// The text entry closed; a drop-down list stays open.
    pub(super) fn close_entry(&mut self) {
        if matches!(self.popup, Some(Popup::Entry(_))) {
            self.popup = None;
        }
    }

    /// Back to the menu above the one open, or none (Escape, Left).
    pub(super) fn dropdown_back(&mut self) {
        if let Some(above) = self.take_dropdown().and_then(Dropdown::back) {
            self.open_dropdown(above);
        }
    }

    /// A drop-down list or an entry is open.
    pub(super) const fn is_popup_open(&self) -> bool {
        self.popup.is_some()
    }

    /// No drop-down list or entry open.
    pub(super) fn close_popup(&mut self) {
        self.popup = None;
    }
}

/// What the mouse holds on the grid, from a press to its release: one
/// thing at a time.
#[derive(Debug, Clone, Copy)]
pub(super) enum GridHold {
    /// A track's name, that may be dragged to another row.
    Track(session_grid::TrackDrag),
    /// A step Shift+pressed, that may be dragged into a zone.
    Zone(session_zone::ZoneDrag),
    /// The lane under the grid, pressed and painted.
    Lane(session_lane::LaneDrag),
    /// The lane's top edge: where, and how tall the lane was.
    LaneResize(session_lane::LaneResize),
    /// The grid's scroll bar.
    Bar(crate::view::BarHold),
}

/// Which panels the window shows around the main view.
#[derive(Debug, Clone, Copy)]
pub(super) struct Panes {
    /// The track editor, on the right.
    track_editor: bool,
    /// The note editor, under the grid and the piano roll.
    note_editor: bool,
    /// The VU meter stripe.
    vus: bool,
}

impl Default for Panes {
    fn default() -> Self {
        Self { track_editor: true, note_editor: true, vus: true }
    }
}

impl Panes {
    pub(super) const fn track_editor(self) -> bool {
        self.track_editor
    }

    pub(super) const fn note_editor(self) -> bool {
        self.note_editor
    }

    pub(super) const fn vus(self) -> bool {
        self.vus
    }

    pub(super) const fn toggle_track_editor(&mut self) {
        self.track_editor = !self.track_editor;
    }

    pub(super) const fn toggle_note_editor(&mut self) {
        self.note_editor = !self.note_editor;
    }

    pub(super) const fn toggle_vus(&mut self) {
        self.vus = !self.vus;
    }

    pub(super) const fn show_track_editor(&mut self) {
        self.track_editor = true;
    }

    /// The compact view: the grid alone, the track editor and the note
    /// editor folded away.
    pub(super) const fn fold_editors(&mut self) {
        self.track_editor = false;
        self.note_editor = false;
    }

    /// The simple view, folded; again, both editors shown.
    const fn toggle_simple_view(&mut self) {
        if self.track_editor || self.note_editor {
            self.fold_editors();
        } else {
            self.track_editor = true;
            self.note_editor = true;
        }
    }
}

/// How fast a VU level falls, per frame drawn.
const LEVEL_DECAY: f32 = 0.88;
/// A VU level below this is silence.
const LEVEL_FLOOR: f32 = 0.001;

/// The session's presentation state.
#[derive(Debug)]
pub(super) struct Chrome {
    /// The open menu, if any.
    pub(super) menu: Option<menu::Open>,
    /// A panel over everything: the file picker, the key list, about.
    pub(super) overlay: Option<Overlay>,
    /// Folder the file picker last picked in.
    pub(super) last_dir: Option<PathBuf>,
    /// The loops of the pattern File > Export pattern as WAV or as MIDI writes, from 1.
    pub(super) export_loop_count: u32,
    /// What the mouse and the keyboard hold among the widgets.
    pub(super) widgets: Widgets,
    /// What of the grid is under the mouse, for its tooltip: a step (its
    /// note's) or a track's name (the track's).
    pub(super) hover: Option<crate::view::Hit>,
    /// What the help line says for what is under the pointer (`session_hints.rs`); `None`: the view's keys.
    pub(super) hint: Option<&'static str>,
    /// The control of a panel under the mouse that has a tooltip, and where it is.
    pub(super) control_hover: Option<(crate::panel::Id, crate::panel::Rect)>,
    /// What the mouse holds on the grid.
    pub(super) grid_hold: Option<GridHold>,
    /// The last press on a track's name and when: a second one soon after opens the piano roll.
    pub(super) last_name_press: Option<(usize, std::time::Instant)>,
    /// The lane's height set by dragging its top edge, in scale units;
    /// `None`: automatic.
    pub(super) lane_height: Option<i32>,
    /// Window size and scale of the last mouse or key event on the widgets.
    pub(super) panel_size: (i32, i32, Scale),
    /// The beats of the pattern the grid shows.
    pub(super) beat_window: crate::view::BeatWindow,
    /// The panels shown around the main view.
    pub(super) panes: Panes,
    /// The zoom of the UI (`session_zoom.rs`).
    pub(super) zoom: session_zoom::Zoom,
    /// The colour scheme in use, from 1, set again on the thread that draws.
    pub(super) color_scheme: usize,
    /// Smoothed per-track levels for the VU meter stripe.
    track_levels: Vec<f32>,
}

impl Default for Chrome {
    fn default() -> Self {
        Self {
            menu: None,
            overlay: None,
            last_dir: None,
            export_loop_count: 1,
            widgets: Widgets::default(),
            hover: None,
            hint: None,
            control_hover: None,
            grid_hold: None,
            last_name_press: None,
            lane_height: None,
            panel_size: (0, 0, Scale::ONE),
            beat_window: crate::view::BeatWindow::default(),
            panes: Panes::default(),
            zoom: session_zoom::Zoom::default(),
            color_scheme: 1,
            track_levels: Vec::new(),
        }
    }
}

impl Chrome {
    /// View > Simple view: the grid alone, the track editor and the note
    /// editor folded; again, both shown.
    pub(super) fn toggle_simple_view(&mut self) {
        self.panes.toggle_simple_view();
        self.widgets.focus = None;
        self.widgets.close_dropdown();
    }

    /// A menu, a popup or an overlay takes the pointer from the panels under it.
    pub(super) fn covered(&self) -> bool {
        self.menu.is_some() || self.widgets.is_popup_open() || self.overlay.is_some()
    }

    /// No menu, overlay, dropdown or entry open.
    pub(super) fn close_popups(&mut self) {
        self.overlay = None;
        self.menu = None;
        self.widgets.close_popup();
    }

    /// The VU levels after a frame that heard `raw`: each holds its peak and falls.
    pub(super) fn smooth_levels(&mut self, raw: &[f32]) -> &[f32] {
        if self.track_levels.len() < raw.len() {
            self.track_levels.resize(raw.len(), 0.0);
        }
        for (i, level) in self.track_levels.iter_mut().enumerate() {
            *level *= LEVEL_DECAY;
            if *level < LEVEL_FLOOR {
                *level = 0.0;
            }
            *level = level.max(raw.get(i).copied().unwrap_or(0.0).abs());
        }
        &self.track_levels
    }
}
