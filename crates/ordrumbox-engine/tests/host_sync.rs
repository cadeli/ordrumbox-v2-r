//! Playback under a plugin host: the controller and the audio side driven
//! block by block with a host transport that starts, plays at its own tempo,
//! jumps, changes tempo and stops.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::sync::Arc;

use ordrumbox_core::{Library, Scales};
use ordrumbox_engine::engine::SEEK_TOLERANCE_FRAMES;
use ordrumbox_engine::kit::{BankSample, KitSample};
use ordrumbox_engine::{AudioSide, Controller, HostTransport, Mode, SampleBank, SampleBuffer, SynthPresets};

const RATE: f64 = 8000.0;
const BLOCK: usize = 256;

fn bank() -> Arc<SampleBank> {
    let sample = KitSample {
        key: "KICK".into(),
        display_name: "k".into(),
        path: "k".into(),
        decay: Some(100.0),
        ..Default::default()
    };
    let buffer = SampleBuffer::mono(vec![0.5; 400], 8000);
    Arc::new(SampleBank { samples: vec![BankSample { kit: "x".into(), sample, buffer: buffer.into() }] })
}

/// A host and the plugin it drives.
struct Session {
    library: Library,
    controller: Controller,
    audio: AudioSide,
    host: HostTransport,
}

impl Session {
    fn new() -> Self {
        Self::with_notes(r#"[{},{"beat":2}]"#)
    }

    /// A stopped host at 60 bpm and a pattern whose KICK track plays `notes`.
    fn with_notes(notes: &str) -> Self {
        // Four beats at 120 bpm of its own.
        let (library, _) = Library::parse(&format!(
            r#"{{"patterns":[{{"name":"p","bpm":120,"tracks":[{{"name":"KICK","notes":{notes}}}]}}]}}"#
        ))
        .unwrap();
        let (mut controller, audio) = Controller::new(RATE as f32, bank(), Scales::default(), SynthPresets::default());
        controller.select_pattern(0);
        Self { library, controller, audio, host: HostTransport { playing: false, bpm: 60.0, beat: 0.0 } }
    }

    /// One block: the UI side's update, then the audio callback; the host
    /// moves on when playing. The left side of what was rendered.
    fn block(&mut self) -> [f32; BLOCK] {
        self.controller.update(&self.library);
        let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
        self.audio.render_synced(&mut left, &mut right, self.host);
        if self.host.playing {
            self.host.beat += BLOCK as f64 / RATE * self.host.bpm / 60.0;
        }
        left
    }

    fn blocks(&mut self, count: usize) {
        self.audio_of(count);
    }

    /// The left side of `count` blocks.
    fn audio_of(&mut self, count: usize) -> Vec<f32> {
        (0..count).flat_map(|_| self.block()).collect()
    }

    /// Frames since beat 0 where the engine is, and where the host is.
    fn frames(&self) -> (u64, u64) {
        let position = self.controller.position().expect("playing");
        let engine = position.pass * position.frames + position.frame;
        let host = (self.host.beat * RATE * 60.0 / self.host.bpm).round() as u64;
        (engine, host)
    }

    fn assert_in_step(&self) {
        let (engine, host) = self.frames();
        assert!(engine.abs_diff(host) <= SEEK_TOLERANCE_FRAMES, "engine at {engine}, host at {host}");
    }
}

#[test]
fn playback_follows_the_host_transport() {
    let mut session = Session::new();
    assert_eq!(session.controller.mode(), Mode::Pattern(0));

    // Stopped host: nothing plays, the tempo is the host's.
    session.blocks(4);
    assert!(!session.controller.is_playing() && session.controller.position().is_none());
    assert_eq!(session.controller.host().map(|h| h.bpm), Some(60.0));

    // The host plays from beat 0 at 60 bpm: four beats are 32000 frames, not
    // the 16000 of the pattern's 120 bpm.
    session.host.playing = true;
    session.blocks(2);
    assert_eq!(session.controller.position().map(|p| p.frames), Some(32_000));
    session.blocks(300);
    assert_eq!(session.controller.position().map(|p| p.pass), Some(2));
    session.assert_in_step();

    // A jump back into the first pass (a loop), then forward into pass 5.
    session.host.beat = 1.5;
    session.blocks(3);
    assert_eq!(session.controller.position().map(|p| p.pass), Some(0));
    session.assert_in_step();
    session.host.beat = 21.0;
    session.blocks(3);
    assert_eq!(session.controller.position().map(|p| p.pass), Some(5));
    session.assert_in_step();

    // The host's tempo doubles: the pass playing is rebuilt at 120 bpm.
    session.host.bpm = 120.0;
    session.blocks(3);
    assert_eq!(session.controller.position().map(|p| p.frames), Some(16_000));
    session.assert_in_step();
    session.blocks(200);
    session.assert_in_step();

    // The host stops: so does playback, at once.
    session.host.playing = false;
    session.blocks(1);
    assert!(session.controller.position().is_none());
    session.blocks(1);
    assert!(!session.controller.is_playing());

    // A pre-roll before beat 0 plays nothing; beat 0 starts pass 0.
    session.host = HostTransport { playing: true, bpm: 120.0, beat: -1.0 };
    session.blocks(3);
    assert!(session.controller.position().is_none());
    session.blocks(60);
    assert_eq!(session.controller.position().map(|p| p.pass), Some(0));
    session.assert_in_step();
}

/// The frame a note starts on, from the first frame that sounds: a voice's
/// envelope rises from 0, so its own first frame is silent.
fn onset(audio: &[f32]) -> usize {
    audio.iter().position(|sample| sample.abs() > 1e-4).expect("some sound") - 1
}

#[test]
fn the_first_rolling_block_plays_the_pass_from_its_first_frame() {
    let mut session = Session::new();
    // The host is stopped: the pass it would start in waits, nothing sounds.
    assert!(session.audio_of(4).iter().all(|&sample| sample == 0.0));
    assert!(session.controller.position().is_none());

    // The host rolls: the block it starts with holds the kick of beat 0, with
    // no block lost waiting for the UI side.
    session.host.playing = true;
    let first = session.block();
    assert_eq!(onset(&first), 0);
    session.block();
    let position = session.controller.position().expect("playing");
    assert_eq!((position.pass, position.frame), (0, 2 * BLOCK as u64));
    session.assert_in_step();
}

#[test]
fn the_host_swing_moves_the_off_sixteenths_and_keeps_the_place() {
    // A kick on the second sixteenth; at the host's 60 bpm a sixteenth is 2000 frames.
    const SIXTEENTH: usize = 2000;
    let onset_at = |swing: Option<f64>| {
        let mut session = Session::with_notes(r#"[{"step":1}]"#);
        if let Some(swing) = swing {
            session.controller.set_host_swing(&session.library, swing);
        }
        session.blocks(2);
        session.host.playing = true;
        onset(&session.audio_of(20))
    };
    let straight = onset_at(None);
    assert_eq!(straight, SIXTEENTH);
    // On the last third of its pair of sixteenths.
    let swung = onset_at(Some(2.0 / 3.0));
    assert!(swung.abs_diff(2 * SIXTEENTH * 2 / 3) <= 1, "swung to frame {swung}");

    // A swing set while playing rebuilds the pass where it is.
    let mut session = Session::with_notes(r#"[{"step":1}]"#);
    session.host.playing = true;
    session.blocks(40);
    let before = session.controller.position().expect("playing");
    session.controller.set_host_swing(&session.library, 2.0 / 3.0);
    session.blocks(1);
    let after = session.controller.position().expect("playing");
    assert_eq!((after.pass, after.frame), (before.pass, before.frame + BLOCK as u64));
    session.assert_in_step();
    // The next pass plays swung.
    let pass_frames = after.frames as usize;
    let rest = session.audio_of((2 * pass_frames - after.frame as usize) / BLOCK);
    let next_pass = &rest[pass_frames - after.frame as usize..];
    assert!(onset(next_pass).abs_diff(2 * SIXTEENTH * 2 / 3) <= 1, "frame {}", onset(next_pass));
}
