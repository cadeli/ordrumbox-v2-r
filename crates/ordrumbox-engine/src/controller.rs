//! The two sides of playback and what they share.
//!
//! - [`Controller`], on the UI thread: builds passes (this allocates) and posts
//!   them ahead of time.
//! - [`AudioSide`], in the audio callback: picks up what was posted with a
//!   `try_lock` that never waits (if the UI holds the lock, it tries again on
//!   the next callback), renders, and publishes its position as one snapshot
//!   (a seqlock: the UI never reads a pass number with another pass's frame).
//!
//! The controller keeps a reference to every pass and bank it posted until it
//! holds the only one left, so the audio thread never drops a last reference.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use ordrumbox_core::{Library, Rng, Scales, Track};
use ordrumbox_dsp::MasterSettings;

use crate::assign::{TrackSound, assign_track};
use crate::engine::{Engine, MAX_TRACKS, Position};
use crate::kit::SampleBank;
use crate::pass::{HitsCache, Pass, PassContext, SongContext, build_pass, build_preview, build_song_pass};
use crate::synth_preset::SynthPresets;

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

/// What the engine publishes after each callback, read as one snapshot: a
/// seqlock, odd `seq` while the audio side writes. The audio side never waits;
/// the UI reads again when a write was under way.
#[derive(Debug, Default)]
struct Published {
    seq: AtomicU64,
    playing: AtomicBool,
    pass: AtomicU64,
    frame: AtomicU64,
    frames: AtomicU64,
    /// Pass index the engine needs, plus one (0: none).
    wants: AtomicU64,
}

impl Published {
    /// Audio side, the only writer.
    fn write(&self, position: Option<Position>, wants: u64) {
        let seq = self.seq.load(Ordering::Relaxed);
        self.seq.store(seq + 1, Ordering::Relaxed);
        fence(Ordering::Release);
        self.playing.store(position.is_some(), Ordering::Relaxed);
        if let Some(Position { pass, frame, frames }) = position {
            self.pass.store(pass, Ordering::Relaxed);
            self.frame.store(frame, Ordering::Relaxed);
            self.frames.store(frames, Ordering::Relaxed);
        }
        self.wants.store(wants, Ordering::Relaxed);
        self.seq.store(seq + 2, Ordering::Release);
    }

    /// The position (`None` when stopped) and `wants`, from one callback.
    fn read(&self) -> (Option<Position>, u64) {
        loop {
            let before = self.seq.load(Ordering::Acquire);
            if before % 2 == 0 {
                let playing = self.playing.load(Ordering::Relaxed);
                let position = Position {
                    pass: self.pass.load(Ordering::Relaxed),
                    frame: self.frame.load(Ordering::Relaxed),
                    frames: self.frames.load(Ordering::Relaxed),
                };
                let wants = self.wants.load(Ordering::Relaxed);
                fence(Ordering::Acquire);
                if self.seq.load(Ordering::Relaxed) == before {
                    return (playing.then_some(position), wants);
                }
            }
            std::hint::spin_loop();
        }
    }
}

/// State shared by both sides.
#[derive(Debug, Default)]
struct Link {
    inbox: Mutex<Inbox>,
    /// Copied from the audio side's own ring when it can take the lock.
    scope: Mutex<Scope>,
    levels: Mutex<Vec<f32>>,
    published: Published,
}

/// The lock, whether or not a thread panicked holding it: the values it
/// guards are plain data, valid at any time.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The lock if it is free (the audio side never waits), poisoned or not.
fn try_lock<T>(mutex: &Mutex<T>) -> Option<MutexGuard<'_, T>> {
    match mutex.try_lock() {
        Ok(guard) => Some(guard),
        Err(TryLockError::Poisoned(poisoned)) => Some(poisoned.into_inner()),
        Err(TryLockError::WouldBlock) => None,
    }
}

/// Drop the references only `list` still holds; the newest is kept when
/// `keep_last` (the next pass follows it).
fn release(list: &mut VecDeque<Arc<Pass>>, keep_last: bool) {
    let last = list.len().saturating_sub(1);
    let mut i = 0;
    list.retain(|pass| {
        let keep = (keep_last && i == last) || Arc::strong_count(pass) > 1;
        i += 1;
        keep
    });
}

/// The audio-callback side: owns the [`Engine`].
#[derive(Debug)]
pub struct AudioSide {
    engine: Engine,
    link: Arc<Link>,
    /// The scope ring, written every callback whatever the UI holds.
    scope: Scope,
}

impl AudioSide {
    pub fn sample_rate(&self) -> f32 {
        self.engine.sample_rate()
    }

    /// Render one callback's worth of stereo frames.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        if let Some(mut inbox) = try_lock(&self.link.inbox) {
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
        // Both hold MAX_TRACKS values from the start: nothing to allocate.
        if let Some(mut levels) = try_lock(&self.link.levels) {
            for (level, peak) in levels.iter_mut().zip(self.engine.peaks()) {
                *level = *peak;
            }
        }
        for (l, r) in left.iter().zip(right.iter()) {
            self.scope.samples[self.scope.at] = (l + r) * 0.5;
            self.scope.at = (self.scope.at + 1) % SCOPE_SIZE;
        }
        if let Some(mut scope) = try_lock(&self.link.scope) {
            scope.samples.copy_from_slice(&self.scope.samples);
            scope.at = self.scope.at;
        }
        let wants = self.engine.wants_pass().map_or(0, |n| n + 1);
        self.link.published.write(self.engine.position(), wants);
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
    /// Banks replaced, kept while the engine may still hold them.
    old_banks: Vec<Arc<SampleBank>>,
    scales: Scales,
    synths: SynthPresets,
    master: MasterSettings,
    kit: String,
    rng: Rng,
    mode: Mode,
    /// Sound of each track, by pattern id then track index, with what it was
    /// decided from: auto-assignment picks at random, once per play.
    assigned: HashMap<String, Vec<Option<(SoundKey, TrackSound)>>>,
    /// Pattern repetitions drawn for the song passes.
    song_notes: HitsCache,
    /// Passes posted, oldest first, kept while the engine or the inbox may
    /// hold them (and the newest: the next pass follows it).
    sent: VecDeque<Arc<Pass>>,
    /// Previews posted, oldest first, kept while the engine or the inbox may hold them.
    previews: VecDeque<Arc<Pass>>,
    playing: bool,
}

/// What a track's sound depends on.
#[derive(Debug, Clone, PartialEq)]
struct SoundKey {
    name: String,
    auto_assign: bool,
    sample_path: String,
    use_synth: bool,
    synth_preset: Option<String>,
}

impl SoundKey {
    fn of(track: &Track) -> Self {
        Self {
            name: track.name.clone(),
            auto_assign: track.auto_assign,
            sample_path: track.sample_path.clone(),
            use_synth: track.use_synth,
            synth_preset: track.synth_preset.clone(),
        }
    }

    /// Whether `track` still has these settings (compared in place, no copy).
    fn matches(&self, track: &Track) -> bool {
        self.name == track.name
            && self.auto_assign == track.auto_assign
            && self.sample_path == track.sample_path
            && self.use_synth == track.use_synth
            && self.synth_preset == track.synth_preset
    }
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
            published: Published::default(),
        });
        let kit = bank.samples.first().map(|s| s.kit.clone()).unwrap_or_default();
        let audio =
            AudioSide { engine: Engine::new(sample_rate, bank.clone()), link: link.clone(), scope: Scope::default() };
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
        lock(&self.link.inbox).master = Some(master);
    }

    /// Per-track peak of the last rendered quantum (0–1), for VU meters.
    pub fn levels(&self) -> Vec<f32> {
        lock(&self.link.levels).clone()
    }

    /// The last [`SCOPE_SIZE`] samples played, oldest first, mono (silence
    /// before anything played).
    pub fn scope(&self, out: &mut [f32; SCOPE_SIZE]) {
        let scope = lock(&self.link.scope);
        let (newer, older) = scope.samples.split_at(scope.at);
        out[..older.len()].copy_from_slice(older);
        out[older.len()..].copy_from_slice(newer);
    }

    /// What each track of pattern `p` plays (assigned once, then kept).
    fn sounds(&mut self, library: &Library, p: usize) -> Vec<TrackSound> {
        let Some(pattern) = library.patterns.get(p) else { return Vec::new() };
        let rng = &mut self.rng;
        let mut pick = |n: usize| rng.below(n as u64) as usize;
        let slots = self.assigned.entry(pattern.id.clone()).or_default();
        if slots.len() < pattern.tracks.len() {
            slots.resize(pattern.tracks.len(), None);
        }
        pattern
            .tracks
            .iter()
            .zip(slots.iter_mut())
            .map(|(track, slot)| match slot {
                Some((key, sound)) if key.matches(track) => sound.clone(),
                _ => {
                    let sound = assign_track(track, &self.bank, &self.kit, &self.synths, &mut pick);
                    *slot = Some((SoundKey::of(track), sound.clone()));
                    sound
                }
            })
            .collect()
    }

    /// What track `t` of pattern `p` plays: what it was assigned, else
    /// (not played yet) the first pick of its assignment.
    pub fn track_sound(&self, library: &Library, p: usize, t: usize) -> TrackSound {
        let Some(pattern) = library.patterns.get(p) else { return TrackSound::None };
        let Some(track) = pattern.tracks.get(t) else { return TrackSound::None };
        match self.assigned.get(&pattern.id).and_then(|slots| slots.get(t)?.as_ref()) {
            Some((key, sound)) if key.matches(track) => sound.clone(),
            _ => assign_track(track, &self.bank, &self.kit, &self.synths, &mut |_| 0),
        }
    }

    /// The transport tick playing now (what the track LFOs are at); `None` when stopped.
    pub fn tick(&self) -> Option<u64> {
        let position = self.position()?;
        // The latest build of that pass: a rebuild replaced the earlier ones.
        let pass = self.sent.iter().rev().find(|p| p.number == position.pass)?;
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
        let pass = build_preview(pattern, t, note, context);
        if !pass.events.is_empty() {
            self.post_preview(pass);
        }
    }

    /// Post `pass` to sound at once, kept here until the engine is done with it.
    fn post_preview(&mut self, pass: Pass) {
        let pass = Arc::new(pass);
        release(&mut self.previews, false);
        self.previews.push_back(pass.clone());
        lock(&self.link.inbox).preview = Some(pass);
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
        self.post_preview(Pass {
            frames: 1,
            ticks: 1,
            events: vec![crate::pass::Event { frame: 0, sound: crate::pass::Sound::Sample(voice), mono: false }],
            strips: vec![ordrumbox_dsp::StripSettings::default()],
            ..Pass::default()
        });
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
        let mut inbox = lock(&self.link.inbox);
        inbox.command = Some(Command::Play(pass));
        inbox.next = None;
        drop(inbox);
        self.playing = true;
    }

    /// The library was edited: rebuild the pass playing now (it goes on from
    /// the same place), and the next ones follow the edit.
    pub fn refresh(&mut self, library: &Library) {
        if !self.playing {
            return;
        }
        let Some(Position { pass: number, .. }) = self.position() else { return };
        // A Play still waiting must not be replaced by a rebuild: none is
        // built. Only this thread posts a Play, so none appears meanwhile.
        if matches!(lock(&self.link.inbox).command, Some(Command::Play(_))) {
            return;
        }
        self.song_notes.clear();
        let Some(pass) = self.build(library, number) else { return };
        let mut inbox = lock(&self.link.inbox);
        inbox.command = Some(Command::Replace(pass));
        inbox.next = None;
    }

    pub fn stop(&mut self) {
        lock(&self.link.inbox).command = Some(Command::Stop);
        self.playing = false;
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// The pass [`Self::update`] builds now, if it builds one: the moment for
    /// the loop start of auto-mode tracks (a pass is a loop of the pattern).
    pub fn pending_pass(&self) -> Option<u64> {
        let wants = self.link.published.read().1.checked_sub(1)?;
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
            lock(&self.link.inbox).next = Some(pass);
        }
        release(&mut self.sent, true);
        release(&mut self.previews, false);
        self.old_banks.retain(|bank| Arc::strong_count(bank) > 1);
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
        lock(&self.link.inbox).bank = Some(bank);
        self.assigned.clear();
    }

    /// Playback position, `None` when stopped.
    pub fn position(&self) -> Option<Position> {
        self.link.published.read().0
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
        // The engine's pass and the queued one, at most, plus the newest.
        assert!(controller.sent.len() <= 3);
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
            let mut scope = lock(&controller.link.scope);
            let at = scope.at;
            scope.samples[at] = 7.0; // the oldest
            scope.samples[(at + SCOPE_SIZE - 1) % SCOPE_SIZE] = 9.0; // the newest
        }
        controller.scope(&mut out);
        assert_eq!((out[0], out[SCOPE_SIZE - 1]), (7.0, 9.0));
    }

    #[test]
    fn passes_are_released_only_from_their_last_holder() {
        let held = Arc::new(Pass::default()); // the engine's
        let mut sent: VecDeque<Arc<Pass>> = [held.clone(), Arc::new(Pass::default()), Arc::new(Pass::default())].into();
        release(&mut sent, true);
        assert_eq!(sent.len(), 2);
        assert!(Arc::ptr_eq(&sent[0], &held));
        drop(held);
        release(&mut sent, false);
        assert!(sent.is_empty());
    }

    #[test]
    fn the_position_is_read_as_written() {
        let published = Published::default();
        assert_eq!(published.read(), (None, 0));
        published.write(Some(Position { pass: 3, frame: 10, frames: 100 }), 4);
        assert_eq!(published.read(), (Some(Position { pass: 3, frame: 10, frames: 100 }), 4));
        published.write(None, 0);
        assert_eq!(published.read(), (None, 0));
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
