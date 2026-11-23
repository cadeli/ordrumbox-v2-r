//! Drawing the pattern grid, its lane and the header.

use super::grid::draw_grid;
use super::kit::{KitLayout, draw_kit};
use super::layout::{Body, HeaderControls, HostState, Layout, MACROS, Transport, header_controls, header_text};
use super::{
    ACCENT, ARRANGEMENT_LABEL, HELP, KIT_HELP, LOGO, MIXER_HELP, PATTERN_LABEL, PLAYING, ROLL_HELP, SONG_HELP,
    SYNTH_HELP, View, WAITING_LABEL, WARNING, background_color,
};
use crate::canvas::{Canvas, chars_in};
use crate::frame::{body_top, content_top, footer_top, margin, status_line};
use crate::panel::{Rect, stroke};
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
    if let Some(arrangement) = &view.arrangement {
        let layout = SongLayout::new(body.main_w, body.editor_h, scale, view.status.len(), arrangement.zoom);
        song_view::draw(canvas, arrangement, &layout);
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

/// The header: the logo, the labelled groups (their widgets are panel
/// items), the status line and the header's lower edge.
fn draw_header(canvas: &mut Canvas, view: &View, scale: Scale) {
    let text = header_text(scale);
    let margin = margin(scale);
    let top = content_top(scale);
    canvas.text(margin, top + 9 * scale, "orDrumbox", text, LOGO);
    let c = match view.transport {
        Transport::Own => header_controls(scale),
        Transport::Host(_) => header_controls(scale).hosted(),
    };
    // The groups' frames, behind their widgets (panel items, drawn later).
    // The generation's only with a pattern: it has nothing to show without.
    let song = view.arrangement.is_some();
    for (i, group) in c.groups(scale).into_iter().enumerate() {
        // The mode switch is the app's: a plugin has no song. The song mode has no generation.
        if (view.pattern.is_none() && i != 4)
            || (i == 0 && matches!(view.transport, Transport::Host(_)))
            || (i == 3 && song)
        {
            continue;
        }
        canvas.fill_rect(group.x, group.y, group.w, group.h, theme::panel());
        stroke(canvas, group, scale.px(), theme::line());
    }
    draw_header_labels(canvas, view, &c, top, scale);
    draw_status_line(canvas, view, scale);
    canvas.fill_rect(0, body_top(scale) - margin / 2 + 2 * scale, canvas.width(), 2 * scale, ACCENT);
}

/// The labels over the header's groups, in a muted colour: the values under
/// them stand out.
fn draw_header_labels(canvas: &mut Canvas, view: &View, c: &HeaderControls, top: i32, scale: Scale) {
    let label = |canvas: &mut Canvas, r: Rect, text: &str| {
        canvas.text(r.x, top, text, scale, theme::muted());
    };
    if view.pattern.is_none() {
        canvas.text(c.play.x, c.play.y + 4 * scale, "no pattern", scale, theme::muted());
        label(canvas, c.views[0], "VIEW");
        return;
    }
    if matches!(view.transport, Transport::Own) {
        label(canvas, c.mode[0], "MODE");
    }
    label(canvas, c.bpm, "BPM");
    if let Transport::Host(host) = view.transport {
        label(canvas, c.play, "HOST");
        draw_host_state(canvas, c, host, scale);
    }
    if view.arrangement.is_some() {
        label(canvas, c.pattern, ARRANGEMENT_LABEL);
        label(canvas, c.kit, "KIT");
        label(canvas, c.views[0], "VIEW");
        return;
    }
    match view.waiting {
        Some(on) => {
            let color = if on { PLAYING } else { theme::muted() };
            canvas.text(c.pattern.x, top, WAITING_LABEL, scale, color);
            if on {
                let r = c.pattern;
                let outer = Rect::new(r.x - scale.px(), r.y - scale.px(), r.w + 2 * scale, r.h + 2 * scale);
                stroke(canvas, outer, scale.px(), PLAYING);
            }
        }
        None => label(canvas, c.pattern, PATTERN_LABEL),
    }
    label(canvas, c.beats, "BEATS");
    label(canvas, c.kit, "KIT");
    label(canvas, c.style, "STYLE");
    for (rect, (_, text)) in c.macros.iter().zip(MACROS) {
        label(canvas, *rect, text);
    }
    label(canvas, c.views[0], "VIEW");
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
    } else if view.arrangement.is_some() {
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

/// A plugin host's transport, where the app has its play button and tempo
/// knob: a triangle while it plays, a square while stopped, and the tempo
/// played. Both keep their place before the host was seen, the tempo a dash.
pub(super) fn draw_host_state(canvas: &mut Canvas, c: &HeaderControls, host: Option<HostState>, scale: Scale) {
    let size = 8 * scale;
    let (x, y) = (c.play.x + 2 * scale, c.play.y + (c.play.h - size) / 2);
    if host.is_some_and(|host| host.playing) {
        // Pointing right: each column shorter by a pixel at both ends.
        for col in 0..size {
            let inset = col / 2;
            canvas.fill_rect(x + col, y + inset, 1, size - 2 * inset, PLAYING);
        }
    } else {
        canvas.fill_rect(x, y, size, size, theme::muted());
    }
    let (tempo, color) = match host {
        Some(host) => (ordrumbox_core::num::text(host.bpm), theme::text()),
        None => ("-".to_owned(), theme::muted()),
    };
    canvas.text(c.bpm.x, c.bpm.y + 4 * scale, &tempo, scale, color);
}
