//! The piano roll (`piano_roll_panel.js` and its sections), in the place of
//! the grid: one track's notes on a grid of steps (X, a page of 4 beats at a
//! time) and pitches (Y, MIDI 12 to 108, a keyboard on the left), with the
//! notes' retriggers, arpeggio steps and euclidean fills as ghosts, the loop
//! point, the cursor, the selected note, and while playing the playhead and
//! the notes sounding.
//!
//! [`RollLayout`] places everything; drawing and mouse hit-testing both use it.

use crate::scale::Scale;
use ordrumbox_core::note_positions::{SubKind, SubPosition};
use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::track::MAX_STEPS_PER_BEAT;
use ordrumbox_core::{Note, Track};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::frame;
use crate::note_drag::{MIDDLE_C, MIDI_MIN, midi_name};
use crate::panel::Rect;
use crate::theme;
use crate::view::{ZONE, ZONE_OPACITY};

/// Height of a key row, px per scale unit (`NOTE_HEIGHT`).
const NOTE_HEIGHT: i32 = 14;
/// Width of the keyboard, and the narrowest cell (`KEYS_COLUMN_WIDTH`, `MIN_CELL_WIDTH`).
const KEYS_WIDTH: i32 = 80;
const MIN_CELL_WIDTH: i32 = 16;
/// Beats a page shows (`BEATS_PER_PAGE`).
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
    theme::line()
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
    /// Page shown.
    pub page: i32,
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

    /// Pages of the pattern.
    pub fn pages(&self) -> i32 {
        ((self.beat_count + BEATS_PER_PAGE - 1) / BEATS_PER_PAGE).max(1)
    }

    /// First step of the page shown, and the step after its last.
    pub fn page_steps(&self) -> (i64, i64) {
        let per_page = i64::from(BEATS_PER_PAGE * steps_per_beat(self.track));
        let start = i64::from(self.page) * per_page;
        (start, (start + per_page).min(self.total_steps()))
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
    Cell {
        step: i64,
        row: i32,
    },
    PreviousPage,
    NextPage,
}

/// Where the piano roll's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RollLayout {
    scale: Scale,
    /// The title line.
    pub title: Rect,
    /// The page buttons.
    pub previous: Rect,
    pub next: Rect,
    pub keys: Rect,
    pub grid: Rect,
    /// Width of a step.
    pub cell: i32,
    pub row: i32,
}

impl RollLayout {
    /// The piano roll on a `width` × `height` window (`width` without the
    /// track editor), for a track of `spb` steps per beat.
    pub fn new(width: i32, height: i32, scale: Scale, status_lines: usize, spb: i32) -> Self {
        let s = scale;
        let margin = 16 * s;
        let top = frame::body_top(scale);
        let bottom = frame::footer_top(height, scale, status_lines) - margin / 2;
        let title = Rect::new(margin, top, (width - 2 * margin).max(0), 12 * s);
        let button = 14 * s;
        let next = Rect::new(title.x + title.w - button, top - 2 * s, button, button);
        let previous = Rect::new(next.x - 56 * s - button, top - 2 * s, button, button);
        let grid_top = top + 18 * s;
        let h = (bottom - grid_top).max(0);
        let keys = Rect::new(margin, grid_top, KEYS_WIDTH * s, h);
        let grid_x = keys.x + keys.w;
        let grid_w = (width - margin - grid_x).max(0);
        let cell = (grid_w / (BEATS_PER_PAGE * spb.max(1))).max(MIN_CELL_WIDTH * s);
        Self {
            scale,
            title,
            previous,
            next,
            keys,
            grid: Rect::new(grid_x, grid_top, grid_w, h),
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

    /// Where note `n` of the view's track is drawn, if it is on the page.
    pub fn note_rect(&self, view: &RollView, n: usize) -> Option<Rect> {
        let note = view.track.notes.get(n)?;
        let step = absolute_step(note.beat, note.step, steps_per_beat(view.track));
        let (start, end) = view.page_steps();
        let row = note_row(note, view.track);
        if step < start || step >= end || !self.shown_row(view, row) {
            return None;
        }
        let s = self.scale.px();
        let (x, y) = (self.step_x(view, step), self.row_y(view.top_row, row));
        Some(Rect::new(x + s, y + s, self.cell - 2 * s, self.row - 2 * s))
    }

    /// What is at `(x, y)`.
    pub fn hit(&self, view: &RollView, x: i32, y: i32) -> Option<RollHit> {
        if view.pages() > 1 {
            if self.previous.contains(x, y) {
                return Some(RollHit::PreviousPage);
            }
            if self.next.contains(x, y) {
                return Some(RollHit::NextPage);
            }
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

/// Draw the piano roll; `title` names the track.
pub fn draw(canvas: &mut Canvas, view: &RollView, layout: &RollLayout, title: &str) {
    let s = layout.scale;
    canvas.text(layout.title.x, layout.title.y, title, s, theme::text());
    if view.pages() > 1 {
        for (rect, label, enabled) in
            [(layout.previous, "<", view.page > 0), (layout.next, ">", view.page < view.pages() - 1)]
        {
            canvas.fill_rect(
                rect.x,
                rect.y,
                rect.w,
                rect.h,
                if enabled { column_half_color() } else { row_black_color() },
            );
            canvas.text(
                rect.x + (rect.w - 8 * s) / 2,
                rect.y + 3 * s,
                label,
                s,
                if enabled { theme::text() } else { theme::muted() },
            );
        }
        let info = format!("{}/{}", view.page + 1, view.pages());
        let x = layout.previous.x
            + layout.previous.w
            + (layout.next.x - layout.previous.x - layout.previous.w - text_width(&info, s)) / 2;
        canvas.text(x, layout.title.y, &info, s, theme::text());
    }

    let spb = steps_per_beat(view.track);
    let (start, end) = view.page_steps();
    let first_row = layout.first_row(view);
    let width = (end - start) as i32 * layout.cell;

    // Keys and row backgrounds.
    for row in first_row..=view.top_row {
        let y = layout.row_y(view.top_row, row);
        let midi = row_midi(row);
        let black = BLACK_KEYS.contains(&midi.rem_euclid(12));
        let c = midi.rem_euclid(12) == 0;
        let k = layout.keys;
        canvas.fill_rect(
            k.x,
            y,
            k.w - s.px(),
            layout.row - s.px(),
            if black { key_black_color() } else { key_white_color() },
        );
        if c {
            canvas.text(
                k.x + k.w - 8 * s - text_width(&midi_name(midi), s),
                y + (layout.row - 8 * s) / 2,
                &midi_name(midi),
                s,
                background_color(),
            );
        }
        canvas.fill_rect(
            layout.grid.x,
            y,
            width,
            layout.row,
            if black { row_black_color() } else { row_white_color() },
        );
        if c {
            canvas.fill_rect(layout.grid.x, y + layout.row - s.px(), width, s.px(), octave_color());
        }
    }
    let grid_h = (view.top_row - first_row + 1) * layout.row;
    // Step columns: beats, half beats, steps.
    for step in start..end {
        let in_beat = (step % i64::from(spb)) as i32;
        let color = if in_beat == 0 {
            column_beat_color()
        } else if spb >= 4 && in_beat * 2 == spb {
            column_half_color()
        } else {
            column_step_color()
        };
        canvas.fill_rect(layout.step_x(view, step), layout.grid.y, s.px(), grid_h, color);
    }
    // The loop point.
    if let Some(loop_length) = view.track.loop_length.map(i64::from).filter(|&l| l > start && l <= end) {
        canvas.fill_rect(layout.step_x(view, loop_length) - s.px(), layout.grid.y, 2 * s, grid_h, LOOP_MARK);
    }

    // Ghosts, then notes.
    for (n, note) in view.track.notes.iter().enumerate() {
        let row = note_row(note, view.track);
        for sub in &view.subs[n] {
            let ghost_row = row + sub.pitch_offset.round() as i32;
            if sub.step < start || sub.step >= end || !layout.shown_row(view, ghost_row) {
                continue;
            }
            let (x, y) = (layout.step_x(view, sub.step), layout.row_y(view.top_row, ghost_row));
            // The same small rounded square as in the pattern grid: half a cell, centred.
            let base = if sub.kind == SubKind::Euclidean { theme::GENERATED } else { theme::muted() };
            let color = theme::note(base, note.velocity);
            let (w, h) = ((layout.cell / 2).max(1), (layout.row / 2).max(1));
            let (gx, gy) = (x + (layout.cell - w) / 2, y + (layout.row - h) / 2);
            let gradient = (theme::mix(color, 0xff_ff_ff, NOTE_LIGHT), theme::mix(color, 0x00_00_00, NOTE_DARK));
            canvas.fill_rounded_gradient(gx, gy, w, h, gradient, s.px());
        }
    }
    for n in 0..view.track.notes.len() {
        let Some(r) = layout.note_rect(view, n) else { continue };
        let note = &view.track.notes[n];
        let lit = view.playhead.is_some_and(|step| view.sounding(n, step));
        let color = theme::mix(
            background_color(),
            if lit { PLAYING } else { view.color },
            note_opacity(note.velocity.clamp(0.0, 1.0)),
        );
        let radius = (2 * s.px()).min(r.w / 2).min(r.h / 2);
        let selected = view.selected == Some(n);
        // A playing note glows in the margin around it.
        if lit {
            canvas.shade_rect(r.x - s.px(), r.y - s.px(), r.w + 2 * s, r.h + 2 * s, PLAYING, NOTE_GLOW);
        }
        // Relief: a soft shadow in the margin below, a gradient, a light top edge.
        canvas.shade_rect(r.x + radius, r.y + r.h, r.w - 2 * radius, s.px(), 0x00_00_00, NOTE_SHADOW);
        // A selected note is ringed: a white rounded shape, the note drawn inside it.
        let body = if selected {
            let ring = s.px();
            canvas.fill_rounded_gradient(r.x, r.y, r.w, r.h, (SELECTED, SELECTED), radius);
            Rect::new(r.x + ring, r.y + ring, r.w - 2 * ring, r.h - 2 * ring)
        } else {
            r
        };
        let light = if lit { NOTE_LIGHT_PLAYING } else { NOTE_LIGHT };
        canvas.fill_rounded_gradient(
            body.x,
            body.y,
            body.w,
            body.h,
            (theme::mix(color, 0xff_ff_ff, light), theme::mix(color, 0x00_00_00, NOTE_DARK)),
            (radius - if selected { s.px() } else { 0 }).max(0),
        );
        canvas.shade_rect(body.x + radius, body.y, body.w - 2 * radius, s.px(), 0xff_ff_ff, NOTE_SHEEN);
        // The velocity, as a bar along the bottom edge.
        let bar = ((f64::from(body.w - 2 * radius) * note.velocity.clamp(0.0, 1.0)).round() as i32).max(s.px());
        canvas.shade_rect(body.x + radius, body.y + body.h - 2 * s, bar, s.px(), 0xff_ff_ff, VELOCITY_BAR);
        // The trigger kind, a marker in the top right corner: probability wins over the cycle.
        let size = 3 * s.px();
        let (mx, my) = (body.x + body.w - size - s.px(), body.y + s.px());
        if note.probability < 1.0 {
            canvas.fill_rounded_gradient(mx, my, size, size, (TRIG_RAND, TRIG_RAND), size / 2);
        } else if note.cycle > 1 {
            diamond(canvas, mx, my, size, TRIG_FIXED);
        }
    }
    if let Some(zone) = &view.zone {
        draw_zone(canvas, view, layout, zone);
    }
    // The cursor, when no note is selected.
    if let Some((step, row)) = view.cursor.filter(|_| view.selected.is_none())
        && step >= start
        && step < end
        && layout.shown_row(view, row)
    {
        let r = Rect::new(layout.step_x(view, step), layout.row_y(view.top_row, row), layout.cell, layout.row);
        stroke(canvas, r, 2 * s, CURSOR);
    }
    if let Some(step) = view.playhead.filter(|&p| p >= start && p < end) {
        canvas.fill_rect(layout.step_x(view, step), layout.grid.y, 2 * s, grid_h, PLAYHEAD);
    }
    if first_row > 0 || view.top_row < TOTAL_KEYS - 1 {
        let hint = format!("{} - {}", midi_name(row_midi(view.top_row)), midi_name(row_midi(first_row)));
        canvas.text(layout.keys.x, layout.grid.y + grid_h + 4 * s, &hint, s, theme::muted());
    }
}

/// The zone over its steps and its rows on the page: shaded, outlined.
fn draw_zone(canvas: &mut Canvas, view: &RollView, layout: &RollLayout, zone: &RollSpan) {
    let (start, end) = view.page_steps();
    let (first, last) = (zone.start_step.max(start), zone.end_step.min(end));
    let (low, high) = (zone.low_row.max(layout.first_row(view)), zone.high_row.min(view.top_row));
    if first >= last || low > high {
        return;
    }
    let (left, right) = (layout.step_x(view, first), layout.step_x(view, last));
    let (top, bottom) = (layout.row_y(view.top_row, high), layout.row_y(view.top_row, low) + layout.row);
    let r = Rect::new(left, top, right - left, bottom - top);
    canvas.shade_rect(r.x, r.y, r.w, r.h, ZONE, ZONE_OPACITY);
    stroke(canvas, r, layout.scale.px(), ZONE);
}

/// A diamond filling the `size` square at `(x, y)`.
fn diamond(canvas: &mut Canvas, x: i32, y: i32, size: i32, color: Rgb) {
    let half = size / 2;
    for row in 0..size {
        let inset = (half - row).abs().min(half);
        canvas.fill_rect(x + inset, y + row, size - 2 * inset, 1, color);
    }
}

/// Rectangle outline, `t` wide, inside `r`.
fn stroke(canvas: &mut Canvas, r: Rect, t: i32, color: Rgb) {
    canvas.fill_rect(r.x, r.y, r.w, t, color);
    canvas.fill_rect(r.x, r.y + r.h - t, r.w, t, color);
    canvas.fill_rect(r.x, r.y, t, r.h, color);
    canvas.fill_rect(r.x + r.w - t, r.y, t, r.h, color);
}

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

    fn view(track: &Track, page: i32, top_row: i32) -> RollView<'_> {
        RollView {
            track,
            color: theme::track(0),
            beat_count: 6,
            page,
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
        let layout = RollLayout::new(1100, 680, Scale::ONE, 0, 4);
        // Track BASS is an octave down: its C is MIDI 48, row 36.
        let row = note_row(&track.notes[0], track);
        assert_eq!(row, 36);
        let top = layout.centered_top(row);
        let v = view(track, 0, top);
        assert_eq!((v.pages(), v.page_steps()), (2, (0, 16)));
        let r = layout.note_rect(&v, 0).unwrap();
        assert_eq!(layout.hit(&v, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 0, row: 36 }));
        let r = layout.note_rect(&v, 1).unwrap();
        assert_eq!(layout.hit(&v, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 6, row: 43 }));
        assert_eq!(layout.hit(&v, layout.keys.x + 2, r.y + 2), Some(RollHit::Key(row_midi(43))));
        // Note 2 (beat 4) is on page 2.
        assert!(layout.note_rect(&v, 2).is_none());
        let v2 = view(track, 1, top);
        assert_eq!(v2.page_steps(), (16, 24));
        let r = layout.note_rect(&v2, 2).unwrap();
        assert_eq!(layout.hit(&v2, r.x + 2, r.y + 2), Some(RollHit::Cell { step: 16, row: 36 }));
        // A drag past the grid stops at the page's last step and the lowest row shown.
        let lowest = layout.first_row(&v2);
        assert_eq!(layout.nearest_cell(&v2, 5000, 5000), Some((23, lowest)));
        assert_eq!(layout.nearest_cell(&v2, 0, 0), Some((16, top)));
        // Past the last step of a short page: nothing.
        assert_eq!(layout.hit(&v2, layout.grid.x + 8 * layout.cell + 2, r.y + 2), None);
        assert_eq!(layout.hit(&v2, layout.previous.x + 2, layout.previous.y + 2), Some(RollHit::PreviousPage));
        assert_eq!(layout.hit(&v2, layout.next.x + 2, layout.next.y + 2), Some(RollHit::NextPage));
    }

    #[test]
    fn scroll_stays_on_the_keyboard() {
        let layout = RollLayout::new(1100, 680, Scale::ONE, 0, 4);
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
            let layout = RollLayout::new(w as i32, h as i32, Scale::ONE, 1, 4);
            for page in [0, 1] {
                draw(&mut canvas, &view(track, page, layout.centered_top(40)), &layout, "Piano Roll - BASS");
            }
        }
    }
}
