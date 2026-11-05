//! What the grid's edits apply to: the cursor, the selected note, the
//! clipboard, and what the note editor follows. Indices into the pattern
//! shown; the session keeps them inside it after each edit.

use ordrumbox_core::{Note, Pattern, Track};

use crate::view::Cursor;

/// What Ctrl+C copied.
#[derive(Debug, Clone)]
pub(super) enum Clipboard {
    Step(Vec<Note>),
    Track(Box<Track>),
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
    pub(super) clipboard: Option<Clipboard>,
    /// What the note editor follows when no note is selected.
    pub(super) note_focus: Option<NoteFocus>,
    /// The default note the note editor shows for a track without notes
    /// (edits stay in it, as in the JS): its track and the note.
    pub(super) detached: Option<(usize, Note)>,
}

impl Selection {
    /// No cursor, no note selected.
    pub(super) fn clear(&mut self) {
        self.cursor = None;
        self.selected = None;
    }

    /// The cursor at `cursor`, no note selected.
    pub(super) fn put_cursor(&mut self, cursor: Cursor) {
        self.cursor = Some(cursor);
        self.selected = None;
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
    }

    /// The tracks reordered, track `t` now at `moved(t)`: everything follows its track.
    pub(super) fn follow_tracks(&mut self, moved: impl Fn(usize) -> usize) {
        if let Some(cursor) = &mut self.cursor {
            cursor.track = moved(cursor.track);
        }
        self.selected = self.selected.map(|(t, n)| (moved(t), n));
        self.note_focus = self.note_focus.map(|focus| match focus {
            NoteFocus::Track(t) => NoteFocus::Track(moved(t)),
            NoteFocus::Step { track, beat, step } => NoteFocus::Step { track: moved(track), beat, step },
        });
        if let Some((t, _)) = &mut self.detached {
            *t = moved(*t);
        }
    }
}
