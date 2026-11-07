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

use ordrumbox_core::steps::TICK;
use ordrumbox_dsp::{MasterBus, MasterSettings, SampleVoice, Strip, StripSettings, SynthPatch, SynthVoice, VoiceStart};

use crate::kit::SampleBank;
use crate::midi::{BlockNote, CHANNEL_COUNT, KEY_COUNT, KeyMap, MidiNote, NoteOut};
use crate::pass::{Event, Pass, Sound};
use crate::strip_settings::{is_slap, saturation_amount, set_saturation};

/// Sample voices playing at once; the oldest is cut when a new one needs a slot.
pub const MAX_VOICES: usize = 64;
/// Synth voices playing at once (the JS synth voice pool size).
pub const MAX_SYNTH_VOICES: usize = 16;
pub use ordrumbox_core::track::MAX_TRACKS;
/// Frames per render quantum (Web Audio's).
pub const QUANTUM: usize = 128;
/// Time constants of parameter changes (`setTargetAtTime`), seconds: effect
/// settings (`RAMP_TIME`), volume and pan, LFO steps
/// (`LFO_SMOOTHING`).
const RAMP_FX: f32 = 0.02;
const RAMP_MIX: f32 = 0.01;
const RAMP_LFO: f32 = 0.005;
/// A strip with no input and an output under this level...
const IDLE_LEVEL: f32 = 1e-6;
/// ...for this long (longer than the longest delay) is not processed until
/// its input comes back.
const IDLE_SECONDS: f32 = 2.5;

/// Notes played live (MIDI in) one block can hold; more are not played.
pub const LIVE_NOTES: usize = 256;
/// MIDI notes (on and off) one block can send; more are not sent. A block
/// would need hundreds of notes on one frame range to fill it.
pub const NOTES_OUT: usize = 1024;

/// Frames the transport may drift from the host's position before it seeks.
pub const SEEK_TOLERANCE_FRAMES: u64 = 32;

/// A plugin host's transport at the start of a block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTransport {
    pub playing: bool,
    pub bpm: f64,
    /// Beats since the start of the host's timeline.
    pub beat: f64,
}

impl HostTransport {
    /// Playing at a place the transport can follow (not in a pre-roll before beat 0).
    pub fn rolling(&self) -> bool {
        self.playing && self.beat.is_finite() && self.beat >= 0.0
    }
}

/// The pass host beat `beat` falls in, passes of `ticks` ticks each, and how
/// far into it (0 to 1); `None` before beat 0 or for an empty pass.
pub fn host_pass(beat: f64, ticks: u32) -> Option<(u64, f64)> {
    if !beat.is_finite() || beat < 0.0 || ticks == 0 {
        return None;
    }
    let pass_beats = f64::from(ticks) / f64::from(TICK);
    let pass = (beat / pass_beats).floor();
    Some((pass as u64, (beat / pass_beats - pass).clamp(0.0, 1.0)))
}

// One bit per track in the host's mute mask.
const _: () = assert!(MAX_TRACKS <= u64::BITS as usize);

/// Whether a plugin host's mute mask `muted` (bit `t` for track `t`) mutes `track`.
pub fn host_muted(muted: u64, track: usize) -> bool {
    u32::try_from(track).ok().and_then(|t| muted.checked_shr(t)).is_some_and(|m| m & 1 == 1)
}

/// Where a note sent and not ended is kept in [`Engine`]'s held notes; `None` out of the MIDI range.
fn held_index(note: MidiNote) -> Option<usize> {
    let (channel, key) = (usize::from(note.channel), usize::from(note.key));
    (channel < CHANNEL_COUNT && key < KEY_COUNT).then_some(channel * KEY_COUNT + key)
}

/// Tracks a plugin host changes ([`HostTrack`]), from the first.
pub const HOST_TRACKS: usize = 16;

/// A plugin host's mix, on top of the pattern's, reached through the
/// strips' ramps.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostMix {
    /// A gain on the master volume (linear).
    pub gain: f32,
    /// The tracks muted: bit `t` for track `t`.
    pub muted: u64,
    /// The changes to each of the first tracks.
    pub tracks: [HostTrack; HOST_TRACKS],
}

impl Default for HostMix {
    fn default() -> Self {
        Self { gain: 1.0, muted: 0, tracks: [HostTrack::default(); HOST_TRACKS] }
    }
}

/// A plugin host's changes to one track, on top of its own settings: at
/// their defaults, none. The effect amounts are added to the track's (0 when
/// its LED is off) and kept within 0 to 1; an effect whose type is `none`,
/// or a filter of type `allpass`, stays off.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HostTrack {
    /// A gain on the volume (linear).
    pub gain: f32,
    /// Added to the pan, the sum kept within -1 to 1.
    pub pan: f32,
    /// Octaves the filter cutoff moves by.
    pub cutoff_octaves: f32,
    /// Added to the filter's Q.
    pub q: f32,
    /// Added to the reverb amount.
    pub reverb: f32,
    /// Added to the delay amount.
    pub delay: f32,
    /// Added to the delay feedback (kept within 0 to 0.95; a slap delay has none).
    pub feedback: f32,
    /// Added to the saturation amount.
    pub saturation: f32,
    /// Semitones the notes started move by (not the MIDI notes sent).
    pub transpose: i32,
}

impl Default for HostTrack {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            cutoff_octaves: 0.0,
            q: 0.0,
            reverb: 0.0,
            delay: 0.0,
            feedback: 0.0,
            saturation: 0.0,
            transpose: 0,
        }
    }
}

/// The highest delay feedback a host can reach (the editor's).
const HOST_FEEDBACK_MAX: f32 = 0.95;

/// What a [`HostTrack`] multiplies, computed when the host's mix changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HostRatios {
    cutoff: f32,
    pitch: f64,
}

impl HostRatios {
    const NONE: Self = Self { cutoff: 1.0, pitch: 1.0 };

    fn of(track: &HostTrack) -> Self {
        Self { cutoff: track.cutoff_octaves.exp2(), pitch: (f64::from(track.transpose) / 12.0).exp2() }
    }
}

impl HostMix {
    /// The volume the strip of `track`, at `volume`, goes to: none when
    /// muted, else scaled by the track's gain.
    #[must_use]
    fn volume(&self, volume: f32, track: usize) -> f32 {
        if host_muted(self.muted, track) { 0.0 } else { volume * self.tracks.get(track).map_or(1.0, |t| t.gain) }
    }

    /// The settings the strip of `track`, at `target`, goes to, its cutoff
    /// multiplied by `ratios.cutoff`.
    #[must_use]
    fn strip(&self, target: &StripSettings, track: usize, ratios: &HostRatios) -> StripSettings {
        let mut s = *target;
        s.volume = self.volume(target.volume, track);
        let Some(host) = self.tracks.get(track) else { return s };
        let add = |value: f32, offset: f32, max: f32| (value + offset).clamp(0.0, max);
        s.pan = (s.pan + host.pan).clamp(-1.0, 1.0);
        if ratios.cutoff != 1.0 {
            s.cutoff = (s.cutoff * ratios.cutoff).clamp(MIN_CUTOFF, MAX_CUTOFF);
        }
        if host.q != 0.0 {
            s.q = (s.q + host.q).clamp(MIN_Q, MAX_Q);
        }
        if host.reverb != 0.0 {
            s.reverb_mix = add(s.reverb_mix, host.reverb, 1.0);
        }
        if host.delay != 0.0 {
            s.delay_mix = add(s.delay_mix, host.delay, 1.0);
        }
        if host.feedback != 0.0 && !is_slap(&s) {
            s.delay_feedback = add(s.delay_feedback, host.feedback, HOST_FEEDBACK_MAX);
        }
        if host.saturation != 0.0 {
            let amount = (saturation_amount(&s) + f64::from(host.saturation)).clamp(0.0, 1.0);
            set_saturation(&mut s, amount);
        }
        s
    }
}

/// The filter's range, as the strip takes it.
const MIN_CUTOFF: f32 = ordrumbox_dsp::MIN_FILTER_FREQ as f32;
const MAX_CUTOFF: f32 = ordrumbox_dsp::MAX_FILTER_FREQ as f32;
const MIN_Q: f32 = 0.707;
const MAX_Q: f32 = 18.707;

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

/// A note played live, to start on its clock frame.
#[derive(Debug, Clone, Copy)]
struct LiveNote {
    clock: u64,
    /// Looked up when the note starts: the keymap may change before.
    note: MidiNote,
    /// 0–1, scaling the event's level.
    velocity: f32,
}

/// A MIDI note sent and not ended yet.
#[derive(Debug, Clone, Copy)]
struct Held {
    /// When it started and when it ends, in clock frames.
    on: u64,
    off: u64,
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
    /// A plugin host's mix.
    host: HostMix,
    /// What its tracks' changes multiply.
    host_ratios: [HostRatios; HOST_TRACKS],
    /// `exp(-quantum / τ)` for each ramp.
    keep: [f32; 3],
    idle_quanta: u32,
    /// What MIDI keys play live; the caller keeps a reference.
    keymap: Option<Arc<KeyMap>>,
    /// Notes played live, by clock, the first `live_next` started.
    live: Vec<LiveNote>,
    live_next: usize,
    /// MIDI notes sent and not taken yet ([`Self::take_notes`]).
    notes_out: Vec<NoteOut>,
    /// The note each key sounds, by key.
    held: Vec<Option<Held>>,
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
            host: HostMix::default(),
            host_ratios: [HostRatios::NONE; HOST_TRACKS],
            keep: [RAMP_FX, RAMP_MIX, RAMP_LFO].map(|tau| (-quantum_seconds / tau).exp()),
            idle_quanta: (IDLE_SECONDS / quantum_seconds).ceil() as u32,
            keymap: None,
            live: Vec::with_capacity(LIVE_NOTES),
            live_next: 0,
            notes_out: Vec::with_capacity(NOTES_OUT),
            held: vec![None; CHANNEL_COUNT * KEY_COUNT],
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

    /// A plugin host's mix, for the blocks to come.
    pub fn set_host_mix(&mut self, mix: HostMix) {
        // The ratios cost an `exp2` each: computed only for a track that changed.
        for ((ratios, new), old) in self.host_ratios.iter_mut().zip(&mix.tracks).zip(&self.host.tracks) {
            if new != old {
                *ratios = HostRatios::of(new);
            }
        }
        self.host = mix;
    }

    /// The pitch ratio a plugin host gives the notes of `track`.
    fn host_pitch(&self, track: usize) -> f64 {
        self.host_ratios.get(track).map_or(1.0, |r| r.pitch)
    }

    /// The settings the strip of `track` goes to, the host's changes on top
    /// of the track's.
    fn host_target(
        host: &HostMix,
        ratios: &[HostRatios; HOST_TRACKS],
        channel: &Channel,
        track: usize,
    ) -> StripSettings {
        host.strip(&channel.target, track, ratios.get(track).unwrap_or(&HostRatios::NONE))
    }

    /// Start playing `pass` from its first frame; its strip settings apply at once.
    pub fn play(&mut self, pass: Arc<Pass>) {
        self.pass_number = pass.number;
        self.load(pass);
        self.next = None;
        self.settle_strips(self.strip_count);
    }

    /// The first `count` strips at their targets at once, as the host mixes them.
    fn settle_strips(&mut self, count: usize) {
        for (track, channel) in self.channels[..count].iter_mut().enumerate() {
            channel.current = Self::host_target(&self.host, &self.host_ratios, channel, track);
        }
    }

    /// The strips of `strips`, at once, when the transport is stopped (a
    /// playing pass keeps its own).
    fn take_strips_when_stopped(&mut self, strips: &[StripSettings]) {
        if self.current.is_some() {
            return;
        }
        self.strip_count = self.strip_count.max(strips.len()).min(MAX_TRACKS);
        for (channel, settings) in self.channels.iter_mut().zip(strips) {
            channel.target = *settings;
            channel.lfo = [false; 4];
        }
        self.settle_strips(strips.len().min(MAX_TRACKS));
    }

    /// Start the events of `pass` now, outside the transport (auditioning a
    /// track or a note). When stopped, its strip settings apply at once.
    /// The caller keeps a reference to `pass` (this drops its own).
    pub fn preview(&mut self, pass: Arc<Pass>) {
        self.take_strips_when_stopped(&pass.strips);
        for event in &pass.events {
            self.start(event, &pass.patches, self.clock, 1.0, 1.0);
        }
    }

    /// What MIDI keys play from now on ([`Self::trigger`]). When stopped,
    /// its strip settings apply at once. The caller keeps a reference to
    /// `keymap` (this drops the one it replaces).
    pub fn set_keymap(&mut self, keymap: Arc<KeyMap>) {
        self.take_strips_when_stopped(&keymap.pass.strips);
        self.keymap = Some(keymap);
    }

    /// Play key `key` of channel `channel` live at `velocity` (0–1),
    /// `offset` frames into the next rendered block, with the keymap the
    /// engine has then: nothing when no track has the note, or when the
    /// block already holds [`LIVE_NOTES`] notes.
    pub fn trigger(&mut self, channel: u8, key: u8, velocity: f32, offset: u32) {
        if self.live.len() == self.live.capacity() {
            return;
        }
        // In the order they come: a note never starts before the one before it.
        let clock = (self.clock + u64::from(offset)).max(self.live.last().map_or(0, |n| n.clock));
        let note = MidiNote { channel, key };
        self.live.push(LiveNote { clock, note, velocity: velocity.clamp(0.0, 1.0) });
    }

    /// Frames rendered since the engine was made.
    pub fn clock(&self) -> u64 {
        self.clock
    }

    /// Give the MIDI notes sent since the last call to `send`, in time
    /// order (a note-off before a note-on on the same frame), timed from
    /// clock frame `block_start` up to `block_frames` frames.
    pub fn take_notes(&mut self, block_start: u64, block_frames: usize, mut send: impl FnMut(BlockNote)) {
        self.notes_out.sort_unstable_by_key(|n| (n.clock, n.velocity.is_some()));
        let last = u32::try_from(block_frames.saturating_sub(1)).unwrap_or(u32::MAX);
        for note in self.notes_out.drain(..) {
            let timing = u32::try_from(note.clock.saturating_sub(block_start)).unwrap_or(u32::MAX).min(last);
            let MidiNote { channel, key } = note.note;
            send(BlockNote { timing, channel, key, velocity: note.velocity });
        }
    }

    /// Queue a MIDI note, unless [`NOTES_OUT`] are waiting already.
    fn send(&mut self, note: NoteOut) {
        if self.notes_out.len() < self.notes_out.capacity() {
            self.notes_out.push(note);
        }
    }

    /// Send `event`'s MIDI note at clock frame `now`, ended `note_frames`
    /// later; a note still sounding is ended first. A note of a track the
    /// host mutes, or too quiet to be a MIDI note, is not sent, nor a second
    /// one on the same channel, key and frame.
    fn send_note(&mut self, event: &Event, pass: &Pass, now: u64) {
        let Some(note) = event.note else { return };
        let velocity = event.velocity.clamp(0.0, 1.0);
        if host_muted(self.host.muted, event.sound.track()) || (velocity * 127.0).round() < 1.0 {
            return;
        }
        let Some(index) = held_index(note) else { return };
        match self.held[index] {
            Some(Held { on, .. }) if on == now => return,
            Some(Held { off, .. }) => self.send(NoteOut { clock: off.min(now), note, velocity: None }),
            None => {}
        }
        self.held[index] = Some(Held { on: now, off: now + pass.note_frames });
        self.send(NoteOut { clock: now, note, velocity: Some(velocity) });
    }

    /// End the MIDI notes due before clock frame `until` (all of them with `None`).
    fn end_notes(&mut self, until: Option<u64>) {
        for channel in (0..=u8::MAX).take(CHANNEL_COUNT) {
            for key in (0..=u8::MAX).take(KEY_COUNT) {
                let note = MidiNote { channel, key };
                let Some(index) = held_index(note) else { continue };
                let Some(Held { off, .. }) = self.held[index] else { continue };
                if until.is_none_or(|until| off < until) {
                    self.held[index] = None;
                    let clock = until.map_or(self.clock, |_| off);
                    self.send(NoteOut { clock, note, velocity: None });
                }
            }
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
        self.end_notes(None);
    }

    pub fn is_playing(&self) -> bool {
        self.current.is_some()
    }

    /// Keep the transport on host beat `beat`: a drift of more than
    /// [`SEEK_TOLERANCE_FRAMES`] inside the playing pass moves playback
    /// there. False when stopped or when the host is in another pass (the
    /// controller posts that one).
    pub fn follow(&mut self, beat: f64) -> bool {
        let Some(pass) = &self.current else { return false };
        let Some((mut number, fraction)) = host_pass(beat, pass.ticks) else { return false };
        let mut frame = (fraction * pass.frames as f64).round() as u64;
        if frame >= pass.frames {
            number += 1;
            frame = 0;
        }
        let at = |n: u64, f: u64| u128::from(n) * u128::from(pass.frames) + u128::from(f);
        if at(self.pass_number, self.frame).abs_diff(at(number, frame)) <= u128::from(SEEK_TOLERANCE_FRAMES) {
            return true;
        }
        if number != self.pass_number {
            return false;
        }
        self.cursor = pass.events.partition_point(|e| e.frame < frame);
        self.frame = frame;
        true
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
        self.end_notes(Some(self.clock));
        if self.live_next >= self.live.len() {
            self.live.clear();
            self.live_next = 0;
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
            let until = self.start_live_notes(offset).map_or(until, |frames_left| until.min(offset + frames_left));
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
        let (host, ratios) = (&self.host, &self.host_ratios);
        for (track, channel) in self.channels[..strips].iter_mut().enumerate() {
            let target = Self::host_target(host, ratios, channel, track);
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
        let target = MasterSettings { master: self.master_target.master * self.host.gain, ..self.master_target };
        self.master_current.approach(&target, keep_fx);
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
                self.start(event, &pass.patches, now, 1.0, 1.0);
                self.send_note(event, &pass, now);
                self.cursor += 1;
            }
            let next_stop = pass.events.get(self.cursor).map_or(pass.frames, |e| e.frame);
            let frames_left = (next_stop - self.frame) as usize;
            self.current = Some(pass);
            return Some(frames_left);
        }
    }

    /// Start the live notes due at `offset` in the quantum; frames until
    /// the next one, `None` when none is left.
    fn start_live_notes(&mut self, offset: usize) -> Option<usize> {
        let now = self.clock + offset as u64;
        // Taken out and put back, so no reference count changes here.
        let keymap = self.keymap.take();
        while let Some(note) = self.live.get(self.live_next).copied().filter(|n| n.clock <= now) {
            if let Some(keymap) = &keymap
                && let Some((event, ratio)) = keymap.event(note.note.channel, note.note.key)
                && let Some(event) = keymap.pass.events.get(event)
            {
                self.start(event, &keymap.pass.patches, now, note.velocity, ratio);
            }
            self.live_next += 1;
        }
        self.keymap = keymap;
        let next = self.live.get(self.live_next)?;
        usize::try_from(next.clock.saturating_sub(now)).ok()
    }

    /// Set the strip targets from the LFO values of the tick playing.
    fn apply_lfos(&mut self) {
        let Some(pass) = &self.current else { return };
        let tick = pass.tick_at(self.frame);
        for lfo in pass.lfos.iter().filter(|l| l.track < MAX_TRACKS) {
            lfo.apply(tick, &mut self.channels[lfo.track].target);
        }
    }

    /// Start `event` at clock frame `now`, its level scaled by `velocity`
    /// and its pitch by `ratio` (1 for a note as built), its synth patch
    /// taken from `patches`.
    fn start(&mut self, event: &Event, patches: &[SynthPatch], now: u64, velocity: f32, ratio: f64) {
        let track = event.sound.track();
        if track >= MAX_TRACKS {
            return;
        }
        let ratio = ratio * self.host_pitch(track);
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
                let params = VoiceStart { peak: params.peak * velocity, ratio: params.ratio * ratio, ..params };
                self.voices[slot].start(&params, &sample.buffer, self.sample_rate, self.serial);
            }
            Sound::Synth(synth) => {
                let Some(&patch) = patches.get(synth.patch) else { return };
                let osc_freq = patch.osc_freq.map(|f| f * ratio);
                let patch = SynthPatch { velocity: patch.velocity * f64::from(velocity), osc_freq, ..patch };
                let slot = free_or_oldest(&self.synths, SynthVoice::is_active, SynthVoice::serial);
                let voice = &mut self.synths[slot];
                voice.start(&patch, track, now_seconds, self.serial);
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
    use crate::midi::{DRUM_CHANNEL, Target};
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
        Event { frame, sound: Sound::Sample(voice), mono: true, velocity: 1.0, note: None }
    }

    fn pass(number: u64, frames: &[u64], length: u64) -> Arc<Pass> {
        let events = frames.iter().map(|&frame| sample_event(frame)).collect();
        Arc::new(Pass {
            number,
            frames: length,
            ticks: TICK,
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
    fn the_host_mix_mutes_scales_and_pans_tracks_and_scales_the_master() {
        let long = || SampleBuffer::mono(vec![1.0; 48_000], 48_000);
        // The peak of the left output once `settle` frames are past.
        let level = |mix: HostMix, settle: usize| {
            let mut engine = engine();
            engine.bank = Arc::new(SampleBank {
                samples: vec![crate::kit::BankSample { buffer: long().into(), ..engine.bank.samples[0].clone() }],
            });
            engine.set_host_mix(mix);
            engine.play(pass(0, &[0], 48_000));
            let signal = render(&mut engine, settle + 128);
            signal[settle..].iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
        };
        let mixed = |change: fn(&mut HostMix)| {
            let mut mix = HostMix::default();
            change(&mut mix);
            mix
        };
        let full = level(HostMix::default(), 2000);
        assert!(full > 0.1);
        assert_eq!(level(mixed(|m| m.muted = 0b1), 0), 0.0, "a muted track is silent from the start");
        assert_eq!(level(mixed(|m| m.muted = 0b10), 2000), full, "another track's mute changes nothing");
        // The master's 20 ms ramp has settled after 0.25 s.
        assert!((level(mixed(|m| m.gain = 0.5), 12_000) / level(HostMix::default(), 12_000) - 0.5).abs() < 1e-3);
        let half = level(mixed(|m| m.tracks[0].gain = 0.5), 2000);
        assert!((half / full - 0.5).abs() < 1e-3, "a track's gain scales it: {half} / {full}");
        assert_eq!(level(mixed(|m| m.tracks[1].gain = 0.0), 2000), full, "another track's gain changes nothing");
        assert!(level(mixed(|m| m.tracks[0].pan = 1.0), 0) < 1e-3, "panned right, from the start");
    }

    #[test]
    fn the_host_moves_the_effects_of_its_tracks_and_leaves_the_rest() {
        let ratios = |host: &HostMix, t: usize| HostRatios::of(&host.tracks[t]);
        let target = StripSettings { cutoff: 1000.0, delay_mode: 1.0, ..StripSettings::default() };
        let none = HostMix::default();
        assert_eq!(none.strip(&target, 0, &ratios(&none, 0)), target, "the defaults change nothing");
        let mut host = HostMix::default();
        host.tracks[0] = HostTrack {
            cutoff_octaves: 1.0,
            q: 2.0,
            reverb: 0.5,
            delay: 2.0,
            feedback: 0.25,
            saturation: 0.5,
            ..HostTrack::default()
        };
        let s = host.strip(&target, 0, &ratios(&host, 0));
        assert_eq!((s.cutoff, s.q), (2000.0, 2.707));
        assert_eq!((s.reverb_mix, s.delay_mix, s.delay_feedback), (0.5, 1.0, 0.65), "amounts kept within 0 to 1");
        assert_eq!((s.saturation_drive, s.saturation_mix), (4.0, 1.0));
        assert_eq!(saturation_amount(&s), 0.5);
        assert_eq!(host.strip(&target, 1, &ratios(&host, 1)), target, "another track changes nothing");
        let slap = StripSettings { delay_mode: 0.0, delay_feedback: 0.0, ..target };
        assert_eq!(host.strip(&slap, 0, &ratios(&host, 0)).delay_feedback, 0.0, "a slap has one echo");
        host.tracks[0].cutoff_octaves = 8.0;
        assert_eq!(host.strip(&target, 0, &ratios(&host, 0)).cutoff, MAX_CUTOFF);
    }

    #[test]
    fn the_host_transposes_the_notes_of_its_tracks() {
        // Frames a 100-frame sample sounds, at a host transpose of `semitones`.
        let sounding = |semitones: i32| {
            let mut engine = engine();
            let mut host = HostMix::default();
            host.tracks[0].transpose = semitones;
            engine.set_host_mix(host);
            engine.play(pass(0, &[0], 48_000));
            render(&mut engine, 400).iter().filter(|s| s.abs() > 0.01).count()
        };
        let (straight, octave_up) = (sounding(0), sounding(12));
        assert!(straight >= 95 && octave_up.abs_diff(straight / 2) <= 4, "{straight} then {octave_up} frames");
    }

    #[test]
    fn synth_notes_play_for_their_length() {
        let mut engine = engine();
        let patch = SynthPatch { velocity: 0.5, volume: 1.0, ..SynthPatch::default() };
        let event = Event {
            frame: 64,
            sound: Sound::Synth(SynthStart { track: 0, patch: 0, length: 0.01 }),
            mono: false,
            velocity: 1.0,
            note: None,
        };
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

    /// A keymap with one event on track 0, going to `target`.
    fn keymap(target: Target) -> Arc<KeyMap> {
        let pass = (*pass(0, &[0], 1)).clone();
        Arc::new(KeyMap::new(pass, &[Some(target)]))
    }

    /// `pass` with each event sending the note of the same index in `notes`, a 16th of 100 frames.
    fn sending(pass: Arc<Pass>, notes: &[MidiNote]) -> Arc<Pass> {
        let mut pass = (*pass).clone();
        for (event, &note) in pass.events.iter_mut().zip(notes) {
            event.note = Some(note);
        }
        pass.note_frames = 100;
        Arc::new(pass)
    }

    const KICK: MidiNote = MidiNote { channel: DRUM_CHANNEL, key: 36 };

    fn notes(engine: &mut Engine, block_start: u64, frames: usize) -> Vec<(u32, u8, Option<f32>)> {
        let mut notes = Vec::new();
        engine.take_notes(block_start, frames, |n| {
            assert_eq!(n.channel, DRUM_CHANNEL);
            notes.push((n.timing, n.key, n.velocity));
        });
        notes
    }

    /// Frames of `signal` that sound.
    fn sounding(signal: &[f32]) -> usize {
        signal.iter().filter(|s| s.abs() > 1e-6).count()
    }

    #[test]
    fn a_live_note_starts_on_its_frame_at_its_velocity() {
        let mut engine = engine();
        engine.trigger(DRUM_CHANNEL, 36, 1.0, 10);
        assert!(render(&mut engine, 256).iter().all(|&s| s == 0.0), "no keymap: nothing plays");
        engine.set_keymap(keymap(Target::Drum(36)));
        engine.trigger(DRUM_CHANNEL, 37, 1.0, 0);
        engine.trigger(DRUM_CHANNEL, 36, 1.0, 200);
        let loud = render(&mut engine, 512);
        assert_eq!(onsets(&loud), [200], "frame 200, across quanta; key 37 plays nothing");

        engine.trigger(DRUM_CHANNEL, 36, 1.0, 300);
        engine.trigger(DRUM_CHANNEL, 36, 1.0, 20);
        let clocks: Vec<u64> = engine.live.iter().map(|n| n.clock).collect();
        assert_eq!(clocks, [1068, 1068], "a note never starts before the one before it");

        let mut quiet = self::engine();
        quiet.set_keymap(keymap(Target::Drum(36)));
        quiet.trigger(3, 36, 0.5, 200);
        let soft = render(&mut quiet, 512);
        assert!((soft[300] / loud[300] - 0.5).abs() < 1e-4, "half the velocity, half the level, from any channel");
    }

    #[test]
    fn a_live_note_on_a_tracks_channel_plays_it_at_the_notes_pitch() {
        let length = |channel: u8, key: u8| {
            let mut engine = engine();
            engine.set_keymap(keymap(Target::Melodic { channel: 2, root: 48.0 }));
            engine.trigger(channel, key, 1.0, 0);
            sounding(&render(&mut engine, 512))
        };
        let (root, octave) = (length(2, 48), length(2, 60));
        assert!(root > 100, "the sample's 100 frames and a release: {root}");
        assert!((root - octave).abs_diff(50) <= 2, "an octave up, twice as fast: 50 frames of the sample, not 100");
        assert_eq!(length(DRUM_CHANNEL, 48), 0, "not on the drum channel");
    }

    #[test]
    fn notes_go_out_on_their_key_and_end_a_16th_later_or_at_stop() {
        let mut engine = engine();
        engine.play(sending(pass(0, &[3, 700], 1000), &[KICK, KICK]));
        render(&mut engine, 512);
        assert_eq!(notes(&mut engine, 0, 512), [(3, 36, Some(1.0)), (103, 36, None)]);
        render(&mut engine, 256);
        engine.stop();
        assert_eq!(notes(&mut engine, 512, 256), [(188, 36, Some(1.0)), (255, 36, None)], "ended at stop");
    }

    #[test]
    fn a_note_struck_again_ends_first_and_a_muted_track_sends_none() {
        let mut engine = engine();
        let played = sending(pass(0, &[3, 50], 1000), &[KICK, KICK]);
        engine.play(played.clone());
        render(&mut engine, 128);
        assert_eq!(notes(&mut engine, 0, 128), [(3, 36, Some(1.0)), (50, 36, None), (50, 36, Some(1.0))]);

        let mut muted = self::engine();
        muted.set_host_mix(HostMix { muted: 0b1, ..HostMix::default() });
        muted.play(played);
        render(&mut muted, 128);
        assert_eq!(notes(&mut muted, 0, 128), []);
    }

    #[test]
    fn notes_go_out_on_their_channel_and_pitch_each_held_on_its_own() {
        let mut engine = engine();
        let (c, e, d) =
            (MidiNote { channel: 2, key: 48 }, MidiNote { channel: 2, key: 52 }, MidiNote { channel: 3, key: 48 });
        engine.play(sending(pass(0, &[0, 10, 20, 30], 1000), &[c, e, d, c]));
        render(&mut engine, 256);
        let mut sent = Vec::new();
        engine.take_notes(0, 256, |n| sent.push((n.timing, n.channel, n.key, n.velocity.is_some())));
        assert_eq!(
            sent,
            [(0, 2, 48, true), (10, 2, 52, true), (20, 3, 48, true), (30, 2, 48, false), (30, 2, 48, true),]
                .into_iter()
                .chain([(110, 2, 52, false), (120, 3, 48, false), (130, 2, 48, false)])
                .collect::<Vec<_>>(),
            "only the same channel and key ends a note early"
        );
    }

    #[test]
    fn host_beats_fall_in_passes() {
        let four_beats = 4 * TICK;
        assert_eq!(host_pass(0.0, four_beats), Some((0, 0.0)));
        assert_eq!(host_pass(5.0, four_beats), Some((1, 0.25)));
        assert_eq!(host_pass(8.0, four_beats), Some((2, 0.0)));
        assert_eq!(host_pass(-0.5, four_beats), None, "a pre-roll");
        assert_eq!(host_pass(f64::NAN, four_beats), None);
        assert_eq!(host_pass(1.0, 0), None);
        let pre_roll = HostTransport { playing: true, bpm: 120.0, beat: -1.0 };
        assert!(!pre_roll.rolling() && HostTransport { beat: 0.0, ..pre_roll }.rolling());
    }

    #[test]
    fn follow_seeks_inside_the_pass_and_tells_another_pass() {
        // `pass` lasts one beat (`TICK` ticks) in 1000 frames.
        let mut engine = engine();
        assert!(!engine.follow(0.0), "stopped");
        engine.play(pass(0, &[0, 600], 1000));
        assert!(engine.follow(0.01), "10 frames off: within the tolerance");
        assert_eq!(engine.position().map(|p| p.frame), Some(0));
        assert!(engine.follow(0.5));
        assert_eq!(engine.position().map(|p| p.frame), Some(500));
        let signal = render(&mut engine, 200);
        assert_eq!(onsets(&signal), [100], "frame 600 of the pass, frame 0 skipped");
        assert!(engine.follow(0.7), "where it is");
        assert!(!engine.follow(3.2), "pass 3: the controller posts it");
        assert_eq!(engine.position().map(|p| p.frame), Some(700));
        // Across the end of a pass, the frames are compared, not the pass numbers.
        render(&mut engine, 290);
        assert!(engine.follow(0.999_999));
        assert_eq!(engine.position().map(|p| (p.pass, p.frame)), Some((0, 990)));
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
