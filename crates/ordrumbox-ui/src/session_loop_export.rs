//! File > Export pattern as WAV and as MIDI: a modal chooses how many loops
//! of the pattern the file holds, then the folder is asked for. The count
//! chosen is kept for the next export, of either.

use super::Session;
use super::session_files::NOTHING_TO_RENDER;
use super::session_overlay::Overlay;
use super::session_types::OverlayKey;
use crate::panel::Step;
use crate::picker::Export;
use crate::scale::Scale;
use crate::view;

use ordrumbox_core::steps::MAX_LOOP_COUNT;

/// The lines a key reaches: the loop count, then the button.
const ROW_COUNT: usize = 2;
const EXPORT_ROW: usize = 1;

/// The modal's state: which export, of which pattern, how many loops, the line chosen.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    export: Export,
    pattern_name: String,
    loop_count: u32,
    row: usize,
}

impl Draft {
    fn title(&self) -> &'static str {
        if self.export == Export::PatternMidi { "Export pattern as MIDI" } else { "Export pattern as WAV" }
    }

    pub(super) fn lines(&self) -> Vec<String> {
        let mark = |row: usize| if row == self.row { ">" } else { " " };
        let loops = if self.loop_count == 1 { "loop " } else { "loops" };
        vec![
            format!("  Pattern   {}", self.pattern_name),
            format!("{} Length    {:>2} {loops}", mark(0), self.loop_count),
            format!("{} [ Export... ]", mark(EXPORT_ROW)),
            String::new(),
            "Up Down: line   Left Right: change   Enter: run   Esc: leave".into(),
        ]
    }

    /// The draft's line `line` (the Pattern line, which no key reaches, is not one).
    fn row_of_line(line: usize) -> Option<usize> {
        line.checked_sub(1).filter(|&row| row < ROW_COUNT)
    }

    /// The loop count one `step` away, from 1 to [`MAX_LOOP_COUNT`] and round.
    fn change(&mut self, step: Step) {
        if self.row != 0 {
            return;
        }
        let max = MAX_LOOP_COUNT;
        self.loop_count = match step {
            Step::Next => self.loop_count % max + 1,
            Step::Previous => (self.loop_count + max - 2) % max + 1,
        };
    }
}

impl Session {
    /// Ctrl+E (`Export::PatternWav`) or Ctrl+Shift+M (`Export::PatternMidi`):
    /// the modal, opened with the loop count of the last export.
    pub(super) fn open_loop_export(&mut self, export: Export) {
        if export == Export::PatternWav && self.render.is_none() {
            self.message = NOTHING_TO_RENDER.into();
            return;
        }
        let Some(pattern) = self.pattern() else {
            self.message = "nothing to export".into();
            return;
        };
        let pattern_name = pattern.name.clone().unwrap_or_else(|| pattern.id.clone());
        let loop_count = self.chrome.export_loop_count;
        self.chrome.overlay = Some(Overlay::LoopExport(Draft { export, pattern_name, loop_count, row: 0 }));
    }

    /// A key while the modal is open; whether it was.
    pub(super) fn loop_export_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::LoopExport(draft)) = &mut self.chrome.overlay else { return false };
        match key {
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Move(delta) => {
                draft.row = (draft.row as isize + delta).clamp(0, ROW_COUNT as isize - 1) as usize
            }
            OverlayKey::Edge(end) => draft.row = if *end { EXPORT_ROW } else { 0 },
            OverlayKey::Side(delta) => draft.change(if *delta < 0 { Step::Previous } else { Step::Next }),
            OverlayKey::Enter if draft.row == EXPORT_ROW => self.confirm_loop_export(),
            OverlayKey::Enter => draft.change(Step::Next),
            OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// A click while the modal is open: outside it leaves it, on a line
    /// chooses it and changes it; whether it was open.
    pub(super) fn loop_export_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::LoopExport(draft)) = &mut self.chrome.overlay else { return false };
        let (title, lines) = (draft.title(), draft.lines());
        let window = (width, height);
        if !view::notice_contains(window, title, &lines, scale, x, y) {
            self.chrome.overlay = None;
        } else if let Some(row) = view::notice_line_at(window, title, &lines, scale, x, y).and_then(Draft::row_of_line)
        {
            draft.row = row;
            if row == EXPORT_ROW {
                self.confirm_loop_export();
            } else {
                draft.change(Step::Next);
            }
        }
        true
    }

    /// The count chosen is kept, and the folder asked for.
    fn confirm_loop_export(&mut self) {
        let Some(Overlay::LoopExport(draft)) = self.chrome.overlay.take() else { return };
        self.chrome.export_loop_count = draft.loop_count;
        self.open_export(draft.export);
    }

    /// The modal, drawn.
    pub(super) fn draw_loop_export(canvas: &mut crate::canvas::Canvas, draft: &Draft, scale: Scale) {
        view::draw_notice(canvas, draft.title(), &draft.lines(), scale);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(loop_count: u32) -> Draft {
        Draft { export: Export::PatternWav, pattern_name: "p".into(), loop_count, row: 0 }
    }

    #[test]
    fn the_loop_count_goes_round_from_1_to_the_most() {
        let mut d = draft(1);
        // ← from 1 goes to the most.
        d.change(Step::Previous);
        assert_eq!(d.loop_count, MAX_LOOP_COUNT);
        d.change(Step::Previous);
        assert_eq!(d.loop_count, MAX_LOOP_COUNT - 1);
        d.change(Step::Next);
        d.change(Step::Next);
        assert_eq!(d.loop_count, 1);
        for _ in 0..3 {
            d.change(Step::Next);
        }
        assert_eq!(d.loop_count, 4);
        d.row = EXPORT_ROW;
        d.change(Step::Next);
        assert_eq!(d.loop_count, 4, "the button changes nothing");
    }

    #[test]
    fn the_lines_keep_their_width() {
        let widths = |n: u32| draft(n).lines().iter().map(String::len).collect::<Vec<_>>();
        assert_eq!(widths(1), widths(64));
    }
}
