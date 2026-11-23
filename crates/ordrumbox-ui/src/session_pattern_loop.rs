//! The loop and the play cursor of a pattern, set on the ruler of the grid
//! and of the piano roll as the song's are on its ruler: a drag selects the
//! steps it plays (and turns the loop on), a drag on one of its ends moves
//! that end, and a click without a drag puts the play cursor on the step
//! clicked (the pattern goes on from there at once when it plays, and plays
//! from there next time). A right-click opens the loop's menu.

use super::Session;
use super::session_song::DRAG_START_PX;
use crate::pattern_ruler::{LoopMarks, PatternRuler, RulerMarks, step_tick};
use crate::scale::Scale;
use crate::scroll_thumb::Edge;
use ordrumbox_core::steps::TICK;
use ordrumbox_engine::{Mode, PatternLoop};

/// A press on a pattern's ruler, held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PatternLoopDrag {
    /// A new loop from step `anchor`, pressed at `press_x`; `started` once the pointer has really moved.
    New { anchor: u32, press_x: i32, started: bool },
    /// An end of the loop, moved.
    End(Edge),
}

/// A position in a pattern, as beats and steps: `2` for a beat's start, `2.3` for its third step.
fn position_text(tick: u32, steps_per_beat: i32) -> String {
    let (beat, in_beat) = (tick / TICK + 1, tick % TICK);
    if in_beat == 0 {
        return beat.to_string();
    }
    let step = in_beat * u32::try_from(steps_per_beat.max(1)).unwrap_or(1) / TICK + 1;
    format!("{beat}.{step}")
}

impl Session {
    /// The loop of the pattern shown, on or off.
    pub(super) fn pattern_loop_marks(&self) -> Option<LoopMarks> {
        let (pattern, first, end) = self.transport.pattern_loop?;
        (pattern == self.pattern_index && first < end).then_some(LoopMarks {
            first,
            end,
            is_on: self.transport.is_loop_on,
        })
    }

    /// The loop the engine plays, with the pattern it is of: none while it is off.
    pub(super) fn pattern_loop_region(&self) -> Option<PatternLoop> {
        let (pattern, first, end) = self.transport.pattern_loop.filter(|_| self.transport.is_loop_on)?;
        PatternLoop::new(pattern, first, end)
    }

    /// What the ruler of the pattern shows besides its beats: the loop and,
    /// while the pattern does not play, where it plays from.
    pub(super) fn ruler_marks(&self) -> RulerMarks {
        let cursor = (self.transport.pattern_cursor > 0 && !self.playback.plays(Mode::Pattern(self.pattern_index)))
            .then_some(self.transport.pattern_cursor);
        RulerMarks { marks: self.pattern_loop_marks(), cursor }
    }

    /// The ruler of the view that shows the pattern (the piano roll or the
    /// grid), `None` in song mode, where the song's ruler is the one.
    pub(super) fn pattern_ruler(&self, size: (i32, i32, Scale)) -> Option<PatternRuler> {
        if self.is_song_mode() {
            return None;
        }
        if self.piano_roll_open() {
            let layout = self.roll_layout(size);
            return Some(layout.pattern_ruler(&self.roll_view(&layout)?));
        }
        let layout = self.grid_layout(size)?;
        let spb = self.selection.cursor.and_then(|c| self.track(c.track)).or(self.pattern()?.tracks.first());
        Some(layout.ruler(spb.map_or(4, crate::view::steps_per_beat)))
    }

    /// The steps of the pattern shown, counted by the ruler's lines.
    fn pattern_step_count(&self, ruler: &PatternRuler) -> u32 {
        let beats = self.pattern().map_or(1, |p| p.beat_count.max(1));
        u32::try_from(beats).unwrap_or(1) * u32::try_from(ruler.steps_per_beat).unwrap_or(1)
    }

    /// A press at `(x, y)` on the ruler of the pattern: a new loop or an end
    /// of the loop held; whether it was on it.
    pub fn press_pattern_ruler(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        let Some(ruler) = self.pattern_ruler(size).filter(|ruler| ruler.rect.contains(x, y)) else { return false };
        let end = self.pattern_loop_marks().and_then(|marks| ruler.end_at(marks, x, y));
        let anchor = ruler.step_at(x, self.pattern_step_count(&ruler));
        self.transport.loop_drag = Some(match end {
            Some(edge) => PatternLoopDrag::End(edge),
            None => PatternLoopDrag::New { anchor, press_x: x, started: false },
        });
        true
    }

    /// The pointer at `x` while the ruler is pressed: the loop selected, or
    /// its end moved, on the steps' lines; whether it changed.
    pub(super) fn pattern_loop_move(&mut self, size: (i32, i32, Scale), x: i32) -> bool {
        let (Some(drag), Some(ruler)) = (self.transport.loop_drag, self.pattern_ruler(size)) else { return false };
        let step_count = self.pattern_step_count(&ruler);
        let spb = ruler.steps_per_beat;
        let total = self.pattern().map_or(1, |p| p.beat_count.max(1)).unsigned_abs() * TICK;
        let before = self.pattern_loop_marks().map(|m| (m.first, m.end));
        let region = match drag {
            PatternLoopDrag::New { anchor, press_x, started } => {
                let started = started || (x - press_x).abs() >= DRAG_START_PX;
                self.transport.loop_drag = Some(PatternLoopDrag::New { anchor, press_x, started });
                if !started {
                    return false;
                }
                let step = ruler.step_at(x, step_count);
                let (low, high) = (anchor.min(step), anchor.max(step));
                Some((step_tick(low, spb), step_tick(high + 1, spb).min(total)))
            }
            PatternLoopDrag::End(edge) => before.and_then(|(start, end)| {
                let line = step_tick(ruler.line_at(x, step_count), spb).min(total);
                let (start, end, edge) = edge.moved_to((start, end), line);
                self.transport.loop_drag = Some(PatternLoopDrag::End(edge));
                (start < end).then_some((start, end))
            }),
        };
        let Some((first, end)) = region.filter(|&region| Some(region) != before) else { return false };
        if matches!(drag, PatternLoopDrag::New { .. }) {
            self.transport.is_loop_on = true;
        }
        self.move_pattern_loop(first, end, spb);
        true
    }

    /// The pattern shown loops over ticks `first..end`, on or off as it was; said as beats and steps.
    fn move_pattern_loop(&mut self, first: u32, end: u32, steps_per_beat: i32) {
        self.transport.pattern_loop = Some((self.pattern_index, first, end));
        let state = if self.transport.is_loop_on { "Ctrl+L turns it off" } else { "off, Ctrl+L turns it on" };
        let at = |tick| position_text(tick, steps_per_beat);
        self.message = format!("loop: {} to {}, {state}", at(first), at(end));
    }

    /// The ruler released: a click without a drag puts the play cursor on its step.
    pub fn pattern_loop_release(&mut self, size: (i32, i32, Scale)) -> bool {
        let Some(drag) = self.transport.loop_drag.take() else { return false };
        let PatternLoopDrag::New { anchor, started: false, .. } = drag else { return true };
        let Some(ruler) = self.pattern_ruler(size) else { return true };
        let tick = step_tick(anchor, ruler.steps_per_beat);
        self.transport.pattern_cursor = tick;
        self.message = format!("play from {}", position_text(tick, ruler.steps_per_beat));
        if self.playback.plays(Mode::Pattern(self.pattern_index)) {
            self.playback.seek_pattern(&self.editor, tick);
        }
        true
    }

    /// A right-click at `(x, y)` on the ruler of the pattern: the loop's menu; whether it was on it.
    pub fn pattern_ruler_menu(&mut self, size: (i32, i32, Scale), x: i32, y: i32) -> bool {
        if !self.pattern_ruler(size).is_some_and(|ruler| ruler.rect.contains(x, y)) {
            return false;
        }
        self.loop_menu(x, y);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test_support::SIZE;

    /// A pattern of eight beats, on the grid or the piano roll.
    fn session(roll: bool) -> Session {
        let (library, _) = ordrumbox_core::Library::parse(
            r#"{"patterns":[{"name":"p","beatCount":8,"tracks":[{"name":"KICK","notes":[{}]},
                {"name":"BASS","pitch":-12,"beatCount":8,"notes":[{"beat":1,"pitch":3}]}]}]}"#,
        )
        .unwrap();
        let mut s = Session::new(library, crate::kits::KitSet::empty(), Vec::new());
        s.set_size(SIZE);
        if roll {
            s.select_roll_track(1);
            s.toggle_piano_roll();
        }
        s
    }

    /// The x and y on the ruler of the line before step `step` (+2 px: inside the step).
    fn at_step(s: &Session, step: u32) -> (i32, i32) {
        let ruler = s.pattern_ruler(SIZE).expect("a ruler");
        let tick = step_tick(step, ruler.steps_per_beat);
        (ruler.x_of(tick) + 2, ruler.rect.y + ruler.rect.h / 2)
    }

    fn drag(s: &mut Session, from: (i32, i32), to_x: i32) {
        assert!(s.press_pattern_ruler(SIZE, from.0, from.1));
        s.mouse_move(SIZE, to_x, from.1, false);
        s.mouse_up();
        s.pattern_loop_release(SIZE);
    }

    #[test]
    fn a_drag_on_the_ruler_sets_a_loop_and_a_click_the_play_cursor() {
        for roll in [false, true] {
            ruler_loop_and_cursor(&mut session(roll));
        }
    }

    fn ruler_loop_and_cursor(s: &mut Session) {
        let s = &mut *s;
        let spb = s.pattern_ruler(SIZE).expect("a ruler").steps_per_beat;
        let ((from, y), (to, _)) = (at_step(s, 2), at_step(s, 5));
        drag(s, (from, y), to);
        let marks = s.pattern_loop_marks().expect("a loop");
        assert_eq!((marks.first, marks.end, marks.is_on), (step_tick(2, spb), step_tick(6, spb), true));
        assert!(s.pattern_loop_region().is_some());

        // A click: the play cursor, the loop kept.
        let (x, y) = at_step(s, 3);
        assert!(s.press_pattern_ruler(SIZE, x, y));
        s.pattern_loop_release(SIZE);
        assert_eq!(s.transport.pattern_cursor, step_tick(3, spb));
        assert_eq!(s.pattern_loop_marks().map(|m| m.first), Some(step_tick(2, spb)));

        // Its end, dragged to another line.
        let end_x = s.pattern_ruler(SIZE).expect("a ruler").x_of(step_tick(6, spb));
        let to = at_step(s, 9).0 - 2;
        drag(s, (end_x, y), to);
        assert_eq!(s.pattern_loop_marks().map(|m| m.end), Some(step_tick(9, spb)));

        // A right-click: the loop's menu, then off.
        assert!(s.pattern_ruler_menu(SIZE, x, y));
        s.toggle_loop();
        assert_eq!((s.pattern_loop_region(), s.pattern_loop_marks().map(|m| m.is_on)), (None, Some(false)));
        s.clear_loop();
        assert_eq!(s.pattern_loop_marks(), None);
    }
}
