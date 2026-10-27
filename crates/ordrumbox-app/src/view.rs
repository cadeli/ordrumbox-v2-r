//! The screen: header (pattern, tempo, kit, transport), one row of steps per
//! track with its notes, the playhead, and the key help.

use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Pattern, Track};

use crate::canvas::{Canvas, Rgb, text_width};

const BACKGROUND: Rgb = 0x0016_1619;
const CELL: Rgb = 0x002A_2A33;
const CELL_BEAT: Rgb = 0x0038_3845;
const NOTE: Rgb = 0x00F2_A93B;
const TEXT: Rgb = 0x00E6_E6EA;
const DIM: Rgb = 0x0080_8090;
const ACCENT: Rgb = 0x00F2_A93B;
const PLAYING: Rgb = 0x005C_D67A;
const WARNING: Rgb = 0x00E8_6A5A;
const PLAYHEAD: Rgb = 0x00FF_FFFF;
const LOOP_MARK: Rgb = 0x00B0_B0C0;

const HELP: &str = "SPACE play/stop   LEFT/RIGHT pattern   UP/DOWN tempo   K kit";

/// What to show.
#[derive(Debug)]
pub struct View<'a> {
    pub pattern: Option<&'a Pattern>,
    pub pattern_index: usize,
    pub pattern_count: usize,
    pub kit: &'a str,
    pub playing: bool,
    /// Playhead, as a fraction of the pattern length.
    pub playhead: Option<f64>,
    /// Problems to show above the key help, most important first.
    pub status: &'a [String],
}

/// Draw `view`; `scale` is the integer UI scale (1 on a standard display, 2 on HiDPI).
pub fn draw(canvas: &mut Canvas, view: &View, scale: i32) {
    canvas.clear(BACKGROUND);
    let text = 2 * scale; // the 8 px font at 16 px per scale unit
    let line = 8 * text + 8 * scale;
    let margin = 16 * scale;

    // Header.
    let mut y = margin;
    let x = canvas.text(margin, y, "orDrumbox", text, ACCENT) + 24 * scale;
    match view.pattern {
        Some(pattern) => {
            let name = pattern.name.as_deref().unwrap_or(&pattern.id);
            let title = format!("< {name} >  {}/{}", view.pattern_index + 1, view.pattern_count);
            canvas.text(x, y, &title, text, TEXT);
            y += line;
            let x = canvas.text(margin, y, &format!("{} BPM", pattern.bpm), text, TEXT) + 24 * scale;
            let x = canvas.text(x, y, &format!("kit {}", view.kit), text, DIM) + 24 * scale;
            let (state, color) = if view.playing { ("PLAY", PLAYING) } else { ("STOP", DIM) };
            canvas.text(x, y, state, text, color);
        }
        None => {
            canvas.text(x, y, "no pattern", text, DIM);
            y += line;
        }
    }
    y += line + margin / 2;

    // Footer: status lines then help, from the bottom up.
    let help_y = canvas.height() - margin - 8 * scale;
    canvas.text(margin, help_y, HELP, scale, DIM);
    let mut status_y = help_y - 12 * scale;
    for message in view.status.iter().rev() {
        status_y -= 10 * scale;
        canvas.text(margin, status_y, message, scale, WARNING);
    }

    if let Some(pattern) = view.pattern {
        draw_grid(canvas, pattern, view.playhead, (margin, y, canvas.width() - margin, status_y - margin), scale);
    }
}

/// The track rows inside `(left, top, right, bottom)`.
fn draw_grid(canvas: &mut Canvas, pattern: &Pattern, playhead: Option<f64>, area: (i32, i32, i32, i32), scale: i32) {
    let (left, top, right, bottom) = area;
    let tracks = pattern.tracks.len().max(1) as i32;
    let row = ((bottom - top) / tracks).clamp(10 * scale, 32 * scale);
    let label_scale = if row >= 24 * scale { 2 * scale } else { scale };
    let label_chars = 10;
    let grid_left = left + text_width(&"W".repeat(label_chars), label_scale) + 12 * scale;
    let grid_width = (right - grid_left).max(1);
    let beats = pattern.beat_count.max(1);
    let beat_width = f64::from(grid_width) / f64::from(beats);
    let gap = scale.max(1);

    for (i, track) in pattern.tracks.iter().enumerate() {
        let y = top + i as i32 * row;
        if y + row > bottom {
            break;
        }
        let label: String = track.name.chars().take(label_chars).collect();
        let label_y = y + (row - 8 * label_scale) / 2;
        canvas.text(left, label_y, &label, label_scale, if track.mute { DIM } else { TEXT });
        draw_track_steps(canvas, track, beats, (grid_left, y, beat_width, row - gap), gap);
    }

    if let Some(fraction) = playhead {
        let x = grid_left + (fraction.clamp(0.0, 1.0) * f64::from(grid_width)) as i32;
        let height = (pattern.tracks.len() as i32 * row).min(bottom - top) - gap;
        canvas.fill_rect(x - scale, top, 2 * scale, height, PLAYHEAD);
    }
}

/// One track's steps over `beats` beats, its notes lit by velocity.
fn draw_track_steps(canvas: &mut Canvas, track: &Track, beats: i32, row: (i32, i32, f64, i32), gap: i32) {
    let (left, y, beat_width, height) = row;
    let spb = track.steps_per_beat.clamp(1, 16);
    let steps = i64::from(beats) * i64::from(spb);
    let cell_width = beat_width / f64::from(spb);
    let step_x = |step: i64| left + (step as f64 * cell_width) as i32;

    let mut velocity = vec![None::<f64>; steps as usize];
    for note in &track.notes {
        let step = absolute_step(note.beat, note.beat_step, spb);
        if let Some(slot) = usize::try_from(step).ok().and_then(|s| velocity.get_mut(s)) {
            *slot = Some(slot.unwrap_or(0.0).max(note.velocity));
        }
    }
    for (step, note) in velocity.iter().enumerate() {
        let step = step as i64;
        let x = step_x(step);
        let width = (step_x(step + 1) - x - gap).max(1);
        let color = match note {
            Some(v) => mix(CELL, NOTE, 0.35 + 0.65 * v.clamp(0.0, 1.0)),
            None if step % i64::from(spb) == 0 => CELL_BEAT,
            None => CELL,
        };
        canvas.fill_rect(x, y, width, height, color);
    }
    if let Some(loop_at) = track.loop_at_step.map(i64::from).filter(|&l| l > 0 && l < steps) {
        canvas.fill_rect(step_x(loop_at) - gap, y, gap * 2, height, LOOP_MARK);
    }
}

/// `from` blended towards `to` by `amount` (0–1), per channel.
fn mix(from: Rgb, to: Rgb, amount: f64) -> Rgb {
    let channel = |shift: u32| {
        let a = f64::from((from >> shift) & 0xFF);
        let b = f64::from((to >> shift) & 0xFF);
        ((a + (b - a) * amount).round() as u32) << shift
    };
    channel(16) | channel(8) | channel(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_blends_channels() {
        assert_eq!(mix(0x000000, 0xFF8040, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xFF8040, 1.0), 0xFF8040);
        assert_eq!(mix(0x000000, 0xFF8040, 0.5), 0x804020);
    }

    #[test]
    fn draws_without_panicking_at_any_size() {
        let (library, _) = ordrumbox_core::Library::parse(
            r#"{"patterns":[{"name":"p","tracks":[{"name":"KICK","loopAtStep":3,"notes":[{},{"beat":9}]},{"name":"X","stepsPerBeat":0}]}]}"#,
        )
        .unwrap();
        for (w, h) in [(1, 1), (40, 30), (1024, 640)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let view = View {
                pattern: library.patterns.first(),
                pattern_index: 0,
                pattern_count: 1,
                kit: "real",
                playing: true,
                playhead: Some(0.5),
                status: &["warning".into()],
            };
            draw(&mut canvas, &view, 1);
        }
    }
}
