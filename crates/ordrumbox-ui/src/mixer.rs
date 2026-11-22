//! The mixer, a main view as the grid is: a strip for each track (its
//! name, its echo and reverb sends, its pan, mute and solo, its fader and
//! level meter), then the shared echo and reverb returns, then the master
//! bus (its compressor and filters, the compressor's curve, the spectrum,
//! its fader and meter). The track strips that do not fit scroll sideways;
//! the returns and the master keep their place at the right.
//!
//! [`MixerLayout`] places everything; the controls are panel items, the
//! strips' backgrounds, the meters, the curve and the spectrum are drawn here.

use crate::canvas::{Canvas, Rgb, fit, text_width};
use crate::output;
use crate::panel::Rect;
use crate::scale::Scale;
use crate::theme;
use ordrumbox_engine::MasterSettings;

/// Width of a strip, and the gap between two, at scale 1.
const STRIP_WIDTH: i32 = 60;
const STRIP_GAP: i32 = 4;
/// The gap between the tracks, the returns and the master, a divider in its middle.
const SECTION_GAP: i32 = 14;
/// A strip's name row, at its top.
const NAME_HEIGHT: i32 = 18;
/// A knob of a strip, and the rows of them under the name.
const KNOB_HEIGHT: i32 = 50;
const KNOB_ROWS: i32 = 4;
/// The mute and solo buttons.
const BUTTON_SIZE: (i32, i32) = (24, 16);
/// The scroll row under the strips: the strips shown, the arrows.
const SCROLL_HEIGHT: i32 = 18;
const ARROW_WIDTH: i32 = 20;
/// The master's columns: its controls three abreast, then its fader.
const MASTER_COLUMNS: i32 = 3;
/// A meter beside a fader.
const METER_WIDTH: i32 = 6;
/// The compressor's on/off button.
const BYPASS_SIZE: (i32, i32) = (36, 14);
/// The compressor's curve, beside its button; the spectrum takes the rest under them.
const CURVE_SIZE: (i32, i32) = (96, 72);
/// The quietest level a meter shows, dB.
const METER_FLOOR_DB: f64 = -48.0;

/// The knob rows of a track strip: its sends, then its pan.
pub const TRACK_KNOBS: [(&str, &str); 3] = [("echoSend", "Echo"), ("reverbSend", "Reverb"), ("pan", "Pan")];

/// Where a track strip's parts are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrackStrip {
    /// The track it shows.
    pub track: usize,
    pub back: Rect,
    pub name: Rect,
    /// The knobs, in the order of [`TRACK_KNOBS`].
    pub knobs: [Rect; 3],
    pub mute: Rect,
    pub solo: Rect,
    pub fader: Rect,
    pub meter: Rect,
}

/// Where a return's strip parts are: its knobs, then its level fader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReturnStrip {
    pub back: Rect,
    pub name: Rect,
    pub knobs: Vec<Rect>,
    pub fader: Rect,
}

/// Where the master's parts are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MasterStrip {
    pub back: Rect,
    pub name: Rect,
    /// Pre-gain, the compressor's six knobs, the two filters.
    pub knobs: [Rect; 9],
    pub bypass: Rect,
    pub curve: Rect,
    pub spectrum: Rect,
    pub fader: Rect,
    pub meter: Rect,
}

/// Where everything of the mixer is, in the main editor's region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixerLayout {
    scale: Scale,
    /// The track strips shown, from the first one scrolled to.
    pub tracks: Vec<TrackStrip>,
    /// How many track strips fit across.
    pub fits: usize,
    /// The scroll row: its text, its arrows (the strips before, after).
    pub scroll_text: Rect,
    pub scroll: [Rect; 2],
    /// The echo's strip, then the reverb's.
    pub returns: [ReturnStrip; 2],
    pub master: MasterStrip,
    /// The x of the dividers between the tracks, the returns and the master.
    dividers: [i32; 2],
    /// The region's top and bottom.
    top: i32,
    bottom: i32,
}

impl MixerLayout {
    /// The mixer in `area`, for `track_count` tracks, the strips from `first` on.
    pub fn new(area: Rect, track_count: usize, first: usize, scale: Scale) -> Self {
        let s = scale;
        let (strip_w, gap, section_gap) = (STRIP_WIDTH * s, STRIP_GAP * s, SECTION_GAP * s);
        let strips_bottom = area.y + area.h - SCROLL_HEIGHT * s;
        // From the right: the master, the reverb, the echo, then the tracks in what is left.
        let master_w = (MASTER_COLUMNS + 1) * strip_w;
        let master_x = area.x + area.w - master_w;
        let reverb_x = master_x - section_gap - strip_w;
        let echo_x = reverb_x - gap - strip_w;
        let tracks_right = echo_x - section_gap;
        let fits = usize::try_from((tracks_right - area.x + gap) / (strip_w + gap)).unwrap_or(0).max(1);
        let first = first.min(track_count.saturating_sub(fits));
        let tracks = (first..track_count)
            .take(fits)
            .enumerate()
            .map(|(i, track)| {
                let x = area.x + i as i32 * (strip_w + gap);
                track_strip(track, Rect::new(x, area.y, strip_w, strips_bottom - area.y), s)
            })
            .collect();
        let arrow_w = ARROW_WIDTH * s;
        let scroll_y = strips_bottom + 2 * s;
        let scroll_h = SCROLL_HEIGHT * s - 4 * s;
        let scroll = [
            Rect::new(area.x, scroll_y, arrow_w, scroll_h),
            Rect::new((tracks_right - arrow_w).max(area.x + arrow_w), scroll_y, arrow_w, scroll_h),
        ];
        let scroll_text = Rect::new(
            scroll[0].x + arrow_w + 6 * s,
            scroll_y,
            (scroll[1].x - scroll[0].x - arrow_w - 12 * s).max(0),
            scroll_h,
        );
        let returns = [
            return_strip(Rect::new(echo_x, area.y, strip_w, strips_bottom - area.y), output::ECHO_CONTROLS - 1, s),
            return_strip(
                Rect::new(reverb_x, area.y, strip_w, strips_bottom - area.y),
                output::RETURNS.len() - output::ECHO_CONTROLS - 1,
                s,
            ),
        ];
        let master = master_strip(Rect::new(master_x, area.y, master_w, strips_bottom - area.y), s);
        let dividers = [tracks_right + section_gap / 2, reverb_x + strip_w + section_gap / 2];
        Self { scale, tracks, fits, scroll_text, scroll, returns, master, dividers, top: area.y, bottom: strips_bottom }
    }
}

/// The top of knob row `row` of a strip whose top is `top`.
fn knob_row(top: i32, row: i32, s: Scale) -> i32 {
    top + NAME_HEIGHT * s + row * KNOB_HEIGHT * s
}

/// The fader of a strip whose top is `top`, under its knob rows, `w` wide at `x`, down to `bottom`.
fn fader_rect(x: i32, w: i32, top: i32, bottom: i32, s: Scale) -> Rect {
    let fader_top = knob_row(top, KNOB_ROWS, s) + 4 * s;
    Rect::new(x, fader_top, w, (bottom - 4 * s - fader_top).max(24 * s))
}

/// The meter beside a fader.
fn meter_rect(fader: Rect, s: Scale) -> Rect {
    let (top, h) = crate::panel::fader_bar(fader, s);
    Rect::new(fader.x + fader.w, top, METER_WIDTH * s, h)
}

fn track_strip(track: usize, back: Rect, s: Scale) -> TrackStrip {
    let knob = |row: i32| Rect::new(back.x + 2 * s, knob_row(back.y, row, s), back.w - 4 * s, KNOB_HEIGHT * s - 2 * s);
    let (bw, bh) = (BUTTON_SIZE.0 * s, BUTTON_SIZE.1 * s);
    let buttons_y = knob_row(back.y, 3, s) + (KNOB_HEIGHT * s - bh) / 2;
    let fader = fader_rect(back.x + 2 * s, back.w - 4 * s - (METER_WIDTH + 6) * s, back.y, back.y + back.h, s);
    TrackStrip {
        track,
        back,
        name: Rect::new(back.x + 4 * s, back.y + 5 * s, back.w - 8 * s, 10 * s),
        knobs: [knob(0), knob(1), knob(2)],
        mute: Rect::new(back.x + back.w / 2 - bw - s.px(), buttons_y, bw, bh),
        solo: Rect::new(back.x + back.w / 2 + s.px(), buttons_y, bw, bh),
        fader,
        meter: meter_rect(fader, s),
    }
}

fn return_strip(back: Rect, knob_count: usize, s: Scale) -> ReturnStrip {
    let knobs = (0..knob_count as i32)
        .map(|row| Rect::new(back.x + 2 * s, knob_row(back.y, row, s), back.w - 4 * s, KNOB_HEIGHT * s - 2 * s))
        .collect();
    ReturnStrip {
        back,
        name: Rect::new(back.x + 4 * s, back.y + 5 * s, back.w - 8 * s, 10 * s),
        knobs,
        fader: fader_rect(back.x + 2 * s, back.w - 4 * s, back.y, back.y + back.h, s),
    }
}

fn master_strip(back: Rect, s: Scale) -> MasterStrip {
    let column = STRIP_WIDTH * s;
    let knobs = std::array::from_fn(|i| {
        let (col, row) = ((i as i32) % MASTER_COLUMNS, (i as i32) / MASTER_COLUMNS);
        Rect::new(back.x + col * column + 2 * s, knob_row(back.y, row, s), column - 4 * s, KNOB_HEIGHT * s - 2 * s)
    });
    let row3 = knob_row(back.y, 3, s);
    let (bw, bh) = (BYPASS_SIZE.0 * s, BYPASS_SIZE.1 * s);
    let bypass = Rect::new(back.x + 6 * s, row3 + 14 * s, bw, bh);
    // Under the knobs: the curve, small, beside the button, then the spectrum across the three columns.
    let plots_left = back.x + 4 * s;
    let plots_right = back.x + MASTER_COLUMNS * column - 4 * s;
    let curve_x = bypass.x + bw + 8 * s;
    let curve =
        Rect::new(curve_x, row3 + 2 * s, (CURVE_SIZE.0 * s).min(plots_right - curve_x).max(0), CURVE_SIZE.1 * s);
    let spectrum_top = curve.y + curve.h + 6 * s;
    let spectrum =
        Rect::new(plots_left, spectrum_top, plots_right - plots_left, (back.y + back.h - 4 * s - spectrum_top).max(0));
    let fader_x = back.x + MASTER_COLUMNS * column + 2 * s;
    let fader = fader_rect(fader_x, column - 4 * s - (METER_WIDTH + 6) * s, back.y, back.y + back.h, s);
    MasterStrip {
        back,
        name: Rect::new(back.x + 4 * s, back.y + 5 * s, back.w - 8 * s, 10 * s),
        knobs,
        bypass,
        curve,
        spectrum,
        fader,
        meter: meter_rect(fader, s),
    }
}

/// What the mixer draws besides its controls.
#[derive(Debug)]
pub struct MixerView {
    /// The level of each track, linear peak (by track index).
    pub levels: Vec<f32>,
    /// The track the track editor edits: its strip is lit.
    pub edited: Option<usize>,
    /// The master's settings, its level (linear peak) and the spectrum's
    /// bytes, one a bin; `None` in a plugin, whose host mixes.
    pub master: Option<(MasterSettings, f32, Vec<u8>)>,
}

fn strip_color() -> Rgb {
    theme::panel()
}
fn edited_color() -> Rgb {
    theme::panel_2()
}
const EDITED_EDGE: Rgb = theme::ACCENT;
const METER: Rgb = theme::LIVE;
const METER_HOT: Rgb = theme::ALERT;
/// A meter turns hot over this level, dB.
const METER_HOT_DB: f64 = -3.0;

/// The strips' backgrounds, the dividers, the meters, the curve and the spectrum.
pub fn draw(canvas: &mut Canvas, view: &MixerView, layout: &MixerLayout) {
    let s = layout.scale;
    for strip in &layout.tracks {
        let edited = view.edited == Some(strip.track);
        let back = strip.back;
        canvas.fill_rect(back.x, back.y, back.w, back.h, if edited { edited_color() } else { strip_color() });
        if edited {
            canvas.fill_rect(back.x, back.y, back.w, 2 * s.px(), EDITED_EDGE);
        }
        draw_meter(canvas, strip.meter, view.levels.get(strip.track).copied().unwrap_or(0.0));
    }
    for strip in &layout.returns {
        canvas.fill_rect(strip.back.x, strip.back.y, strip.back.w, strip.back.h, strip_color());
    }
    let master = layout.master.back;
    canvas.fill_rect(master.x, master.y, master.w, master.h, strip_color());
    for x in layout.dividers {
        canvas.fill_rect(x, layout.top, s.px(), layout.bottom - layout.top, theme::line());
    }
    match &view.master {
        Some((settings, level, spectrum)) => {
            draw_meter(canvas, layout.master.meter, *level);
            if layout.master.curve.h > 8 * s {
                output::draw_curve(canvas, settings, layout.master.curve, s);
            }
            if layout.master.spectrum.h > 8 * s {
                output::draw_spectrum(canvas, spectrum, layout.master.spectrum, s);
            }
        }
        None => {
            // A plugin: the host mixes; its returns and master are the defaults.
            let lines =
                ["The host mixes:", "the returns and", "the master play", "at their defaults,", "set in the app."];
            let r = Rect::new(layout.returns[0].back.x, layout.top, master.x + master.w - layout.returns[0].back.x, 0);
            for (i, line) in lines.iter().enumerate() {
                let y = r.y + (40 + 12 * i as i32) * s;
                let text = fit(line, r.w - 8 * s, s);
                canvas.text(r.x + (r.w - text_width(&text, s)) / 2, y, &text, s, theme::muted());
            }
        }
    }
}

/// A meter: lit from its foot up to `level` (linear peak) on a dB scale, hot near the top.
fn draw_meter(canvas: &mut Canvas, r: Rect, level: f32) {
    canvas.fill_rect(r.x, r.y, r.w, r.h, theme::screen());
    let fraction = meter_fraction(f64::from(level));
    let lit = (fraction * f64::from(r.h)) as i32;
    if lit <= 0 {
        return;
    }
    let hot = (meter_fraction(10f64.powf(METER_HOT_DB / 20.0)) * f64::from(r.h)) as i32;
    canvas.fill_rect(r.x, r.y + r.h - lit, r.w, lit, METER);
    if lit > hot {
        canvas.fill_rect(r.x, r.y + r.h - lit, r.w, lit - hot, METER_HOT);
    }
}

/// Where a linear peak `level` sits on a meter, 0 at [`METER_FLOOR_DB`] and under, 1 at 0 dB and over.
fn meter_fraction(level: f64) -> f64 {
    if level <= 0.0 {
        return 0.0;
    }
    (1.0 - 20.0 * level.log10() / METER_FLOOR_DB).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect::new(8, 40, 760, 455);

    #[test]
    fn the_strips_sit_side_by_side_and_inside_the_region() {
        let layout = MixerLayout::new(AREA, 20, 0, Scale::ONE);
        assert!(layout.fits >= 4, "{}", layout.fits);
        assert_eq!(layout.tracks.len(), layout.fits);
        let backs: Vec<Rect> = layout
            .tracks
            .iter()
            .map(|t| t.back)
            .chain(layout.returns.iter().map(|r| r.back))
            .chain([layout.master.back])
            .collect();
        assert!(backs.windows(2).all(|w| w[0].x + w[0].w <= w[1].x), "no strip over another");
        assert!(backs.iter().all(|b| b.x >= AREA.x && b.x + b.w <= AREA.x + AREA.w && b.y + b.h <= AREA.y + AREA.h));
        // Every fader starts at the same height, and so does every knob row.
        let faders: Vec<i32> = layout
            .tracks
            .iter()
            .map(|t| t.fader.y)
            .chain(layout.returns.iter().map(|r| r.fader.y))
            .chain([layout.master.fader.y])
            .collect();
        assert!(faders.iter().all(|&y| y == faders[0]));
        assert_eq!(layout.tracks[0].knobs[0].y, layout.returns[0].knobs[0].y);
        assert_eq!(layout.returns[0].knobs.len(), 4, "time, feedback, tone, to reverb");
        assert_eq!(layout.returns[1].knobs.len(), 3, "size, damping, spring");
        let m = layout.master;
        assert!(m.curve.w > 40 && m.curve.h > 40 && m.spectrum.h > 40, "{m:?}");
        assert!(m.spectrum.w > m.curve.w && m.spectrum.h > m.curve.h, "the spectrum is the big plot: {m:?}");
        assert!(m.curve.y + m.curve.h <= m.spectrum.y, "the curve over the spectrum");
        assert!(m.knobs.iter().all(|k| k.y + k.h <= m.curve.y), "the plots under the knobs");
    }

    #[test]
    fn the_returns_and_the_master_keep_their_place_whatever_is_scrolled() {
        let few = MixerLayout::new(AREA, 2, 0, Scale::ONE);
        let many = MixerLayout::new(AREA, 40, 7, Scale::ONE);
        assert_eq!(few.returns, many.returns);
        assert_eq!(few.master, many.master);
        assert_eq!(few.scroll, many.scroll);
        assert_eq!(many.tracks[0].track, 7);
        assert_eq!(many.tracks[0].back, few.tracks[0].back, "the first strip shown is where the first is");
        // Scrolled past the end: the last strips fill the row.
        let end = MixerLayout::new(AREA, 10, 9, Scale::ONE);
        assert_eq!(end.tracks.last().map(|t| t.track), Some(9));
        assert_eq!(end.tracks.len(), end.fits.min(10));
    }

    #[test]
    fn a_meter_shows_its_level_on_a_db_scale() {
        assert_eq!(meter_fraction(0.0), 0.0);
        assert_eq!(meter_fraction(1.0), 1.0);
        assert_eq!(meter_fraction(2.0), 1.0);
        assert!((meter_fraction(10f64.powf(-24.0 / 20.0)) - 0.5).abs() < 1e-9);
        assert_eq!(meter_fraction(1e-4), 0.0, "under the floor");
    }

    #[test]
    fn draws_at_any_size() {
        for (w, h) in [(1, 1), (60, 40), (900, 520)] {
            let mut pixels = vec![0; w * h];
            let mut canvas = Canvas::new(&mut pixels, w, h);
            let layout = MixerLayout::new(Rect::new(0, 0, w as i32, h as i32), 12, 3, Scale::ONE);
            for master in [None, Some((MasterSettings::default(), 0.9, vec![128; 256]))] {
                let view = MixerView { levels: vec![0.5; 12], edited: Some(4), master };
                draw(&mut canvas, &view, &layout);
            }
        }
    }
}
