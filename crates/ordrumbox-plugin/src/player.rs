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

use crate::lockstep::Steps;
use crate::params::{HostControls, KIT_CHOICES, Names, OrdrumboxParams, PATTERN_CHOICES, choice_value};
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
    /// The host's pattern parameter, followed.
    pattern: Follow,
    /// The host's kit parameter, followed.
    kit: Follow,
    /// The library revision whose pattern names the host has; `None`: none yet.
    named: Option<u64>,
    /// The host changed what the window shows since it was last drawn.
    redraw: bool,
}

/// A host's choice parameter (the pattern or the kit) followed by the
/// session, and the choices made in the window given back to it.
#[derive(Debug, Default)]
struct Follow {
    /// The host's value last seen; `None`: none since the session started.
    seen: Option<usize>,
    /// That value was refused: it is applied once the session has it,
    /// unless a choice is made in the window first.
    refused: bool,
    /// The session's choice as of the last look; `None`: none yet.
    known: Option<usize>,
    /// A choice made in the window since, not yet given to the host.
    moved: Option<usize>,
}

impl Follow {
    /// The session's choice is `index`: a change since the last look was made in the window.
    fn look(&mut self, index: usize) {
        if self.known.is_some_and(|known| known != index) {
            self.moved = Some(index);
            self.refused = false;
        }
        self.known = Some(index);
    }

    /// Whether the host's `value` is to be applied: it moved, or it was
    /// refused and the session's `count` choices now have it.
    fn due(&mut self, value: usize, count: usize) -> bool {
        self.seen.replace(value) != Some(value) || (self.refused && value < count)
    }
}

/// The values (from 1) for a host's pattern and kit parameters: the
/// choices made in the window.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WindowChoices {
    pub pattern: Option<i32>,
    pub kit: Option<i32>,
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
        (self.pattern, self.kit, self.named) = (Follow::default(), Follow::default(), None);
        self.sample_rate = sample_rate;
        self.publish(state);
        Started::New(sides.try_recv().ok().map(Box::new))
    }

    /// One step of the UI side: the host's pattern and kit applied when
    /// they changed (or when a refused one can be), its mutes shown, its
    /// swing applied, the controller fed, the names the host shows updated,
    /// the library saved into the project when it changed.
    pub fn tick(&mut self, state: &LibraryState, host: HostControls, names: &Names) {
        let Some(ui) = &mut self.ui else { return };
        let session = &mut ui.session;
        self.pattern.look(session.pattern_index());
        self.kit.look(session.kit_index());
        if self.kit.due(host.kit, session.kit_names().count()) {
            self.kit.refused = !session.host_select_kit(host.kit);
            self.redraw = true;
        }
        if self.pattern.due(host.pattern, session.pattern_names().count()) {
            self.pattern.refused = !session.host_select_pattern(host.pattern);
            self.redraw = true;
        }
        self.redraw |= session.set_host_muted(host.muted);
        session.set_host_swing(host.swing);
        session.update();
        // What the host or the playback chose is not given back to the host.
        self.pattern.known = Some(session.pattern_index());
        self.kit.known = Some(session.kit_index());
        let revision = session.library_revision();
        if self.named != Some(revision) {
            names.set_patterns(session.pattern_names());
            self.named = Some(revision);
        }
        names.set_kits(session.kit_names());
        self.publish(state);
    }

    /// The choices made in the window since the last call, for the host's
    /// parameters; one past a parameter's range is refused with a message.
    pub fn take_window_choices(&mut self) -> WindowChoices {
        let Some(ui) = &mut self.ui else { return WindowChoices::default() };
        let session = &mut ui.session;
        self.pattern.look(session.pattern_index());
        self.kit.look(session.kit_index());
        let mut value = |follow: &mut Follow, max: i32, what: &str| {
            let index = follow.moved.take()?;
            let value = choice_value(index, max);
            if value.is_none() {
                session.tell(format!("host: {what} {} has no value in the host (1-{max})", index + 1));
            }
            value
        };
        WindowChoices {
            pattern: value(&mut self.pattern, PATTERN_CHOICES, "pattern"),
            kit: value(&mut self.kit, KIT_CHOICES, "kit"),
        }
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

/// The thread that ticks the player every [`TICK_INTERVAL`], or at once
/// when the audio thread asks ([`Steps`]); dropping it stops the thread.
#[derive(Debug)]
pub struct Ticker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Ticker {
    pub fn spawn(player: SharedPlayer, params: Arc<OrdrumboxParams>, steps: Arc<Steps>) -> std::io::Result<Self> {
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let thread = std::thread::Builder::new().name("ordrumbox-player".into()).spawn(move || {
            while !stopped.load(Ordering::Acquire) {
                let asked = steps.wait(TICK_INTERVAL);
                lock(&player).tick(&params.library, params.controls(), &params.names);
                steps.done(asked);
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
    use ordrumbox_core::track::STRAIGHT_SWING;
    use ordrumbox_engine::HostTransport;

    use super::*;
    use crate::state::LibraryText;

    const FIRST: HostControls = HostControls { pattern: 0, kit: 0, muted: 0, swing: STRAIGHT_SWING };

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
    fn a_host_tempo_out_of_bounds_is_said_once() {
        let state = Arc::new(LibraryState::default());
        let mut player = Player::default();
        let mut audio = audio(player.start(48_000.0, &state));
        let (mut left, mut right) = (vec![0.0; 512], vec![0.0; 512]);
        let mut play_at = |player: &mut Player, bpm| {
            audio.render_synced(&mut left, &mut right, HostTransport { playing: true, bpm, beat: 0.0 });
            player.tick(&state, FIRST, &Names::default());
            player.ui().map(|ui| ui.session.message().to_owned()).unwrap_or_default()
        };
        assert_eq!(play_at(&mut player, 400.0), "host tempo 400 outside 20–300: plays at 300");
        // Said once: another message stays.
        if let Some(ui) = player.ui() {
            ui.session.run(ordrumbox_ui::Command::Undo);
        }
        assert_eq!(play_at(&mut player, 400.0), "nothing to undo");
        play_at(&mut player, 120.0);
        assert_eq!(play_at(&mut player, 10.0), "host tempo 10 outside 20–300: plays at 20");
    }

    #[test]
    fn the_host_chooses_the_pattern_when_its_parameter_changes() {
        let state = Arc::new(LibraryState::default());
        let names = Names::default();
        let mut player = Player::default();
        let text = r#"{"patterns":[{"name":"a","tracks":[]},{"name":"b","tracks":[]}]}"#;
        state.set(LibraryText(text.into()));
        let _audio = audio(player.start(48_000.0, &state));
        let shown = |player: &mut Player| player.ui().and_then(|ui| ui.session.pattern()?.name.clone());

        let b = HostControls { pattern: 1, ..FIRST };
        player.tick(&state, b, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("b"));
        assert!(player.take_redraw() && !player.take_redraw());
        assert_eq!(player.take_window_choices(), WindowChoices::default(), "the host's own choice");

        // Unchanged, the parameter leaves the window's own choice alone,
        // and the host is given it.
        player.ui().unwrap().session.step_pattern(1);
        player.tick(&state, b, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("a"));
        assert_eq!(player.take_window_choices(), WindowChoices { pattern: Some(1), kit: None });
        assert_eq!(player.take_window_choices(), WindowChoices::default());

        // Refused, then applied once the library has it.
        let third = HostControls { pattern: 2, ..FIRST };
        player.tick(&state, third, &names);
        assert_eq!(shown(&mut player).as_deref(), Some("a"));
        assert_eq!(player.ui().unwrap().session.message(), "host: no pattern 3 (the library has 2)");
        let session = &mut player.ui().unwrap().session;
        session.new_pattern();
        session.step_pattern(-2);
        player.tick(&state, third, &names);
        assert_eq!(player.ui().unwrap().session.pattern_index(), 2);
    }

    #[test]
    fn host_mutes_are_shown() {
        let state = Arc::new(LibraryState::default());
        let mut player = Player::default();
        let _audio = audio(player.start(48_000.0, &state));
        player.tick(&state, FIRST, &Names::default());
        player.take_redraw();
        player.tick(&state, HostControls { muted: 0b1, ..FIRST }, &Names::default());
        assert!(player.take_redraw());
        player.tick(&state, HostControls { muted: 0b1, ..FIRST }, &Names::default());
        assert!(!player.take_redraw());
    }

    #[test]
    fn the_ticker_stops_when_dropped() {
        let player = SharedPlayer::default();
        let ticker = Ticker::spawn(player.clone(), Arc::default(), Arc::default()).unwrap();
        drop(ticker);
        assert_eq!(Arc::strong_count(&player), 1);
    }
}
