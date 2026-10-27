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

use std::collections::HashMap;

use ordrumbox_core::{Library, Rng, Scales};
use ordrumbox_dsp::MasterSettings;

use crate::assign::{TrackSound, assign_track};
use crate::engine::{Engine, Position};
use crate::kit::SampleBank;
use crate::pass::{FlatNotesCache, Pass, PassContext, SongContext, build_pass, build_preview, build_song_pass};
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

/// What plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Pattern `index`, looped at its own tempo.
    Pattern(usize),
    /// Song `index`: its arrangement, a measure per pass, at its tempo.
    Song(usize),
}

/// The UI side: transport commands, sound assignment and pass scheduling.
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
    mode: Mode,
    /// Sound of each track, by (pattern id, track index), with what it was
    /// decided from: auto-assignment picks at random, once per play.
    assigned: HashMap<(String, usize), (String, TrackSound)>,
    /// Pattern repetitions drawn for the song passes.
    song_notes: FlatNotesCache,
    /// Passes posted, oldest first.
    sent: VecDeque<Arc<Pass>>,
    /// Previews posted, oldest first.
    previews: VecDeque<Arc<Pass>>,
    playing: bool,
}

/// What a track's sound depends on.
fn sound_key(track: &ordrumbox_core::Track) -> String {
    format!(
        "{}|{}|{}|{}|{:?}",
        track.name, track.use_auto_assign_sound, track.sample_id, track.use_soft_synth, track.synth_sound_key
    )
}

impl Controller {
    /// Both sides for a stream at `sample_rate`.
    pub fn new(sample_rate: f32, bank: Arc<SampleBank>, scales: Scales, synths: SynthPresets) -> (Self, AudioSide) {
        Self::with_rng(sample_rate, bank, scales, synths, Rng::from_time())
    }

    /// The same with a given random stream (offline renders, tests).
    pub fn with_rng(
        sample_rate: f32,
        bank: Arc<SampleBank>,
        scales: Scales,
        synths: SynthPresets,
        rng: Rng,
    ) -> (Self, AudioSide) {
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
            rng,
            mode: Mode::Pattern(0),
            assigned: HashMap::new(),
            song_notes: FlatNotesCache::new(),
            sent: VecDeque::new(),
            previews: VecDeque::new(),
            playing: false,
        };
        (controller, audio)
    }

    pub fn kit(&self) -> &str {
        &self.kit
    }

    /// Use kit `name` (sounds are assigned again: `autoAssignSoundsForNewDrumkit`).
    pub fn set_kit(&mut self, name: &str) {
        self.kit = name.to_owned();
        self.assigned.clear();
    }

    pub fn bank(&self) -> &Arc<SampleBank> {
        &self.bank
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Play pattern `index` from the next pass on (pattern mode).
    pub fn select_pattern(&mut self, index: usize) {
        if let Mode::Pattern(_) = self.mode {
            self.mode = Mode::Pattern(index);
        }
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

    /// What each track of pattern `p` plays (assigned once, then kept).
    fn sounds(&mut self, library: &Library, p: usize) -> Vec<TrackSound> {
        let Some(pattern) = library.patterns.get(p) else { return Vec::new() };
        let rng = &mut self.rng;
        let mut pick = |n: usize| rng.below(n as u64) as usize;
        pattern
            .tracks
            .iter()
            .enumerate()
            .map(|(t, track)| {
                let key = sound_key(track);
                let slot = (pattern.id.clone(), t);
                match self.assigned.get(&slot) {
                    Some((known, sound)) if *known == key => sound.clone(),
                    _ => {
                        let sound = assign_track(track, &self.bank, &self.kit, &self.synths, &mut pick);
                        self.assigned.insert(slot, (key, sound.clone()));
                        sound
                    }
                }
            })
            .collect()
    }

    /// The transport tick pass `number` starts at: after the previous pass.
    fn first_tick(&self, number: u64) -> u64 {
        self.sent
            .iter()
            .rev()
            .find_map(|p| match p.number {
                n if n == number => Some(p.first_tick),
                n if n + 1 == number => Some(p.first_tick + u64::from(p.ticks)),
                _ => None,
            })
            .unwrap_or(0)
    }

    /// Pass `number` of what plays; `None` when there is nothing to play.
    fn build(&mut self, library: &Library, number: u64) -> Option<Arc<Pass>> {
        let pass = match self.mode {
            Mode::Pattern(p) => {
                let pattern = library.patterns.get(p)?;
                let sounds = self.sounds(library, p);
                let context = PassContext {
                    bank: &self.bank,
                    sounds: &sounds,
                    scales: &self.scales,
                    synths: &self.synths,
                    sample_rate: self.sample_rate,
                    first_tick: self.first_tick(number),
                };
                build_pass(pattern, number, &mut self.rng, context)
            }
            Mode::Song(s) => {
                let song = library.songs.get(s)?;
                let used: Vec<usize> = song
                    .clips
                    .iter()
                    .filter_map(|c| library.patterns.iter().rposition(|p| p.id == c.pattern))
                    .collect();
                let mut sounds = vec![Vec::new(); library.patterns.len()];
                for &p in &used {
                    if sounds[p].is_empty() {
                        sounds[p] = self.sounds(library, p);
                    }
                }
                let context = SongContext {
                    bank: &self.bank,
                    patterns: &library.patterns,
                    sounds: &sounds,
                    scales: &self.scales,
                    synths: &self.synths,
                    sample_rate: self.sample_rate,
                };
                build_song_pass(song, number, &mut self.rng, &mut self.song_notes, context)
            }
        };
        let pass = Arc::new(pass);
        self.sent.push_back(pass.clone());
        Some(pass)
    }

    /// Audition track `t` of pattern `p` (`simpleBeep`): `note` (an index in
    /// its notes) or a default note, through the track's strip.
    pub fn preview(&mut self, library: &Library, p: usize, t: usize, note: Option<usize>) {
        let Some(pattern) = library.patterns.get(p) else { return };
        let sounds = self.sounds(library, p);
        let note = note.and_then(|n| pattern.tracks.get(t)?.notes.get(n));
        let context = PassContext {
            bank: &self.bank,
            sounds: &sounds,
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

    /// Start `mode` from its beginning; sounds are assigned afresh.
    pub fn play(&mut self, library: &Library, mode: Mode) {
        self.mode = mode;
        self.assigned.clear();
        self.song_notes.clear();
        let Some(pass) = self.build(library, 0) else { return };
        self.link.inbox().command = Some(Command::Play(pass));
        self.playing = true;
    }

    /// The library was edited: rebuild the pass playing now (it goes on from
    /// the same place), and the next ones follow the edit.
    pub fn refresh(&mut self, library: &Library) {
        if !self.playing || !self.link.playing.load(Ordering::Acquire) {
            return;
        }
        let number = self.link.pass.load(Ordering::Acquire);
        self.song_notes.clear();
        let Some(pass) = self.build(library, number) else { return };
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

    /// The pass [`Self::update`] builds now, if it builds one: the moment for
    /// the loop start of auto-mode tracks (a pass is a loop of the pattern).
    pub fn pending_pass(&self) -> Option<u64> {
        let wants = self.link.wants.load(Ordering::Acquire);
        let already_sent = self.sent.back().is_some_and(|p| p.number + 1 == wants);
        (self.playing && wants > 0 && !already_sent).then(|| wants - 1)
    }

    /// Call regularly (every UI frame) while playing: posts the next pass
    /// when the engine needs it, so tempo, kit and pattern changes take
    /// effect at the next pass boundary.
    pub fn update(&mut self, library: &Library) {
        if let Some(number) = self.pending_pass()
            && let Some(pass) = self.build(library, number)
        {
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
        self.assigned.clear();
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
        let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
        controller.play(&library, Mode::Pattern(0));
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        let mut seen_passes = Vec::new();
        // 4 beats at 120 bpm = 2 s = 16000 frames per pass: run 3 passes.
        for _ in 0..(3 * 16_000 / 256) {
            controller.update(&library);
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
        controller.preview(&library, 0, 0, None);
        controller.preview(&library, 0, 5, None); // no such track: nothing
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        audio.render(&mut l, &mut r);
        assert!(l.iter().any(|s| s.abs() > 0.01));
        assert_eq!(controller.position(), None);
    }
}
