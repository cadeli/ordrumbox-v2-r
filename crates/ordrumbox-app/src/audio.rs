//! The audio output: the default device through cpal (WASAPI on Windows,
//! CoreAudio on macOS), fed by the engine's [`AudioSide`].

use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};
use ordrumbox_core::Scales;
use ordrumbox_engine::{AudioSide, Controller, SampleBank, SynthPresets};

/// Frames rendered per engine call inside a callback (scratch size).
const SCRATCH_FRAMES: usize = 4096;

/// A running output stream; dropping it stops the sound.
pub struct Output {
    _stream: cpal::Stream,
    pub device: String,
    pub sample_rate: u32,
}

impl std::fmt::Debug for Output {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Output").field("device", &self.device).field("sample_rate", &self.sample_rate).finish()
    }
}

/// Open the default output device and start rendering.
pub fn start(bank: Arc<SampleBank>, scales: Scales, synths: SynthPresets) -> Result<(Output, Controller), String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no audio output device")?;
    let name = device.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "default".into());
    let supported = device.default_output_config().map_err(|e| format!("{name}: {e}"))?;
    let config: StreamConfig = supported.config();
    let (controller, audio) = Controller::new(config.sample_rate as f32, bank, scales, synths);

    let stream = match supported.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, &config, audio),
        SampleFormat::I16 => build::<i16>(&device, &config, audio),
        SampleFormat::I32 => build::<i32>(&device, &config, audio),
        SampleFormat::U16 => build::<u16>(&device, &config, audio),
        other => return Err(format!("{name}: unsupported sample format {other}")),
    }
    .map_err(|e| format!("{name}: {e}"))?;
    stream.play().map_err(|e| format!("{name}: {e}"))?;
    Ok((Output { _stream: stream, device: name, sample_rate: config.sample_rate }, controller))
}

fn build<T>(device: &cpal::Device, config: &StreamConfig, mut audio: AudioSide) -> Result<cpal::Stream, cpal::Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    // Allocated here, before the stream starts: the callback never allocates.
    let mut left = vec![0.0f32; SCRATCH_FRAMES];
    let mut right = vec![0.0f32; SCRATCH_FRAMES];
    device.build_output_stream(
        *config,
        move |data: &mut [T], _| {
            for chunk in data.chunks_mut(SCRATCH_FRAMES * channels) {
                let frames = chunk.len() / channels;
                audio.render(&mut left[..frames], &mut right[..frames]);
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
        |err| eprintln!("audio stream: {err}"),
        None,
    )
}
