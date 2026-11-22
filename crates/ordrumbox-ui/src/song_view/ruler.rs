//! The arrangement's ruler: a cell per measure, every fourth marked,
//! numbered as often as the numbers fit, and, as the zoom widens the
//! measures, ticks on their beats and steps, then the beats' numbers, so
//! that the ruler is as precise as the lanes under it.

use super::draw::Page;
use super::*;
use ordrumbox_core::automation::{STEPS_PER_BEAT, STEPS_PER_MEASURE};

/// What the ruler shows inside a measure, from the least to the most: each
/// level shows what the ones before it show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum RulerDetail {
    Measures,
    BeatTicks,
    StepTicks,
    BeatNumbers,
}

/// The narrowest beat and step (scale units) that show their tick, and the
/// room a number takes past its tick.
const BEAT_TICK: i32 = 8;
const STEP_TICK: i32 = 4;
const NUMBER_PAD: i32 = 3;
/// Heights of the ticks, at the bottom of the ruler.
const BEAT_TICK_HEIGHT: i32 = 5;
const STEP_TICK_HEIGHT: i32 = 3;
const BEATS_PER_MEASURE: u32 = STEPS_PER_MEASURE / STEPS_PER_BEAT;

impl SongLayout {
    /// What the ruler shows inside a measure at this zoom: the most whose
    /// ticks are not too close and whose numbers fit between them.
    pub(super) fn ruler_detail(&self) -> RulerDetail {
        let s = self.scale;
        let beat = self.measure_width() / BEATS_PER_MEASURE as i32;
        let step = self.measure_width() / STEPS_PER_MEASURE as i32;
        // The widest number: a beat's of a late measure.
        let beat_number = text_width("999.4", s) + 2 * NUMBER_PAD * s;
        if beat >= beat_number {
            RulerDetail::BeatNumbers
        } else if step >= STEP_TICK * s {
            RulerDetail::StepTicks
        } else if beat >= BEAT_TICK * s {
            RulerDetail::BeatTicks
        } else {
            RulerDetail::Measures
        }
    }

    /// How many steps a selection is made of at this zoom (a painted zone of
    /// a lane, the song's loop): a measure, a beat or a step, as fine as the
    /// ruler is marked.
    #[must_use]
    pub fn snap_steps(&self) -> u32 {
        match self.ruler_detail() {
            RulerDetail::Measures => STEPS_PER_MEASURE,
            RulerDetail::BeatTicks => STEPS_PER_BEAT,
            RulerDetail::StepTicks | RulerDetail::BeatNumbers => 1,
        }
    }
}

/// The ruler of `page`: its cells, their ticks and their numbers.
pub(super) fn draw(canvas: &mut Canvas, view: &SongView, layout: &SongLayout, page: &Page) {
    let s = layout.scale;
    for m in page.first_measure..page.first_measure + page.shown_measures {
        let x = layout.measure_x(f64::from(m), page.first_measure);
        let color = if m % 4 == 0 { major_color() } else { cell_color() };
        canvas.fill_rect(x, layout.ruler_top(), layout.measure_width() - s.px(), HEADER_HEIGHT * s - s.px(), color);
    }
    super::song_loop::draw(canvas, view, layout, page);
    let detail = layout.ruler_detail();
    draw_ticks(canvas, layout, page, detail);
    draw_numbers(canvas, layout, page, detail);
}

/// A tick on each beat, and on each step, as far as `detail` goes.
fn draw_ticks(canvas: &mut Canvas, layout: &SongLayout, page: &Page, detail: RulerDetail) {
    if detail < RulerDetail::BeatTicks {
        return;
    }
    let s = layout.scale;
    let bottom = layout.ruler_top() + HEADER_HEIGHT * s - s.px();
    let first = page.first_measure * STEPS_PER_MEASURE;
    let end = (page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    for step in (first..end).filter(|step| step % STEPS_PER_MEASURE != 0) {
        let height = match (step % STEPS_PER_BEAT == 0, detail >= RulerDetail::StepTicks) {
            (true, _) => BEAT_TICK_HEIGHT,
            (false, true) => STEP_TICK_HEIGHT,
            (false, false) => continue,
        };
        let x = layout.step_x(step, page.first_measure);
        canvas.fill_rect(x - s.px(), bottom - height * s, s.px(), height * s, theme::muted());
    }
}

/// The measures' numbers, as often as they fit (centred in their cell while
/// they are alone there), then the beats' (`measure.beat`), each after its
/// tick, none over another.
fn draw_numbers(canvas: &mut Canvas, layout: &SongLayout, page: &Page, detail: RulerDetail) {
    let s = layout.scale;
    let y = layout.ruler_top() + (HEADER_HEIGHT * s - 8 * s) / 2;
    let end = layout.body_left() + page.body_w;
    let label_step = layout.label_step(page.grid_total);
    let mut free_from = i32::MIN;
    let first = page.first_measure * STEPS_PER_MEASURE;
    let last = (page.first_measure + page.shown_measures) * STEPS_PER_MEASURE;
    for step in (first..last).step_by(STEPS_PER_BEAT as usize) {
        let (m, in_measure) = (step / STEPS_PER_MEASURE, step % STEPS_PER_MEASURE);
        let beat = in_measure / STEPS_PER_BEAT;
        let x = layout.step_x(step, page.first_measure);
        let (label, color) = match beat {
            0 if m % label_step == 0 => {
                let color = if m % 4 == 0 { theme::text() } else { theme::muted() };
                ((m + 1).to_string(), color)
            }
            1.. if detail >= RulerDetail::BeatNumbers => (format!("{}.{}", m + 1, beat + 1), theme::muted()),
            _ => continue,
        };
        let width = text_width(&label, s);
        let tx = if in_measure == 0 && detail < RulerDetail::BeatNumbers {
            x + ((layout.measure_width() - width) / 2).max(s.px())
        } else {
            x + NUMBER_PAD * s
        };
        if tx + width > end {
            break;
        }
        if tx < free_from {
            continue;
        }
        canvas.text(tx, y, &label, s, color);
        free_from = tx + width + NUMBER_PAD * s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ruler_shows_more_as_the_zoom_widens() {
        let detail = |units| SongLayout::new(1100, 680, Scale::ONE, 0, MeasureZoom::new(units)).ruler_detail();
        let details: Vec<_> = [MeasureZoom::MIN, 24, 64, MeasureZoom::STEPS, 256, MeasureZoom::MAX].map(detail).into();
        assert!(details.windows(2).all(|pair| pair[0] <= pair[1]), "{details:?}");
        assert_eq!(details[0], RulerDetail::Measures);
        assert_eq!(details[5], RulerDetail::BeatNumbers);
    }
}
