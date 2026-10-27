//! The screen: header (pattern, generation, tempo, length, kit, transport,
//! undo), one row per track (mute and solo boxes, name — green in auto mode —,
//! steps with their notes), the grid cursor, the selected note, the playhead,
//! the messages and the key help.
//!
//! [`Layout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Pattern, Track};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::kits::Field;
use crate::menu;

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

/// The key help line under the grid (every command is in the menus, with
/// its shortcut; Help > Keyboard shortcuts lists them all).
const HELP: &str = "F10 or ALT: menus   ARROWS: cursor   ENTER or click: note   SHIFT+ARROWS: velocity/pitch   1-9: mute   Q-P: hear   F1: all keys";
/// The same in the kit editor.
const KIT_HELP: &str = "KIT EDITOR   UP DOWN: sample   LEFT RIGHT: field   - =: change (SHIFT: finely)   ENTER: hear   DEL: remove   ESC: close";

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
    /// Generation on (tracks in auto mode): drums, bass, chords.
    pub generation: [bool; 3],
    /// The genre the pattern was generated in.
    pub genre: Option<&'a str>,
    pub cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    pub selected: Option<(usize, usize)>,
    pub undo: Option<&'a str>,
    pub redo: Option<&'a str>,
    /// Outcome of the last action.
    pub message: &'a str,
    /// Problems to show above the key help, most important first.
    pub status: &'a [String],
    /// The kit editor, when open (it takes the place of the grid).
    pub kit_view: Option<KitView<'a>>,
}

/// What the kit editor shows.
#[derive(Debug)]
pub struct KitView<'a> {
    pub kit: &'a str,
    /// Display name and instrument key of each sample.
    pub samples: Vec<(&'a str, &'a str)>,
    pub selected: usize,
    pub field: Field,
    /// Values of the fields of the selected sample, in [`Field::ALL`] order.
    pub values: Option<[String; 5]>,
    /// Note, length and levels of the selected sample.
    pub info: Option<String>,
    /// The instrument its name suggests.
    pub detected: Option<String>,
    /// Peak of each slice, 0–1.
    pub envelope: &'a [f64],
    /// Where the decay ends, as a fraction of the sample.
    pub decay_marker: Option<f64>,
}

/// What is under the mouse in the kit editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KitHit {
    Sample(usize),
    Field(Field),
}

/// Where the kit editor's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitLayout {
    scale: i32,
    top: i32,
    bottom: i32,
    left: i32,
    list_width: i32,
    row: i32,
    detail_x: i32,
    field_top: i32,
    width: i32,
}

impl KitLayout {
    pub fn new(width: i32, height: i32, scale: i32, status_lines: usize) -> Self {
        let margin = margin(scale);
        let top = content_top(scale) + 3 * header_line(scale) + margin / 2;
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let list_width = ((width - 2 * margin) / 3).max(40 * scale);
        let row = 12 * scale;
        let detail_x = margin + list_width + 16 * scale;
        Self { scale, top, bottom, left: margin, list_width, row, detail_x, field_top: top + 16 * scale, width }
    }

    fn shown(&self) -> usize {
        usize::try_from((self.bottom - self.top) / self.row).unwrap_or(0)
    }

    /// First list row shown, so that `selected` is visible.
    fn first(&self, selected: usize) -> usize {
        selected.saturating_sub(self.shown().saturating_sub(1))
    }

    fn field_y(&self, i: usize) -> i32 {
        self.field_top + i as i32 * 14 * self.scale
    }

    pub fn hit(&self, view_selected: usize, count: usize, x: i32, y: i32) -> Option<KitHit> {
        if y < self.top || y >= self.bottom {
            return None;
        }
        if (self.left..self.left + self.list_width).contains(&x) {
            let i = self.first(view_selected) + usize::try_from((y - self.top) / self.row).ok()?;
            return (i < count).then_some(KitHit::Sample(i));
        }
        if x >= self.detail_x && x < self.width {
            return Field::ALL.iter().enumerate().find_map(|(i, f)| {
                let fy = self.field_y(i);
                (fy - 2 * self.scale..fy + 12 * self.scale).contains(&y).then_some(KitHit::Field(*f))
            });
        }
        None
    }
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
/// Top of the header: under the menu bar.
fn content_top(scale: i32) -> i32 {
    menu::bar_height(scale) + margin(scale) / 2
}

fn footer_top(height: i32, scale: i32, status_lines: usize) -> i32 {
    height - margin(scale) - 10 * scale - status_lines as i32 * 10 * scale - 4 * scale
}

impl Layout {
    /// The rows of `pattern` on a `width` × `height` screen, with
    /// `status_lines` message lines in the footer.
    pub fn new(width: i32, height: i32, scale: i32, pattern: &Pattern, status_lines: usize) -> Self {
        let margin = margin(scale);
        let top = content_top(scale) + 3 * header_line(scale) + margin / 2;
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
    let mut y = content_top(scale);
    let x = canvas.text(margin, y, "orDrumbox", text, ACCENT) + 24 * scale;
    match view.pattern {
        Some(pattern) => {
            let name = pattern.name.as_deref().unwrap_or(&pattern.id);
            let title = format!("{name}  {}/{}", view.pattern_index + 1, view.pattern_count);
            let mut x = canvas.text(x, y, &title, text, TEXT) + 24 * scale;
            let labels = ["DRUMS", "BASS", "CHORDS"];
            for (label, on) in labels.iter().zip(view.generation) {
                x = canvas.text(x, y + 4 * scale, label, scale, if on { PLAYING } else { DIM }) + 8 * scale;
            }
            if let Some(genre) = view.genre {
                canvas.text(x + 8 * scale, y + 4 * scale, genre, scale, DIM);
            }
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
    canvas.text(margin, footer_y, if view.kit_view.is_some() { KIT_HELP } else { HELP }, scale, DIM);

    if let Some(kit) = &view.kit_view {
        let layout = KitLayout::new(canvas.width(), canvas.height(), scale, view.status.len());
        draw_kit(canvas, kit, &layout);
    } else if let Some(pattern) = view.pattern {
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
        let color = if track.mute {
            DIM
        } else if track.auto {
            PLAYING // a new line every loop
        } else {
            TEXT
        };
        canvas.text(layout.name_x, label_y, &label, layout.label_scale, color);
        draw_track_steps(canvas, track, i, view, layout);
    }

    if let Some(fraction) = view.playhead {
        let x = layout.grid_left + (fraction.clamp(0.0, 1.0) * f64::from(layout.grid_width)) as i32;
        let height = (layout.tracks as i32 * layout.row).min(layout.bottom - layout.top) - gap;
        canvas.fill_rect(x - scale, layout.top, 2 * scale, height, PLAYHEAD);
    }
}

/// A panel over the window: a title and lines of text (the key list, about).
pub fn draw_notice(canvas: &mut Canvas, title: &str, lines: &[String], scale: i32) {
    let line_h = 11 * scale;
    let width =
        lines.iter().map(|l| text_width(l, scale)).chain([text_width(title, scale)]).max().unwrap_or(0) + 32 * scale;
    let height = (lines.len() as i32 + 3) * line_h + 16 * scale;
    let x = ((canvas.width() - width) / 2).max(0);
    let y = ((canvas.height() - height) / 2).max(menu::bar_height(scale));
    canvas.fill_rect(x - 4 * scale, y - 4 * scale, width + 8 * scale, height + 8 * scale, 0x0010_1014);
    canvas.fill_rect(x, y, width, height, 0x0048_4858);
    canvas.fill_rect(x + scale, y + scale, width - 2 * scale, height - 2 * scale, 0x0024_2430);
    canvas.text(x + 16 * scale, y + 10 * scale, title, scale, ACCENT);
    for (i, line) in lines.iter().enumerate() {
        canvas.text(x + 16 * scale, y + 10 * scale + (i as i32 + 2) * line_h, line, scale, TEXT);
    }
    let close = "ESC or click: close";
    canvas.text(x + width - 16 * scale - text_width(close, scale), y + 10 * scale, close, scale, DIM);
}

/// The kit editor: the kit's samples on the left, the selected one on the
/// right (fields, analysis, envelope with the decay marked).
fn draw_kit(canvas: &mut Canvas, kit: &KitView, layout: &KitLayout) {
    let scale = layout.scale;
    let first = layout.first(kit.selected);
    if kit.samples.is_empty() {
        canvas.text(layout.left, layout.top, "No samples in this kit", scale, DIM);
    }
    for (row, (i, (name, key))) in kit.samples.iter().enumerate().skip(first).take(layout.shown()).enumerate() {
        let y = layout.top + row as i32 * layout.row;
        if i == kit.selected {
            canvas.fill_rect(layout.left - 2 * scale, y - 2 * scale, layout.list_width, layout.row, FOCUS_ROW);
        }
        let color = if i == kit.selected { ACCENT } else { TEXT };
        let x = canvas.text(layout.left, y, name, scale, color);
        canvas.text(x + 8 * scale, y, key, scale, DIM);
    }

    let x = layout.detail_x;
    let title = format!("kit {}", kit.kit);
    canvas.text(x, layout.top, &title, scale, DIM);
    let Some(values) = &kit.values else { return };
    let label_width = text_width("Instrument  ", scale);
    for (i, field) in Field::ALL.iter().enumerate() {
        let y = layout.field_y(i);
        let selected = *field == kit.field;
        if selected {
            canvas.fill_rect(x - 4 * scale, y - 2 * scale, layout.width - x - 12 * scale, 14 * scale, FOCUS_ROW);
        }
        canvas.text(x, y, field.label(), scale, if selected { ACCENT } else { DIM });
        canvas.text(x + label_width, y, &values[i], scale, if selected { ACCENT } else { TEXT });
    }
    let mut y = layout.field_y(Field::ALL.len()) + 6 * scale;
    for line in [kit.info.as_deref(), kit.detected.as_deref()].into_iter().flatten() {
        canvas.text(x, y, line, scale, DIM);
        y += 12 * scale;
    }

    // Envelope: one bar per point, the decay end as a line.
    let (width, height) = (layout.width - x - 16 * scale, (layout.bottom - y - 4 * scale).min(80 * scale));
    if kit.envelope.is_empty() || width <= 0 || height <= 4 * scale {
        return;
    }
    canvas.fill_rect(x, y, width, height, CELL);
    let n = kit.envelope.len() as i32;
    for (i, v) in kit.envelope.iter().enumerate() {
        let bar_x = x + i as i32 * width / n;
        let bar_w = ((i as i32 + 1) * width / n - i as i32 * width / n).max(1);
        let bar_h = (v.clamp(0.0, 1.0) * f64::from(height)).round() as i32;
        canvas.fill_rect(bar_x, y + height - bar_h, bar_w, bar_h, NOTE);
    }
    if let Some(ratio) = kit.decay_marker {
        let mx = x + (ratio.clamp(0.0, 1.0) * f64::from(width - 1)) as i32;
        canvas.fill_rect(mx, y, scale.max(1), height, PLAYHEAD);
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
                generation: [true, false, true],
                genre: Some("house"),
                kit_view: None,
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
    fn kit_editor_draws_and_hits() {
        let samples: Vec<(&str, &str)> = (0..40).map(|_| ("kick.wav", "KICK")).collect();
        let values = ["real".to_owned(), "KICK".into(), "+0.0 dB".into(), "+0.0 st".into(), "500 ms".into()];
        let envelope = [0.0, 1.0, 0.5, 0.25];
        for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let kit = KitView {
                kit: "real",
                samples: samples.clone(),
                selected: 39,
                field: Field::Gain,
                values: Some(values.clone()),
                info: Some("A4 0ct".into()),
                detected: Some("Detected: KICK".into()),
                envelope: &envelope,
                decay_marker: Some(0.5),
            };
            draw_kit(&mut canvas, &kit, &KitLayout::new(w as i32, h as i32, 1, 0));
        }
        let layout = KitLayout::new(1024, 640, 1, 0);
        assert_eq!(layout.hit(0, 40, layout.left + 2, layout.top + 1), Some(KitHit::Sample(0)));
        assert_eq!(layout.hit(0, 1, layout.left + 2, layout.top + layout.row + 1), None);
        assert_eq!(layout.hit(0, 40, layout.detail_x + 2, layout.field_y(2) + 1), Some(KitHit::Field(Field::Gain)));
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
