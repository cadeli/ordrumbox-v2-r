//! What each control a lane sets is: its keys, its name, and how a lane's
//! value (0 to 1) reads in the control's own unit.

use super::{AutomationKind, AutomationTarget, is_on, switch_value, volume_db, volume_gain, volume_level};
use crate::Track;

impl AutomationTarget {
    pub const ALL: [Self; 18] = [
        Self::Mute,
        Self::Solo,
        Self::Volume,
        Self::Pan,
        Self::Cutoff,
        Self::Q,
        Self::EchoSend,
        Self::ReverbSend,
        Self::ReverbMix,
        Self::DelayMix,
        Self::DelayFeedback,
        Self::Saturation,
        Self::Bitcrush,
        Self::HardClip,
        Self::CompMix,
        Self::Transpose,
        Self::Probability,
        Self::Swing,
    ];

    /// Its key in a file.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Mute => "mute",
            Self::Solo => "solo",
            Self::Volume => "volume",
            Self::Pan => "pan",
            Self::Cutoff => "cutoff",
            Self::Q => "q",
            Self::EchoSend => "echoSend",
            Self::ReverbSend => "reverbSend",
            Self::ReverbMix => "reverbMix",
            Self::DelayMix => "delayMix",
            Self::DelayFeedback => "delayFeedback",
            Self::Saturation => "saturation",
            Self::Bitcrush => "bitcrush",
            Self::HardClip => "hardClip",
            Self::CompMix => "compMix",
            Self::Transpose => "transpose",
            Self::Probability => "probability",
            Self::Swing => "swing",
        }
    }

    /// Its name as shown.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Mute => "Mute",
            Self::Solo => "Solo",
            Self::Volume => "Volume",
            Self::Pan => "Pan",
            Self::Cutoff => "Cutoff",
            Self::Q => "Q",
            Self::EchoSend => "Echo send",
            Self::ReverbSend => "Reverb send",
            Self::ReverbMix => "Reverb mix",
            Self::DelayMix => "Delay mix",
            Self::DelayFeedback => "Delay feedback",
            Self::Saturation => "Saturation",
            Self::Bitcrush => "Bitcrush",
            Self::HardClip => "Hard clip",
            Self::CompMix => "Comp mix",
            Self::Transpose => "Transpose",
            Self::Probability => "Probability",
            Self::Swing => "Swing",
        }
    }

    /// The key of the track's field it sets in place of the pattern's.
    #[must_use]
    pub const fn track_key(self) -> &'static str {
        match self {
            Self::Cutoff => "filterCutoff",
            Self::Q => "filterQ",
            Self::Saturation => "saturationMix",
            Self::Swing => "swingAmount",
            _ => self.key(),
        }
    }

    /// The target that sets the track's field `key`, if one does.
    #[must_use]
    pub fn from_track_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.track_key() == key)
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|target| target.key() == key)
    }

    #[must_use]
    pub const fn kind(self) -> AutomationKind {
        match self {
            Self::Mute | Self::Solo => AutomationKind::Switch,
            _ => AutomationKind::Level,
        }
    }

    /// The bounds of the track's field, which a level's 0 and 1 set.
    fn range(self) -> (f64, f64) {
        Track::value_range(self.track_key()).unwrap_or((0.0, 1.0))
    }

    /// `value` as shown: `on` or `off` for a switch, decibels for a volume,
    /// a side for a pan, hertz for a cutoff, a share for a send or an amount.
    #[must_use]
    pub fn value_label(self, value: f64) -> String {
        let control = self.to_control(value);
        match self {
            Self::Mute | Self::Solo => (if is_on(value) { "on" } else { "off" }).to_owned(),
            Self::Volume => volume_db(value).map_or_else(|| "silent".to_owned(), |db| format!("{db:.1} dB")),
            Self::Pan if control.abs() < 0.005 => "center".to_owned(),
            Self::Pan if control < 0.0 => format!("left {:.0}%", -control * 100.0),
            Self::Pan => format!("right {:.0}%", control * 100.0),
            Self::Cutoff if control >= 1000.0 => format!("{:.2} kHz", control / 1000.0),
            Self::Cutoff => format!("{control:.0} Hz"),
            Self::Q => format!("{control:.2}"),
            Self::Transpose => format!("{control:+} st"),
            Self::Swing => format!("{control:.2} beat"),
            _ => format!("{:.0}%", control * 100.0),
        }
    }

    /// The control's own value a lane's `value` (0 to 1) sets: a switch's 0
    /// or 1, a volume's gain ([`volume_gain`]), a cutoff in hertz
    /// (logarithmic), a transpose in whole semitones, else the track's field
    /// over its range (a pan from -1, left, to 1, right).
    #[must_use]
    pub fn to_control(self, value: f64) -> f64 {
        let value = value.clamp(0.0, 1.0);
        let (min, max) = self.range();
        match self {
            Self::Mute | Self::Solo => switch_value(is_on(value)),
            Self::Volume => volume_gain(value),
            Self::Cutoff => min * (max / min).powf(value),
            Self::Transpose => (min + value * (max - min)).round(),
            _ => min + value * (max - min),
        }
    }

    /// The lane's value (0 to 1) that sets the control to `control`: the
    /// inverse of [`Self::to_control`], held within the lane's range.
    #[must_use]
    pub fn from_control(self, control: f64) -> f64 {
        let (min, max) = self.range();
        let value = match self {
            Self::Mute | Self::Solo => switch_value(is_on(control)),
            Self::Volume => volume_level(control),
            Self::Cutoff => (control.clamp(min, max) / min).ln() / (max / min).ln(),
            _ => (control - min) / (max - min),
        };
        value.clamp(0.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Its place in [`AutomationTarget::ALL`]: a match without a wildcard,
    /// so a new target fails to build here until it is listed.
    fn position(target: AutomationTarget) -> usize {
        use AutomationTarget::*;
        match target {
            Mute => 0,
            Solo => 1,
            Volume => 2,
            Pan => 3,
            Cutoff => 4,
            Q => 5,
            EchoSend => 6,
            ReverbSend => 7,
            ReverbMix => 8,
            DelayMix => 9,
            DelayFeedback => 10,
            Saturation => 11,
            Bitcrush => 12,
            HardClip => 13,
            CompMix => 14,
            Transpose => 15,
            Probability => 16,
            Swing => 17,
        }
    }

    #[test]
    fn every_target_is_listed_once_and_read_back_from_its_key() {
        for (i, target) in AutomationTarget::ALL.into_iter().enumerate() {
            assert_eq!(position(target), i, "{target:?}");
            assert_eq!(AutomationTarget::from_key(target.key()), Some(target));
        }
    }

    #[test]
    fn a_level_lane_sets_its_control_in_its_own_unit() {
        use AutomationTarget::{Cutoff, DelayFeedback, EchoSend, Pan, Q, Saturation};
        assert_eq!([0.0, 0.5, 1.0].map(|v| Pan.to_control(v)), [-1.0, 0.0, 1.0]);
        assert_eq!(
            [0.0, 0.25, 0.5, 1.0].map(|v| Pan.value_label(v)),
            ["left 100%", "left 50%", "center", "right 100%"]
        );
        let hz = [0.0, 0.5, 1.0].map(|v| Cutoff.to_control(v));
        assert!((hz[0] - 20.0).abs() < 1e-9 && (hz[1] - 632.456).abs() < 1e-3 && (hz[2] - 20_000.0).abs() < 1e-6);
        assert_eq!([Cutoff.value_label(0.5), Cutoff.value_label(1.0)], ["632 Hz", "20.00 kHz"]);
        assert_eq!([Q.to_control(0.0), Q.to_control(1.0)], [0.707, 18.707]);
        assert_eq!(Q.value_label(0.5), "9.71");
        for target in AutomationTarget::ALL {
            for value in [0.0, 0.25, 0.5, 1.0] {
                let back = target.from_control(target.to_control(value));
                let expected = if target.kind() == AutomationKind::Switch { switch_value(is_on(value)) } else { value };
                assert!((back - expected).abs() < 1e-9, "{target:?} {value}");
            }
        }
        assert_eq!([Cutoff.from_control(5.0), Q.from_control(40.0), Pan.from_control(-3.0)], [0.0, 1.0, 0.0]);
        assert_eq!(AutomationTarget::from_key("q"), Some(Q));
        assert_eq!(AutomationTarget::from_track_key("filterCutoff"), Some(Cutoff));
        assert_eq!(AutomationTarget::from_track_key("beatCount"), None);
        assert_eq!([DelayFeedback.to_control(1.0), EchoSend.to_control(0.25)], [0.95, 0.25]);
        assert_eq!([EchoSend.value_label(0.25), DelayFeedback.value_label(1.0)], ["25%", "95%"]);
        assert_eq!(AutomationTarget::from_track_key("saturationMix"), Some(Saturation));
        assert_eq!(AutomationTarget::from_key("saturation"), Some(Saturation));
        let transpose = AutomationTarget::Transpose;
        assert_eq!([0.0, 0.51, 0.5, 1.0].map(|v| transpose.to_control(v)), [-24.0, 0.0, 0.0, 24.0], "whole semitones");
        assert_eq!([transpose.value_label(0.75), transpose.value_label(0.0)], ["+12 st", "-24 st"]);
        assert_eq!(AutomationTarget::Swing.value_label(0.33), "0.33 beat");
        assert_eq!(AutomationTarget::Probability.value_label(0.5), "50%");
        // A probability has no range of its own to clamp to: 0 to 1.
        let without_range = [AutomationTarget::Mute, AutomationTarget::Solo, AutomationTarget::Probability];
        for target in AutomationTarget::ALL {
            assert!(Track::value_range(target.track_key()).is_some() || without_range.contains(&target), "{target:?}");
        }
    }
}
