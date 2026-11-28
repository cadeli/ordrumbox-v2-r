//! Drawing the pattern grid, its lane and the header.

use super::grid::draw_grid;
use super::kit::{KitLayout, draw_kit};
use super::layout::{Body, Layout};
use super::{
    ACCENT, HELP, KIT_HELP, MIXER_HELP, PLAYING, ROLL_HELP, SONG_HELP, SYNTH_HELP, View, WARNING, background_color,
};
use crate::canvas::{Canvas, chars_in};
use crate::frame::{body_top, footer_top, margin, status_line};
use crate::piano_roll::RollLayout;
use crate::scale::Scale;
use crate::song_view::SongLayout;
use crate::{piano_roll, song_view, theme};

/// Draw `view`; `scale` is the integer UI scale (1 on a standard display, 2 on HiDPI).
pub fn draw(canvas: &mut Canvas, view: &View, scale: Scale) {
    canvas.clear(background_color());
    draw_header(canvas, view, scale);
    draw_footer(canvas, view, scale);

    // The window split: one main editor top-left, the track editor down the right.
    let body = Body::new(canvas.width(), canvas.height(), scale, view.right, view.status.len(), view.note_editor);

    // The main editor (top-left): exactly one of the song panel, the kit
    // editor, the synth editor (painted by the session), the piano roll, the grid.
    if let Some(song) = &view.song_view {
        let layout = SongLayout::new(body.main_w, body.editor_h, scale, view.status.len(), song.zoom);
        song_view::draw(canvas, song, &layout);
    } else if let Some(kit) = &view.kit_view {
        let layout = KitLayout::new(body.main_w, body.editor_h, scale, view.status.len());
        draw_kit(canvas, kit, &layout);
    } else if view.synth || view.mixer {
        // Painted by the session (`draw_synth`, `draw_mixer`).
    } else if let Some((roll, title)) = &view.piano_roll {
        let spb = piano_roll::steps_per_beat(roll.track);
        let layout = RollLayout::new(body.main_w, body.editor_h, scale, view.status.len(), (spb, roll.shown_beats));
        piano_roll::draw(canvas, roll, &layout, title);
    } else if let Some(pattern) = view.pattern {
        let layout = Layout::new(
            body.main_w,
            body.editor_h,
            scale,
            pattern,
            view.status.len(),
            view.beat_window,
            view.lane_height,
        );
        draw_grid(canvas, pattern, view, &layout);
    }
}

/// The header: the status line and the header's lower edge. Its
/// controls, on their metal and cards, are panel items (`view/header.rs`).
fn draw_header(canvas: &mut Canvas, view: &View, scale: Scale) {
    draw_status_line(canvas, view, scale);
    canvas.fill_rect(0, body_top(scale) - margin(scale) / 2 + 2 * scale, canvas.width(), 2 * scale, ACCENT);
}

/// The second line of the header: the last action, undo and redo, the song.
fn draw_status_line(canvas: &mut Canvas, view: &View, scale: Scale) {
    let margin = margin(scale);
    let y = status_line(scale);
    let history = match (view.undo, view.redo) {
        (Some(undo), Some(redo)) => format!("undo: {undo}   redo: {redo}"),
        (Some(undo), None) => format!("undo: {undo}"),
        (None, Some(redo)) => format!("redo: {redo}"),
        (None, None) => String::new(),
    };
    let x = canvas.text(margin, y, view.message, scale, theme::text()) + 24 * scale;
    let x = canvas.text(x.max(margin + canvas.width() / 3), y, &history, scale, theme::muted()) + 24 * scale;
    if let Some(song) = view.song {
        let color = if song.starts_with("SONG") { PLAYING } else { theme::muted() };
        canvas.text(x.max(margin + canvas.width() * 2 / 3), y, song, scale, color);
    }
}

/// The footer: the problems at start, each a band of its own, hard to miss
/// until quit; then the help of the view.
fn draw_footer(canvas: &mut Canvas, view: &View, scale: Scale) {
    let margin = margin(scale);
    let mut footer_y = footer_top(canvas.height(), scale, view.status.len()) + 4 * scale;
    for message in view.status {
        canvas.fill_rect(0, footer_y - scale.px(), canvas.width(), 10 * scale, WARNING);
        let text: String = format!("! {message}").chars().take(chars_in(canvas.width() - 2 * margin, scale)).collect();
        canvas.text(margin, footer_y, &text, scale, background_color());
        footer_y += 10 * scale;
    }
    let help = if let Some(hint) = view.hint {
        hint
    } else if view.kit_view.is_some() {
        KIT_HELP
    } else if view.song_view.is_some() {
        SONG_HELP
    } else if view.piano_roll.is_some() {
        ROLL_HELP
    } else if view.synth {
        SYNTH_HELP
    } else if view.mixer {
        MIXER_HELP
    } else {
        HELP
    };
    let help: String = help.chars().take(chars_in(canvas.width() - 2 * margin, scale)).collect();
    canvas.text(margin, footer_y, &help, scale, theme::muted());
}
