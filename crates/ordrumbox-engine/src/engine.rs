//! The audio-thread side: plays passes, sample-accurately, through one strip
//! per track and the master bus, into a stereo buffer.
//!
//! Like a browser's audio graph, it renders in quanta of 128 frames: strip
//! and master parameters (and LFO values) are read once per quantum. Voices
//! start on their exact frame (a quantum is split at every event).
//!
//! `render` never allocates, locks or blocks; the passes and the bank it drops
//! are never their last reference (the [`crate::Controller`] keeps one), so it
//! never frees memory either.

use std::sync::Arc;

use ordrumbox_dsp::{MasterBus, MasterSettings, SampleVoice, Strip, StripSettings, SynthVoice};

use crate::kit::SampleBank;
use crate::pass::{Event, Pass, Sound};

/// Sample voices playing at once; the oldest is cut when a new one needs a slot.
pub const MAX_VOICES: usize = 64;
/// Synth voices playing at once (the JS synth voice pool size).
pub const MAX_SYNTH_VOICES: usize = 16;
/// Tracks with their own strip; notes of tracks beyond are dropped.
pub const MAX_TRACKS: usize = 64;
/// Frames per render quantum (Web Audio's).
pub const QUANTUM: usize = 128;
/// Time constants of parameter changes (`setTargetAtTime`), seconds: effect
/// settings (`RAMP_TIME`), volume and pan (`applyTrackToStrip`), LFO steps
/// (`LFO_SMOOTHING`).
const RAMP_FX: f32 = 0.02;
const RAMP_MIX: f32 = 0.01;
const RAMP_LFO: f32 = 0.005;
/// A strip with no input and an output under this level...
const IDLE_LEVEL: f32 = 1e-6;
/// ...for this long (longer than the longest delay) is not processed until
/// its input comes back.
const IDLE_SECONDS: f32 = 2.5;

/// Where playback is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// Pass index since play started.
    pub pass: u64,
    /// Frame within the pass.
    pub frame: u64,
    /// Length of the pass, in frames.
    pub frames: u64,
}

/// One track's strip and its parameter state.
#[derive(Debug, Clone)]
struct Channel {
    strip: Strip,
    /// Values in use, moving toward `target`.
    current: StripSettings,
    target: StripSettings,
    /// Volume, pan, cutoff, q driven by an LFO (faster ramp).
    lfo: [bool; 4],
    /// Quanta in a row with no input and a quiet output.
    idle: u32,
}

#[derive(Debug)]
pub struct Engine {
    sample_rate: f32,
    bank: Arc<SampleBank>,
    current: Option<Arc<Pass>>,
    next: Option<Arc<Pass>>,
    /// Index of the pass being played (`current` is replayed when the next is late).
    pass_number: u64,
    frame: u64,
    /// Next event of `current` to start.
    cursor: usize,
    /// Frames rendered since creation (the synth voices' clock).
    clock: u64,
    voices: Vec<SampleVoice>,
    synths: Vec<SynthVoice>,
    serial: u64,
    channels: Vec<Channel>,
    /// Per-track peak of the last rendered quantum (0–1), for VU meters.
    peaks: Vec<f32>,
    /// Strips in use (the most tracks any pass had).
    strip_count: usize,
    input_left: Vec<f32>,
    input_right: Vec<f32>,
    master: MasterBus,
    master_current: MasterSettings,
    master_target: MasterSettings,
    /// `exp(-quantum / τ)` for each ramp.
    keep: [f32; 3],
    idle_quanta: u32,
}

impl Engine {
    /// Allocates everything the audio thread will ever need.
    pub fn new(sample_rate: f32, bank: Arc<SampleBank>) -> Self {
        let channel = Channel {
            strip: Strip::new(sample_rate),
            current: StripSettings::default(),
            target: StripSettings::default(),
            lfo: [false; 4],
            idle: 0,
        };
        let quantum_seconds = QUANTUM as f32 / sample_rate;
        Self {
            sample_rate,
            bank,
            current: None,
            next: None,
            pass_number: 0,
            frame: 0,
            cursor: 0,
            clock: 0,
            voices: vec![SampleVoice::default(); MAX_VOICES],
            synths: vec![SynthVoice::default(); MAX_SYNTH_VOICES],
            serial: 0,
            channels: vec![channel; MAX_TRACKS],
            peaks: vec![0.0; MAX_TRACKS],
            strip_count: 0,
            input_left: vec![0.0; MAX_TRACKS * QUANTUM],
            input_right: vec![0.0; MAX_TRACKS * QUANTUM],
            master: MasterBus::new(sample_rate),
            master_current: MasterSettings::default(),
            master_target: MasterSettings::default(),
            keep: [RAMP_FX, RAMP_MIX, RAMP_LFO].map(|tau| (-quantum_seconds / tau).exp()),
            idle_quanta: (IDLE_SECONDS / quantum_seconds).ceil() as u32,
        }
    }

    pub fn sample_rate(&self) -> f32 {
        self.sample_rate
    }

    /// Replace the sample bank. Voices playing an index the new bank lacks stop.
    /// The caller keeps a reference to the old bank (dropping the last one
    /// here would free memory on the audio thread).
    pub fn set_bank(&mut self, bank: Arc<SampleBank>) {
        self.bank = bank;
    }

    /// Master bus settings (reached through a 20 ms ramp).
    pub fn set_master(&mut self, settings: MasterSettings) {
        self.master_target = settings;
    }

    pub fn master(&self) -> MasterSettings {
        self.master_target
    }

    /// Master volume, 0–2.
    pub fn set_volume(&mut self, volume: f32) {
        self.master_target.master = volume;
    }

    /// Start playing `pass` from its first frame; its strip settings apply at once.
    pub fn play(&mut self, pass: Arc<Pass>) {
        self.pass_number = pass.number;
        self.load(pass);
        self.next = None;
        for channel in &mut self.channels[..self.strip_count] {
            channel.current = channel.target;
        }
    }

    /// Start the events of `pass` now, outside the transport (auditioning a
    /// track or a note). When stopped, its strip settings apply at once.
    /// The caller keeps a reference to `pass` (this drops its own).
    pub fn preview(&mut self, pass: Arc<Pass>) {
        if self.current.is_none() {
            self.strip_count = self.strip_count.max(pass.strips.len()).min(MAX_TRACKS);
            for (channel, settings) in self.channels.iter_mut().zip(&pass.strips) {
                channel.target = *settings;
                channel.current = *settings;
                channel.lfo = [false; 4];
            }
        }
        for event in &pass.events {
            self.start(event, &pass, self.clock);
        }
    }

    /// Swap the playing pass for `pass`, a rebuild of the same index (the
    /// pattern was edited): playback goes on from the same frame with the new
    /// events still ahead; the queued pass, built from the old pattern, is
    /// dropped so the next one is requested again.
    pub fn replace(&mut self, pass: Arc<Pass>) {
        if self.current.as_ref().is_none_or(|c| c.number != pass.number) || pass.number != self.pass_number {
            return;
        }
        let frame = self.frame;
        self.next = None;
        self.load(pass);
        self.frame = frame;
        if let Some(current) = &self.current {
            self.cursor = current.events.partition_point(|e| e.frame < frame);
        }
    }

    /// Pass to play after the current one, if it is the next index.
    pub fn queue(&mut self, pass: Arc<Pass>) {
        if self.current.is_some() && pass.number == self.pass_number + 1 {
            self.next = Some(pass);
        }
    }

    /// Stop the transport; playing voices are released, effect tails ring out.
    pub fn stop(&mut self) {
        self.current = None;
        self.next = None;
        for voice in &mut self.voices {
            voice.release(self.sample_rate);
        }
        let now = self.seconds(self.clock);
        for synth in &mut self.synths {
            synth.release_at(now);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.current.is_some()
    }

    pub fn position(&self) -> Option<Position> {
        let pass = self.current.as_ref()?;
        Some(Position { pass: self.pass_number, frame: self.frame, frames: pass.frames })
    }

    /// Index of the pass the engine needs next, if it has none queued.
    pub fn wants_pass(&self) -> Option<u64> {
        (self.current.is_some() && self.next.is_none()).then_some(self.pass_number + 1)
    }

    /// Per-track peak of the last rendered quantum (0–1), for VU meters.
    pub fn peaks(&self) -> &[f32] {
        &self.peaks
    }

    fn seconds(&self, clock: u64) -> f64 {
        clock as f64 / f64::from(self.sample_rate)
    }

    fn load(&mut self, pass: Arc<Pass>) {
        self.strip_count = self.strip_count.max(pass.strips.len()).min(MAX_TRACKS);
        for (channel, settings) in self.channels.iter_mut().zip(&pass.strips) {
            channel.target = *settings;
            channel.lfo = [false; 4];
        }
        for lfo in pass.lfos.iter().filter(|l| l.track < MAX_TRACKS) {
            self.channels[lfo.track].lfo =
                [!lfo.volume.is_empty(), !lfo.pan.is_empty(), !lfo.cutoff.is_empty(), !lfo.q.is_empty()];
        }
        self.frame = 0;
        self.cursor = 0;
        self.current = Some(pass);
    }

    /// Render `left.len()` frames (both slices the same length), replacing their content.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(QUANTUM);
            self.render_quantum(&mut left[done..done + n], &mut right[done..done + n]);
            done += n;
        }
    }

    fn render_quantum(&mut self, out_left: &mut [f32], out_right: &mut [f32]) {
        let n = out_left.len();
        let strips = self.strip_count;
        self.input_left[..strips * QUANTUM].fill(0.0);
        self.input_right[..strips * QUANTUM].fill(0.0);

        // Voices, split at every event and pass end so each starts on its frame.
        let mut offset = 0;
        while offset < n {
            let until = self.dispatch_due_events(offset).map_or(n, |frames_left| (offset + frames_left).min(n));
            if offset == 0 {
                self.apply_lfos();
            }
            self.render_voices(offset, until);
            self.frame += (until - offset) as u64;
            offset = until;
        }

        // Strips into the bus.
        out_left.fill(0.0);
        out_right.fill(0.0);
        let [keep_fx, keep_mix, keep_lfo] = self.keep;
        for (track, channel) in self.channels[..strips].iter_mut().enumerate() {
            let target = channel.target;
            let before = channel.current;
            channel.current.approach(&target, keep_fx);
            let ramp = |from: f32, to: f32, keep: f32| to + (from - to) * keep;
            let [lfo_volume, lfo_pan, lfo_cutoff, lfo_q] = channel.lfo;
            let mix_or_lfo = |lfo: bool| if lfo { keep_lfo } else { keep_mix };
            channel.current.volume = ramp(before.volume, target.volume, mix_or_lfo(lfo_volume));
            channel.current.pan = ramp(before.pan, target.pan, mix_or_lfo(lfo_pan));
            if lfo_cutoff {
                channel.current.cutoff = ramp(before.cutoff, target.cutoff, keep_lfo);
            }
            if lfo_q {
                channel.current.q = ramp(before.q, target.q, keep_lfo);
            }

            let range = track * QUANTUM..track * QUANTUM + n;
            let (in_l, in_r) = (&self.input_left[range.clone()], &self.input_right[range]);
            let silent_input = in_l.iter().chain(in_r).all(|&s| s == 0.0);
            if silent_input && channel.idle > self.idle_quanta {
                self.peaks[track] = 0.0;
                continue;
            }
            // Rendered on the side to measure the strip's own level.
            let (mut strip_l, mut strip_r) = ([0.0_f32; QUANTUM], [0.0_f32; QUANTUM]);
            channel.strip.process(&channel.current, in_l, in_r, &mut strip_l[..n], &mut strip_r[..n]);
            let mut peak = 0.0_f32;
            for (out, s) in out_left.iter_mut().zip(&strip_l[..n]).chain(out_right.iter_mut().zip(&strip_r[..n])) {
                *out += s;
                peak = peak.max(s.abs());
            }
            channel.idle = if silent_input && peak < IDLE_LEVEL { channel.idle.saturating_add(1) } else { 0 };
            self.peaks[track] = peak;
        }

        // Master bus.
        self.master_current.approach(&self.master_target, keep_fx);
        self.master.process(&self.master_current, out_left, out_right);
        self.clock += n as u64;
    }

    /// Start the events due at the current frame (moving to the next pass at
    /// the end of this one); frames until the next event or pass end, `None`
    /// when stopped. `offset` is the position in the quantum.
    fn dispatch_due_events(&mut self, offset: usize) -> Option<usize> {
        loop {
            // Taken out and put back, so no reference count changes here.
            let pass = self.current.take()?;
            if self.frame >= pass.frames {
                self.pass_number += 1;
                match self.next.take() {
                    // `pass` is dropped here, never its last reference (the controller keeps one).
                    Some(next) if next.number == self.pass_number => self.load(next),
                    _ => {
                        // Next pass late: play this one again rather than stop.
                        self.frame = 0;
                        self.cursor = 0;
                        self.current = Some(pass);
                    }
                }
                continue;
            }
            let now = self.clock + offset as u64;
            while let Some(event) = pass.events.get(self.cursor).filter(|e| e.frame <= self.frame) {
                self.start(event, &pass, now);
                self.cursor += 1;
            }
            let next_stop = pass.events.get(self.cursor).map_or(pass.frames, |e| e.frame);
            let frames_left = (next_stop - self.frame) as usize;
            self.current = Some(pass);
            return Some(frames_left);
        }
    }

    /// Set the strip targets from the LFO values of the tick playing.
    fn apply_lfos(&mut self) {
        let Some(pass) = &self.current else { return };
        let tick = pass.tick_at(self.frame);
        for lfo in pass.lfos.iter().filter(|l| l.track < MAX_TRACKS) {
            lfo.apply(tick, &mut self.channels[lfo.track].target);
        }
    }

    fn start(&mut self, event: &Event, pass: &Pass, now: u64) {
        let track = event.sound.track();
        if track >= MAX_TRACKS {
            return;
        }
        let now_seconds = self.seconds(now);
        if event.mono {
            for voice in self.voices.iter_mut().filter(|v| v.is_active() && v.track() == track) {
                voice.release(self.sample_rate);
            }
            for synth in self.synths.iter_mut().filter(|v| v.is_active() && v.track() == track) {
                synth.release_at(now_seconds);
            }
        }
        self.serial += 1;
        match event.sound {
            Sound::Sample(params) => {
                let Some(sample) = self.bank.get(params.sample) else { return };
                let slot = free_or_oldest(&self.voices, SampleVoice::is_active, SampleVoice::serial);
                self.voices[slot].start(&params, &sample.buffer, self.sample_rate, self.serial);
            }
            Sound::Synth(synth) => {
                let Some(patch) = pass.patches.get(synth.patch) else { return };
                let slot = free_or_oldest(&self.synths, SynthVoice::is_active, SynthVoice::serial);
                let voice = &mut self.synths[slot];
                voice.start(patch, track, now_seconds, self.serial);
                voice.release_at(now_seconds + synth.length);
            }
        }
    }

    fn render_voices(&mut self, from: usize, to: usize) {
        if from >= to {
            return;
        }
        for voice in self.voices.iter_mut().filter(|v| v.is_active()) {
            let Some(sample) = self.bank.get(voice.sample()) else {
                *voice = SampleVoice::default();
                continue;
            };
            let base = voice.track() * QUANTUM;
            voice.render(
                &sample.buffer,
                &mut self.input_left[base + from..base + to],
                &mut self.input_right[base + from..base + to],
            );
        }
        let first_frame = self.clock + from as u64;
        for synth in self.synths.iter_mut().filter(|v| v.is_active()) {
            let base = synth.track() * QUANTUM;
            synth.render(
                self.sample_rate,
                first_frame,
                &mut self.input_left[base + from..base + to],
                &mut self.input_right[base + from..base + to],
            );
        }
    }
}

/// A free slot, else the one started first.
fn free_or_oldest<V>(voices: &[V], active: fn(&V) -> bool, serial: fn(&V) -> u64) -> usize {
    voices
        .iter()
        .position(|v| !active(v))
        .unwrap_or_else(|| (0..voices.len()).min_by_key(|&i| serial(&voices[i])).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::SynthStart;
    use ordrumbox_dsp::{SampleBuffer, SynthPatch, VoiceStart};

    const RATE: f32 = 48_000.0;

    fn bank() -> Arc<SampleBank> {
        let buffer = SampleBuffer::mono(vec![1.0; 100], 48_000);
        let instrument = crate::kit::KitSample {
            key: "K".into(),
            display_name: "k".into(),
            path: "k".into(),
            decay: Some(10_000.0),
            ..Default::default()
        };
        Arc::new(SampleBank {
            samples: vec![crate::kit::BankSample { kit: "x".into(), sample: instrument, buffer: buffer.into() }],
        })
    }

    fn sample_event(frame: u64) -> Event {
        let voice = VoiceStart { sample: 0, track: 0, ratio: 1.0, peak: 1.0, decay: 10.0, pan: 0.0, delay: 0 };
        Event { frame, sound: Sound::Sample(voice), mono: true }
    }

    fn pass(number: u64, frames: &[u64], length: u64) -> Arc<Pass> {
        let events = frames.iter().map(|&frame| sample_event(frame)).collect();
        Arc::new(Pass {
            number,
            frames: length,
            ticks: 32,
            events,
            strips: vec![StripSettings::default()],
            ..Pass::default()
        })
    }

    /// An engine with the master bus out of the way (bypassed, unity gain).
    fn engine() -> Engine {
        let mut engine = Engine::new(RATE, bank());
        engine.set_master(MasterSettings { bypass: 1.0, ..MasterSettings::default() });
        engine.master_current = engine.master_target;
        engine
    }

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        engine.render(&mut l, &mut r);
        l
    }

    /// Frame before each note's first sound (an attack starts from 0): the
    /// signal rises after 16 quiet frames (the strip filter's ringing at the
    /// end of a note is not an onset).
    fn onsets(signal: &[f32]) -> Vec<usize> {
        let quiet = |i: usize| signal[i.saturating_sub(16)..i].iter().all(|s| s.abs() < 1e-4);
        (1..signal.len()).filter(|&i| quiet(i) && signal[i].abs() >= 1e-4).map(|i| i - 1).collect()
    }

    #[test]
    fn events_start_on_their_frame_across_quanta_and_passes() {
        let mut engine = engine();
        engine.play(pass(0, &[3, 700], 1000));
        assert_eq!(engine.wants_pass(), Some(1));
        engine.queue(pass(1, &[10], 1000));
        let signal = render(&mut engine, 1500);
        assert_eq!(onsets(&signal), [3, 700, 1010]);
        assert_eq!(engine.position(), Some(Position { pass: 1, frame: 500, frames: 1000 }));
    }

    #[test]
    fn late_next_pass_replays_the_current_one() {
        let mut engine = engine();
        engine.play(pass(0, &[0], 200));
        let signal = render(&mut engine, 450);
        assert_eq!(onsets(&signal), [0, 200, 400]);
        assert_eq!(engine.position().map(|p| p.pass), Some(2));
        engine.queue(pass(1, &[0], 200)); // stale: ignored
        assert_eq!(engine.wants_pass(), Some(3));
    }

    #[test]
    fn stop_releases_the_voices() {
        let mut engine = engine();
        let long = SampleBuffer::mono(vec![1.0; 48_000], 48_000);
        engine.bank = Arc::new(SampleBank {
            samples: vec![crate::kit::BankSample { buffer: long.into(), ..engine.bank.samples[0].clone() }],
        });
        engine.play(pass(0, &[0], 48_000));
        render(&mut engine, 1000);
        engine.stop();
        let tail = render(&mut engine, 2000);
        assert!(tail[0].abs() > 0.1 && tail[1500].abs() < 1e-6);
        assert!(!engine.is_playing() && engine.position().is_none());
    }

    #[test]
    fn a_silent_strip_outputs_nothing() {
        let mut engine = engine();
        let mut muted = (*pass(0, &[0], 1000)).clone();
        muted.strips[0].volume = 0.0;
        engine.play(Arc::new(muted));
        assert!(render(&mut engine, 500).iter().all(|&s| s == 0.0));
    }

    #[test]
    fn synth_notes_play_for_their_length() {
        let mut engine = engine();
        let patch = SynthPatch { velocity: 0.5, volume: 1.0, ..SynthPatch::default() };
        let event =
            Event { frame: 64, sound: Sound::Synth(SynthStart { track: 0, patch: 0, length: 0.01 }), mono: false };
        let pass = Pass {
            frames: 48_000,
            ticks: 128,
            events: vec![event],
            strips: vec![StripSettings::default()],
            patches: vec![patch],
            ..Pass::default()
        };
        engine.play(Arc::new(pass));
        let signal = render(&mut engine, 24_000);
        assert!(signal[..64].iter().all(|&s| s == 0.0) && signal[65] != 0.0, "starts on frame 64");
        // Released after 10 ms, with a 0.1 s release.
        assert!(signal[64 + 480 + 4800 + 200..].iter().all(|s| s.abs() < 1e-6));
        assert!(engine.synths.iter().all(|s| !s.is_active()));
    }

    #[test]
    fn master_bus_limits_the_output() {
        let mut engine = Engine::new(RATE, bank());
        let mut loud = (*pass(0, &[0, 50], 1000)).clone();
        loud.strips[0].volume = 2.0;
        engine.play(Arc::new(loud));
        assert!(render(&mut engine, 1000).iter().all(|s| s.abs() <= 1.0));
    }

    #[test]
    fn replace_keeps_the_position_and_plays_what_is_ahead() {
        let mut engine = engine();
        engine.play(pass(0, &[0, 600], 1000));
        engine.queue(pass(1, &[0], 1000));
        render(&mut engine, 300);
        engine.replace(pass(0, &[100, 400, 800], 1000));
        assert_eq!(engine.wants_pass(), Some(1), "the old next pass is dropped");
        let signal = render(&mut engine, 700);
        assert_eq!(onsets(&signal), [100, 500], "frames 400 and 800 of the pass");
        engine.replace(pass(5, &[0], 1000)); // not the playing pass: ignored
        assert_eq!(engine.position().map(|p| p.frame), Some(1000));
    }
}
