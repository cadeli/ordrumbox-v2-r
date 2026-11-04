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
        // Four beats at 120 bpm of its own.
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"p","bpm":120,"tracks":[{"name":"KICK","notes":[{},{"beat":2}]}]}]}"#,
        )
        .unwrap();
        let (mut controller, audio) = Controller::new(RATE as f32, bank(), Scales::default(), SynthPresets::default());
        controller.select_pattern(0);
        Self { library, controller, audio, host: HostTransport { playing: false, bpm: 60.0, beat: 0.0 } }
    }

    /// One block: the UI side's update, then the audio callback; the host moves on when playing.
    fn block(&mut self) {
        self.controller.update(&self.library);
        let (mut left, mut right) = ([0.0; BLOCK], [0.0; BLOCK]);
        self.audio.render_synced(&mut left, &mut right, self.host);
        if self.host.playing {
            self.host.beat += BLOCK as f64 / RATE * self.host.bpm / 60.0;
        }
    }

    fn blocks(&mut self, count: usize) {
        for _ in 0..count {
            self.block();
        }
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
