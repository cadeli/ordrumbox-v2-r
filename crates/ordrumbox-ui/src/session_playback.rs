//! The session's playback: the engine's controller and the sound output it
//! feeds, the editor revision the playing pass was built from, and the loop
//! of a pattern in auto mode posted ahead. The session reaches the engine
//! only through it.

use ordrumbox_core::Track;
use ordrumbox_engine::{Controller, HostMix, HostTransport, Mode, Position};
use ordrumbox_logic::Editor;

use crate::audio_output::{AudioOutput, OutputDevices, StartOutput};

/// The next loop of a pattern in auto mode, posted ahead to the engine. Its
/// lines enter the library when that loop starts: what the grid shows and what
/// an edit changes is the loop playing.
#[derive(Debug)]
struct UpcomingLoop {
    pattern_index: usize,
    pass: u64,
    tracks: Vec<Track>,
}

/// What the program lets the user choose, and the MIDI input it has open:
/// kept when the session reloads.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct Offers {
    /// Lists the sound devices (`None`: the program offers no choice).
    pub(super) devices: Option<OutputDevices>,
    /// Lists the MIDI inputs (`None`: the program has no MIDI input).
    pub(super) midi_ports: Option<OutputDevices>,
    /// A MIDI input is open.
    pub(super) midi_open: bool,
    /// Lists the MIDI outputs (`None`: the program has no MIDI output).
    pub(super) midi_out_ports: Option<OutputDevices>,
    /// The transport follows the MIDI clock of the input.
    pub(super) clock_follow: bool,
    /// The machine whose clock is followed plays.
    pub(super) clock_running: bool,
}

/// The engine as the session drives it; without a sound device, everything
/// it is asked does nothing and it never plays.
#[derive(Default)]
pub(super) struct Playback {
    /// `None` when no audio device could be opened.
    controller: Option<Controller>,
    output: Option<Box<dyn AudioOutput>>,
    /// What started `output`, to start it again when the session reloads
    /// (`None`: a session built without assets or by a test).
    start_output: Option<StartOutput>,
    /// What the program offers for the sound and the MIDI input.
    offers: Offers,
    /// Editor revision the playing pass was built from.
    played_revision: u64,
    upcoming_loop: Option<UpcomingLoop>,
}

impl std::fmt::Debug for Playback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Playback")
            .field("audio", &self.controller.is_some())
            .field("played_revision", &self.played_revision)
            .finish_non_exhaustive()
    }
}

impl Playback {
    /// The playback of `controller`, playing through `output`.
    pub(super) fn new(output: Box<dyn AudioOutput>, controller: Controller) -> Self {
        Self { controller: Some(controller), output: Some(output), ..Self::default() }
    }

    /// The playback of `controller` with no sound output (a test renders its audio side).
    #[cfg(test)]
    pub(super) fn without_output(controller: Controller) -> Self {
        Self { controller: Some(controller), ..Self::default() }
    }

    /// Keep `start_output`, to start the sound again on a reload.
    pub(super) fn keep_start_output(&mut self, start_output: StartOutput) {
        self.start_output = Some(start_output);
    }

    pub(super) fn offers(&self) -> Offers {
        self.offers
    }

    pub(super) fn offers_mut(&mut self) -> &mut Offers {
        &mut self.offers
    }

    /// The MIDI input open or not: the notes it plays reach the pattern's tracks.
    pub(super) fn set_live_midi(&mut self, on: bool) {
        if let Some(controller) = &mut self.controller {
            controller.set_live_midi(on);
        }
    }

    /// Pattern `index` plays now, if a pattern plays: its pass starts over.
    pub(super) fn restart_pattern(&mut self, editor: &Editor, index: usize) {
        let Some(controller) = &mut self.controller else { return };
        controller.select_pattern(index);
        if let (true, Mode::Pattern(_), Some(at)) = (controller.is_playing(), controller.mode(), controller.position())
        {
            self.upcoming_loop = None;
            controller.jump(editor.library(), at.pass);
            self.played_revision = editor.revision();
        }
    }

    /// The live mix a MIDI controller's knobs make.
    pub(super) fn set_live_mix(&mut self, mix: HostMix) {
        if let Some(controller) = &mut self.controller {
            controller.set_live_mix(mix);
        }
    }

    /// The transport follows a MIDI clock, or does not any more.
    pub(super) fn set_clock_follow(&mut self, editor: &Editor, on: bool) {
        match &mut self.controller {
            Some(controller) if on => controller.follow_clock_again(),
            Some(controller) => controller.leave_host(editor.library()),
            None => {}
        }
    }

    /// The track and pitch MIDI key `key` of channel `channel` plays.
    pub(super) fn midi_track(&mut self, editor: &Editor, channel: u8, key: u8) -> Option<(usize, f64)> {
        self.controller.as_mut()?.midi_track(editor.library(), channel, key)
    }

    /// What started the sound, given up for a new session.
    pub(super) fn take_start_output(&mut self) -> Option<StartOutput> {
        self.start_output.take()
    }

    /// An error the sound output reported since the last call.
    pub(super) fn take_output_error(&self) -> Option<String> {
        self.output.as_ref().and_then(|output| output.take_error())
    }

    /// The controller, for what only it is asked (`None`: no sound).
    pub(super) fn controller(&self) -> Option<&Controller> {
        self.controller.as_ref()
    }

    /// The controller, for what is posted to it without changing what plays.
    pub(super) fn controller_mut(&mut self) -> Option<&mut Controller> {
        self.controller.as_mut()
    }

    pub(super) fn is_playing(&self) -> bool {
        self.controller.as_ref().is_some_and(Controller::is_playing)
    }

    /// What plays, or played last (`None`: no sound).
    pub(super) fn mode(&self) -> Option<Mode> {
        self.controller.as_ref().map(Controller::mode)
    }

    /// Whether `mode` is what plays now.
    pub(super) fn plays(&self, mode: Mode) -> bool {
        self.is_playing() && self.mode() == Some(mode)
    }

    pub(super) fn position(&self) -> Option<Position> {
        self.controller.as_ref().and_then(Controller::position)
    }

    pub(super) fn tick(&self) -> Option<u64> {
        self.controller.as_ref().and_then(Controller::tick)
    }

    /// The plugin host's transport, once seen.
    pub(super) fn host(&self) -> Option<HostTransport> {
        self.controller.as_ref().and_then(Controller::host)
    }

    /// The tracks' levels now (none without sound).
    pub(super) fn levels(&self) -> Vec<f32> {
        self.controller.as_ref().map(Controller::levels).unwrap_or_default()
    }

    /// Play `mode` of `editor`'s library from pass `first`.
    pub(super) fn play_from(&mut self, editor: &Editor, mode: Mode, first: u64) {
        let Some(controller) = &mut self.controller else { return };
        self.upcoming_loop = None;
        controller.play_from(editor.library(), mode, first);
        self.played_revision = editor.revision();
    }

    /// Play `mode` of `editor`'s library where it is.
    pub(super) fn play(&mut self, editor: &Editor, mode: Mode) {
        let Some(controller) = &mut self.controller else { return };
        self.upcoming_loop = None;
        controller.play(editor.library(), mode);
        self.played_revision = editor.revision();
    }

    /// The measures a song plays in a loop (`None`: on to its end).
    pub(super) fn set_song_loop(&mut self, editor: &Editor, region: Option<(u64, u64)>) {
        if let Some(controller) = &mut self.controller {
            controller.set_song_loop(editor.library(), region);
        }
    }

    /// The metronome on or off.
    pub(super) fn set_metronome(&mut self, editor: &Editor, on: bool) {
        if let Some(controller) = &mut self.controller {
            controller.set_metronome(editor.library(), on);
        }
    }

    /// Stop; the loop posted ahead is dropped with what played.
    pub(super) fn stop(&mut self) {
        if let Some(controller) = &mut self.controller {
            controller.stop();
        }
        self.upcoming_loop = None;
    }

    /// Keep the engine fed with `editor`'s library, pattern `pattern_index`
    /// selected. The next pass of a pattern is built with the new lines
    /// `auto_loop` gives its tracks in auto mode (the JS loop start); they
    /// replace the library's when that pass starts.
    pub(super) fn update(
        &mut self,
        editor: &mut Editor,
        pattern_index: usize,
        mut auto_loop: impl FnMut(&mut Editor, usize, u64) -> bool,
    ) {
        let Some(controller) = &mut self.controller else { return };
        controller.select_pattern(pattern_index);
        if editor.revision() != self.played_revision {
            // The loop ahead was generated before the edit: it is generated again.
            self.upcoming_loop = None;
            controller.refresh(editor.library());
        }
        if !matches!(controller.mode(), Mode::Pattern(_)) {
            self.upcoming_loop = None;
        }
        // Read before the position: asking for the pass after the upcoming
        // one means that one has started, and the position then says so.
        let pending = controller.pending_pass();
        let position = controller.position();
        if let Some(mut started) = self.upcoming_loop.take_if(|u| position.is_some_and(|at| at.pass >= u.pass)) {
            editor.swap_tracks(started.pattern_index, &mut started.tracks);
        }
        let mut upcoming = None;
        if let (Some(pass), Mode::Pattern(p)) = (pending, controller.mode())
            && let Some(playing) = editor.library().patterns.get(p).map(|pattern| pattern.tracks.clone())
            && auto_loop(editor, p, pass)
        {
            upcoming = Some(UpcomingLoop { pattern_index: p, pass, tracks: playing });
        }
        controller.update(editor.library());
        if let Some(mut next) = upcoming {
            editor.swap_tracks(next.pattern_index, &mut next.tracks);
            self.upcoming_loop = Some(next);
        }
        self.played_revision = editor.revision();
    }
}
