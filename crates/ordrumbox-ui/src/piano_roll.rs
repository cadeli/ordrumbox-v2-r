//! The piano roll (`piano_roll_panel.js` and its sections), in the place of
//! the grid: one track's notes on a grid of steps (X, 4 beats across, or as
//! many as the scroll bar under it zooms to) and pitches (Y, MIDI 12 to 108, a keyboard on the left), with the
//! notes' retriggers, arpeggio steps and euclidean fills as ghosts, the loop
//! point, the cursor, the selected note, and while playing the playhead and
//! the notes sounding.
//!
//! [`RollLayout`] places everything; drawing and mouse hit-testing both use it.

use crate::scale::Scale;
use crate::scroll_thumb::{self, Edge};
use ordrumbox_core::note_positions::{SubKind, SubPosition};
use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::track::MAX_STEPS_PER_BEAT;
use ordrumbox_core::{Note, Track};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::frame;
use crate::note_drag::{MIDDLE_C, MIDI_MIN, midi_name};
use crate::panel::{Rect, stroke};
use crate::theme;
use crate::theme::{ZONE, ZONE_OPACITY};

/// Height of a key row, px per scale unit (`NOTE_HEIGHT`).
const NOTE_HEIGHT: i32 = 14;
/// Width of the keyboard (`KEYS_COLUMN_WIDTH`).
const KEYS_WIDTH: i32 = 80;
/// Height of the scroll bar under the grid, and the gap above it, per scale unit.
const BAR_HEIGHT: i32 = 12;
const BAR_GAP: i32 = 4;
/// The shortest thumb of the scroll bar, per scale unit: room for its two handles and a grip.
const BAR_MIN_THUMB: i32 = 3 * scroll_thumb::ZOOM_HANDLE;
/// The narrowest a note is drawn, and the width of its right edge, which a
/// drag draws out, px per scale unit.
const MIN_NOTE_WIDTH: i32 = 3;
const NOTE_EDGE: i32 = 5;
/// Beats the grid shows across until the scroll bar zooms (`BEATS_PER_PAGE`).
pub const BEATS_PER_PAGE: i32 = 4;
/// Keys, MIDI 12 to 108 (`TOTAL_KEYS`).
pub const TOTAL_KEYS: i32 = 97;
/// The black keys, by pitch class.
const BLACK_KEYS: [i32; 5] = [1, 3, 6, 8, 10];

fn background_color() -> Rgb {
    theme::bg()
}
fn row_white_color() -> Rgb {
    theme::cell()
}
fn row_black_color() -> Rgb {
    theme::panel_2()
}
/// The row of a C: the white row tinted, so each octave stands out.
fn row_c_color() -> Rgb {
    theme::mix(theme::cell(), theme::FOCUS, 0.22)
}
fn column_beat_color() -> Rgb {
    theme::line()
}
fn column_half_color() -> Rgb {
    theme::cell_beat()
}
fn column_step_color() -> Rgb {
    theme::panel_2()
}
fn octave_color() -> Rgb {
    theme::mix(theme::line(), theme::FOCUS, 0.6)
}
fn key_white_color() -> Rgb {
    theme::text()
}
fn key_black_color() -> Rgb {
    theme::panel()
}

const PLAYING: Rgb = theme::LIVE;
const SELECTED: Rgb = theme::WHITE;
const CURSOR: Rgb = theme::FOCUS;
const LOOP_MARK: Rgb = theme::VALUE;
const PLAYHEAD: Rgb = theme::LIVE;
const TRIG_RAND: Rgb = theme::CHANCE;
const TRIG_FIXED: Rgb = theme::VALUE;
/// How much lighter the top of a note is than its colour, and how much darker the bottom.
pub(crate) const NOTE_LIGHT: f64 = 0.18;
pub(crate) const NOTE_DARK: f64 = 0.12;
/// Opacity (0–255) of the shadow under a note and of the light edge on top.
const NOTE_SHADOW: u8 = 70;
const NOTE_SHEEN: u8 = 80;
/// A playing note is lighter, and glows around it.
pub(crate) const NOTE_LIGHT_PLAYING: f64 = 0.35;
const NOTE_GLOW: u8 = 90;
/// Opacity of the velocity bar.
const VELOCITY_BAR: u8 = 150;

/// The MIDI note of row `row` (0: the lowest key).
pub fn row_midi(row: i32) -> i32 {
    MIDI_MIN as i32 + row
}

/// The row of middle C.
const MIDDLE_C_ROW: i32 = (MIDDLE_C - MIDI_MIN) as i32;

/// The key of row `row`: semitones from middle C (`roll_block::note_key`).
pub fn row_key(row: i32) -> i32 {
    row - MIDDLE_C_ROW
}

/// The row of key `key`.
pub fn key_row(key: i32) -> i32 {
    key + MIDDLE_C_ROW
}

/// The row a note of `track` is on (may be off the keyboard).
pub fn note_row(note: &Note, track: &Track) -> i32 {
    (MIDDLE_C + track.transpose + note.pitch - MIDI_MIN).round() as i32
}

/// Steps per beat of `track`, as the roll counts them.
pub fn steps_per_beat(track: &Track) -> i32 {
    track.steps_per_beat.clamp(1, MAX_STEPS_PER_BEAT)
}

/// Opacity a note is drawn at: 0.5 to 1 (the velocity bar tells the rest).
fn note_opacity(velocity: f64) -> f64 {
    0.5 + velocity * 0.5
}

/// What the piano roll shows.
#[derive(Debug)]
pub struct RollView<'a> {
    pub track: &'a Track,
    /// The track's colour (its notes).
    pub color: Rgb,
    /// The pattern's length, beats.
    pub beat_count: i32,
    /// First beat shown, and how many beats are across.
    pub first_beat: i32,
    pub shown_beats: i32,
    /// Highest row shown.
    pub top_row: i32,
    /// The selected note (an index in the track's notes).
    pub selected: Option<usize>,
    /// The cursor: step, row.
    pub cursor: Option<(i64, i32)>,
    /// The step playing, while playing.
    pub playhead: Option<i64>,
    /// Sub-positions of each note.
    pub subs: Vec<Vec<SubPosition>>,
    /// The zone drawn with Shift+drag.
    pub zone: Option<RollSpan>,
}

/// A zone of the roll: steps `start_step..end_step`, rows `low_row..=high_row`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RollSpan {
    pub start_step: i64,
    pub end_step: i64,
    pub low_row: i32,
    pub high_row: i32,
}

impl RollView<'_> {
    /// Steps in the track.
    pub fn total_steps(&self) -> i64 {
        i64::from(self.beat_count) * i64::from(steps_per_beat(self.track))
    }

    /// First step shown, and the step after the last.
    pub fn page_steps(&self) -> (i64, i64) {
        let spb = i64::from(steps_per_beat(self.track));
        let start = i64::from(self.first_beat.clamp(0, self.beat_count - 1)) * spb;
        (start, (start + i64::from(self.shown_beats) * spb).min(self.total_steps()))
    }

    /// Where the track loops: its loop point, else its end.
    fn loop_length(&self) -> i64 {
        self.track.loop_length.map_or(self.total_steps(), i64::from).max(1)
    }

    /// Whether note `n` sounds on `step`.
    fn sounding(&self, n: usize, step: i64) -> bool {
        let note = &self.track.notes[n];
        let loop_length = self.loop_length();
        let base = absolute_step(note.beat, note.step, steps_per_beat(self.track));
        if base >= loop_length {
            return false;
        }
        step % loop_length == base || self.subs[n].iter().any(|s| s.step < loop_length && step % loop_length == s.step)
    }
}

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollHit {
    /// A key of the keyboard: its MIDI note.
    Key(i32),
    /// A cell of the grid: absolute step, row.
    Cell { step: i64, row: i32 },
    /// The scroll bar: a drag scrolls, or zooms at a handle (`Some`).
    ScrollBar(Option<Edge>),
}

/// Where the piano roll's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollLayout {
    scale: Scale,
    /// The title line.
    pub title: Rect,
    pub keys: Rect,
    pub grid: Rect,
    /// The beats' scroll bar, under the grid, whose ends zoom.
    pub bar: Rect,
    /// Width of a step.
    pub cell: i32,
    pub row: i32,
}

impl RollLayout {
    /// The piano roll on a `width` × `height` window (`width` without the
    /// track editor), for a track of `spb` steps per beat, `shown_beats` across.
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize, (spb, shown_beats): (i32, i32)) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = frame::body_top(scale);
        let bottom = frame::footer_top(height, scale, status_lines) - margin / 2;
        let title = Rect::new(margin, top, (width - 2 * margin).max(0), 12 * s);
        let grid_top = top + 18 * s;
        let bar_h = BAR_HEIGHT * s;
        let h = (bottom - grid_top - bar_h - BAR_GAP * s).max(0);
        let keys = Rect::new(margin, grid_top, KEYS_WIDTH * s, h);
        let grid_x = keys.x + keys.w;
        let grid_w = (width - margin - grid_x).max(0);
        let cell = (grid_w / (shown_beats.max(1) * spb.max(1))).max(1);
        Self {
            scale,
            title,
            keys,
            grid: Rect::new(grid_x, grid_top, grid_w, h),
            bar: Rect::new(grid_x, grid_top + h + BAR_GAP * s, grid_w, bar_h),
            cell,
            row: NOTE_HEIGHT * s,
        }
    }

    /// Rows that fit.
    pub fn visible_rows(&self) -> i32 {
        (self.grid.h / self.row).clamp(1, TOTAL_KEYS)
    }

    /// The highest row shown so that `row` is in the middle.
    pub fn centered_top(&self, row: i32) -> i32 {
        self.clamp_top(row + self.visible_rows() / 2)
    }

    /// `top` kept so that the keyboard fills the view.
    pub fn clamp_top(&self, top: i32) -> i32 {
        top.clamp((self.visible_rows() - 1).min(TOTAL_KEYS - 1), TOTAL_KEYS - 1)
    }

    /// The bar's thumb `(start, len)`: the beats shown, of the pattern's.
    #[must_use]
    pub fn bar_thumb(&self, view: &RollView) -> (i32, i32) {
        let first = view.page_steps().0 / i64::from(steps_per_beat(view.track));
        let min = BAR_MIN_THUMB * self.scale;
        let track = (self.bar.x, self.bar.w);
        scroll_thumb::thumb(track, first as usize, self.shown(view), view.beat_count as usize, min)
    }

    /// The beats across the grid, the last page's rest left out.
    #[must_use]
    pub fn shown(&self, view: &RollView) -> usize {
        let (start, end) = view.page_steps();
        let spb = i64::from(steps_per_beat(view.track));
        ((end - start + spb - 1) / spb).max(1) as usize
    }

    /// The first beat that puts the thumb centred on `x`.
    #[must_use]
    pub fn first_beat_at(&self, view: &RollView, x: i32) -> i32 {
        let min = BAR_MIN_THUMB * self.scale;
        scroll_thumb::first_at((self.bar.x, self.bar.w), x, self.shown(view), view.beat_count as usize, min) as i32
    }

    /// The beat (fractional) the bar's track puts at `x`.
    #[must_use]
    pub fn track_beat(&self, view: &RollView, x: i32) -> f64 {
        f64::from(x - self.bar.x) / f64::from(self.bar.w.max(1)) * f64::from(view.beat_count)
    }

    /// The beat (fractional) the grid puts at `x`.
    #[must_use]
    pub fn beat_under(&self, view: &RollView, x: i32) -> f64 {
        let spb = f64::from(steps_per_beat(view.track));
        view.page_steps().0 as f64 / spb + f64::from(x - self.grid.x) / (f64::from(self.cell) * spb)
    }

    fn row_y(&self, top_row: i32, row: i32) -> i32 {
        self.grid.y + (top_row - row) * self.row
    }

    fn step_x(&self, view: &RollView, step: i64) -> i32 {
        self.grid.x + (step - view.page_steps().0) as i32 * self.cell
    }

    /// The lowest row shown.
    fn first_row(&self, view: &RollView) -> i32 {
        (view.top_row - self.visible_rows() + 1).max(0)
    }

    /// The cell of the page nearest to `(x, y)`: step, row (a drag past the
    /// grid stops at its edge).
    pub fn nearest_cell(&self, view: &RollView, x: i32, y: i32) -> Option<(i64, i32)> {
        let (start, end) = view.page_steps();
        if end <= start {
            return None;
        }
        let column = i64::from((x - self.grid.x).max(0) / self.cell.max(1)).min(end - start - 1);
        let row = (view.top_row - (y - self.grid.y).max(0) / self.row.max(1)).clamp(self.first_row(view), view.top_row);
        Some((start + column, row))
    }

    fn shown_row(&self, view: &RollView, row: i32) -> bool {
        row <= view.top_row && row > view.top_row - self.visible_rows() && (0..TOTAL_KEYS).contains(&row)
    }

    /// Where note `n` of the view's track is drawn, if it is on the page:
    /// from its step, moved by its shift, as wide as it is long (up to the
    /// end of the page).
    pub fn note_rect(&self, view: &RollView, n: usize) -> Option<Rect> {
        let note = view.track.notes.get(n)?;
        let step = absolute_step(note.beat, note.step, steps_per_beat(view.track));
        let (start, end) = view.page_steps();
        let row = note_row(note, view.track);
        if step < start || step >= end || !self.shown_row(view, row) {
            return None;
        }
        let s = self.scale.px();
        let cell = f64::from(self.cell);
        let x = (self.step_x(view, step) + (note.late_steps() * cell).round() as i32).max(self.grid.x);
        let y = self.row_y(view.top_row, row);
        let right = (x + (note.held_steps() * cell).round() as i32).min(self.step_x(view, end));
        Some(Rect::new(x + s, y + s, (right - x - 2 * s).max(MIN_NOTE_WIDTH * s), self.row - 2 * s))
    }

    /// The note drawn at `(x, y)` (the last one, drawn over the others), and
    /// whether the pointer is on its right edge, where a drag draws it out.
    pub fn note_at(&self, view: &RollView, x: i32, y: i32) -> Option<(usize, bool)> {
        (0..view.track.notes.len()).rev().find_map(|n| {
            let r = self.note_rect(view, n).filter(|r| r.contains(x, y))?;
            let edge = (NOTE_EDGE * self.scale).min(r.w / 3);
            Some((n, x >= r.x + r.w - edge))
        })
    }

    /// What is at `(x, y)`.
    pub fn hit(&self, view: &RollView, x: i32, y: i32) -> Option<RollHit> {
        if self.bar.contains(x, y) {
            let handle = scroll_thumb::ZOOM_HANDLE * self.scale;
            return Some(RollHit::ScrollBar(scroll_thumb::handle_at(x, self.bar_thumb(view), handle)));
        }
        if y < self.grid.y || y >= self.grid.y + self.grid.h {
            return None;
        }
        let row = view.top_row - (y - self.grid.y) / self.row;
        if !(0..TOTAL_KEYS).contains(&row) {
            return None;
        }
        if self.keys.contains(x, y) {
            return Some(RollHit::Key(row_midi(row)));
        }
        if x < self.grid.x {
            return None;
        }
        let (start, end) = view.page_steps();
        let step = start + i64::from((x - self.grid.x) / self.cell);
        (step < end).then_some(RollHit::Cell { step, row })
    }
}

#[path = "piano_roll/draw.rs"]
mod draw;
pub use draw::draw;

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn library() -> Library {
        Library::parse(
            r#"{"patterns":[{"beatCount":6,"tracks":[{"name":"BASS","pitch":-12,"beatCount":6,"loopLength":20,
                "notes":[{"beat":0,"pitch":0},{"beat":1,"step":2,"pitch":7,"velocity":0.3,"prob":0.5},
                         {"beat":4,"pitch":0,"retriggerCount":3,"rate":4}]}]}]}"#,
        )
        .unwrap()
        .0
    }

    fn view(track: &Track, first_beat: i32, top_row: i32) -> RollView<'_> {
        RollView {
            track,
            color: theme::track(0),
            beat_count: 6,
            first_beat,
            shown_beats: 4,
            top_row,
            selected: Some(1),
            cursor: Some((3, 40)),
            playhead: Some(16),
            subs: vec![Vec::new(); 3],
            zone: Some(RollSpan { start_step: 2, end_step: 20, low_row: 30, high_row: 44 }),
        }
    }

    #[test]
    fn hit_testing_matches_the_drawing() {
        let library = library();
        let track = &library.patterns[0].tracks[0];
        let layout = RollLayout::new(1100, 680, Scale::ONE, 0, (4, 4));
        // Track BASS is an octave down: its C is MIDI 48, row 36.
        let row = note_row(&track.notes[0], track);
        assert_eq!(row, 36);
        let top = layout.centered_top(row);
        let v = view(track, 0, top);
        assert_eq!(v.page_steps(), (0, 16));
        let r = layout.note_rect(&v, 0).unwrap();
        assert_eq!(layout.hit(&v, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 0, row: 36 }));
        let r = layout.note_rect(&v, 1).unwrap();
        assert_eq!(layout.hit(&v, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 6, row: 43 }));
        assert_eq!(layout.hit(&v, layout.keys.x + 2, r.y + 2), Some(RollHit::Key(row_midi(43))));
        // Note 2 (beat 4) is on page 2.
        assert!(layout.note_rect(&v, 2).is_none());
        let v2 = view(track, 4, top);
        assert_eq!(v2.page_steps(), (16, 24));
        let r = layout.note_rect(&v2, 2).unwrap();
        assert_eq!(layout.hit(&v2, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 16, row: 36 }));
        // A drag past the grid stops at the page's last step and the lowest row shown.
        let lowest = layout.first_row(&v2);
        assert_eq!(layout.nearest_cell(&v2, 5000, 5000), Some((23, lowest)));
        assert_eq!(layout.nearest_cell(&v2, 0, 0), Some((16, top)));
        // Past the last step of a short page: nothing.
        assert_eq!(layout.hit(&v2, layout.grid.x + 8 * layout.cell + 2, r.y + 2), None);
        // The bar: the thumb under the last beats, its ends the handles.
        let (start, len) = layout.bar_thumb(&v2);
        let y = layout.bar.y + 2;
        assert_eq!(layout.hit(&v2, start + len / 2, y), Some(RollHit::ScrollBar(None)));
        assert_eq!(layout.hit(&v2, start + len - 1, y), Some(RollHit::ScrollBar(Some(Edge::End))));
        assert!(start + len <= layout.bar.x + layout.bar.w);
    }

    #[test]
    fn scroll_stays_on_the_keyboard() {
        let layout = RollLayout::new(1100, 680, Scale::ONE, 0, (4, 4));
        let rows = layout.visible_rows();
        assert!(rows < TOTAL_KEYS);
        assert_eq!(layout.centered_top(0), rows - 1);
        assert_eq!(layout.centered_top(96), 96);
        assert_eq!(layout.centered_top(48), 48 + rows / 2);
    }

    #[test]
    fn sounding_notes_follow_the_loop() {
        let library = library();
        let track = &library.patterns[0].tracks[0];
        let v = view(track, 0, 60);
        // Loop at step 20: step 20 plays step 0 again; note 2 (step 16) at 16 and 36.
        assert!(v.sounding(0, 20) && v.sounding(2, 16) && v.sounding(2, 36) && !v.sounding(1, 20));
    }

    #[test]
    fn draws_without_panicking_at_any_size() {
        let library = library();
        let track = &library.patterns[0].tracks[0];
        for (w, h) in [(1, 1), (40, 30), (1100, 680)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = RollLayout::new(w as i32, h as i32, Scale::ONE, 1, (4, 4));
            for first_beat in [0, 4] {
                draw(&mut canvas, &view(track, first_beat, layout.centered_top(40)), &layout, "Piano Roll - BASS");
            }
        }
    }
}
