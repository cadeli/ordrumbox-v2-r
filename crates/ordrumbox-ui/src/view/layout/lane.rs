//! The lane under the grid's rows: its list of settings with the logo above
//! it, its bars, and the strip that sets its height, in the grid's [`Layout`].

use super::{Layout, VOLUME_WIDTH};
use crate::panel::Rect;

/// Height of the lane's list box, per scale unit: the list keeps the bottom
/// of the grid, whatever the bars' height.
pub(super) const LANE_LIST_HEIGHT: i32 = 14;
/// Height of the logo above the lane's list, per scale unit: `logo2.png` at
/// half its natural size, fitted to the list's width.
pub(super) const LANE_LOGO_HEIGHT: i32 = 32;
/// The gap between the logo and the lane's list, per scale unit.
pub(super) const LANE_LOGO_GAP: i32 = 4;
/// The gap between the add-track button and the lane, per scale unit.
pub(super) const LANE_GAP: i32 = 8;
/// The tallest the lane's bars get, per scale unit: they take the room the
/// rows leave, up to this.
pub(super) const MAX_LANE: i32 = 480;
/// The tallest the lane can be set to, per scale unit.
pub const MAX_LANE_SET: i32 = 960;

/// Height of the lane under the rows at its smallest, per scale unit: the
/// logo and the list box left of its bars.
pub const fn lane_height() -> i32 {
    LANE_LOGO_HEIGHT + LANE_LOGO_GAP + LANE_LIST_HEIGHT
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

    /// The list that picks the lane's setting, left of the bars: as wide as
    /// the add-track button, and kept at the bottom of the grid whatever the
    /// bars' height.
    #[must_use]
    pub fn lane_list_rect(&self) -> Rect {
        let h = LANE_LIST_HEIGHT * self.scale;
        Rect::new(self.left, self.bottom - h, self.list_width(), h)
    }

    /// The logo just above the lane's list, as wide as it: the lane is never
    /// shorter than the two, so the logo keeps its place whatever the bars' height.
    #[must_use]
    pub fn lane_logo_rect(&self) -> Rect {
        let list = self.lane_list_rect();
        let h = LANE_LOGO_HEIGHT * self.scale;
        Rect::new(self.left, list.y - LANE_LOGO_GAP * self.scale - h, self.list_width(), h)
    }

    /// The width of the lane's list and logo: the add-track button's.
    fn list_width(&self) -> i32 {
        self.volume_x + VOLUME_WIDTH * self.scale - self.left
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
