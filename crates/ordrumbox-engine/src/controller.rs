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
use crate::engine::{Engine, MAX_TRACKS, Position};
use crate::kit::SampleBank;
use crate::pass::{HitsCache, Pass, PassContext, SongContext, build_pass, build_preview, build_song_pass};
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

/// Samples the scope keeps (the JS analyser's `fftSize`).
pub const SCOPE_SIZE: usize = 4096;

/// The last samples played, mono, as a ring (the analyser's input).
#[derive(Debug)]
struct Scope {
    samples: Vec<f32>,
    /// Where the next sample goes.
    at: usize,
}

impl Default for Scope {
    fn default() -> Self {
        Self { samples: vec![0.0; SCOPE_SIZE], at: 0 }
    }
}

/// State shared by both sides.
#[derive(Debug, Default)]
struct Link {
    inbox: Mutex<Inbox>,
    /// Written by the audio side when it can take it, never waited for.
    scope: Mutex<Scope>,
    levels: Mutex<Vec<f32>>,
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
        if let Ok(mut levels) = self.link.levels.try_lock() {
            let peaks = self.engine.peaks();
            if levels.len() < peaks.len() {
                levels.resize(peaks.len(), 0.0);
            }
            for (l, p) in levels.iter_mut().zip(peaks) {
                *l = *p;
            }
        }
        if let Ok(mut scope) = self.link.scope.try_lock() {
            for (l, r) in left.iter().zip(right.iter()) {
                let at = scope.at;
                scope.samples[at] = (l + r) * 0.5;
                scope.at = (at + 1) % SCOPE_SIZE;
            }
        }

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
    song_notes: HitsCache,
    /// Passes posted, oldest first.
    sent: VecDeque<Arc<Pass>>,
    /// Previews posted, oldest first.
    previews: VecDeque<Arc<Pass>>,
    playing: bool,
}

/// What a track's sound depends on.
fn sound_key(track: &ordrumbox_core::Track) -> String {
    format!("{}|{}|{}|{}|{:?}", track.name, track.auto_assign, track.sample_path, track.use_synth, track.synth_preset)
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
        let link = Arc::new(Link {
            inbox: Mutex::new(Inbox::default()),
            scope: Mutex::new(Scope::default()),
            levels: Mutex::new(vec![0.0; MAX_TRACKS]),
            playing: AtomicBool::new(false),
            pass: AtomicU64::new(0),
            frame: AtomicU64::new(0),
            frames: AtomicU64::new(0),
            wants: AtomicU64::new(0),
        });
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
            song_notes: HitsCache::new(),
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

    /// Per-track peak of the last rendered quantum (0–1), for VU meters.
    pub fn levels(&self) -> Vec<f32> {
        self.link.levels.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// The last [`SCOPE_SIZE`] samples played, oldest first, mono (silence
    /// before anything played).
    pub fn scope(&self, out: &mut [f32; SCOPE_SIZE]) {
        let scope = self.link.scope.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let (newer, older) = scope.samples.split_at(scope.at);
        out[..older.len()].copy_from_slice(older);
        out[older.len()..].copy_from_slice(newer);
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

    /// What track `t` of pattern `p` plays: what it was assigned, else
    /// (not played yet) the first pick of its assignment.
    pub fn track_sound(&self, library: &Library, p: usize, t: usize) -> TrackSound {
        let Some(pattern) = library.patterns.get(p) else { return TrackSound::None };
        let Some(track) = pattern.tracks.get(t) else { return TrackSound::None };
        match self.assigned.get(&(pattern.id.clone(), t)) {
            Some((key, sound)) if *key == sound_key(track) => sound.clone(),
            _ => assign_track(track, &self.bank, &self.kit, &self.synths, &mut |_| 0),
        }
    }

    /// The transport tick playing now (what the track LFOs are at); `None` when stopped.
    pub fn tick(&self) -> Option<u64> {
        let position = self.position()?;
        let pass = self.sent.iter().find(|p| p.number == position.pass)?;
        Some(pass.first_tick + position.frame * u64::from(pass.ticks) / position.frames.max(1))
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
                    .filter_map(|c| library.patterns.iter().rposition(|p| p.id == c.pattern_id))
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

    /// Audition track `t` of pattern `p`: `note` (an index in
    /// its notes) or a default note, through the track's strip.
    pub fn preview(&mut self, library: &Library, p: usize, t: usize, note: Option<usize>) {
        let note = note.and_then(|n| library.patterns.get(p)?.tracks.get(t)?.notes.get(n)).cloned();
        self.preview_note(library, p, t, note.as_ref());
    }

    /// Audition `note` (else a default note) on track `t` of pattern `p`,
    /// whether the track has it or not (a piano roll key: `#playKey`).
    pub fn preview_note(&mut self, library: &Library, p: usize, t: usize, note: Option<&ordrumbox_core::Note>) {
        let Some(pattern) = library.patterns.get(p) else { return };
        let sounds = self.sounds(library, p);
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

    /// Audition sample `index` of the bank as it is (`DrumkitManager.audition`):
    /// whole, at its tune and gain trim, centred, through a neutral strip
    /// (the first track's while playing).
    pub fn audition_sample(&mut self, index: usize) {
        let Some(sample) = self.bank.get(index) else { return };
        let voice = ordrumbox_dsp::VoiceStart {
            sample: index,
            track: 0,
            ratio: (sample.sample.transpose / 12.0).exp2(),
            peak: sample.sample.gain() as f32,
            // No decay envelope: one that outlasts any sample.
            decay: 600.0,
            pan: 0.0,
            delay: 0,
        };
        let pass = Arc::new(Pass {
            frames: 1,
            ticks: 1,
            events: vec![crate::pass::Event { frame: 0, sound: crate::pass::Sound::Sample(voice), mono: false }],
            strips: vec![ordrumbox_dsp::StripSettings::default()],
            ..Pass::default()
        });
        self.previews.push_back(pass.clone());
        while self.previews.len() > KEPT_PASSES {
            self.previews.pop_front();
        }
        self.link.inbox().preview = Some(pass);
    }

    /// Start `mode` from its beginning; sounds are assigned afresh.
    pub fn play(&mut self, library: &Library, mode: Mode) {
        self.play_from(library, mode, 0);
    }

    /// Start `mode` from pass `first` (a song: from measure `first`, as from
    /// the arrangement cursor); sounds are assigned afresh.
    pub fn play_from(&mut self, library: &Library, mode: Mode, first: u64) {
        self.mode = mode;
        self.assigned.clear();
        self.jump(library, first);
    }

    /// Go on from pass `first` at once (`setSongCursor` while playing: the
    /// measure clicked sounds right away), with the sounds already assigned.
    pub fn jump(&mut self, library: &Library, first: u64) {
        self.song_notes.clear();
        let Some(pass) = self.build(library, first) else { return };
        let mut inbox = self.link.inbox();
        inbox.command = Some(Command::Play(pass));
        inbox.next = None;
        drop(inbox);
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
        let wants = self.link.wants.load(Ordering::Acquire).checked_sub(1)?;
        // The pass after the last one posted: not one already sent, and not
        // one the engine asked for before a jump it has not taken yet.
        let follows = self.sent.back().is_none_or(|p| p.number + 1 == wants);
        (self.playing && follows).then_some(wants)
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

    /// Replace the synth presets (an edit in the synth editor): the pass
    /// playing is rebuilt with them.
    pub fn set_synths(&mut self, library: &Library, synths: SynthPresets) {
        self.synths = synths;
        self.refresh(library);
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
        let instrument = crate::kit::KitSample {
            key: "KICK".into(),
            display_name: "k".into(),
            path: "k".into(),
            decay: Some(100.0),
            ..Default::default()
        };
        let buffer = SampleBuffer::mono(vec![0.5; 400], 8000);
        Arc::new(SampleBank {
            samples: vec![crate::kit::BankSample { kit: "x".into(), sample: instrument, buffer: buffer.into() }],
        })
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
    fn a_song_starts_at_a_measure_and_jumps() {
        let (library, _) = Library::parse(
            r#"{"songs":[{"name":"s","bpm":120,"clips":[{"pattern":"p","startMeasure":0,"measureCount":8}]}],
                "patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{}]}]}]}"#,
        )
        .unwrap();
        let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
        controller.play_from(&library, Mode::Song(0), 3);
        let mut seen = Vec::new();
        // One callback (with the UI's update first, or not); the passes met.
        let mut step = |controller: &mut Controller, update: bool, seen: &mut Vec<u64>| {
            let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
            if update {
                controller.update(&library);
            }
            audio.render(&mut l, &mut r);
            let pass = controller.position().expect("playing").pass;
            if seen.last() != Some(&pass) {
                seen.push(pass);
            }
        };
        // A measure at 120 bpm = 2 s = 16000 frames per pass.
        for _ in 0..20_000 / 256 {
            step(&mut controller, true, &mut seen);
        }
        assert_eq!(seen, [3, 4]);
        assert_eq!(controller.tick().map(|t| t / (u64::from(ordrumbox_core::steps::TICK) * 4)), Some(4));
        // Into measure 5 without the UI: the engine now asks for measure 6.
        while seen.last() == Some(&4) {
            step(&mut controller, false, &mut seen);
        }
        assert_eq!(controller.pending_pass(), Some(6));
        // Back to measure 1 before that request is served: it must not be.
        controller.jump(&library, 1);
        assert_eq!(controller.pending_pass(), None);
        for _ in 0..34_000 / 256 {
            step(&mut controller, true, &mut seen);
        }
        assert_eq!(seen, [3, 4, 5, 1, 2, 3]);
    }

    #[test]
    fn the_scope_keeps_the_last_samples_in_order() {
        let (controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
        let mut out = [1.0; SCOPE_SIZE];
        controller.scope(&mut out);
        assert!(out.iter().all(|&s| s == 0.0));
        // Silence goes in; then the ring is read oldest first.
        let (mut l, mut r) = (vec![0.0; 3000], vec![0.0; 3000]);
        audio.render(&mut l, &mut r);
        audio.render(&mut l, &mut r);
        {
            let mut scope = controller.link.scope.lock().unwrap();
            let at = scope.at;
            scope.samples[at] = 7.0; // the oldest
            scope.samples[(at + SCOPE_SIZE - 1) % SCOPE_SIZE] = 9.0; // the newest
        }
        controller.scope(&mut out);
        assert_eq!((out[0], out[SCOPE_SIZE - 1]), (7.0, 9.0));
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
