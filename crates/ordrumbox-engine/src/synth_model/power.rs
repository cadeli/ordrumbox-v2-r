//! The cards' power: which cards have a power button and its flag, which
//! show off (an oscillator silent and modulating nothing, an LFO at 0 Hz),
//! what turning one on or off does, and which controls do nothing as a
//! preset stands.

use ordrumbox_core::json::{Object, Value};

use super::{LFOS, NOISE_MODES, OSC_BYPASS_FLAGS, WAVES, number, set, text};
use crate::synth_preset::FILTER2_ROUTINGS;

/// The cards' bypass flags (`CARD_BYPASS_FLAGS`).
pub const BYPASS_FLAGS: [(&str, &str); 13] = [
    ("vco1", OSC_BYPASS_FLAGS[0]),
    ("vco2", OSC_BYPASS_FLAGS[1]),
    ("vco3", OSC_BYPASS_FLAGS[2]),
    ("noise", "bypassNoise"),
    ("filter", "bypassFilter"),
    ("filterEnv", "bypassFilterEnv"),
    ("envelope", "bypassEnv"),
    ("lfo", "bypassLfo1"),
    ("lfo2", "bypassLfo2"),
    ("fm", "bypassFm"),
    ("modEnvelope", "bypassModEnv"),
    ("pitchEnv", "bypassPitchEnv"),
    ("filter2", "bypassFilter2"),
];

/// Which cards show bypassed: a card whose bypass flag is set, an
/// oscillator at gain 0 that modulates nothing either, an LFO at 0 Hz, the
/// second filter while the preset has none.
pub fn bypassed(draft: &Object, group: &str) -> bool {
    if group == "filter2" && draft.get(group).is_none() {
        return true;
    }
    let is_flagged = BYPASS_FLAGS
        .iter()
        .find(|(g, _)| *g == group)
        .is_some_and(|(_, flag)| draft.get(flag).is_some_and(Value::is_true));
    let is_silent = |n| number(draft, &format!("{group}.gain")) == Some(0.0) && !is_fm_modulator(draft, n);
    is_flagged || vco_index(group).is_some_and(is_silent) || is_still_lfo(draft, group)
}

/// Whether `group` is an LFO that does not move: at 0 Hz, synced to nothing.
fn is_still_lfo(draft: &Object, group: &str) -> bool {
    let is_synced = text(draft, &format!("{group}.sync")).is_some_and(|sync| sync != "off");
    LFOS.iter().any(|lfo| lfo.group == group)
        && !is_synced
        && number(draft, &format!("{group}.freq")).is_none_or(|freq| freq <= 0.0)
}

/// The rate an LFO turned on at 0 Hz takes, Hz.
pub(super) const LFO_ON_HZ: f64 = 1.0;

/// Whether a card has a bypass state (a power button).
pub fn has_power(group: &str) -> bool {
    BYPASS_FLAGS.iter().any(|(g, _)| *g == group)
}

/// A card turned on or off by its power button: a flagged card sets its
/// flag (the envelope, turned on, is reset); an oscillator turned on that
/// would still be silent and modulate nothing gets its gain back, an LFO
/// at 0 Hz a rate.
pub fn toggle_card(draft: &mut Object, group: &str) {
    let off = !bypassed(draft, group);
    let Some((_, flag)) = BYPASS_FLAGS.iter().find(|(g, _)| *g == group) else { return };
    draft.insert(*flag, Value::Bool(off));
    if group == "envelope" && !off {
        draft.insert("_resetEnv", Value::Bool(true));
    }
    if group == "filter2" && draft.get(group).is_none() {
        draft.insert(group, super::extras::filter2_defaults());
    }
    if !off && is_still_lfo(draft, group) {
        set(draft, &format!("{group}.freq"), Value::from(LFO_ON_HZ));
    }
    if !off && vco_index(group).is_some() && bypassed(draft, group) {
        let gain = if group == "vco1" { 1.0 } else { IMPLICIT_GAIN };
        if let Some(Value::Object(vco)) = draft.get_mut(group) {
            let saved = vco.remove(SAVED_GAIN).and_then(|v| v.as_f64()).filter(|g| *g > 0.0);
            vco.insert("gain", Value::from(saved.unwrap_or(gain)));
        }
    }
}

/// Where an oscillator kept its gain when older versions switched it off by
/// setting it to 0.
pub(super) const SAVED_GAIN: &str = "_savedGain";
/// The gain an oscillator off by default takes when one of its controls is touched.
pub(super) const IMPLICIT_GAIN: f64 = 0.5;

/// The index of oscillator `group` (`vco1` to `vco3`), 0 to 2.
pub(super) fn vco_index(group: &str) -> Option<usize> {
    ["vco1", "vco2", "vco3"].iter().position(|g| *g == group)
}

/// Whether oscillator `n` (0 to 2) modulates another through FM in `draft`.
fn is_fm_modulator(draft: &Object, n: usize) -> bool {
    let is_fm_on =
        !draft.get("bypassFm").is_some_and(Value::is_true) && number(draft, "fm.amount").unwrap_or(0.0) > 0.0;
    // A whole algorithm, 0 to 4: the cast saturates.
    let algorithm = number(draft, "fm.algo").unwrap_or(0.0).round().max(0.0) as usize;
    is_fm_on && ordrumbox_dsp::synth::fm_modulators(algorithm)[n]
}

/// Whether control `path` does nothing as `draft` stands: an oscillator's
/// pulse width under a wave other than the square, the noise's clock in
/// white noise, the balance of the two filters in series.
#[must_use]
pub fn is_inert(draft: &Object, path: &str) -> bool {
    match path.split_once('.') {
        Some((group, "width")) if vco_index(group).is_some() => {
            text(draft, &format!("{group}.wave")).unwrap_or(WAVES[0]) != SQUARE
        }
        Some(("noise", "rate")) => text(draft, "noise.mode").unwrap_or(NOISE_MODES[0]) == NOISE_MODES[0],
        Some(("filter2", "balance")) => {
            text(draft, "filter2.routing").unwrap_or(FILTER2_ROUTINGS[0]) == FILTER2_ROUTINGS[0]
        }
        _ => false,
    }
}

/// The wave whose pulse width an oscillator's `width` sets.
const SQUARE: &str = "square";

#[cfg(test)]
mod tests {
    use super::super::defaults;
    use super::*;

    #[test]
    fn the_width_and_the_noise_clock_do_nothing_where_they_have_no_effect() {
        let mut d = defaults();
        assert!(is_inert(&d, "vco2.width") && is_inert(&d, "noise.rate"));
        set(&mut d, "vco2.wave", Value::from(SQUARE));
        set(&mut d, "noise.mode", Value::from(NOISE_MODES[1]));
        assert!(!is_inert(&d, "vco2.width") && !is_inert(&d, "noise.rate"));
        assert!(is_inert(&d, "vco1.width"));
        assert!(!is_inert(&d, "vco2.gain"));
    }

    #[test]
    fn the_balance_does_nothing_with_the_filters_in_series() {
        let mut d = defaults();
        assert!(is_inert(&d, "filter2.balance"));
        set(&mut d, "filter2.routing", Value::from(FILTER2_ROUTINGS[1]));
        assert!(!is_inert(&d, "filter2.balance"));
    }
}
