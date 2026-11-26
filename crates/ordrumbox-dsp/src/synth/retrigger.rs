//! A note played over a voice still sounding, as mono and legato play
//! them: the envelopes start again or go on, and the pitch may glide from
//! where it stood.

use super::*;

/// How a note played over a voice still sounding takes it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retrigger {
    /// The envelopes start again, at the new pitch at once.
    Restart,
    /// The envelopes start again, and the pitch glides from where it stood (mono).
    Glide,
    /// The envelopes go on, and the pitch glides from where it stood (legato).
    Legato,
}

impl SynthVoice {
    /// Play a note of `patch` at `time` (seconds, same clock) over the note
    /// this active voice plays, as `how` says; the release set is cleared.
    /// Phases and filters go on: no click.
    pub fn retrigger(&mut self, patch: &SynthPatch, time: f64, serial: u64, how: Retrigger) {
        let offset = if how == Retrigger::Restart { 0.0 } else { self.pitch_offset_to(patch) };
        let sources = SourceValues {
            velocity: clamp(patch.note_velocity, 0.0, 1.0),
            key: clamp(patch.note_key, -1.0, 1.0),
            random: noise_sample(xorshift32(noise_seed(serial))),
            ..self.sources
        };
        self.serial = serial;
        self.release_time = -1.0;
        self.glide = PitchFall::START;
        self.glide_semitones = if patch.glide_seconds > 0.0 { offset } else { 0.0 };
        if how == Retrigger::Legato {
            // The note goes on: its level and what was drawn for it stay.
            self.patch =
                SynthPatch { velocity: self.patch.velocity, note_velocity: self.patch.note_velocity, ..*patch };
            self.sources.key = sources.key;
            return;
        }
        self.patch = *patch;
        self.sources = sources;
        let t = time - self.start_time;
        self.env.restart(t);
        self.filter_env.restart(t);
        self.filter_adsr.restart(t);
        self.mod_env.restart(t);
        self.pitch_fall = PitchFall::START;
        self.note_start = time;
        // Seeded again: an LFO that restarts with each note starts with this one.
        self.lfo_seeded = false;
    }

    /// Semitones from the pitch of `patch` to the one this voice sounds now
    /// (its glide included), by the first oscillator both have; 0 without one.
    fn pitch_offset_to(&self, patch: &SynthPatch) -> f64 {
        let Some((now, next)) =
            self.patch.osc_freq.iter().zip(patch.osc_freq).find(|(now, next)| **now > 0.0 && *next > 0.0)
        else {
            return 0.0;
        };
        12.0 * (now / next).log2() + self.glide_semitones * self.glide.left()
    }
}
