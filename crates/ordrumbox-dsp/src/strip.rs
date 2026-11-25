//! A track's channel strip: a stereo
//! TPT state-variable filter (12 dB/oct, or two in series for 24 dB/oct),
//! a compressor, saturation, a bitcrusher, a hard clipper, a Freeverb reverb (with a pre-delay, a spring's dispersion and a
//! gate), a delay (tape wow, ping-pong), then volume and compensated
//! equal-power pan.
//!
//! The settings added since the first version default to what it did, so
//! the fixtures play them unset. The reverb's tunings follow the sample rate (Freeverb's lengths are
//! given at 44.1 kHz), and the `tape` saturation is asymmetric.
//!
//! The reverb and delay memories are allocated by [`Strip::new`];
//! [`Strip::process`] never allocates.

use crate::denormal::flush;
use crate::svf::Svf;

use blocks::{
    ClipBlock, Compressor, CrushBlock, DelayBlock, Ducker, FilterBlock, PanBlock, ReverbBlock, SaturationBlock,
};
use lines::{Allpass, Comb, DelayLine, Dispersion};
use stages::{mix_out, saturate};

mod blocks;
mod curves;
mod lines;
mod stages;

const COMB_TUNINGS_L: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const COMB_TUNINGS_R: [usize; 8] = [1139, 1211, 1300, 1379, 1445, 1514, 1580, 1640];
const ALLPASS_TUNINGS_L: [usize; 4] = [556, 441, 341, 225];
const ALLPASS_TUNINGS_R: [usize; 4] = [579, 464, 364, 248];
/// Rate the Freeverb tunings are given at.
const TUNING_RATE: f64 = 44_100.0;
/// Longest reverb pre-delay, seconds.
const PREDELAY_SECONDS: f64 = 0.1;
/// First-order allpasses in series that spread a spring's input in time.
const DISPERSION_STAGES: usize = 8;
/// Input level (|left| + |right|) that opens the reverb gate.
const GATE_KEY: f64 = 0.05;
/// Time constants of the reverb gate, seconds: opening, closing.
const GATE_OPEN_SECONDS: f64 = 0.001;
const GATE_CLOSE_SECONDS: f64 = 0.01;
/// Rate of the tape delay's wow, Hz.
const WOW_HZ: f64 = 0.6;
/// Bit depth of the bitcrusher at the smallest amount, and at full amount.
const CRUSH_BITS_MAX: f64 = 16.0;
const CRUSH_BITS_MIN: f64 = 4.0;
/// Rate the bitcrusher holds samples at, at the smallest amount, Hz; full
/// amount lowers it by [`CRUSH_OCTAVES`] (to about 1.4 kHz).
const CRUSH_RATE_HZ: f64 = 44_100.0;
const CRUSH_OCTAVES: f64 = 5.0;
/// Gain the hard clipper drives into its ceiling at full amount, dB.
const CLIP_DRIVE_DB: f64 = 18.0;
/// The hard clipper's ceiling: −3 dBFS, the peak of a kit sample, so the
/// clipper adds density, not peak level.
const CLIP_CEILING: f64 = 0.707_945_784_384_137_9;
/// Longest delay, seconds (the `dlyTime*` parameters stop at 2).
const DELAY_SECONDS: f64 = 2.1;
/// The delay line is sized for 2.1 s at 48 kHz at least, whatever the rate.
const DELAY_MIN_RATE: f64 = 48_000.0;

audio_params! {
    /// The strip processor's parameters (its `parameterDescriptors`).
    pub struct StripSettings {
        /// 0 skips the filter (the `allpass` filter type), 1 filters.
        filter_on = 1.0, 0.0, 1.0;
        /// Filter cutoff, Hz.
        cutoff = crate::MAX_FILTER_FREQ as f32, crate::MIN_FILTER_FREQ as f32, crate::MAX_FILTER_FREQ as f32;
        q = crate::MIN_FILTER_Q as f32, crate::MIN_FILTER_Q as f32, crate::MAX_FILTER_Q as f32;
        /// 0 lowpass, 1 highpass, 2 bandpass, 3 notch.
        filter_mode = 0.0, 0.0, 3.0;
        /// dB per octave: 12 (one stage) or 24 (two stages).
        filter_slope = 12.0, 12.0, 24.0;
        /// Compressor, after the filter: its dry/wet (parallel compression),
        /// 0 skips it.
        comp_mix = 0.0, 0.0, 1.0;
        /// Level, dBFS, above which the compressor lowers the gain.
        comp_threshold_db = 0.0, -60.0, 0.0;
        /// dB in for one dB out above the threshold.
        comp_ratio = 4.0, 1.0, 20.0;
        /// Seconds the gain reduction takes to set in, and to let go.
        comp_attack_seconds = 0.01, 0.0001, 0.2;
        comp_release_seconds = 0.1, 0.01, 2.0;
        /// 0 tanh, 1 hard clip, 2 tape (an asymmetric tanh).
        saturation_type = 0.0, 0.0, 2.0;
        saturation_drive = 1.0, 1.0, 7.0;
        saturation_output = 1.0, 0.0, 1.0;
        saturation_mix = 0.0, 0.0, 1.0;
        /// Bitcrusher amount: 0 off; up to 1, fewer bits (16 to 4) and a
        /// lower rate (44.1 kHz to about 1.4 kHz).
        crush = 0.0, 0.0, 1.0;
        /// Hard clipper amount: 0 off; up to 1, a drive of up to
        /// [`CLIP_DRIVE_DB`] into a ceiling of −3 dBFS, anti-aliased.
        clip = 0.0, 0.0, 1.0;
        reverb_room = 0.5, 0.0, 1.0;
        reverb_damping = 0.5, 0.0, 1.0;
        reverb_width = 1.0, 0.0, 1.0;
        /// 0 skips the reverb (the `none` reverb type), 1 lets `reverb_mix` through.
        reverb_on = 1.0, 0.0, 1.0;
        reverb_mix = 0.0, 0.0, 1.0;
        /// Seconds before the reverb's input enters it.
        reverb_predelay = 0.0, 0.0, 0.1;
        /// Spread of a spring (allpass coefficient), 0 none.
        reverb_dispersion = 0.0, 0.0, 0.9;
        /// Seconds the reverb stays open after its input falls under the
        /// gate's key, then it closes (gated reverb); 0 no gate.
        reverb_gate = 0.0, 0.0, 1.0;
        /// Delay times, seconds.
        delay_time_l = 0.25, 0.0, 2.0;
        delay_time_r = 0.25, 0.0, 2.0;
        delay_feedback = 0.4, 0.0, 0.99;
        /// 0 skips the delay (the `none` delay type), 1 lets `delay_mix` through.
        delay_on = 1.0, 0.0, 1.0;
        delay_mix = 0.0, 0.0, 1.0;
        /// 0 and 1 each side repeats itself, 2 the sides feed each other
        /// (ping-pong).
        delay_mode = 1.0, 0.0, 2.0;
        /// Coefficient of the one-pole lowpass in the feedback: lower is darker.
        delay_tone = 0.5, 0.05, 1.0;
        /// Depth of the tape wow, seconds of delay time.
        delay_wow = 0.0, 0.0, 0.005;
        /// 1 feeds the mono sum into the left line only (a ping-pong starts
        /// on the left), 0 each side into its line.
        delay_mono_in = 0.0, 0.0, 1.0;
        /// Sidechain ducking: the share of the gain a key at full level
        /// takes off, 0 none (it needs a key, see [`Strip::process`]).
        duck_amount = 0.0, 0.0, 1.0;
        /// Seconds the gain takes to come back once the key falls.
        duck_release_seconds = 0.15, 0.01, 2.0;
        /// Level of the strip's own signal in its output: 0 for a return,
        /// which plays only its reverb and delay.
        dry = 1.0, 0.0, 1.0;
        volume = 1.0, 0.0, 2.0;
        pan = 0.0, -1.0, 1.0;
        /// Sends of the strip's output (after volume and pan) to the shared
        /// echo and reverb returns. The strip does not read them: the engine
        /// feeds the returns with them ([`crate::returns`]).
        echo_send = 0.0, 0.0, 1.0;
        reverb_send = 0.0, 0.0, 1.0;
    }
}

/// State of one strip.
#[derive(Debug, Clone)]
pub struct Strip {
    sample_rate: f64,
    /// Filter stages: [left, right] of the first, then of the second
    /// (24 dB/oct only).
    filter: [Svf; 2],
    filter_2: [Svf; 2],
    combs_l: [Comb; 8],
    combs_r: [Comb; 8],
    allpass_l: [Allpass; 4],
    allpass_r: [Allpass; 4],
    predelay: DelayLine,
    dispersion: [Dispersion; DISPERSION_STAGES],
    /// Frames the gate stays open, and its gain.
    gate_hold: f64,
    gate_gain: f64,
    delay_l: DelayLine,
    delay_r: DelayLine,
    delay_filter_l: f64,
    delay_filter_r: f64,
    /// Phase of the wow, cycles.
    wow_phase: f64,
    /// Phase of the bitcrusher's sample and hold, samples (a new one at 1),
    /// and the samples held, left and right.
    crush_phase: f64,
    crush_hold: [f64; 2],
    /// The hard clipper's previous input, left and right, over its ceiling.
    clip_prev: [f64; 2],
    /// The compressor's gain reduction, dB (0 or less).
    comp_reduction_db: f64,
    /// The sidechain ducking's envelope of the key, linear.
    duck_envelope: f64,
}

impl Strip {
    /// A silent strip at `sample_rate` (allocates its reverb and delay memories).
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = f64::from(sample_rate);
        let delay_len = (DELAY_SECONDS * sample_rate.max(DELAY_MIN_RATE)).ceil() as usize;
        // The same reverb times at any rate.
        let tuned = |len: usize| ((len as f64 * sample_rate / TUNING_RATE).round() as usize).max(1);
        Self {
            sample_rate,
            filter: [Svf::default(); 2],
            filter_2: [Svf::default(); 2],
            combs_l: COMB_TUNINGS_L.map(|len| Comb::new(tuned(len))),
            combs_r: COMB_TUNINGS_R.map(|len| Comb::new(tuned(len))),
            allpass_l: ALLPASS_TUNINGS_L.map(|len| Allpass::new(tuned(len))),
            allpass_r: ALLPASS_TUNINGS_R.map(|len| Allpass::new(tuned(len))),
            predelay: DelayLine::new((PREDELAY_SECONDS * sample_rate).ceil() as usize + 2),
            dispersion: [Dispersion::default(); DISPERSION_STAGES],
            gate_hold: 0.0,
            gate_gain: 0.0,
            delay_l: DelayLine::new(delay_len),
            delay_r: DelayLine::new(delay_len),
            delay_filter_l: 0.0,
            delay_filter_r: 0.0,
            wow_phase: 0.0,
            crush_phase: 1.0,
            crush_hold: [0.0; 2],
            clip_prev: [0.0; 2],
            comp_reduction_db: 0.0,
            duck_envelope: 0.0,
        }
    }

    /// Process `in_l`/`in_r` with `settings` and ADD the result to
    /// `out_l`/`out_r` (all four the same length).
    ///
    /// `key` is a sidechain: the level, per frame, of another track. When it
    /// is given (and as long as the input), the compressor reads it instead of
    /// the strip's own signal, and `duck_amount` lowers the gain by it.
    pub fn process(
        &mut self,
        settings: &StripSettings,
        key: Option<&[f32]>,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        match key.and_then(|key| key.get(..in_l.len())) {
            Some(key) => self.run::<true>(settings, key, in_l, in_r, out_l, out_r),
            None => self.run::<false>(settings, &[], in_l, in_r, out_l, out_r),
        }
    }

    /// [`Self::process`], with the key's case chosen once for the block.
    fn run<const KEYED: bool>(
        &mut self,
        settings: &StripSettings,
        key: &[f32],
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
    ) {
        let s = settings.clamped();
        let sr = self.sample_rate;
        let filter = FilterBlock::new(&s, sr);
        let saturation = SaturationBlock::new(&s);
        let crush = CrushBlock::new(&s, sr);
        let clip = ClipBlock::new(&s);
        let reverb = ReverbBlock::new(&s, sr);
        let delay = DelayBlock::new(&s, sr);
        let pan = PanBlock::new(f64::from(s.pan));
        let volume = f64::from(s.volume);
        let dry_level = f64::from(s.dry);
        let comp = Compressor::new(&s, sr);
        let duck = if KEYED { Ducker::new(&s, sr) } else { None };

        // A stage turned back on starts from silence, or released.
        if clip.is_none() {
            self.clip_prev = [0.0; 2];
        }
        if comp.is_none() {
            self.comp_reduction_db = 0.0;
        }
        if duck.is_none() {
            self.duck_envelope = 0.0;
        }
        if filter.stage_2.is_none() {
            self.filter_2 = [Svf::default(); 2];
        }

        let inputs = in_l.iter().zip(in_r);
        let outputs = out_l.iter_mut().zip(out_r.iter_mut());
        for (frame, ((&x_l, &x_r), (o_l, o_r))) in inputs.zip(outputs).enumerate() {
            let (mut dry_l, mut dry_r) = self.filter_stage(&filter, f64::from(x_l), f64::from(x_r));

            if let Some(comp) = &comp {
                let peak = if KEYED { f64::from(key[frame]) } else { dry_l.abs().max(dry_r.abs()) };
                (dry_l, dry_r) = comp.process(&mut self.comp_reduction_db, peak, dry_l, dry_r);
            }

            let mut sat_l = saturate(saturation.as_ref(), dry_l);
            let mut sat_r = saturate(saturation.as_ref(), dry_r);
            if let Some(crush) = &crush {
                (sat_l, sat_r) = self.crush_stage(crush, sat_l, sat_r);
            }
            if let Some(clip) = &clip {
                (sat_l, sat_r) = self.clip_stage(clip, sat_l, sat_r);
            }

            let rev = reverb.as_ref().map_or((0.0, 0.0), |reverb| self.reverb_stage(reverb, sat_l, sat_r));
            let wet = self.delay_stage(&delay, sat_l, sat_r);

            let duck_gain = duck.map_or(1.0, |duck| duck.gain(&mut self.duck_envelope, f64::from(key[frame])));
            let (left, right) = mix_out(&pan, volume * duck_gain, (sat_l * dry_level, sat_r * dry_level), rev, wet);
            *o_l += left;
            *o_r += right;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::blocks::COMP_MAKEUP_SHARE;
    use super::*;

    fn run(settings: &StripSettings, input: &[f32]) -> (Vec<f32>, Vec<f32>) {
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(settings, None, input, input, &mut l, &mut r);
        (l, r)
    }

    #[test]
    fn defaults_pass_the_signal_through() {
        let input: Vec<f32> = (0..256).map(|i| if i % 50 == 0 { 0.5 } else { 0.0 }).collect();
        let (l, r) = run(&StripSettings::default(), &input);
        assert_eq!(l, r);
        // 20 kHz lowpass at 48 kHz: the energy stays.
        let energy = |s: &[f32]| s.iter().map(|x| x * x).sum::<f32>();
        assert!((energy(&l) / energy(&input) - 1.0).abs() < 0.2);
    }

    /// Gain in dB of the strip on a sine of `freq` Hz, measured once settled.
    fn gain_db(settings: &StripSettings, freq: f64) -> f64 {
        let input: Vec<f32> = (0..24_000).map(|i| (TAU * freq * f64::from(i) / 48_000.0).sin() as f32).collect();
        let (l, _) = run(settings, &input);
        let energy = |s: &[f32]| s[12_000..].iter().map(|x| f64::from(*x).powi(2)).sum::<f64>();
        10.0 * (energy(&l) / energy(&input)).log10()
    }

    #[test]
    fn slopes_of_12_and_24_db() {
        for (mode, (near, far)) in [(0.0, (2_000.0, 4_000.0)), (1.0, (500.0, 250.0))] {
            let one = StripSettings { cutoff: 1_000.0, filter_mode: mode, ..StripSettings::default() };
            let two = StripSettings { filter_slope: 24.0, ..one };
            for (settings, slope) in [(one, 12.0), (two, 24.0)] {
                assert!((gain_db(&settings, 1_000.0) + 3.0).abs() < 0.3, "mode {mode}: -3 dB at the cutoff");
                let octave = gain_db(&settings, near) - gain_db(&settings, far);
                assert!((octave - slope).abs() < 1.5, "mode {mode}, {slope} dB/oct: {octave}");
            }
        }
    }

    #[test]
    fn resonance_peaks_as_high_at_24_db() {
        let one = StripSettings { cutoff: 1_000.0, q: 4.0, ..StripSettings::default() };
        let two = StripSettings { filter_slope: 24.0, ..one };
        assert!((gain_db(&one, 1_000.0) - gain_db(&two, 1_000.0)).abs() < 0.5);
        let bandpass = StripSettings { filter_mode: 2.0, ..two };
        assert!((gain_db(&bandpass, 1_000.0) - gain_db(&one, 1_000.0)).abs() < 0.5);
    }

    #[test]
    fn hard_pan_and_volume() {
        let input = vec![0.25; 64];
        let settings = StripSettings { pan: 1.0, volume: 2.0, ..StripSettings::default() };
        let (l, r) = run(&settings, &input);
        assert!(l[63].abs() < 1e-6);
        assert!((r[63] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn reverb_tail_outlasts_the_input() {
        let mut input = vec![0.0; 6000];
        input[0] = 1.0;
        let settings = StripSettings { reverb_mix: 0.5, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        assert!(l[3000..].iter().any(|s| s.abs() > 1e-4));
    }

    /// Frame of the loudest sample of `s` from `from` on.
    fn loudest(s: &[f32], from: usize) -> usize {
        s[from..].iter().enumerate().max_by(|a, b| a.1.abs().total_cmp(&b.1.abs())).unwrap().0 + from
    }

    #[test]
    fn a_bypassed_filter_leaves_the_signal_as_it_is() {
        let input: Vec<f32> = (0..512).map(|i| ((i * 7919) % 200) as f32 / 100.0 - 1.0).collect();
        let settings = StripSettings { filter_on: 0.0, cutoff: 200.0, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        assert_eq!(l, input);
    }

    #[test]
    fn full_crush_holds_samples_on_4_bit_levels() {
        let input: Vec<f32> = (0..480).map(|i| (f64::from(i) / 480.0 - 0.5) as f32).collect();
        let crushed = StripSettings { filter_on: 0.0, crush: 1.0, ..StripSettings::default() };
        let (l, r) = run(&crushed, &input);
        assert_eq!(l, r);
        // 4 bits: 8 levels on each side of 0.
        for x in &l {
            let level = f64::from(*x) * 8.0;
            assert!((level - level.round()).abs() < 1e-4, "{x} is not on a level");
        }
        // Held at about 1.4 kHz: a new value every 35 frames at 48 kHz.
        let changes = l.windows(2).filter(|w| w[0] != w[1]).count();
        assert!((5..=14).contains(&changes), "{changes} changes");
        let (plain, _) = run(&StripSettings { filter_on: 0.0, ..StripSettings::default() }, &input);
        assert!(plain.windows(2).filter(|w| w[0] != w[1]).count() > 400, "off: untouched");
    }

    #[test]
    fn the_clipper_holds_its_ceiling_and_aliases_less_than_a_plain_clip() {
        // A loud 5 kHz sine, driven 18 dB into the ceiling.
        let rate = 48_000.0;
        let frames = 4_800;
        let freq = 5_000.0;
        let input: Vec<f32> = (0..frames).map(|i| (0.7 * (TAU * freq * f64::from(i) / rate).sin()) as f32).collect();
        let clipped = StripSettings { filter_on: 0.0, clip: 1.0, ..StripSettings::default() };
        let (l, r) = run(&clipped, &input);
        assert_eq!(l, r);
        let peak = l.iter().fold(0.0_f64, |m, x| m.max(f64::from(x.abs())));
        assert!(peak <= CLIP_CEILING + 1e-6 && peak > CLIP_CEILING - 0.01, "peak {peak}");

        // Energy away from the harmonics of 5 kHz (odd ones fold back
        // between them), against a plain clip of the same drive.
        let gain = 10_f64.powf(CLIP_DRIVE_DB / 20.0) / CLIP_CEILING;
        let plain: Vec<f64> = input.iter().map(|&x| (f64::from(x) * gain).clamp(-1.0, 1.0) * CLIP_CEILING).collect();
        let ours: Vec<f64> = l.iter().map(|&x| f64::from(x)).collect();
        let alias = |signal: &[f64]| -> f64 {
            // 1 kHz bins up to 23 kHz, skipping the multiples of 5 kHz.
            (1..24)
                .filter(|k| k % 5 != 0)
                .map(|k| {
                    let w = TAU * f64::from(k) * 1_000.0 / rate;
                    let (re, im) = signal.iter().enumerate().fold((0.0, 0.0), |(re, im), (n, x)| {
                        (re + x * (w * n as f64).cos(), im - x * (w * n as f64).sin())
                    });
                    re * re + im * im
                })
                .sum()
        };
        let (ours, plain) = (alias(&ours[480..]), alias(&plain[480..]));
        assert!(ours < plain * 0.5, "aliasing {ours} against {plain}");
    }

    #[test]
    fn reverb_times_follow_the_sample_rate() {
        let at = |rate: f32| Strip::new(rate).combs_l[0].len();
        assert_eq!(at(44_100.0), COMB_TUNINGS_L[0]);
        assert_eq!(at(88_200.0), 2 * COMB_TUNINGS_L[0]);
    }

    #[test]
    fn predelay_holds_the_reverb_back() {
        let mut input = vec![0.0; 4000];
        input[0] = 1.0;
        let reverb = StripSettings { filter_on: 0.0, reverb_mix: 1.0, ..StripSettings::default() };
        let first =
            |settings: &StripSettings| run(settings, &input).0[1..].iter().position(|s| s.abs() > 1e-6).unwrap();
        let late = first(&StripSettings { reverb_predelay: 0.02, ..reverb });
        assert_eq!(late - first(&reverb), 960);
    }

    #[test]
    fn dispersion_spreads_a_click_and_keeps_its_energy() {
        let mut chain = [Dispersion::default(); DISPERSION_STAGES];
        let response: Vec<f64> = (0..4000)
            .map(|i| chain.iter_mut().fold(if i == 0 { 1.0 } else { 0.0 }, |x, stage| stage.process(x, 0.6)))
            .collect();
        let energy: f64 = response.iter().map(|x| x * x).sum();
        assert!((energy - 1.0).abs() < 1e-9, "{energy}");
        assert!(response[0].abs() < 0.1, "the click is spread, not passed");
    }

    #[test]
    fn a_gated_reverb_stops_after_its_hold() {
        let mut input = vec![0.0; 48_000];
        input[..200].fill(0.5);
        let reverb = StripSettings { reverb_room: 0.9, reverb_mix: 1.0, ..StripSettings::default() };
        let tail =
            |settings: &StripSettings| run(settings, &input).0[24_000..].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let gated = StripSettings { reverb_gate: 0.25, ..reverb };
        assert!(tail(&reverb) > 1e-3);
        assert!(tail(&gated) < 1e-6, "{}", tail(&gated));
    }

    #[test]
    fn a_ping_pong_starts_left_then_alternates() {
        let mut input = vec![0.0; 3000];
        input[0] = 1.0;
        let settings = StripSettings {
            delay_time_l: 0.01,
            delay_time_r: 0.01,
            delay_mode: 2.0,
            delay_mono_in: 1.0,
            delay_feedback: 0.9,
            delay_mix: 1.0,
            ..StripSettings::default()
        };
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(&settings, None, &input, &input, &mut l, &mut r);
        // The dry hit is on both sides; then 480 left, 960 right, 1440 left.
        let echo = |s: &[f32], at: usize| s[at - 5..at + 5].iter().fold(0.0f32, |m, x| m.max(x.abs()));
        assert!(echo(&l, 480) > 0.1 && echo(&r, 480) < 1e-6);
        assert!(echo(&r, 960) > 0.01 && echo(&l, 960) < 1e-6);
        assert!(echo(&l, 1440) > 0.001 && echo(&r, 1440) < 1e-6);
    }

    #[test]
    fn the_compressor_lowers_loud_parts_by_its_ratio() {
        // A 0 dBFS square, one second, through a -24 dB threshold at 4:1.
        let input: Vec<f32> = (0..48_000).map(|i| if (i / 50) % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let off = StripSettings { filter_on: 0.0, ..StripSettings::default() };
        let comp = StripSettings { comp_mix: 1.0, comp_threshold_db: -24.0, comp_ratio: 4.0, ..off };
        let (l, r) = run(&comp, &input);
        assert_eq!(l, r);
        // 24 dB over: 18 dB taken off, half of it made up, settled.
        let settled = f64::from(l[47_999].abs());
        let expected = (-18.0_f64 + 9.0) / 20.0;
        assert!((settled.log10() - expected).abs() < 0.01, "{}", 20.0 * settled.log10());
        // The attack lets the first hit through louder than the settled level.
        assert!(f64::from(l[0].abs()) > settled * 2.0);
        // Off, the signal goes through untouched.
        assert_eq!(run(&off, &input).0, input);
    }

    /// `input` through a fresh strip with `key` as its sidechain.
    fn run_keyed(settings: &StripSettings, key: &[f32], input: &[f32]) -> Vec<f32> {
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(settings, Some(key), input, input, &mut l, &mut r);
        l
    }

    #[test]
    fn a_silent_key_ducks_nothing_and_a_loud_one_does_by_its_amount() {
        let input = vec![0.25_f32; 9_600];
        let ducked =
            StripSettings { filter_on: 0.0, duck_amount: 0.8, duck_release_seconds: 0.05, ..Default::default() };
        let silence = vec![0.0_f32; input.len()];
        assert_eq!(run_keyed(&ducked, &silence, &input), input);
        // A key past the full level, held: the gain settles at 1 - amount.
        let loud = vec![1.0_f32; input.len()];
        let out = run_keyed(&ducked, &loud, &input);
        assert!((out[9_599] - 0.25 * 0.2).abs() < 1e-4, "{}", out[9_599]);
        // Without a key (or with a key that is too short), the ducking is off.
        assert_eq!(run(&ducked, &input).0, input);
        let mut strip = Strip::new(48_000.0);
        let (mut l, mut r) = (vec![0.0; input.len()], vec![0.0; input.len()]);
        strip.process(&ducked, Some(&loud[..10]), &input, &input, &mut l, &mut r);
        assert_eq!(l, input);
    }

    #[test]
    fn the_ducking_lets_go_after_the_key() {
        let input = vec![0.25_f32; 24_000];
        let ducked =
            StripSettings { filter_on: 0.0, duck_amount: 1.0, duck_release_seconds: 0.05, ..Default::default() };
        let key: Vec<f32> = (0..input.len()).map(|i| if i < 2_400 { 1.0 } else { 0.0 }).collect();
        let out = run_keyed(&ducked, &key, &input);
        assert!(out[2_000] < 0.01, "ducked while the key plays");
        assert!(out[2_400 + 4_800] > out[2_400 + 480], "it comes back");
        assert!((out[23_999] - 0.25).abs() < 1e-3, "and is back");
    }

    #[test]
    fn a_keyed_compressor_follows_the_key_not_its_own_signal() {
        let input = vec![0.01_f32; 24_000];
        let comp = StripSettings {
            filter_on: 0.0,
            comp_mix: 1.0,
            comp_threshold_db: -24.0,
            comp_ratio: 20.0,
            comp_attack_seconds: 0.0001,
            ..Default::default()
        };
        let own = run(&comp, &input).0[23_999];
        let loud = run_keyed(&comp, &vec![1.0; input.len()], &input)[23_999];
        let quiet = run_keyed(&comp, &vec![0.0; input.len()], &input)[23_999];
        assert!(loud < own * 0.3, "a loud key compresses a quiet signal: {loud} against {own}");
        assert!((quiet - own).abs() < 1e-6, "a silent key is as the signal's own level (under the knee)");
    }

    #[test]
    fn the_compressor_leaves_quiet_parts_and_mixes_in_parallel() {
        let quiet = vec![0.01_f32; 4_800];
        let comp =
            StripSettings { filter_on: 0.0, comp_mix: 1.0, comp_threshold_db: -12.0, ..StripSettings::default() };
        let (l, _) = run(&comp, &quiet);
        // -40 dBFS is far under the knee: only the make-up gain (4.5 dB).
        let makeup = 10_f64.powf(12.0 * 0.75 * COMP_MAKEUP_SHARE / 20.0);
        assert!((f64::from(l[4_799]) / 0.01 - makeup).abs() < 1e-4, "{}", l[4_799]);

        let loud = vec![1.0_f32; 48_000];
        let full = run(&StripSettings { comp_threshold_db: -30.0, ..comp }, &loud).0[47_999];
        let half = run(&StripSettings { comp_threshold_db: -30.0, comp_mix: 0.5, ..comp }, &loud).0[47_999];
        assert!((f64::from(half) - (0.5 + 0.5 * f64::from(full))).abs() < 1e-4, "{half} {full}");
    }

    #[test]
    fn delay_repeats_after_its_time() {
        let mut input = vec![0.0; 2000];
        input[0] = 1.0;
        let settings =
            StripSettings { delay_time_l: 0.01, delay_time_r: 0.01, delay_mix: 1.0, ..StripSettings::default() };
        let (l, _) = run(&settings, &input);
        let echo = loudest(&l[..600], 400);
        assert!((479..=482).contains(&echo), "echo at {echo}");
    }
}
