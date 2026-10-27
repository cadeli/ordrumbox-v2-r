//! The screen: header (pattern, tempo, length, kit, transport, undo), one row
//! per track (mute and solo boxes, name, steps with their notes), the grid
//! cursor, the selected note, the playhead, the messages and the key help.
//!
//! [`Layout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Pattern, Track};

use crate::canvas::{Canvas, Rgb, text_width};

const BACKGROUND: Rgb = 0x0016_1619;
const CELL: Rgb = 0x002A_2A33;
const CELL_BEAT: Rgb = 0x0038_3845;
const NOTE: Rgb = 0x00F2_A93B;
const TEXT: Rgb = 0x00E6_E6EA;
const DIM: Rgb = 0x0080_8090;
const ACCENT: Rgb = 0x00F2_A93B;
const PLAYING: Rgb = 0x005C_D67A;
const WARNING: Rgb = 0x00E8_6A5A;
const PLAYHEAD: Rgb = 0x00FF_FFFF;
const LOOP_MARK: Rgb = 0x00B0_B0C0;
const CURSOR: Rgb = 0x006A_B7FF;
const SELECTED: Rgb = 0x00FF_FFFF;
const FOCUS_ROW: Rgb = 0x0020_2028;
const MUTE_ON: Rgb = 0x00E8_6A5A;
const SOLO_ON: Rgb = 0x00F2_D43B;
const BOX_OFF: Rgb = 0x0038_3845;

const HELP: [&str; 4] = [
    "SPACE play (SHIFT: song)  [ ] pattern (SHIFT: song)  - = tempo (SHIFT x10)  , . kit  1-9 mute (SHIFT: solo)  Q-P hear",
    "click/ENTER note (again: delete)  ARROWS move  SHIFT ARROWS velocity/pitch  DEL clear step  ESC deselect",
    "CTRL: Z undo  Y redo  C/V copy/paste step (SHIFT: track)  T add track  BACKSPACE del track  G grid  L loop  -/= length",
    "CTRL: N new pattern  D duplicate  S save  E export WAV (SHIFT: song)",
];
/// Characters of a track name shown.
const LABEL_CHARS: usize = 10;

/// The grid cursor (keyboard focus): track row and step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    pub track: usize,
    pub beat: i32,
    pub step: i32,
}

/// What to show.
#[derive(Debug)]
pub struct View<'a> {
    pub pattern: Option<&'a Pattern>,
    pub pattern_index: usize,
    pub pattern_count: usize,
    pub kit: &'a str,
    pub playing: bool,
    /// Playhead, as a fraction of the pattern length.
    pub playhead: Option<f64>,
    /// The selected song, or the song playing and where.
    pub song: Option<&'a str>,
    pub cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    pub selected: Option<(usize, usize)>,
    pub undo: Option<&'a str>,
    pub redo: Option<&'a str>,
    /// Outcome of the last action.
    pub message: &'a str,
    /// Problems to show above the key help, most important first.
    pub status: &'a [String],
}

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Mute(usize),
    Solo(usize),
    Name(usize),
    Cell { track: usize, beat: i32, step: i32 },
}

/// Where the track rows are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    scale: i32,
    /// Top of the first row, height of a row, bottom of the grid area.
    top: i32,
    row: i32,
    bottom: i32,
    left: i32,
    box_size: i32,
    name_x: i32,
    label_scale: i32,
    grid_left: i32,
    grid_width: i32,
    beats: i32,
    tracks: usize,
}

/// Text scale of the header (the 8 px font at 16 px per scale unit).
fn header_text(scale: i32) -> i32 {
    2 * scale
}

fn header_line(scale: i32) -> i32 {
    8 * header_text(scale) + 8 * scale
}

fn margin(scale: i32) -> i32 {
    16 * scale
}

/// Top of the footer (status lines then help).
fn footer_top(height: i32, scale: i32, status_lines: usize) -> i32 {
    height - margin(scale) - (HELP.len() as i32) * 10 * scale - status_lines as i32 * 10 * scale - 4 * scale
}

impl Layout {
    /// The rows of `pattern` on a `width` × `height` screen, with
    /// `status_lines` message lines in the footer.
    pub fn new(width: i32, height: i32, scale: i32, pattern: &Pattern, status_lines: usize) -> Self {
        let margin = margin(scale);
        let top = margin + 3 * header_line(scale) + margin / 2;
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let tracks = pattern.tracks.len();
        let row = ((bottom - top) / tracks.max(1) as i32).clamp(10 * scale, 32 * scale);
        let label_scale = if row >= 24 * scale { 2 * scale } else { scale };
        let box_size = (row - 4 * scale).clamp(6 * scale, 14 * scale);
        let left = margin;
        let name_x = left + 2 * (box_size + 3 * scale) + 4 * scale;
        let grid_left = name_x + text_width(&"W".repeat(LABEL_CHARS), label_scale) + 12 * scale;
        let shown = usize::try_from((bottom - top).max(0) / row.max(1)).unwrap_or(0).min(tracks);
        Self {
            scale,
            top,
            row,
            bottom,
            left,
            box_size,
            name_x,
            label_scale,
            grid_left,
            grid_width: (width - margin - grid_left).max(1),
            beats: pattern.beat_count.max(1),
            tracks: shown,
        }
    }

    fn row_top(&self, track: usize) -> i32 {
        self.top + track as i32 * self.row
    }

    fn beat_width(&self) -> f64 {
        f64::from(self.grid_width) / f64::from(self.beats)
    }

    fn box_x(&self, solo: bool) -> i32 {
        self.left + if solo { self.box_size + 3 * self.scale } else { 0 }
    }

    /// Left edge of `step` (from the track start) at `spb` steps per beat.
    fn step_x(&self, step: i64, spb: i32) -> i32 {
        self.grid_left + (step as f64 * self.beat_width() / f64::from(spb)) as i32
    }

    /// What is at `(x, y)`.
    pub fn hit(&self, pattern: &Pattern, x: i32, y: i32) -> Option<Hit> {
        if y < self.top || x < self.left {
            return None;
        }
        let track = usize::try_from((y - self.top) / self.row).ok().filter(|&t| t < self.tracks)?;
        let in_box = |solo: bool| {
            let bx = self.box_x(solo);
            let by = self.row_top(track) + (self.row - self.box_size) / 2;
            (bx..bx + self.box_size).contains(&x) && (by..by + self.box_size).contains(&y)
        };
        if in_box(false) {
            return Some(Hit::Mute(track));
        }
        if in_box(true) {
            return Some(Hit::Solo(track));
        }
        if x < self.grid_left {
            return (x >= self.name_x).then_some(Hit::Name(track));
        }
        let spb = steps_per_beat(&pattern.tracks[track]);
        let step = ((f64::from(x - self.grid_left) * f64::from(spb)) / self.beat_width()) as i64;
        let steps = i64::from(self.beats) * i64::from(spb);
        (step < steps).then(|| Hit::Cell {
            track,
            beat: (step / i64::from(spb)) as i32,
            step: (step % i64::from(spb)) as i32,
        })
    }
}

fn steps_per_beat(track: &Track) -> i32 {
    track.steps_per_beat.clamp(1, 16)
}

/// Draw `view`; `scale` is the integer UI scale (1 on a standard display, 2 on HiDPI).
pub fn draw(canvas: &mut Canvas, view: &View, scale: i32) {
    canvas.clear(BACKGROUND);
    let text = header_text(scale);
    let line = header_line(scale);
    let margin = margin(scale);

    // Header.
    let mut y = margin;
    let x = canvas.text(margin, y, "orDrumbox", text, ACCENT) + 24 * scale;
    match view.pattern {
        Some(pattern) => {
            let name = pattern.name.as_deref().unwrap_or(&pattern.id);
            let title = format!("{name}  {}/{}", view.pattern_index + 1, view.pattern_count);
            canvas.text(x, y, &title, text, TEXT);
            y += line;
            let tempo = format!("{} BPM  {} beats", pattern.bpm, pattern.beat_count);
            let x = canvas.text(margin, y, &tempo, text, TEXT) + 24 * scale;
            let x = canvas.text(x, y, &format!("kit {}", view.kit), text, DIM) + 24 * scale;
            let (state, color) = if view.playing { ("PLAY", PLAYING) } else { ("STOP", DIM) };
            let x = canvas.text(x, y, state, text, color) + 24 * scale;
            if let Some(song) = view.song {
                canvas.text(x, y + 4 * scale, song, scale, if song.starts_with("SONG") { PLAYING } else { DIM });
            }
        }
        None => {
            canvas.text(x, y, "no pattern", text, DIM);
            y += line;
        }
    }
    y += line;
    let history = match (view.undo, view.redo) {
        (Some(undo), Some(redo)) => format!("undo: {undo}   redo: {redo}"),
        (Some(undo), None) => format!("undo: {undo}"),
        (None, Some(redo)) => format!("redo: {redo}"),
        (None, None) => String::new(),
    };
    let x = canvas.text(margin, y, view.message, scale, TEXT) + 24 * scale;
    canvas.text(x.max(margin + canvas.width() / 3), y, &history, scale, DIM);

    // Footer: status lines then help.
    let mut footer_y = footer_top(canvas.height(), scale, view.status.len()) + 4 * scale;
    for message in view.status {
        canvas.text(margin, footer_y, message, scale, WARNING);
        footer_y += 10 * scale;
    }
    for help in HELP {
        canvas.text(margin, footer_y, help, scale, DIM);
        footer_y += 10 * scale;
    }

    if let Some(pattern) = view.pattern {
        let layout = Layout::new(canvas.width(), canvas.height(), scale, pattern, view.status.len());
        draw_grid(canvas, pattern, view, &layout);
    }
}

fn draw_grid(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let gap = scale.max(1);
    for (i, track) in pattern.tracks.iter().enumerate().take(layout.tracks) {
        let y = layout.row_top(i);
        if view.cursor.is_some_and(|c| c.track == i) {
            canvas.fill_rect(layout.left - 4 * scale, y, layout.grid_left - layout.left, layout.row - gap, FOCUS_ROW);
        }
        let box_y = y + (layout.row - layout.box_size) / 2;
        for (solo, on, color, letter) in [(false, track.mute, MUTE_ON, "M"), (true, track.solo, SOLO_ON, "S")] {
            let x = layout.box_x(solo);
            canvas.fill_rect(x, box_y, layout.box_size, layout.box_size, if on { color } else { BOX_OFF });
            let letter_scale = (layout.box_size / 10).max(1);
            let inset = (layout.box_size - 8 * letter_scale) / 2;
            canvas.text(x + inset, box_y + inset, letter, letter_scale, if on { BACKGROUND } else { DIM });
        }
        let label: String = track.name.chars().take(LABEL_CHARS).collect();
        let label_y = y + (layout.row - 8 * layout.label_scale) / 2;
        canvas.text(layout.name_x, label_y, &label, layout.label_scale, if track.mute { DIM } else { TEXT });
        draw_track_steps(canvas, track, i, view, layout);
    }

    if let Some(fraction) = view.playhead {
        let x = layout.grid_left + (fraction.clamp(0.0, 1.0) * f64::from(layout.grid_width)) as i32;
        let height = (layout.tracks as i32 * layout.row).min(layout.bottom - layout.top) - gap;
        canvas.fill_rect(x - scale, layout.top, 2 * scale, height, PLAYHEAD);
    }
}

/// One track's steps, its notes lit by velocity, the cursor and the selection.
fn draw_track_steps(canvas: &mut Canvas, track: &Track, index: usize, view: &View, layout: &Layout) {
    let gap = layout.scale.max(1);
    let spb = steps_per_beat(track);
    let steps = i64::from(layout.beats) * i64::from(spb);
    let y = layout.row_top(index);
    let height = layout.row - gap;
    let cell = |step: i64| {
        let x = layout.step_x(step, spb);
        (x, (layout.step_x(step + 1, spb) - x - gap).max(1))
    };

    let mut velocity = vec![None::<f64>; steps as usize];
    for note in &track.notes {
        let step = absolute_step(note.beat, note.beat_step, spb);
        if let Some(slot) = usize::try_from(step).ok().and_then(|s| velocity.get_mut(s)) {
            *slot = Some(slot.unwrap_or(0.0).max(note.velocity));
        }
    }
    for (step, note) in velocity.iter().enumerate() {
        let step = step as i64;
        let (x, width) = cell(step);
        let color = match note {
            Some(v) => mix(CELL, NOTE, 0.35 + 0.65 * v.clamp(0.0, 1.0)),
            None if step % i64::from(spb) == 0 => CELL_BEAT,
            None => CELL,
        };
        canvas.fill_rect(x, y, width, height, color);
    }
    if let Some(loop_at) = track.loop_at_step.map(i64::from).filter(|&l| l > 0 && l < steps) {
        let (x, _) = cell(loop_at);
        canvas.fill_rect(x - gap, y, gap * 2, height, LOOP_MARK);
    }

    let outline = |canvas: &mut Canvas, step: i64, color: Rgb| {
        if (0..steps).contains(&step) {
            let (x, width) = cell(step);
            stroke(canvas, x, y, width, height, (2 * layout.scale).max(1), color);
        }
    };
    if let Some(c) = view.cursor.filter(|c| c.track == index) {
        outline(canvas, absolute_step(c.beat, c.step, spb), CURSOR);
    }
    if let Some(note) = view.selected.filter(|(t, _)| *t == index).and_then(|(_, n)| track.notes.get(n)) {
        outline(canvas, absolute_step(note.beat, note.beat_step, spb), SELECTED);
    }
}

/// Rectangle outline, `thickness` wide, inside `(x, y, width, height)`.
fn stroke(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, thickness: i32, color: Rgb) {
    canvas.fill_rect(x, y, width, thickness, color);
    canvas.fill_rect(x, y + height - thickness, width, thickness, color);
    canvas.fill_rect(x, y, thickness, height, color);
    canvas.fill_rect(x + width - thickness, y, thickness, height, color);
}

/// `from` blended towards `to` by `amount` (0–1), per channel.
fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let channel = |shift: u32| {
        let a = f64::from((from >> shift) & 0xFF);
        let b = f64::from((to >> shift) & 0xFF);
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn library() -> Library {
        Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","loopAtStep":3,"notes":[{},{"beat":9}]},{"name":"X","stepsPerBeat":0},{"name":"H","stepsPerBeat":2}]}]}"#,
        )
        .unwrap()
        .0
    }

    #[test]
    fn mix_blends_channels() {
        assert_eq!(mix(0x000000, 0xFF8040, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xFF8040, 1.0), 0xFF8040);
        assert_eq!(mix(0x000000, 0xFF8040, 0.5), 0x804020);
    }

    #[test]
    fn draws_without_panicking_at_any_size() {
        let library = library();
        for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let view = View {
                pattern: library.patterns.first(),
                pattern_index: 0,
                pattern_count: 1,
                kit: "real",
                playing: true,
                playhead: Some(0.5),
                song: Some("SONG demo  measure 3/66"),
                cursor: Some(Cursor { track: 0, beat: 1, step: 2 }),
                selected: Some((0, 0)),
                undo: Some("Add note"),
                redo: None,
                message: "hello",
                status: &["warning".into()],
            };
            draw(&mut canvas, &view, 1);
        }
    }

    #[test]
    fn hit_testing_matches_the_drawing() {
        let library = library();
        let pattern = &library.patterns[0];
        let layout = Layout::new(1024, 640, 1, pattern, 0);
        let y = layout.row_top(2) + layout.row / 2;
        assert_eq!(layout.hit(pattern, layout.box_x(false) + 1, y), Some(Hit::Mute(2)));
        assert_eq!(layout.hit(pattern, layout.box_x(true) + 1, y), Some(Hit::Solo(2)));
        assert_eq!(layout.hit(pattern, layout.name_x + 2, y), Some(Hit::Name(2)));
        // Track H: 2 steps per beat; the middle of beat 1's second step.
        let x = layout.step_x(3, 2) + 2;
        assert_eq!(layout.hit(pattern, x, y), Some(Hit::Cell { track: 2, beat: 1, step: 1 }));
        assert_eq!(layout.hit(pattern, layout.grid_left + layout.grid_width + 5, y), None);
        assert_eq!(layout.hit(pattern, x, layout.row_top(3) + 1), None);
        assert_eq!(layout.hit(pattern, x, layout.top - 1), None);
    }
}
