//! The actions of the menus a right-click or a list opens: each menu has its
//! enum, each line of it holds one action, picked as it is (`Session::pick`).

use crate::view::Cursor;
use ordrumbox_core::automation::AutomationTarget;
use ordrumbox_engine::synth_preset::tags::Facet;
use std::path::PathBuf;

/// The action of a line of an open menu, by the menu it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub enum MenuAction {
    Song(SongAction),
    Loop(LoopAction),
    /// A project of the Open list, loaded once confirmed.
    LoadProject(PathBuf),
    Roll(RollAction),
    Lane(LaneAction),
    Automation(AutomationAction),
    Grid(GridAction),
    /// A line of the instrument menus (renaming or adding a track, a kit
    /// sample's instrument, the Sound tab's instrument).
    Instrument(InstrumentAction),
    Control(ControlAction),
    Pattern(PatternAction),
    SynthPreset(SynthPresetAction),
}

/// Each menu's action as a line of an open menu.
macro_rules! menu_action_from {
    ($($action:ident => $variant:ident),* $(,)?) => {$(
        impl From<$action> for MenuAction {
            fn from(action: $action) -> Self {
                Self::$variant(action)
            }
        }
    )*};
}

menu_action_from! {
    SongAction => Song,
    LoopAction => Loop,
    RollAction => Roll,
    LaneAction => Lane,
    AutomationAction => Automation,
    GridAction => Grid,
    InstrumentAction => Instrument,
    ControlAction => Control,
    PatternAction => Pattern,
    SynthPresetAction => SynthPreset,
}

/// An action of a song panel menu: a pattern of the list, a clip, a row's
/// name, the bare grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SongAction {
    /// Pattern `pattern_id` added to the song at `measure` (0-based).
    Add {
        measure: u32,
        pattern_id: String,
    },
    /// The clip repeated after itself.
    RepeatClip(usize),
    RemoveClip(usize),
    MoveRowUp(usize),
    MoveRowDown(usize),
    /// Every clip of the pattern of this id removed: its row.
    RemoveRow(String),
    /// A pattern of the library, by its index.
    MovePatternUp(usize),
    MovePatternDown(usize),
    RenamePattern(usize),
    RemovePattern(usize),
}

/// An action of the loop's menu (the song ruler's).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopAction {
    Toggle,
    Clear,
}

/// An action of a piano roll menu, on the track it shows.
#[derive(Debug, Clone, PartialEq)]
pub enum RollAction {
    /// Every note of the track removed.
    Clear,
    /// The track's notes replaced by the next progression from `tonic`.
    Sequence { tonic: f64 },
    /// A note added at absolute step `step`, at `pitch` (from the track's
    /// transposition).
    AddNote { step: i64, pitch: f64 },
    /// The track's note of this index deleted.
    DeleteNote(usize),
    /// The notes of `chord` from `root` added at absolute step `step`.
    AddChord { chord: RollChord, step: i64, root: f64 },
}

/// A chord type offered by the piano roll's grid menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RollChord {
    Minor,
    Major,
    Diminished,
    Major7,
    Minor7,
    Sus4,
    Seventh,
}

impl RollChord {
    /// The chords the menu offers, in its order: the common triads first,
    /// then the enriched (seventh) ones.
    pub const ALL: [Self; 7] =
        [Self::Minor, Self::Major, Self::Diminished, Self::Major7, Self::Minor7, Self::Sus4, Self::Seventh];

    /// Its name in the menu and in the result message (`Added minor chord`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Diminished => "diminished",
            Self::Major7 => "major7",
            Self::Minor7 => "minor7",
            Self::Sus4 => "sus4",
            Self::Seventh => "seventh",
        }
    }

    /// Its intervals in semitones from the root.
    #[must_use]
    pub const fn intervals(self) -> &'static [f64] {
        match self {
            Self::Minor => &[0.0, 3.0, 7.0],
            Self::Major => &[0.0, 4.0, 7.0],
            Self::Diminished => &[0.0, 3.0, 6.0],
            Self::Major7 => &[0.0, 4.0, 7.0, 11.0],
            Self::Minor7 => &[0.0, 3.0, 7.0, 10.0],
            Self::Sus4 => &[0.0, 5.0, 7.0],
            Self::Seventh => &[0.0, 4.0, 7.0, 10.0],
        }
    }
}

/// An action of the menu of the grid's lane (a note setting drawn as bars).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneAction {
    /// The notes of this absolute step back to the setting's default.
    ResetStep(i64),
    /// Every note of the track back to the default.
    ResetAll,
    /// The lane's height back to automatic.
    AutoHeight,
}

/// An action of the song's automation menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutomationAction {
    /// The controls of this instrument, opened beside.
    Instrument(String),
    /// A lane of `target` for `instrument` added.
    AddLane {
        target: AutomationTarget,
        instrument: String,
    },
    /// A lane, by its index, that a control took back, played again.
    Reenable(usize),
    Taller(usize),
    Shorter(usize),
    /// Every point of the lane removed.
    Clear(usize),
    Remove(usize),
    RemoveAll,
}

/// An action of a grid menu: a track's, or a step's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GridAction {
    /// An action of the menu of the track of this index.
    Track(usize, TrackAction),
    /// An action of the menu of the step at this place.
    Step(Cursor, StepAction),
}

impl GridAction {
    /// Where the cursor goes when it runs: the step, or the track's first one.
    #[must_use]
    pub const fn cursor(self) -> Cursor {
        match self {
            Self::Track(track, _) => Cursor { track, beat: 0, step: 0 },
            Self::Step(at, _) => at,
        }
    }
}

/// An action of a track's menu in the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackAction {
    /// The instrument menus that rename it, opened beside.
    Rename,
    Copy,
    Paste,
    MoveUp,
    MoveDown,
    Duplicate,
    Delete,
    Randomize,
    Clear,
    /// The instrument menus that add a track, opened beside.
    AddTrack,
}

/// An action of a step's menu in the grid; the zone's act on the zone drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepAction {
    CopyZone,
    CutZone,
    DeleteZone,
    CopyNotes,
    PasteNotes,
    DeleteNotes,
    RandomNote,
}

/// A line of the instrument menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstrumentAction {
    /// The instruments of the catalog's category of this index, opened beside.
    Category(usize),
    /// The instrument of this id.
    Instrument(String),
    /// The entry of any name (renaming a track only).
    FreeName,
}

/// An action of a knob's or a slider's right-click menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlAction {
    /// The entry of a typed value.
    Type,
    Default,
    Copy,
    Paste,
}

/// A line of the header's PATTERN menus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternAction {
    /// The patterns of the style group of this index, opened beside.
    Style(usize),
    /// The pattern of the library of this index, shown.
    Pattern(usize),
}

/// A line of the synth preset menus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SynthPresetAction {
    /// The presets of the group of this index, by this facet, opened beside.
    Group(Facet, usize),
    /// The preset of this key, set.
    Preset(String),
}
