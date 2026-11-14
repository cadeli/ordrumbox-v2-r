use super::*;
use crate::midi::{ClockMessage, DRUM_CHANNEL, Target};
use crate::pass::SynthStart;
use crate::strip_settings::saturation_amount;
use ordrumbox_core::steps::TICK;
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
    let voice = VoiceStart {
        sample: 0,
        track: 0,
        ratio: 1.0,
        peak: 1.0,
        decay: 10.0,
        pan: 0.0,
        delay: 0,
        length: 0.0,
        shape: Default::default(),
    };
    Event { frame, sound: Sound::Sample(voice), mono: true, velocity: 1.0, note: None, note_frames: 0 }
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

/// The tracks of `playing` play the same long sample at frame 0 through two
/// strips, the second of which ducks fully by the key `keys` gives it. The
/// left output's level, once the ducking has settled.
fn ducked_level(playing: &[usize], keys: Vec<Option<usize>>) -> f32 {
    let mut engine = engine();
    engine.bank = Arc::new(SampleBank {
        samples: vec![crate::kit::BankSample {
            buffer: SampleBuffer::mono(vec![1.0; 48_000], 48_000).into(),
            ..engine.bank.samples[0].clone()
        }],
    });
    let voice = |track| VoiceStart {
        sample: 0,
        track,
        ratio: 1.0,
        peak: 1.0,
        decay: 10.0,
        pan: 0.0,
        delay: 0,
        length: 0.0,
        shape: Default::default(),
    };
    let events = playing.iter().map(|&track| Event { sound: Sound::Sample(voice(track)), ..sample_event(0) }).collect();
    let ducked = StripSettings { duck_amount: 1.0, ..StripSettings::default() };
    engine.play(Arc::new(Pass {
        frames: 48_000,
        ticks: TICK,
        events,
        strips: vec![StripSettings::default(), ducked],
        keys,
        ..Pass::default()
    }));
    let signal = render(&mut engine, 9_600);
    signal[9_000..].iter().fold(0.0_f32, |peak, s| peak.max(s.abs()))
}

#[test]
fn a_strip_ducks_by_the_key_track_it_is_given() {
    let alone = ducked_level(&[0], vec![]);
    let free = ducked_level(&[0, 1], vec![]);
    assert!(free > alone * 1.3, "both tracks are louder than one: {free} against {alone}");
    // Keyed by the first track, which plays, the second is ducked away.
    let keyed = ducked_level(&[0, 1], vec![None, Some(0)]);
    assert!((keyed - alone).abs() < alone * 0.02, "{keyed} against {alone}");
    // Keyed by a track that is silent, or by one past the strips: no ducking.
    for (playing, keys) in [(&[1][..], vec![None, Some(0)]), (&[0, 1][..], vec![None, Some(7)])] {
        let level = ducked_level(playing, keys);
        let expected = if playing.len() == 1 { alone } else { free };
        assert!((level - expected).abs() < expected * 0.02, "{level} against {expected}");
    }
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
fn a_split_track_leaves_the_mix_for_an_output_of_its_own() {
    let frames = 300;
    let rendered = |split: u64| {
        let mut engine = engine();
        engine.set_host_mix(HostMix { split, ..HostMix::default() });
        engine.play(pass(0, &[0], 48_000));
        let (mut left, mut right) = (vec![0.0; frames], vec![0.0; frames]);
        let mut own = vec![0.0; frames];
        engine.render_split(&mut left, &mut right, |track, at, l, _| {
            assert_eq!(track, 0);
            own[at..at + l.len()].copy_from_slice(l);
        });
        (left, own, engine.peaks()[0])
    };
    let (mixed, none, peak) = rendered(0b10);
    assert!(mixed.iter().any(|&s| s != 0.0) && none.iter().all(|&s| s == 0.0), "another track's split");
    let (main, own, own_peak) = rendered(0b1);
    assert!(main.iter().all(|&s| s == 0.0), "nothing of it is left in the mix");
    // The master bus is bypassed at unity gain: the same samples, across quanta.
    assert_eq!(own, mixed);
    assert_eq!(own_peak, peak, "its meter still moves");
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
        compressor: 0.8,
        threshold_db: -100.0,
        ..HostTrack::default()
    };
    let s = host.strip(&target, 0, &ratios(&host, 0));
    assert_eq!((s.cutoff, s.q), (2000.0, 2.707));
    assert_eq!((s.reverb_mix, s.delay_mix, s.delay_feedback), (0.5, 1.0, 0.65), "amounts kept within 0 to 1");
    assert_eq!((s.saturation_drive, s.saturation_mix), (4.0, 1.0));
    assert_eq!(saturation_amount(&s), 0.5);
    assert_eq!((s.comp_mix, s.comp_threshold_db), (0.8, MIN_THRESHOLD_DB), "the threshold kept in its range");
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
        note_frames: 0,
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
        event.note_frames = 100;
    }
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

fn clock(engine: &mut Engine, block_start: u64, frames: usize) -> Vec<(u32, ClockMessage)> {
    let mut sent = Vec::new();
    engine.take_clock(block_start, frames, |c| sent.push((c.timing, c.message)));
    sent
}

#[test]
fn the_clock_goes_out_as_a_start_24_pulses_a_beat_and_a_stop() {
    let mut engine = engine();
    // Off unless asked for.
    engine.play(pass(0, &[], 1000));
    render(&mut engine, 1000);
    assert_eq!(clock(&mut engine, 0, 1000), []);

    engine.set_clock_out(true);
    engine.play(pass(0, &[], 1000));
    render(&mut engine, 1000);
    let sent = clock(&mut engine, 1000, 1000);
    assert_eq!(sent[0], (0, ClockMessage::Start));
    assert!(sent[1..].iter().all(|&(_, m)| m == ClockMessage::Pulse));
    let pulses: Vec<u32> = sent[1..].iter().map(|&(timing, _)| timing).collect();
    assert_eq!(pulses.len(), 24, "a pass is a beat");
    assert_eq!(pulses[..3], [0, 42, 84], "a pulse every 1000 / 24 frames, rounded up");
    assert_eq!(pulses[23], 959);

    // The pass plays again (the next one is late): the pulses start again with it.
    render(&mut engine, 1000);
    let again = clock(&mut engine, 2000, 1000);
    assert_eq!(again.len(), 24);
    assert_eq!(again[0], (0, ClockMessage::Pulse));

    engine.stop();
    render(&mut engine, 10);
    assert_eq!(clock(&mut engine, 3000, 10), [(0, ClockMessage::Stop)]);
    engine.stop();
    render(&mut engine, 10);
    assert_eq!(clock(&mut engine, 3010, 10), [], "stopped already");
}

#[test]
fn a_seek_skips_the_pulses_before_it_and_a_later_pass_continues_from_its_position() {
    let mut engine = engine();
    engine.set_clock_out(true);
    engine.play(pass(0, &[], 1000));
    assert!(engine.follow(0.5));
    render(&mut engine, 100);
    let sent = clock(&mut engine, 0, 100);
    assert_eq!(sent[0], (0, ClockMessage::Start));
    assert_eq!(sent[1], (0, ClockMessage::Pulse), "the 13th pulse, at frame 500, is due now");
    assert_eq!(sent.len(), 4, "the Start and the pulses at 500, 542 and 584");

    let mut later = (*pass(4, &[], 1000)).clone();
    later.first_tick = 4 * u64::from(TICK);
    engine.play(Arc::new(later));
    render(&mut engine, 10);
    let sent = clock(&mut engine, 100, 10);
    assert_eq!(&sent[..2], [(0, ClockMessage::Position(16)), (0, ClockMessage::Continue)], "4 beats on: 16 sixteenths");
}
