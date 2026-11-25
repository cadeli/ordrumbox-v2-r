//! The edits several tools share.

use ordrumbox_core::steps::MAX_BEATS;

use crate::editor::Editor;
use ordrumbox_core::json::Value;

/// Absolute step `step` of a track as its beat and its step in the beat;
/// `None` past the beats a note can be on (`i32`).
pub(super) fn beat_and_step(step: u32, steps_per_beat: i32) -> Option<(i32, i32)> {
    let steps_per_beat = u32::try_from(steps_per_beat).ok().filter(|&s| s > 0).unwrap_or(1);
    Some((i32::try_from(step / steps_per_beat).ok()?, i32::try_from(step % steps_per_beat).ok()?))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_is_a_beat_and_a_step_in_it_until_past_i32() {
        assert_eq!(beat_and_step(9, 4), Some((2, 1)));
        assert_eq!(beat_and_step(3, 0), Some((3, 0)), "no steps a beat counts as one");
        assert_eq!(beat_and_step(u32::MAX, 1), None, "past i32: no beat holds it");
        assert_eq!(beat_and_step(u32::MAX, 4), Some((1_073_741_823, 3)));
    }
}
