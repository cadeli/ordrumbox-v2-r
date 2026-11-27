//! Synth presets (`assets/data/generated_sounds.json`) and the patch a note
//! plays with (`WorkletSynthVoice#sendUpdate()`).
//!
//! Presets are free-form JSON, read leniently: a number or a numeric string,
//! else the default; a flag on only when `true`; an unknown enum name 0.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_core::num::clamp;
use ordrumbox_core::{Issue, refusal};
use ordrumbox_dsp::SynthPatch;
use ordrumbox_dsp::lfo::Wave;
use ordrumbox_dsp::slope::FilterMode;
use ordrumbox_dsp::synth::patch::{
    Adsr, Filter2, Filter2Routing, FilterEnv, LfoTarget, MATRIX_SIZE, MatrixRoute, MatrixSource, NoiseMode, PitchEnv,
    SynthBypass, SynthLfo, VoiceMode,
};
use ordrumbox_dsp::synth::{MAX_PITCH_ENV_SEMITONES, MAX_UNISON};

pub mod legacy_modulation;
pub mod tags;

/// `C3_FREQ`, Hz: the pitch of ratio 1.
const C3_FREQ: f64 = 130.812_782_650_299_3;
/// `MIN_NOTE_RATIO`.
const MIN_NOTE_RATIO: f64 = 0.0001;
/// The synth's level against a kit sample's: −3 dB, the peak a kit sample is
/// normalised to, so a synth note at full velocity peaks where a sample does.
const SYNTH_TRIM: f64 = 0.707_945_784_384_137_9;

/// The oscillator and LFO waves, by code.
pub const WAVES: [&str; 5] = ordrumbox_dsp::lfo::WAVE_NAMES;
/// The filter types, by code (a strip's and a synth voice's filter mode).
pub const FILTER_TYPES: [&str; 4] = ["lowpass", "highpass", "bandpass", "notch"];
/// The noise's modes, by code: white, the console's long and short shift register.
pub const NOISE_MODES: [&str; 3] = ["white", "long", "short"];
/// The clock rate of the console noise, Hz: its range, and what a preset without one gets.
pub const NOISE_RATE_RANGE: (f64, f64) = (100.0, 32_000.0);
use ordrumbox_dsp::synth::patch::NOISE_RATE_DEFAULT;
/// What oscillator 2 restarts its cycle with (`vco2.sync`), by code: nothing, or oscillator 1.
pub const OSC2_SYNCS: [&str; 2] = ["off", "vco1"];
/// The pitch envelope's offset, semitones, and its decay, seconds: their ranges.
pub const PITCH_ENV_AMOUNT_RANGE: (f64, f64) = (-MAX_PITCH_ENV_SEMITONES, MAX_PITCH_ENV_SEMITONES);
pub const PITCH_ENV_DECAY_RANGE: (f64, f64) = (0.005, 1.0);
/// The longest attack, decay and release of an envelope, seconds.
pub const ATTACK_MAX: f64 = 2.0;
pub const DECAY_MAX: f64 = 4.0;
pub const RELEASE_MAX: f64 = 4.0;
/// The fastest LFO, Hz.
pub const LFO_FREQ_MAX: f64 = 100.0;
/// The longest an LFO takes to fade in, seconds.
pub const LFO_DELAY_MAX: f64 = 4.0;
/// How a track's notes share a voice (`voice.mode`), by code.
pub const VOICE_MODES: [&str; 3] = ["poly", "mono", "legato"];
/// The longest glide, seconds.
pub const GLIDE_MAX: f64 = 2.0;
/// How much louder a note at full velocity plays at full accent: half again.
const ACCENT_LEVEL: f64 = 0.5;
/// The second filter a preset turns on without saying more: a highpass at 100 Hz.
pub const FILTER2_TYPE_DEFAULT: &str = "highpass";
pub const FILTER2_FREQ_DEFAULT: f64 = 100.0;
pub const FILTER2_Q_DEFAULT: f64 = 0.7;
/// Where the second filter sits (`filter2.routing`), by code: after the first, or beside it.
pub const FILTER2_ROUTINGS: [&str; 2] = ["serial", "parallel"];
/// The balance of the two filters in parallel a preset that does not set it plays with: both at full level.
pub const FILTER2_BALANCE_DEFAULT: f64 = 0.5;
/// The stages of the filter envelope's own ADSR (`filterEnv.<stage>`).
pub const FILTER_ENV_STAGES: [&str; 4] = ["attack", "decay", "sustain", "release"];
/// The semitones an oscillator is tuned by, on top of its octave.
pub const SEMITONE_RANGE: (f64, f64) = (-12.0, 12.0);
/// The decay of a preset's pitch envelope when it gives none, seconds.
pub const PITCH_ENV_DECAY_DEFAULT: f64 = 0.05;
/// What a line of a preset's `matrix` reads, by code.
pub const MATRIX_SOURCES: [&str; 8] = ["off", "lfo1", "lfo2", "env", "modEnv", "velocity", "key", "random"];
/// The LFO targets by their name in a preset (two names for the filter envelope amount;
/// older names: [`legacy_modulation`]).
const LFO_TARGETS: [(&str, LfoTarget); 43] = [
    ("NOT", LfoTarget::Off),
    ("masterVolume", LfoTarget::Volume),
    ("vco1.gain", LfoTarget::Osc1Gain),
    ("vco1.detune", LfoTarget::Osc1Detune),
    ("vco1.octave", LfoTarget::Osc1Octave),
    ("vco2.gain", LfoTarget::Osc2Gain),
    ("vco2.detune", LfoTarget::Osc2Detune),
    ("vco2.octave", LfoTarget::Osc2Octave),
    ("vco3.gain", LfoTarget::Osc3Gain),
    ("vco3.detune", LfoTarget::Osc3Detune),
    ("vco3.octave", LfoTarget::Osc3Octave),
    ("filter.freq", LfoTarget::FilterFreq),
    ("filter.filterEnvelopeAmount", LfoTarget::FilterEnvAmount),
    ("filterEnv.filterEnvelopeAmount", LfoTarget::FilterEnvAmount),
    ("filter.Q", LfoTarget::FilterQ),
    ("noise.mix", LfoTarget::NoiseMix),
    ("subGain", LfoTarget::SubGain),
    ("filter.drive", LfoTarget::Drive),
    ("noise.filterFreq", LfoTarget::NoiseFilterFreq),
    ("noise.filterQ", LfoTarget::NoiseFilterQ),
    ("fm.amount", LfoTarget::FmAmount),
    ("fm.algo", LfoTarget::FmAlgorithm),
    ("envelope.attack", LfoTarget::EnvAttack),
    ("envelope.decay", LfoTarget::EnvDecay),
    ("envelope.sustain", LfoTarget::EnvSustain),
    ("envelope.release", LfoTarget::EnvRelease),
    ("modEnvelope.attack", LfoTarget::ModEnvAttack),
    ("modEnvelope.decay", LfoTarget::ModEnvDecay),
    ("modEnvelope.sustain", LfoTarget::ModEnvSustain),
    ("modEnvelope.release", LfoTarget::ModEnvRelease),
    ("vco1.width", LfoTarget::Osc1Width),
    ("vco2.width", LfoTarget::Osc2Width),
    ("vco3.width", LfoTarget::Osc3Width),
    ("noise.rate", LfoTarget::NoiseRate),
    ("pitchEnv.amount", LfoTarget::PitchEnvAmount),
    ("pitchEnv.decay", LfoTarget::PitchEnvDecay),
    ("vco2.ring", LfoTarget::Osc2Ring),
    ("filter.open", LfoTarget::FilterOpen),
    ("pitch", LfoTarget::Pitch),
    ("fm.boost", LfoTarget::FmBoost),
    ("shape", LfoTarget::Shape),
    ("filter2.freq", LfoTarget::Filter2Freq),
    ("filter2.balance", LfoTarget::Filter2Balance),
];

/// The targets that are no control of the editor but an effect on several:
/// open the filter, the pitch of the three oscillators, more FM, the shape.
pub const EFFECT_TARGETS: [&str; 4] = ["filter.open", "pitch", "fm.boost", "shape"];
/// `SYNC_NOTE_HZ`: LFO cycles per beat-second for each sync label.
const SYNC_NOTE_HZ: [(&str, f64); 7] = [
    ("1/1", 0.25),
    ("1/2", 0.5),
    ("1/4", 1.0),
    ("1/8", 2.0),
    ("1/16", 4.0),
    ("1/8T", (2.0 * 2.0) / 3.0),
    ("1/16T", (4.0 * 2.0) / 3.0),
];

/// Every preset by key (`generated_sounds.json` is an object of presets).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SynthPresets(Object);

impl SynthPresets {
    /// Read a presets file; anything but an object is an error.
    pub fn from_json(value: &Value) -> Result<Self, String> {
        value.as_object().cloned().map(Self).ok_or_else(|| "not an object of presets".to_owned())
    }

    /// What of the presets a patch cannot read, and plays as its default
    /// instead: a preset that is not an object, a group of one that is not
    /// an object, a value in a group that is not a number, text or yes/no,
    /// a level that is not a number, an older LFO or modulation envelope
    /// target the matrix has no free line for.
    #[must_use]
    pub fn issues(&self) -> Vec<Issue> {
        let mut issues = Vec::new();
        for (key, preset) in self.iter() {
            let Some(preset) = preset.as_object() else {
                issues.push(Issue::new(key, format!("not an object ({}), plays as the defaults", preset.to_json())));
                continue;
            };
            for (name, value) in preset.iter() {
                let at = format!("{key}.{name}");
                if PRESET_GROUPS.contains(&name) {
                    group_issues(value, &at, &mut issues);
                } else if PRESET_NUMBERS.contains(&name) && value.to_number().is_none() {
                    issues.push(Issue::new(at, refusal(value)));
                }
            }
            let mut upgraded = preset.clone();
            legacy_modulation::upgrade(&mut upgraded);
            for group in legacy_modulation::stranded(&upgraded) {
                let text = "no free line in the matrix for this target: not played";
                issues.push(Issue::new(format!("{key}.{group}.target"), text.to_owned()));
            }
        }
        issues
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Each preset with its key, in file order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter()
    }

    /// Preset keys, in file order.
    pub fn keys(&self) -> Vec<&str> {
        self.0.iter().map(|(k, _)| k).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Set preset `key` (a new key goes last).
    pub fn set(&mut self, key: &str, preset: Value) {
        self.0.insert(key, preset);
    }

    pub fn remove(&mut self, key: &str) -> Option<Value> {
        self.0.remove(key)
    }

    /// Add the presets of `other` this one does not hold, leaving its own as they are.
    pub fn add_missing(&mut self, other: &Self) {
        for key in other.keys() {
            if self.get(key).is_none()
                && let Some(preset) = other.get(key)
            {
                self.set(key, preset.clone());
            }
        }
    }

    /// The presets as `generated_sounds.json` holds them.
    pub fn to_json(&self) -> Value {
        Value::Object(self.0.clone())
    }
}

/// The keys of a preset that hold a group of settings (an object).
const PRESET_GROUPS: [&str; 16] = [
    "vco1",
    "vco2",
    "vco3",
    "noise",
    "filter",
    "filterEnv",
    "envelope",
    "modEnvelope",
    "pitchEnv",
    "fm",
    "unison",
    "lfo",
    "lfo2",
    "matrix",
    "filter2",
    "voice",
];
/// The keys of a preset that hold one number (`pitchPunch`: an older one,
/// read as the pitch envelope).
const PRESET_NUMBERS: [&str; 3] = ["pitchPunch", "subGain", "masterVolume"];

/// Report group `value` of a preset at `at` when it is not an object, or a
/// value of it no setting reads (a list, an object, `null`).
fn group_issues(value: &Value, at: &str, issues: &mut Vec<Issue>) {
    let Some(group) = value.as_object() else {
        issues.push(Issue::new(at, refusal(value)));
        return;
    };
    for (name, value) in group.iter() {
        if matches!(value, Value::Array(_) | Value::Object(_) | Value::Null) {
            issues.push(Issue::new(format!("{at}.{name}"), refusal(value)));
        }
    }
}

/// The note a patch is built for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SynthNote {
    /// Pitch ratio (1 = C3).
    pub ratio: f64,
    pub velocity: f64,
    pub pan: f64,
    pub bpm: f64,
}

/// The number `value` holds (a number or a numeric string), `fallback` without one.
fn finite(value: Option<&Value>, fallback: f64) -> f64 {
    value.and_then(Value::to_number).unwrap_or(fallback)
}

/// A level of a preset (a gain, a mix, the master volume), inside 0–1 as
/// its control is: a file's level outside would play louder than full scale.
fn level(value: Option<&Value>, fallback: f64) -> f64 {
    clamp(finite(value, fallback), 0.0, 1.0)
}

/// The choice of `choices` at the place `value` has in `names`; an unknown
/// name is the first choice.
fn choice<T: Copy + Default>(value: Option<&Value>, names: &[&str], choices: &[T]) -> T {
    let name = value.and_then(Value::as_str);
    names.iter().zip(choices).find(|(n, _)| Some(**n) == name).map_or_else(T::default, |(_, choice)| *choice)
}

/// The routes of a preset's `matrix` group: line `n` (1 to [`MATRIX_SIZE`])
/// is its `source<n>`, `target<n>` and `amount<n>`; a line missing is empty.
fn matrix(group: Option<&Value>) -> [MatrixRoute; MATRIX_SIZE] {
    std::array::from_fn(|i| {
        let key = |name: &str| format!("{name}{}", i + 1);
        MatrixRoute {
            source: choice(get(group, &key("source")), &MATRIX_SOURCES, &MatrixSource::ALL),
            target: lfo_target(get(group, &key("target"))),
            amount: clamp(finite(get(group, &key("amount")), 0.0), -1.0, 1.0),
        }
    })
}

fn lfo_target(value: Option<&Value>) -> LfoTarget {
    value.and_then(Value::as_str).map_or(LfoTarget::Off, lfo_target_named)
}

/// The LFO target a preset names `name`; off for an unknown one.
#[must_use]
pub fn lfo_target_named(name: &str) -> LfoTarget {
    LFO_TARGETS.iter().find(|(n, _)| *n == name).map_or(LfoTarget::Off, |&(_, target)| target)
}

/// The rate, Hz, of `sync` (a note value) at `bpm`: `None` when off, unknown, or without a tempo.
pub fn sync_to_hz(sync: Option<&Value>, bpm: f64) -> Option<f64> {
    let sync = sync?.as_str()?;
    if sync == "off" || bpm.is_nan() || bpm <= 0.0 {
        return None;
    }
    let base = SYNC_NOTE_HZ.iter().find(|(label, _)| *label == sync)?.1;
    Some(base * (bpm / 60.0))
}

/// Property `key` of `value` (`value?.[key]`; `None` for non-objects).
fn get<'a>(value: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    value.and_then(|v| v.as_object()).and_then(|o| o.get(key))
}

/// `a` unless missing or null, else `b`.
fn or_else<'a>(a: Option<&'a Value>, b: Option<&'a Value>) -> Option<&'a Value> {
    a.filter(|v| !v.is_null()).or(b)
}

/// The loudness of a note of `velocity` on a preset whose oscillators have
/// `gains`: normalised by what the voice mixes, then the master volume,
/// accented above half, raised by the voice's accent (`voice_accent`, 0–1),
/// and trimmed to the level of the kit samples.
fn peak_velocity(preset: &Value, gains: [f64; 3], velocity: f64, voice_accent: f64) -> f64 {
    let at = |key: &str| get(Some(preset), key);
    let total_gain = gains[0] + gains[1] + gains[2];
    // The voice mixes oscillators and noise by `noise.mix`: both count, or a
    // sound mostly made of noise would be raised by its quiet oscillators.
    let noise_mix =
        if at("bypassNoise").is_some_and(Value::is_true) { 0.0 } else { level(get(at("noise"), "mix"), 0.0) };
    let mix_gain = total_gain * (1.0 - noise_mix) + noise_mix;
    let mix_norm = if mix_gain > 0.001 { 1.0 / mix_gain } else { 1.0 };
    let note_velo = velocity * mix_norm;
    let accent = if note_velo > 0.5 { 1.0 + 0.5 * 0.5 } else { 1.0 };
    let master_volume = level(at("masterVolume"), 0.8);
    // The voice's accent, on top: louder as the velocity rises.
    let voice_accent = 1.0 + ACCENT_LEVEL * voice_accent * velocity;
    note_velo * master_volume * accent * voice_accent * SYNTH_TRIM
}

/// The frequency of each oscillator at `note_ratio` (1 is C3), its octave
/// and semitones added; 0 for a missing one.
fn osc_frequencies(vcos: [Option<&Value>; 3], note_ratio: f64) -> [f64; 3] {
    vcos.map(|vco| {
        if !vco.is_some_and(|v| v.as_object().is_some()) {
            return 0.0;
        }
        let octave = clamp(finite(get(vco, "octave"), 0.0), -4.0, 4.0);
        let semitones = clamp(finite(get(vco, "semitone"), 0.0), SEMITONE_RANGE.0, SEMITONE_RANGE.1);
        C3_FREQ * note_ratio.max(MIN_NOTE_RATIO) * 2.0_f64.powf(octave + semitones / 12.0)
    })
}

/// An envelope from `env`'s attack, decay, sustain (`sustain` when absent)
/// and release.
pub(crate) fn adsr(env: Option<&Value>, sustain: f64) -> Adsr {
    Adsr {
        attack: clamp(finite(get(env, "attack"), 0.01), 0.003, ATTACK_MAX),
        decay: finite(get(env, "decay"), 0.1).min(DECAY_MAX),
        sustain: finite(get(env, "sustain"), sustain),
        release: clamp(finite(get(env, "release"), 0.1), 0.008, RELEASE_MAX),
    }
}

/// The amplitude envelope of a preset's `envelope` group.
pub(crate) fn amp_adsr(env: Option<&Value>) -> Adsr {
    adsr(env, 0.7)
}

/// What a stage of the filter envelope's ADSR that a preset does not set
/// plays with: the amplitude envelope's attack, decay and release
/// (`envelope`), no sustain, as the filter envelope of the presets before.
#[must_use]
pub fn filter_adsr_default(envelope: Adsr) -> Adsr {
    Adsr { sustain: 0.0, ..envelope }
}

/// The filter envelope's own ADSR (`filterEnv`), when the preset sets one
/// of its stages; the others then take [`filter_adsr_default`].
fn filter_adsr(filter_env: Option<&Value>, envelope: Adsr) -> Option<Adsr> {
    if !FILTER_ENV_STAGES.iter().any(|stage| get(filter_env, stage).is_some()) {
        return None;
    }
    let base = filter_adsr_default(envelope);
    Some(Adsr {
        attack: clamp(finite(get(filter_env, "attack"), base.attack), 0.0, ATTACK_MAX),
        decay: clamp(finite(get(filter_env, "decay"), base.decay), 0.0, DECAY_MAX),
        sustain: clamp(finite(get(filter_env, "sustain"), base.sustain), 0.0, 1.0),
        release: clamp(finite(get(filter_env, "release"), base.release), 0.0, RELEASE_MAX),
    })
}

/// The second filter of a `filter2` group: off without one, or when bypassed.
fn filter2(group: Option<&Value>, is_bypassed: bool) -> Option<Filter2> {
    if is_bypassed || !group.is_some_and(|g| g.as_object().is_some()) {
        return None;
    }
    let mode = get(group, "type").and_then(Value::as_str).unwrap_or(FILTER2_TYPE_DEFAULT);
    Some(Filter2 {
        mode: choice(Some(&Value::from(mode)), &FILTER_TYPES, &FilterMode::ALL),
        freq: finite(get(group, "freq"), FILTER2_FREQ_DEFAULT),
        q: finite(get(group, "Q"), FILTER2_Q_DEFAULT),
        routing: choice(get(group, "routing"), &FILTER2_ROUTINGS, &Filter2Routing::ALL),
        balance: clamp(finite(get(group, "balance"), FILTER2_BALANCE_DEFAULT), 0.0, 1.0),
    })
}

/// The flag that switches each oscillator off.
pub const OSC_BYPASS_FLAGS: [&str; 3] = ["bypassVco1", "bypassVco2", "bypassVco3"];

/// Which modules of the preset are bypassed.
fn bypass_flags(preset: &Value) -> SynthBypass {
    let flag = |key: &str| get(Some(preset), key).is_some_and(Value::is_true);
    SynthBypass {
        noise: flag("bypassNoise"),
        filter: flag("bypassFilter"),
        env: flag("bypassEnv"),
        lfo1: flag("bypassLfo1"),
        lfo2: flag("bypassLfo2"),
        fm: flag("bypassFm"),
        mod_env: flag("bypassModEnv"),
        filter_env: flag("bypassFilterEnv"),
        osc: OSC_BYPASS_FLAGS.map(flag),
    }
}

/// The LFO an `lfo` object of a preset describes, its sync read at `bpm`.
fn synth_lfo(lfo: Option<&Value>, bpm: f64) -> SynthLfo {
    SynthLfo {
        wave: choice(get(lfo, "wave"), &WAVES, &Wave::ALL),
        freq: sync_to_hz(get(lfo, "sync"), bpm).unwrap_or_else(|| finite(get(lfo, "freq"), 0.0)),
        delay_seconds: clamp(finite(get(lfo, "delay"), 0.0), 0.0, LFO_DELAY_MAX),
        is_retriggered: get(lfo, "retrigger").is_some_and(Value::is_true),
    }
}

/// The pitch envelope a `pitchEnv` object of a preset describes, in range.
fn pitch_env(pitch_env: Option<&Value>) -> PitchEnv {
    PitchEnv {
        amount: clamp(finite(get(pitch_env, "amount"), 0.0), PITCH_ENV_AMOUNT_RANGE.0, PITCH_ENV_AMOUNT_RANGE.1),
        decay: clamp(
            finite(get(pitch_env, "decay"), PITCH_ENV_DECAY_DEFAULT),
            PITCH_ENV_DECAY_RANGE.0,
            PITCH_ENV_DECAY_RANGE.1,
        ),
    }
}

/// The `update` message `WorkletSynthVoice` sends for `note` with `preset`.
pub fn synth_patch(preset: &Value, note: SynthNote) -> SynthPatch {
    let preset = legacy_modulation::upgraded(preset);
    let preset = preset.as_ref();
    let gs = Some(preset);
    let at = |key: &str| get(gs, key);
    let note_ratio = finite(Some(&Value::Number(note.ratio)), 1.0).max(MIN_NOTE_RATIO);

    let vcos = [at("vco1"), at("vco2"), at("vco3")];
    let bypass = bypass_flags(preset);
    // Switched off, an oscillator counts for nothing in the mix.
    let gains = std::array::from_fn(|n| if bypass.osc[n] { 0.0 } else { level(get(vcos[n], "gain"), 0.0) });
    let voice = at("voice");
    let accent = level(get(voice, "accent"), 0.0);
    let velocity = clamp(finite(Some(&Value::Number(note.velocity)), 0.0), 0.0, 1.0);
    let peak = peak_velocity(preset, gains, note.velocity, accent);

    let osc_freq = osc_frequencies(vcos, note_ratio);

    let noise = at("noise");
    let filter = at("filter");
    let env = at("envelope");
    let mod_env = at("modEnvelope");
    let fm = at("fm");
    let unison = at("unison");
    // Bypassed, the pitch envelope moves nothing.
    let is_pitch_bypassed = at("bypassPitchEnv").is_some_and(Value::is_true);
    let envelope = amp_adsr(env);
    let filter_env = or_else(at("filterEnv"), filter);
    let filter_env_amount = clamp(finite(get(filter_env, "filterEnvelopeAmount"), 0.0), 0.0, 1.0);

    SynthPatch {
        osc_freq,
        osc_gain: gains,
        osc_detune: vcos.map(|vco| finite(get(vco, "detune"), 0.0)),
        osc_wave: vcos.map(|vco| choice(get(vco, "wave"), &WAVES, &Wave::ALL)),
        osc_width: vcos.map(|vco| level(get(vco, "width"), 0.5)),
        ring: level(get(vcos[1], "ring"), 0.0),
        is_osc2_synced: get(vcos[1], "sync").and_then(Value::as_str) == Some(OSC2_SYNCS[1]),
        noise_mix: level(get(noise, "mix"), 0.0),
        noise_mode: choice(get(noise, "mode"), &NOISE_MODES, &NoiseMode::ALL),
        noise_rate: clamp(finite(get(noise, "rate"), NOISE_RATE_DEFAULT), NOISE_RATE_RANGE.0, NOISE_RATE_RANGE.1),
        noise_filter_type: choice(get(noise, "filterType"), &FILTER_TYPES, &FilterMode::ALL),
        noise_filter_freq: finite(get(noise, "filterFreq"), 1000.0),
        noise_filter_q: finite(get(noise, "filterQ"), 0.7),
        filter_type: choice(get(filter, "type"), &FILTER_TYPES, &FilterMode::ALL),
        filter_freq: finite(get(filter, "freq"), 1000.0),
        filter_q: finite(get(filter, "Q"), 0.7),
        filter_slope: finite(get(filter, "slope"), 12.0),
        drive: finite(get(filter, "drive"), 0.0),
        pitch_env: if is_pitch_bypassed {
            PitchEnv { amount: 0.0, ..pitch_env(at("pitchEnv")) }
        } else {
            pitch_env(at("pitchEnv"))
        },
        sub_gain: level(at("subGain"), 0.0),
        envelope,
        volume: 1.0,
        pan: finite(Some(&Value::Number(note.pan)), 0.0),
        velocity: peak,
        lfo1: synth_lfo(at("lfo"), note.bpm),
        lfo2: synth_lfo(at("lfo2"), note.bpm),
        // The accent opens it further, the more the louder the note.
        filter_env_amount: filter_env_amount + (1.0 - filter_env_amount) * accent * velocity,
        filter_env: FilterEnv {
            adsr: filter_adsr(at("filterEnv"), envelope),
            is_inverted: get(at("filterEnv"), "invert").is_some_and(Value::is_true),
        },
        filter2: filter2(at("filter2"), at("bypassFilter2").is_some_and(Value::is_true)),
        voice_mode: choice(get(voice, "mode"), &VOICE_MODES, &VoiceMode::ALL),
        glide_seconds: clamp(finite(get(voice, "glide"), 0.0), 0.0, GLIDE_MAX),
        fm_amount: clamp(finite(get(fm, "amount"), 0.0), 0.0, 1.0),
        fm_algorithm: clamp(finite(get(fm, "algo"), 0.0).round(), 0.0, 4.0),
        mod_envelope: adsr(mod_env, 0.0),
        unison_count: unison_count(get(unison, "count")),
        unison_detune: clamp(finite(get(unison, "detune"), 0.0), 0.0, 100.0),
        unison_spread: clamp(finite(get(unison, "spread"), 0.0), 0.0, 1.0),
        matrix: matrix(at("matrix")),
        note_velocity: velocity,
        // Two octaves each way fill the source: ratio 1 is C3.
        note_key: clamp(note_ratio.log2() / 2.0, -1.0, 1.0),
        bypass,
    }
}

/// The layers of a unison from a preset's `count`: a whole number from 1 to [`MAX_UNISON`].
fn unison_count(value: Option<&Value>) -> usize {
    let count = clamp(finite(value, 1.0).round(), 1.0, MAX_UNISON as f64);
    // In range and whole: the conversion loses nothing.
    count as usize
}

/// Note length of a synth note: one grid step (`60 / bpm / stepsPerBeat`).
pub fn note_seconds(bpm: f64, steps_per_beat: i32) -> f64 {
    60.0 / bpm / f64::from(steps_per_beat.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn a_level_outside_its_control_is_brought_inside() {
        let patch = synth_patch(
            &json::parse(
                r#"{"masterVolume":1e308,"subGain":-3,"vco1":{"gain":2},"vco2":{"gain":-1e308},"noise":{"mix":7}}"#,
            )
            .unwrap(),
            SynthNote { ratio: 1.0, velocity: 0.4, pan: 0.0, bpm: 120.0 },
        );
        assert_eq!((patch.osc_gain, patch.noise_mix, patch.sub_gain), ([1.0, 0.0, 0.0], 1.0, 0.0));
        // All noise at full mix: the note's velocity at a master volume of 1, trimmed.
        assert!((patch.velocity - 0.4 * SYNTH_TRIM).abs() < 1e-12, "{}", patch.velocity);
    }

    #[test]
    fn missing_sections_fall_back() {
        let patch = synth_patch(
            &json::parse(r#"{"vco1":{"gain":0.5,"wave":"square","octave":-1}}"#).unwrap(),
            SynthNote { ratio: 2.0, velocity: 0.8, pan: 0.0, bpm: 120.0 },
        );
        assert_eq!(patch.osc_freq, [C3_FREQ, 0.0, 0.0]);
        assert_eq!(patch.osc_wave, [Wave::Square, Wave::Sine, Wave::Sine]);
        // 0.8 / 0.5 = 1.6, accented (×1.25), master volume 0.8, trimmed.
        assert!((patch.velocity - 1.6 * 0.8 * 1.25 * SYNTH_TRIM).abs() < 1e-12);
        assert_eq!(patch.envelope, Adsr { attack: 0.01, decay: 0.1, sustain: 0.7, release: 0.1 });
    }

    #[test]
    fn the_chip_controls_are_read_and_bounded() {
        let note = SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 120.0 };
        let patch = synth_patch(
            &json::parse(
                r#"{"vco1":{"wave":"square","width":0.125},"vco2":{"width":9},"noise":{"mode":"short","rate":1}}"#,
            )
            .unwrap(),
            note,
        );
        assert_eq!(patch.osc_width, [0.125, 1.0, 0.5]);
        assert_eq!((patch.noise_mode, patch.noise_rate), (NoiseMode::Short, NOISE_RATE_RANGE.0));
        let plain = synth_patch(&json::parse("{}").unwrap(), note);
        assert_eq!((plain.noise_mode, plain.noise_rate), (NoiseMode::White, NOISE_RATE_DEFAULT));
    }

    #[test]
    fn the_pitch_envelope_ring_and_sync_are_read_and_bounded() {
        let note = SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 120.0 };
        let patch = synth_patch(
            &json::parse(r#"{"pitchEnv":{"amount":-99,"decay":0},"vco2":{"ring":0.25,"sync":"vco1"}}"#).unwrap(),
            note,
        );
        assert_eq!(patch.pitch_env, PitchEnv { amount: -MAX_PITCH_ENV_SEMITONES, decay: PITCH_ENV_DECAY_RANGE.0 });
        assert_eq!((patch.ring, patch.is_osc2_synced), (0.25, true));
        let plain = synth_patch(&json::parse(r#"{"vco2":{"sync":"nope"}}"#).unwrap(), note);
        assert_eq!(plain.pitch_env, PitchEnv { amount: 0.0, decay: PITCH_ENV_DECAY_DEFAULT });
        assert_eq!((plain.ring, plain.is_osc2_synced), (0.0, false));
        // Bypassed: no swing nor punch, the decay kept for when it is switched back on.
        let preset = r#"{"pitchEnv":{"amount":12,"decay":0.2},"bypassPitchEnv":true}"#;
        let bypassed = synth_patch(&json::parse(preset).unwrap(), note);
        assert_eq!(bypassed.pitch_env, PitchEnv { amount: 0.0, decay: 0.2 });
    }

    #[test]
    fn what_a_patch_cannot_read_is_said() {
        let text = r#"{"ok":{"vco1":{"wave":"saw","gain":0.5},"subGain":"0.2"},"flat":3,
            "odd":{"filter":[1],"noise":{"mix":null},"masterVolume":"loud","style":"x"}}"#;
        let presets = SynthPresets::from_json(&json::parse(text).unwrap()).unwrap();
        let at: Vec<String> = presets.issues().into_iter().map(|i| i.at).collect();
        assert_eq!(at, ["flat", "odd.filter", "odd.noise.mix", "odd.masterVolume"]);
    }

    #[test]
    fn add_missing_keeps_what_is_there() {
        let presets = |text: &str| SynthPresets::from_json(&json::parse(text).unwrap()).unwrap();
        let mut saved = presets(r#"{"A":{"masterVolume":0.1}}"#);
        saved.add_missing(&presets(r#"{"A":{"masterVolume":0.9},"B":{"masterVolume":0.5}}"#));
        assert_eq!(saved, presets(r#"{"A":{"masterVolume":0.1},"B":{"masterVolume":0.5}}"#));
    }

    #[test]
    fn the_level_counts_the_noise_in_the_mix() {
        let velocity = |preset: &str| {
            synth_patch(&json::parse(preset).unwrap(), SynthNote { ratio: 1.0, velocity: 0.8, pan: 0.0, bpm: 120.0 })
                .velocity
        };
        // Oscillators at 0.4 for 20% of the mix, noise for 80%: 0.88 in all.
        let noisy = velocity(r#"{"masterVolume":1,"vco1":{"gain":0.4},"noise":{"mix":0.8}}"#);
        assert!((noisy - 0.8 / 0.88 * 1.25 * SYNTH_TRIM).abs() < 1e-12, "{noisy}");
        // Bypassed, the noise is not in the mix.
        let bypassed = velocity(r#"{"masterVolume":1,"vco1":{"gain":0.4},"noise":{"mix":0.8},"bypassNoise":true}"#);
        assert!((bypassed - 0.8 / 0.4 * 1.25 * SYNTH_TRIM).abs() < 1e-12, "{bypassed}");
    }

    #[test]
    fn lfo_sync_and_targets() {
        let preset = json::parse(
            r#"{"lfo":{"target":"filter.Q","depth":0.3,"wave":"random","freq":3,"sync":"1/8"},"lfo2":{"target":"x"}}"#,
        )
        .unwrap();
        let patch = synth_patch(&preset, SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 90.0 });
        assert_eq!((patch.lfo1.wave, patch.lfo1.freq), (Wave::Random, 3.0));
        // An older LFO target is a line of the matrix; one without depth plays nothing.
        let mut routes = [MatrixRoute::default(); MATRIX_SIZE];
        routes[0] = MatrixRoute { source: MatrixSource::Lfo1, target: LfoTarget::FilterQ, amount: 0.3 };
        assert_eq!(patch.matrix, routes);
        assert_eq!(note_seconds(120.0, 4), 0.125);
    }

    #[test]
    fn the_filter_envelope_filter_2_and_voice_are_read() {
        let note = SynthNote { ratio: 1.0, velocity: 0.5, pan: 0.0, bpm: 120.0 };
        let read = |text: &str| synth_patch(&json::parse(text).unwrap(), note);
        let plain = read(r#"{"envelope":{"attack":0.02,"decay":0.3,"release":0.4}}"#);
        assert_eq!(plain.filter_env, FilterEnv { adsr: None, is_inverted: false });
        assert_eq!((plain.filter2, plain.voice_mode, plain.glide_seconds), (None, VoiceMode::Poly, 0.0));
        // One stage set: the others from the amplitude envelope, no sustain.
        let own =
            read(r#"{"envelope":{"attack":0.02,"decay":0.3,"release":0.4},"filterEnv":{"sustain":0.6,"invert":true}}"#);
        let adsr = Adsr { attack: 0.02, decay: 0.3, sustain: 0.6, release: 0.4 };
        assert_eq!(own.filter_env, FilterEnv { adsr: Some(adsr), is_inverted: true });
        let second = read(r#"{"filter2":{"freq":2000},"voice":{"mode":"legato","glide":9}}"#);
        let serial = Filter2 {
            mode: FilterMode::Highpass,
            freq: 2000.0,
            q: FILTER2_Q_DEFAULT,
            routing: Filter2Routing::Serial,
            balance: FILTER2_BALANCE_DEFAULT,
        };
        assert_eq!(second.filter2, Some(serial));
        let beside = read(r#"{"filter2":{"routing":"parallel","balance":3}}"#).filter2;
        assert_eq!(beside.map(|f| (f.routing, f.balance)), Some((Filter2Routing::Parallel, 1.0)));
        assert_eq!((second.voice_mode, second.glide_seconds), (VoiceMode::Legato, GLIDE_MAX));
        assert_eq!(read(r#"{"filter2":{},"bypassFilter2":true}"#).filter2, None);
        let lfo = read(r#"{"lfo":{"delay":0.5,"retrigger":true}}"#).lfo1;
        assert_eq!((lfo.delay_seconds, lfo.is_retriggered), (0.5, true));
    }

    #[test]
    fn the_accent_opens_the_filter_envelope_and_raises_the_level() {
        let at = |accent: f64, velocity: f64| {
            let text = format!(r#"{{"filterEnv":{{"filterEnvelopeAmount":0.2}},"voice":{{"accent":{accent}}}}}"#);
            synth_patch(&json::parse(&text).unwrap(), SynthNote { ratio: 1.0, velocity, pan: 0.0, bpm: 120.0 })
        };
        assert_eq!(at(0.0, 1.0).filter_env_amount, 0.2);
        assert!((at(1.0, 1.0).filter_env_amount - 1.0).abs() < 1e-12);
        assert!((at(1.0, 0.5).filter_env_amount - 0.6).abs() < 1e-12);
        assert!((at(1.0, 1.0).velocity / at(0.0, 1.0).velocity - 1.5).abs() < 1e-12);
    }

    #[test]
    fn a_matrix_is_read_line_by_line() {
        let preset = r#"{"matrix":{"source2":"lfo2","target2":"filter.freq","amount2":0.5,
            "source3":"nope","target3":"nope","amount3":9,"amount4":"-0.25"}}"#;
        let note = SynthNote { ratio: 4.0, velocity: 0.5, pan: 0.0, bpm: 120.0 };
        let patch = synth_patch(&json::parse(preset).unwrap(), note);
        let mut routes = [MatrixRoute::default(); MATRIX_SIZE];
        routes[1] = MatrixRoute { source: MatrixSource::Lfo2, target: LfoTarget::FilterFreq, amount: 0.5 };
        // A name that is none leaves a line empty, and an amount stays in range.
        routes[2].amount = 1.0;
        routes[3].amount = -0.25;
        assert_eq!(patch.matrix, routes);
        // Two octaves up is the end of the key source.
        assert_eq!((patch.note_key, patch.note_velocity), (1.0, 0.5));
    }
}
