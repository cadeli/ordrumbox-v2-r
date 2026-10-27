//! The audio-thread side: plays passes, sample-accurately, into a stereo
//! buffer. `render` never allocates, locks or blocks; the passes and the bank
//! it drops are never their last reference (the [`crate::Controller`] keeps
//! one), so it never frees memory either.

use std::sync::Arc;

use ordrumbox_dsp::{SampleVoice, StripParams};

use crate::kit::SampleBank;
use crate::pass::Pass;

/// Voices playing at once; the oldest is cut when a new one needs a slot.
pub const MAX_VOICES: usize = 64;
/// Tracks with their own strip; notes of tracks beyond are dropped.
pub const MAX_TRACKS: usize = 64;
/// Frames rendered per internal block (longer requests are split).
const BLOCK: usize = 512;

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
    voices: Vec<SampleVoice>,
    serial: u64,
    strips: [StripParams; MAX_TRACKS],
    strip_left: Vec<f32>,
    strip_right: Vec<f32>,
    /// Master gain.
    pub volume: f32,
}

impl Engine {
    /// Allocates everything the audio thread will ever need.
    pub fn new(sample_rate: f32, bank: Arc<SampleBank>) -> Self {
        Self {
            sample_rate,
            bank,
            current: None,
            next: None,
            pass_number: 0,
            frame: 0,
            cursor: 0,
            voices: vec![SampleVoice::default(); MAX_VOICES],
            serial: 0,
            strips: [StripParams::default(); MAX_TRACKS],
            strip_left: vec![0.0; MAX_TRACKS * BLOCK],
            strip_right: vec![0.0; MAX_TRACKS * BLOCK],
            volume: 0.8,
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

    /// Start playing `pass` from its first frame.
    pub fn play(&mut self, pass: Arc<Pass>) {
        self.pass_number = pass.number;
        self.load(pass);
        self.next = None;
    }

    /// Pass to play after the current one, if it is the next index.
    pub fn queue(&mut self, pass: Arc<Pass>) {
        if self.current.is_some() && pass.number == self.pass_number + 1 {
            self.next = Some(pass);
        }
    }

    /// Stop the transport; playing voices fade out.
    pub fn stop(&mut self) {
        self.current = None;
        self.next = None;
        for voice in &mut self.voices {
            voice.release(self.sample_rate);
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

    fn load(&mut self, pass: Arc<Pass>) {
        for (strip, params) in self.strips.iter_mut().zip(&pass.strips) {
            *strip = *params;
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
            let n = (frames - done).min(BLOCK);
            self.render_block(&mut left[done..done + n], &mut right[done..done + n]);
            done += n;
        }
    }

    fn render_block(&mut self, out_left: &mut [f32], out_right: &mut [f32]) {
        let n = out_left.len();
        let tracks = self.voices.iter().filter(|v| v.is_active()).map(|v| v.track() + 1).max().unwrap_or(0);
        let tracks = tracks.max(self.current.as_ref().map_or(0, |p| p.strips.len())).min(MAX_TRACKS);
        self.strip_left[..tracks * BLOCK].fill(0.0);
        self.strip_right[..tracks * BLOCK].fill(0.0);

        // Split the block at every event and pass end, so each starts on its frame.
        let mut offset = 0;
        while offset < n {
            let until = self.dispatch_due_events().map_or(n, |frames_left| (offset + frames_left).min(n));
            self.render_voices(offset, until);
            self.frame += (until - offset) as u64;
            offset = until;
        }

        out_left.fill(0.0);
        out_right.fill(0.0);
        for track in 0..tracks {
            let range = track * BLOCK..track * BLOCK + n;
            self.strips[track].mix_into(&self.strip_left[range.clone()], &self.strip_right[range], out_left, out_right);
        }
        for sample in out_left.iter_mut().chain(out_right.iter_mut()) {
            *sample = (*sample * self.volume).clamp(-1.0, 1.0);
        }
    }

    /// Start the events due at the current frame (moving to the next pass at
    /// the end of this one); frames until the next event or pass end, `None`
    /// when stopped.
    fn dispatch_due_events(&mut self) -> Option<usize> {
        loop {
            // Taken out and put back, so no reference count changes here.
            let pass = self.current.take()?;
            if self.frame >= pass.frames {
                self.pass_number += 1;
                match self.next.take() {
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
            while let Some(event) = pass.events.get(self.cursor).filter(|e| e.frame <= self.frame) {
                self.start_voice(event);
                self.cursor += 1;
            }
            let next_stop = pass.events.get(self.cursor).map_or(pass.frames, |e| e.frame);
            let frames_left = (next_stop - self.frame) as usize;
            self.current = Some(pass);
            return Some(frames_left);
        }
    }

    fn start_voice(&mut self, event: &crate::pass::Event) {
        let params = &event.voice;
        let Some(sample) = self.bank.get(params.sample) else { return };
        if params.track >= MAX_TRACKS {
            return;
        }
        if event.mono {
            for voice in self.voices.iter_mut().filter(|v| v.is_active() && v.track() == params.track) {
                voice.release(self.sample_rate);
            }
        }
        let slot = self
            .voices
            .iter()
            .position(|v| !v.is_active())
            .unwrap_or_else(|| (0..MAX_VOICES).min_by_key(|&i| self.voices[i].serial()).unwrap_or(0));
        self.serial += 1;
        self.voices[slot].start(params, &sample.buffer, self.sample_rate, self.serial);
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
            let base = voice.track() * BLOCK;
            voice.render(
                &sample.buffer,
                &mut self.strip_left[base + from..base + to],
                &mut self.strip_right[base + from..base + to],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::Event;
    use ordrumbox_dsp::{SampleBuffer, VoiceStart};

    fn bank() -> Arc<SampleBank> {
        let buffer = SampleBuffer::mono(vec![1.0; 100], 1000);
        let instrument =
            crate::kit::Instrument { key: "K".into(), display_name: "k".into(), url: "k".into(), decay_ms: 10_000.0 };
        Arc::new(SampleBank { samples: vec![crate::kit::BankSample { kit: "x".into(), instrument, buffer }] })
    }

    fn pass(number: u64, frames: &[u64], length: u64) -> Arc<Pass> {
        let voice = VoiceStart { sample: 0, track: 0, ratio: 1.0, peak: 1.0, decay: 10.0, pan: 0.0, delay: 0 };
        let events = frames.iter().map(|&frame| Event { frame, voice, mono: true }).collect();
        Arc::new(Pass { number, frames: length, ticks: 32, events, strips: vec![StripParams::default()] })
    }

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        engine.render(&mut l, &mut r);
        l
    }

    /// First frame of each note: where the signal jumps from (near) silence.
    fn onsets(signal: &[f32]) -> Vec<usize> {
        (1..signal.len()).filter(|&i| signal[i - 1] < 1e-6 && signal[i] > 1e-6).map(|i| i - 1).collect()
    }

    #[test]
    fn events_start_on_their_frame_across_blocks_and_passes() {
        let mut engine = Engine::new(1000.0, bank());
        engine.volume = 1.0;
        engine.play(pass(0, &[3, 700], 1000));
        assert_eq!(engine.wants_pass(), Some(1));
        engine.queue(pass(1, &[10], 1000));
        let signal = render(&mut engine, 1500);
        assert_eq!(onsets(&signal), [3, 700, 1010]);
        assert_eq!(engine.position(), Some(Position { pass: 1, frame: 500, frames: 1000 }));
    }

    #[test]
    fn late_next_pass_replays_the_current_one() {
        let mut engine = Engine::new(1000.0, bank());
        engine.play(pass(0, &[0], 200));
        let signal = render(&mut engine, 450);
        assert_eq!(onsets(&signal), [0, 200, 400]);
        assert_eq!(engine.position().map(|p| p.pass), Some(2));
        engine.queue(pass(1, &[0], 200)); // stale: ignored
        assert_eq!(engine.wants_pass(), Some(3));
    }

    #[test]
    fn stop_fades_and_silences() {
        let mut engine = Engine::new(1000.0, bank());
        engine.play(pass(0, &[0], 1000));
        render(&mut engine, 20);
        engine.stop();
        let tail = render(&mut engine, 100);
        assert!(tail[0] > 0.0 && tail[50] == 0.0);
        assert!(!engine.is_playing() && engine.position().is_none());
    }

    #[test]
    fn mute_silences_the_strip() {
        let mut engine = Engine::new(1000.0, bank());
        let mut muted = (*pass(0, &[0], 1000)).clone();
        muted.strips[0].silent = true;
        engine.play(Arc::new(muted));
        assert!(render(&mut engine, 50).iter().all(|&s| s == 0.0));
    }
}
