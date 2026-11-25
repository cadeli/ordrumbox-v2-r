//! Edit > Quantize and Edit > Humanize: the notes of the zone selected (else
//! of the cursor's track, else of the pattern) moved towards the lines of a
//! grid, or off them and to other velocities by a random amount. A modal
//! chooses two settings, then runs the tool as one undo step. Notes recorded
//! off the grid (`shiftSteps`) come back to their step with a full strength.

use ordrumbox_logic::humanize::Humanize;
use ordrumbox_logic::quantize::{Quantize, Scope};

use super::Session;
use super::session_labels::notes_label;
use super::session_overlay::Overlay;
use super::session_types::{OverlayKey, RowClick, RowKey};
use super::session_zone::zone_label;
use crate::panel::Step;
use crate::scale::Scale;
use crate::view;

/// The grids offered: lines to a beat, and how they are called.
const GRIDS: [(u32, &str); 4] = [(1, "1/4"), (2, "1/8"), (4, "1/16"), (8, "1/32")];
/// The strengths offered, in percent.
const STRENGTHS: [u32; 4] = [25, 50, 75, 100];
/// The ranges Humanize offers, either way, in percent: of a step for the
/// timing, of the note's velocity for the velocity.
const SPREADS: [u32; 5] = [0, 5, 10, 20, 30];

/// The tool the modal runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tool {
    Quantize,
    Humanize,
}

impl Tool {
    fn title(self) -> &'static str {
        match self {
            Self::Quantize => "Quantize",
            Self::Humanize => "Humanize",
        }
    }

    /// How many values each setting has.
    fn value_counts(self) -> [usize; 2] {
        match self {
            Self::Quantize => [GRIDS.len(), STRENGTHS.len()],
            Self::Humanize => [SPREADS.len(); 2],
        }
    }

    /// The values the modal opens with: sixteenths at 100 %; ±10 % and ±10 %.
    fn defaults(self) -> [usize; 2] {
        match self {
            Self::Quantize => [2, STRENGTHS.len() - 1],
            Self::Humanize => [2, 2],
        }
    }

    /// The two setting lines, unmarked, for values `values`.
    fn setting_lines(self, values: [usize; 2]) -> [String; 2] {
        match self {
            Self::Quantize => {
                [format!("Grid      {:<5}", GRIDS[values[0]].1), format!("Strength  {:>3}%", STRENGTHS[values[1]])]
            }
            Self::Humanize => [
                format!("Timing   ±{:>3}% of a step", SPREADS[values[0]]),
                format!("Velocity ±{:>3}%", SPREADS[values[1]]),
            ],
        }
    }
}

/// The lines a key reaches: the two settings, then the button.
const ROW_COUNT: usize = 3;
const APPLY_ROW: usize = 2;

/// The modal's state: which tool, which notes, and how.
#[derive(Debug, Clone)]
pub(super) struct Draft {
    tool: Tool,
    scope: Scope,
    /// What the scope is, for the modal and the message.
    what: String,
    values: [usize; 2],
    row: usize,
}

impl Draft {
    fn quantize(&self) -> Quantize {
        Quantize { lines_per_beat: GRIDS[self.values[0]].0, strength: f64::from(STRENGTHS[self.values[1]]) / 100.0 }
    }

    fn humanize(&self) -> Humanize {
        let share = |index: usize| f64::from(SPREADS[index]) / 100.0;
        Humanize { timing_steps: share(self.values[0]), velocity: share(self.values[1]) }
    }

    pub(super) fn lines(&self) -> Vec<String> {
        let mark = |row: usize| if row == self.row { ">" } else { " " };
        let [first, second] = self.tool.setting_lines(self.values);
        vec![
            format!("  Notes     {}", self.what),
            format!("{} {first}", mark(0)),
            format!("{} {second}", mark(1)),
            format!("{} [ {} ]", mark(APPLY_ROW), self.tool.title()),
            String::new(),
            "Up Down: line   Left Right: change   Enter: run   Esc: leave".into(),
        ]
    }

    /// The value of the chosen line one `step` away, round its choices.
    fn change(&mut self, step: Step) {
        let Some(count) = self.tool.value_counts().get(self.row).copied() else { return };
        let value = &mut self.values[self.row];
        *value = step.target_index(*value, count).unwrap_or(*value);
    }
}

impl Session {
    /// Edit > Quantize or Edit > Humanize: the modal of `tool`.
    pub(super) fn open_note_tool(&mut self, tool: Tool) {
        let Some(pattern) = self.pattern() else {
            self.message = format!("{}: no pattern", tool.title().to_lowercase());
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
        let values = tool.defaults();
        self.chrome.overlay = Some(Overlay::NoteTool(Draft { tool, scope, what, values, row: 0 }));
    }

    /// A key while the modal is open; whether it was.
    pub(super) fn note_tool_key(&mut self, key: &OverlayKey) -> bool {
        let Some(Overlay::NoteTool(draft)) = &mut self.chrome.overlay else { return false };
        match key.row_key(draft.row, APPLY_ROW) {
            RowKey::Close => self.chrome.overlay = None,
            RowKey::Row(row) => draft.row = row,
            RowKey::Change(step) => draft.change(step),
            RowKey::Run => self.apply_note_tool(),
            RowKey::Ignored => {}
        }
        true
    }

    /// A click while the modal is open: outside it leaves it, on a line
    /// chooses it and changes it; whether it was open.
    pub(super) fn note_tool_click(&mut self, width: i32, height: i32, scale: Scale, x: i32, y: i32) -> bool {
        let Some(Overlay::NoteTool(draft)) = &mut self.chrome.overlay else { return false };
        match RowClick::at((width, height), (draft.tool.title(), &draft.lines()), scale, (x, y), ROW_COUNT) {
            RowClick::Outside => self.chrome.overlay = None,
            // A click on a row is Enter on it.
            RowClick::Row(row) => {
                draft.row = row;
                self.note_tool_key(&OverlayKey::Enter);
            }
            RowClick::Elsewhere => {}
        }
        true
    }

    fn apply_note_tool(&mut self) {
        let Some(Overlay::NoteTool(draft)) = self.chrome.overlay.take() else { return };
        self.undoable(|s| {
            let (changed, done) = match draft.tool {
                Tool::Quantize => {
                    let moved = s.editor.quantize(s.pattern_index, draft.scope, draft.quantize());
                    let (grid, strength) = (GRIDS[draft.values[0]].1, STRENGTHS[draft.values[1]]);
                    (moved, format!("Quantized {} to {grid} at {strength}%", notes_label(moved)))
                }
                Tool::Humanize => {
                    let how = draft.humanize();
                    let changed = s.editor.humanize(s.pattern_index, draft.scope, how, &mut s.generators.rng);
                    let (timing, velocity) = (SPREADS[draft.values[0]], SPREADS[draft.values[1]]);
                    let done =
                        format!("Humanized {} by ±{timing}% of a step, ±{velocity}% velocity", notes_label(changed));
                    (changed, done)
                }
            };
            if changed > 0 {
                s.edited();
            }
            s.message = format!("{done} — {}", draft.what);
        });
    }

    /// The modal, drawn.
    pub(super) fn draw_note_tool(canvas: &mut crate::canvas::Canvas, draft: &Draft, scale: Scale) {
        view::draw_notice(canvas, draft.tool.title(), &draft.lines(), scale);
    }
}
