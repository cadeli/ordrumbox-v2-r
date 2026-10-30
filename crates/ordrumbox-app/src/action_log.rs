//! What was done since the app started: the outcome of each action and each
//! undo step, shown newest first in a panel opened from the status line.

use crate::canvas::{Canvas, text_width};
use crate::panel::Rect;
use crate::theme;

/// Lines kept; the oldest go first.
const MAX_LINES: usize = 1000;

/// What the session shows at a frame, compared to the frame before.
#[derive(Debug, Clone, Copy)]
pub struct Seen<'a> {
    pub message: &'a str,
    /// The editor's revision.
    pub revision: u64,
    /// Undo steps there are.
    pub steps: usize,
    /// What undo would revert.
    pub undo: Option<&'a str>,
    /// A drag is going on: its outcomes are one line.
    pub dragging: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ActionLog {
    lines: Vec<String>,
    message: String,
    revision: u64,
    steps: usize,
    /// The last line is the open undo step, which later edits may extend.
    last_is_step: bool,
    /// The last line is the outcome of the drag going on.
    last_is_drag: bool,
}

impl ActionLog {
    /// Add what changed since the last call. Most actions set a message; an
    /// edit without one (a slider) is known by its undo step.
    pub fn note(&mut self, seen: Seen) {
        if seen.message != self.message {
            self.message = seen.message.to_owned();
            if !seen.message.is_empty() {
                self.put(seen.message, seen.dragging && self.last_is_drag);
                self.last_is_drag = seen.dragging;
                self.last_is_step = false;
            }
        } else if seen.revision != self.revision {
            match (seen.steps.cmp(&self.steps), seen.undo) {
                (std::cmp::Ordering::Greater, Some(undo)) => {
                    self.put(undo, false);
                    self.last_is_step = true;
                    self.last_is_drag = false;
                }
                // A coalesced edit: the same step, its description the newest one.
                (std::cmp::Ordering::Equal, Some(undo)) if self.last_is_step => self.put(undo, true),
                // A second undo with the same description: the message did not change.
                (std::cmp::Ordering::Less, _) => {
                    self.put(seen.message, false);
                    self.last_is_step = false;
                    self.last_is_drag = false;
                }
                _ => {}
            }
        }
        self.revision = seen.revision;
        self.steps = seen.steps;
    }

    fn put(&mut self, line: &str, replace_last: bool) {
        if replace_last && let Some(last) = self.lines.last_mut() {
            *last = line.to_owned();
            return;
        }
        if self.lines.len() == MAX_LINES {
            self.lines.remove(0);
        }
        self.lines.push(line.to_owned());
    }

    /// The rows of the panel, newest action first; a long line takes several.
    fn rows(&self, chars_per_row: usize) -> Vec<String> {
        let wrap = |line: &String| -> Vec<String> {
            let chars: Vec<char> = line.chars().collect();
            chars.chunks(chars_per_row.max(1)).map(|c| c.iter().collect()).collect()
        };
        self.lines.iter().rev().flat_map(wrap).collect()
    }
}

/// Where the panel and its rows are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogLayout {
    panel: Rect,
    list_top: i32,
    row: i32,
    rows: usize,
    chars_per_row: usize,
    scale: i32,
}

impl LogLayout {
    pub fn new(width: i32, height: i32, scale: i32) -> Self {
        let margin = 40 * scale;
        let (w, h) = ((width - 2 * margin).max(80 * scale), (height - 2 * margin).max(80 * scale));
        let (x, y) = ((width - w) / 2, (height - h) / 2);
        let row = 12 * scale;
        let list_top = y + 24 * scale;
        let rows = usize::try_from((y + h - 20 * scale - list_top) / row).unwrap_or(0).max(1);
        // Room for the margins and the scroll bar.
        let chars_per_row = ((w - 32 * scale) / text_width("W", scale).max(1)).max(4) as usize;
        Self { panel: Rect { x, y, w, h }, list_top, row, rows, chars_per_row, scale }
    }

    /// Inside the panel.
    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.panel.contains(x, y)
    }

    /// The largest scroll: the last rows at the bottom of the panel.
    pub fn max_scroll(&self, log: &ActionLog) -> usize {
        log.rows(self.chars_per_row).len().saturating_sub(self.rows)
    }
}

/// The panel, its rows from `scroll` down, with a scroll bar when they do not all fit.
pub fn draw(canvas: &mut Canvas, log: &ActionLog, scroll: usize, layout: &LogLayout) {
    let s = layout.scale;
    let Rect { x, y, w, h } = layout.panel;
    canvas.fill_rect(x - 4 * s, y - 4 * s, w + 8 * s, h + 8 * s, theme::SHADE);
    canvas.fill_rect(x, y, w, h, theme::LINE);
    canvas.fill_rect(x + s, y + s, w - 2 * s, h - 2 * s, theme::PANEL);
    let text_x = x + 10 * s;
    canvas.text(text_x, y + 8 * s, "History", s, theme::PINK);
    let rows = log.rows(layout.chars_per_row);
    if rows.is_empty() {
        canvas.text(text_x, layout.list_top, "(nothing done yet)", s, theme::MUTED);
    }
    let scroll = scroll.min(layout.max_scroll(log));
    for (i, row) in rows.iter().skip(scroll).take(layout.rows).enumerate() {
        canvas.text(text_x, layout.list_top + i as i32 * layout.row, row, s, theme::TEXT);
    }
    if rows.len() > layout.rows {
        let track = layout.rows as i32 * layout.row;
        let thumb = (track * layout.rows as i32 / rows.len() as i32).max(8 * s);
        let top = (track - thumb) * scroll as i32 / layout.max_scroll(log).max(1) as i32;
        let bar_x = x + w - 8 * s;
        canvas.fill_rect(bar_x, layout.list_top, 3 * s, track, theme::LINE);
        canvas.fill_rect(bar_x, layout.list_top + top, 3 * s, thumb, theme::PINK);
    }
    let hint = "wheel, arrows, PAGE UP/DOWN, HOME/END: scroll   ESC or click outside: close";
    canvas.text(text_x, y + h - 14 * s, hint, s, theme::MUTED);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen<'a>(message: &'a str, revision: u64, steps: usize, undo: Option<&'a str>) -> Seen<'a> {
        Seen { message, revision, steps, undo, dragging: false }
    }

    #[test]
    fn logs_messages_and_steps_without_message() {
        let mut log = ActionLog::default();
        log.note(seen("", 0, 0, None));
        assert!(log.lines.is_empty());
        log.note(seen("saved", 0, 0, None));
        log.note(seen("saved", 0, 0, None)); // the next frame
        assert_eq!(log.lines, ["saved"]);
        // A slider: no message; its drag is one step, whose description follows.
        log.note(seen("saved", 1, 1, Some("BPM 120")));
        log.note(seen("saved", 2, 1, Some("BPM 121")));
        assert_eq!(log.lines, ["saved", "BPM 121"]);
        // An edit with a message is one line, not two.
        log.note(seen("Added note", 3, 2, Some("Add note")));
        assert_eq!(log.lines, ["saved", "BPM 121", "Added note"]);
        // Two undos with the same description.
        log.note(seen("Undo: x", 4, 1, Some("BPM 121")));
        log.note(seen("Undo: x", 5, 0, None));
        assert_eq!(log.lines[3..], ["Undo: x", "Undo: x"]);
    }

    #[test]
    fn a_drag_is_one_line() {
        let mut log = ActionLog::default();
        for message in ["pitch 1", "pitch 2", "pitch 3"] {
            log.note(Seen { dragging: true, ..seen(message, 0, 0, None) });
        }
        assert_eq!(log.lines, ["pitch 3"]);
        log.note(seen("pitch 4", 0, 0, None));
        assert_eq!(log.lines, ["pitch 3", "pitch 4"]);
    }

    #[test]
    fn keeps_the_newest_lines() {
        let mut log = ActionLog::default();
        for i in 0..MAX_LINES + 2 {
            log.note(seen(&i.to_string(), 0, 0, None));
        }
        assert_eq!(log.lines.len(), MAX_LINES);
        assert_eq!(log.lines[0], "2");
    }

    #[test]
    fn newest_first_wrapped_and_scrollable() {
        let mut log = ActionLog::default();
        log.note(seen("old", 0, 0, None));
        log.note(seen("abcdefgh", 0, 0, None));
        assert_eq!(log.rows(3), ["abc", "def", "gh", "old"]);

        let layout = LogLayout::new(800, 600, 1);
        assert_eq!(layout.max_scroll(&log), 0);
        for i in 0..200 {
            log.note(seen(&i.to_string(), 0, 0, None));
        }
        assert_eq!(layout.max_scroll(&log), 202 - layout.rows);
        assert!(layout.contains(400, 300));
        assert!(!layout.contains(1, 1));
        let mut pixels = vec![0; 800 * 600];
        draw(&mut Canvas::new(&mut pixels, 800, 600), &log, 10_000, &layout);
    }
}
