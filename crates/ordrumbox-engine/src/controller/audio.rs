//! The audio-callback side of playback.

use super::link::{Command, Inbox, Link, SCOPE_SIZE, Scope, Snapshot, lock, try_lock};
use crate::engine::{Engine, HostMix, HostTransport, host_pass};
use crate::midi::BlockNote;
use std::sync::Arc;

/// The audio-callback side: owns the [`Engine`].
#[derive(Debug)]
pub struct AudioSide {
    pub(super) engine: Engine,
    pub(super) link: Arc<Link>,
    /// The scope ring, written every callback whatever the UI holds.
    pub(super) scope: Scope,
    /// The engine's clock at the start of the block being rendered (MIDI notes are timed from it).
    pub(super) block_start: u64,
    /// Frames in that block.
    pub(super) block_frames: usize,
}

impl AudioSide {
    pub fn sample_rate(&self) -> f32 {
        self.engine.sample_rate()
    }

    /// A plugin host's mix for the blocks to come ([`Engine::set_host_mix`]).
    pub fn set_host_mix(&mut self, mix: HostMix) {
        self.engine.set_host_mix(mix);
    }

    /// Play MIDI key `key` of channel `channel` at `velocity` (0–1),
    /// `offset` frames into the next block: the track of the pattern that
    /// has the note, at its pitch ([`crate::midi`]). Called before rendering the block.
    pub fn trigger(&mut self, channel: u8, key: u8, velocity: f32, offset: u32) {
        self.engine.trigger(channel, key, velocity, offset);
    }

    /// The MIDI notes the block just rendered sent, in time order, on the
    /// GM drum channel, each track on its key ([`crate::midi`]).
    pub fn take_notes(&mut self, send: impl FnMut(BlockNote)) {
        self.engine.take_notes(self.block_start, self.block_frames, send);
    }

    /// Render one callback's worth of stereo frames.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.take_inbox();
        self.engine.render(left, right);
        self.publish(left, right, None, false);
    }

    /// Render one block of a plugin host, following its transport: the
    /// engine stops when the host stops, and keeps to the host's position
    /// inside the pass it plays. When the host is in another pass, it says
    /// so, and the controller posts that pass ([`Controller::update`]).
    pub fn render_synced(&mut self, left: &mut [f32], right: &mut [f32], host: HostTransport) {
        self.render_synced_split(left, right, host, |_, _, _, _| {});
    }

    /// [`Self::render_synced`], the tracks the host's mix splits given to
    /// `aux` instead of the mix ([`Engine::render_split`]).
    pub fn render_synced_split(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        host: HostTransport,
        aux: impl FnMut(usize, usize, &[f32], &[f32]),
    ) {
        (self.block_start, self.block_frames) = (self.engine.clock(), left.len().min(right.len()));
        if let Some(mut inbox) = try_lock(&self.link.inbox) {
            take(&mut self.engine, &mut inbox, Some(host));
        }
        let lost = self.follow(host);
        self.engine.render_split(left, right, aux);
        self.publish(left, right, Some(host), lost);
    }

    /// Ready the block a plugin host is about to render, for a host that
    /// renders ahead of real time (a bounce): what the controller posted is
    /// taken, the host's transport followed and published, so that a
    /// [`Controller::update`] made now posts what this very block needs.
    /// [`Self::receive`] then takes it, and [`Self::render_synced`] renders.
    pub fn announce(&mut self, host: HostTransport) {
        self.receive(host);
        let lost = self.follow(host);
        self.link.published.write(self.snapshot(Some(host), lost));
    }

    /// Take what the controller posted, waiting for the lock: for a block
    /// rendered ahead of real time, which can wait ([`Self::announce`]).
    pub fn receive(&mut self, host: HostTransport) {
        let mut inbox = lock(&self.link.inbox);
        take(&mut self.engine, &mut inbox, Some(host));
    }

    /// Keep to the host's transport: stop with it, or move to its position
    /// in the pass playing. True when the host is in another pass (or none plays).
    pub(super) fn follow(&mut self, host: HostTransport) -> bool {
        if host.rolling() {
            return !self.engine.follow(host.beat);
        }
        if self.engine.is_playing() {
            self.engine.stop();
        }
        false
    }

    /// Take what the controller posted, if the lock is free.
    pub(super) fn take_inbox(&mut self) {
        if let Some(mut inbox) = try_lock(&self.link.inbox) {
            take(&mut self.engine, &mut inbox, None);
        }
    }

    /// Publish levels, scope and position after rendering `left` and `right`.
    pub(super) fn publish(&mut self, left: &[f32], right: &[f32], host: Option<HostTransport>, lost: bool) {
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
        self.link.published.write(self.snapshot(host, lost));
    }

    /// What the engine publishes now.
    pub(super) fn snapshot(&self, host: Option<HostTransport>, lost: bool) -> Snapshot {
        let wants = self.engine.wants_pass().map_or(0, |n| n + 1);
        Snapshot { position: self.engine.position(), wants, host, lost }
    }
}

/// What the controller posted into `inbox`, taken by `engine`; under a
/// plugin host that starts, the cue it posted when the host is at its pass.
pub(super) fn take(engine: &mut Engine, inbox: &mut Inbox, host: Option<HostTransport>) {
    if let Some(bank) = inbox.bank.take() {
        engine.set_bank(bank);
    }
    if let Some(master) = inbox.master.take() {
        engine.set_master(master);
    }
    match inbox.command.take() {
        Some(Command::Play(pass)) => engine.play(pass),
        Some(Command::Replace(pass)) => engine.replace(pass),
        Some(Command::Stop) => engine.stop(),
        None => {}
    }
    if let Some(next) = inbox.next.take() {
        engine.queue(next);
    }
    if let Some(preview) = inbox.preview.take() {
        engine.preview(preview);
    }
    if let Some(keymap) = inbox.keymap.take() {
        engine.set_keymap(keymap);
    }
    if let Some(host) = host.filter(|host| host.rolling() && !engine.is_playing())
        && let Some(cue) =
            inbox.cue.take_if(|cue| host_pass(host.beat, cue.ticks).is_some_and(|(n, _)| n == cue.number))
    {
        // Moved, not dropped: the controller keeps its own reference.
        engine.play(cue);
    }
}
