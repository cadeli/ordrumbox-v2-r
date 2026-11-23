//! The lane under the grid's rows: its tabs, its bars and the strip that
//! sets its height, in the grid's [`Layout`].

use super::{Layout, VOLUME_WIDTH};
use crate::editors;
use crate::panel::Rect;

/// Height of a lane tab, the gap between two, per scale unit: the tabs stand
/// in rows of [`LANE_TAB_COLUMNS`] left of the lane, which is as high as they are.
pub(super) const LANE_TAB_HEIGHT: i32 = 14;
pub(super) const LANE_TAB_GAP: i32 = 2;
/// The lane tabs side by side in a row.
pub(super) const LANE_TAB_COLUMNS: usize = 2;

/// The rows the lane's tabs stand in.
#[must_use]
pub const fn lane_tab_rows() -> i32 {
    editors::LANES.len().div_ceil(LANE_TAB_COLUMNS) as i32
}
/// The gap between the add-track button and the lane, per scale unit.
pub(super) const LANE_GAP: i32 = 8;
/// The tallest the lane's bars get, per scale unit: they take the room the
/// rows leave, up to this.
pub(super) const MAX_LANE: i32 = 160;
/// The tallest the lane can be set to, per scale unit.
pub const MAX_LANE_SET: i32 = 320;

/// Height of the lane under the rows, per scale unit: the rows of its tabs.
pub const fn lane_height() -> i32 {
    let rows = lane_tab_rows();
    rows * LANE_TAB_HEIGHT + (rows - 1) * LANE_TAB_GAP
}

impl Layout {
    /// The lane's bars: under the steps, as wide as they are.
    #[must_use]
    pub fn lane_rect(&self) -> Rect {
        Rect::new(self.grid_left, self.lane_top, self.grid_width, self.bottom - self.lane_top)
    }

    /// The strip above the lane's bars, which a drag takes to set its height.
    #[must_use]
    pub fn lane_edge_rect(&self) -> Rect {
        let gap = (LANE_GAP - 1) * self.scale;
        Rect::new(self.grid_left, self.lane_top - gap, self.grid_width, gap)
    }

    /// The tab of lane `lane`, left of the bars: the tabs stand in rows of
    /// [`LANE_TAB_COLUMNS`], together as wide as the add-track button, and
    /// keep the bottom of the grid, whatever the bars' height.
    #[must_use]
    pub fn lane_tab_rect(&self, lane: usize) -> Rect {
        let (row, column) = ((lane / LANE_TAB_COLUMNS) as i32, (lane % LANE_TAB_COLUMNS) as i32);
        let y = self.bottom - lane_height() * self.scale + row * (LANE_TAB_HEIGHT + LANE_TAB_GAP) * self.scale;
        let gap = LANE_TAB_GAP * self.scale;
        let columns = LANE_TAB_COLUMNS as i32;
        let w = (self.volume_x + VOLUME_WIDTH * self.scale - self.left - (columns - 1) * gap) / columns;
        Rect::new(self.left + column * (w + gap), y, w, LANE_TAB_HEIGHT * self.scale)
    }

    /// The step of the page (0: its first) of a track of `spb` steps per
    /// beat whose bar is at `(x, y)`, if any.
    #[must_use]
    pub fn lane_step(&self, x: i32, y: i32, spb: i32) -> Option<i64> {
        if !self.lane_rect().contains(x, y) {
            return None;
        }
        let step = ((f64::from(x - self.grid_left) * f64::from(spb)) / self.beat_width()) as i64;
        (step < i64::from(self.beats) * i64::from(spb)).then_some(step)
    }

    /// The step under `x` of a track of `spb` steps per beat, kept to the
    /// page: where a drag along the lane is.
    #[must_use]
    pub fn lane_step_clamped(&self, x: i32, spb: i32) -> i64 {
        let step = ((f64::from(x - self.grid_left) * f64::from(spb)) / self.beat_width()).floor() as i64;
        step.clamp(0, (i64::from(self.beats) * i64::from(spb) - 1).max(0))
    }

    /// The fraction of the lane's height at `y`: 0 on its lowest pixel row, 1 on its top one.
    #[must_use]
    pub fn lane_fraction(&self, y: i32) -> f64 {
        let h = (self.bottom - self.lane_top - 1).max(1);
        (f64::from(self.bottom - 1 - y) / f64::from(h)).clamp(0.0, 1.0)
    }
}
