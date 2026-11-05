//! The UI side of the plugin: the app's [`Ui`] and its session (the library,
//! the engine's controller), updated by a thread of its own whether the
//! editor is open or not.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

use ordrumbox_engine::AudioSide;
use ordrumbox_ui::{LibrarySource, Session, Ui};

use crate::params::{HostChoice, Names, OrdrumboxParams};
use crate::state::LibraryState;

/// How often the controller posts passes and follows the host.
const TICK_INTERVAL: Duration = Duration::from_millis(10);

/// What [`Player::start`] did with the engine.
#[derive(Debug)]
pub enum Started {
    /// The session and its engine are the same as before.
    Kept,
    /// A new session and engine: its audio side, `None` when there is no
    /// sound (the session's status says why).
    New(Option<Box<AudioSide>>),
}

/// The UI and the library revision last saved into the project.
#[derive(Debug, Default)]
pub struct Player {
    ui: Option<Ui>,
    /// The sample rate of the running engine.
    sample_rate: f32,
    /// The revision whose text the project holds; `None`: not saved from this session.
    published: Option<u64>,
    /// The host's pattern and kit last applied to this session; `None`: none yet.
    applied: Option<HostChoice>,
    /// The host changed what the window shows since it was last drawn.
    redraw: bool,
}

impl Player {
    /// Called by `initialize()`: a session for a project the host restored or
    /// for a new sample rate (the library carried over), else the running
    /// one kept.
    pub fn start(&mut self, sample_rate: f32, state: &LibraryState) -> Started {
        let restored = state.take_restored();
        if self.ui.is_some() && restored.is_none() && self.sample_rate == sample_rate {
            return Started::Kept;
        }
        let carried = self.ui.as_ref().map(|ui| ui.session.library_text());
        let (start, sides) = ordrumbox_ui::host_output(sample_rate);
        let session = Session::load_with_assets(start, LibrarySource::Host(restored.or(carried)), &bundle_assets());
        // The project keeps its own text (even one that could not be read)
        // until the library is edited; a new instance saves what it plays.
        self.published = if state.is_empty() { None } else { Some(session.library_revision()) };
        self.ui = Some(Ui::new(session));
        self.applied = None;
        self.sample_rate = sample_rate;
        self.publish(state);
        Started::New(sides.try_recv().ok().map(Box::new))
    }

    /// One step of the UI side: the host's pattern and kit applied when
    /// they changed, the controller fed, the names the host shows updated,
    /// the library saved into the project when it changed.
    pub fn tick(&mut self, state: &LibraryState, choice: HostChoice, names: &Names) {
        if let Some(ui) = &mut self.ui {
            let last = self.applied.replace(choice);
            if last.is_none_or(|last| last.kit != choice.kit) {
                ui.session.host_select_kit(choice.kit);
                self.redraw = true;
            }
            if last.is_none_or(|last| last.pattern != choice.pattern) {
                ui.session.host_select_pattern(choice.pattern);
                self.redraw = true;
            }
            ui.session.update();
            names.set_patterns(ui.session.pattern_names());
            names.set_kits(ui.session.kit_names());
        }
        self.publish(state);
    }

    /// Whether the host changed what the window shows since the last call.
    pub fn take_redraw(&mut self) -> bool {
        std::mem::take(&mut self.redraw)
    }

    fn publish(&mut self, state: &LibraryState) {
        let Some(ui) = &self.ui else { return };
        let revision = ui.session.library_revision();
        if self.published != Some(revision) {
            state.publish(ui.session.library_text());
            self.published = Some(revision);
        }
    }

    pub fn ui(&mut self) -> Option<&mut Ui> {
        self.ui.as_mut()
    }
}

/// `Contents/Resources/assets` of the bundle the plugin binary is in, and
/// `assets/` next to it: `X.clap/Contents/MacOS/X`,
/// `X.vst3/Contents/x86_64-win/X.vst3`, a single `X.clap` on Windows.
fn bundle_assets() -> Vec<PathBuf> {
    let Some(binary) = process_path::get_dylib_path() else { return Vec::new() };
    let Some(dir) = binary.parent() else { return Vec::new() };
    let mut dirs = Vec::new();
    if let Some(contents) = dir.parent() {
        dirs.push(contents.join("Resources").join("assets"));
    }
    dirs.push(dir.join("assets"));
    dirs
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
    pub fn spawn(player: SharedPlayer, params: Arc<OrdrumboxParams>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new().name("ordrumbox-player".into()).spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                lock(&player).tick(&params.library, params.choice(), &params.names);
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
    use nih_plug::params::persist::PersistentField;
    use ordrumbox_engine::HostTransport;

    use super::*;
    use crate::state::LibraryText;

    const FIRST: HostChoice = HostChoice { pattern: 0, kit: 0 };

    fn audio(started: Started) -> Box<AudioSide> {
        match started {
            Started::New(Some(audio)) => audio,
            other => panic!("expected a new engine, got {other:?}"),
        }
    }

    #[test]
    fn a_new_player_plays_with_the_host_and_saves_its_library() {
        let state = Arc::new(LibraryState::default());
        let mut player = Player::default();
        let mut audio = audio(player.start(48_000.0, &state));
        assert!(!state.is_empty(), "the library is saved with the project");

        let host = HostTransport { playing: true, bpm: 100.0, beat: 0.0 };
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        for _ in 0..4 {
            player.tick(&state, FIRST, &Names::default());
            audio.render_synced(&mut left, &mut right, host);
        }
        assert!(player.ui().is_some_and(|ui| ui.session.playing()));

        assert!(matches!(player.start(48_000.0, &state), Started::Kept));
        assert!(matches!(player.start(44_100.0, &state), Started::New(Some(_))));
    }

    #[test]
    fn a_restored_project_is_loaded_and_its_text_kept_until_an_edit() {
        let state = Arc::new(LibraryState::default());
        let mut player = Player::default();
        let _audio = player.start(48_000.0, &state);

        // A text the session cannot read: the project keeps it.
        state.set(LibraryText("{".into()));
        let _audio = audio(player.start(48_000.0, &state));
        player.tick(&state, FIRST, &Names::default());
        assert_eq!(state.map(|t| t.0.clone()), "{");

        let text = r#"{"patterns":[{"name":"mine","tracks":[]}]}"#;
        state.set(LibraryText(text.into()));
        let _audio = audio(player.start(48_000.0, &state));
        player.tick(&state, FIRST, &Names::default());
        assert!(player.ui().is_some_and(|ui| ui.session.library_text().contains("mine")));
    }

    #[test]
    fn the_host_chooses_the_pattern_when_its_parameter_changes() {
        let state = Arc::new(LibraryState::default());
        let names = Names::default();
        let mut player = Player::default();
        let _audio = player.start(48_000.0, &state);
        let text = r#"{"patterns":[{"name":"a","tracks":[]},{"name":"b","tracks":[]}]}"#;
        state.set(LibraryText(text.into()));
        let _audio = audio(player.start(48_000.0, &state));
        let shown = |player: &mut Player| player.ui().and_then(|ui| ui.session.pattern()?.name.clone());

        player.tick(&state, HostChoice { pattern: 1, ..FIRST }, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("b"));
        assert!(player.take_redraw() && !player.take_redraw());

        // Unchanged, the parameter leaves the window's own choice alone.
        player.ui().unwrap().session.step_pattern(1);
        player.tick(&state, HostChoice { pattern: 1, ..FIRST }, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("a"));

        player.tick(&state, HostChoice { pattern: 4, ..FIRST }, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("a"));
        assert_eq!(player.ui().unwrap().session.message(), "host: no pattern 5 (the library has 2)");
    }

    #[test]
    fn the_ticker_stops_when_dropped() {
        let player = SharedPlayer::default();
        let ticker = Ticker::spawn(player.clone(), Arc::default()).unwrap();
        drop(ticker);
        assert_eq!(Arc::strong_count(&player), 1);
    }
}
