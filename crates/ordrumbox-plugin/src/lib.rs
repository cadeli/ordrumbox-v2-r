//! orDrumbox as a CLAP (and, with the `vst3` feature, VST3) instrument: no
//! audio input, a stereo output, playback that follows the host's transport
//! (play and stop, position, tempo), the library saved in the host's
//! project, and parameters a host automates (`params.rs`).
//!
//! MIDI: the notes the pattern plays go out where a note in plays them:
//! drums on channel 10 at their GM key, the other tracks each on a channel
//! of its own at the note's pitch (`ordrumbox_engine::midi`).
//!
//! Threads: the host's audio thread runs [`AudioSide::render_synced`]; the
//! player thread ([`player::Ticker`]) builds and posts passes; the host's GUI
//! thread runs the editor. The audio thread never touches the player's lock.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nih_plug::prelude::*;
use ordrumbox_engine::{AudioSide, BlockNote, HostTransport};

mod editor;
pub mod lockstep;
pub mod params;
pub mod player;
pub mod state;

use lockstep::{Pace, Steps};
use params::OrdrumboxParams;
use player::{SharedPlayer, Started, Ticker, lock};

/// The tempo used until the host gives one.
const DEFAULT_BPM: f64 = 120.0;

/// The plugin instance the host holds.
#[derive(Debug)]
pub struct Ordrumbox {
    params: Arc<OrdrumboxParams>,
    player: SharedPlayer,
    /// Made in `initialize`, at the host's sample rate.
    audio: Option<Box<AudioSide>>,
    ticker: Option<Ticker>,
    /// The audio thread's handshake with the ticker, for a host ahead of real time.
    steps: Arc<Steps>,
    /// How far ahead of real time the host renders.
    pace: Pace,
    /// The host's last tempo (a host may leave it out of a block).
    bpm: f64,
}

impl Default for Ordrumbox {
    fn default() -> Self {
        Self {
            params: Arc::new(OrdrumboxParams::default()),
            player: SharedPlayer::default(),
            audio: None,
            ticker: None,
            steps: Arc::default(),
            pace: Pace::new(false),
            bpm: DEFAULT_BPM,
        }
    }
}

/// What the host's transport tells for the block starting now: whether it
/// plays, its tempo and its position in beats, if it gives them.
fn host_transport(playing: bool, tempo: Option<f64>, beat: Option<f64>, last_bpm: f64) -> HostTransport {
    let bpm = tempo.filter(|bpm| bpm.is_finite() && *bpm > 0.0).unwrap_or(last_bpm);
    match beat {
        Some(beat) => HostTransport { playing, bpm, beat },
        // Nowhere to follow: as if stopped.
        None => HostTransport { playing: false, bpm, beat: 0.0 },
    }
}

/// Before a block of `frames` frames: when the host renders ahead of real
/// time, the player thread ticks for this block first, so that its passes,
/// pattern and kit are there however fast the host goes.
fn ready(audio: &mut AudioSide, pace: &mut Pace, steps: &Steps, host: HostTransport, frames: usize) {
    let length = Duration::from_secs_f64(frames as f64 / f64::from(audio.sample_rate()));
    if let Some(budget) = pace.budget(Instant::now(), length) {
        audio.announce(host);
        if steps.tick(budget) {
            audio.receive(host);
        }
    }
}

/// A note the engine sent, as the host takes it.
fn note_event(note: BlockNote) -> NoteEvent<()> {
    let BlockNote { timing, channel, key, velocity } = note;
    match velocity {
        Some(velocity) => NoteEvent::NoteOn { timing, voice_id: None, channel, note: key, velocity },
        None => NoteEvent::NoteOff { timing, voice_id: None, channel, note: key, velocity: 0.0 },
    }
}

impl Plugin for Ordrumbox {
    const NAME: &'static str = "orDrumbox";
    const VENDOR: &'static str = "orDrumbox";
    const URL: &'static str = env!("CARGO_PKG_REPOSITORY");
    const EMAIL: &'static str = "";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const MIDI_OUTPUT: MidiConfig = MidiConfig::Basic;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn editor(&mut self, _async_executor: AsyncExecutor<Self>) -> Option<Box<dyn Editor>> {
        Some(Box::new(editor::OrdrumboxEditor { player: self.player.clone(), params: self.params.clone() }))
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        // On the host's main thread: the engine made here, and the one it
        // replaces dropped here, allocate and free off the audio thread.
        // nih-plug initializes again after the host restores a project: the
        // session is then loaded from it.
        if let Started::New(audio) = lock(&self.player).start(buffer_config.sample_rate, &self.params.library) {
            self.audio = audio;
        }
        // nih-plug may keep the mode it had before a host's change: a host
        // ahead of real time is also seen by its pace.
        self.pace = Pace::new(buffer_config.process_mode == ProcessMode::Offline);
        if self.ticker.is_none() {
            match Ticker::spawn(self.player.clone(), self.params.clone(), self.steps.clone()) {
                Ok(ticker) => self.ticker = Some(ticker),
                Err(e) => {
                    nih_log!("orDrumbox: cannot start the player thread: {e}");
                    return false;
                }
            }
        }
        true
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let transport = context.transport();
        let host = host_transport(transport.playing, transport.tempo, transport.pos_beats(), self.bpm);
        self.bpm = host.bpm;
        match (&mut self.audio, buffer.as_slice()) {
            (Some(audio), [left, right, ..]) => {
                ready(audio, &mut self.pace, &self.steps, host, left.len());
                audio.set_host_mix(self.params.mix());
                while let Some(event) = context.next_event() {
                    if let NoteEvent::NoteOn { timing, channel, note, velocity, .. } = event {
                        audio.trigger(channel, note, velocity, timing);
                    }
                }
                audio.render_synced(left, right, host);
                audio.take_notes(|note| context.send_event(note_event(note)));
            }
            (_, channels) => channels.iter_mut().for_each(|c| c.fill(0.0)),
        }
        // A sequencer sounds with no input: the host must keep calling.
        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for Ordrumbox {
    const CLAP_ID: &'static str = "io.github.cadeli.ordrumbox";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Drum machine and step sequencer");
    const CLAP_MANUAL_URL: Option<&'static str> = Some(Self::URL);
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::DrumMachine, ClapFeature::Synthesizer, ClapFeature::Stereo];
}

#[cfg(feature = "vst3")]
impl Vst3Plugin for Ordrumbox {
    const VST3_CLASS_ID: [u8; 16] = *b"orDrumboxPlugin1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Drum, Vst3SubCategory::Stereo];
}

// The entry points a host loads, written by nih-plug's macros. Their `unsafe`
// comes from another crate's macro, which the `unsafe_code` lint does not see.
mod clap_export {
    nih_plug::nih_export_clap!(crate::Ordrumbox);
}

#[cfg(feature = "vst3")]
#[expect(semicolon_in_expressions_from_non_local_macros, reason = "in vst3-com's `vtable` macro")]
mod vst3_export {
    nih_plug::nih_export_vst3!(crate::Ordrumbox);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_transport_keeps_the_last_tempo_and_stops_without_a_position() {
        let host = host_transport(true, Some(90.0), Some(3.5), DEFAULT_BPM);
        assert_eq!(host, HostTransport { playing: true, bpm: 90.0, beat: 3.5 });
        assert_eq!(host_transport(true, None, Some(4.0), 90.0).bpm, 90.0);
        let host = host_transport(true, Some(0.0), None, 90.0);
        assert_eq!(host, HostTransport { playing: false, bpm: 90.0, beat: 0.0 });
    }

    #[test]
    fn a_bounce_plays_every_note_however_fast_the_host_goes() {
        use nih_plug::params::persist::PersistentField;
        use player::Player;
        use state::LibraryText;

        let params = Arc::new(params::OrdrumboxParams::default());
        let kick = r#"{"name":"KICK","notes":[{},{"beat":1},{"beat":2},{"beat":3}]}"#;
        params.library.set(LibraryText(format!(r#"{{"patterns":[{{"name":"p","tracks":[{kick}]}}]}}"#)));
        let player = Arc::new(std::sync::Mutex::new(Player::default()));
        let Started::New(Some(mut audio)) = lock(&player).start(48_000.0, &params.library) else { panic!("no sound") };
        let steps = Arc::new(Steps::default());
        let _ticker = Ticker::spawn(player.clone(), params.clone(), steps.clone()).unwrap();

        // Offline, back to back: 3 passes of 4 beats at 120 bpm (24000 frames a beat).
        let mut pace = Pace::new(true);
        let (frames, beat_frames) = (512_u32, 24_000_u32);
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        let mut sent = Vec::new();
        for block in 0..12 * beat_frames / frames {
            let at = block * frames;
            let host = HostTransport { playing: true, bpm: 120.0, beat: f64::from(at) / f64::from(beat_frames) };
            ready(&mut audio, &mut pace, &steps, host, 512);
            audio.render_synced(&mut left, &mut right, host);
            audio.take_notes(|n| {
                if n.velocity.is_some() {
                    sent.push(at + n.timing);
                }
            });
        }
        let beats: Vec<u32> = (0..12).map(|b| b * beat_frames).collect();
        assert_eq!(sent, beats, "every beat, from the first frame, once");
    }
}
