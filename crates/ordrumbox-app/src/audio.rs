//! The audio output: the default device through cpal (WASAPI on Windows,
//! CoreAudio on macOS), fed by the engine's [`AudioSide`]. The session gets it
//! as an [`AudioOutput`].

use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use ordrumbox_core::Scales;
use ordrumbox_engine::{AudioSide, Controller, HostTransport, SampleBank, SynthPresets};
use ordrumbox_ui::{AudioOutput, StartedOutput};

/// Frames rendered per engine call inside a callback (scratch size).
const SCRATCH_FRAMES: usize = 4096;

/// A running output stream; dropping it stops the sound.
struct Output {
    _stream: cpal::Stream,
    device: String,
    sample_rate: u32,
    /// The last stream error (a device unplugged…), until the UI takes it.
    error: Arc<Mutex<Option<String>>>,
}

impl AudioOutput for Output {
    fn device(&self) -> &str {
        &self.device
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn take_error(&self) -> Option<String> {
        self.error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take()
    }
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").field("device", &self.device).field("sample_rate", &self.sample_rate).finish()
    }
}

fn name_of(device: &cpal::Device) -> String {
    device.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "unnamed device".into())
}

/// The names of the output devices, for File > Audio output.
pub fn devices() -> Vec<String> {
    // A host that cannot list its devices offers none: the menu then holds the default alone.
    cpal::default_host().output_devices().map(|devices| devices.map(|d| name_of(&d)).collect()).unwrap_or_default()
}

/// Open the output device chosen in File > Audio output, else the default
/// one, and start rendering. A device chosen that is gone (unplugged) gives
/// way to the default one, and the name shown says so.
pub fn start(bank: Arc<SampleBank>, scales: Scales, synths: SynthPresets) -> StartedOutput {
    let host = cpal::default_host();
    let wanted = ordrumbox_ui::chosen_device()?;
    let chosen =
        wanted.as_deref().and_then(|wanted| host.output_devices().ok()?.find(|device| name_of(device) == wanted));
    let missing = wanted.as_deref().filter(|_| chosen.is_none());
    let device = chosen.or_else(|| host.default_output_device()).ok_or("no audio output device")?;
    let name = match missing {
        Some(missing) => format!("{} (\"{missing}\" not found)", name_of(&device)),
        None => name_of(&device),
    };
    let supported = device.default_output_config().map_err(|e| format!("{name}: {e}"))?;
    let config: StreamConfig = supported.config();
    let (controller, audio) = Controller::new(config.sample_rate as f32, bank, scales, synths);

    let error = Arc::new(Mutex::new(None));
    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, audio, error.clone()),
        SampleFormat::I16 => build::<i16>(&device, &config, audio, error.clone()),
        SampleFormat::I32 => build::<i32>(&device, &config, audio, error.clone()),
        SampleFormat::U16 => build::<u16>(&device, &config, audio, error.clone()),
        other => return Err(format!("{name}: unsupported sample format {other}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    stream.play().map_err(|e| format!("{name}: {e}"))?;
    Ok((Box::new(Output { _stream: stream, device: name, sample_rate: config.sample_rate, error }), controller))
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut audio: AudioSide,
    error: Arc<Mutex<Option<String>>>,
) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let rate = config.sample_rate;
    // The transport the last block followed, kept when the MIDI thread holds the clock.
    let mut followed = HostTransport { playing: false, bpm: 120.0, beat: 0.0 };
    // Allocated here, before the stream starts: the callback never allocates.
    let mut left = vec![0.0f32; SCRATCH_FRAMES];
    let mut right = vec![0.0f32; SCRATCH_FRAMES];
    device.build_output_stream(
        *config,
        move |data: &mut [T], _| {
            // The notes played on the MIDI keyboard since the last callback.
            while let Some(note) = crate::midi::NOTES.pop() {
                audio.trigger(note.channel, note.key, note.velocity, 0);
            }
            let mut done = 0;
            for chunk in data.chunks_mut(SCRATCH_FRAMES * channels) {
                let frames = chunk.len() / channels;
                if crate::midi_sync::following() {
                    // The MIDI clock's transport, where it is when this chunk plays.
                    let ahead = done as f64 / f64::from(rate);
                    followed = crate::midi_sync::transport(ahead).unwrap_or(followed);
                    audio.render_synced(&mut left[..frames], &mut right[..frames], followed);
                } else {
                    audio.render(&mut left[..frames], &mut right[..frames]);
                }
                crate::midi_out::send_block(&mut audio, done, rate);
                done += frames;
                for (i, frame) in chunk.chunks_mut(channels).enumerate() {
                    match frame {
                        [mono] => *mono = T::from_sample((left[i] + right[i]) * 0.5),
                        [l, r, rest @ ..] => {
                            *l = T::from_sample(left[i]);
                            *r = T::from_sample(right[i]);
                            rest.fill(T::from_sample(0.0));
                        }
                        [] => {}
                    }
                }
            }
        },
        // Called outside the audio callback: it may lock and allocate.
        move |err| *error.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(err.to_string()),
        None,
    )
}
