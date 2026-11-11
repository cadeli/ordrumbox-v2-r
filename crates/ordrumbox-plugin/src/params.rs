//! The parameters a host shows and automates: the pattern and the kit
//! played, the master volume, the swing, and per track a mute, a volume, a
//! pan, the filter's cutoff and Q, the reverb, delay and saturation amounts,
//! the delay feedback, the compressor's mix and threshold, a transpose and the output it plays on (the mix, or
//! one of its own). The pattern, the kit and the swing
//! are applied by the player thread, which also shows the mutes in the
//! window; the track parameters are applied by the audio thread, every
//! block, on top of the pattern's own (the library is not edited).
//! A pattern or kit chosen in the window is given back to the host
//! (`editor.rs`).

use std::sync::{Arc, RwLock};

use nih_plug::prelude::*;
use ordrumbox_core::track::STRAIGHT_SWING;
use ordrumbox_engine::{HOST_TRACKS, HostMix, HostTrack};

use crate::editor;
use crate::state::{EditorSize, LibraryState};

/// Patterns a host can choose (the parameter's range, from 1).
pub const PATTERN_CHOICES: i32 = 128;
/// Kits a host can choose (the parameter's range, from 1).
pub const KIT_CHOICES: i32 = 64;
/// The range of the volumes (the master's and the tracks'), linear (+6 dB at the top).
const VOLUME_MAX: f32 = 2.0;
/// The most swing: the second step of each pair on its last quarter.
const SWING_MAX: f32 = 0.75;
/// The lowest volume shown in dB on the knob's scale; below it, silence.
const VOLUME_FLOOR_DB: f32 = -60.0;
/// The most a host moves a track's filter cutoff, octaves each way.
const CUTOFF_OCTAVES_MAX: f32 = 4.0;
/// The most a host adds to or takes from a track's filter Q (its range is
/// 0.707 to 18.707).
const Q_OFFSET_MAX: f32 = 18.0;
/// The most a host moves a track's compressor threshold, dB each way (its range is
/// -60 to 0 dBFS).
const THRESHOLD_OFFSET_MAX_DB: f32 = 60.0;
/// The most a host transposes a track, semitones each way (the editor's).
const TRANSPOSE_MAX: i32 = 24;

/// What the output parameter of a track shows: the mix, or its own output.
const OUTPUT_MAIN: &str = "Main";
const OUTPUT_OWN: &str = "Own";

/// What the project saves and the host automates. The IDs are what a host
/// saves automation under: never rename them.
#[derive(Debug, Params)]
pub struct OrdrumboxParams {
    #[persist = "library"]
    pub library: Arc<LibraryState>,
    /// The window's size, in logical pixels.
    #[persist = "editor-size"]
    pub editor_size: Arc<EditorSize>,
    /// The pattern played, from 1.
    #[id = "pattern"]
    pub pattern: IntParam,
    /// The kit used, from 1.
    #[id = "kit"]
    pub kit: IntParam,
    /// A gain on the master volume.
    #[id = "volume"]
    pub volume: FloatParam,
    /// A swing on every track, on top of their own: where the second step
    /// of each pair starts, 50 % (straight) to 75 %.
    #[id = "swing"]
    pub swing: FloatParam,
    /// Track `t + 1`'s parameters; ids `mute_1`, `track_volume_1`,
    /// `track_pan_1`, `track_cutoff_1`… to `_16`.
    #[nested(array, group = "Track")]
    pub tracks: [TrackParams; HOST_TRACKS],
    /// The names the pattern and kit parameters show.
    pub names: Arc<Names>,
}

/// One track's parameters, each changing nothing at its default.
#[derive(Debug, Params)]
pub struct TrackParams {
    #[id = "mute"]
    pub mute: BoolParam,
    /// A gain on the track's volume.
    #[id = "track_volume"]
    pub volume: FloatParam,
    /// Added to the track's pan: -1 (left) to 1 (right).
    #[id = "track_pan"]
    pub pan: FloatParam,
    /// Octaves the filter cutoff moves by.
    #[id = "track_cutoff"]
    pub cutoff: FloatParam,
    /// Added to the filter's Q.
    #[id = "track_q"]
    pub q: FloatParam,
    /// Added to the reverb amount.
    #[id = "track_reverb"]
    pub reverb: FloatParam,
    /// Added to the delay amount.
    #[id = "track_delay"]
    pub delay: FloatParam,
    /// Added to the delay feedback.
    #[id = "track_feedback"]
    pub feedback: FloatParam,
    /// Added to the saturation amount.
    #[id = "track_saturation"]
    pub saturation: FloatParam,
    /// Added to the compressor's mix.
    #[id = "track_compressor"]
    pub compressor: FloatParam,
    /// Decibels added to the compressor's threshold.
    #[id = "track_threshold"]
    pub threshold: FloatParam,
    /// Semitones added to the track's transpose.
    #[id = "track_transpose"]
    pub transpose: IntParam,
    /// On: the track leaves the mix for the output of its number.
    #[id = "track_output"]
    pub output: BoolParam,
}

impl TrackParams {
    /// The parameters of track `number` (from 1).
    fn new(number: usize) -> Self {
        Self {
            mute: BoolParam::new(format!("Mute track {number}"), false),
            volume: volume_param(format!("Volume track {number}")),
            pan: FloatParam::new(format!("Pan track {number}"), 0.0, FloatRange::Linear { min: -1.0, max: 1.0 })
                .with_value_to_string(formatters::v2s_f32_panning())
                .with_string_to_value(formatters::s2v_f32_panning()),
            cutoff: offset_param(format!("Cutoff track {number}"), CUTOFF_OCTAVES_MAX).with_unit(" oct"),
            q: offset_param(format!("Q track {number}"), Q_OFFSET_MAX),
            reverb: offset_param(format!("Reverb track {number}"), 1.0),
            delay: offset_param(format!("Delay track {number}"), 1.0),
            feedback: offset_param(format!("Feedback track {number}"), 1.0),
            saturation: offset_param(format!("Saturation track {number}"), 1.0),
            compressor: offset_param(format!("Compressor track {number}"), 1.0),
            threshold: FloatParam::new(
                format!("Threshold track {number}"),
                0.0,
                FloatRange::Linear { min: -THRESHOLD_OFFSET_MAX_DB, max: THRESHOLD_OFFSET_MAX_DB },
            )
            .with_step_size(0.1)
            .with_unit(" dB")
            .with_value_to_string(Arc::new(|v| format!("{v:+.1}"))),
            transpose: IntParam::new(
                format!("Transpose track {number}"),
                0,
                IntRange::Linear { min: -TRANSPOSE_MAX, max: TRANSPOSE_MAX },
            )
            .with_unit(" st"),
            output: BoolParam::new(format!("Output track {number}"), false)
                .with_value_to_string(Arc::new(|own| if own { OUTPUT_OWN } else { OUTPUT_MAIN }.to_owned()))
                .with_string_to_value(Arc::new(|text| match text.trim() {
                    t if t.eq_ignore_ascii_case(OUTPUT_OWN) => Some(true),
                    t if t.eq_ignore_ascii_case(OUTPUT_MAIN) => Some(false),
                    _ => None,
                })),
        }
    }

    /// What the audio thread applies to the track.
    fn host_track(&self) -> HostTrack {
        HostTrack {
            gain: self.volume.value(),
            pan: self.pan.value(),
            cutoff_octaves: self.cutoff.value(),
            q: self.q.value(),
            reverb: self.reverb.value(),
            delay: self.delay.value(),
            feedback: self.feedback.value(),
            saturation: self.saturation.value(),
            compressor: self.compressor.value(),
            threshold_db: self.threshold.value(),
            transpose: self.transpose.value(),
        }
    }
}

/// An offset parameter named `name`, from `-max` to `max`, at 0, shown with
/// its sign.
fn offset_param(name: impl Into<String>, max: f32) -> FloatParam {
    FloatParam::new(name, 0.0, FloatRange::Linear { min: -max, max })
        .with_step_size(0.01)
        .with_value_to_string(Arc::new(|v| format!("{v:+.2}")))
}

/// A volume parameter named `name`, a linear gain shown in dB, at 0 dB.
fn volume_param(name: impl Into<String>) -> FloatParam {
    FloatParam::new(
        name,
        1.0,
        FloatRange::Skewed {
            min: 0.0,
            max: VOLUME_MAX,
            factor: FloatRange::gain_skew_factor(VOLUME_FLOOR_DB, util::gain_to_db(VOLUME_MAX)),
        },
    )
    .with_unit(" dB")
    .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
    .with_string_to_value(formatters::s2v_f32_gain_to_db())
}

impl Default for OrdrumboxParams {
    fn default() -> Self {
        let names = Arc::new(Names::default());
        let (pattern_names, kit_names) = (names.clone(), names.clone());
        Self {
            library: Arc::default(),
            editor_size: Arc::new(EditorSize::new(editor::default_size())),
            pattern: IntParam::new("Pattern", 1, IntRange::Linear { min: 1, max: PATTERN_CHOICES })
                .with_value_to_string(Arc::new(move |v| pattern_names.show(&pattern_names.patterns, v)))
                .with_string_to_value(Arc::new(|s| choice_number(s, PATTERN_CHOICES))),
            kit: IntParam::new("Kit", 1, IntRange::Linear { min: 1, max: KIT_CHOICES })
                .with_value_to_string(Arc::new(move |v| kit_names.show(&kit_names.kits, v)))
                .with_string_to_value(Arc::new(|s| choice_number(s, KIT_CHOICES))),
            volume: volume_param("Volume"),
            swing: FloatParam::new(
                "Swing",
                straight_swing(),
                FloatRange::Linear { min: straight_swing(), max: SWING_MAX },
            )
            .with_unit(" %")
            .with_value_to_string(formatters::v2s_f32_percentage(1))
            .with_string_to_value(formatters::s2v_f32_percentage()),
            tracks: std::array::from_fn(|t| TrackParams::new(t + 1)),
            names,
        }
    }
}

impl OrdrumboxParams {
    /// What the player thread follows: the pattern and the kit the host asks
    /// for, the tracks it mutes and its swing.
    pub fn controls(&self) -> HostControls {
        HostControls {
            pattern: choice_index(&self.pattern),
            kit: choice_index(&self.kit),
            muted: self.muted_tracks(),
            swing: f64::from(self.swing.value()),
        }
    }

    /// What the audio thread applies: the master volume, the mutes, the
    /// tracks on an output of their own and the track parameters.
    pub fn mix(&self) -> HostMix {
        HostMix {
            gain: self.volume.value(),
            muted: self.muted_tracks(),
            split: self.track_mask(|track| track.output.value()),
            tracks: self.tracks.each_ref().map(TrackParams::host_track),
        }
    }

    /// The tracks muted, bit `t` for track `t`.
    pub fn muted_tracks(&self) -> u64 {
        self.track_mask(|track| track.mute.value())
    }

    /// The tracks `on` holds for, bit `t` for track `t`.
    fn track_mask(&self, on: impl Fn(&TrackParams) -> bool) -> u64 {
        self.tracks.iter().enumerate().fold(0, |mask, (t, track)| mask | u64::from(on(track)) << t)
    }
}

/// The swing parameter's straight value.
fn straight_swing() -> f32 {
    // 0.5: exact in `f32`.
    STRAIGHT_SWING as f32
}

/// What a host's parameters ask of the player thread.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostControls {
    /// The pattern, from 0.
    pub pattern: usize,
    /// The kit, from 0.
    pub kit: usize,
    /// The tracks muted, bit `t` for track `t`.
    pub muted: u64,
    /// Where the second step of each pair starts ([`STRAIGHT_SWING`]: straight).
    pub swing: f64,
}

/// Choice parameter `param` (from 1) as an index from 0.
fn choice_index(param: &IntParam) -> usize {
    // The parameter's range starts at 1: `value() - 1` is never negative.
    param.value().max(1).unsigned_abs() as usize - 1
}

/// Index `index` (from 0) as the value of a choice parameter of `max`
/// choices; `None` past them.
pub fn choice_value(index: usize, max: i32) -> Option<i32> {
    i32::try_from(index + 1).ok().filter(|&v| v <= max)
}

/// The names of the patterns and the kits, kept up to date by the player
/// thread, for the host to show next to the parameters' numbers. nih-plug
/// cannot tell the host that these texts changed: a host that keeps the
/// texts it read shows the old names until it asks again.
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
        assert_eq!(choice_value(0, PATTERN_CHOICES), Some(1));
        assert_eq!(choice_value(127, PATTERN_CHOICES), Some(128));
        assert_eq!(choice_value(128, PATTERN_CHOICES), None);
    }

    #[test]
    fn a_track_on_its_own_output_is_split_from_the_mix() {
        let params = OrdrumboxParams::default();
        let output = &params.tracks[2].output;
        assert_eq!(output.normalized_value_to_string(0.0, true), "Main");
        assert_eq!(output.string_to_normalized_value(" own"), Some(1.0));
        assert_eq!(output.string_to_normalized_value("3"), None);
    }

    #[test]
    fn the_defaults_choose_the_first_pattern_and_kit_and_change_nothing() {
        let params = OrdrumboxParams::default();
        let controls = HostControls { pattern: 0, kit: 0, muted: 0, swing: STRAIGHT_SWING };
        assert_eq!(params.controls(), controls);
        assert_eq!(params.muted_tracks(), 0);
        assert_eq!(params.mix(), HostMix::default());
        assert_eq!(params.swing.normalized_value_to_string(0.0, true), "50.0 %");
        assert_eq!(params.tracks[0].pan.normalized_value_to_string(0.5, true), "C");
        assert_eq!(params.tracks[0].cutoff.normalized_value_to_string(1.0, true), "+4.00 oct");
        assert_eq!(params.tracks[0].transpose.normalized_value_to_string(0.0, true), "-24 st");
        assert_eq!(params.tracks[0].threshold.normalized_value_to_string(0.25, true), "-30.0 dB");
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();
        let saved = ["pattern", "kit", "volume", "mute_1", "mute_16"];
        let added = ["swing", "track_volume_1", "track_pan_1", "track_volume_16", "track_pan_16"];
        let fx = ["track_cutoff_16", "track_q_1", "track_reverb_1", "track_delay_1", "track_feedback_1"];
        let more =
            ["track_saturation_1", "track_transpose_1", "track_transpose_16", "track_output_1", "track_output_16"];
        let comp = ["track_compressor_1", "track_compressor_16", "track_threshold_1", "track_threshold_16"];
        for id in saved.into_iter().chain(added).chain(fx).chain(more).chain(comp) {
            assert!(ids.iter().any(|i| i == id), "no parameter {id} in {ids:?}");
        }
    }
}
