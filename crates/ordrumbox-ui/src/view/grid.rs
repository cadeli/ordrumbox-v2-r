//! Drawing the pattern grid: the ruler, the rows and their steps, the
//! lane under them and the zone.

use super::layout::{ADD_TRACK_LABEL, Layout, RULER_HEIGHT, steps_per_beat};
use super::{
    ACCENT, AUTO, CURSOR, LABEL_CHARS, LOOP_MARK, MUTE_ON, MuteSolo, PAGE_STEPS_PER_BEAT, PLAYHEAD, PLAYING, SELECTED,
    SOLO_ON, View, background_color, box_off_color, cell_beat_color, cell_color, focus_row_color, mute_live_color,
};
use crate::canvas::{Canvas, Rgb, small_cell, small_char_width, text_width};
use crate::panel::{MENU_MARK, Rect, stroke};
use crate::scale::Scale;
use crate::scroll_thumb;
use crate::theme::{ZONE, ZONE_OPACITY};
use crate::{editors, piano_roll, theme};
use ordrumbox_core::note_positions::SubKind;
use ordrumbox_core::steps::absolute_step;
use ordrumbox_core::{Note, Pattern, RetriggerSpacing, Track};
use ordrumbox_logic::block::Zone;

/// The beat ruler above the rows: each beat's number (the one playing lit),
/// a tick where it starts, and a dot per step of the focused track.
pub(super) fn draw_ruler(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let y = layout.top - RULER_HEIGHT * scale;
    let spb =
        view.cursor.and_then(|c| pattern.tracks.get(c.track)).or(pattern.tracks.first()).map_or(4, steps_per_beat);
    let ruler = layout.ruler(spb);
    ruler.draw_marks(canvas, &view.ruler_marks);
    let playing =
        view.playhead.map(|f| layout.start_beat + (f.clamp(0.0, 1.0) * f64::from(layout.beats)).floor() as i32);
    ruler.draw_beats(
        canvas,
        layout.start_beat..layout.start_beat + layout.beats,
        layout.total_beats,
        (playing, PLAYING),
    );
    // The beats shown, left of the ruler, when the pattern has more.
    if layout.beats < layout.total_beats {
        let text = format!("{}-{}/{}", layout.start_beat + 1, layout.start_beat + layout.beats, layout.total_beats);
        canvas.text(layout.name_x, y, &text, scale, theme::muted());
    }
}

pub(super) fn draw_grid(canvas: &mut Canvas, pattern: &Pattern, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let gap = scale.px();
    draw_ruler(canvas, pattern, view, layout);
    for (i, track) in pattern.tracks.iter().enumerate().take(layout.tracks) {
        draw_track_row(canvas, track, i, view, layout);
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

    draw_scroll_bar(canvas, layout);

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
        canvas.fill_rect(x - scale.px(), layout.top, 2 * scale, rows_h, PLAYHEAD);
        let lane = layout.lane_rect();
        canvas.fill_rect(x - scale.px(), lane.y, 2 * scale, lane.h, PLAYHEAD);
    }
}

/// Row `i` of the grid: its focus band, VU stripe, mute and solo boxes, name
/// and sound, then its steps.
fn draw_track_row(canvas: &mut Canvas, track: &Track, i: usize, view: &View, layout: &Layout) {
    let scale = layout.scale;
    let y = layout.row_top(i);
    let focused = view.cursor.is_some_and(|c| c.track == i);
    if focused {
        // The whole row, its steps included.
        let right = layout.grid_left + layout.grid_width;
        let band = Rect::new(layout.left - 4 * scale, y, right - layout.left + 4 * scale, layout.row - scale.px());
        canvas.fill_rect(band.x, band.y, band.w, band.h, focus_row_color());
    }
    draw_vu_stripe(canvas, i, y, view, layout);
    let mute = draw_mute_solo(canvas, track, i, y, view, layout);
    let color = if mute {
        theme::muted()
    } else if track.auto_mode {
        AUTO
    } else if focused {
        theme::track(i)
    } else {
        theme::text()
    };
    draw_track_name(canvas, track, i, y, color, view, layout);
    draw_track_steps(canvas, track, i, view, layout);
}

/// The track's VU meter stripe (level from the audio engine).
fn draw_vu_stripe(canvas: &mut Canvas, i: usize, y: i32, view: &View, layout: &Layout) {
    let gap = layout.scale.px();
    let stripe_x = layout.left - 8 * layout.scale;
    let stripe_w = 3 * layout.scale;
    let stripe_top = y + gap;
    let stripe_h = layout.row - 3 * gap;
    if !view.show_vus {
        canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::track(i));
        return;
    }
    let level = view.levels.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    canvas.fill_rect(stripe_x, stripe_top, stripe_w, stripe_h, theme::mix(theme::track(i), theme::bg(), 0.75));
    let fill_h = (f64::from(stripe_h) * f64::from(level)).round() as i32;
    if fill_h > 0 {
        canvas.fill_rect(stripe_x, stripe_top + stripe_h - fill_h, stripe_w, fill_h, theme::track(i));
    }
}

/// The mute and solo boxes of row `i`; whether the track is muted (by itself
/// or by the MIDI controls).
fn draw_mute_solo(canvas: &mut Canvas, track: &Track, i: usize, y: i32, view: &View, layout: &Layout) -> bool {
    let box_y = y + (layout.row - layout.box_size) / 2;
    // A mute from the MIDI controls is lit in its own colour: the track's own stays off.
    let live_mute = ordrumbox_engine::host_muted(view.live_muted, i);
    // The song's lanes, while it plays, else the track's own.
    let shown = view.switches.get(i).copied().unwrap_or(MuteSolo { mute: track.mute, solo: track.solo });
    let (own_mute, solo) = (shown.mute, shown.solo);
    let taken = view.switches_taken_back.get(i).copied().unwrap_or_default();
    let (mute_taken, solo_taken) = (taken.mute, taken.solo);
    let (mute, mute_color) = if own_mute { (true, MUTE_ON) } else { (live_mute, mute_live_color()) };
    let boxes = [(false, mute, mute_color, "M", mute_taken), (true, solo, SOLO_ON, "S", solo_taken)];
    for (solo, on, color, letter, is_taken_back) in boxes {
        let x = layout.box_x(solo);
        canvas.fill_rect(x, box_y, layout.box_size, layout.box_size, if on { color } else { box_off_color() });
        let letter_scale = Scale::whole((layout.box_size / 10).max(1));
        let inset = (layout.box_size - 8 * letter_scale) / 2;
        let letter_color = if on { background_color() } else { theme::muted() };
        canvas.text(x + inset, box_y + inset, letter, letter_scale, letter_color);
        if is_taken_back {
            // Taken back from its lane: a corner mark, as on the panels' controls.
            let mark = (layout.box_size / 3).max(1);
            canvas.fill_rect(x + layout.box_size - mark, box_y, mark, mark, theme::TAKEN_BACK);
        }
    }
    mute
}

/// The name of row `i` in `color`, and under it, in the small font, what the
/// track plays: its kit, then its sample, as many lines as the row has room
/// for, the sample's first (all of it in the name's tooltip). The room alone
/// sets the lines, so the name never moves with the sound.
fn draw_track_name(canvas: &mut Canvas, track: &Track, i: usize, y: i32, color: Rgb, view: &View, layout: &Layout) {
    let scale = layout.scale;
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
    let label: String = track.name.chars().take(LABEL_CHARS).collect();
    if view.hovered_track == Some(i) {
        // Under the pointer: a lighter name, the mark of its menu at the end of the name's room.
        let end = layout.volume_x - 4 * scale;
        canvas.fill_rect(
            layout.name_x - 2 * scale,
            label_y - 2 * scale,
            end - layout.name_x + 2 * scale,
            12 * scale,
            theme::highlight(),
        );
        canvas.text(end - text_width(MENU_MARK, layout.scale), label_y, MENU_MARK, layout.scale, theme::muted());
    }
    canvas.text(layout.name_x, label_y, &label, layout.scale, color);
}

/// The scroll bar of the beats shown, beside the add-track button: its
/// thumb, whose ends are the handles that zoom.
fn draw_scroll_bar(canvas: &mut Canvas, layout: &Layout) {
    let bar = layout.scroll_bar();
    let thumb = layout.bar_thumb();
    canvas.fill_rect(bar.x, bar.y, bar.w, bar.h, cell_color());
    let color = if layout.beats < layout.total_beats { theme::muted() } else { theme::line() };
    canvas.fill_rect(thumb.0, bar.y + layout.scale.px(), thumb.1, bar.h - 2 * layout.scale.px(), color);
    scroll_thumb::draw_handles(canvas, bar, thumb, layout.scale);
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
        let Some(value) = editors::note_lane_value(note, prop) else { continue };
        let (x, w) = cell(local);
        let y = y_of(value);
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
    let generated =
        note.euclidean_pulses > 0 || note.retrigger_count > 1 || note.retrigger_spacing != RetriggerSpacing::default();
    if generated {
        theme::GENERATED
    } else if note.probability < 1.0 || note.cycle > 1 {
        theme::CHANCE
    } else {
        theme::LIVE
    }
}

/// Where a track's steps are on the page, and the cell of each.
struct TrackSteps<'a> {
    layout: &'a Layout,
    steps_per_beat: i32,
    /// Steps on the page.
    steps: i64,
    /// The page's first step, absolute.
    start: i64,
    y: i32,
    height: i32,
}

impl<'a> TrackSteps<'a> {
    fn new(track: &Track, index: usize, layout: &'a Layout) -> Self {
        let steps_per_beat = steps_per_beat(track);
        Self {
            layout,
            steps_per_beat,
            steps: i64::from(layout.beats) * i64::from(steps_per_beat),
            start: i64::from(layout.start_beat) * i64::from(steps_per_beat),
            y: layout.row_top(index),
            height: layout.row - layout.scale.px(),
        }
    }

    fn gap(&self) -> i32 {
        self.layout.scale.px()
    }

    /// The step of the page that the absolute step `absolute` is.
    fn local(&self, absolute: i64) -> i64 {
        absolute - self.start
    }

    fn on_page(&self, local: i64) -> bool {
        (0..self.steps).contains(&local)
    }

    /// The cell of page step `step`.
    fn cell(&self, step: i64) -> Rect {
        let x = self.layout.step_x(step, self.steps_per_beat);
        let width = (self.layout.step_x(step + 1, self.steps_per_beat) - x - self.gap()).max(1);
        Rect::new(x, self.y, width, self.height)
    }
}

/// The notes of `track` by page step, the loudest where several share one.
fn strongest_notes<'a>(track: &'a Track, grid: &TrackSteps) -> Vec<Option<&'a Note>> {
    let mut note_at: Vec<Option<&Note>> = vec![None; grid.steps as usize];
    for note in &track.notes {
        let local = grid.local(absolute_step(note.beat, note.step, grid.steps_per_beat));
        if let Some(slot) = usize::try_from(local).ok().and_then(|s| note_at.get_mut(s))
            && slot.is_none_or(|existing| note.velocity > existing.velocity)
        {
            *slot = Some(note);
        }
    }
    note_at
}

/// The sub-events (retriggers, arpeggio steps, euclidean fills) that the
/// notes produce on the page, by page step.
fn sub_note_ghosts<'a>(track: &'a Track, grid: &TrackSteps) -> Vec<(i64, SubKind, &'a Note)> {
    let track_total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let span = ordrumbox_core::pattern_engine::SpanResolver::new(track);
    let mut rng = ordrumbox_core::Rng::new(0);
    let mut ghosts = Vec::new();
    for note in &track.notes {
        for sub in ordrumbox_core::note_positions::sub_positions(note, track, track_total, &span, &mut rng) {
            let local = grid.local(sub.step);
            if grid.on_page(local) {
                ghosts.push((local, sub.kind, note));
            }
        }
    }
    ghosts
}

/// The notes that re-trigger when the track loops at its `loop_steps`, by
/// page step.
fn loop_ghosts<'a>(track: &'a Track, grid: &TrackSteps) -> Vec<(i64, &'a Note)> {
    let track_total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let Some(loop_steps) = track.loop_steps.map(i64::from).filter(|&l| l > 0) else { return Vec::new() };
    let mut ghosts = Vec::new();
    for note in &track.notes {
        let base = absolute_step(note.beat, note.step, grid.steps_per_beat);
        if base >= loop_steps {
            continue;
        }
        let mut pos = base + loop_steps;
        while pos < track_total {
            let local = grid.local(pos);
            if grid.on_page(local) {
                ghosts.push((local, note));
            }
            pos += loop_steps;
        }
    }
    ghosts
}

/// A ghost in `cell`: smaller (half size) than a note, in its own colour.
fn draw_ghost(canvas: &mut Canvas, cell: Rect, color: Rgb, gap: i32) {
    let iw = ((f64::from(cell.w) * 0.5).round() as i32).max(1);
    let ih = ((f64::from(cell.h) * 0.5).round() as i32).max(1);
    let (gx, gy) = (cell.x + (cell.w - iw) / 2, cell.y + (cell.h - ih) / 2);
    canvas.fill_rounded_gradient(gx, gy, iw, ih, note_gradient(color, false), gap);
}

/// One track's steps, its notes lit by velocity, the cursor and the selection.
pub(super) fn draw_track_steps(canvas: &mut Canvas, track: &Track, index: usize, view: &View, layout: &Layout) {
    let grid = TrackSteps::new(track, index, layout);
    let gap = grid.gap();
    let note_at = strongest_notes(track, &grid);
    let ghosts = sub_note_ghosts(track, &grid);
    let loop_ghosts = loop_ghosts(track, &grid);
    for (step, note) in note_at.iter().enumerate() {
        let step = step as i64;
        let cell = grid.cell(step);
        let is_beat = step % i64::from(grid.steps_per_beat) == 0;
        canvas.fill_rect(cell.x, cell.y, cell.w, cell.h, if is_beat { cell_beat_color() } else { cell_color() });
        if let Some(n) = note {
            draw_note_cell(canvas, cell, n, gap);
        }
        for (_, kind, ghost) in ghosts.iter().filter(|(at, _, _)| *at == step) {
            let color = match kind {
                SubKind::Euclidean => theme::GENERATED,
                SubKind::Retrigger => theme::muted(),
            };
            draw_ghost(canvas, cell, theme::note(color, ghost.velocity), gap);
        }
        for (_, ghost) in loop_ghosts.iter().filter(|(at, _)| *at == step) {
            let color = theme::note(theme::mix(theme::track(index), theme::bg(), 0.3), ghost.velocity);
            draw_ghost(canvas, cell, color, gap);
        }
    }
    if let Some(fraction) = view.playhead {
        let has_ghost =
            |step: i64| ghosts.iter().any(|(at, _, _)| *at == step) || loop_ghosts.iter().any(|(at, _)| *at == step);
        flash_playing(canvas, &grid, fraction, &note_at, has_ghost);
    }
    draw_loop_mark(canvas, track, &grid);

    if let Some(c) = view.cursor.filter(|c| c.track == index) {
        draw_step_outline(canvas, &grid, grid.local(absolute_step(c.beat, c.step, grid.steps_per_beat)), CURSOR);
    }
    if let Some(note) = view.selected.filter(|(t, _)| *t == index).and_then(|(_, n)| track.notes.get(n)) {
        draw_step_outline(
            canvas,
            &grid,
            grid.local(absolute_step(note.beat, note.step, grid.steps_per_beat)),
            SELECTED,
        );
    }
}

/// Flash (with glow) the cell being played, when a note or a ghost is on it.
fn flash_playing(
    canvas: &mut Canvas,
    grid: &TrackSteps,
    fraction: f64,
    note_at: &[Option<&Note>],
    has_ghost: impl Fn(i64) -> bool,
) {
    let current = (fraction * grid.steps as f64).floor() as i64;
    if !grid.on_page(current) {
        return;
    }
    let base = note_at.get(current as usize).copied().flatten();
    if base.is_none() && !has_ghost(current) {
        return;
    }
    let (cell, gap) = (grid.cell(current), grid.gap());
    let base_color = base.map_or(cell_color(), |n| theme::note(note_color(n), n.velocity));
    let flash = theme::mix(base_color, theme::WHITE, 0.6);
    let glow = theme::mix(flash, theme::bg(), 0.5);
    canvas.fill_rect(cell.x - 2 * gap, cell.y - 2 * gap, cell.w + 4 * gap, cell.h + 4 * gap, glow);
    canvas.fill_rounded_gradient(cell.x, cell.y, cell.w, cell.h, note_gradient(flash, true), 2 * gap);
}

/// A line where the track's loop starts over, when it is on the page.
fn draw_loop_mark(canvas: &mut Canvas, track: &Track, grid: &TrackSteps) {
    let total = i64::from(track.steps_per_beat) * i64::from(track.beat_count);
    let Some(loop_steps) = track.loop_steps.map(i64::from).filter(|&l| l > 0 && l < total) else { return };
    let local = grid.local(loop_steps);
    if grid.on_page(local) {
        let gap = grid.gap();
        canvas.fill_rect(grid.cell(local).x - gap, grid.y, gap * 2, grid.height, LOOP_MARK);
    }
}

/// An outline round page step `local`, when it is on the page.
fn draw_step_outline(canvas: &mut Canvas, grid: &TrackSteps, local: i64, color: Rgb) {
    if grid.on_page(local) {
        stroke(canvas, grid.cell(local), (2 * grid.layout.scale).max(1), color);
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
    stroke(canvas, Rect::new(left, top, right - left, height), layout.scale.px(), ZONE);
}
