//! Which ticks of a pattern each pass plays: all of it, or the part a loop
//! keeps, and the first pass after a seek, which starts inside it. A pass is
//! built whole, then cut to its ticks.

use crate::pass::Pass;

/// The ticks `first..end` (`end` excluded) of pattern `pattern` that play in a loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternLoop {
    pattern: usize,
    first: u32,
    end: u32,
}

impl PatternLoop {
    /// The loop of `pattern` over ticks `first..end`; `None` when it holds no tick.
    #[must_use]
    pub fn new(pattern: usize, first: u32, end: u32) -> Option<Self> {
        (first < end).then_some(Self { pattern, first, end })
    }

    /// The pattern it loops.
    #[must_use]
    pub const fn pattern(&self) -> usize {
        self.pattern
    }
}

/// A seek: pass `number` starts at tick `tick` of its pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Seek {
    pub(super) number: u64,
    pub(super) tick: u32,
}

/// The ticks `(first, end)` pass `number` plays of a pattern `ticks` long
/// with `pattern_loop` (already the pattern's own) and `seek`.
///
/// The loop's ticks, else all of them. The pass a seek starts goes on from
/// its tick to the loop's end, or to the pattern's when the tick is past it;
/// the passes after it are the loop's again.
#[must_use]
pub(super) fn pattern_span(
    number: u64,
    ticks: u32,
    pattern_loop: Option<PatternLoop>,
    seek: Option<Seek>,
) -> (u32, u32) {
    let (first, end) = pattern_loop
        .map(|l| (l.first.min(ticks), l.end.min(ticks)))
        .filter(|(first, end)| first < end)
        .unwrap_or((0, ticks));
    match seek {
        Some(seek) if seek.number == number && seek.tick < ticks => {
            (seek.tick, if seek.tick < end { end } else { ticks })
        }
        _ => (first, end),
    }
}

impl Pass {
    /// Keep ticks `first..end` of this pass, built whole: its notes there,
    /// its LFO values there, and the length of what is left.
    pub(super) fn cut(&mut self, first: u32, end: u32) {
        let end = end.min(self.ticks);
        let first = first.min(end);
        if (first, end) == (0, self.ticks) {
            return;
        }
        let frame_at = |tick: u32| {
            u64::try_from(u128::from(self.frames) * u128::from(tick) / u128::from(self.ticks.max(1)))
                .unwrap_or(u64::MAX)
        };
        let (from, to) = (frame_at(first), frame_at(end));
        self.events.retain(|e| (from..to).contains(&e.frame));
        for event in &mut self.events {
            event.frame -= from;
        }
        let keep = |values: &mut Vec<f32>| {
            if values.is_empty() {
                return;
            }
            values.truncate(end as usize);
            values.drain(..(first as usize).min(values.len()));
        };
        for lfo in &mut self.lfos {
            [&mut lfo.volume, &mut lfo.pan, &mut lfo.cutoff, &mut lfo.q].into_iter().for_each(keep);
            lfo.controls.iter_mut().for_each(|(_, values)| keep(values));
        }
        self.first_tick += u64::from(first);
        self.frames = (to - from).max(1);
        self.ticks = end - first;
        self.offset_ticks += first;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pass::{Event, Sound, SynthStart};
    use crate::strip_settings::StripLfo;

    #[test]
    fn a_pass_plays_the_loop_and_a_seek_starts_inside_it() {
        let looped = PatternLoop::new(0, 24, 72);
        assert_eq!(PatternLoop::new(0, 5, 5), None);
        assert_eq!(pattern_span(0, 96, None, None), (0, 96));
        assert_eq!(pattern_span(7, 96, looped, None), (24, 72));
        // The loop's end past the pattern (it was shortened): cut to it.
        assert_eq!(pattern_span(1, 48, looped, None), (24, 48));
        let at = |number, tick| Some(Seek { number, tick });
        assert_eq!(pattern_span(0, 96, looped, at(0, 40)), (40, 72), "from the seek to the loop's end");
        assert_eq!(pattern_span(1, 96, looped, at(0, 40)), (24, 72), "then the loop");
        assert_eq!(pattern_span(0, 96, looped, at(0, 80)), (80, 96), "past the loop: to the pattern's end");
        assert_eq!(pattern_span(0, 96, None, at(0, 40)), (40, 96));
        assert_eq!(pattern_span(0, 96, None, at(0, 200)), (0, 96), "a seek past the pattern is none");
    }

    #[test]
    fn a_cut_keeps_the_notes_and_the_lfo_values_of_its_ticks() {
        let event = |frame| Event {
            frame,
            sound: Sound::Synth(SynthStart { track: 0, patch: 0, length: 0.1 }),
            mono: false,
            velocity: 1.0,
            note: None,
            note_frames: 0,
            lock: None,
        };
        let mut pass = Pass {
            first_tick: 100,
            frames: 960,
            ticks: 96,
            events: [0, 100, 300, 500, 959].map(event).into(),
            lfos: vec![StripLfo { track: 0, volume: (0..96).map(|t| t as f32).collect(), ..StripLfo::default() }],
            ..Pass::default()
        };
        pass.cut(24, 72);
        assert_eq!((pass.first_tick, pass.frames, pass.ticks, pass.offset_ticks), (124, 480, 48, 24));
        assert_eq!(pass.events.iter().map(|e| e.frame).collect::<Vec<_>>(), [60, 260]);
        assert_eq!(pass.lfos[0].volume.first().copied(), Some(24.0));
        assert_eq!(pass.lfos[0].volume.len(), 48);
    }
}
