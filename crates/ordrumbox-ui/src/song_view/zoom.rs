//! The arrangement's zoom, as a DAW timeline has it: how wide a measure is
//! drawn, the handles at the ends of the measures' thumb that set it, and
//! which measures the ruler numbers at that width.

use super::{Axis, MEASURE_WIDTH, SongLayout};
use crate::canvas::text_width;

/// How wide a measure is drawn, in scale units: the arrangement's zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeasureZoom(i32);

impl Default for MeasureZoom {
    fn default() -> Self {
        Self(MEASURE_WIDTH)
    }
}

impl MeasureZoom {
    /// The narrowest measure: a long song shown whole.
    pub const MIN: i32 = 2;
    /// The widest measure: a few measures across the window.
    pub const MAX: i32 = 160;

    /// A measure `units` wide, kept within [`Self::MIN`]..=[`Self::MAX`].
    #[must_use]
    pub fn new(units: i32) -> Self {
        Self(units.clamp(Self::MIN, Self::MAX))
    }

    /// The width of a measure, in scale units.
    #[must_use]
    pub const fn units(self) -> i32 {
        self.0
    }

    /// This zoom `factor` times wider (narrower below 1), at least one unit
    /// further when the factor is not 1, so that a notch always changes it.
    #[must_use]
    pub fn times(self, factor: f64) -> Self {
        let units = (f64::from(self.0) * factor).round() as i32;
        let units = match factor.total_cmp(&1.0) {
            std::cmp::Ordering::Greater => units.max(self.0 + 1),
            std::cmp::Ordering::Less => units.min(self.0 - 1),
            std::cmp::Ordering::Equal => units,
        };
        Self::new(units)
    }
}

/// An end of the measures' thumb, held to zoom.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Start,
    End,
}

/// Width of a handle at an end of the measures' thumb.
const HANDLE: i32 = 6;

impl SongLayout {
    /// The zoom at which `measures` fill the arrangement's width.
    #[must_use]
    pub fn zoom_for(&self, measures: f64) -> MeasureZoom {
        let (across, measures) = (self.measures_width(), measures.max(1.0));
        let units = f64::from(across) * 4.0 / (measures * f64::from(self.scale.quarters()));
        // A measure's pixels are rounded: one unit less when they would not fit.
        let mut zoom = MeasureZoom::new(units.floor() as i32);
        while zoom.units() > MeasureZoom::MIN && f64::from(zoom.units() * self.scale) * measures > f64::from(across) {
            zoom = MeasureZoom::new(zoom.units() - 1);
        }
        zoom
    }

    /// The width the measures are drawn in, left of the rows' scroll bar.
    pub(super) fn measures_width(&self) -> i32 {
        (self.grid.w - self.label_width() - self.grid_bar()).max(0)
    }

    /// The handle at `x` of the measures' thumb `(start, len)`, if any. A
    /// thumb too short for two handles and a middle keeps only its end handle.
    #[must_use]
    pub fn zoom_handle(&self, x: i32, (start, len): (i32, i32)) -> Option<Edge> {
        let handle = HANDLE * self.scale;
        if !(start..start + len).contains(&x) {
            return None;
        }
        if x >= start + len - handle {
            Some(Edge::End)
        } else if x < start + handle && len >= 3 * handle {
            Some(Edge::Start)
        } else {
            None
        }
    }

    /// Where the handles of the measures' thumb `(start, len)` are along it,
    /// and how wide (`None`: the start's, left out).
    #[must_use]
    pub(super) fn handle_spans(&self, (start, len): (i32, i32)) -> (Option<(i32, i32)>, (i32, i32)) {
        let handle = HANDLE * self.scale;
        let start_handle = (len >= 3 * handle).then_some((start, handle));
        (start_handle, (start + len - handle.min(len), handle.min(len)))
    }

    /// The measure (fractional) the measures' track puts at `x` when it
    /// spans `span` measures.
    #[must_use]
    pub fn track_measure(&self, x: i32, span: u32) -> f64 {
        let (start, len) = self.track(Axis::Measures);
        f64::from(x - start) / f64::from(len.max(1)) * f64::from(span)
    }

    /// The measure (fractional) at `x` over the arrangement, `first` the first shown.
    #[must_use]
    pub fn measure_under(&self, x: i32, first: u32) -> f64 {
        f64::from(first) + f64::from(x - self.body_left()) / f64::from(self.measure_width().max(1))
    }

    /// Every how many measures the ruler is numbered: each one while its
    /// number fits, else every 2, 4, 8… so that the numbers do not overlap.
    #[must_use]
    pub(super) fn label_step(&self, last_measure: u32) -> u32 {
        let s = self.scale;
        let room = text_width(&(last_measure + 1).to_string(), s) + 4 * s;
        let mut step = 1;
        while step * self.measure_width().max(1) < room && step < 1 << 16 {
            step *= 2;
        }
        u32::try_from(step).unwrap_or(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scale::Scale;

    #[test]
    fn the_zoom_stays_in_its_range_and_a_notch_always_moves_it() {
        assert_eq!(MeasureZoom::new(0).units(), MeasureZoom::MIN);
        assert_eq!(MeasureZoom::new(1000).units(), MeasureZoom::MAX);
        assert_eq!(MeasureZoom::new(2).times(1.25).units(), 3);
        assert_eq!(MeasureZoom::new(3).times(0.8).units(), 2);
        assert_eq!(MeasureZoom::new(24).times(1.25).units(), 30);
        assert_eq!(MeasureZoom::new(MeasureZoom::MAX).times(2.0).units(), MeasureZoom::MAX);
    }

    #[test]
    fn a_zoom_for_measures_fits_them_and_the_ruler_numbers_fewer_when_narrow() {
        for scale in [Scale::ONE, Scale::from_quarters(5), Scale::whole(2)] {
            let wide = SongLayout::new(1100, 680, scale, 0, MeasureZoom::default());
            for measures in [4.0, 33.0, 100.0] {
                let layout = SongLayout::new(1100, 680, scale, 0, wide.zoom_for(measures));
                assert!(f64::from(layout.visible_measures()) >= measures, "{scale} {measures}");
            }
            let narrow = SongLayout::new(1100, 680, scale, 0, MeasureZoom::new(MeasureZoom::MIN));
            assert_eq!(wide.label_step(99), 1);
            assert!(narrow.label_step(99) >= 8);
        }
    }

    #[test]
    fn the_thumb_has_a_handle_at_each_end_and_a_short_one_only_its_end() {
        let layout = SongLayout::new(1100, 680, Scale::ONE, 0, MeasureZoom::default());
        let thumb @ (start, len) = layout.thumb(Axis::Measures, 0, 10, 40);
        assert_eq!(layout.zoom_handle(start + 1, thumb), Some(Edge::Start));
        assert_eq!(layout.zoom_handle(start + len - 1, thumb), Some(Edge::End));
        assert_eq!(layout.zoom_handle(start + len / 2, thumb), None);
        assert_eq!(layout.zoom_handle(start + len + 4, thumb), None);
        let thumb @ (start, len) = layout.thumb(Axis::Measures, 0, 1, 10_000);
        assert!(len < 3 * HANDLE);
        assert_eq!(layout.zoom_handle(start + len - 1, thumb), Some(Edge::End));
        assert_eq!(layout.zoom_handle(start, thumb), None, "the rest of it scrolls");
    }
}
