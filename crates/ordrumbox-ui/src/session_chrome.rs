//! What the window shows around the library, and nothing else reads: the
//! open menu and overlay, the widgets the mouse and keyboard hold, the grid
//! page, the panels shown, the VU levels. Undo, saving and playback never
//! touch it.

use crate::scale::Scale;
use std::path::PathBuf;

use super::session_overlay::Overlay;
use super::{menu, session_grid, session_lane, session_zone};
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
    /// A slider's bar (left, width), when it follows the mouse.
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
    pub(super) dropdown: Option<Dropdown>,
    pub(super) entry: Option<Entry>,
    /// The mouse, at its last move or press: where a text entry opens.
    pub(super) pointer: (i32, i32),
    /// Last click on a widget, for double-clicks.
    pub(super) last_click: Option<(Id, Instant)>,
    /// A control's value copied from its right-click menu, to paste on
    /// another whose range holds it.
    pub(super) copied_value: Option<f64>,
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
    /// What the mouse and the keyboard hold among the widgets.
    pub(super) widgets: Widgets,
    /// What of the grid is under the mouse, for its tooltip: a step (its
    /// note's) or a track's name (the track's).
    pub(super) hover: Option<crate::view::Hit>,
    /// The control of a panel under the mouse that has a tooltip, and where it is.
    pub(super) control_hover: Option<(crate::panel::Id, crate::panel::Rect)>,
    /// A track pressed by its name, that may be dragged to another row.
    pub(super) track_drag: Option<session_grid::TrackDrag>,
    /// A step Shift+pressed, that may be dragged into a zone.
    pub(super) zone_drag: Option<session_zone::ZoneDrag>,
    /// The lane under the grid, pressed and painted.
    pub(super) lane_drag: Option<session_lane::LaneDrag>,
    /// Window size and scale of the last mouse or key event on the widgets.
    pub(super) panel_size: (i32, i32, Scale),
    /// Current page of the pattern grid (`view::page_beats` beats).
    pub(super) current_page: usize,
    /// The track editor shown on the right.
    pub(super) show_track_editor: bool,
    /// The note editor shown under the grid and the piano roll.
    pub(super) show_note_editor: bool,
    /// The master strip shown along the bottom (in the app: a plugin has none).
    pub(super) show_master_strip: bool,
    /// Whether the VU meter stripe is shown.
    pub(super) show_vus: bool,
    /// The display's scale factor (at least ×1), to the nearest quarter.
    pub(super) display_scale: Scale,
    /// Quarters of a scale unit added to the display's (Ctrl++, Ctrl+-; `session_zoom.rs`).
    pub(super) zoom: i32,
    /// The window's size in physical pixels, which "Zoom: fit to window" fits.
    pub(super) window: (i32, i32),
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
            widgets: Widgets::default(),
            hover: None,
            control_hover: None,
            track_drag: None,
            zone_drag: None,
            lane_drag: None,
            panel_size: (0, 0, Scale::ONE),
            current_page: 0,
            show_track_editor: true,
            show_note_editor: true,
            show_master_strip: true,
            show_vus: true,
            display_scale: Scale::ONE,
            zoom: 0,
            window: (0, 0),
            color_scheme: 1,
            track_levels: Vec::new(),
        }
    }
}

impl Chrome {
    /// The compact view: the grid alone, the track editor and the note
    /// editor folded away (a plugin's window starts so, small over the
    /// host's arrangement).
    pub(super) fn fold_editors(&mut self) {
        self.show_track_editor = false;
        self.show_note_editor = false;
    }

    /// A dropdown or an entry is open.
    pub(super) fn popup_open(&self) -> bool {
        self.widgets.dropdown.is_some() || self.widgets.entry.is_some()
    }

    /// A menu, a popup or an overlay takes the pointer from the panels under it.
    pub(super) fn covered(&self) -> bool {
        self.menu.is_some() || self.popup_open() || self.overlay.is_some()
    }

    /// No menu, overlay, dropdown or entry open.
    pub(super) fn close_popups(&mut self) {
        self.overlay = None;
        self.menu = None;
        self.widgets.dropdown = None;
        self.widgets.entry = None;
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
