//! The parameters a host shows and automates: the pattern and the kit
//! played, the master volume, a mute per track. The pattern and the kit are
//! applied by the player thread; the volume and the mutes by the audio
//! thread, every block.

use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;

use crate::state::LibraryState;

/// Patterns a host can choose (the parameter's range, from 1).
pub const PATTERN_CHOICES: i32 = 128;
/// Kits a host can choose (the parameter's range, from 1).
pub const KIT_CHOICES: i32 = 64;
/// Tracks with a mute parameter, from the first.
pub const MUTE_TRACKS: usize = 16;
/// The master volume's range, linear (+6 dB at the top).
const VOLUME_MAX: f32 = 2.0;
/// The lowest volume shown in dB on the knob's scale; below it, silence.
const VOLUME_FLOOR_DB: f32 = -60.0;

/// What the project saves and the host automates. The IDs are what a host
/// saves automation under: never rename them.
#[derive(Debug, Params)]
pub struct OrdrumboxParams {
    #[persist = "library"]
    pub library: Arc<LibraryState>,
    /// The pattern played, from 1.
    #[id = "pattern"]
    pub pattern: IntParam,
    /// The kit used, from 1.
    #[id = "kit"]
    pub kit: IntParam,
    /// A gain on the master volume.
    #[id = "volume"]
    pub volume: FloatParam,
    /// Track `t + 1` muted; ids `mute_1` to `mute_16`.
    #[nested(array, group = "Mute")]
    pub mutes: [MuteParam; MUTE_TRACKS],
    /// The names the pattern and kit parameters show.
    pub names: Arc<Names>,
}

/// One track's mute.
#[derive(Debug, Params)]
pub struct MuteParam {
    #[id = "mute"]
    pub mute: BoolParam,
}

impl Default for OrdrumboxParams {
    fn default() -> Self {
        let names = Arc::new(Names::default());
        let (pattern_names, kit_names) = (names.clone(), names.clone());
        Self {
            library: Arc::default(),
            pattern: IntParam::new("Pattern", 1, IntRange::Linear { min: 1, max: PATTERN_CHOICES })
                .with_value_to_string(Arc::new(move |v| pattern_names.show(&pattern_names.patterns, v)))
                .with_string_to_value(Arc::new(|s| choice_number(s, PATTERN_CHOICES))),
            kit: IntParam::new("Kit", 1, IntRange::Linear { min: 1, max: KIT_CHOICES })
                .with_value_to_string(Arc::new(move |v| kit_names.show(&kit_names.kits, v)))
                .with_string_to_value(Arc::new(|s| choice_number(s, KIT_CHOICES))),
            volume: FloatParam::new(
                "Volume",
                1.0,
                FloatRange::Skewed {
                    min: 0.0,
                    max: VOLUME_MAX,
                    factor: FloatRange::gain_skew_factor(VOLUME_FLOOR_DB, util::gain_to_db(VOLUME_MAX)),
                },
            )
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),
            mutes: std::array::from_fn(|t| MuteParam { mute: BoolParam::new(format!("Mute track {}", t + 1), false) }),
            names,
        }
    }
}

impl OrdrumboxParams {
    /// The pattern and the kit the host asks for, from 0.
    pub fn choice(&self) -> HostChoice {
        let index = |param: &IntParam| usize::try_from(param.value() - 1).unwrap_or(0);
        HostChoice { pattern: index(&self.pattern), kit: index(&self.kit) }
    }

    /// The tracks muted, bit `t` for track `t`.
    pub fn muted_tracks(&self) -> u64 {
        self.mutes.iter().enumerate().fold(0, |mask, (t, m)| mask | u64::from(m.mute.value()) << t)
    }
}

/// The pattern and the kit a host's parameters ask for, from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostChoice {
    pub pattern: usize,
    pub kit: usize,
}

/// The names of the patterns and the kits, kept up to date by the player
/// thread, for the host to show next to the parameters' numbers.
#[derive(Debug, Default)]
pub struct Names {
    patterns: RwLock<Vec<String>>,
    kits: RwLock<Vec<String>>,
}

impl Names {
    /// The pattern names, if they changed (unnamed patterns: empty).
    pub fn set_patterns<'a>(&self, names: impl Iterator<Item = Option<&'a str>> + Clone) {
        set_if_changed(&self.patterns, names.map(Option::unwrap_or_default));
    }

    /// The kit names, if they changed.
    pub fn set_kits<'a>(&self, names: impl Iterator<Item = &'a str> + Clone) {
        set_if_changed(&self.kits, names);
    }

    /// Choice `value` (from 1) as the host shows it: its number, and its
    /// name when there is one.
    fn show(&self, list: &RwLock<Vec<String>>, value: i32) -> String {
        let list = list.read().unwrap_or_else(|poisoned| poisoned.into_inner());
        let name = usize::try_from(value - 1).ok().and_then(|i| list.get(i)).filter(|n| !n.is_empty());
        match name {
            Some(name) => format!("{value} {name}"),
            None => value.to_string(),
        }
    }
}

/// `list` replaced by `names` when they differ (no allocation otherwise).
fn set_if_changed<'a>(list: &RwLock<Vec<String>>, names: impl Iterator<Item = &'a str> + Clone) {
    let same =
        list.read().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().map(String::as_str).eq(names.clone());
    if !same {
        *list.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = names.map(str::to_owned).collect();
    }
}

/// The choice a host's text entry gives: its leading number, within `1..=max`.
fn choice_number(text: &str, max: i32) -> Option<i32> {
    let digits: String = text.trim().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|n| (1..=max).contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choices_show_their_names_and_read_their_numbers() {
        let params = OrdrumboxParams::default();
        params.names.set_patterns([Some("funk"), None].into_iter());
        params.names.set_kits(["electro"].into_iter());
        assert_eq!(params.pattern.normalized_value_to_string(0.0, true), "1 funk");
        assert_eq!(params.names.show(&params.names.patterns, 2), "2");
        assert_eq!(params.names.show(&params.names.kits, 1), "1 electro");
        assert_eq!(choice_number(" 12 funk", PATTERN_CHOICES), Some(12));
        assert_eq!(choice_number("0", PATTERN_CHOICES), None);
        assert_eq!(choice_number("funk", PATTERN_CHOICES), None);
    }

    #[test]
    fn the_defaults_choose_the_first_pattern_and_kit_and_mute_nothing() {
        let params = OrdrumboxParams::default();
        assert_eq!(params.choice(), HostChoice { pattern: 0, kit: 0 });
        assert_eq!(params.muted_tracks(), 0);
        assert_eq!(params.volume.value(), 1.0);
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();
        for id in ["pattern", "kit", "volume", "mute_1", "mute_16"] {
            assert!(ids.iter().any(|i| i == id), "no parameter {id} in {ids:?}");
        }
    }
}
