//! Dragging a note of the piano roll moves it: sideways in time, by the
//! steps the pointer crossed since the press, up and down in pitch, by the
//! rows. A press that never takes the note off its cell is a click.

/// The piano roll's range and middle C (`piano_roll_constants.js`).
pub const MIDI_MIN: f64 = 12.0;
pub const MIDDLE_C: f64 = 60.0;

/// A note pressed in the piano roll, which a drag moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteMove {
    /// Where the press was, px.
    press: (i32, i32),
    /// The note's cell at the press: step, row.
    from: (i64, i32),
    /// A step's width and a row's height, px.
    cell: (i32, i32),
    /// The note has left its cell since the press.
    moved: bool,
}

impl NoteMove {
    /// A press at `press` on the note at cell `from` (step, row), a cell
    /// being `cell` px (width, height).
    #[must_use]
    pub fn new(press: (i32, i32), from: (i64, i32), cell: (i32, i32)) -> Self {
        Self { press, from, cell, moved: false }
    }

    /// The note has left its cell: the press is no click.
    #[must_use]
    pub fn has_moved(&self) -> bool {
        self.moved
    }

    /// The cell the note goes to with the pointer at `(x, y)`: as far from
    /// its own as the pointer is from the press, rounded to whole steps and
    /// rows (up is higher), within `step_count` steps and `row_count` rows.
    pub fn target(&mut self, (x, y): (i32, i32), (step_count, row_count): (i64, i32)) -> (i64, i32) {
        let cells = |delta: i32, size: i32| (f64::from(delta) / f64::from(size.max(1))).round() as i32;
        let steps = i64::from(cells(x - self.press.0, self.cell.0));
        let rows = cells(self.press.1 - y, self.cell.1);
        let step = (self.from.0 + steps).clamp(0, (step_count - 1).max(0));
        let row = (self.from.1 + rows).clamp(0, (row_count - 1).max(0));
        self.moved |= (step, row) != self.from;
        (step, row)
    }
}

/// The name of MIDI note `midi`, C4 = 60.
pub fn midi_name(midi: i32) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    format!("{}{}", NAMES[midi.rem_euclid(12) as usize], midi.div_euclid(12) - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_moves_the_note_by_whole_cells_within_the_roll() {
        // Cells 20 px wide and 10 px high; the note at step 4, row 30, of 16 steps and 97 rows.
        let mut drag = NoteMove::new((100, 100), (4, 30), (20, 10));
        let bounds = (16, 97);
        assert_eq!(drag.target((105, 97), bounds), (4, 30), "less than half a cell");
        assert!(!drag.has_moved(), "still a click");
        assert_eq!(drag.target((111, 100), bounds), (5, 30), "half a step to the right");
        assert_eq!(drag.target((140, 75), bounds), (6, 33), "two steps later, three rows up");
        assert_eq!(drag.target((20, 120), bounds), (0, 28), "earlier, down");
        assert_eq!(drag.target((900, -2000), bounds), (15, 96), "kept within the roll");
        assert_eq!(drag.target((100, 100), bounds), (4, 30), "back where it was");
        assert!(drag.has_moved(), "it moved on the way: no click");
    }

    #[test]
    fn midi_names() {
        let names = [
            (-2, "A#-2"),
            (0, "C-1"),
            (1, "C#-1"),
            (11, "B-1"),
            (12, "C0"),
            (21, "A0"),
            (59, "B3"),
            (60, "C4"),
            (61, "C#4"),
            (69, "A4"),
            (108, "C8"),
            (127, "G9"),
        ];
        for (midi, name) in names {
            assert_eq!(midi_name(midi), name);
        }
    }
}
