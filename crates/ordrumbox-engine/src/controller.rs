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
use std::sync::{Arc, Mutex};

use ordrumbox_core::song::clamp_bpm;
use ordrumbox_core::track::STRAIGHT_SWING;
use ordrumbox_core::{Library, Rng, Scales, Track};
use ordrumbox_dsp::MasterSettings;

use crate::assign::{TrackSound, assign_track};
use crate::engine::{Engine, HostTransport, MAX_TRACKS, Position, host_pass};
use crate::kit::SampleBank;
use crate::midi::{KeyMap, Target};
use crate::pass::{
    HitsCache, Pass, PassContext, SONG_PASS_TICKS, SongContext, build_keymap, build_pass, build_preview,
    build_song_pass_at, pattern_pass_ticks,
};
use crate::synth_preset::SynthPresets;

#[path = "controller/audio.rs"]
mod audio;
#[path = "controller/link.rs"]
mod link;
#[cfg(test)]
#[path = "controller/tests.rs"]
mod tests;

pub use audio::AudioSide;
pub use link::SCOPE_SIZE;
use link::*;

/// A host tempo change smaller than this is not one (passes are not rebuilt).
const TEMPO_EPSILON: f64 = 1e-6;
/// A host swing change smaller than this is not one (the pass is not rebuilt).
const SWING_EPSILON: f64 = 1e-4;

/// What plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Pattern `index`, looped at the library's tempo.
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
    /// The pattern of each pass built in pattern mode, by pass number, oldest
    /// first (a rebuild comes after the build it replaces).
    pass_patterns: VecDeque<(u64, usize)>,
    /// Previews posted, oldest first, kept while the engine or the inbox may hold them.
    previews: VecDeque<Arc<Pass>>,
    /// Keymaps posted, oldest first, kept while the engine or the inbox may
    /// hold them (and the newest: the engine plays with it).
    keymaps: VecDeque<Arc<KeyMap>>,
    playing: bool,
    /// The plugin host's tempo the passes are built at, once a host transport was seen.
    tempo: Option<f64>,
    /// A plugin host's swing on every track ([`PassContext::swing`]).
    swing: f64,
    /// What the cue posted was built from; `None`: none posted, or one to build again.
    cue: Option<CueKey>,
    /// The measures a song plays in a loop, `first..end` (`end` excluded).
    song_loop: Option<(u64, u64)>,
    /// A click on every beat of the passes built.
    metronome: bool,
    /// A MIDI input is open: the pattern's key map is posted for its notes, as under a host.
    live_midi: bool,
}

/// What a cue ([`Inbox::cue`]) is built from: a cue built from the same is
/// not built again.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CueKey {
    number: u64,
    mode: Mode,
    tempo: Option<f64>,
    swing: f64,
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
        let audio = AudioSide {
            engine: Engine::new(sample_rate, bank.clone()),
            link: link.clone(),
            scope: Scope::default(),
            block_start: 0,
            block_frames: 0,
        };
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
            pass_patterns: VecDeque::new(),
            previews: VecDeque::new(),
            keymaps: VecDeque::new(),
            playing: false,
            tempo: None,
            swing: STRAIGHT_SWING,
            cue: None,
            song_loop: None,
            metronome: false,
            live_midi: false,
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
        self.cue = None;
    }

    pub fn bank(&self) -> &Arc<SampleBank> {
        &self.bank
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Play pattern `index` from the next pass on (pattern mode): the pass
    /// playing ends first, and a next pass already posted is built again.
    pub fn select_pattern(&mut self, index: usize) {
        if let Mode::Pattern(_) = self.mode {
            self.mode = Mode::Pattern(index);
        }
    }

    /// The pattern of the pass playing now (pattern mode); `None` when
    /// stopped, in song mode, or before the first pass has started.
    pub fn playing_pattern(&self) -> Option<usize> {
        if !self.playing || !matches!(self.mode, Mode::Pattern(_)) {
            return None;
        }
        self.pattern_of(self.position()?.pass)
    }

    /// The pattern pass `number` was last built from.
    fn pattern_of(&self, number: u64) -> Option<usize> {
        self.pass_patterns.iter().rev().find(|(n, _)| *n == number).map(|(_, p)| *p)
    }

    /// Pattern `p` left the library: the later ones move down one. Playback
    /// stops when `p` plays or is the one to play next.
    pub fn remove_pattern(&mut self, p: usize) {
        let Mode::Pattern(selected) = self.mode else { return };
        if selected == p || self.pass_patterns.iter().any(|(_, q)| *q == p) {
            self.stop();
            self.pass_patterns.clear();
            return;
        }
        let shift = |q: usize| if q > p { q - 1 } else { q };
        self.mode = Mode::Pattern(shift(selected));
        for (_, q) in &mut self.pass_patterns {
            *q = shift(*q);
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
        self.build_of(library, number, self.mode)
    }

    /// The tempo `mode` plays at: the plugin host's, else the library's for
    /// a pattern and the song's own for a song.
    fn bpm_of(&self, library: &Library, mode: Mode) -> f64 {
        let own = match mode {
            Mode::Pattern(_) => library.bpm,
            Mode::Song(s) => library.songs.get(s).map_or(library.bpm, |song| song.bpm),
        };
        self.tempo.unwrap_or_else(|| clamp_bpm(own))
    }

    /// The measures of a song that play in a loop, `first..end`, or `None`
    /// to play on to the end. The pass playing is rebuilt, the next ones
    /// follow the region: a song that reaches `end` goes back to `first`.
    pub fn set_song_loop(&mut self, library: &Library, region: Option<(u64, u64)>) {
        let region = region.filter(|(first, end)| first < end);
        if self.song_loop != region {
            self.song_loop = region;
            self.refresh(library);
        }
    }

    /// The metronome on or off: the pass playing is rebuilt with it, the
    /// next ones follow.
    pub fn set_metronome(&mut self, library: &Library, on: bool) {
        if self.metronome != on {
            self.metronome = on;
            self.cue = None;
            self.refresh(library);
        }
    }

    /// A MIDI input is open (or not): the notes it plays reach the tracks
    /// of the pattern ([`AudioSide::trigger`]).
    pub fn set_live_midi(&mut self, on: bool) {
        self.live_midi = on;
    }

    /// The track of the pattern shown (the one playing, else the selected
    /// one) that MIDI key `key` of channel `channel` plays, and the pitch the
    /// note then has: what a note played live is recorded as. Like the
    /// engine's key map: a channel with a melodic track plays it, any other
    /// channel the drum keys.
    pub fn midi_track(&mut self, library: &Library, channel: u8, key: u8) -> Option<(usize, f64)> {
        let Mode::Pattern(selected) = self.mode else { return None };
        let p = self.playing_pattern().unwrap_or(selected);
        let pattern = library.patterns.get(p)?;
        let sounds = self.sounds(library, p);
        let targets = crate::pass::keymap_targets(pattern, &sounds, &self.bank);
        let melodic =
            targets.iter().position(|t| matches!(t, Some(Target::Melodic { channel: c, .. }) if *c == channel));
        if let Some(t) = melodic
            && let Some(Target::Melodic { root, .. }) = targets[t]
        {
            return Some((t, f64::from(key) - root));
        }
        targets.iter().position(|t| matches!(t, Some(Target::Drum(k)) if *k == key)).map(|t| (t, 0.0))
    }

    pub fn metronome(&self) -> bool {
        self.metronome
    }

    /// The song measure the pass of sequence `number` plays: the pass numbers
    /// only count up (the engine takes them in order), so the loop is made
    /// here, in what each pass holds.
    fn song_measure(&self, number: u64) -> u64 {
        match self.song_loop {
            Some((first, end)) if number >= end => first + (number - first) % (end - first),
            _ => number,
        }
    }

    /// Pass `number` of `mode`.
    fn build_of(&mut self, library: &Library, number: u64, mode: Mode) -> Option<Arc<Pass>> {
        let pass = match mode {
            Mode::Pattern(p) => {
                self.pass_patterns.push_back((number, p));
                let pattern = library.patterns.get(p)?;
                let sounds = self.sounds(library, p);
                let context = PassContext {
                    bank: &self.bank,
                    sounds: &sounds,
                    scales: &self.scales,
                    synths: &self.synths,
                    sample_rate: self.sample_rate,
                    first_tick: self.first_tick(number),
                    swing: self.swing,
                };
                let bpm = self.bpm_of(library, mode);
                build_pass(pattern, number, &mut self.rng, context, bpm)
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
                    swing: self.swing,
                };
                let bpm = self.bpm_of(library, mode);
                let measure = self.song_measure(number);
                let mut pass = build_song_pass_at(song, measure, &mut self.rng, &mut self.song_notes, context, bpm);
                pass.number = number;
                pass
            }
        };
        let mut pass = pass;
        if self.metronome {
            crate::metronome::add_clicks(&mut pass);
        }
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
            swing: self.swing,
        };
        let pass = build_preview(pattern, t, note, context, self.bpm_of(library, self.mode));
        if !pass.events.is_empty() {
            self.post_preview(pass);
        }
    }

    /// Post what MIDI keys play now (the pattern playing, else the one
    /// selected), when it is not what the engine has already: an edit, a
    /// new kit or tempo, another pattern. Nothing in song mode.
    fn post_keymap(&mut self, library: &Library) {
        let Mode::Pattern(selected) = self.mode else { return };
        let p = self.playing_pattern().unwrap_or(selected);
        let Some(pattern) = library.patterns.get(p) else { return };
        let sounds = self.sounds(library, p);
        let context = PassContext {
            bank: &self.bank,
            sounds: &sounds,
            scales: &self.scales,
            synths: &self.synths,
            sample_rate: self.sample_rate,
            first_tick: 0,
            swing: self.swing,
        };
        let keymap = build_keymap(pattern, context, self.bpm_of(library, self.mode));
        if self.keymaps.back().is_some_and(|posted| **posted == keymap) {
            return;
        }
        let keymap = Arc::new(keymap);
        self.keymaps.push_back(keymap.clone());
        lock(&self.link.inbox).keymap = Some(keymap);
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
            events: vec![crate::pass::Event {
                frame: 0,
                sound: crate::pass::Sound::Sample(voice),
                mono: false,
                velocity: 1.0,
                note: None,
                note_frames: 0,
            }],
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
        self.pass_patterns.clear();
        let Some(pass) = self.build(library, first) else { return };
        let mut inbox = lock(&self.link.inbox);
        inbox.command = Some(Command::Play(pass));
        inbox.next = None;
        inbox.cue = None;
        drop(inbox);
        self.playing = true;
        self.cue = None;
    }

    /// The library was edited: rebuild the pass playing now (it goes on from
    /// the same place), and the next ones follow the edit.
    pub fn refresh(&mut self, library: &Library) {
        // A cue posted is built again from the edit.
        self.cue = None;
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
        // The pass playing keeps its pattern: a pattern selected meanwhile waits for the next.
        let mode = match self.mode {
            Mode::Pattern(p) => Mode::Pattern(self.pattern_of(number).unwrap_or(p)),
            song => song,
        };
        let Some(pass) = self.build_of(library, number, mode) else { return };
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
        let wants = self.link.published.read().wants.checked_sub(1)?;
        // The pass after the last one posted: not one already sent, and not
        // one the engine asked for before a jump it has not taken yet.
        let follows = self.sent.back().is_none_or(|p| p.number + 1 == wants);
        (self.playing && follows).then_some(wants)
    }

    /// The next pass, already posted, when it was built from another
    /// pattern than the one selected since: it is built again.
    fn stale_next_pass(&self) -> Option<u64> {
        let Mode::Pattern(selected) = self.mode else { return None };
        let next = self.sent.back()?.number;
        let playing = self.position()?.pass;
        let waiting_play = matches!(lock(&self.link.inbox).command, Some(Command::Play(_)));
        (self.playing && next > playing && !waiting_play && self.pattern_of(next) != Some(selected)).then_some(next)
    }

    /// Call regularly (every UI frame) while playing: posts the next pass
    /// when the engine needs it, so tempo, kit and pattern changes take
    /// effect at the next pass boundary. Under a plugin host
    /// ([`AudioSide::render_synced`]), call it even while stopped: it starts,
    /// stops and moves playback with the host.
    pub fn update(&mut self, library: &Library) {
        let snapshot = self.link.published.read();
        if let Some(host) = snapshot.host {
            self.follow_host(library, host, snapshot);
        }
        if snapshot.host.is_some() || self.live_midi {
            self.post_keymap(library);
        }
        if let Some(number) = self.pending_pass().or_else(|| self.stale_next_pass())
            && let Some(pass) = self.build(library, number)
        {
            lock(&self.link.inbox).next = Some(pass);
        }
        release(&mut self.sent, true);
        // The lowest number held, not the first: a jump starts the numbers again.
        if let Some(oldest) = self.sent.iter().map(|p| p.number).min() {
            self.pass_patterns.retain(|(n, _)| *n >= oldest);
        }
        release(&mut self.previews, false);
        release(&mut self.keymaps, true);
        self.old_banks.retain(|bank| Arc::strong_count(bank) > 1);
    }

    /// A plugin host's swing on every track ([`PassContext::swing`]): when
    /// it changes, the pass playing is rebuilt with it.
    pub fn set_host_swing(&mut self, library: &Library, swing: f64) {
        if (self.swing - swing).abs() > SWING_EPSILON {
            self.swing = swing;
            self.refresh(library);
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
        lock(&self.link.inbox).bank = Some(bank);
        self.assigned.clear();
        self.cue = None;
    }

    /// Playback position, `None` when stopped.
    pub fn position(&self) -> Option<Position> {
        self.link.published.read().position
    }

    /// The plugin host's transport seen by the last callback, if it followed one.
    pub fn host(&self) -> Option<HostTransport> {
        self.link.published.read().host
    }

    /// Ticks in one pass of what plays.
    fn pass_ticks(&self, library: &Library) -> Option<u32> {
        match self.mode {
            Mode::Pattern(p) => library.patterns.get(p).map(pattern_pass_ticks),
            Mode::Song(s) => library.songs.get(s).map(|_| SONG_PASS_TICKS),
        }
    }

    /// Follow the plugin host's transport: its tempo for every pass, start
    /// and stop with it, and the pass it is in when it jumps (a loop, a
    /// click on its timeline). Stopped, the host is given a cue: the pass it
    /// would start in, which the engine starts on its own.
    fn follow_host(&mut self, library: &Library, host: HostTransport, snapshot: Snapshot) {
        let bpm = clamp_bpm(host.bpm);
        let new_tempo = self.tempo.is_none_or(|tempo| (tempo - bpm).abs() > TEMPO_EPSILON);
        if new_tempo {
            self.tempo = Some(bpm);
        }
        if !host.rolling() {
            if self.playing {
                self.stop();
                // The next play assigns the sounds afresh.
                self.assigned.clear();
                self.cue = None;
            }
            self.post_cue(library, host.beat);
            return;
        }
        // A Play still waiting is the host's pass already.
        if matches!(lock(&self.link.inbox).command, Some(Command::Play(_))) {
            return;
        }
        let cued = self.cue.is_some_and(|cue| snapshot.position.is_some_and(|at| at.pass == cue.number));
        if !self.playing && !snapshot.lost && cued {
            // The engine started from the cue: it plays on, at the tempo now.
            self.playing = true;
            self.cue = None;
            if new_tempo {
                self.refresh(library);
            }
        } else if !self.playing || snapshot.lost {
            let Some((number, _)) = self.pass_ticks(library).and_then(|ticks| host_pass(host.beat, ticks)) else {
                return;
            };
            if !self.playing {
                self.assigned.clear();
            }
            self.jump(library, number);
        } else if new_tempo {
            self.refresh(library);
        }
    }

    /// Under a stopped host at `beat`: the pass it would start in posted as
    /// the cue, unless the one posted is still there and built from the same.
    fn post_cue(&mut self, library: &Library, beat: f64) {
        let Some((number, _)) = self.pass_ticks(library).and_then(|ticks| host_pass(beat, ticks)) else { return };
        let key = CueKey { number, mode: self.mode, tempo: self.tempo, swing: self.swing };
        // The engine took a cue when the host started and stopped before this thread saw it.
        if self.cue == Some(key) && lock(&self.link.inbox).cue.is_some() {
            return;
        }
        self.song_notes.clear();
        self.pass_patterns.clear();
        let Some(pass) = self.build(library, number) else { return };
        lock(&self.link.inbox).cue = Some(pass);
        self.cue = Some(key);
    }
}
