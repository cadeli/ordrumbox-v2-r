//! Which beats of the pattern the grid shows: where they start and how many,
//! as the grid's scroll bar sets them.

use crate::scroll_thumb::Edge;

/// The fewest beats a view zooms in to.
pub const MIN_ZOOM_BEATS: i32 = 1;
/// A notch of Ctrl+wheel: this many times more (or fewer) beats shown.
const ZOOM_STEP: f64 = 1.25;

/// What the mouse holds on a scroll bar of beats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarHold {
    /// The thumb, centred on the pointer.
    Scroll,
    /// An end of the thumb: the other end stays on beat `fixed`.
    Zoom { edge: Edge, fixed: i32 },
}

impl BarHold {
    /// A hold on `edge` of the thumb that shows `shown` beats from `first`.
    #[must_use]
    pub const fn zoom(edge: Edge, first: i32, shown: i32) -> Self {
        Self::Zoom { edge, fixed: if matches!(edge, Edge::Start) { first + shown } else { first } }
    }
}

/// The beats the grid shows: from `first`, as many as the window fits (a
/// page, `view::page_beats`) or as many as the zoom of the scroll bar sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BeatWindow {
    first: i32,
    zoom: Option<i32>,
    /// The page the playhead was in at the last follow, and the cursor's
    /// beat: the window follows them only when they move on, so that a
    /// scroll away from them stays where it is.
    seen_page: Option<i32>,
    seen_cursor: Option<i32>,
}

impl BeatWindow {
    /// The first beat shown.
    #[must_use]
    pub const fn first(self) -> i32 {
        self.first
    }

    /// The beats a full window shows of a pattern of `total` beats, `fit`
    /// being what the window's width allows.
    #[must_use]
    pub fn shown(self, fit: i32, total: i32) -> i32 {
        self.zoom.map_or(fit, |zoom| zoom.min(total)).max(MIN_ZOOM_BEATS)
    }

    /// The window starts at `first` (from the first beat).
    pub fn scroll_to(&mut self, first: i32) {
        self.first = first.max(0);
    }

    /// The window shows `beats` of the pattern from now on.
    pub fn zoom_to(&mut self, beats: i32) {
        self.zoom = Some(beats.max(MIN_ZOOM_BEATS));
    }

    /// The bar held, with the pointer at `at` (a beat, fractional, along the
    /// bar's track of `total` beats) and `centred` the first beat that puts
    /// the thumb on the pointer.
    pub fn hold(&mut self, hold: BarHold, centred: i32, at: f64, total: i32) {
        match hold {
            BarHold::Scroll => self.scroll_to(centred),
            BarHold::Zoom { edge, fixed } => {
                let beats = match edge {
                    Edge::Start => f64::from(fixed) - at,
                    Edge::End => at - f64::from(fixed),
                };
                let beats = (beats.round() as i32).clamp(MIN_ZOOM_BEATS, total.max(MIN_ZOOM_BEATS));
                self.zoom_to(beats);
                if edge == Edge::Start {
                    self.scroll_to(fixed - beats);
                }
            }
        }
    }

    /// `notches` of Ctrl+wheel (up: fewer beats), the beat `anchor` kept
    /// under the pointer, `across` of the way along the beats shown, which
    /// are `shown` of `total`.
    pub fn zoom_by(&mut self, notches: isize, (shown, total): (i32, i32), anchor: f64, across: f64) {
        let mut beats = shown;
        for _ in 0..notches.unsigned_abs() {
            // A notch always changes it, whatever the rounding.
            beats = if notches > 0 {
                ((f64::from(beats) / ZOOM_STEP).round() as i32).min(beats - 1)
            } else {
                ((f64::from(beats) * ZOOM_STEP).round() as i32).max(beats + 1)
            }
            .clamp(MIN_ZOOM_BEATS, total.max(MIN_ZOOM_BEATS));
        }
        self.zoom_to(beats);
        self.scroll_to((anchor - across * f64::from(beats)).round() as i32);
    }

    /// `notches` of wheel, a beat each: positive ones show what is before.
    pub fn scroll_by(&mut self, notches: isize, shown: i32, total: i32) {
        let last = i64::from((total - shown).max(0));
        self.scroll_to((i64::from(self.first) - notches as i64).clamp(0, last) as i32);
    }

    /// Bring `beat` in sight, in a window of `shown` beats.
    pub fn reveal(&mut self, shown: i32, beat: i32) {
        let shown = shown.max(1);
        if !(self.first..self.first + shown).contains(&beat) {
            self.first = beat.max(0) / shown * shown;
        }
    }

    /// Back to the first beat, the zoom kept.
    pub fn rewind(&mut self) {
        self.first = 0;
        self.seen_page = None;
        self.seen_cursor = None;
    }

    /// Bring the playhead (a beat) in sight when it moves on to another
    /// page of `shown` beats; else the cursor (a beat) when it moves.
    pub fn follow(&mut self, shown: i32, playhead: Option<i32>, cursor: Option<i32>) {
        let shown = shown.max(1);
        let visible = |first: i32, beat: i32| (first..first + shown).contains(&beat);
        if let Some(beat) = playhead {
            let page = beat / shown;
            if self.seen_page != Some(page) && !visible(self.first, beat) {
                self.first = page * shown;
            }
            self.seen_page = Some(page);
            self.seen_cursor = None;
        } else {
            self.seen_page = None;
            if let Some(beat) = cursor.filter(|&beat| self.seen_cursor != Some(beat)) {
                if !visible(self.first, beat) {
                    self.first = beat / shown * shown;
                }
                self.seen_cursor = Some(beat);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_window_follows_the_playhead_to_the_next_page_only() {
        let mut window = BeatWindow::default();
        window.follow(4, Some(1), None);
        assert_eq!(window.first(), 0);
        window.follow(4, Some(5), None);
        assert_eq!(window.first(), 4);
        // Scrolled away: the playhead's next beat on the same page leaves it be.
        window.scroll_to(12);
        window.follow(4, Some(6), None);
        assert_eq!(window.first(), 12);
        window.follow(4, Some(9), None);
        assert_eq!(window.first(), 8);
    }

    #[test]
    fn the_window_follows_a_cursor_that_moves_out_of_sight() {
        let mut window = BeatWindow::default();
        window.follow(4, None, Some(9));
        assert_eq!(window.first(), 8);
        window.scroll_to(0);
        window.follow(4, None, Some(9));
        assert_eq!(window.first(), 0, "the cursor did not move");
        window.follow(4, None, Some(10));
        assert_eq!(window.first(), 8);
    }

    #[test]
    fn the_zoom_is_kept_inside_the_pattern() {
        let mut window = BeatWindow::default();
        assert_eq!(window.shown(8, 32), 8);
        window.zoom_to(0);
        assert_eq!(window.shown(8, 32), MIN_ZOOM_BEATS);
        window.zoom_to(100);
        assert_eq!(window.shown(8, 32), 32);
    }
}
