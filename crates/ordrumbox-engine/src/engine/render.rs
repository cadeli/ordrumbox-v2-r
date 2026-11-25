//! The render half of the engine: the quanta, the events that start voices
//! on their frame, the LFOs and the voices' mix into the strips.

use super::*;

impl Engine {
    /// Render `left.len()` frames (both slices the same length), replacing their content.
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.render_split(left, right, |_, _, _, _| {});
    }

    /// [`Self::render`], the tracks the live mix splits ([`HostMix::split`])
    /// left out of the mix and given to `aux` instead: the track, the frame
    /// of the block its samples start at, and its strip's output, left and
    /// right, which the master bus has not touched. A track whose strip is
    /// idle gives nothing: its output is silent there.
    pub fn render_split(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        mut aux: impl FnMut(usize, usize, &[f32], &[f32]),
    ) {
        let frames = left.len().min(right.len());
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(QUANTUM);
            self.render_quantum(&mut left[done..done + n], &mut right[done..done + n], done, &mut aux);
            done += n;
        }
        self.end_notes(Some(self.clock));
        if self.live_next >= self.live.len() {
            self.live.clear();
            self.live_next = 0;
        }
    }

    /// One quantum, `at` frames into the block.
    fn render_quantum(
        &mut self,
        out_left: &mut [f32],
        out_right: &mut [f32],
        at: usize,
        aux: &mut impl FnMut(usize, usize, &[f32], &[f32]),
    ) {
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

        // Strips into the bus, and into the returns by their sends.
        out_left.fill(0.0);
        out_right.fill(0.0);
        self.returns.clear();
        let [keep_fx, keep_mix, keep_lfo] = self.keep;
        let (host, ratios) = (&self.host, &self.cutoff_ratios);
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
            // The key is the level of another track's sound, before its strip.
            let mut key = [0.0_f32; QUANTUM];
            let key = channel.key.filter(|&k| k < strips).map(|k| {
                let range = k * QUANTUM..k * QUANTUM + n;
                let (key_l, key_r) = (&self.input_left[range.clone()], &self.input_right[range]);
                for (level, (l, r)) in key.iter_mut().zip(key_l.iter().zip(key_r)) {
                    *level = l.abs().max(r.abs());
                }
                &key[..n]
            });
            channel.strip.process(&channel.current, key, in_l, in_r, &mut strip_l[..n], &mut strip_r[..n]);
            self.returns.send(&channel.current, &strip_l[..n], &strip_r[..n]);
            let mut peak = 0.0_f32;
            if host_muted(host.split, track) {
                // On an output of its own: not in the mix.
                aux(track, at, &strip_l[..n], &strip_r[..n]);
                peak = strip_l[..n].iter().chain(&strip_r[..n]).fold(peak, |peak, s| peak.max(s.abs()));
            } else {
                for (out, s) in out_left.iter_mut().zip(&strip_l[..n]).chain(out_right.iter_mut().zip(&strip_r[..n])) {
                    *out += s;
                    peak = peak.max(s.abs());
                }
            }
            channel.idle = if silent_input && peak < IDLE_LEVEL { channel.idle.saturating_add(1) } else { 0 };
            self.peaks[track] = peak;
        }

        self.returns.render(out_left, out_right, host.echo_feedback, keep_fx, self.idle_quanta);

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
                        self.clock_out.restart();
                        self.current = Some(pass);
                    }
                }
                continue;
            }
            let now = self.clock + offset as u64;
            while let Some(event) = pass.events.get(self.cursor).filter(|e| e.frame <= self.frame) {
                self.start(event, &pass.patches, now, 1.0, 1.0);
                self.send_note(event, now);
                self.lock_strip(event, &pass);
                self.cursor += 1;
            }
            let next_stop = pass.events.get(self.cursor).map_or(pass.frames, |e| e.frame);
            let next_stop = self.clock_out.due(&pass, self.frame, now).map_or(next_stop, |pulse| pulse.min(next_stop));
            let frames_left = (next_stop - self.frame) as usize;
            self.current = Some(pass);
            return Some(frames_left);
        }
    }

    /// Set the strip of `event`'s track as its note locks it
    /// ([`StripLock`]): at once, like a voice starting, not ramped.
    fn lock_strip(&mut self, event: &Event, pass: &Pass) {
        let track = event.sound.track();
        let (Some(lock), Some(base), Some(channel)) =
            (&event.lock, pass.strips.get(track), self.channels.get_mut(track))
        else {
            return;
        };
        lock.apply(base, &mut channel.target);
        lock.apply(base, &mut channel.current);
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

    /// Release the voices a note of `event` on `track` cuts: the track's own
    /// when it is mono, and those of every track of its choke group.
    fn release_cut(&mut self, track: usize, event: &Event, now_seconds: f64) {
        let (mono, group) = (event.mono, event.choke_group);
        if !mono && group.is_none() {
            return;
        }
        let groups = &self.choke_groups;
        let cuts = |t: usize| (mono && t == track) || group.is_some_and(|g| groups.get(t) == Some(&Some(g)));
        let sample_rate = self.sample_rate;
        for voice in self.voices.iter_mut().filter(|v| v.is_active() && cuts(v.track())) {
            voice.release(sample_rate);
        }
        for synth in self.synths.iter_mut().filter(|v| v.is_active() && cuts(v.track())) {
            synth.release_at(now_seconds);
        }
    }

    /// Start `event` at clock frame `now`, its level scaled by `velocity`
    /// and its pitch by `ratio` (1 for a note as built), its synth patch
    /// taken from `patches`.
    pub(super) fn start(&mut self, event: &Event, patches: &[SynthPatch], now: u64, velocity: f32, ratio: f64) {
        let track = event.sound.track();
        if track >= MAX_TRACKS {
            return;
        }
        let now_seconds = self.seconds(now);
        self.choke_groups[track] = event.choke_group;
        self.release_cut(track, event, now_seconds);
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
                self.synth_samples[slot] = synth.sample;
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
        for (synth, sample) in self.synths.iter_mut().zip(&self.synth_samples).filter(|(v, _)| v.is_active()) {
            let base = synth.track() * QUANTUM;
            // The random wave of the oscillators reads the track's sample.
            let source = sample.and_then(|i| self.bank.get(i)).map_or(&[][..], |s| &s.buffer.left[..]);
            synth.render(
                self.sample_rate,
                first_frame,
                source,
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
