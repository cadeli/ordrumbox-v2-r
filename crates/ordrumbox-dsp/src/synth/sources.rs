//! What the sources of a voice play each sample: the frequencies of the
//! three oscillators (detune, pitch envelope, FM), their output with
//! unison and the hard sync of oscillator 2, and the noise.

use super::*;

impl SynthVoice {
    /// The frequencies of the three oscillators for this sample, Hz: detune,
    /// the ratio of the pitch envelope and the glide (`pitch_env`), FM and what the `late` values
    /// of the matrix add (pitch, more FM), which also raise `gains` (shape).
    #[inline]
    pub(super) fn frequencies(
        &self,
        p: &SynthPatch,
        b: &Block,
        mods: &Mods,
        gains: &mut [f64; 3],
        (late, pitch_env): (&LateMods, f64),
    ) -> [f64; 3] {
        let mut detuned = [0.0; 3];
        for (n, frequency) in detuned.iter_mut().enumerate() {
            *frequency = p.osc_freq[n] * cents_ratio(modulated(mods, p.osc_detune[n], DETUNE_SLOTS[n]));
        }
        if pitch_env != 1.0 {
            detuned = detuned.map(|frequency| frequency * pitch_env);
        }

        let fm_amount = clamp(modulated(mods, p.fm_amount, Slot::FmAmount), 0.0, 1.0);
        let algorithm = clamp(modulated(mods, p.fm_algorithm.round(), Slot::FmAlgorithm).round(), 0.0, 4.0);
        let carriers = (detuned[0], detuned[1]);
        let [mut f1, mut f2, mut f3] = detuned;
        if !p.bypass.fm && fm_amount > SILENT {
            (f1, f2) = fm_frequencies(fm_amount, algorithm, carriers, self.raw_oscillators(p, b, detuned));
        }
        let pitch = late[Late::Pitch];
        if pitch.abs() > SILENT {
            let ratio = ((pitch * 1200.0) * LN2_OVER_1200).exp();
            [f1, f2, f3] = [f1 * ratio, f2 * ratio, f3 * ratio];
        }
        let boost = fm_amount * late[Late::FmBoost];
        if !p.bypass.fm && boost > SILENT {
            let (a, b) = fm_frequencies(boost, algorithm, carriers, self.raw_oscillators(p, b, detuned));
            f1 += a - carriers.0;
            f2 += b - carriers.1;
        }
        let shape = late[Late::Shape];
        if shape.abs() > SILENT {
            *gains = gains.map(|gain| clamp(gain + shape * (1.0 - gain), 0.0, 1.0));
        }
        [f1, f2, f3]
    }

    /// The frequency ratio of the pitch envelope this sample; it then moves on by a sample.
    #[inline]
    pub(super) fn pitch_env_ratio(&mut self, p: &SynthPatch, b: &Block, mods: &Mods) -> f64 {
        let fall = if is_moved(mods, Slot::PitchEnvDecay) {
            fall_per_sample(modulated(mods, p.pitch_env.decay, Slot::PitchEnvDecay), b.sample_rate)
        } else {
            b.pitch_fall
        };
        self.pitch_fall.step(modulated(mods, p.pitch_env.amount, Slot::PitchEnvAmount), fall)
    }

    /// The three oscillators where they stand, at full gain, but 0 for one
    /// switched off: what FM modulates with.
    #[inline]
    pub(super) fn raw_oscillators(&self, p: &SynthPatch, b: &Block, frequencies: [f64; 3]) -> [f64; 3] {
        let mut raw = [0.0; 3];
        for (n, value) in raw.iter_mut().enumerate() {
            if p.bypass.osc[n] {
                continue;
            }
            let dt = (frequencies[n] / b.sample_rate).min(0.49);
            *value = oscillator(b.tables, p.osc_wave[n], self.phase[n][0], dt, self.osc_cycle[n][0], self.width[n]);
        }
        raw
    }

    /// Move the oscillators on by a sample at `frequencies` and mix them,
    /// with the sub-oscillator, at `gains`: the left and right samples, the
    /// same unless the layers of a unison are spread. Oscillator 2 is
    /// synced to oscillator 1 and ring-modulated by it, if the patch says so.
    #[inline]
    pub(super) fn oscillators(
        &mut self,
        p: &SynthPatch,
        b: &Block,
        mods: &Mods,
        frequencies: [f64; 3],
        gains: [f64; 3],
    ) -> [f64; 2] {
        let sub_gain = if p.bypass.osc[0] { 0.0 } else { clamp(modulated(mods, p.sub_gain, Slot::SubGain), 0.0, 1.0) };
        let ring_amount = if b.has_ring { clamp(modulated(mods, p.ring, Slot::Ring), 0.0, 1.0) } else { 0.0 };
        let mut sum = if b.layer_count > 1 {
            self.layered_oscillators(p, b, frequencies, gains, ring_amount)
        } else {
            let mono = self.single_oscillators(p, b, frequencies, gains, ring_amount);
            [mono, mono]
        };
        // One cycle over two of oscillator 1: an octave below it.
        if sub_gain > SILENT {
            let sub =
                sin_lookup(b.tables.sine, (self.osc_cycle[0][0].rem_euclid(2.0) + self.phase[0][0]) * 0.5) * sub_gain;
            sum = sum.map(|side| side + sub);
        }
        sum
    }

    /// The oscillators of a unison, moved on by a sample and mixed at
    /// `gains`, left and right; oscillator 2 ring-modulated at `ring_amount`.
    #[inline]
    pub(super) fn layered_oscillators(
        &mut self,
        p: &SynthPatch,
        b: &Block,
        frequencies: [f64; 3],
        gains: [f64; 3],
        ring_amount: f64,
    ) -> [f64; 2] {
        let is_ringing = ring_amount > SILENT;
        // Oscillator 1 moves on first: where it stood tells whether it began a cycle.
        let osc1_cycles_before = self.osc_cycle[0];
        // Each oscillator at full gain, left and right.
        let mut layered = [[0.0; 2]; 3];
        for (n, out) in layered.iter_mut().enumerate() {
            // An oscillator at no gain only keeps its first layer going,
            // for FM and the sub, unless oscillator 2 follows its layers.
            let drives_osc2 = n == 0 && (is_ringing || b.is_synced);
            let used = if gains[n] > 0.0 || drives_osc2 { b.layer_count } else { 1 };
            if n == 1 && b.is_synced {
                self.sync_osc2(b, &b.layers[..used], &osc1_cycles_before, frequencies);
            }
            *out = unison_oscillator(
                b.tables,
                (p.osc_wave[n], self.width[n]),
                &mut self.phase[n][..used],
                &mut self.osc_cycle[n][..used],
                &b.layers[..used],
                frequencies[n],
                b.sample_rate,
            );
        }
        if is_ringing {
            layered[1] = [0, 1].map(|side| ring(layered[1][side], layered[0][side], ring_amount));
        }
        let mut sum = [0.0; 2];
        for (out, gain) in layered.iter().zip(gains) {
            sum[0] += out[0] * gain;
            sum[1] += out[1] * gain;
        }
        sum
    }

    /// The three oscillators of one layer each, moved on by a sample and
    /// mixed at `gains`; oscillator 2 ring-modulated at `ring_amount`.
    #[inline]
    pub(super) fn single_oscillators(
        &mut self,
        p: &SynthPatch,
        b: &Block,
        frequencies: [f64; 3],
        gains: [f64; 3],
        ring_amount: f64,
    ) -> f64 {
        let osc1_cycles_before = [self.osc_cycle[0][0]];
        let o1 = self.single_oscillator(p, b, 0, frequencies[0]);
        if b.is_synced {
            self.sync_osc2(b, &b.layers[..1], &osc1_cycles_before, frequencies);
        }
        let mut outputs =
            [o1, self.single_oscillator(p, b, 1, frequencies[1]), self.single_oscillator(p, b, 2, frequencies[2])];
        if ring_amount > SILENT {
            outputs[1] = ring(outputs[1], outputs[0], ring_amount);
        }
        outputs[0] * gains[0] + outputs[1] * gains[1] + outputs[2] * gains[2]
    }

    /// Oscillator `n` of one layer moved on by a sample at `frequency` Hz, at full gain.
    #[inline]
    pub(super) fn single_oscillator(&mut self, p: &SynthPatch, b: &Block, n: usize, frequency: f64) -> f64 {
        let dt = phase_step(frequency, b.sample_rate);
        let advanced = self.phase[n][0] + dt;
        self.osc_cycle[n][0] += advanced.floor();
        self.phase[n][0] = advanced.rem_euclid(1.0);
        oscillator(b.tables, p.osc_wave[n], self.phase[n][0], dt.abs(), self.osc_cycle[n][0], self.width[n])
    }

    /// Restart the layers of oscillator 2 whose layer of oscillator 1 began
    /// a cycle in this sample, which oscillator 1 has just been moved on by
    /// from `osc1_cycles_before`.
    #[inline]
    pub(super) fn sync_osc2(&mut self, b: &Block, layers: &[Layer], osc1_cycles_before: &[f64], [f1, f2, _]: [f64; 3]) {
        let used = layers.len();
        let osc1 = Moved {
            phases: &self.phase[0][..used],
            cycles: &self.osc_cycle[0][..used],
            cycles_before: &osc1_cycles_before[..used],
        };
        let starts = cycle_starts(osc1, layers, f1, b.sample_rate);
        let [_, osc2, _] = &mut self.phase;
        let [_, cycles2, _] = &mut self.osc_cycle;
        restart((&mut osc2[..used], &mut cycles2[..used]), &starts, layers, f2, b.sample_rate);
    }

    /// The next raw noise sample, -1 to 1: white, or the shift register's
    /// output, held until its next step.
    #[inline]
    pub(super) fn noise_source(&mut self, p: &SynthPatch, b: &Block, mods: &Mods) -> f64 {
        if b.noise_mode == NoiseMode::White {
            return noise_sample(self.rng);
        }
        self.lfsr_phase += if is_moved(mods, Slot::NoiseRate) {
            clamp(modulated(mods, p.noise_rate, Slot::NoiseRate) / b.sample_rate, 0.0, 1.0)
        } else {
            b.noise_step
        };
        if self.lfsr_phase >= 1.0 {
            self.lfsr_phase -= 1.0;
            self.lfsr = lfsr_step(self.lfsr, b.noise_mode == NoiseMode::Short);
            self.noise_held = lfsr_sample(self.lfsr);
        }
        self.noise_held
    }

    /// The next sample of filtered noise, at its level in the mix.
    #[inline]
    pub(super) fn noise(&mut self, p: &SynthPatch, b: &Block, mods: &Mods) -> f64 {
        self.rng = xorshift32(self.rng);
        if p.bypass.noise {
            return 0.0;
        }
        let level = clamp(modulated(mods, p.noise_mix, Slot::NoiseMix), 0.0, 1.0);
        let raw = self.noise_source(p, b, mods) * level;
        let freq = modulated(mods, p.noise_filter_freq, Slot::NoiseFreq);
        let q = clamp(modulated(mods, p.noise_filter_q, Slot::NoiseQ), 0.1, 20.0);
        let (g, k) = if freq != p.noise_filter_freq || q != p.noise_filter_q {
            (cutoff_coefficient(freq, b.sample_rate), 1.0 / q)
        } else {
            (b.noise_g, b.noise_k)
        };
        tap(self.noise_filter.step(raw, &coefficients(g, k)), b.noise_weights)
    }
}
