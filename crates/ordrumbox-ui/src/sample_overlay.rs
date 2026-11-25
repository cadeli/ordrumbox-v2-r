//! What is drawn over a sample's waveform: where playback starts and ends,
//! which way it reads, whether it loops, and the level curve (attack, hold,
//! decay) the voice follows. Shared by the kit editor and the track editor.

use ordrumbox_core::steps::semitone_ratio;
use ordrumbox_engine::kit::KitSample;

use crate::canvas::Canvas;
use crate::scale::Scale;
use crate::theme;

/// Level the decay ends at (-60 dB), as in the voice.
const FLOOR: f64 = 0.001;
/// Darkness of the parts of the sample that are not played, out of 255.
const OUTSIDE_SHADE: u8 = 150;

/// Where the settings of a sample put its playback, in fractions of the
/// sample's duration (0 = its start, 1 = its end).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overlay {
    /// First and last point read, `start < end`.
    pub start: f64,
    pub end: f64,
    /// The loop's points, inside `start`–`end`.
    pub loop_start: f64,
    pub loop_end: f64,
    pub reverse: bool,
    pub looping: bool,
    /// Lengths of the attack, the hold and the decay, in the sample's time
    /// (a tune above 0 plays it faster, so these are shorter).
    pub attack: f64,
    pub hold: f64,
    pub decay: f64,
}

/// The settings of a sample's playback, in the units the kit stores them in.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Shape {
    /// The sample's length, ms (0 when unknown).
    pub duration_ms: f64,
    pub start_ms: f64,
    pub end_ms: f64,
    /// The loop's points: the region's start and end unless the kit sets them.
    pub loop_start_ms: f64,
    pub loop_end_ms: f64,
    pub attack_ms: f64,
    pub hold_ms: f64,
    pub decay_ms: f64,
    /// Tune, semitones.
    pub transpose: f64,
    pub reverse: bool,
    pub looping: bool,
    pub gated: bool,
}

impl Shape {
    /// The playback settings of `sample`, whose length is `duration_ms`.
    #[must_use]
    pub fn of(sample: &KitSample, duration_ms: f64) -> Self {
        let end_ms = sample.end_ms.unwrap_or(duration_ms);
        Self {
            duration_ms,
            start_ms: sample.start_ms,
            end_ms,
            loop_start_ms: sample.loop_start_ms.unwrap_or(sample.start_ms),
            loop_end_ms: sample.loop_end_ms.unwrap_or(end_ms),
            attack_ms: sample.attack_ms(),
            hold_ms: sample.hold_ms,
            decay_ms: sample.decay_ms(),
            transpose: sample.transpose,
            reverse: sample.reverse,
            looping: sample.looping,
            gated: sample.gated,
        }
    }
}

/// A point of a sample's playback that a drag on its waveform moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    Start,
    End,
    LoopStart,
    LoopEnd,
    /// Where the decay reaches -60 dB.
    DecayEnd,
}

/// The shortest region, and loop, a drag leaves, ms.
const MIN_REGION_MS: f64 = 1.0;

impl Shape {
    /// The value, ms, that `handle` dragged to `fraction` of the waveform
    /// (0 its start, 1 its end) gives its setting: within the sample, the
    /// start before the end, the loop inside them; the decay measured from
    /// where reading begins, after the attack and the hold, at the tune's speed.
    #[must_use]
    pub fn dragged(&self, handle: Handle, fraction: f64) -> f64 {
        let ms = fraction.clamp(0.0, 1.0) * self.duration_ms;
        match handle {
            Handle::Start => ms.min(self.end_ms - MIN_REGION_MS).max(0.0),
            Handle::End => ms.max(self.start_ms + MIN_REGION_MS).min(self.duration_ms),
            Handle::LoopStart => ms.clamp(self.start_ms, (self.loop_end_ms - MIN_REGION_MS).max(self.start_ms)),
            Handle::LoopEnd => ms.clamp((self.loop_start_ms + MIN_REGION_MS).min(self.end_ms), self.end_ms),
            Handle::DecayEnd => {
                let origin = if self.reverse { self.end_ms } else { self.start_ms };
                let read = (ms - origin).abs() / semitone_ratio(self.transpose);
                read - self.attack_ms - self.hold_ms
            }
        }
    }
}

impl Overlay {
    /// The overlay of `shape`; `None` while the sample's length is not known.
    #[must_use]
    pub fn new(shape: &Shape) -> Option<Self> {
        if shape.duration_ms <= 0.0 {
            return None;
        }
        let speed = semitone_ratio(shape.transpose);
        let fraction = |ms: f64| (ms / shape.duration_ms).clamp(0.0, 1.0);
        let length = |ms: f64| ms * speed / shape.duration_ms;
        let start = fraction(shape.start_ms);
        let end = fraction(shape.end_ms).max(start);
        // As the voice keeps them: inside the region, the end after the start.
        let loop_start = fraction(shape.loop_start_ms).clamp(start, end);
        let loop_end = fraction(shape.loop_end_ms).clamp(loop_start, end);
        Some(Self {
            start,
            end,
            loop_start,
            loop_end,
            reverse: shape.reverse,
            looping: shape.looping,
            attack: length(shape.attack_ms),
            hold: length(shape.hold_ms),
            decay: length(shape.decay_ms),
        })
    }

    /// Where each handle is, in fractions of the waveform: the loop's only
    /// while it loops.
    #[must_use]
    pub fn handles(&self) -> Vec<(Handle, f64)> {
        let reach = self.attack + self.hold + self.decay;
        let decay_end = if self.reverse { self.end - reach } else { self.start + reach };
        let mut handles = vec![(Handle::Start, self.start), (Handle::End, self.end)];
        if self.looping {
            handles.extend([(Handle::LoopStart, self.loop_start), (Handle::LoopEnd, self.loop_end)]);
        }
        if (0.0..=1.0).contains(&decay_end) {
            handles.push((Handle::DecayEnd, decay_end));
        }
        handles
    }

    /// The handle nearest to `fraction`, within `reach` of it.
    #[must_use]
    pub fn handle_at(&self, fraction: f64, reach: f64) -> Option<Handle> {
        self.handles()
            .into_iter()
            .map(|(handle, at)| (handle, (at - fraction).abs()))
            .filter(|&(_, distance)| distance <= reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(handle, _)| handle)
    }

    /// The level (0 to 1) after the voice has read `distance` of the sample.
    fn level(&self, distance: f64) -> f64 {
        if distance < self.attack {
            return distance / self.attack;
        }
        let into_decay = distance - self.attack - self.hold;
        if into_decay <= 0.0 { 1.0 } else { FLOOR.powf(into_decay / self.decay.max(f64::EPSILON)) }
    }

    /// Draw it over the waveform in the `w` by `h` box at `x`, `y`.
    pub fn draw(&self, canvas: &mut Canvas, (x, y, w, h): (i32, i32, i32, i32), scale: Scale) {
        if w < 2 || h < 2 {
            return;
        }
        let t = scale.px();
        let column = |fraction: f64| x + (fraction.clamp(0.0, 1.0) * f64::from(w - 1)).round() as i32;
        let (start_x, end_x) = (column(self.start), column(self.end));

        // What is not played, darkened.
        canvas.shade_rect(x, y, start_x - x, h, theme::panel(), OUTSIDE_SHADE);
        canvas.shade_rect(end_x + t, y, x + w - end_x - t, h, theme::panel(), OUTSIDE_SHADE);
        if self.looping {
            let (loop_x, loop_end_x) = (column(self.loop_start), column(self.loop_end));
            for bar_y in [y, y + h - 2 * t] {
                canvas.fill_rect(loop_x, bar_y, loop_end_x - loop_x + t, 2 * t, theme::CHANCE);
            }
            canvas.fill_rect(loop_x, y, t, h, theme::CHANCE);
            canvas.fill_rect(loop_end_x, y, t, h, theme::CHANCE);
        }
        self.draw_curve(canvas, (x, y, w, h), (start_x, end_x), t);
        canvas.fill_rect(start_x, y, t, h, theme::ACCENT);
        canvas.fill_rect(end_x, y, t, h, theme::ACCENT);
        // A flag on the line where reading begins.
        let flag = 4 * scale;
        let flag_x = if self.reverse { end_x + t - flag } else { start_x };
        canvas.fill_rect(flag_x, y, flag, 3 * t, theme::ACCENT);
        // An arrowhead beside the other line, pointing at it: the way it reads.
        for j in 0..3 {
            let half = (j + 1) * t;
            let arrow_x = if self.reverse { start_x + (j + 1) * t } else { end_x - (j + 1) * t };
            canvas.fill_rect(arrow_x, y + h / 2 - half, t, 2 * half, theme::ACCENT);
        }
    }

    /// The level curve, a line from where reading begins, in the direction it goes.
    fn draw_curve(
        &self,
        canvas: &mut Canvas,
        (x, y, w, h): (i32, i32, i32, i32),
        (start_x, end_x): (i32, i32),
        t: i32,
    ) {
        let bottom = y + h - t;
        let height = f64::from(bottom - y);
        let (origin, limit, step) = if self.reverse { (end_x, start_x, -1) } else { (start_x, end_x, 1) };
        // Past the region the voice goes on only without a loop, up to the sample's edge.
        let limit = if self.looping {
            limit
        } else if self.reverse {
            x
        } else {
            x + w - 1
        };
        let mut previous = bottom;
        let mut column = origin;
        while (limit - column) * step >= 0 {
            let distance = f64::from((column - origin) * step) / f64::from(w - 1);
            let level = self.level(distance);
            let line_y = bottom - (level * height).round() as i32;
            let (top, span) = (line_y.min(previous), (line_y - previous).abs() + t);
            canvas.fill_rect(column, top, t, span, theme::VALUE);
            previous = line_y;
            if distance > self.attack + self.hold + self.decay {
                break;
            }
            column += step;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape() -> Shape {
        Shape {
            duration_ms: 1000.0,
            end_ms: 1000.0,
            attack_ms: 100.0,
            hold_ms: 100.0,
            decay_ms: 400.0,
            ..Shape::default()
        }
    }

    #[test]
    fn lengths_are_fractions_of_the_sample() {
        let overlay = Overlay::new(&Shape { start_ms: 250.0, end_ms: 750.0, ..shape() }).unwrap();
        assert_eq!((overlay.start, overlay.end), (0.25, 0.75));
        assert_eq!((overlay.attack, overlay.hold, overlay.decay), (0.1, 0.1, 0.4));
        assert!(Overlay::new(&Shape::default()).is_none(), "no length, no overlay");
    }

    #[test]
    fn a_drag_moves_a_handle_within_its_bounds() {
        let shape = Shape { start_ms: 100.0, end_ms: 800.0, loop_start_ms: 300.0, loop_end_ms: 600.0, ..shape() };
        assert_eq!(shape.dragged(Handle::Start, 0.2), 200.0);
        assert_eq!(shape.dragged(Handle::Start, 0.95), 799.0, "before the end");
        assert_eq!(shape.dragged(Handle::End, 0.05), 101.0, "after the start");
        assert_eq!(shape.dragged(Handle::End, 1.5), 1000.0, "within the sample");
        assert_eq!(shape.dragged(Handle::LoopStart, 0.0), 100.0, "inside the region");
        assert_eq!(shape.dragged(Handle::LoopStart, 0.9), 599.0, "before the loop's end");
        assert_eq!(shape.dragged(Handle::LoopEnd, 0.9), 800.0);
        // The decay: from the start, past the attack and the hold (100 ms each).
        assert_eq!(shape.dragged(Handle::DecayEnd, 0.7), 400.0);
        let octave_up = Shape { transpose: 12.0, ..shape };
        assert_eq!(octave_up.dragged(Handle::DecayEnd, 0.7), 100.0, "read twice as fast");
    }

    #[test]
    fn the_handle_under_the_pointer_is_the_nearest_in_reach() {
        let shape = Shape { start_ms: 100.0, end_ms: 800.0, loop_start_ms: 300.0, loop_end_ms: 600.0, ..shape() };
        let plain = Overlay::new(&shape).unwrap();
        assert_eq!(plain.handle_at(0.11, 0.02), Some(Handle::Start));
        assert_eq!(plain.handle_at(0.31, 0.02), None, "no loop: no loop handles");
        assert_eq!(plain.handle_at(0.7, 0.01), Some(Handle::DecayEnd), "start + 600 ms");
        assert_eq!(plain.handle_at(0.5, 0.02), None);
        let looping = Overlay::new(&Shape { looping: true, ..shape }).unwrap();
        assert_eq!(looping.handle_at(0.31, 0.02), Some(Handle::LoopStart));
        assert_eq!(looping.handle_at(0.59, 0.02), Some(Handle::LoopEnd));
    }

    #[test]
    fn a_tune_up_shortens_the_curve() {
        let octave = Overlay::new(&Shape { transpose: 12.0, ..shape() }).unwrap();
        assert!((octave.decay - 0.8).abs() < 1e-9, "twice the speed reads twice the sample in the same time");
    }

    #[test]
    fn the_level_rises_holds_then_falls_to_the_floor() {
        let overlay = Overlay::new(&shape()).unwrap();
        assert_eq!(overlay.level(0.0), 0.0);
        assert!((overlay.level(0.05) - 0.5).abs() < 1e-9);
        assert_eq!(overlay.level(0.15), 1.0);
        assert!((overlay.level(0.6) - FLOOR).abs() < 1e-9);
    }
}
