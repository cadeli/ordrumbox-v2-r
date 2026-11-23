//! Which beats of the pattern the grid shows: where they start and how many,
//! as the grid's scroll bar sets them.

/// The fewest beats the grid zooms in to.
pub const MIN_ZOOM_BEATS: i32 = 1;

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
