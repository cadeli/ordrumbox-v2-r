//! Drawing the pattern grid, its lane and the header.

use super::kit::{KitLayout, draw_kit};
use super::layout::{
    ADD_TRACK_LABEL, Body, HeaderControls, HostState, Layout, MACROS, RULER_HEIGHT, Transport, header_controls,
    header_text, steps_per_beat,
};
use super::{
    ACCENT, AUTO, CURSOR, HELP, KIT_HELP, LABEL_CHARS, LOGO, LOOP_MARK, MUTE_ON, PAGE_STEPS_PER_BEAT, PATTERN_LABEL,
    PLAYHEAD, PLAYING, ROLL_HELP, SELECTED, SOLO_ON, SONG_HELP, SYNTH_HELP, View, WAITING_LABEL, WARNING,
    background_color, box_off_color, cell_beat_color, cell_color, focus_row_color, mute_host_color,
};
use crate::canvas::{Canvas, Rgb, chars_in, small_cell, small_char_width, text_width};
use crate::frame::{body_top, content_top, footer_top, margin, status_line};
use crate::output::OutputLayout;
use crate::panel::Rect;
use crate::piano_roll::RollLayout;
use crate::scale::Scale;
use crate::song_view::SongLayout;
use crate::theme::{ZONE, ZONE_OPACITY};
use crate::{editors, output, piano_roll, song_view, theme};
use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Note, Pattern, Track};
use ordrumbox_logic::block::Zone;

/// Draw `view`; `scale` is the integer UI scale (1 on a standard display, 2 on HiDPI).
pub fn draw(canvas: &mut Canvas, view: &View, scale: Scale) {
    canvas.clear(background_color());
    let text = header_text(scale);
    let margin = margin(scale);

    // Header: the logo, then the labelled groups (their widgets are panel items).
    let top = content_top(scale);
    canvas.text(margin, top + 9 * scale, "orDrumbox", text, LOGO);
    let c = match view.transport {
        Transport::Own => header_controls(scale),
        Transport::Host(_) => header_controls(scale).hosted(),
    };
    // The groups' frames, behind their widgets (panel items, drawn later).
    // The generation's only with a pattern: it has nothing to show without.
    for (i, group) in c.groups(scale).into_iter().enumerate() {
        // The mode switch is the app's: a plugin has no song.
        if (view.pattern.is_none() && i != 4) || (i == 0 && matches!(view.transport, Transport::Host(_))) {
            continue;
        }
        canvas.fill_rect(group.x, group.y, group.w, group.h, theme::panel());
        stroke(canvas, group.x, group.y, group.w, group.h, scale.px(), theme::line());
    }
    // Labels in a muted color: the values under them stand out.
    let label = |canvas: &mut Canvas, r: Rect, text: &str| {
        canvas.text(r.x, top, text, scale, theme::muted());
    };
    if view.pattern.is_some() {
        if matches!(view.transport, Transport::Own) {
            label(canvas, c.mode[0], "MODE");
        }
        label(canvas, c.bpm, "BPM");
        if let Transport::Host(host) = view.transport {
            label(canvas, c.play, "HOST");
            draw_host_state(canvas, &c, host, scale);
        }
        match (view.waiting, view.pattern) {
            (Some(on), _) => {
                let color = if on { PLAYING } else { theme::muted() };
                canvas.text(c.pattern.x, top, WAITING_LABEL, scale, color);
                if on {
                    let r = c.pattern;
                    stroke(
                        canvas,
                        r.x - scale.px(),
                        r.y - scale.px(),
                        r.w + 2 * scale,
                        r.h + 2 * scale,
                        scale.px(),
                        PLAYING,
                    );
                }
            }
            (None, _) => label(canvas, c.pattern, PATTERN_LABEL),
        }
        label(canvas, c.beats, "BEATS");
        label(canvas, c.kit, "KIT");
        label(canvas, c.style, "STYLE");
        for (rect, (_, text)) in c.macros.iter().zip(MACROS) {
            label(canvas, *rect, text);
        }
    } else {
        canvas.text(c.play.x, c.play.y + 4 * scale, "no pattern", scale, theme::muted());
    }
    label(canvas, c.views[0], "VIEW");

    // Second line: the last action, undo and redo, the song.
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
    // The header's lower edge.
    canvas.fill_rect(0, body_top(scale) - margin / 2 + 2 * scale, canvas.width(), 2 * scale, ACCENT);

    // Footer: status lines then help.
    let mut footer_y = footer_top(canvas.height(), scale, view.status.len()) + 4 * scale;
    // Problems at start: a band of their own, hard to miss, until quit.
    for message in view.status {
        canvas.fill_rect(0, footer_y - scale.px(), canvas.width(), 10 * scale, WARNING);
        let text: String = format!("! {message}").chars().take(chars_in(canvas.width() - 2 * margin, scale)).collect();
        canvas.text(margin, footer_y, &text, scale, background_color());
        footer_y += 10 * scale;
    }
    let help = if view.kit_view.is_some() {
        KIT_HELP
    } else if view.arrangement.is_some() {
        SONG_HELP
    } else if view.piano_roll.is_some() {
        ROLL_HELP
    } else if view.synth {
        SYNTH_HELP
    } else {
        HELP
    };
    canvas.text(margin, footer_y, help, scale, theme::muted());

    // The window split: one main editor top-left, the track editor down the
    // right, the output strip along the bottom.
    let body = Body::new(
        canvas.width(),
        canvas.height(),
        scale,
        view.right,
        view.status.len(),
        view.output.is_some(),
        view.note_editor,
    );

    // The main editor (top-left): exactly one of the song panel, the kit
    // editor, the synth editor (painted by the session), the piano roll, the grid.
    if let Some(arrangement) = &view.arrangement {
        let layout = SongLayout::new(body.main_w, body.editor_h, scale, view.status.len());
        song_view::draw(canvas, arrangement, &layout);
    } else if let Some(kit) = &view.kit_view {
        let layout = KitLayout::new(body.main_w, body.editor_h, scale, view.status.len());
        draw_kit(canvas, kit, &layout);
    } else if view.synth {
        // Painted by the session (`draw_synth`).
    } else if let Some((roll, title)) = &view.piano_roll {
        let spb = piano_roll::steps_per_beat(roll.track);
        let layout = RollLayout::new(body.main_w, body.editor_h, scale, view.status.len(), spb);
        piano_roll::draw(canvas, roll, &layout, title);
    } else if let Some(pattern) = view.pattern {
        let layout = Layout::new(body.main_w, body.editor_h, scale, pattern, view.status.len(), view.current_page);
        draw_grid(canvas, pattern, view, &layout);
    }

    // The output panel, always shown as a strip along the bottom.
    if let Some(out) = &view.output {
        output::draw(canvas, out, &OutputLayout::strip(canvas.width(), body.output, scale));
    }
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

/// The beat ruler above the rows: each beat's number (the one playing lit),
/// a tick where it starts, and a dot per step of the focused track.
pub(super) fn draw_ruler(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let y = layout.top - RULER_HEIGHT * scale;
    let spb =
        view.cursor.and_then(|c| pattern.tracks.get(c.track)).or(pattern.tracks.first()).map_or(4, steps_per_beat);
    let playing = view.playhead.map(|f| (f.clamp(0.0, 1.0) * f64::from(layout.beats)).floor() as i32);
    // Every beat numbered when they fit, else every second, fourth...
    let widest = text_width(&(layout.start_beat + layout.beats).to_string(), scale) + 4 * scale;
    let mut every = 1;
    while layout.beat_width() * f64::from(every) < f64::from(widest) && every < layout.beats {
        every *= 2;
    }
    // The page, left of the ruler, when the pattern takes more than one.
    let pages = (pattern.beat_count.max(1) as usize).div_ceil(layout.page_beats as usize);
    if pages > 1 {
        let text = format!("page {}/{pages}", view.current_page + 1);
        canvas.text(layout.name_x, y, &text, scale, theme::muted());
    }
    let dot_y = layout.top - 3 * scale;
    for beat in 0..layout.beats {
        let x = layout.beat_x(beat);
        canvas.fill_rect(x, y, scale.px(), RULER_HEIGHT * scale - 2 * scale, theme::muted());
        for step in 1..spb {
            let sx = layout.step_x(i64::from(beat) * i64::from(spb) + i64::from(step), spb);
            canvas.fill_rect(sx, dot_y, scale.px(), scale.px(), theme::muted());
        }
        if beat % every == 0 {
            let color = if playing == Some(beat) { PLAYING } else { theme::muted() };
            canvas.text(x + 3 * scale, y, &(layout.start_beat + beat + 1).to_string(), scale, color);
        }
    }
}

pub(super) fn draw_grid(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let gap = scale.px();
    draw_ruler(canvas, pattern, view, layout);
    for (i, track) in pattern.tracks.iter().enumerate().take(layout.tracks) {
        let y = layout.row_top(i);
        let focused = view.cursor.is_some_and(|c| c.track == i);
        if focused {
            // The whole row, its steps included.
            let right = layout.grid_left + layout.grid_width;
            canvas.fill_rect(
                layout.left - 4 * scale,
                y,
                right - layout.left + 4 * scale,
                layout.row - gap,
                focus_row_color(),
            );
        }
        // The track's VU meter stripe (level from the audio engine).
        let stripe_x = layout.left - 8 * scale;
        let stripe_w = 3 * scale;
        let stripe_top = y + gap;
        let stripe_h = layout.row - 3 * gap;
        let level = view.levels.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
        if view.show_vus {
            canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::mix(theme::track(i), theme::bg(), 0.75));
            let fill_h = (f64::from(stripe_h) * f64::from(level)).round() as i32;
            if fill_h > 0 {
                canvas.fill_rect(stripe_x, stripe_top + stripe_h - fill_h, stripe_w, fill_h, theme::track(i));
            }
        } else {
            canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::track(i));
        }
        let box_y = y + (layout.row - layout.box_size) / 2;
        // A mute from the host is lit in its own colour: the track's own stays off.
        let host_mute = ordrumbox_engine::host_muted(view.host_muted, i);
        let (mute, mute_color) = if track.mute { (true, MUTE_ON) } else { (host_mute, mute_host_color()) };
        for (solo, on, color, letter) in [(false, mute, mute_color, "M"), (true, track.solo, SOLO_ON, "S")] {
            let x = layout.box_x(solo);
            canvas.fill_rect(x, box_y, layout.box_size, layout.box_size, if on { color } else { box_off_color() });
            let letter_scale = Scale::whole((layout.box_size / 10).max(1));
            let inset = (layout.box_size - 8 * letter_scale) / 2;
            canvas.text(
                x + inset,
                box_y + inset,
                letter,
                letter_scale,
                if on { background_color() } else { theme::muted() },
            );
        }
        let label: String = track.name.chars().take(LABEL_CHARS).collect();
        // The name, and under it, in the small font, what the track plays:
        // its kit, then its sample, as many lines as the row has room for,
        // the sample's first (all of it in the name's tooltip). The room
        // alone sets the lines, so the name never moves with the sound.
        let sound_h = small_cell(scale) + 2 * scale;
        let lines = usize::try_from((layout.row - 8 * layout.scale) / sound_h.max(1)).unwrap_or(0).min(2);
        let label_y = y + (layout.row - 8 * layout.scale - lines as i32 * sound_h) / 2;
        if let Some(sound) = view.sounds.get(i) {
            let width = layout.volume_x - layout.name_x - 4 * scale;
            let chars = usize::try_from(width / small_char_width(scale).max(1)).unwrap_or(0);
            let shown = [(&sound.source, theme::disabled()), (&sound.name, theme::muted())];
            for (line, (text, color)) in shown[2 - lines..].iter().enumerate() {
                let text: String = text.chars().take(chars).collect();
                let line_y = label_y + 8 * layout.scale + 2 * scale + line as i32 * sound_h;
                canvas.small_text(layout.name_x, line_y, &text, scale, *color);
            }
        }
        let color = if mute {
            theme::muted()
        } else if track.auto_mode {
            AUTO
        } else if focused {
            theme::track(i)
        } else {
            theme::text()
        };
        canvas.text(layout.name_x, label_y, &label, layout.scale, color);
        draw_track_steps(canvas, track, i, view, layout);
    }

    // A line in the gap before each beat, across the rows: the beats stand
    // out whatever the theme's cell colours.
    let rows_h = (layout.tracks as i32 * layout.row).min(layout.bottom - layout.top) - gap;
    for beat in 1..layout.beats {
        canvas.fill_rect(layout.beat_x(beat) - gap, layout.top, gap, rows_h, theme::muted());
    }

    let add = layout.add_track_rect();
    canvas.fill_rect(add.x, add.y, add.w, add.h, box_off_color());
    let label_x = add.x + (add.w - text_width(ADD_TRACK_LABEL, scale)) / 2;
    canvas.text(label_x, add.y + (add.h - 8 * scale) / 2, ADD_TRACK_LABEL, scale, theme::text());

    if let Some(zone) = view.zone {
        draw_zone(canvas, &zone, layout);
    }

    if let Some((from, to)) = view.track_drop.filter(|(from, to)| from != to) {
        // Where the track lands: above the row it takes moving up, below it moving down.
        let y = layout.row_top(if to > from { to + 1 } else { to });
        let right = layout.grid_left + layout.grid_width;
        canvas.fill_rect(layout.left - 4 * scale, y - scale.px(), right - layout.left + 4 * scale, 2 * scale, ACCENT);
    }

    draw_lane(canvas, pattern, view, layout);

    if let Some(fraction) = view.playhead {
        let x = layout.grid_left + (fraction.clamp(0.0, 1.0) * f64::from(layout.grid_width)) as i32;
        let height = (layout.tracks as i32 * layout.row).min(layout.bottom - layout.top) - gap;
        canvas.fill_rect(x - scale.px(), layout.top, 2 * scale, height, PLAYHEAD);
        let lane = layout.lane_rect();
        canvas.fill_rect(x - scale.px(), lane.y, 2 * scale, lane.h, PLAYHEAD);
    }
}

/// The lane under the rows: a cell per step of the lane's track on the
/// page, and in it a bar as high as the lane's setting of the step's notes,
/// from the bottom, or from the middle for a setting either side of 0 (pan).
/// Without a track, the lane is drawn empty.
pub(super) fn draw_lane(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let gap = scale.px();
    let r = layout.lane_rect();
    canvas.fill_rect(r.x - gap, r.y - gap, r.w + gap, r.h + 2 * gap, theme::panel());
    let (lane_track, lane) = view.lane;
    let track = lane_track.and_then(|t| Some((t, pattern.tracks.get(t)?)));
    let spb = track.map_or(PAGE_STEPS_PER_BEAT, |(_, track)| steps_per_beat(track));
    let steps = i64::from(layout.beats) * i64::from(spb);
    let cell = |step: i64| {
        let x = layout.step_x(step, spb);
        (x, (layout.step_x(step + 1, spb) - x - gap).max(1))
    };
    for step in 0..steps {
        let (x, w) = cell(step);
        let color = if step % i64::from(spb) == 0 { cell_beat_color() } else { cell_color() };
        canvas.fill_rect(x, r.y, w, r.h, color);
    }
    let Some((t, track)) = track else { return };
    let Some(prop) = editors::LANES.get(lane).and_then(|(key, _)| editors::note_prop(key)) else { return };
    let span = prop.max - prop.min;
    if span <= 0.0 {
        return;
    }
    let y_of = |v: f64| r.y + r.h - ((v.clamp(prop.min, prop.max) - prop.min) / span * f64::from(r.h)).round() as i32;
    let base_y = y_of(if prop.min < 0.0 && prop.max > 0.0 { 0.0 } else { prop.min });
    if base_y < r.y + r.h {
        canvas.fill_rect(r.x, base_y, r.w, gap, theme::muted());
    }
    let start = i64::from(layout.start_beat) * i64::from(spb);
    for (n, note) in track.notes.iter().enumerate() {
        let local = absolute_step(note.beat, note.step, spb) - start;
        if !(0..steps).contains(&local) {
            continue;
        }
        let (x, w) = cell(local);
        let y = y_of(editors::note_value(note, prop));
        let (top, h) = if y < base_y { (y, base_y - y) } else { (base_y, y - base_y) };
        let bar = Rect::new(x + gap, top.min(r.y + r.h - 2 * gap), (w - 2 * gap).max(1), h.max(2 * gap));
        // A selected note's bar is ringed in white, like the note in the grid.
        if view.selected == Some((t, n)) {
            let top = (bar.y - gap).max(r.y);
            let bottom = (bar.y + bar.h + gap).min(r.y + r.h);
            canvas.fill_rect(bar.x - gap, top, bar.w + 2 * gap, bottom - top, SELECTED);
        }
        canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, theme::track(t));
    }
}

/// The smallest share of a cell a note fills, so the softest stays visible.
pub(super) const MIN_NOTE_FILL: f64 = 0.25;

/// A note in its cell: filled from the bottom as high as it is loud, its
/// colour fading with it too.
pub(super) fn draw_note_cell(canvas: &mut Canvas, cell: Rect, note: &Note, gap: i32) {
    let velocity = note.velocity.clamp(0.0, 1.0);
    let fill = ((f64::from(cell.h) * velocity.max(MIN_NOTE_FILL)).round() as i32).clamp(1, cell.h);
    let color = theme::note(note_color(note), velocity);
    let radius = 2 * gap;
    canvas.fill_rounded_gradient(cell.x, cell.y + cell.h - fill, cell.w, fill, note_gradient(color, false), radius);
}

/// The light-to-dark gradient of a note of `color`, lighter while it plays;
/// the piano roll's, so both views shade a note alike.
pub(super) fn note_gradient(color: Rgb, lit: bool) -> (Rgb, Rgb) {
    let light = if lit { piano_roll::NOTE_LIGHT_PLAYING } else { piano_roll::NOTE_LIGHT };
    (theme::mix(color, theme::WHITE, light), theme::mix(color, 0x00_00_00, piano_roll::NOTE_DARK))
}

/// Base colour of a note in the grid, from its own properties rather than
/// from its track: one that makes hits of its own (retrigger, euclidean)
/// is [`theme::GENERATED`], one that may not play (probability, every Nth
/// loop) [`theme::CHANCE`], a plain one [`theme::LIVE`].
pub(super) fn note_color(note: &Note) -> Rgb {
    let generated = note.euclidean_pulses > 0 || note.retrigger_count > 1 || (note.retrigger_rate - 1.0).abs() > 1e-9;
    if generated {
        theme::GENERATED
    } else if note.probability < 1.0 || note.cycle > 1 {
        theme::CHANCE
    } else {
        theme::LIVE
    }
}

/// One track's steps, its notes lit by velocity, the cursor and the selection.
pub(super) fn draw_track_steps(canvas: &mut Canvas, track: &Track, index: usize, view: &View, layout: &Layout) {
    let gap = layout.scale.px();
    let spb = steps_per_beat(track);
    let steps = i64::from(layout.beats) * i64::from(spb);
    let start = i64::from(layout.start_beat) * i64::from(spb);
    let y = layout.row_top(index);
    let height = layout.row - gap;
    let cell = |step: i64| {
        let x = layout.step_x(step, spb);
        (x, (layout.step_x(step + 1, spb) - x - gap).max(1))
    };

    let mut note_at: Vec<Option<&Note>> = vec![None; steps as usize];
    for note in &track.notes {
        let gstep = absolute_step(note.beat, note.step, spb);
        let local = gstep - start;
        if let Some(slot) = usize::try_from(local).ok().and_then(|s| note_at.get_mut(s))
            && slot.is_none_or(|existing| note.velocity > existing.velocity)
        {
            *slot = Some(note);
        }
    }
    // Ghosts: the sub-events (retriggers, arpeggio steps, euclidean fills)
    // a note produces, drawn smaller than the base note.
    let track_total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let span = ordrumbox_core::pattern_engine::SpanResolver::new(track);
    let mut rng = ordrumbox_core::Rng::new(0);
    let mut ghosts: Vec<(i64, ordrumbox_core::note_positions::SubKind, &Note)> = Vec::new();
    for note in &track.notes {
        for sub in ordrumbox_core::note_positions::sub_positions(note, track, track_total, &span, &mut rng) {
            let local = sub.step - start;
            if (0..steps).contains(&local) {
                ghosts.push((local, sub.kind, note));
            }
        }
    }
    // Loop ghosts: notes that re-trigger when the track loops at loop_length.
    let mut loop_ghosts: Vec<(i64, &Note)> = Vec::new();
    if let Some(loop_length) = track.loop_length.map(i64::from).filter(|&l| l > 0) {
        for note in &track.notes {
            let base = absolute_step(note.beat, note.step, spb);
            if base >= loop_length {
                continue;
            }
            let mut pos = base + loop_length;
            while pos < track_total {
                let local = pos - start;
                if (0..steps).contains(&local) {
                    loop_ghosts.push((local, note));
                }
                pos += loop_length;
            }
        }
    }
    for (step, note) in note_at.iter().enumerate() {
        let step = step as i64;
        let (x, width) = cell(step);
        let is_beat = step % i64::from(spb) == 0;
        let bg = if is_beat { cell_beat_color() } else { cell_color() };
        canvas.fill_rect(x, y, width, height, bg);
        if let Some(n) = note {
            draw_note_cell(canvas, Rect::new(x, y, width, height), n, gap);
        }
        // Ghosts on this step, drawn smaller (half size) and in a distinct colour.
        for (gpos, gkind, gnote) in &ghosts {
            if *gpos != step {
                continue;
            }
            let gcolor = match gkind {
                ordrumbox_core::note_positions::SubKind::Euclidean => theme::GENERATED,
                ordrumbox_core::note_positions::SubKind::Retrigger => theme::muted(),
            };
            let color = theme::note(gcolor, gnote.velocity);
            let iw = ((f64::from(width) * 0.5).round() as i32).max(1);
            let ih = ((f64::from(height) * 0.5).round() as i32).max(1);
            let (gx, gy) = (x + (width - iw) / 2, y + (height - ih) / 2);
            canvas.fill_rounded_gradient(gx, gy, iw, ih, note_gradient(color, false), gap);
        }
        for (gpos, gnote) in &loop_ghosts {
            if *gpos != step {
                continue;
            }
            let color = theme::note(theme::mix(theme::track(index), theme::bg(), 0.3), gnote.velocity);
            let iw = ((f64::from(width) * 0.5).round() as i32).max(1);
            let ih = ((f64::from(height) * 0.5).round() as i32).max(1);
            let (gx, gy) = (x + (width - iw) / 2, y + (height - ih) / 2);
            canvas.fill_rounded_gradient(gx, gy, iw, ih, note_gradient(color, false), gap);
        }
    }
    // Flash (with glow) the cells currently being played.
    if let Some(frac) = view.playhead {
        let cur = (frac * steps as f64).floor() as i64;
        if (0..steps).contains(&cur) {
            let has_base = note_at.get(cur as usize).copied().flatten().is_some();
            let has_ghost = ghosts.iter().any(|(p, _, _)| *p == cur) || loop_ghosts.iter().any(|(p, _)| *p == cur);
            if has_base || has_ghost {
                let (x, width) = cell(cur);
                let base_color = note_at
                    .get(cur as usize)
                    .copied()
                    .flatten()
                    .map(|n| theme::note(note_color(n), n.velocity))
                    .unwrap_or(cell_color());
                let flash = theme::mix(base_color, theme::WHITE, 0.6);
                let glow = theme::mix(flash, theme::bg(), 0.5);
                canvas.fill_rect(x - 2 * gap, y - 2 * gap, width + 4 * gap, height + 4 * gap, glow);
                canvas.fill_rounded_gradient(x, y, width, height, note_gradient(flash, true), 2 * gap);
            }
        }
    }
    if let Some(loop_length) = track
        .loop_length
        .map(i64::from)
        .filter(|&l| l > 0 && l < i64::from(track.steps_per_beat) * i64::from(track.beat_count))
    {
        let local = loop_length - start;
        if (0..steps).contains(&local) {
            let (x, _) = cell(local);
            canvas.fill_rect(x - gap, y, gap * 2, height, LOOP_MARK);
        }
    }

    let outline = |canvas: &mut Canvas, step: i64, color: Rgb| {
        if (0..steps).contains(&step) {
            let (x, width) = cell(step);
            stroke(canvas, x, y, width, height, (2 * layout.scale).max(1), color);
        }
    };
    if let Some(c) = view.cursor.filter(|c| c.track == index) {
        let s = absolute_step(c.beat, c.step, spb) - start;
        outline(canvas, s, CURSOR);
    }
    if let Some(note) = view.selected.filter(|(t, _)| *t == index).and_then(|(_, n)| track.notes.get(n)) {
        let s = absolute_step(note.beat, note.step, spb) - start;
        outline(canvas, s, SELECTED);
    }
}

/// The zone over its rows and its time on the page: shaded, outlined.
pub(super) fn draw_zone(canvas: &mut Canvas, zone: &Zone, layout: &Layout) {
    let last = zone.last_track.min(layout.tracks.saturating_sub(1));
    if zone.first_track > last {
        return;
    }
    let (left, right) = (layout.units_x(zone.start_units), layout.units_x(zone.end_units));
    if right <= left {
        return;
    }
    let top = layout.row_top(zone.first_track);
    let height = layout.row_top(last + 1) - top - layout.scale.px();
    canvas.shade_rect(left, top, right - left, height, ZONE, ZONE_OPACITY);
    stroke(canvas, left, top, right - left, height, layout.scale.px(), ZONE);
}

/// Rectangle outline, `thickness` wide, inside `(x, y, width, height)`.
pub(super) fn stroke(canvas: &mut Canvas, x: i32, y: i32, width: i32, height: i32, thickness: i32, color: Rgb) {
    canvas.fill_rect(x, y, width, thickness, color);
    canvas.fill_rect(x, y + height - thickness, width, thickness, color);
    canvas.fill_rect(x, y, thickness, height, color);
    canvas.fill_rect(x + width - thickness, y, thickness, height, color);
}
