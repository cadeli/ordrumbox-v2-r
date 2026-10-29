//! The screen: header (pattern, generation, tempo, length, kit, transport,
//! undo), then the song panel (`song_view`), the kit editor, or one row per track (mute and solo boxes, name — green in auto mode —,
//! steps with their notes), the grid cursor, the selected note, the playhead,
//! the messages and the key help.
//!
//! [`Layout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Note, Pattern, Track};

use crate::canvas::{Canvas, Rgb, text_width};
use crate::menu;
use crate::output::{self, OutputLayout, OutputView};
use crate::panel::Rect;

/// Beats shown per page of the pattern grid (JS `BEATS_PER_PAGE`).
pub const BEATS_PER_PAGE: i32 = 4;
use crate::piano_roll::{self, RollLayout, RollView};
use crate::song_view::{self, SongLayout, SongView};
use crate::theme;

const BACKGROUND: Rgb = theme::BG;
const CELL: Rgb = theme::CELL;
const CELL_BEAT: Rgb = theme::CELL_BEAT;
const NOTE: Rgb = theme::PINK;
const TEXT: Rgb = theme::TEXT;
const DIM: Rgb = theme::MUTED;
const ACCENT: Rgb = theme::PINK;
const PLAYING: Rgb = theme::LIME;
const WARNING: Rgb = theme::RED;
const PLAYHEAD: Rgb = theme::LIME;
const LOOP_MARK: Rgb = theme::YELLOW;
const CURSOR: Rgb = theme::CYAN;
const SELECTED: Rgb = theme::WHITE;
const FOCUS_ROW: Rgb = theme::SELECTED_ROW;
const MUTE_ON: Rgb = theme::RED;
/// The logo, the header's labels, values.
const LOGO: Rgb = theme::LIME;
const LABEL: Rgb = theme::CYAN;
const VALUE: Rgb = theme::YELLOW;
const SOLO_ON: Rgb = theme::YELLOW;
const BOX_OFF: Rgb = theme::LINE;

/// The key help line under the grid (every command is in the menus, with
/// its shortcut; Help > Keyboard shortcuts lists them all).
const HELP: &str = "F10 or ALT: menus   ARROWS: cursor   ENTER or click: note   SHIFT+ARROWS: velocity/pitch   TAB: track editor   F1: all keys";
/// The same in the song panel.
const SONG_HELP: &str = "SONG   click the ruler: cursor   right-click: add / repeat / remove   SPACE: play from the cursor   F3 or ESC: grid";
/// The same in the piano roll.
const ROLL_HELP: &str =
    "PIANO ROLL   click: add / select / delete   drag: velocity / pitch   right-click: menu   F4 or ESC: grid";
/// The same in the synth editor.
const SYNTH_HELP: &str = "SYNTH   TAB: controls   drag, wheel: knobs   click: waves, lists, power   F6 or ESC: grid";
/// The same in the kit editor.
const KIT_HELP: &str =
    "KIT EDITOR   UP DOWN: sample   TAB: controls   ENTER: hear   DEL: remove   N: normalize   ESC: close";

/// Characters of a track name shown.
const LABEL_CHARS: usize = 10;
/// Width of a row's volume slider, per scale unit.
const VOLUME_WIDTH: i32 = 48;

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
    /// Playhead, as a fraction of the pattern length.
    pub playhead: Option<f64>,
    /// The selected song, or the song playing and where.
    pub song: Option<&'a str>,
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
    /// The song panel, when shown (it takes the place of the grid).
    pub arrangement: Option<SongView<'a>>,
    /// The piano roll and its title, when shown (it takes the place of the grid).
    pub piano_roll: Option<(RollView<'a>, String)>,
    /// The output panel, always shown (the bottom strip).
    pub output: Option<OutputView>,
    /// The synth editor is shown (the session paints it).
    pub synth: bool,
    /// What each track plays (its sample, its synth preset), under its name.
    pub sounds: Vec<String>,
    /// Width kept on the right (for the track editor).
    pub right: i32,
    /// Current page of the pattern grid (BEATS_PER_PAGE beats).
    pub current_page: usize,
    /// Per-track levels (0–1) for the VU meter stripe, when playing.
    pub levels: Vec<f32>,
    /// Whether the VU meter stripe is shown.
    pub show_vus: bool,
}

/// What the kit editor shows.
#[derive(Debug)]
pub struct KitView<'a> {
    pub kit: &'a str,
    /// Display name and instrument key of each sample.
    pub samples: Vec<(&'a str, &'a str)>,
    pub selected: usize,
    /// Top of the sample's details, under its controls (selects and knobs).
    pub details_top: i32,
    /// Note, length and levels of the selected sample.
    pub info: Option<String>,
    /// The instrument its name suggests.
    pub detected: Option<String>,
    /// Peak of each slice, 0–1.
    pub envelope: &'a [f64],
    /// Where the decay ends, as a fraction of the sample.
    pub decay_marker: Option<f64>,
}

/// Where the kit editor's parts are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KitLayout {
    scale: i32,
    top: i32,
    list_top: i32,
    bottom: i32,
    left: i32,
    list_width: i32,
    row: i32,
    detail_x: i32,
    detail_y: i32,
    list_rows: i32,
    width: i32,
}

impl KitLayout {
    pub fn new(width: i32, height: i32, scale: i32, status_lines: usize) -> Self {
        let margin = margin(scale);
        let top = body_top(scale);
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        // The sample list takes the full width (long file names), on the top
        // rows; the selected sample's details go below it.
        let list_width = (width - 2 * margin).max(40 * scale);
        let row = 12 * scale;
        let list_top = top + row;
        let list_rows = ((bottom - top) / (row * 4)).clamp(3, 10);
        let detail_y = list_top + list_rows * row + 8 * scale;
        Self {
            scale,
            top,
            list_top,
            bottom,
            left: margin,
            list_width,
            row,
            detail_x: margin,
            detail_y,
            width,
            list_rows,
        }
    }

    /// Where the selected sample's selects and knobs go.
    pub fn controls_area(&self) -> Rect {
        let s = self.scale;
        Rect::new(self.detail_x, self.detail_y, (self.width - 2 * self.detail_x).clamp(0, 420 * s), 0)
    }

    fn shown(&self) -> usize {
        self.list_rows as usize
    }

    /// First list row shown, so that `selected` is visible.
    fn first(&self, selected: usize) -> usize {
        selected.saturating_sub(self.shown().saturating_sub(1))
    }

    /// The sample of the list under `(x, y)`.
    pub fn hit(&self, view_selected: usize, count: usize, x: i32, y: i32) -> Option<usize> {
        let list_bottom = self.list_top + self.shown() as i32 * self.row;
        if y < self.list_top || y >= list_bottom || !(self.left..self.left + self.list_width).contains(&x) {
            return None;
        }
        let i = self.first(view_selected) + usize::try_from((y - self.list_top) / self.row).ok()?;
        (i < count).then_some(i)
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
    /// The volume slider after the name.
    volume_x: i32,
    grid_left: i32,
    grid_width: i32,
    beats: i32,
    /// First beat shown on this page.
    start_beat: i32,
    tracks: usize,
}

/// Text scale of the header (the 8 px font at 16 px per scale unit).
fn header_text(scale: i32) -> i32 {
    2 * scale
}

fn margin(scale: i32) -> i32 {
    16 * scale
}

/// Top of the header: under the menu bar.
pub fn content_top(scale: i32) -> i32 {
    menu::bar_height(scale) + margin(scale) / 2
}

/// Height of the header's row of controls: a label, then the widget.
fn controls_height(scale: i32) -> i32 {
    28 * scale
}

/// Top of the header's second line (the last action, undo, the song).
fn status_line(scale: i32) -> i32 {
    content_top(scale) + controls_height(scale) + 4 * scale
}

/// Top of what is under the header (the grid, the kit editor, the song panel).
pub fn body_top(scale: i32) -> i32 {
    status_line(scale) + 10 * scale + margin(scale) / 2
}

/// The header's pattern list and kit list (`PatternNav`'s two selects).
pub fn header_selects(scale: i32) -> (Rect, Rect) {
    let c = header_controls(scale);
    (c.pattern, c.kit)
}

/// The header (the JS toolbar and its pattern settings panel), a row of
/// labelled groups after the logo: play; BPM (the tempo knob); PATTERN;
/// BEATS; KIT; GENERATE (drums, bass, chords); VIEW (grid, piano roll,
/// song, synth, output).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderControls {
    pub play: Rect,
    pub bpm: Rect,
    pub pattern: Rect,
    pub beats: Rect,
    pub kit: Rect,
    pub generation: [Rect; 3],
    pub views: [Rect; 5],
}

/// The view buttons' labels: the main editor (grid, piano roll, song, synth)
/// and the kit editor.
pub const VIEWS: [&str; 5] = ["GRID", "ROLL", "SONG", "SYNTH", "KIT"];

pub fn header_controls(scale: i32) -> HeaderControls {
    let s = scale;
    let (margin, top) = (margin(scale), content_top(scale));
    let y = top + 11 * s;
    let mut x = margin + text_width("orDrumbox", header_text(scale)) + 12 * s;
    let mut next = |w: i32, gap: i32| {
        let rect = Rect::new(x, y, w * s, 16 * s);
        x += (w + gap) * s;
        rect
    };
    let play = next(36, 10);
    let bpm = next(56, 10);
    let pattern = next(150, 10);
    let beats = next(40, 10);
    let kit = next(110, 14);
    let generation = [next(48, 4), next(48, 4), next(64, 14)];
    let views = [next(40, 3), next(40, 3), next(40, 3), next(48, 3), next(40, 0)];
    HeaderControls { play, bpm, pattern, beats, kit, generation, views }
}

/// Top of the footer (status lines then help).
pub fn footer_top(height: i32, scale: i32, status_lines: usize) -> i32 {
    height - margin(scale) - 10 * scale - status_lines as i32 * 10 * scale - 4 * scale
}

/// Height of the output strip always shown along the bottom of the window.
pub fn output_height(scale: i32) -> i32 {
    88 * scale
}

/// How the window is split: one main editor (grid, piano roll, song, synth or
/// the kit editor) in the top-left, the track editor down the right edge (shown
/// or hidden), and the output panel as a full-width strip along the bottom,
/// always shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    /// The main editor's width (the window minus the track editor).
    pub main_w: i32,
    /// The height to hand the main-editor layouts (grid, roll, song, synth,
    /// kit). They place their bottom edge at `footer_top(height, …)`, so this
    /// is the height whose `footer_top` lands just above the output strip —
    /// `height - margin/2 - output_height` — not the region's literal height.
    pub editor_h: i32,
    /// The real bottom edge (y) shared by the main editor and the track editor.
    pub main_bottom: i32,
    /// The output strip's bounds (full width).
    pub output: Rect,
}

impl Body {
    /// The split of a `width` × `height` window at `scale`, `right` pixels
    /// wide on the right for the track editor, with `status_lines` footer lines.
    pub fn new(width: i32, height: i32, scale: i32, right: i32, status_lines: usize) -> Self {
        let margin = margin(scale);
        let output_h = output_height(scale);
        let output_bottom = footer_top(height, scale, status_lines) - margin / 2;
        let output_top = output_bottom - output_h;
        Self {
            main_w: (width - right).max(1),
            editor_h: height - margin / 2 - output_h,
            main_bottom: output_top - margin / 2,
            output: Rect::new(0, output_top, width, output_h),
        }
    }
}

impl Layout {
    /// The rows of `pattern` on a `width` × `height` screen, with
    /// `status_lines` message lines in the footer.
    pub fn new(width: i32, height: i32, scale: i32, pattern: &Pattern, status_lines: usize, page: usize) -> Self {
        let margin = margin(scale);
        let top = body_top(scale);
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let tracks = pattern.tracks.len();
        let row = ((bottom - top) / tracks.max(1) as i32).clamp(10 * scale, 32 * scale);
        let label_scale = if row >= 24 * scale { 2 * scale } else { scale };
        let box_size = (row - 4 * scale).clamp(6 * scale, 14 * scale);
        let left = margin;
        let name_x = left + 2 * (box_size + 3 * scale) + 4 * scale;
        let volume_x = name_x + text_width(&"W".repeat(LABEL_CHARS), label_scale) + 6 * scale;
        let grid_left = volume_x + VOLUME_WIDTH * scale + 8 * scale;
        // Pagination: BEATS_PER_PAGE beats per page (JS BEATS_PER_PAGE).
        let total_beats = pattern.beat_count.max(1);
        let start_beat = (page as i32 * BEATS_PER_PAGE).min(total_beats - 1).max(0);
        let beats = (total_beats - start_beat).clamp(1, BEATS_PER_PAGE);
        // One row under the tracks for the master volume.
        let shown = usize::try_from((bottom - top - row).max(0) / row.max(1)).unwrap_or(0).min(tracks);
        Self {
            scale,
            top,
            row,
            bottom,
            left,
            box_size,
            name_x,
            label_scale,
            volume_x,
            grid_left,
            grid_width: (width - margin - grid_left).max(1),
            beats,
            start_beat,
            tracks: shown,
        }
    }

    fn row_top(&self, track: usize) -> i32 {
        self.top + track as i32 * self.row
    }

    /// The cell of a step of track `track` (`spb` steps per beat).
    pub fn cell_rect(&self, track: usize, beat: i32, step: i32, spb: i32) -> Rect {
        let spb = spb.clamp(1, 16);
        let local_beat = beat - self.start_beat;
        if local_beat < 0 || local_beat >= self.beats {
            return Rect::new(0, 0, 0, 0);
        }
        let at = i64::from(local_beat) * i64::from(spb) + i64::from(step);
        let x = self.step_x(at, spb);
        Rect::new(x, self.row_top(track), self.step_x(at + 1, spb) - x, self.row)
    }

    /// Tracks shown.
    pub fn tracks(&self) -> usize {
        self.tracks
    }

    /// The volume slider of row `track`.
    pub fn volume_rect(&self, track: usize) -> Rect {
        let h = self.row.min(14 * self.scale);
        Rect::new(self.volume_x, self.row_top(track) + (self.row - h) / 2, VOLUME_WIDTH * self.scale, h)
    }

    /// The master volume slider, under the last row.
    pub fn master_rect(&self) -> Rect {
        let h = self.row.min(14 * self.scale);
        let y = self.row_top(self.tracks) + (self.row - h) / 2;
        Rect::new(self.volume_x, y, VOLUME_WIDTH * self.scale, h)
    }

    /// Where the master volume's label goes.
    fn master_label(&self) -> (i32, i32) {
        (self.name_x, self.row_top(self.tracks) + (self.row - 8 * self.label_scale) / 2)
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
            return (x >= self.name_x && x < self.volume_x).then_some(Hit::Name(track));
        }
        let spb = steps_per_beat(&pattern.tracks[track]);
        let step = ((f64::from(x - self.grid_left) * f64::from(spb)) / self.beat_width()) as i64;
        let steps = i64::from(self.beats) * i64::from(spb);
        (step < steps).then(|| Hit::Cell {
            track,
            beat: self.start_beat + (step / i64::from(spb)) as i32,
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
    let margin = margin(scale);

    // Header: the logo, then the labelled groups (their widgets are panel items).
    let top = content_top(scale);
    canvas.text(margin, top + 9 * scale, "orDrumbox", text, LOGO);
    let c = header_controls(scale);
    let label = |canvas: &mut Canvas, r: Rect, text: &str| {
        canvas.text(r.x, top, text, scale, LABEL);
    };
    if view.pattern.is_some() {
        label(canvas, c.bpm, "BPM");
        if let Some(pattern) = view.pattern {
            let pages = (pattern.beat_count.max(1) as usize).div_ceil(BEATS_PER_PAGE as usize).max(1);
            label(canvas, c.pattern, &format!("Page {}/{}", view.current_page + 1, pages));
        }
        label(canvas, c.beats, "BEATS");
        label(canvas, c.kit, "KIT");
        let generate = match view.genre {
            Some(genre) => format!("GENERATE {genre}"),
            None => "GENERATE".to_owned(),
        };
        label(canvas, c.generation[0], &generate);
    } else {
        canvas.text(c.play.x, c.play.y + 4 * scale, "no pattern", scale, DIM);
    }
    label(canvas, c.views[0], "VIEW");

    // Second line: the last action, undo and redo, the song.
    let y = status_line(scale);
    let history = match (view.undo, view.redo) {
        (Some(undo), Some(redo)) => format!("undo: {undo}   redo: {redo}"),
        (Some(undo), None) => format!("undo: {undo}"),
        (None, Some(redo)) => format!("redo: {redo}"),
        (None, None) => String::new(),
    };
    let x = canvas.text(margin, y, view.message, scale, VALUE) + 24 * scale;
    let x = canvas.text(x.max(margin + canvas.width() / 3), y, &history, scale, DIM) + 24 * scale;
    if let Some(song) = view.song {
        let color = if song.starts_with("SONG") { PLAYING } else { DIM };
        canvas.text(x.max(margin + canvas.width() * 2 / 3), y, song, scale, color);
    }
    // The header's lower edge.
    canvas.fill_rect(0, body_top(scale) - margin / 2 + 2 * scale, canvas.width(), 2 * scale, ACCENT);

    // Footer: status lines then help.
    let mut footer_y = footer_top(canvas.height(), scale, view.status.len()) + 4 * scale;
    for message in view.status {
        canvas.text(margin, footer_y, message, scale, WARNING);
        footer_y += 10 * scale;
    }
    let help = if view.kit_view.is_some() {
        KIT_HELP
    } else if view.arrangement.is_some() {
        SONG_HELP
    } else if view.piano_roll.is_some() {
        ROLL_HELP
    } else if view.synth {
        SYNTH_HELP
    } else {
        HELP
    };
    canvas.text(margin, footer_y, help, scale, DIM);

    // The window split: one main editor top-left, the track editor down the
    // right, the output strip along the bottom.
    let body = Body::new(canvas.width(), canvas.height(), scale, view.right, view.status.len());

    // The main editor (top-left): exactly one of the song panel, the kit
    // editor, the synth editor (painted by the session), the piano roll, the grid.
    if let Some(arrangement) = &view.arrangement {
        let layout = SongLayout::new(body.main_w, body.editor_h, scale, view.status.len());
        song_view::draw(canvas, arrangement, &layout);
    } else if let Some(kit) = &view.kit_view {
        let layout = KitLayout::new(body.main_w, body.editor_h, scale, view.status.len());
        draw_kit(canvas, kit, &layout);
    } else if view.synth {
        // Painted by the session (`draw_synth`).
    } else if let Some((roll, title)) = &view.piano_roll {
        let spb = piano_roll::steps_per_beat(roll.track);
        let layout = RollLayout::new(body.main_w, body.editor_h, scale, view.status.len(), spb);
        piano_roll::draw(canvas, roll, &layout, title);
    } else if let Some(pattern) = view.pattern {
        let layout = Layout::new(body.main_w, body.editor_h, scale, pattern, view.status.len(), view.current_page);
        draw_grid(canvas, pattern, view, &layout);
    }

    // The output panel, always shown as a strip along the bottom.
    if let Some(out) = &view.output {
        output::draw(canvas, out, &OutputLayout::strip(canvas.width(), body.output, scale));
    }
}

fn draw_grid(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let gap = scale.max(1);
    for (i, track) in pattern.tracks.iter().enumerate().take(layout.tracks) {
        let y = layout.row_top(i);
        let focused = view.cursor.is_some_and(|c| c.track == i);
        if focused {
            // The whole row, its steps included.
            let right = layout.grid_left + layout.grid_width;
            canvas.fill_rect(layout.left - 4 * scale, y, right - layout.left + 4 * scale, layout.row - gap, FOCUS_ROW);
        }
        // The track's VU meter stripe (level from the audio engine).
        let stripe_x = layout.left - 8 * scale;
        let stripe_w = 3 * scale;
        let stripe_top = y + gap;
        let stripe_h = layout.row - 3 * gap;
        let level = view.levels.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        if view.show_vus {
            canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::mix(theme::track(i), theme::BG, 0.75));
            let fill_h = (f64::from(stripe_h) * f64::from(level)).round() as i32;
            if fill_h > 0 {
                canvas.fill_rect(stripe_x, stripe_top + stripe_h - fill_h, stripe_w, fill_h, theme::track(i));
            }
        } else {
            canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::track(i));
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
        // The name, and under it what the track plays when there is room.
        let sound = view.sounds.get(i).filter(|s| !s.is_empty() && layout.row >= 8 * layout.label_scale + 10 * scale);
        let block = 8 * layout.label_scale + if sound.is_some() { 10 * scale } else { 0 };
        let label_y = y + (layout.row - block) / 2;
        if let Some(sound) = sound {
            let width = layout.volume_x - layout.name_x - 4 * scale;
            let chars = usize::try_from(width / (8 * scale)).unwrap_or(0);
            let shown: String = sound.chars().take(chars).collect();
            canvas.text(layout.name_x, label_y + 8 * layout.label_scale + 2 * scale, &shown, scale, DIM);
        }
        let color = if track.mute {
            DIM
        } else if track.auto {
            PLAYING // a new line every loop
        } else if focused {
            theme::track(i)
        } else {
            TEXT
        };
        canvas.text(layout.name_x, label_y, &label, layout.label_scale, color);
        draw_track_steps(canvas, track, i, view, layout);
    }

    let (x, y) = layout.master_label();
    canvas.text(x, y, "Master", layout.label_scale, DIM);

    if let Some(fraction) = view.playhead {
        let x = layout.grid_left + (fraction.clamp(0.0, 1.0) * f64::from(layout.grid_width)) as i32;
        let height = (layout.tracks as i32 * layout.row).min(layout.bottom - layout.top) - gap;
        canvas.fill_rect(x - scale, layout.top, 2 * scale, height, PLAYHEAD);
    }
}

/// The hover bubble over a note (`pp-tooltip`): centered above `cell`, kept in the window.
pub fn draw_tooltip(canvas: &mut Canvas, text: &str, cell: Rect, scale: i32) {
    let (w, h) = (text_width(text, scale) + 12 * scale, 14 * scale);
    let x = (cell.x + cell.w / 2 - w / 2).clamp(0, (canvas.width() - w).max(0));
    let y = (cell.y - h - 4 * scale).max(0);
    canvas.fill_rect(x, y, w, h, theme::LINE);
    canvas.fill_rect(x + scale, y + scale, w - 2 * scale, h - 2 * scale, theme::SHADE);
    canvas.text(x + 6 * scale, y + 3 * scale, text, scale, TEXT);
}

/// A panel over the window: a title and lines of text (the key list, about).
pub fn draw_notice(canvas: &mut Canvas, title: &str, lines: &[String], scale: i32) {
    let line_h = 11 * scale;
    let width =
        lines.iter().map(|l| text_width(l, scale)).chain([text_width(title, scale)]).max().unwrap_or(0) + 32 * scale;
    let height = (lines.len() as i32 + 3) * line_h + 16 * scale;
    let x = ((canvas.width() - width) / 2).max(0);
    let y = ((canvas.height() - height) / 2).max(menu::bar_height(scale));
    canvas.fill_rect(x - 4 * scale, y - 4 * scale, width + 8 * scale, height + 8 * scale, theme::SHADE);
    canvas.fill_rect(x, y, width, height, theme::LINE);
    canvas.fill_rect(x + scale, y + scale, width - 2 * scale, height - 2 * scale, theme::PANEL);
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
    // Title line above the list.
    canvas.text(layout.left, layout.top + 2 * scale, &format!("kit {}", kit.kit), scale, DIM);
    let first = layout.first(kit.selected);
    if kit.samples.is_empty() {
        canvas.text(layout.left, layout.list_top, "No samples in this kit", scale, DIM);
    }
    for (row, (i, (name, key))) in kit.samples.iter().enumerate().skip(first).take(layout.shown()).enumerate() {
        let y = layout.list_top + row as i32 * layout.row;
        if i == kit.selected {
            canvas.fill_rect(layout.left - 2 * scale, y - 2 * scale, layout.list_width, layout.row, FOCUS_ROW);
        }
        let color = if i == kit.selected { ACCENT } else { TEXT };
        let x = canvas.text(layout.left, y, name, scale, color);
        canvas.text(x + 8 * scale, y, key, scale, DIM);
    }

    let x = layout.detail_x;
    let mut y = kit.details_top + 4 * scale;
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

/// Base colour of a note in the grid, from its own properties (trigger
/// probability, retrigger, euclidean, every-Nth-pass) rather than from its
/// track: a plain note is lime, a probabilistic one cyan, a fixed-trigger
/// one violet, a retriggered one orange, an euclidean one yellow.
fn note_color(note: &Note) -> Rgb {
    if note.euclidean_pulses > 0 {
        theme::YELLOW
    } else if note.retrigger_count > 1 || (note.retrigger_rate - 1.0).abs() > 1e-9 {
        theme::ORANGE
    } else if note.probability < 1.0 {
        theme::CYAN
    } else if note.every > 1 {
        theme::VIOLET
    } else {
        theme::LIME
    }
}

/// One track's steps, its notes lit by velocity, the cursor and the selection.
fn draw_track_steps(canvas: &mut Canvas, track: &Track, index: usize, view: &View, layout: &Layout) {
    let gap = layout.scale.max(1);
    let spb = steps_per_beat(track);
    let steps = i64::from(layout.beats) * i64::from(spb);
    let start = i64::from(layout.start_beat) * i64::from(spb);
    let y = layout.row_top(index);
    let height = layout.row - gap;
    let cell = |step: i64| {
        let x = layout.step_x(step, spb);
        (x, (layout.step_x(step + 1, spb) - x - gap).max(1))
    };

    let mut note_at: Vec<Option<&Note>> = vec![None; steps as usize];
    for note in &track.notes {
        let gstep = absolute_step(note.beat, note.beat_step, spb);
        let local = gstep - start;
        if let Some(slot) = usize::try_from(local).ok().and_then(|s| note_at.get_mut(s)) {
            if slot.is_none_or(|existing| note.velocity > existing.velocity) {
                *slot = Some(note);
            }
        }
    }
    // Ghosts: the sub-events (retriggers, arpeggio steps, euclidean fills)
    // a note produces, drawn smaller than the base note.
    let track_total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let span = ordrumbox_core::pattern_engine::SpanResolver::new(track);
    let mut rng = ordrumbox_core::Rng::new(0);
    let mut ghosts: Vec<(i64, ordrumbox_core::note_positions::SubKind, &Note)> = Vec::new();
    for note in &track.notes {
        for sub in ordrumbox_core::note_positions::sub_positions(note, track, track_total, &span, &mut rng) {
            let local = sub.step - start;
            if (0..steps).contains(&local) {
                ghosts.push((local, sub.kind, note));
            }
        }
    }
    // Loop ghosts: notes that re-trigger when the track loops at loop_length.
    let mut loop_ghosts: Vec<(i64, &Note)> = Vec::new();
    if let Some(loop_length) = track.loop_length.map(i64::from).filter(|&l| l > 0) {
        for note in &track.notes {
            let base = absolute_step(note.beat, note.beat_step, spb);
            if base >= loop_length {
                continue;
            }
            let mut pos = base + loop_length;
            while pos < track_total {
                let local = pos - start;
                if (0..steps).contains(&local) {
                    loop_ghosts.push((local, note));
                }
                pos += loop_length;
            }
        }
    }
    for (step, note) in note_at.iter().enumerate() {
        let step = step as i64;
        let (x, width) = cell(step);
        let is_beat = step % i64::from(spb) == 0;
        let bg = if is_beat { CELL_BEAT } else { CELL };
        canvas.fill_rect(x, y, width, height, bg);
        if let Some(n) = note {
            let color = theme::note(note_color(n), n.velocity);
            canvas.fill_rect(x, y, width, height, color);
        }
        // Ghosts on this step, drawn smaller (half size) and in a distinct colour.
        for (gpos, gkind, gnote) in &ghosts {
            if *gpos != step {
                continue;
            }
            let gcolor = match gkind {
                ordrumbox_core::note_positions::SubKind::Euclidean => theme::YELLOW,
                ordrumbox_core::note_positions::SubKind::Retrigger => theme::MUTED,
            };
            let color = theme::note(gcolor, gnote.velocity);
            let iw = ((f64::from(width) * 0.5).round() as i32).max(1);
            let ih = ((f64::from(height) * 0.5).round() as i32).max(1);
            canvas.fill_rect(x + (width - iw) / 2, y + (height - ih) / 2, iw, ih, color);
        }
        for (gpos, gnote) in &loop_ghosts {
            if *gpos != step {
                continue;
            }
            let color = theme::note(theme::mix(theme::track(index), theme::BG, 0.3), gnote.velocity);
            let iw = ((f64::from(width) * 0.5).round() as i32).max(1);
            let ih = ((f64::from(height) * 0.5).round() as i32).max(1);
            canvas.fill_rect(x + (width - iw) / 2, y + (height - ih) / 2, iw, ih, color);
        }
    }
    // Flash (with glow) the cells currently being played.
    if let Some(frac) = view.playhead {
        let cur = (frac * steps as f64).floor() as i64;
        if (0..steps).contains(&cur) {
            let has_base = note_at.get(cur as usize).copied().flatten().is_some();
            let has_ghost = ghosts.iter().any(|(p, _, _)| *p == cur) || loop_ghosts.iter().any(|(p, _)| *p == cur);
            if has_base || has_ghost {
                let (x, width) = cell(cur);
                let base_color = note_at
                    .get(cur as usize)
                    .copied()
                    .flatten()
                    .map(|n| theme::note(note_color(n), n.velocity))
                    .unwrap_or(CELL);
                let flash = theme::mix(base_color, theme::WHITE, 0.6);
                let glow = theme::mix(flash, theme::BG, 0.5);
                canvas.fill_rect(x - 2 * gap, y - 2 * gap, width + 4 * gap, height + 4 * gap, glow);
                canvas.fill_rect(x, y, width, height, flash);
            }
        }
    }
    if let Some(loop_length) = track
        .loop_length
        .map(i64::from)
        .filter(|&l| l > 0 && l < i64::from(track.steps_per_beat) * i64::from(track.beat_count))
    {
        let local = loop_length - start;
        if (0..steps).contains(&local) {
            let (x, _) = cell(local);
            canvas.fill_rect(x - gap, y, gap * 2, height, LOOP_MARK);
        }
    }

    let outline = |canvas: &mut Canvas, step: i64, color: Rgb| {
        if (0..steps).contains(&step) {
            let (x, width) = cell(step);
            stroke(canvas, x, y, width, height, (2 * layout.scale).max(1), color);
        }
    };
    if let Some(c) = view.cursor.filter(|c| c.track == index) {
        let s = absolute_step(c.beat, c.step, spb) - start;
        outline(canvas, s, CURSOR);
    }
    if let Some(note) = view.selected.filter(|(t, _)| *t == index).and_then(|(_, n)| track.notes.get(n)) {
        let s = absolute_step(note.beat, note.beat_step, spb) - start;
        outline(canvas, s, SELECTED);
    }
}

/// Rectangle outline, `thickness` wide, inside `(x, y, width, height)`.
fn stroke(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, thickness: i32, color: Rgb) {
    canvas.fill_rect(x, y, width, thickness, color);
    canvas.fill_rect(x, y + height - thickness, width, thickness, color);
    canvas.fill_rect(x, y, thickness, height, color);
    canvas.fill_rect(x + width - thickness, y, thickness, height, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;

    fn library() -> Library {
        Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","loopLength":3,"notes":[{},{"beat":9}]},{"name":"X","stepsPerBeat":0},{"name":"H","stepsPerBeat":2}]}]}"#,
        )
        .unwrap()
        .0
    }

    #[test]
    fn draws_without_panicking_at_any_size() {
        let library = library();
        for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let view = View {
                pattern: library.patterns.first(),
                playhead: Some(0.5),
                song: Some("SONG demo  measure 3/66"),
                genre: Some("house"),
                kit_view: None,
                sounds: vec!["real/kick.wav".into(), "synth BASS5".into()],
                arrangement: None,
                piano_roll: None,
                output: None,
                synth: false,
                right: 300,
                current_page: 0,
                levels: vec![0.0; 2],
                show_vus: true,
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
    fn body_splits_the_window_above_the_output_strip() {
        let library = library();
        let pattern = library.patterns.first().unwrap();
        for scale in [1, 2] {
            for (w, h) in [(1100, 680), (1280, 800)] {
                for status in [0, 1, 3] {
                    for right in [0, 300 * scale] {
                        let body = Body::new(w, h, scale, right, status);
                        assert_eq!(body.main_w, w - right);
                        // The strip's bottom edge is the footer's top, less a margin.
                        assert_eq!(body.output.y + body.output.h, footer_top(h, scale, status) - margin(scale) / 2);
                        // The main editor stops above the strip, with a gap.
                        assert!(body.main_bottom < body.output.y);
                        // The editor height is virtual: a layout built with it
                        // (and the real status count) ends exactly at main_bottom.
                        let grid = Layout::new(body.main_w, body.editor_h, scale, pattern, status, 0);
                        assert_eq!(grid.bottom, body.main_bottom);
                    }
                }
            }
        }
    }

    #[test]
    fn kit_editor_draws_and_hits() {
        let samples: Vec<(&str, &str)> = (0..40).map(|_| ("kick.wav", "KICK")).collect();
        let envelope = [0.0, 1.0, 0.5, 0.25];
        for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let kit = KitView {
                kit: "real",
                samples: samples.clone(),
                selected: 39,
                details_top: 200,
                info: Some("A4 0ct".into()),
                detected: Some("Detected: KICK".into()),
                envelope: &envelope,
                decay_marker: Some(0.5),
            };
            draw_kit(&mut canvas, &kit, &KitLayout::new(w as i32, h as i32, 1, 0));
        }
        let layout = KitLayout::new(1024, 640, 1, 0);
        assert_eq!(layout.hit(0, 40, layout.left + 2, layout.list_top + 1), Some(0));
        assert_eq!(layout.hit(0, 1, layout.left + 2, layout.list_top + layout.row + 1), None);
        assert_eq!(layout.hit(0, 40, layout.left + 2, layout.detail_y), None);
    }

    #[test]
    fn hit_testing_matches_the_drawing() {
        let library = library();
        let pattern = &library.patterns[0];
        let layout = Layout::new(1024, 640, 1, pattern, 0, 0);
        let y = layout.row_top(2) + layout.row / 2;
        assert_eq!(layout.hit(pattern, layout.box_x(false) + 1, y), Some(Hit::Mute(2)));
        assert_eq!(layout.hit(pattern, layout.box_x(true) + 1, y), Some(Hit::Solo(2)));
        assert_eq!(layout.hit(pattern, layout.name_x + 2, y), Some(Hit::Name(2)));
        assert_eq!(layout.hit(pattern, layout.volume_x + 2, y), None);
        assert!(layout.volume_rect(2).contains(layout.volume_x + 2, y));
        // Track H: 2 steps per beat; the middle of beat 1's second step.
        let x = layout.step_x(3, 2) + 2;
        assert_eq!(layout.hit(pattern, x, y), Some(Hit::Cell { track: 2, beat: 1, step: 1 }));
        assert_eq!(layout.hit(pattern, layout.grid_left + layout.grid_width + 5, y), None);
        assert_eq!(layout.hit(pattern, x, layout.row_top(3) + 1), None);
        assert_eq!(layout.hit(pattern, x, layout.top - 1), None);
    }
}
