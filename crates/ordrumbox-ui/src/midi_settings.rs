//! The MIDI settings of `settings.json`, chosen in Edit > Preferences: the
//! input and the output port, the clock sent and the clock followed. The
//! program reads them each time it opens its ports.

use crate::store;

/// The `settings.json` key of the MIDI input chosen: absent or empty, the
/// first port there is; `"off"`, none; else the name of a port.
pub const MIDI_KEY: &str = "midiInput";
/// The key of the MIDI output chosen: absent or `"off"`, none; else the name of a port.
pub const MIDI_OUT_KEY: &str = "midiOutput";
/// The key of the clock sent through the MIDI output (`true`; absent: not sent).
pub const CLOCK_OUT_KEY: &str = "midiClockOut";
/// The key of the clock followed from the MIDI input (`true`; absent: not followed).
pub const CLOCK_IN_KEY: &str = "midiClockIn";

/// The value of [`MIDI_KEY`] that turns the MIDI input off.
pub const MIDI_OFF: &str = "off";

/// Which MIDI port the program opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MidiPort {
    /// The first port there is, which a controller plugged in later takes over.
    First,
    Off,
    Named(String),
}

impl MidiPort {
    /// The setting's text.
    pub fn setting(&self) -> &str {
        match self {
            Self::First => "",
            Self::Off => MIDI_OFF,
            Self::Named(name) => name,
        }
    }

    fn from_setting(text: &str) -> Self {
        match text {
            "" => Self::First,
            MIDI_OFF => Self::Off,
            name => Self::Named(name.to_owned()),
        }
    }
}

/// The MIDI input chosen in Edit > Preferences.
pub fn chosen_midi_input() -> Result<MidiPort, String> {
    chosen_port(MIDI_KEY, MidiPort::First)
}

/// The MIDI output chosen in Edit > Preferences: none, until one is.
pub fn chosen_midi_output() -> Result<MidiPort, String> {
    chosen_port(MIDI_OUT_KEY, MidiPort::Off)
}

/// Whether the MIDI clock is sent through the output chosen.
pub fn chosen_clock_out() -> Result<bool, String> {
    chosen_flag(CLOCK_OUT_KEY)
}

/// Whether the transport follows the MIDI clock of the input chosen.
pub fn chosen_clock_in() -> Result<bool, String> {
    chosen_flag(CLOCK_IN_KEY)
}

fn chosen_port(key: &str, absent: MidiPort) -> Result<MidiPort, String> {
    let Some(path) = store::settings_path() else { return Ok(absent) };
    let value = store::load_setting(&path, key)?;
    Ok(value.as_ref().and_then(|v| v.as_str()).map_or(absent, MidiPort::from_setting))
}

fn chosen_flag(key: &str) -> Result<bool, String> {
    let Some(path) = store::settings_path() else { return Ok(false) };
    match store::load_setting(&path, key)? {
        None => Ok(false),
        Some(value) => value.as_bool().ok_or_else(|| format!("settings.json: {key}: true or false expected")),
    }
}
