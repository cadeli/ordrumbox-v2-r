//! Recording notes while the pattern plays: with recording on, a row key
//! (Q-P, the keys that hear the tracks) or a note played on a MIDI keyboard
//! also puts a note on its track, on the step the playhead is nearest to.
//! Each note is one undo step.

use super::Session;
use super::session_labels::beat_label;
use super::session_types::Workspace;
use ordrumbox_engine::Mode;

impl Session {
    pub fn recording(&self) -> bool {
        self.recording
    }

    /// Ctrl+R: recording on or off.
    pub fn toggle_record(&mut self) {
        self.recording = !self.recording;
        self.message = match (self.recording, self.playing()) {
            (false, _) => "recording off".into(),
            (true, true) => "recording: hit Q-P for the tracks, each key puts a note on the step".into(),
            (true, false) => "recording: Space plays, then hit Q-P for the tracks".into(),
        };
    }

    /// A row key hit while recording: a note on track `t` at the playhead.
    /// Nothing happens when recording is off; hitting it with nothing
    /// playing, in the song panel, or on a step that has a note says so.
    pub fn record_note(&mut self, t: usize) {
        self.record_at(t, 0.0, None);
    }

    /// A MIDI note played (key `key` of channel `channel`, `velocity` 0–1):
    /// recorded on the track of the pattern that key plays, at its pitch and
    /// its velocity, when recording.
    pub fn midi_note(&mut self, channel: u8, key: u8, velocity: f32) {
        if !self.recording {
            return;
        }
        match self.playback.midi_track(&self.editor, channel, key) {
            Some((t, pitch)) => self.record_at(t, pitch, Some(f64::from(velocity))),
            None => self.message = format!("recording: no track plays MIDI key {key}"),
        }
    }

    fn record_at(&mut self, t: usize, pitch: f64, velocity: Option<f64>) {
        if !self.recording {
            return;
        }
        if self.workspace == Workspace::Song || !matches!(self.playback.mode(), Some(Mode::Pattern(_))) {
            self.message = "recording: the pattern must play, in the grid or the piano roll".into();
            return;
        }
        let Some(position) = self.playback.position().filter(|_| self.playing()) else {
            self.message = "recording: Space plays the pattern first".into();
            return;
        };
        let Some(Mode::Pattern(p)) = self.playback.mode() else { return };
        let Some(pattern) = self.editor.library().patterns.get(p) else { return };
        let Some(track) = pattern.tracks.get(t) else { return };
        let (beat, step) = playhead_step(
            position.frame,
            position.frames,
            i64::from(pattern.beat_count.max(1)),
            i64::from(track.steps_per_beat.max(1)),
        );
        if track.notes.iter().any(|n| n.beat == beat && n.step == step && n.pitch == pitch) {
            self.message = format!("{} @ {}: a note is there already", track.name, beat_label(beat, step));
            return;
        }
        let name = track.name.clone();
        if self.editor.add_note_played(p, t, beat, step, pitch, velocity).is_some() {
            self.edited();
            self.message = format!("recorded {name} @ {}", beat_label(beat, step));
        }
    }
}

/// The beat and the step of a track (`steps_per_beat` a beat) nearest to
/// `frame` of a pass of `frames` over `beats` beats: the end of the pass is
/// the first step of the next one.
fn playhead_step(frame: u64, frames: u64, beats: i64, steps_per_beat: i64) -> (i32, i32) {
    let total = beats * steps_per_beat;
    let nearest = if frames == 0 {
        0
    } else {
        // Rounded to the nearest step, in integers: (2·frame·total + frames) / 2·frames.
        let scaled = (2 * u128::from(frame) * total as u128 + u128::from(frames)) / (2 * u128::from(frames));
        i64::try_from(scaled).unwrap_or(0).rem_euclid(total)
    };
    (i32::try_from(nearest / steps_per_beat).unwrap_or(0), i32::try_from(nearest % steps_per_beat).unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_playhead_snaps_to_the_nearest_step_and_the_end_wraps() {
        // 4 beats of 4 steps over 1600 frames: a step is 100 frames.
        assert_eq!(playhead_step(0, 1600, 4, 4), (0, 0));
        assert_eq!(playhead_step(49, 1600, 4, 4), (0, 0));
        assert_eq!(playhead_step(51, 1600, 4, 4), (0, 1));
        assert_eq!(playhead_step(1000, 1600, 4, 4), (2, 2));
        assert_eq!(playhead_step(1590, 1600, 4, 4), (0, 0), "nearer the next loop's start");
        assert_eq!(playhead_step(100, 0, 4, 4), (0, 0));
        // Another track's steps: three to a beat.
        assert_eq!(playhead_step(700, 1600, 4, 3), (1, 2));
    }
}
