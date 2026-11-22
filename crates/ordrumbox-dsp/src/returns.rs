//! The two returns every track can send to: a tape echo and a spring
//! reverb, shared, so that a sound sent for a moment keeps echoing after its
//! send closes (a dub "throw"). Each return is played by a [`crate::Strip`]
//! that plays only its effect; this module gives the settings it plays with.

use crate::StripSettings;

/// Cutoff of the highpass on the echo's return, Hz: the repeats stay out of
/// the kick's and the bass's range.
const ECHO_LOW_CUT_HZ: f32 = 120.0;
/// The strip's filter mode for a highpass.
const HIGHPASS: f32 = 1.0;
/// The strip's delay mode where each side repeats itself (a tape).
const TAPE_MODE: f32 = 1.0;

audio_params! {
    /// The returns' settings.
    pub struct ReturnSettings {
        /// Echo time, beats (the engine fits it to the delay line at the tempo).
        echo_beats = 0.75, 0.25, 4.0;
        /// Share of each repeat fed back: near the top, the echo builds up.
        echo_feedback = 0.55, 0.0, 0.99;
        /// The one-pole lowpass in the feedback: lower is darker.
        echo_tone = 0.35, 0.05, 1.0;
        /// Depth of the tape wow, seconds of echo time.
        echo_wow = 0.0015, 0.0, 0.005;
        /// Share of the echo's output sent on to the reverb.
        echo_reverb = 0.25, 0.0, 1.0;
        /// Level of the echo's return in the mix.
        echo_level = 1.0, 0.0, 1.0;
        reverb_room = 0.8, 0.0, 1.0;
        reverb_damping = 0.4, 0.0, 1.0;
        /// Spread of the spring (the strip's `reverb_dispersion`).
        reverb_spring = 0.6, 0.0, 0.9;
        /// Level of the reverb's return in the mix.
        reverb_level = 1.0, 0.0, 1.0;
    }
}

impl ReturnSettings {
    /// The echo return's strip, its echo `delay_seconds` long.
    #[must_use]
    pub fn echo_strip(&self, delay_seconds: f32) -> StripSettings {
        StripSettings {
            cutoff: ECHO_LOW_CUT_HZ,
            filter_mode: HIGHPASS,
            reverb_on: 0.0,
            delay_time_l: delay_seconds,
            delay_time_r: delay_seconds,
            delay_feedback: self.echo_feedback,
            delay_mix: 1.0,
            delay_mode: TAPE_MODE,
            delay_tone: self.echo_tone,
            delay_wow: self.echo_wow,
            dry: 0.0,
            volume: self.echo_level,
            ..StripSettings::default()
        }
    }

    /// The reverb return's strip.
    #[must_use]
    pub fn reverb_strip(&self) -> StripSettings {
        StripSettings {
            filter_on: 0.0,
            reverb_room: self.reverb_room,
            reverb_damping: self.reverb_damping,
            reverb_mix: 1.0,
            reverb_dispersion: self.reverb_spring,
            delay_on: 0.0,
            delay_mix: 0.0,
            dry: 0.0,
            volume: self.reverb_level,
            ..StripSettings::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Strip;

    /// A click through a fresh strip set to `settings`, left side.
    fn click_through(settings: &StripSettings, frames: usize) -> Vec<f32> {
        let mut input = vec![0.0; frames];
        input[0] = 0.5;
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        strip.process(settings, None, &input, &input, &mut l, &mut r);
        l
    }

    #[test]
    fn the_echo_return_plays_the_repeats_and_not_the_sound() {
        let returns = ReturnSettings::default();
        let out = click_through(&returns.echo_strip(0.01), 2_000);
        assert!(out[..400].iter().all(|s| s.abs() < 1e-6), "no dry sound");
        assert!(out[470..500].iter().any(|s| s.abs() > 1e-3), "a repeat after 480 frames");
        assert!(out[950..980].iter().any(|s| s.abs() > 1e-4), "and another");
    }

    #[test]
    fn the_reverb_return_plays_only_its_tail() {
        let out = click_through(&ReturnSettings::default().reverb_strip(), 24_000);
        assert!(out[0].abs() < 1e-6, "no dry sound");
        assert!(out[4_800..].iter().any(|s| s.abs() > 1e-5), "a tail");
    }

    #[test]
    fn an_echo_at_the_top_of_its_feedback_builds_up_and_stays_bounded() {
        let returns = ReturnSettings { echo_feedback: ReturnSettings::MAX.echo_feedback, ..ReturnSettings::default() };
        let out = click_through(&returns.echo_strip(0.05), 48_000 * 10);
        let last_second = &out[48_000 * 9..];
        let peak = last_second.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        assert!(peak > 1e-3, "still repeating after 10 s: {peak}");
        assert!(out.iter().all(|s| s.is_finite() && s.abs() <= 2.0), "bounded");
    }

    #[test]
    fn a_return_at_level_zero_is_silent() {
        let returns = ReturnSettings { echo_level: 0.0, reverb_level: 0.0, ..ReturnSettings::default() };
        assert!(click_through(&returns.echo_strip(0.01), 2_000).iter().all(|s| *s == 0.0));
        assert!(click_through(&returns.reverb_strip(), 4_000).iter().all(|s| *s == 0.0));
    }
}
