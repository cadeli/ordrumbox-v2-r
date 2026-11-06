//! What a control of the track editor, the note editor, the kit editor or the
//! header is, for its tooltip: its whole name, then what it does. The panels
//! label their controls with a word or two; the tooltip says what they do.

use crate::kits::Field;
use crate::panel::{Id, LfoParam};

/// A control's tooltip: its whole name and what it does.
pub type Help = [&'static str; 2];

/// The tooltip of control `id` (or of the row label of a setting), if it has one.
pub fn help(id: Id) -> Option<Help> {
    Some(match id {
        Id::Tab(tab) => tab_help(tab)?,
        Id::FxTab(fx) | Id::FxLed(fx) => fx_help(fx)?,
        Id::GenerationTab(tab) => match tab {
            0 => ["Groove (Manual out of auto mode)", "how the hits vary from loop to loop"],
            1 => ["Engine", "what the auto engine adds to the notes"],
            _ => ["Compose", "how a drum line is written: hits, complexity, style"],
        },
        Id::GenerationAdvanced => ["Advanced settings", "the manual, Groove, Engine and Compose settings"],
        Id::Regenerate => ["New line", "write this track's line again, in its style"],
        Id::NoteTab(tab) => match tab {
            0 => ["Trigger", "when the note plays: every N loops, its chance"],
            1 => ["Retrigger", "repeats of the note, how fast, their chance"],
            2 => ["Euclidean", "hits spread evenly up to the next note"],
            _ => ["Arpeggio", "the note played as a run of scale notes"],
        },
        Id::LfoTarget => ["LFO target", "the setting the LFO below moves"],
        Id::LfoLed(_) => ["LFO on / off", "a lit LED: this setting has an LFO"],
        Id::LfoType => ["LFO wave", "the shape the setting follows"],
        Id::Lfo(param) => match param {
            LfoParam::Freq => ["LFO frequency", "how fast the setting moves, in Hz"],
            LfoParam::Min => ["LFO minimum", "the lowest value the setting reaches"],
            LfoParam::Max => ["LFO maximum", "the highest value the setting reaches"],
            LfoParam::Phase => ["LFO phase", "where in its wave the LFO starts"],
        },
        Id::FilterIcon(_) => ["Filter type", "LP lowpass, HP highpass, BP bandpass, NT notch"],
        Id::AutoAssign => ["Auto-assign", "the kit picks the sample from the track's name"],
        Id::Instrument => ["Instrument", "the kind of sound the track asks the kit for"],
        Id::Sample => ["Sample", "the audio file the track plays"],
        Id::Synth => ["Synth preset", "the soft-synth sound the track plays instead of a sample"],
        Id::LoadSample => ["Load sample", "replace the track's sample with a file"],
        Id::KitKnob(field) => match field {
            Field::Gain => ["Gain", "the sample's level, in dB"],
            Field::Transpose => ["Transpose", "the sample's pitch, in semitones"],
            Field::Decay => ["Decay", "how long the sample rings, in ms"],
        },
        Id::Bpm => ["Tempo", "beats per minute: every pattern's; in the song panel, the song's"],
        Id::Beats => ["Beats", "the pattern's length, in beats"],
        Id::StyleList => ["Style", "picking one makes the pattern again in it"],
        Id::GeneratePattern => ["Make again (B)", "the pattern made again in its style"],
        Id::AutoAll => ["Auto, every track", "every track in auto mode, or out of it"],
        Id::Macro(key) => macro_help(key)?,
        Id::View(view) => view_help(view)?,
        Id::TrackVolume(_) => ["Volume", "the track's level: the track editor's Volume knob too"],
        Id::Decay => setting_help("decay")?,
        Id::KnobBar(key)
        | Id::Fx(key)
        | Id::Generation(key)
        | Id::Compose(key)
        | Id::Loop(key)
        | Id::TrackSelect(key)
        | Id::Toggle(key)
        | Id::Row(key)
        | Id::NoteKnob(key)
        | Id::NoteSlider(key)
        | Id::NoteSelect(key) => setting_help(key)?,
        _ => return None,
    })
}

fn tab_help(tab: u8) -> Option<Help> {
    Some(match tab {
        0 => ["Effects", "reverb, delay, saturation, filter"],
        1 => ["Sound", "the sample or synth the track plays"],
        2 => ["LFO", "a setting moved up and down as it plays"],
        3 => ["Loop", "steps per beat, loop length, swing"],
        4 => ["Generation", "auto mode and how the line is written"],
        _ => return None,
    })
}

fn fx_help(fx: u8) -> Option<Help> {
    Some(match fx {
        0 => ["Reverb", "the room around the sound (the LED: on / off)"],
        1 => ["Delay", "echoes of the sound (the LED: on / off)"],
        2 => ["Saturation", "drive and grit (the LED: on / off)"],
        3 => ["Filter", "cuts lows or highs (the LED: on / off)"],
        _ => return None,
    })
}

fn macro_help(key: &str) -> Option<Help> {
    use ordrumbox_core::style::{COMPLEXITY, ENERGY, VARIATION};
    Some(match key {
        ENERGY => ["Energy", "hits and densities of every track, 50: as written"],
        COMPLEXITY => ["Complexity", "complexity and syncopation of the drums, 50: as written"],
        VARIATION => ["Variation", "how much lines and play vary, 50: as written"],
        _ => return None,
    })
}

fn view_help(view: u8) -> Option<Help> {
    Some(match view {
        0 => ["Grid (F5)", "the step grid"],
        1 => ["Piano roll (F4)", "the focused track's notes by pitch"],
        2 => ["Song (F3)", "patterns arranged into a song"],
        3 => ["Synth (F6)", "the soft-synth presets"],
        4 => ["Kit editor (Ctrl+K)", "the kit's samples and their settings"],
        _ => return None,
    })
}

/// A track or note setting, by its key in the library file.
fn setting_help(key: &str) -> Option<Help> {
    Some(match key {
        // Knob bar.
        "volume" => ["Volume", "the track's level: the slider by its name in the grid too"],
        "pan" => ["Pan", "left (-1) to right (+1)"],
        "transpose" => ["Transpose", "the track's pitch, in semitones"],
        "decay" => ["Decay", "how long the sample rings, in ms (the sample's, every track)"],
        // Effects.
        "reverbMix" | "delayMix" | "saturationMix" => ["Mix", "how much of the effect is heard"],
        "reverbType" => ["Reverb type", "the kind of room"],
        "delayBeats" => ["Delay time", "the time between echoes, in beats"],
        "delayFeedback" => ["Feedback", "how much of each echo comes back"],
        "delayType" => ["Delay type", "slap: one echo; tape; pingpong: left and right"],
        "saturationType" => ["Saturation type", "soft, hard or tape drive"],
        "filterType" => ["Filter type", "lowpass, highpass, bandpass, notch"],
        "filterSlope" => ["Filter slope", "how steeply it cuts, dB per octave"],
        "filterCutoff" => ["Cutoff", "where the filter cuts, in Hz"],
        "filterQ" => ["Resonance (Q)", "how much the cutoff rings"],
        // Sound.
        "mono" => ["Mono", "one note at a time: a new one cuts the last"],
        // Loop.
        "stepsPerBeat" => ["Steps per beat", "the grid of the track: 4 is 16ths, 3 triplets"],
        "loopLength" => ["Loop length", "the track loops after this many steps"],
        "swingAmount" => ["Swing", "delays every other step"],
        // Generation, simple view and manual.
        "autoMode" => ["Auto mode (Z)", "the engine writes and varies the line as it plays"],
        "composeLock" => ["Lock", "the generation leaves this track as it is"],
        "hitVariation" | "autoHitVariation" => ["Hit variation", "how much the hits change from loop to loop"],
        "noteVariation" | "autoNoteVariation" => ["Note variation", "how much the notes change from loop to loop"],
        "probability" => ["Probability", "the chance a note plays, 0 to 1"],
        // Groove.
        "autoPitchChance" => ["Pitch chance", "a hit changes pitch"],
        "autoVelocityChance" => ["Velocity chance", "a hit gets louder or softer"],
        "autoSilenceChance" => ["Silence chance", "a hit is left out"],
        "autoFillChance" => ["Fill chance", "a hit is added between the others"],
        "autoGhostChance" => ["Ghost chance", "a soft ghost note is added"],
        "pitchRange" => ["Pitch range", "how far a pitch change goes, in semitones"],
        "pitchScaleLock" => ["Scale lock", "pitch changes stay in the scale"],
        // Engine.
        "autoRetriggerChance" => ["Retrigger chance", "a note gets repeats"],
        "autoEuclideanChance" => ["Euclidean chance", "a note gets evenly spread hits"],
        "autoNoteProbabilityChance" => ["Note probability chance", "a note gets a chance to play"],
        "autoNoteProbabilityRange" => ["Probability depth", "how low that chance can go"],
        "autoArpChance" => ["Arpeggio chance", "a note becomes an arpeggio"],
        "autoVariant" => ["Variant", "the kind of bass, chord or melody line"],
        "autoDensity" => ["Density", "fewer (-) or more (+) notes; auto: the engine picks"],
        // Compose.
        "composeHitsPerMeasure" => ["Hits per measure", "how many hits a measure gets"],
        "composeComplexity" => ["Complexity", "hits moved toward the weak steps"],
        "composeSyncopation" => ["Syncopation", "hits off the beat"],
        "composeCoupling" => ["Coupling", "avoids (-) or follows (+) the style's partner part"],
        "composeVariation" => ["Variation", "hits moved from loop to loop in auto mode"],
        "composeStyleWeight" => ["Style weight", "the style's typical steps against the plain meter"],
        "composeSeed" => ["Seed", "the same seed writes the same line; 0: new each time"],
        // Note.
        "velocity" => ["Velocity", "how loud the note is; from 90%: an accent"],
        "pitch" => ["Pitch", "the note's transposition, in semitones"],
        "cycle" => ["Cycle", "the note plays once every N loops"],
        "cycleOffset" => ["Offset", "on which loop of its cycle the note plays"],
        "retriggerCount" => ["Retriggers", "the note and its repeats, in hits"],
        "retriggerRate" => ["Rate", "the gap between repeats: 1-7 eighths of a step, 8 and up: steps"],
        "retriggerProbability" => ["Probability", "the chance of each repeat or arpeggio note"],
        "euclideanPulses" => ["Pulses", "hits spread evenly up to the next note (0: off)"],
        "euclideanRotation" => ["Rotation", "the spread hits shifted, in steps"],
        "arpScale" => ["Arpeggio scale", "the scale its notes come from"],
        "arpDirection" => ["Direction", "up, down, up and down, or random"],
        "arpRange" => ["Range", "how many scale notes the arpeggio plays (0: off)"],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editors::{
        FX_KNOBS, GENERATION_COMPOSE, GENERATION_ENGINE, GENERATION_GROOVE, GENERATION_MANUAL, GenerationRow, KNOB_BAR,
        LOOP_PROPS, NOTE_GROUPS, NOTE_TABS, NoteRow, TABS,
    };

    #[test]
    fn every_setting_of_the_editors_and_every_tab_has_a_tooltip() {
        let mut keys: Vec<&str> = KNOB_BAR.iter().map(|(p, _)| p.key).collect();
        keys.extend(
            FX_KNOBS.iter().chain(&GENERATION_MANUAL).chain(&GENERATION_COMPOSE).chain(&LOOP_PROPS).map(|p| p.key),
        );
        for row in GENERATION_GROOVE.iter().chain(&GENERATION_ENGINE) {
            keys.push(match row {
                GenerationRow::Slider(p) => p.key,
                GenerationRow::Toggle(key, _) | GenerationRow::Select(key, _) => key,
            });
        }
        for row in NOTE_GROUPS.iter().flat_map(|g| g.iter()) {
            keys.push(match row {
                NoteRow::Slider(p) => p.key,
                NoteRow::Select(key, _) => key,
            });
        }
        keys.extend(["mono", "autoMode", "composeLock", "delayBeats", "filterSlope", "reverbType", "delayType"]);
        for key in keys {
            assert!(help(Id::Fx(key)).is_some(), "{key}");
            assert!(help(Id::Row(key)).is_some(), "the label of {key}");
        }
        for tab in 0..TABS.len() as u8 {
            assert!(help(Id::Tab(tab)).is_some(), "tab {tab}");
        }
        for tab in 0..NOTE_TABS.len() as u8 {
            assert!(help(Id::NoteTab(tab)).is_some(), "note tab {tab}");
        }
        for fx in 0..4 {
            assert!(help(Id::FxTab(fx)).is_some(), "effect {fx}");
        }
        for view in 0..crate::view::VIEWS.len() as u8 {
            assert!(help(Id::View(view)).is_some(), "view {view}");
        }
        for (key, _) in crate::view::MACROS {
            assert!(help(Id::Macro(key)).is_some(), "{key}");
        }
        assert_eq!(help(Id::NoteTab(1)).map(|h| h[0]), Some("Retrigger"));
        assert_eq!(help(Id::Fx("transpose")).map(|h| h[0]), Some("Transpose"));
    }
}
