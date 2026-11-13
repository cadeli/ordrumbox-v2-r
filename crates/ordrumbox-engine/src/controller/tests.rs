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
fn under_a_host_midi_keys_play_the_tracks_and_the_notes_go_out() {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{"beat":1}]},{"name":"SNARE"}]}]}"#,
    )
    .unwrap();
    let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let stopped = HostTransport { playing: false, bpm: 120.0, beat: 0.0 };
    audio.render_synced(&mut l, &mut r, stopped);
    controller.update(&library);
    assert_eq!(controller.keymaps.len(), 1, "a keymap once a host is seen");
    controller.update(&library);
    assert_eq!(controller.keymaps.len(), 1, "posted again only when it changes");

    // Key 36 (Bass Drum 1) plays the KICK track, stopped or not.
    audio.trigger(crate::DRUM_CHANNEL, 36, 1.0, 0);
    audio.render_synced(&mut l, &mut r, stopped);
    assert!(l.iter().any(|&s| s != 0.0));

    // Playing: KICK's note on beat 1 (frame 4000 at 120 bpm) goes out on key 36.
    let mut sent = Vec::new();
    for block in 0..24_u32 {
        let beat = f64::from(block * 256) / 4000.0;
        controller.update(&library);
        audio.render_synced(&mut l, &mut r, HostTransport { playing: true, bpm: 120.0, beat });
        audio.take_notes(|n| sent.push((block * 256 + n.timing, n.channel, n.key, n.velocity.is_some())));
    }
    assert_eq!(sent, [(4000, 9, 36, true), (5000, 9, 36, false)], "on channel 10, a 16th long");
}

/// The notes `audio` sends over `blocks` blocks of 256 frames from host
/// beat `from` at 120 bpm (8000 Hz), with no controller update between:
/// (frame from the first block, key, note on).
fn host_notes(audio: &mut AudioSide, from: f64, blocks: u32) -> Vec<(u32, u8, bool)> {
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut sent = Vec::new();
    for block in 0..blocks {
        let beat = from + f64::from(block * 256) / 4000.0;
        audio.render_synced(&mut l, &mut r, HostTransport { playing: true, bpm: 120.0, beat });
        audio.take_notes(|n| sent.push((block * 256 + n.timing, n.key, n.velocity.is_some())));
    }
    sent
}

#[test]
fn a_stopped_host_is_given_a_cue_that_plays_from_its_first_block() {
    let parse = |json: &str| Library::parse(json).unwrap().0;
    let library = parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{}]}]}]}"#);
    let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let stopped = |beat| HostTransport { playing: false, bpm: 120.0, beat };
    audio.render_synced(&mut l, &mut r, stopped(0.0));
    controller.update(&library);
    let cue = lock(&controller.link.inbox).cue.clone().expect("a cue for the stopped host");
    controller.update(&library);
    assert!(Arc::ptr_eq(&cue, lock(&controller.link.inbox).cue.as_ref().unwrap()), "built once");
    drop(cue);

    // The host starts: the downbeat sounds on its first frame, with no update between.
    assert_eq!(host_notes(&mut audio, 0.0, 1), [(0, 36, true)]);
    controller.update(&library);
    assert!(controller.is_playing(), "the controller follows the engine");
    assert!(lock(&controller.link.inbox).command.is_none(), "no Play posted over the cue");

    // Stopped at beat 5 (the second pass of 4 beats), after an edit: the
    // cue is that pass, with the edit.
    audio.render_synced(&mut l, &mut r, stopped(5.0));
    audio.take_notes(|_| {});
    controller.update(&library);
    assert!(!controller.is_playing());
    let edited = parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[{"beat":2}]}]}]}"#);
    controller.refresh(&edited);
    controller.update(&edited);
    assert_eq!(lock(&controller.link.inbox).cue.as_ref().map(|c| c.number), Some(1));
    // From beat 5, beat 6 (the second pass's beat 2) is 4000 frames on.
    assert_eq!(host_notes(&mut audio, 5.0, 16), [(4000, 36, true)]);
}

#[test]
fn plays_pass_after_pass_like_an_audio_callback() {
    let (library, _) =
        Library::parse(r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK","notes":[{},{"beat":2}]}]}]}"#)
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
fn a_pattern_selected_while_playing_waits_for_the_end_of_the_pass() {
    let (library, _) = Library::parse(
        r#"{"patterns":[{"name":"a","bpm":120,"tracks":[{"name":"KICK","notes":[{}]}]},
                        {"name":"b","bpm":120,"tracks":[{"name":"KICK","notes":[{"beat":1}]}]}]}"#,
    )
    .unwrap();
    let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    controller.play(&library, Mode::Pattern(0));
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    let mut callback = |controller: &mut Controller| {
        controller.update(&library);
        audio.render(&mut l, &mut r);
        let at = controller.position().expect("playing").pass;
        (at, controller.playing_pattern())
    };
    // A quarter of the first pass (16000 frames): the next one is posted by then.
    for _ in 0..16 {
        callback(&mut controller);
    }
    assert!(controller.sent.iter().any(|p| p.number == 1), "the next pass is posted ahead");
    controller.select_pattern(1);
    // An edit meanwhile rebuilds the pass playing from its own pattern.
    controller.refresh(&library);
    let mut seen = Vec::new();
    for _ in 0..(2 * 16_000 / 256) {
        let met = callback(&mut controller);
        if seen.last() != Some(&met) {
            seen.push(met);
        }
    }
    assert_eq!(seen, [(0, Some(0)), (1, Some(1)), (2, Some(1))]);
}

#[test]
fn removing_a_pattern_moves_the_later_ones_down_and_stops_on_the_one_playing() {
    let (library, _) =
        Library::parse(r#"{"patterns":[{"name":"a","tracks":[]},{"name":"b","tracks":[]},{"name":"c","tracks":[]}]}"#)
            .unwrap();
    let (mut controller, _audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    controller.play(&library, Mode::Pattern(2));
    controller.remove_pattern(0);
    assert_eq!(controller.mode(), Mode::Pattern(1));
    assert_eq!(controller.pattern_of(0), Some(1));
    controller.remove_pattern(1);
    assert!(!controller.is_playing());
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
fn a_midi_key_finds_the_track_that_plays_it_and_its_pitch() {
    let (mut controller, _audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    let (library, _) =
        Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK"},{"name":"SNARE"}]}]}"#).unwrap();
    assert_eq!(controller.midi_track(&library, 9, 36), Some((0, 0.0)), "GM kick");
    assert_eq!(controller.midi_track(&library, 9, 38), Some((1, 0.0)), "GM snare");
    assert_eq!(controller.midi_track(&library, 9, 100), None);
    // A song plays no pattern: no live notes to write.
    controller.mode = Mode::Song(0);
    assert_eq!(controller.midi_track(&library, 9, 36), None);
}

#[test]
fn a_song_loop_wraps_the_measures_but_not_the_pass_numbers() {
    let (mut controller, _audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    let library = Library::default();
    assert_eq!((0..5).map(|n| controller.song_measure(n)).collect::<Vec<_>>(), [0, 1, 2, 3, 4], "no loop");
    controller.set_song_loop(&library, Some((2, 4)));
    assert_eq!((0..8).map(|n| controller.song_measure(n)).collect::<Vec<_>>(), [0, 1, 2, 3, 2, 3, 2, 3]);
    controller.set_song_loop(&library, Some((3, 3)));
    assert_eq!(controller.song_measure(9), 9, "an empty region is no loop");
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
    let stopped = Snapshot { position: None, wants: 0, host: None, lost: false };
    assert_eq!(published.read(), stopped);
    let host = HostTransport { playing: true, bpm: 97.5, beat: 12.25 };
    let playing = Snapshot {
        position: Some(Position { pass: 3, frame: 10, frames: 100 }),
        wants: 4,
        host: Some(host),
        lost: true,
    };
    published.write(playing);
    assert_eq!(published.read(), playing);
    published.write(stopped);
    assert_eq!(published.read(), stopped);
}

#[test]
fn preview_sounds_while_stopped() {
    let (library, _) = Library::parse(r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","notes":[]}]}]}"#).unwrap();
    let (mut controller, mut audio) = Controller::new(8000.0, bank(), Scales::default(), SynthPresets::default());
    controller.preview(&library, 0, 0, None);
    controller.preview(&library, 0, 5, None); // no such track: nothing
    let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
    audio.render(&mut l, &mut r);
    assert!(l.iter().any(|s| s.abs() > 0.01));
    assert_eq!(controller.position(), None);
}
