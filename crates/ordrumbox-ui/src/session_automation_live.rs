//! The song's automation and the mixer while the song plays, as a DAW has
//! them. The mixer shows what the lanes set (a fader moves with its lane).
//! A control of the mixer moved over a lane takes it back: the lane is no
//! longer played for that instrument, until Play > Re-enable automation.
//! With Play > Record automation on, moving the control writes its lane
//! instead (made if there is none): a fader or a knob (volume, pan,
//! cutoff, Q) as long as it is held, from
//! step to step under the playhead; a button from the playhead's step to
//! the lane's next point. A gesture is one undo step.

use super::Session;
use ordrumbox_core::automation::{
    AutomationKind, AutomationStroke, AutomationTarget, STEPS_PER_MEASURE, is_on, switch_value,
};
use ordrumbox_core::steps::TICK;

/// What a control of the mixer moved while the song plays did to its lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MixerMove {
    /// Written to its lane: the track is left as it is.
    Written,
    /// Its lane taken back: the track takes the value.
    TakenBack,
    /// No lane to take back, or the song does not play.
    Untouched,
}

/// The live state of the song's automation.
#[derive(Debug, Default)]
pub(super) struct LiveAutomation {
    /// Play > Record automation: the mixer writes the lanes while the song plays.
    is_recording: bool,
    /// The lanes taken back, of the song `song`: an instrument and its control.
    overridden: Vec<(String, AutomationTarget)>,
    song: usize,
    /// The fader held while recording.
    writing: Option<Writing>,
}

/// A fader writing its lane while it is held.
#[derive(Debug, Clone, PartialEq)]
struct Writing {
    lane: usize,
    /// The lane's value it writes.
    value: f64,
    /// The last step written.
    last_step: u32,
    /// The undo key of this gesture's edits.
    key: String,
}

impl Session {
    /// The step (with its fraction) the song is playing; `None` when it does not play.
    fn playing_position(&self) -> Option<f64> {
        if !self.song_playing() {
            return None;
        }
        let tick = self.playback.tick()?;
        Some(self.song()?.automation_position_at(tick, TICK))
    }

    /// The lanes of the selected song taken back from.
    pub(super) fn overridden_lanes(&self) -> &[(String, AutomationTarget)] {
        let live = &self.automation_live;
        if live.song == self.editor.selected_song() { &live.overridden } else { &[] }
    }

    /// What the song's lanes set `target` of track `t` to now, the song
    /// playing and the lane not taken back: a fader's gain, a button's 0 or 1.
    pub(super) fn automated_now(&self, t: usize, target: AutomationTarget) -> Option<f64> {
        let position = self.playing_position()?;
        let name = &self.track(t)?.name;
        if self.overridden_lanes().iter().any(|(n, tg)| n == name && *tg == target) {
            return None;
        }
        let lanes = &self.song()?.automations;
        let value = ordrumbox_core::automation::automated_at(lanes, name, target, position)?;
        Some(target.to_control(value))
    }

    /// The track editor's knobs of track `t` (`lfo`: a key and the value
    /// shown) moved by the song's level lanes while it plays, as an LFO
    /// would move them.
    pub(super) fn show_lanes_on_knobs(&self, t: usize, lfo: &mut Vec<(&'static str, f64)>) {
        let levels = AutomationTarget::ALL.into_iter().filter(|target| target.kind() == AutomationKind::Level);
        for target in levels {
            if let Some(value) = self.automated_now(t, target) {
                lfo.retain(|&(key, _)| key != target.track_key());
                lfo.push((target.track_key(), value));
            }
        }
    }

    /// Mute and solo of track `t` as shown: what the song's lanes set while
    /// it plays, else the track's own.
    pub(super) fn shown_switch(&self, t: usize) -> (bool, bool) {
        let Some(track) = self.track(t) else { return (false, false) };
        let shown = |target, own| self.automated_now(t, target).map_or(own, is_on);
        (shown(AutomationTarget::Mute, track.mute), shown(AutomationTarget::Solo, track.solo))
    }

    /// Every track's [`Self::shown_switch`] while the song plays; empty
    /// otherwise (the views show the tracks' own).
    pub(super) fn shown_switches(&self) -> Vec<(bool, bool)> {
        if self.playing_position().is_none() {
            return Vec::new();
        }
        let count = self.pattern().map_or(0, |pattern| pattern.tracks.len());
        (0..count).map(|t| self.shown_switch(t)).collect()
    }

    /// Mute or solo (`key`) of track `t` switched from what is shown (the
    /// mixer, the grid, M, S, 1-9): written to its lane while the song plays
    /// and records automation, else its lane taken back.
    pub fn switch_track(&mut self, t: usize, key: &str) {
        let target = if key == "mute" { AutomationTarget::Mute } else { AutomationTarget::Solo };
        let Some(own) = self.track(t).map(|track| if key == "mute" { track.mute } else { track.solo }) else { return };
        let is_on = !self.automated_now(t, target).map_or(own, is_on);
        let moved = self.automate_from_mixer(t, target, switch_value(is_on));
        if moved == MixerMove::Written {
            return;
        }
        if is_on != own {
            self.toggle_row(t, key);
        }
        if moved == MixerMove::TakenBack {
            self.say_taken_back(t, target);
        }
    }

    /// Track `t`'s `target` set to `value` from the mixer (a fader's gain, a
    /// button's 0 or 1) while the song plays: written to its lane when
    /// recording, else its lane taken back.
    pub(super) fn automate_from_mixer(&mut self, t: usize, target: AutomationTarget, value: f64) -> MixerMove {
        let Some(position) = self.playing_position() else { return MixerMove::Untouched };
        let Some(name) = self.track(t).map(|track| track.name.clone()) else { return MixerMove::Untouched };
        let lane = self.song().and_then(|song| song.automations.iter().position(|l| l.is_for(&name, target)));
        if !self.automation_live.is_recording {
            if lane.is_none() || self.overridden_lanes().iter().any(|(n, tg)| *n == name && *tg == target) {
                return MixerMove::Untouched;
            }
            self.take_back(name, target);
            return MixerMove::TakenBack;
        }
        let lane = match lane {
            Some(lane) => lane,
            None => match self.editor.add_automation_lane(None, &name, target) {
                Ok(lane) => lane,
                Err(message) => {
                    self.message = message;
                    return MixerMove::Written;
                }
            },
        };
        let step = position.floor() as u32;
        match target.kind() {
            AutomationKind::Level => self.write_level(lane, step, target.from_control(value)),
            AutomationKind::Switch => self.write_switch(lane, step, value),
        }
        MixerMove::Written
    }

    /// The lane of `name`'s `target` taken back: played no more until re-enabled.
    fn take_back(&mut self, name: String, target: AutomationTarget) {
        let song = self.editor.selected_song();
        let live = &mut self.automation_live;
        if live.song != song {
            (live.song, live.overridden) = (song, Vec::new());
        }
        live.overridden.push((name, target));
    }

    /// What the mixer says once a control took its lane back.
    pub(super) fn say_taken_back(&mut self, t: usize, target: AutomationTarget) {
        let name = self.track_name(t);
        self.message = format!("{name} {} taken back from its lane: Play > Re-enable automation", target.label());
    }

    /// Play > Re-enable automation: every lane taken back plays again.
    pub fn reenable_automation(&mut self) {
        let count = std::mem::take(&mut self.automation_live.overridden).len();
        self.message = match count {
            0 => "no lane taken back".into(),
            1 => "its lane plays again".into(),
            _ => format!("{count} lanes play again"),
        };
    }

    /// Play > Record automation: on or off.
    pub fn toggle_automation_recording(&mut self) {
        let live = &mut self.automation_live;
        live.is_recording = !live.is_recording;
        live.writing = None;
        self.message = if live.is_recording {
            "record automation on: the mixer writes the song's lanes while it plays".into()
        } else {
            "record automation off".into()
        };
    }

    pub(super) fn is_recording_automation(&self) -> bool {
        self.automation_live.is_recording
    }

    /// A button written at `step` of lane `lane`: `value` up to the lane's next point.
    fn write_switch(&mut self, lane: usize, step: u32, value: f64) {
        let Some(base) = self.song().and_then(|song| song.automations.get(lane)).cloned() else { return };
        let total = self.song().map_or(1, |song| song.length_measures().max(1.0).ceil() as u32) * STEPS_PER_MEASURE;
        let next = base.points().iter().find(|point| point.step > step).map_or(total, |point| point.step);
        let key = format!("automation-write:{lane}:{}", self.editor.revision());
        let stroke = AutomationStroke::Fill(Some(value));
        self.paint_written(lane, &base, (step, next.saturating_sub(1).max(step)), stroke, &key);
    }

    /// A fader's `level` written at `step` of lane `lane`, its gesture
    /// begun or carried on.
    fn write_level(&mut self, lane: usize, step: u32, level: f64) {
        let writing = match self.automation_live.writing.take().filter(|w| w.lane == lane) {
            Some(writing) => Writing { value: level, ..writing },
            None => Writing {
                lane,
                value: level,
                last_step: step,
                key: format!("automation-write:{lane}:{}", self.editor.revision()),
            },
        };
        self.automation_live.writing = Some(writing);
        self.write_held(step);
    }

    /// The fader held written up to `step`: from the step after the last
    /// written (or `step` itself when the playhead went back, a loop).
    fn write_held(&mut self, step: u32) {
        let Some(writing) = self.automation_live.writing.clone() else { return };
        let Some(base) = self.song().and_then(|song| song.automations.get(writing.lane)).cloned() else { return };
        let from = if step > writing.last_step { writing.last_step + 1 } else { step };
        let stroke = AutomationStroke::Fill(Some(writing.value));
        self.paint_written(writing.lane, &base, (from, step), stroke, &writing.key);
        if let Some(writing) = &mut self.automation_live.writing {
            writing.last_step = step;
        }
    }

    fn paint_written(
        &mut self,
        lane: usize,
        base: &ordrumbox_core::AutomationLane,
        zone: (u32, u32),
        stroke: AutomationStroke,
        key: &str,
    ) {
        match self.editor.paint_automation(None, lane, base, zone, stroke, key) {
            Ok(()) => self.edited(),
            Err(message) => self.message = message,
        }
    }

    /// Every frame: a control held while recording (a fader, a knob)
    /// writes on under the playhead; released, or the song stopped, its
    /// gesture ends. The lanes taken back reach the engine.
    pub(super) fn update_live_automation(&mut self) {
        // Only a level written by a control sets `writing`: the drag is its.
        let held = self.chrome.widgets.drag.is_some();
        match self.playing_position() {
            Some(position) if held && self.automation_live.writing.is_some() => {
                self.write_held(position.floor() as u32)
            }
            _ => self.automation_live.writing = None,
        }
        let overridden = self.overridden_lanes().to_vec();
        self.playback.set_overridden(&self.editor, &overridden);
    }
}
