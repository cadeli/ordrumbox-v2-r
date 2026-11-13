//! The sound output the program gives the session: the default device in the
//! app, the plugin host later. The session renders nothing itself: it drives
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

/// Lists the sound devices the program can play on, by name.
pub type OutputDevices = fn() -> Vec<String>;

/// The `settings.json` key of the output device chosen (a name; absent: the system's default).
pub const DEVICE_KEY: &str = "audioDevice";

/// The output device chosen in File > Audio output, by name; `None`: the
/// system's default. The program reads it each time it starts the sound.
pub fn chosen_device() -> Result<Option<String>, String> {
    let Some(path) = crate::store::settings_path() else { return Ok(None) };
    let value = crate::store::load_setting(&path, DEVICE_KEY)?;
    Ok(value.and_then(|v| v.as_str().map(str::to_owned)).filter(|name| !name.is_empty()))
}

/// The output of a plugin: its host plays what the engine renders.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostOutput {
    pub sample_rate: u32,
}

impl AudioOutput for HostOutput {
    fn device(&self) -> &str {
        "host"
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn take_error(&self) -> Option<String> {
        None
    }
}

/// A [`StartOutput`] for a plugin at the host's `sample_rate`: each start
/// builds the engine's controller for the session and sends its
/// [`AudioSide`] on the channel returned, for the plugin to take before
/// processing (never on the audio thread: an old side is dropped there).
pub fn host_output(sample_rate: f32) -> (StartOutput, Receiver<AudioSide>) {
    let (sender, receiver) = mpsc::channel();
    let start: StartOutput = Box::new(move |bank, scales, synths| {
        let (controller, audio) = Controller::new(sample_rate, bank, scales, synths);
        sender.send(audio).map_err(|_| "the plugin no longer takes its audio side".to_owned())?;
        Ok((Box::new(HostOutput { sample_rate: sample_rate as u32 }) as Box<dyn AudioOutput>, controller))
    });
    (start, receiver)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_output_hands_over_the_audio_side() {
        let (mut start, sides) = host_output(48_000.0);
        let (output, _controller) =
            start(Arc::new(SampleBank::default()), Scales::default(), SynthPresets::default()).unwrap();
        assert_eq!((output.device(), output.sample_rate(), output.take_error()), ("host", 48_000, None));
        assert!(sides.try_recv().is_ok());
        drop(sides);
        assert!(start(Arc::new(SampleBank::default()), Scales::default(), SynthPresets::default()).is_err());
    }
}
