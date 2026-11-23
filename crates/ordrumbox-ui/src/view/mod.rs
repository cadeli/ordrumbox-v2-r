//! The screen: header (pattern, generation, tempo, length, kit, transport,
//! undo), then the song panel (`song_view`), the kit editor, or one row per track (mute and solo boxes, name — violet in auto mode —,
//! steps with their notes), the grid cursor, the selected note, the playhead,
//! the messages and the key help.
//!
//! [`Layout`] places everything; drawing and mouse hit-testing both use it.

use ordrumbox_core::Pattern;
use ordrumbox_logic::block::Zone;

use crate::canvas::Rgb;

/// The fewest beats a page of the pattern grid shows; a wide grid shows 8
/// or 16 ([`page_beats`]).
pub const BEATS_PER_PAGE: i32 = 4;
/// The beats a page of the grid may show, the most first: whole measures of 4/4.
const PAGE_BEATS: [i32; 3] = [16, 8, BEATS_PER_PAGE];
/// The narrowest step, at scale 1, of a track of [`PAGE_STEPS_PER_BEAT`]
/// steps per beat, for a page of more than [`BEATS_PER_PAGE`] beats.
const MIN_STEP_WIDTH: i32 = 16;
/// The steps per beat a page's width is reckoned for (the default): the
/// beats of a page depend on the window only, not on the tracks.
const PAGE_STEPS_PER_BEAT: i32 = 4;
use crate::pattern_ruler::RulerMarks;
use crate::piano_roll::RollView;
use crate::song_view::SongView;
use crate::theme;

fn background_color() -> Rgb {
    theme::bg()
}
fn cell_color() -> Rgb {
    theme::cell()
}
fn cell_beat_color() -> Rgb {
    theme::cell_beat()
}
/// A sample's envelope in the kit editor: the sound's shape.
const ENVELOPE: Rgb = theme::LIVE;
const ACCENT: Rgb = theme::ACCENT;
const PLAYING: Rgb = theme::LIVE;
/// A track in auto mode (a new line every loop).
const AUTO: Rgb = theme::CHANCE;
/// Above the pattern list while the pattern shown waits for the one playing to end its loop.
const WAITING_LABEL: &str = "NEXT";
/// The label above the pattern list.
const PATTERN_LABEL: &str = "PATTERN";
const ARRANGEMENT_LABEL: &str = "ARRANGEMENT";
const WARNING: Rgb = theme::ALERT;
const PLAYHEAD: Rgb = theme::LIVE;
const LOOP_MARK: Rgb = theme::VALUE;
const CURSOR: Rgb = theme::FOCUS;
const SELECTED: Rgb = theme::WHITE;
fn focus_row_color() -> Rgb {
    theme::selected_row()
}
const MUTE_ON: Rgb = theme::ALERT;
/// A track a plugin host mutes: a mute, dimmed.
fn mute_host_color() -> Rgb {
    theme::mix(theme::ALERT, theme::bg(), 0.5)
}
const LOGO: Rgb = theme::ACCENT;
const SOLO_ON: Rgb = theme::VALUE;
fn box_off_color() -> Rgb {
    theme::line()
}

/// The key help line under the grid (every command is in the menus, with
/// its shortcut; Help > Keyboard shortcuts lists them all).
const HELP: &str = "F10 or ALT: menus   ARROWS: cursor   ENTER or click: step on/off   SHIFT+ARROWS: select   ALT+ARROWS: nudge note   TAB: track editor   F1: all keys";
/// The same in the song panel.
const SONG_HELP: &str = "SONG   click: act   right-click: menu   SPACE: play   ARROWS: cursor, pattern   DELETE: the clip selected   F3 or ESC: grid";
/// The same in the piano roll.
const ROLL_HELP: &str =
    "PIANO ROLL   click: add / select / delete   drag: velocity / pitch   right-click: menu   F4 or ESC: grid";
/// The same in the synth editor.
const SYNTH_HELP: &str = "SYNTH   TAB: controls   drag, wheel: knobs   click: waves, lists, power   F6 or ESC: grid";
/// The same in the mixer.
const MIXER_HELP: &str =
    "MIXER   drag, wheel: knobs and faders   wheel on a strip: scroll   click a name: track editor   F12 or ESC: grid";
/// The same in the kit editor.
const KIT_HELP: &str =
    "KIT EDITOR   UP DOWN: sample   TAB: controls   ENTER: hear   DEL: remove   N: normalize   ESC: close";

/// Characters of a track name shown.
const LABEL_CHARS: usize = 10;
/// Width of a row's volume slider, per scale unit.
const VOLUME_WIDTH: i32 = 48;

/// What a track plays, in the two small lines under its name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundLabel {
    /// Where the sound comes from: its kit (after `for CONGAS:` when it
    /// stands in for the track's instrument), `synth`; empty for none.
    pub source: String,
    /// The sample's file name without its extension, the synth preset, or
    /// `no sound`.
    pub name: String,
}

impl SoundLabel {
    /// The label of the sample at `path` (`kit/file.wav`): the kit above, the
    /// file without its extension below.
    #[must_use]
    pub fn sample(path: &str) -> Self {
        let (kit, file) = path.rsplit_once('/').unwrap_or(("", path));
        let name = file.rsplit_once('.').map_or(file, |(stem, _)| if stem.is_empty() { file } else { stem });
        Self { source: kit.to_owned(), name: name.to_owned() }
    }
}

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
    /// The pattern shown waits for another to end its loop: the blink of
    /// its marker, on or off.
    pub waiting: Option<bool>,
    /// The selected song, or the song playing and where.
    pub song: Option<&'a str>,
    pub cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    pub selected: Option<(usize, usize)>,
    /// The zone drawn over the grid (Shift+drag).
    pub zone: Option<Zone>,
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
    /// The synth editor is shown (the session paints it).
    pub synth: bool,
    /// The mixer is shown (the session paints it).
    pub mixer: bool,
    /// What each track plays (its sample, its synth preset), under its name.
    pub sounds: Vec<SoundLabel>,
    /// Width kept on the right (for the track editor).
    pub right: i32,
    /// The beats of the pattern the grid shows.
    pub beat_window: BeatWindow,
    /// The loop and the play cursor on the ruler.
    pub ruler_marks: RulerMarks,
    /// The note editor is under the main editor (the grid, the piano roll).
    pub note_editor: bool,
    /// The track the lane under the grid shows, and the lane (an index in
    /// `editors::LANES`).
    pub lane: (Option<usize>, usize),
    /// Per-track levels (0–1) for the VU meter stripe, when playing.
    pub levels: Vec<f32>,
    /// Whether the VU meter stripe is shown.
    pub show_vus: bool,
    /// What the header's first group shows.
    pub transport: Transport,
    /// A track being dragged by its name: from which row to which.
    pub track_drop: Option<(usize, usize)>,
    /// The tracks a plugin host mutes, bit `t` for track `t`.
    pub host_muted: u64,
    /// Mute and solo as shown, by track: while the song plays, what its
    /// automation lanes set; empty, or past its end, the tracks' own.
    pub switches: Vec<(bool, bool)>,
    /// The help line for what is under the pointer; `None`: the view's keys.
    pub hint: Option<&'a str>,
    /// The track whose name is under the pointer: drawn lighter, with the mark of its menu.
    pub hovered_track: Option<usize>,
    /// Mute and solo taken back from their lanes, by track: marked on their boxes.
    pub switches_taken_back: Vec<(bool, bool)>,
    /// The tracks a plugin host moves off their settings, bit `t` for track `t`.
    pub host_moved: u64,
    /// The tracks a plugin host plays on an output of their own, bit `t` for track `t`.
    pub host_split: u64,
}

mod beat_window;
mod draw;
mod grid;
mod kit;
mod layout;
mod notice;
#[cfg(test)]
mod tests;

pub use beat_window::{BarHold, BeatWindow};
pub use draw::*;
pub use kit::*;
pub use layout::*;
pub use notice::*;
