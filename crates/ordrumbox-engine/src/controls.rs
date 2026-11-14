//! What a MIDI controller's knobs do: each control change (a channel and a
//! controller number) is bound to one control (the master volume, a track's
//! volume, pan, filter cutoff or mute), and moves it on top of the settings
//! the tracks have, as a plugin host's mix does ([`HostMix`]). Nothing here
//! is saved with the library or an undo step: it is how the set is played.

use ordrumbox_core::json::{Object, Value};

use crate::engine::{HOST_TRACKS, HostMix};

/// Octaves the filter cutoff moves by when a knob goes from its middle to an end.
const CUTOFF_OCTAVES: f32 = 4.0;
/// The value of a knob at its middle: no pan, no cutoff change; from it on, a mute.
const MIDDLE: u8 = 64;

/// A MIDI control change's origin: its channel (0–15) and controller number (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Knob {
    pub channel: u8,
    pub controller: u8,
}

/// What a knob moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Control {
    /// The master volume, from silence (knob down) to the one set (knob up).
    Master,
    /// A track's volume, from silence to the one set.
    Volume(usize),
    /// A track's pan, moved from the one set (the middle of the knob: none).
    Pan(usize),
    /// A track's filter cutoff, moved up or down from the one set (the middle: none).
    Cutoff(usize),
    /// A track muted from the middle of the knob up.
    Mute(usize),
}

impl Control {
    /// Every control of a pattern of `track_count` tracks (those past the
    /// first [`HOST_TRACKS`] have none), the master first.
    #[must_use]
    pub fn all(track_count: usize) -> Vec<Self> {
        let tracks = (0..track_count.min(HOST_TRACKS))
            .flat_map(|t| [Self::Volume(t), Self::Pan(t), Self::Cutoff(t), Self::Mute(t)]);
        std::iter::once(Self::Master).chain(tracks).collect()
    }

    /// What it moves, for a list.
    #[must_use]
    pub const fn what(self) -> &'static str {
        match self {
            Self::Master => "master volume",
            Self::Volume(_) => "volume",
            Self::Pan(_) => "pan",
            Self::Cutoff(_) => "cutoff",
            Self::Mute(_) => "mute",
        }
    }

    /// The track it moves, if it is a track's.
    #[must_use]
    pub const fn track(self) -> Option<usize> {
        match self {
            Self::Master => None,
            Self::Volume(t) | Self::Pan(t) | Self::Cutoff(t) | Self::Mute(t) => Some(t),
        }
    }

    fn from_parts(what: &str, track: Option<usize>) -> Option<Self> {
        let track = track.filter(|&t| t < HOST_TRACKS);
        match (what, track) {
            ("master", _) => Some(Self::Master),
            ("volume", Some(t)) => Some(Self::Volume(t)),
            ("pan", Some(t)) => Some(Self::Pan(t)),
            ("cutoff", Some(t)) => Some(Self::Cutoff(t)),
            ("mute", Some(t)) => Some(Self::Mute(t)),
            _ => None,
        }
    }

    /// The key of the control in the settings file.
    const fn key(self) -> &'static str {
        match self {
            Self::Master => "master",
            other => other.what(),
        }
    }
}

/// The knobs bound, and the mix they make.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ControlMap {
    bindings: Vec<(Knob, Control)>,
    mix: HostMix,
}

impl ControlMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The knob bound to `control`.
    #[must_use]
    pub fn knob_of(&self, control: Control) -> Option<Knob> {
        self.bindings.iter().find(|(_, c)| *c == control).map(|(k, _)| *k)
    }

    /// Bind `knob` to `control`: a knob moves one control, a control has one
    /// knob, so what either had is let go (and put back where it was).
    pub fn bind(&mut self, knob: Knob, control: Control) {
        let freed: Vec<Control> =
            self.bindings.iter().filter(|(k, c)| *k == knob || *c == control).map(|(_, c)| *c).collect();
        self.bindings.retain(|(k, c)| *k != knob && *c != control);
        for control in freed {
            self.reset(control);
        }
        self.bindings.push((knob, control));
    }

    /// Let `control` go: no knob moves it, it is back where its track has it.
    pub fn unbind(&mut self, control: Control) {
        self.bindings.retain(|(_, c)| *c != control);
        self.reset(control);
    }

    /// The knob `knob` turned to `value` (0–127): the control it is bound to,
    /// moved. `None` when it is bound to none.
    pub fn turn(&mut self, knob: Knob, value: u8) -> Option<Control> {
        let (_, control) = *self.bindings.iter().find(|(k, _)| *k == knob)?;
        self.set(control, value.min(127));
        Some(control)
    }

    /// The mix the knobs make, for the engine.
    #[must_use]
    pub fn mix(&self) -> &HostMix {
        &self.mix
    }

    fn set(&mut self, control: Control, value: u8) {
        let unit = f32::from(value) / 127.0;
        let centered = (f32::from(value) - f32::from(MIDDLE)) / f32::from(MIDDLE - 1);
        let mix = &mut self.mix;
        match control {
            Control::Master => mix.gain = unit,
            Control::Volume(t) => mix.tracks[t].gain = unit,
            Control::Pan(t) => mix.tracks[t].pan = centered.clamp(-1.0, 1.0),
            Control::Cutoff(t) => mix.tracks[t].cutoff_octaves = centered.clamp(-1.0, 1.0) * CUTOFF_OCTAVES,
            Control::Mute(t) => {
                let bit = 1u64 << t;
                mix.muted = if value >= MIDDLE { mix.muted | bit } else { mix.muted & !bit };
            }
        }
    }

    /// `control` where its track has it: no change.
    fn reset(&mut self, control: Control) {
        let none = ControlMap::default().mix;
        match control {
            Control::Master => self.mix.gain = none.gain,
            Control::Volume(t) => self.mix.tracks[t].gain = none.tracks[t].gain,
            Control::Pan(t) => self.mix.tracks[t].pan = none.tracks[t].pan,
            Control::Cutoff(t) => self.mix.tracks[t].cutoff_octaves = none.tracks[t].cutoff_octaves,
            Control::Mute(t) => self.mix.muted &= !(1u64 << t),
        }
    }

    /// The bindings as the settings file keeps them.
    #[must_use]
    pub fn to_json(&self) -> Value {
        Value::Array(
            self.bindings
                .iter()
                .map(|(knob, control)| {
                    let mut object = Object::new();
                    object.insert("channel", Value::Number(f64::from(knob.channel)));
                    object.insert("controller", Value::Number(f64::from(knob.controller)));
                    object.insert("control", Value::from(control.key()));
                    if let Some(track) = control.track() {
                        object.insert("track", Value::Number(track as f64));
                    }
                    Value::Object(object)
                })
                .collect(),
        )
    }

    /// The bindings of a settings file's `value`, and what was wrong with the
    /// ones left out.
    #[must_use]
    pub fn from_json(value: &Value) -> (Self, Vec<String>) {
        let mut map = Self::new();
        let mut issues = Vec::new();
        let Some(entries) = value.as_array() else {
            return (map, vec![format!("{}: a list of bindings expected", value.to_json())]);
        };
        for entry in entries {
            match Self::binding_of(entry) {
                Some((knob, control)) => map.bind(knob, control),
                None => issues.push(format!("{}: not a binding, left out", entry.to_json())),
            }
        }
        (map, issues)
    }

    fn binding_of(entry: &Value) -> Option<(Knob, Control)> {
        let number = |key: &str, max: f64| {
            let n = entry.get(key)?.as_f64().filter(|n| n.fract() == 0.0 && (0.0..=max).contains(n))?;
            Some(n as u8)
        };
        let knob = Knob { channel: number("channel", 15.0)?, controller: number("controller", 127.0)? };
        let track = entry.get("track").and_then(Value::as_f64).filter(|t| t.fract() == 0.0 && *t >= 0.0);
        let control = Control::from_parts(entry.get("control")?.as_str()?, track.map(|t| t as usize))?;
        Some((knob, control))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOB: Knob = Knob { channel: 0, controller: 21 };

    #[test]
    fn a_knob_moves_the_control_it_is_bound_to() {
        let mut map = ControlMap::new();
        assert_eq!(map.turn(KNOB, 100), None, "bound to nothing");
        map.bind(KNOB, Control::Volume(2));
        assert_eq!(map.turn(KNOB, 127), Some(Control::Volume(2)));
        assert_eq!(map.mix().tracks[2].gain, 1.0);
        map.turn(KNOB, 0);
        assert_eq!(map.mix().tracks[2].gain, 0.0);
        assert_eq!(map.turn(Knob { channel: 1, ..KNOB }, 5), None, "another channel is another knob");
    }

    #[test]
    fn pan_and_cutoff_are_centred_and_a_mute_flips_at_the_middle() {
        let mut map = ControlMap::new();
        map.bind(Knob { controller: 1, ..KNOB }, Control::Pan(0));
        map.bind(Knob { controller: 2, ..KNOB }, Control::Cutoff(0));
        map.bind(Knob { controller: 3, ..KNOB }, Control::Mute(1));
        map.turn(Knob { controller: 1, ..KNOB }, 64);
        map.turn(Knob { controller: 2, ..KNOB }, 64);
        assert_eq!((map.mix().tracks[0].pan, map.mix().tracks[0].cutoff_octaves), (0.0, 0.0));
        map.turn(Knob { controller: 1, ..KNOB }, 127);
        map.turn(Knob { controller: 2, ..KNOB }, 0);
        assert_eq!(map.mix().tracks[0].pan, 1.0);
        assert_eq!(map.mix().tracks[0].cutoff_octaves, -CUTOFF_OCTAVES);
        map.turn(Knob { controller: 3, ..KNOB }, 63);
        assert_eq!(map.mix().muted, 0);
        map.turn(Knob { controller: 3, ..KNOB }, 64);
        assert_eq!(map.mix().muted, 0b10);
        map.turn(Knob { controller: 3, ..KNOB }, 0);
        assert_eq!(map.mix().muted, 0);
    }

    #[test]
    fn binding_again_lets_go_of_what_either_side_had() {
        let mut map = ControlMap::new();
        map.bind(KNOB, Control::Mute(0));
        map.turn(KNOB, 127);
        assert_eq!(map.mix().muted, 1);
        // The same knob for another control: the mute is let go, and unmuted.
        map.bind(KNOB, Control::Master);
        assert_eq!(map.mix().muted, 0);
        assert_eq!(map.knob_of(Control::Mute(0)), None);
        // The same control for another knob.
        let other = Knob { controller: 22, ..KNOB };
        map.bind(other, Control::Master);
        assert_eq!(map.knob_of(Control::Master), Some(other));
        assert_eq!(map.turn(KNOB, 1), None);
        map.unbind(Control::Master);
        assert_eq!(map.mix().gain, 1.0, "back where it was");
    }

    #[test]
    fn the_bindings_survive_the_settings_file_and_a_bad_one_is_reported() {
        let mut map = ControlMap::new();
        map.bind(KNOB, Control::Cutoff(3));
        map.bind(Knob { channel: 9, controller: 7 }, Control::Master);
        let (back, issues) = ControlMap::from_json(&map.to_json());
        assert!(issues.is_empty(), "{issues:?}");
        assert_eq!(back.knob_of(Control::Cutoff(3)), Some(KNOB));
        assert_eq!(back.knob_of(Control::Master), Some(Knob { channel: 9, controller: 7 }));
        let bad = ordrumbox_core::json::parse(
            r#"[{"channel":16,"controller":1,"control":"master"},{"channel":0,"controller":1,"control":"volume"}]"#,
        )
        .unwrap();
        let (none, issues) = ControlMap::from_json(&bad);
        assert_eq!(issues.len(), 2);
        assert_eq!(none.knob_of(Control::Master), None);
    }

    #[test]
    fn a_pattern_has_the_master_and_four_controls_a_track_up_to_the_host_s_sixteen() {
        assert_eq!(Control::all(0), [Control::Master]);
        assert_eq!(Control::all(2).len(), 1 + 8);
        assert_eq!(Control::all(40).len(), 1 + 4 * HOST_TRACKS);
    }
}
