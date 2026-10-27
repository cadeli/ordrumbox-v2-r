//! The two sides of playback and what they share.
//!
//! - [`Controller`], on the UI thread: builds passes (this allocates) and posts
//!   them ahead of time.
//! - [`AudioSide`], in the audio callback: picks up what was posted with a
//!   `try_lock` that never waits (if the UI holds the lock, it tries again on
//!   the next callback), renders, and publishes its position through atomics.
//!
//! The controller keeps a reference to every pass and bank it posted until the
//! engine is past them, so the audio thread never drops a last reference.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use ordrumbox_core::{Pattern, Rng, Scales};
use ordrumbox_dsp::MasterSettings;

use crate::engine::{Engine, Position};
use crate::kit::{SampleBank, assign_samples};
use crate::pass::{Pass, PassContext, build_pass, build_preview};
use crate::synth_preset::SynthPresets;

/// Passes kept alive. The engine holds at most its current pass and the
/// queued one, and the inbox one more, all among the last three posted.
const KEPT_PASSES: usize = 4;

#[derive(Debug)]
enum Command {
    Play(Arc<Pass>),
    /// A rebuild of the playing pass.
    Replace(Arc<Pass>),
    Stop,
}

#[derive(Debug, Default)]
struct Inbox {
    command: Option<Command>,
    next: Option<Arc<Pass>>,
    bank: Option<Arc<SampleBank>>,
    master: Option<MasterSettings>,
    preview: Option<Arc<Pass>>,
}

/// State shared by both sides.
#[derive(Debug, Default)]
struct Link {
    inbox: Mutex<Inbox>,
    playing: AtomicBool,
    pass: AtomicU64,
    frame: AtomicU64,
    frames: AtomicU64,
    /// Pass index the engine needs, plus one (0: none).
    wants: AtomicU64,
}

impl Link {
    fn inbox(&self) -> MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The audio-callback side: owns the [`Engine`].
#[derive(Debug)]
pub struct AudioSide {
    engine: Engine,
    link: Arc<Link>,
}

impl AudioSide {
    pub fn sample_rate(&self) -> f32 {
        self.engine.sample_rate()
    }

    /// Render one callback's worth of stereo frames.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if let Ok(mut inbox) = self.link.inbox.try_lock() {
            if let Some(bank) = inbox.bank.take() {
                self.engine.set_bank(bank);
            }
            if let Some(master) = inbox.master.take() {
                self.engine.set_master(master);
            }
            match inbox.command.take() {
                Some(Command::Play(pass)) => self.engine.play(pass),
                Some(Command::Replace(pass)) => self.engine.replace(pass),
                Some(Command::Stop) => self.engine.stop(),
                None => {}
            }
            if let Some(next) = inbox.next.take() {
                self.engine.queue(next);
            }
            if let Some(preview) = inbox.preview.take() {
                self.engine.preview(preview);
            }
        }
        self.engine.render(left, right);

        let position = self.engine.position();
        self.link.playing.store(position.is_some(), Ordering::Release);
        if let Some(Position { pass, frame, frames }) = position {
            self.link.pass.store(pass, Ordering::Release);
            self.link.frame.store(frame, Ordering::Release);
            self.link.frames.store(frames, Ordering::Release);
        }
        self.link.wants.store(self.engine.wants_pass().map_or(0, |n| n + 1), Ordering::Release);
    }
}

/// The UI side: transport commands and pass scheduling.
#[derive(Debug)]
pub struct Controller {
    link: Arc<Link>,
    sample_rate: f64,
    bank: Arc<SampleBank>,
    /// Banks posted to the engine, kept until it has surely switched.
    old_banks: Vec<Arc<SampleBank>>,
    scales: Scales,
    synths: SynthPresets,
    master: MasterSettings,
    kit: String,
    rng: Rng,
    /// Passes posted, oldest first.
    sent: VecDeque<Arc<Pass>>,
    /// Previews posted, oldest first.
    previews: VecDeque<Arc<Pass>>,
    playing: bool,
}

impl Controller {
    /// Both sides for a stream at `sample_rate`.
    pub fn new(sample_rate: f32, bank: Arc<SampleBank>, scales: Scales, synths: SynthPresets) -> (Self, AudioSide) {
        let link = Arc::new(Link::default());
        let kit = bank.samples.first().map(|s| s.kit.clone()).unwrap_or_default();
        let audio = AudioSide { engine: Engine::new(sample_rate, bank.clone()), link: link.clone() };
        let controller = Self {
            link,
            sample_rate: f64::from(sample_rate),
            bank,
            old_banks: Vec::new(),
            scales,
            synths,
            master: MasterSettings::default(),
            kit,
            rng: Rng::from_time(),
            sent: VecDeque::new(),
            previews: VecDeque::new(),
            playing: false,
        };
        (controller, audio)
    }

    pub fn kit(&self) -> &str {
        &self.kit
    }

    /// Prefer the samples of kit `name` from the next pass on.
    pub fn set_kit(&mut self, name: &str) {
        self.kit = name.to_owned();
    }

    pub fn bank(&self) -> &Arc<SampleBank> {
        &self.bank
    }

    /// Master bus settings (`settings.master` of the JS app).
    pub fn master(&self) -> MasterSettings {
        self.master
    }

    pub fn set_master(&mut self, master: MasterSettings) {
        self.master = master;
        self.link.inbox().master = Some(master);
    }

    /// Master volume, 0–2.
    pub fn set_volume(&mut self, volume: f32) {
        self.set_master(MasterSettings { master: volume.clamp(0.0, 2.0), ..self.master });
    }

    fn build(&mut self, pattern: &Pattern, number: u64) -> Arc<Pass> {
        let samples = assign_samples(pattern, &self.bank, &self.kit);
        // The transport tick runs on from play: after the previous pass.
        let first_tick = self
            .sent
            .iter()
            .rev()
            .find_map(|p| match p.number {
                n if n == number => Some(p.first_tick),
                n if n + 1 == number => Some(p.first_tick + u64::from(p.ticks)),
                _ => None,
            })
            .unwrap_or(0);
        let context = PassContext {
            bank: &self.bank,
            samples: &samples,
            scales: &self.scales,
            synths: &self.synths,
            sample_rate: self.sample_rate,
            first_tick,
        };
        let pass = Arc::new(build_pass(pattern, number, &mut self.rng, context));
        self.sent.push_back(pass.clone());
        pass
    }

    /// Audition track `t` of `pattern` (`simpleBeep`): `note` (an index in
    /// its notes) or a default note, through the track's strip.
    pub fn preview(&mut self, pattern: &Pattern, t: usize, note: Option<usize>) {
        let samples = assign_samples(pattern, &self.bank, &self.kit);
        let note = note.and_then(|n| pattern.tracks.get(t)?.notes.get(n));
        let context = PassContext {
            bank: &self.bank,
            samples: &samples,
            scales: &self.scales,
            synths: &self.synths,
            sample_rate: self.sample_rate,
            first_tick: 0,
        };
        let pass = Arc::new(build_preview(pattern, t, note, context));
        if pass.events.is_empty() {
            return;
        }
        self.previews.push_back(pass.clone());
        while self.previews.len() > KEPT_PASSES {
            self.previews.pop_front();
        }
        self.link.inbox().preview = Some(pass);
    }

    /// Start `pattern` from its first pass.
    pub fn play(&mut self, pattern: &Pattern) {
        let pass = self.build(pattern, 0);
        self.link.inbox().command = Some(Command::Play(pass));
        self.playing = true;
    }

    /// `pattern` was edited: rebuild the pass playing now (it goes on from
    /// the same place), and the next ones follow the edit.
    pub fn refresh(&mut self, pattern: &Pattern) {
        if !self.playing || !self.link.playing.load(Ordering::Acquire) {
            return;
        }
        let number = self.link.pass.load(Ordering::Acquire);
        let pass = self.build(pattern, number);
        let mut inbox = self.link.inbox();
        // A Play still waiting must not be replaced by a rebuild.
        if !matches!(inbox.command, Some(Command::Play(_))) {
            inbox.command = Some(Command::Replace(pass));
            inbox.next = None;
        }
    }

    pub fn stop(&mut self) {
        self.link.inbox().command = Some(Command::Stop);
        self.playing = false;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Call regularly (every UI frame) while playing: posts the next pass of
    /// `pattern` when the engine needs it, so tempo, kit and pattern changes
    /// take effect at the next pass boundary.
    pub fn update(&mut self, pattern: &Pattern) {
        let wants = self.link.wants.load(Ordering::Acquire);
        let already_sent = self.sent.back().is_some_and(|p| p.number + 1 == wants);
        if self.playing && wants > 0 && !already_sent {
            let pass = self.build(pattern, wants - 1);
            self.link.inbox().next = Some(pass);
        }
        while self.sent.len() > KEPT_PASSES {
            self.sent.pop_front();
        }
        if self.link.inbox().bank.is_none() {
            self.old_banks.clear(); // the engine took the last bank posted
        }
    }

    /// Replace the samples (after a reload); the engine switches on its next callback.
    pub fn set_bank(&mut self, bank: Arc<SampleBank>) {
        self.old_banks.push(std::mem::replace(&mut self.bank, bank.clone()));
        self.link.inbox().bank = Some(bank);
    }

    /// Playback position, `None` when stopped.
    pub fn position(&self) -> Option<Position> {
        self.link.playing.load(Ordering::Acquire).then(|| Position {
            pass: self.link.pass.load(Ordering::Acquire),
            frame: self.link.frame.load(Ordering::Acquire),
            frames: self.link.frames.load(Ordering::Acquire),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::Library;
    use ordrumbox_dsp::SampleBuffer;

    fn bank() -> Arc<SampleBank> {
        let instrument =
            crate::kit::Instrument { key: "KICK".into(), display_name: "k".into(), url: "k".into(), decay_ms: 100.0 };
        let buffer = SampleBuffer::mono(vec![0.5; 400], 8000);
        Arc::new(SampleBank { samples: vec![crate::kit::BankSample { kit: "x".into(), instrument, buffer }] })
    }

    #[test]
    fn plays_pass_after_pass_like_an_audio_callback() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK","notes":[{},{"beat":2}]}]}]}"#,
        )
        .unwrap();
        let pattern = &library.patterns[0];
        let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
        controller.play(pattern);
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        let mut seen_passes = Vec::new();
        // 4 beats at 120 bpm = 2 s = 16000 frames per pass: run 3 passes.
        for _ in 0..(3 * 16_000 / 256) {
            controller.update(pattern);
            audio.render(&mut l, &mut r);
            let pass = controller.position().expect("playing").pass;
            if seen_passes.last() != Some(&pass) {
                seen_passes.push(pass);
            }
        }
        assert_eq!(seen_passes, [0, 1, 2]);
        assert!(controller.sent.len() <= KEPT_PASSES);
        controller.stop();
        audio.render(&mut l, &mut r);
        assert_eq!(controller.position(), None);
    }

    #[test]
    fn preview_sounds_while_stopped() {
        let (library, _) =
            Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[]}]}]}"#).unwrap();
        let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
        controller.preview(&library.patterns[0], 0, None);
        controller.preview(&library.patterns[0], 5, None); // no such track: nothing
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        audio.render(&mut l, &mut r);
        assert!(l.iter().any(|s| s.abs() > 0.01));
        assert_eq!(controller.position(), None);
    }
}
