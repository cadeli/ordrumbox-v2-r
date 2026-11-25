//! The edits several tools share.

use ordrumbox_core::steps::MAX_BEATS;

use crate::editor::Editor;
use ordrumbox_core::json::Value;

/// Absolute step `step` of a track as its beat and its step in the beat.
pub(super) fn beat_and_step(step: u32, steps_per_beat: i32) -> (i32, i32) {
    let steps_per_beat = steps_per_beat.max(1);
    (step as i32 / steps_per_beat, step as i32 % steps_per_beat)
}

/// Pattern `p` at least `beats` long: the next multiple of 4 beats (at most
/// 16). A track that looped over its whole length still does.
pub(super) fn grow_pattern(editor: &mut Editor, p: usize, beats: i32) {
    let pattern = &editor.library().patterns[p];
    if beats <= pattern.beat_count {
        return;
    }
    let whole: Vec<usize> = (0..pattern.tracks.len())
        .filter(|&t| {
            let track = &pattern.tracks[t];
            track.loop_steps == Some(track.beat_count * track.steps_per_beat)
        })
        .collect();
    let beats = ((beats + 3) / 4 * 4).min(MAX_BEATS);
    editor.set_pattern_beat_count(p, f64::from(beats));
    for t in whole {
        let length = beats * editor.library().patterns[p].tracks[t].steps_per_beat;
        editor.update_track(p, t, &[("loopLength", Value::from(length))], None, false);
    }
}
