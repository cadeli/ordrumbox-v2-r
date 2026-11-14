//! Edit > Quantize: the notes of the zone selected (else of the cursor's
//! track, else of the pattern) moved towards the lines of a grid, all the
//! way or part of it. One undo step. Notes recorded off the grid
//! (`shiftSteps`) come back to their step with a full strength.

use ordrumbox_logic::quantize::{Quantize, Scope};

use super::Session;
use super::session_labels::notes_label;
use super::session_overlay::Overlay;
use super::session_types::OverlayKey;
use super::session_zone::zone_label;
use crate::scale::Scale;
use crate::view;

const TITLE: &str = "Quantize";
/// The grids offered: lines to a beat, and how they are called.
const GRIDS: [(u32, &str); 4] = [(1, "1/4"), (2, "1/8"), (4, "1/16"), (8, "1/32")];
/// The strengths offered, in percent.
const STRENGTHS: [u32; 4] = [25, 50, 75, 100];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Grid,
    Strength,
    Apply,
}

const ROWS: [Row; 3] = [Row::Grid, Row::Strength, Row::Apply];

/// The modal's state: which notes, and how.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    scope: Scope,
    /// What the scope is, for the modal and the message.
    what: String,
    grid: usize,
    strength: usize,
    row: usize,
}

impl Draft {
    fn how(&self) -> Quantize {
        Quantize { lines_per_beat: GRIDS[self.grid].0, strength: f64::from(STRENGTHS[self.strength]) / 100.0 }
    }

    pub(super) fn lines(&self) -> Vec<String> {
        let mark = |row: usize| if row == self.row { ">" } else { " " };
        vec![
            format!("  Notes     {}", self.what),
            format!("{} Grid      {:<5}", mark(0), GRIDS[self.grid].1),
            format!("{} Strength  {:>3}%", mark(1), STRENGTHS[self.strength]),
            format!("{} [ Quantize ]", mark(2)),
            String::new(),
            "Up Down: line   Left Right: change   Enter: run   Esc: leave".into(),
        ]
    }

    /// The draft's line `line` (the Notes line, which no key reaches, is not one).
    fn row_of_line(line: usize) -> Option<usize> {
        line.checked_sub(1).filter(|&row| row < ROWS.len())
    }

    fn change(&mut self, delta: isize) {
        let wrap = |at: usize, len: usize| (at as isize + delta).rem_euclid(len as isize) as usize;
        match ROWS[self.row] {
            Row::Grid => self.grid = wrap(self.grid, GRIDS.len()),
            Row::Strength => self.strength = wrap(self.strength, STRENGTHS.len()),
            Row::Apply => {}
        }
    }
}

impl Session {
    /// Edit > Quantize.
    pub fn open_quantize(&mut self) {
        let Some(pattern) = self.pattern() else {
            self.message = "quantize: no pattern".into();
            return;
        };
        let (scope, what) = match (self.selection.zone, self.selection.cursor) {
            (Some(zone), _) => (Scope::Zone(zone), format!("the zone: {}", zone_label(&zone))),
            (None, Some(cursor)) => {
                let name = pattern.tracks.get(cursor.track).map_or("", |t| t.name.as_str());
                (Scope::Track(cursor.track), format!("track {} {name}", cursor.track + 1))
            }
            (None, None) => (Scope::Pattern, "the whole pattern".to_owned()),
        };
        let (grid, strength) = (2, STRENGTHS.len() - 1);
        self.chrome.overlay = Some(Overlay::Quantize(Draft { scope, what, grid, strength, row: 0 }));
    }

    /// A key while the modal is open; whether it was.
    pub(super) fn quantize_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::Quantize(draft)) = &mut self.chrome.overlay else { return false };
        match key {
            OverlayKey::Escape => self.chrome.overlay = None,
            OverlayKey::Move(delta) => {
                draft.row = (draft.row as isize + delta).clamp(0, ROWS.len() as isize - 1) as usize
            }
            OverlayKey::Edge(end) => draft.row = if *end { ROWS.len() - 1 } else { 0 },
            OverlayKey::Side(delta) => draft.change(*delta),
            OverlayKey::Enter if ROWS[draft.row] == Row::Apply => self.apply_quantize(),
            OverlayKey::Enter => draft.change(1),
            OverlayKey::Back | OverlayKey::Letter(_) => {}
        }
        true
    }

    /// A click while the modal is open: outside it leaves it, on a line
    /// chooses it and changes it; whether it was open.
    pub(super) fn quantize_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::Quantize(draft)) = &mut self.chrome.overlay else { return false };
        let lines = draft.lines();
        let window = (width, height);
        if !view::notice_contains(window, TITLE, &lines, scale, x, y) {
            self.chrome.overlay = None;
        } else if let Some(row) = view::notice_line_at(window, TITLE, &lines, scale, x, y).and_then(Draft::row_of_line)
        {
            draft.row = row;
            if ROWS[row] == Row::Apply {
                self.apply_quantize();
            } else {
                draft.change(1);
            }
        }
        true
    }

    fn apply_quantize(&mut self) {
        let Some(Overlay::Quantize(draft)) = self.chrome.overlay.take() else { return };
        self.undoable(|s| {
            let moved = s.editor.quantize(s.pattern_index, draft.scope, draft.how());
            if moved > 0 {
                s.edited();
            }
            let grid = GRIDS[draft.grid].1;
            let strength = STRENGTHS[draft.strength];
            s.message = format!("Quantized {} to {grid} at {strength}% — {}", notes_label(moved), draft.what);
        });
    }

    /// The modal, drawn.
    pub(super) fn draw_quantize(canvas: &mut crate::canvas::Canvas, draft: &Draft, scale: Scale) {
        view::draw_notice(canvas, TITLE, &draft.lines(), scale);
    }
}
