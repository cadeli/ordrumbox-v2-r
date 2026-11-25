//! The tabs and view buttons an [`Id`](super::Id) names: which page of an
//! editor, or which view, a click shows.

/// A main tab of the track editor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum EditorTab {
    #[default]
    Fx,
    Sound,
    Lfo,
    Loop,
    Generate,
}

impl EditorTab {
    /// The tabs, left to right.
    pub const ALL: [Self; 5] = [Self::Fx, Self::Sound, Self::Lfo, Self::Loop, Self::Generate];

    /// The tab's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fx => "FX",
            Self::Sound => "Sound",
            Self::Lfo => "LFO",
            Self::Loop => "Loop",
            Self::Generate => "Generate",
        }
    }
}

/// A sub-tab of the Generate tab's Advanced settings. Out of auto mode,
/// Groove's slot is labelled Manual and Engine has none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GenerationTab {
    #[default]
    Groove,
    Engine,
    /// A drum track's compose settings (only shown for one).
    Compose,
}

impl GenerationTab {
    /// The sub-tabs, left to right.
    pub const ALL: [Self; 3] = [Self::Groove, Self::Engine, Self::Compose];
}

/// A tab of the synth editor, in the order of `synth_model::TABS`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum SynthTab {
    #[default]
    Oscillators,
    Filter,
    Modulation,
    Envelopes,
}

impl SynthTab {
    /// The tabs, left to right, as `synth_model::TABS` labels them.
    pub const ALL: [Self; 4] = [Self::Oscillators, Self::Filter, Self::Modulation, Self::Envelopes];
}

/// A view button of the header: a view of the main editor, or the kit
/// editor, which it toggles over the main editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewButton {
    Grid,
    PianoRoll,
    /// The song has no button of its own: it is the mode switch's (its slot
    /// has no width).
    Song,
    Synth,
    Mixer,
    Kit,
}

impl ViewButton {
    /// The buttons, left to right, as `view::HeaderControls::views` places them.
    pub const ALL: [Self; 6] = [Self::Grid, Self::PianoRoll, Self::Song, Self::Synth, Self::Mixer, Self::Kit];

    /// The button's label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Grid => "GRID",
            Self::PianoRoll => "ROLL",
            Self::Song => "SONG",
            Self::Synth => "SYNTH",
            Self::Mixer => "MIX",
            Self::Kit => "KIT",
        }
    }
}
