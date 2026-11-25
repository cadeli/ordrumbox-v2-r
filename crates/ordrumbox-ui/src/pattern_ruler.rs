//! The ruler of a pattern, the grid's and the piano roll's: where its beats
//! and steps are, and the loop and the play cursor drawn on it, as the
//! song's ruler shows the song's.

use crate::canvas::{Canvas, Rgb, text_width};
use crate::loop_marks;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::scroll_thumb::Edge;
use crate::theme;
use ordrumbox_core::steps::{TICK, absolute_step};
use std::ops::Range;

/// The play cursor's colour, and its width (scale units).
const CURSOR: Rgb = theme::FOCUS;
const CURSOR_WIDTH: i32 = 2;

/// The ticks of a pattern that play in a loop, `first..end` (`end` excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopMarks {
    pub first: u32,
    pub end: u32,
    /// The loop plays; off, its marks stay, grey.
    pub is_on: bool,
}

/// What a pattern's ruler shows besides its beats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RulerMarks {
    pub marks: Option<LoopMarks>,
    /// Where the pattern plays from, in ticks, while it does not play.
    pub cursor: Option<u32>,
}

/// Where a pattern's beats are along a ruler.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PatternRuler {
    pub rect: Rect,
    /// The beat at the ruler's left edge, and the width of a beat.
    pub first_beat: f64,
    pub beat_px: f64,
    /// Steps per beat of the track whose steps the ruler's lines follow.
    pub steps_per_beat: i32,
    scale: Scale,
}

/// The tick step `step` of a track of `steps_per_beat` steps per beat starts at.
#[must_use]
pub fn step_tick(step: u32, steps_per_beat: i32) -> u32 {
    step * TICK / u32::try_from(steps_per_beat.max(1)).unwrap_or(1)
}

impl PatternRuler {
    #[must_use]
    pub fn new(rect: Rect, (first_beat, beat_px): (f64, f64), steps_per_beat: i32, scale: Scale) -> Self {
        Self { rect, first_beat, beat_px: beat_px.max(1.0), steps_per_beat: steps_per_beat.max(1), scale }
    }

    /// The beat (fractional) at `x`.
    #[must_use]
    pub fn beat_at(&self, x: i32) -> f64 {
        self.first_beat + f64::from(x - self.rect.x) / self.beat_px
    }

    /// The step `x` is on, of the `step_count` of the pattern.
    #[must_use]
    pub fn step_at(&self, x: i32, step_count: u32) -> u32 {
        let step = (self.beat_at(x) * f64::from(self.steps_per_beat)).floor().max(0.0) as u32;
        step.min(step_count.saturating_sub(1))
    }

    /// The line between steps nearest to `x`, of the `step_count` of the pattern.
    #[must_use]
    pub fn line_at(&self, x: i32, step_count: u32) -> u32 {
        let line = (self.beat_at(x) * f64::from(self.steps_per_beat)).round().max(0.0) as u32;
        line.min(step_count)
    }

    /// The left edge of tick `tick`.
    #[must_use]
    pub fn x_of(&self, tick: u32) -> i32 {
        let beats = f64::from(tick) / f64::from(TICK) - self.first_beat;
        self.rect.x + (beats * self.beat_px).round() as i32
    }

    /// The end of `marks` at `(x, y)` of the ruler, the nearer one when both are close.
    #[must_use]
    pub fn end_at(&self, marks: LoopMarks, x: i32, y: i32) -> Option<Edge> {
        if !self.rect.contains(x, y) {
            return None;
        }
        loop_marks::end_near((self.x_of(marks.first), self.x_of(marks.end)), x, self.scale)
    }

    /// The beats `beats` (indexes from 0, of the `total_beats` of the
    /// pattern): a tick where each starts, a dot per step, and its number,
    /// every second, fourth... beat when they do not all fit; the beat
    /// `playing` in `playing_color`.
    pub fn draw_beats(&self, canvas: &mut Canvas, beats: Range<i32>, total_beats: i32, playing: (Option<i32>, Rgb)) {
        let (r, s) = (self.rect, self.scale);
        let right = r.x + r.w;
        let widest = text_width(&total_beats.to_string(), s) + 4 * s;
        let mut every = 1;
        while self.beat_px * f64::from(every) < f64::from(widest) && every < total_beats {
            every *= 2;
        }
        for beat in beats {
            let x = self.x_of(u32::try_from(beat).unwrap_or(0) * TICK);
            canvas.fill_rect(x, r.y, s.px(), r.h - 2 * s, theme::muted());
            for step in 1..self.steps_per_beat {
                let at = u32::try_from(absolute_step(beat, step, self.steps_per_beat)).unwrap_or(0);
                let dot = self.x_of(step_tick(at, self.steps_per_beat));
                if dot < right {
                    canvas.fill_rect(dot, r.y + r.h - 3 * s, s.px(), s.px(), theme::muted());
                }
            }
            if beat % every == 0 {
                let color = if playing.0 == Some(beat) { playing.1 } else { theme::muted() };
                canvas.text(x + 3 * s, r.y, &(beat + 1).to_string(), s, color);
            }
        }
    }

    /// The loop, then the play cursor, over the ruler.
    pub fn draw_marks(&self, canvas: &mut Canvas, marks: &RulerMarks) {
        let (r, s) = (self.rect, self.scale);
        let shown = r.x..=r.x + r.w;
        if let Some(LoopMarks { first, end, is_on }) = marks.marks {
            loop_marks::draw(canvas, r, (self.x_of(first), self.x_of(end)), is_on, s);
        }
        if let Some(x) = marks.cursor.map(|tick| self.x_of(tick)).filter(|x| shown.contains(x)) {
            canvas.fill_rect(x, r.y, CURSOR_WIDTH * s, r.h, CURSOR);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ruler() -> PatternRuler {
        // Two beats of 100 px from beat 4, 4 steps a beat.
        PatternRuler::new(Rect::new(10, 0, 200, 12), (4.0, 100.0), 4, Scale::ONE)
    }

    #[test]
    fn x_and_ticks_and_steps_agree() {
        let r = ruler();
        assert_eq!(r.x_of(4 * TICK), 10);
        assert_eq!(r.x_of(5 * TICK), 110);
        assert_eq!(r.step_at(10 + 26, 64), 17, "a quarter of the beat past beat 4: step 17");
        assert_eq!(r.line_at(10 + 30, 64), 17, "nearer the line of step 17 than 18");
        assert_eq!(r.step_at(10, 8), 7, "the last step at most");
        assert_eq!(step_tick(5, 4), 5 * TICK / 4);
    }

    #[test]
    fn an_end_is_taken_near_it_on_the_ruler_only() {
        let r = ruler();
        let marks = LoopMarks { first: 4 * TICK, end: 5 * TICK, is_on: true };
        assert_eq!(r.end_at(marks, 12, 5), Some(Edge::Start));
        assert_eq!(r.end_at(marks, 108, 5), Some(Edge::End));
        assert_eq!(r.end_at(marks, 60, 5), None);
        assert_eq!(r.end_at(marks, 12, 40), None, "below the ruler");
    }
}
