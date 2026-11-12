//! What the grid's edits apply to: the cursor, the selected note, the zone
//! drawn with Shift+drag, the clipboard, and what the note editor follows. Indices into the pattern
//! shown; the session keeps them inside it after each edit.

use ordrumbox_core::{Note, Pattern, Track};
use ordrumbox_logic::block::{Block, Zone};
use ordrumbox_logic::roll_block::{RollBlock, RollZone};

use crate::view::Cursor;

/// What Ctrl+C copied.
#[derive(Debug, Clone)]
pub(super) enum Clipboard {
    Step(Vec<Note>),
    Track(Box<Track>),
    /// The notes of a zone, row by row.
    Block(Block),
    /// The notes of a piano roll zone.
    Roll(RollBlock),
}

/// What the note editor shows when no note is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum NoteFocus {
    /// A track was picked: its first note.
    Track(usize),
    /// The grid cursor moved to a step: its note, or none.
    Step { track: usize, beat: i32, step: i32 },
}

/// The session's selection in the pattern shown.
#[derive(Debug, Default)]
pub(super) struct Selection {
    pub(super) cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    pub(super) selected: Option<(usize, usize)>,
    /// The zone drawn over the grid: copy, cut, delete and paste take it.
    pub(super) zone: Option<Zone>,
    /// The zone drawn over the piano roll (Shift+drag): its track, steps and keys.
    pub(super) roll_zone: Option<RollZone>,
    pub(super) clipboard: Option<Clipboard>,
    /// What the note editor follows when no note is selected.
    pub(super) note_focus: Option<NoteFocus>,
    /// The default note the note editor shows for a track without notes
    /// (edits stay in it, as in the JS): its track and the note.
    pub(super) detached: Option<(usize, Note)>,
}

impl Selection {
    /// No cursor, no note selected, no zone.
    pub(super) fn clear(&mut self) {
        self.cursor = None;
        self.selected = None;
        self.zone = None;
        self.roll_zone = None;
    }

    /// The cursor at `cursor`, no note selected, no zone.
    pub(super) fn put_cursor(&mut self, cursor: Cursor) {
        self.cursor = Some(cursor);
        self.selected = None;
        self.zone = None;
        self.roll_zone = None;
    }

    /// The track the cursor is on, else the selected note's.
    pub(super) fn focus_track(&self) -> Option<usize> {
        self.cursor.map(|c| c.track).or(self.selected.map(|(t, _)| t))
    }

    /// The cursor and the selection kept inside `pattern`.
    pub(super) fn clamp(&mut self, pattern: Option<&Pattern>) {
        let tracks = pattern.map_or(0, |p| p.tracks.len());
        if let Some(cursor) = &mut self.cursor {
            if tracks == 0 {
                self.cursor = None;
            } else {
                cursor.track = cursor.track.min(tracks - 1);
            }
        }
        let valid = |&(t, n): &(usize, usize)| pattern.and_then(|p| p.tracks.get(t)).is_some_and(|t| n < t.notes.len());
        self.selected = self.selected.filter(valid);
        self.zone = self.zone.filter(|z| z.last_track < tracks);
        self.roll_zone = self.roll_zone.filter(|z| z.track < tracks);
    }

    /// The tracks reordered, track `t` now at `moved(t)`: everything follows its track.
    pub(super) fn follow_tracks(&mut self, moved: impl Fn(usize) -> usize) {
        if let Some(cursor) = &mut self.cursor {
            cursor.track = moved(cursor.track);
        }
        self.selected = self.selected.map(|(t, n)| (moved(t), n));
        // Its rows are no longer side by side.
        self.zone = None;
        self.roll_zone = self.roll_zone.map(|z| RollZone { track: moved(z.track), ..z });
        self.note_focus = self.note_focus.map(|focus| match focus {
            NoteFocus::Track(t) => NoteFocus::Track(moved(t)),
            NoteFocus::Step { track, beat, step } => NoteFocus::Step { track: moved(track), beat, step },
        });
        if let Some((t, _)) = &mut self.detached {
            *t = moved(*t);
        }
    }
}
