//! The sound output the program gives the session: the default device in the
//! app, or none in a test that renders itself. The session renders nothing itself: it drives
//! the [`Controller`] the output was started with and shows its errors.

use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use ordrumbox_core::Scales;
use ordrumbox_engine::{AudioSide, Controller, SampleBank, SynthPresets};

/// A running sound output; dropping it stops the sound.
pub trait AudioOutput: std::fmt::Debug + Send {
    /// The name of the device it plays on.
    fn device(&self) -> &str;
    /// Frames per second it plays.
    fn sample_rate(&self) -> u32;
    /// The output error reported since the last call, if any.
    fn take_error(&self) -> Option<String>;
}

/// What starts the output, given what the engine plays from: the output and
/// the controller of its engine, or why there is no sound.
pub type StartedOutput = Result<(Box<dyn AudioOutput>, Controller), String>;

/// Starts the output for a bank, scales and synth presets: at
/// `Session::load`, and again when a factory reset reloads the session.
pub type StartOutput = Box<dyn FnMut(Arc<SampleBank>, Scales, SynthPresets) -> StartedOutput + Send>;

/// Lists the sound devices (or MIDI ports) the program can use, by name, or
/// says why it cannot.
pub type OutputDevices = fn() -> Result<Vec<String>, String>;

/// The `settings.json` key of the output device chosen (a name; absent: the system's default).
pub const DEVICE_KEY: &str = "audioDevice";

/// The output device chosen in File > Audio output, by name; `None`: the
/// system's default. The program reads it each time it starts the sound.
pub fn chosen_device() -> Result<Option<String>, String> {
    let Some(path) = crate::store::settings_path() else { return Ok(None) };
    let value = crate::store::load_setting(&path, DEVICE_KEY)?;
    Ok(value.and_then(|v| v.as_str().map(str::to_owned)).filter(|name| !name.is_empty()))
}

/// An output with no device: whoever takes the [`AudioSide`] renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HandedOutput {
    pub sample_rate: u32,
}

impl AudioOutput for HandedOutput {
    fn device(&self) -> &str {
        "none"
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn take_error(&self) -> Option<String> {
        None
    }
}

/// A [`StartOutput`] at `sample_rate` with no device (a test, a tool):
/// each start builds the engine's controller for the session and sends its
/// [`AudioSide`] on the channel returned, for the one that renders to take.
pub fn handed_output(sample_rate: f32) -> (StartOutput, Receiver<AudioSide>) {
    let (sender, receiver) = mpsc::channel();
    let start: StartOutput = Box::new(move |bank, scales, synths| {
        let (controller, audio) = Controller::new(sample_rate, bank, scales, synths);
        sender.send(audio).map_err(|_| "no one takes the audio side any more".to_owned())?;
        Ok((Box::new(HandedOutput { sample_rate: sample_rate as u32 }) as Box<dyn AudioOutput>, controller))
    });
    (start, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handed_output_hands_over_the_audio_side() {
        let (mut start, sides) = handed_output(48_000.0);
        let (output, _controller) =
            start(Arc::new(SampleBank::default()), Scales::default(), SynthPresets::default()).unwrap();
        assert_eq!((output.device(), output.sample_rate(), output.take_error()), ("none", 48_000, None));
        assert!(sides.try_recv().is_ok());
        drop(sides);
        assert!(start(Arc::new(SampleBank::default()), Scales::default(), SynthPresets::default()).is_err());
    }
}
