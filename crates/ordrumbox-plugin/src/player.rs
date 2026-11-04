//! The UI side of the plugin's playback: the library and the engine's
//! [`Controller`], updated by a thread of its own whether the editor is open
//! or not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use ordrumbox_core::{Library, json};
use ordrumbox_engine::{AudioSide, Controller};

use crate::assets::{self, Assets};
use crate::state::LibraryState;

/// How often the controller posts passes and follows the host.
const TICK_INTERVAL: Duration = Duration::from_millis(10);

/// The library, the controller of the running engine, and what went wrong.
#[derive(Debug, Default)]
pub struct Player {
    /// Loaded on the first [`start`](Self::start), not when a host scans plugins.
    assets: Option<Assets>,
    library: Library,
    controller: Option<Controller>,
    /// Errors to show: assets that could not be read, a project's library that could not.
    pub status: Vec<String>,
}

impl Player {
    /// A new engine at `sample_rate`; the audio side is the caller's, for its
    /// process callback. The assets are loaded on the first call.
    pub fn start(&mut self, sample_rate: f32, state: &LibraryState) -> AudioSide {
        let assets = self.assets.get_or_insert_with(|| {
            let assets = assets::find_and_load();
            self.status.extend(assets.errors.iter().cloned());
            assets
        });
        if self.controller.is_none() {
            self.library = assets.library.clone();
            if state.is_empty() {
                state.publish(self.library.to_json().to_json());
            }
        }
        let bank = Arc::new(assets.bank.clone());
        let (mut controller, audio) = Controller::new(sample_rate, bank, assets.scales.clone(), assets.synths.clone());
        controller.select_pattern(0);
        self.controller = Some(controller);
        audio
    }

    /// One step of the UI side: a library the host restored, then the controller.
    pub fn tick(&mut self, state: &LibraryState) {
        if let Some(text) = state.take_restored() {
            self.restore(&text);
        }
        if let Some(controller) = &mut self.controller {
            controller.update(&self.library);
        }
    }

    fn restore(&mut self, text: &str) {
        match json::parse(text) {
            Ok(value) => {
                let (library, report) = Library::from_json(&value);
                self.status.extend(report.issues.iter().map(|issue| format!("project library: {issue:?}")));
                self.library = library;
                if let Some(controller) = &mut self.controller {
                    controller.select_pattern(0);
                    controller.refresh(&self.library);
                }
            }
            // The library played until now stays; the project keeps its text.
            Err(e) => self.status.push(format!("project library: {e}")),
        }
    }

    pub fn library(&self) -> &Library {
        &self.library
    }

    pub fn controller(&self) -> Option<&Controller> {
        self.controller.as_ref()
    }
}

/// A [`Player`] shared by the tick thread, the editor and the plugin.
pub type SharedPlayer = Arc<Mutex<Player>>;

/// The player, locked; a panic on another thread left it usable.
pub fn lock(player: &SharedPlayer) -> MutexGuard<'_, Player> {
    player.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The thread that ticks the player; dropping it stops the thread.
#[derive(Debug)]
pub struct Ticker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Ticker {
    pub fn spawn(player: SharedPlayer, state: Arc<LibraryState>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new().name("ordrumbox-player".into()).spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                lock(&player).tick(&state);
                std::thread::sleep(TICK_INTERVAL);
            }
        })?;
        Ok(Self { stop, thread: Some(thread) })
    }
}

impl Drop for Ticker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            nih_plug::nih_log!("the player thread panicked");
        }
    }
}

#[cfg(test)]
mod tests {
    use ordrumbox_engine::HostTransport;

    use super::*;

    #[test]
    fn a_new_player_plays_the_demo_library_with_the_host() {
        let state = LibraryState::default();
        let mut player = Player::default();
        let mut audio = player.start(48_000.0, &state);
        assert_eq!(player.status, Vec::<String>::new());
        assert!(!state.is_empty(), "the demo library is saved with the project");

        let host = HostTransport { playing: true, bpm: 100.0, beat: 0.0 };
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..4 {
            player.tick(&state);
            audio.render_synced(&mut left, &mut right, host);
        }
        assert!(player.controller().is_some_and(Controller::is_playing));
        assert_eq!(player.controller().and_then(Controller::host).map(|h| h.bpm), Some(100.0));
    }

    #[test]
    fn a_restored_library_replaces_the_demo_and_a_bad_one_is_reported() {
        use nih_plug::params::persist::PersistentField;

        use crate::state::LibraryText;

        let state = Arc::new(LibraryState::default());
        let mut player = Player::default();
        let _audio = player.start(48_000.0, &state);
        state.set(LibraryText(r#"{"patterns":[{"name":"mine","tracks":[]}]}"#.into()));
        player.tick(&state);
        assert_eq!(player.library().patterns.len(), 1);
        assert_eq!(player.library().patterns[0].name.as_deref(), Some("mine"));

        state.set(LibraryText("{".into()));
        player.tick(&state);
        assert_eq!(player.library().patterns[0].name.as_deref(), Some("mine"));
        assert_eq!(player.status.len(), 1);
    }

    #[test]
    fn the_ticker_stops_when_dropped() {
        let player = SharedPlayer::default();
        let ticker = Ticker::spawn(player.clone(), Arc::default()).unwrap();
        drop(ticker);
        assert_eq!(Arc::strong_count(&player), 1);
    }
}
