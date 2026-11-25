//! Where everything sits: the grid, the header, the body and what is under the mouse.

use super::{BEATS_PER_PAGE, BeatWindow, LABEL_CHARS, MIN_STEP_WIDTH, PAGE_BEATS, PAGE_STEPS_PER_BEAT, VOLUME_WIDTH};
use crate::canvas::text_width;
use crate::editors;
use crate::frame::{body_top, content_top, footer_top, margin};
use crate::panel::{Rect, row_at};
use crate::pattern_ruler::PatternRuler;
use crate::scale::Scale;
use crate::scroll_thumb::{self, Edge};
use ordrumbox_core::track::MAX_STEPS_PER_BEAT;
use ordrumbox_core::{Pattern, Track};
use ordrumbox_logic::block::BEAT_UNITS;

mod lane;
#[cfg(test)]
pub use lane::lane_tab_rows;
use lane::{LANE_GAP, MAX_LANE};
pub use lane::{MAX_LANE_SET, lane_height};

/// What is under the mouse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    Mute(usize),
    Solo(usize),
    Name(usize),
    Cell {
        track: usize,
        beat: i32,
        step: i32,
    },
    /// The button under the track list that adds a track.
    AddTrack,
}

/// The tallest a track row gets, at scale 1: rows share the grid's height up
/// to this, so a tall window gives bigger cells rather than an empty area.
pub(super) const MAX_ROW: i32 = 56;
/// Height of the add-track button under the rows, and the gap above it, per scale unit.
pub(super) const ADD_TRACK_HEIGHT: i32 = 14;
pub(super) const ADD_TRACK_GAP: i32 = 4;
/// Height of the beat ruler above the rows, per scale unit.
pub(super) const RULER_HEIGHT: i32 = 12;
/// The add-track button's label.
pub(super) const ADD_TRACK_LABEL: &str = "+ NEW TRACK";
/// The shortest thumb of the grid's scroll bar, per scale unit: room for its two handles and a grip.
const BAR_MIN_THUMB: i32 = 3 * scroll_thumb::ZOOM_HANDLE;
/// The left edge of the grid's steps, `box_size` the size of its mute and
/// solo boxes; and the name's, the volume slider's.
pub(super) fn grid_columns(left: i32, box_size: i32, scale: Scale) -> (i32, i32, i32) {
    let name_x = left + 2 * (box_size + 3 * scale) + 4 * scale;
    let volume_x = name_x + text_width(&"W".repeat(LABEL_CHARS), scale) + 6 * scale;
    (name_x, volume_x, volume_x + VOLUME_WIDTH * scale + 8 * scale)
}

/// The beats a page of the grid shows in a main editor `width` wide: the
/// most of [`PAGE_BEATS`] whose steps keep [`MIN_STEP_WIDTH`], else
/// [`BEATS_PER_PAGE`]. The widest boxes are reckoned with, so that the
/// count depends on the window only.
#[must_use]
pub fn page_beats(width: i32, scale: Scale) -> i32 {
    let margin = margin(scale);
    let (_, _, grid_left) = grid_columns(margin, MAX_BOX * scale, scale);
    let grid_width = width - margin - grid_left;
    let fits = |beats: i32| grid_width >= beats * PAGE_STEPS_PER_BEAT * MIN_STEP_WIDTH * scale;
    PAGE_BEATS.into_iter().find(|&beats| fits(beats)).unwrap_or(BEATS_PER_PAGE)
}

/// The biggest mute or solo box, per scale unit.
pub(super) const MAX_BOX: i32 = 14;

/// Where the track rows are.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    pub(super) scale: Scale,
    /// Top of the first row, height of a row, bottom of the grid area.
    pub(super) top: i32,
    pub(super) row: i32,
    pub(super) bottom: i32,
    pub(super) left: i32,
    pub(super) box_size: i32,
    pub(super) name_x: i32,
    /// The volume slider after the name.
    pub(super) volume_x: i32,
    pub(super) grid_left: i32,
    pub(super) grid_width: i32,
    pub(super) beats: i32,
    /// First beat shown.
    pub(super) start_beat: i32,
    /// The pattern's beats, the scroll bar's track.
    pub(super) total_beats: i32,
    pub(super) tracks: usize,
    /// Top of the lane's bars under the rows (their bottom is `bottom`), the
    /// tabs' top or higher.
    pub(super) lane_top: i32,
}

/// Text scale of the header (the 8 px font at 16 px per scale unit).
pub(super) fn header_text(scale: Scale) -> Scale {
    scale.times(2)
}

/// The header, a row of
/// labelled groups after the logo: MODE (the pattern or the song: what PLAY
/// plays and what the main editor shows); play; BPM (the tempo knob); PATTERN;
/// BEATS; KIT; GENERATE (drums, bass, chords); VIEW (grid, piano roll,
/// synth, kit editor).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeaderControls {
    /// The mode switch: the pattern, the song.
    pub mode: [Rect; 2],
    pub play: Rect,
    /// REC beside PLAY: in song mode, the mixer writes the song's automation.
    pub record: Rect,
    pub bpm: Rect,
    pub pattern: Rect,
    pub beats: Rect,
    pub kit: Rect,
    /// The generation: the style list, ⟳, AUTO, and the energy, complexity
    /// and variation macros.
    pub style: Rect,
    pub generate: Rect,
    pub auto: Rect,
    pub macros: [Rect; 3],
    pub views: [Rect; 6],
}

/// The header's macro knobs: their pattern key and label.
pub const MACROS: [(&str, &str); 3] = [
    (ordrumbox_core::style::ENERGY, "ENERGY"),
    (ordrumbox_core::style::COMPLEXITY, "COMPLEXITY"),
    (ordrumbox_core::style::VARIATION, "VARIATION"),
];

/// The mode switch's labels: what the window edits and plays.
pub const MODES: [&str; 2] = ["PATT", "SONG"];

/// The view buttons' labels: the main editor (grid, piano roll, song, synth,
/// mixer) and the kit editor. The song has no button of its own: it is the
/// mode switch's (its slot has no width).
pub const VIEWS: [&str; 6] = ["GRID", "ROLL", "SONG", "SYNTH", "MIX", "KIT"];

/// The kit editor's view button, the last: it opens over the main editor.
pub const KIT_VIEW: u8 = 5;

pub fn header_controls(scale: Scale) -> HeaderControls {
    let s = scale;
    let (margin, top) = (margin(scale), content_top(scale));
    let y = top + 11 * s;
    let mut x = margin + text_width("orDrumbox", header_text(scale)) + 12 * s;
    let mut next = |w: i32, gap: i32| {
        let rect = Rect::new(x, y, w * s, 16 * s);
        x += (w + gap) * s;
        rect
    };
    // Close inside a group, wide between groups: the same total width as
    // when every gap was alike, so the window's minimum width is kept.
    let mode = [next(52, 2), next(52, HEADER_GROUP_GAP)];
    let play = next(36, 4);
    let record = next(30, 4);
    let bpm = next(52, HEADER_GROUP_GAP);
    let pattern = next(104, 4);
    let beats = next(36, 4);
    let kit = next(84, HEADER_GROUP_GAP);
    let style = next(90, 4);
    let generate = next(22, 4);
    let auto = next(38, 6);
    let macros = [next(44, 4), next(64, 4), next(56, HEADER_GROUP_GAP + 2)];
    let (grid, roll) = (next(40, 2), next(40, 2));
    // The song's slot, closed: the mode switch is its button.
    let song = next(0, 0);
    let views = [grid, roll, song, next(48, 2), next(36, 2), next(40, 0)];
    HeaderControls { mode, play, record, bpm, pattern, beats, kit, style, generate, auto, macros, views }
}

/// The gap between two of the header's groups, at scale 1: room for both
/// groups' frames and a gap between them.
pub(super) const HEADER_GROUP_GAP: i32 = 14;
/// How far a header group's frame reaches beyond its controls, at scale 1.
pub(super) const HEADER_GROUP_PAD: i32 = 4;

impl HeaderControls {
    /// The song list of the song mode, in the place of the pattern
    /// list and the beats.
    #[must_use]
    pub fn song(&self) -> Rect {
        Rect::new(self.pattern.x, self.pattern.y, self.beats.x + self.beats.w - self.pattern.x, self.pattern.h)
    }

    /// The frames of the header's groups: mode (pattern or song), transport
    /// (play, tempo), pattern (pattern, beats, kit), generation (style, ⟳,
    /// AUTO, macros) and views. Each covers its controls and their labels
    /// above them.
    #[must_use]
    pub fn groups(&self, scale: Scale) -> [Rect; 5] {
        let pad = HEADER_GROUP_PAD * scale;
        let top = content_top(scale) - 3 * scale;
        let frame = |first: Rect, last: Rect| {
            Rect::new(first.x - pad, top, last.x + last.w - first.x + 2 * pad, first.y + first.h + 3 * scale - top)
        };
        [
            frame(self.mode[0], self.mode[1]),
            frame(self.play, self.bpm),
            frame(self.pattern, self.kit),
            frame(self.style, self.macros[2]),
            frame(self.views[0], self.views[5]),
        ]
    }
}

/// The room between the main editor and the note editor under it, per scale unit.
pub(super) const NOTE_GAP: i32 = 14;

/// The width of the track editor, at scale 1 (half the window at most): the
/// label column and four knob slots, the FX and LFO strips' widest row.
pub const TRACK_EDITOR_WIDTH: i32 = 310;

/// The height the track editor takes at most, at scale 1: the FX tab's six
/// effects, one strip each (the compressor's on two rows of knobs), under the sample bar (kept even without a
/// sample). The note editor is under the grid and the piano roll, not here.
/// A test checks that every tab fits in it.
pub const TRACK_EDITOR_HEIGHT: i32 = 503;

/// The pixels the track editor's rows add up to beyond `TRACK_EDITOR_HEIGHT *
/// scale` at a scale of an odd number of quarters (×1.25, ×1.75…): each row
/// is rounded to the pixel on its own. A test checks every scale up to
/// [`Scale::MAX`].
pub(super) const TRACK_EDITOR_ROUNDING: i32 = 9;

/// The footer status lines the smallest window leaves room for.
pub(super) const MIN_STATUS_LINES: usize = 1;

/// The track rows the grid has room for without the track editor, at their
/// smallest: a compact window (the grid alone) still shows
/// a full drum pattern.
pub(super) const COMPACT_ROWS: i32 = 16;

/// The smallest row the grid draws, per scale unit.
pub(super) const MIN_ROW: i32 = 10;

/// The height the grid takes at least without the track editor: the ruler,
/// [`COMPACT_ROWS`] rows, the add-track button and the lane, each rounded to
/// the pixel as [`Layout::new`] rounds it.
pub(super) fn compact_grid_height(scale: Scale) -> i32 {
    let rows = COMPACT_ROWS * (MIN_ROW * scale);
    RULER_HEIGHT * scale + rows + (ADD_TRACK_GAP + ADD_TRACK_HEIGHT) * scale + LANE_GAP * scale + lane_height() * scale
}

/// What the window shows that sets the room the layout needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shown {
    /// The track editor down the right edge.
    pub track_editor: bool,
    /// The main editor is the grid (another one keeps the track editor's height).
    pub grid: bool,
    /// The note editor under the grid.
    pub note_editor: bool,
}

impl Shown {
    /// Everything, the tallest state.
    #[must_use]
    pub const fn all() -> Self {
        Self { track_editor: true, grid: true, note_editor: true }
    }

    /// The compact view: the grid alone.
    #[must_use]
    pub const fn grid_alone() -> Self {
        Self { track_editor: false, grid: true, note_editor: false }
    }
}

/// The smallest window, in physical pixels, that the layout fits at `scale`
/// with what `shown` shows: the header's controls across; down, the header,
/// the tallest track editor (or, the grid without it, the grid and its note
/// editor) and a footer of one status line.
pub fn min_window_size(scale: Scale, shown: Shown) -> (i32, i32) {
    let views = header_controls(scale).views;
    let width = views[5].x + views[5].w + margin(scale);
    // `Body::new` places the editors' bottom at a fixed distance above the
    // window's bottom edge: at a height of 0 it is minus that distance.
    let below = -Body::new(0, 0, scale, 0, MIN_STATUS_LINES, false).main_bottom;
    let main = if shown.track_editor || !shown.grid {
        let rounding = if scale.quarters() % 2 == 1 { TRACK_EDITOR_ROUNDING } else { 0 };
        TRACK_EDITOR_HEIGHT * scale + rounding
    } else {
        let note = if shown.note_editor { editors::note_strip_height(scale) + NOTE_GAP * scale } else { 0 };
        compact_grid_height(scale) + note
    };
    (width, body_top(scale) + main + below)
}

/// How the window is split: one main editor (grid, piano roll, song, synth,
/// mixer or the kit editor) in the top-left, under the grid and the piano
/// roll the note editor, the track editor down the right edge (shown or hidden).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body {
    /// The main editor's width (the window minus the track editor).
    pub main_w: i32,
    /// The height to hand the main-editor layouts (grid, roll, song, synth,
    /// kit). They place their bottom edge at `footer_top(height, …)`, so this
    /// is the height whose `footer_top` lands just above the editors' bottom
    /// — `height - margin/2` — not the region's literal height.
    pub editor_h: i32,
    /// The real bottom edge (y) shared by the main editor (its note editor
    /// included) and the track editor.
    pub main_bottom: i32,
    /// The note editor, under the main editor and as wide (0 high: none).
    pub note: Rect,
}

impl Body {
    /// The split of a `width` × `height` window at `scale`, `right` pixels
    /// wide on the right for the track editor, with `status_lines` footer
    /// lines; `note`: with the note editor under the main editor (the
    /// grid's, the piano roll's).
    pub fn new(width: i32, height: i32, scale: Scale, right: i32, status_lines: usize, note: bool) -> Self {
        let margin = margin(scale);
        let main_bottom = footer_top(height, scale, status_lines) - margin;
        let main_w = (width - right).max(1);
        // The note editor's card reaches a few pixels around it, as the
        // track editor's do: the room above it keeps them apart.
        let note_h = if note { editors::note_strip_height(scale) } else { 0 };
        let above_note = if note { NOTE_GAP * scale } else { 0 };
        let note_top = main_bottom - note_h;
        Self {
            main_w,
            editor_h: height - margin / 2 - note_h - above_note,
            main_bottom,
            note: Rect::new(margin, note_top, (main_w - 2 * margin).max(1), note_h),
        }
    }
}

impl Layout {
    /// The rows of `pattern` on a `width` × `height` screen, with
    /// `status_lines` message lines in the footer.
    pub fn new(
        width: i32,
        height: i32,
        scale: Scale,
        pattern: &Pattern,
        status_lines: usize,
        window: BeatWindow,
        lane: Option<i32>,
    ) -> Self {
        let margin = margin(scale);
        let top = body_top(scale) + RULER_HEIGHT * scale;
        let bottom = footer_top(height, scale, status_lines) - margin / 2;
        let tracks = pattern.tracks.len();
        // The lane keeps the bottom of the grid; the rows share the height
        // left above the add-track button, and the lane's bars take what
        // the rows leave (its tabs stay at the bottom).
        let tabs_top = bottom - lane_height() * scale;
        let fixed = (ADD_TRACK_HEIGHT + ADD_TRACK_GAP + LANE_GAP) * scale;
        // A lane set by the user (`lane`, in scale units) is as tall as it
        // asks, within what leaves the rows their smallest size.
        let tallest = (bottom - top - fixed - tracks.max(1) as i32 * MIN_ROW * scale).max(lane_height() * scale);
        let set_height = lane.map(|h| (h.clamp(lane_height(), MAX_LANE_SET) * scale).min(tallest));
        let lane_edge = set_height.map_or(tabs_top, |h| bottom - h);
        let rows_h = lane_edge - top - fixed;
        let row = (rows_h / tracks.max(1) as i32).clamp(MIN_ROW * scale, MAX_ROW * scale);
        let box_size = (row - 4 * scale).clamp(6 * scale, MAX_BOX * scale);
        let left = margin;
        let (name_x, volume_x, grid_left) = grid_columns(left, box_size, scale);
        let total_beats = pattern.beat_count.max(1);
        let page_beats = window.shown(page_beats(width, scale), total_beats);
        let start_beat = window.first().clamp(0, total_beats - 1);
        let beats = (total_beats - start_beat).clamp(1, page_beats);
        let shown = usize::try_from(rows_h.max(0) / row.max(1)).unwrap_or(0).min(tracks);
        let under_rows = top + shown as i32 * row + fixed;
        let lane_top = match set_height {
            Some(h) => (bottom - h).max(under_rows).min(tabs_top),
            None => under_rows.max(bottom - MAX_LANE * scale).min(tabs_top),
        };
        Self {
            scale,
            top,
            row,
            bottom,
            left,
            box_size,
            name_x,
            volume_x,
            grid_left,
            grid_width: (width - margin - grid_left).max(1),
            beats,
            start_beat,
            total_beats,
            tracks: shown,
            lane_top,
        }
    }

    /// First beat shown.
    #[must_use]
    pub const fn start_beat(&self) -> i32 {
        self.start_beat
    }

    /// The beats a full window shows.
    #[must_use]
    pub const fn beats(&self) -> i32 {
        self.beats
    }

    /// The pattern's beats.
    #[must_use]
    pub const fn total_beats(&self) -> i32 {
        self.total_beats
    }

    /// The steps' area: the ruler and the rows' width, down to the lane.
    #[must_use]
    pub fn beats_area(&self) -> Rect {
        let top = self.top - RULER_HEIGHT * self.scale;
        Rect::new(self.grid_left, top, self.grid_width, self.bottom - top)
    }

    /// The ruler's beats and steps (those of a track of `steps_per_beat`),
    /// above the rows.
    #[must_use]
    pub fn ruler(&self, steps_per_beat: i32) -> PatternRuler {
        let rect =
            Rect::new(self.grid_left, self.top - RULER_HEIGHT * self.scale, self.grid_width, RULER_HEIGHT * self.scale);
        PatternRuler::new(rect, (f64::from(self.start_beat), self.beat_width()), steps_per_beat, self.scale)
    }

    /// The grid's scroll bar, on the add-track button's row, as wide as the steps.
    #[must_use]
    pub fn scroll_bar(&self) -> Rect {
        let button = self.add_track_rect();
        Rect::new(self.grid_left, button.y, self.grid_width, button.h)
    }

    /// The bar's track: where its thumb slides, along x.
    #[must_use]
    pub fn bar_track(&self) -> (i32, i32) {
        (self.grid_left, self.grid_width)
    }

    /// The bar's thumb `(start, len)`: the beats shown, of the pattern's.
    #[must_use]
    pub fn bar_thumb(&self) -> (i32, i32) {
        let (first, shown, total) = (self.start_beat, self.beats, self.total_beats);
        scroll_thumb::thumb(
            self.bar_track(),
            first as usize,
            shown as usize,
            total as usize,
            BAR_MIN_THUMB * self.scale,
        )
    }

    /// The first beat that puts the thumb centred on `x`.
    #[must_use]
    pub fn first_beat_at(&self, x: i32) -> i32 {
        let (shown, total) = (self.beats as usize, self.total_beats as usize);
        scroll_thumb::first_at(self.bar_track(), x, shown, total, BAR_MIN_THUMB * self.scale) as i32
    }

    /// The beat (fractional) the bar's track puts at `x`.
    #[must_use]
    pub fn track_beat(&self, x: i32) -> f64 {
        let (start, len) = self.bar_track();
        f64::from(x - start) / f64::from(len.max(1)) * f64::from(self.total_beats)
    }

    /// The beat (fractional) the steps put at `x`.
    #[must_use]
    pub fn beat_under(&self, x: i32) -> f64 {
        f64::from(self.start_beat) + f64::from(x - self.grid_left) / self.beat_width()
    }

    /// What of the scroll bar is at `(x, y)`: `Some(None)` its track or
    /// thumb (a drag scrolls), `Some(Some(edge))` a zoom handle.
    #[must_use]
    pub fn bar_at(&self, x: i32, y: i32) -> Option<Option<Edge>> {
        self.scroll_bar()
            .contains(x, y)
            .then(|| scroll_thumb::handle_at(x, self.bar_thumb(), scroll_thumb::ZOOM_HANDLE * self.scale))
    }

    pub(crate) fn row_top(&self, track: usize) -> i32 {
        self.top + track as i32 * self.row
    }

    /// The cell of a step of track `track` (`track_data` gives its steps per beat).
    pub fn cell_rect(&self, track: usize, beat: i32, step: i32, track_data: &Track) -> Rect {
        let spb = steps_per_beat(track_data);
        let local_beat = beat - self.start_beat;
        if local_beat < 0 || local_beat >= self.beats {
            return Rect::new(0, 0, 0, 0);
        }
        let at = i64::from(local_beat) * i64::from(spb) + i64::from(step);
        let x = self.step_x(at, spb);
        Rect::new(x, self.row_top(track), self.step_x(at + 1, spb) - x, self.row)
    }

    /// The x of a time in units ([`BEAT_UNITS`] a beat), kept on the page.
    pub(super) fn units_x(&self, units: i64) -> i32 {
        let page = (units - i64::from(self.start_beat) * BEAT_UNITS).clamp(0, i64::from(self.beats) * BEAT_UNITS);
        self.grid_left + (page as f64 * self.beat_width() / BEAT_UNITS as f64) as i32
    }

    /// The step under `(x, y)`, or the nearest of the page when it is off
    /// the steps: track, beat and step. A drag that leaves the grid ends on
    /// its edge.
    #[must_use]
    pub fn nearest_cell(&self, pattern: &Pattern, x: i32, y: i32) -> Option<(usize, i32, i32)> {
        let rows = i32::try_from(self.tracks.min(pattern.tracks.len())).ok().filter(|&r| r > 0)?;
        let y = y.clamp(self.top, self.top + rows * self.row - 1);
        let x = x.clamp(self.grid_left, self.grid_left + self.grid_width - 1);
        match self.spot_at(pattern, x, y)? {
            Spot::Cell { track, beat, step } => Some((track, beat, step)),
            Spot::Mute(_) | Spot::Solo(_) | Spot::Name(_) | Spot::AddTrack => None,
        }
    }

    /// Tracks shown.
    pub fn tracks(&self) -> usize {
        self.tracks
    }

    /// The row a track dragged to `y` takes: the one under it, else the
    /// first or the last shown.
    pub fn drop_row(&self, y: i32) -> usize {
        let row = (y - self.top).max(0) / self.row.max(1);
        usize::try_from(row).unwrap_or(0).min(self.tracks.saturating_sub(1))
    }

    /// The add-track button, under the last row shown, as wide as the
    /// columns before the steps.
    pub fn add_track_rect(&self) -> Rect {
        let y = self.row_top(self.tracks) + ADD_TRACK_GAP * self.scale;
        let w = self.volume_x + VOLUME_WIDTH * self.scale - self.left;
        Rect::new(self.left, y, w, ADD_TRACK_HEIGHT * self.scale)
    }

    /// The name (and sound) of row `track`.
    pub fn name_rect(&self, track: usize) -> Rect {
        Rect::new(self.name_x, self.row_top(track), self.volume_x - self.name_x, self.row)
    }

    /// The volume slider of row `track`.
    pub fn volume_rect(&self, track: usize) -> Rect {
        let h = self.row.min(14 * self.scale);
        Rect::new(self.volume_x, self.row_top(track) + (self.row - h) / 2, VOLUME_WIDTH * self.scale, h)
    }

    pub(super) fn beat_width(&self) -> f64 {
        f64::from(self.grid_width) / f64::from(self.beats)
    }

    pub(super) fn box_x(&self, solo: bool) -> i32 {
        self.left + if solo { self.box_size + 3 * self.scale } else { 0 }
    }

    /// Left edge of `step` (from the track start) at `spb` steps per beat.
    pub(super) fn step_x(&self, step: i64, spb: i32) -> i32 {
        self.grid_left + (step as f64 * self.beat_width() / f64::from(spb)) as i32
    }

    /// Left edge of beat `beat` of the page (0 is its first beat).
    pub(super) fn beat_x(&self, beat: i32) -> i32 {
        self.step_x(i64::from(beat), 1)
    }

    /// What is at `(x, y)`.
    pub fn spot_at(&self, pattern: &Pattern, x: i32, y: i32) -> Option<Spot> {
        if y < self.top || x < self.left {
            return None;
        }
        if self.add_track_rect().contains(x, y) {
            return Some(Spot::AddTrack);
        }
        let track = row_at(y, self.top, self.row).filter(|&t| t < self.tracks)?;
        let in_box = |solo: bool| {
            let bx = self.box_x(solo);
            let by = self.row_top(track) + (self.row - self.box_size) / 2;
            (bx..bx + self.box_size).contains(&x) && (by..by + self.box_size).contains(&y)
        };
        if in_box(false) {
            return Some(Spot::Mute(track));
        }
        if in_box(true) {
            return Some(Spot::Solo(track));
        }
        if x < self.grid_left {
            return (x >= self.name_x && x < self.volume_x).then_some(Spot::Name(track));
        }
        let spb = steps_per_beat(&pattern.tracks[track]);
        let step = ((f64::from(x - self.grid_left) * f64::from(spb)) / self.beat_width()) as i64;
        let steps = i64::from(self.beats) * i64::from(spb);
        (step < steps).then(|| Spot::Cell {
            track,
            beat: self.start_beat + (step / i64::from(spb)) as i32,
            step: (step % i64::from(spb)) as i32,
        })
    }
}

/// The steps per beat the grid shows of `track`.
pub(crate) fn steps_per_beat(track: &Track) -> i32 {
    track.steps_per_beat.clamp(1, MAX_STEPS_PER_BEAT)
}
