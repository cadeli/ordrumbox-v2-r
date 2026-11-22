//! Which ticks of a song each pass plays: a measure each, and in a loop
//! whose ends fall between measures, the parts of measures that stay in it.
//! The pass numbers only count up; the loop is made here.

use crate::pass::SONG_PASS_TICKS;

/// The ticks of a song played in a loop, `first..end` (`end` excluded).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SongLoop {
    first: u64,
    end: u64,
}

impl SongLoop {
    /// The loop `first..end`; `None` when it holds no tick.
    #[must_use]
    pub fn new(first: u64, end: u64) -> Option<Self> {
        (first < end).then_some(Self { first, end })
    }
}

/// The ticks `(first_tick, ticks)` pass `number` plays of a song with `song_loop`.
///
/// Before the loop's end, the passes are the song's measures, the last one
/// cut at that end; then they go round the loop: its first measure from its
/// first tick, its last one up to its end.
#[must_use]
pub fn song_span(number: u64, song_loop: Option<SongLoop>) -> (u64, u32) {
    let measure = u64::from(SONG_PASS_TICKS);
    let whole = |m: u64| (m * measure, SONG_PASS_TICKS);
    let Some(SongLoop { first, end }) = song_loop else { return whole(number) };
    let span = |from: u64, to: u64| (from, u32::try_from(to - from).unwrap_or(SONG_PASS_TICKS));
    // Passes before the measure the loop ends in, whole.
    let end_measure = end / measure;
    if number < end_measure {
        return whole(number);
    }
    let cut = !end.is_multiple_of(measure);
    if cut && number == end_measure {
        return span(end_measure * measure, end);
    }
    let round_start = end_measure + u64::from(cut);
    let first_measure = first / measure;
    let parts = (end - 1) / measure - first_measure + 1;
    let part = (number - round_start) % parts;
    let from = if part == 0 { first } else { (first_measure + part) * measure };
    span(from, ((first_measure + part + 1) * measure).min(end))
}

#[cfg(test)]
mod tests {
    use super::*;

    const M: u64 = SONG_PASS_TICKS as u64;

    fn spans(song_loop: Option<SongLoop>, numbers: std::ops::Range<u64>) -> Vec<(u64, u64)> {
        numbers.map(|n| song_span(n, song_loop)).map(|(from, ticks)| (from, from + u64::from(ticks))).collect()
    }

    #[test]
    fn passes_are_measures_until_the_loop_then_go_round_it() {
        assert_eq!(spans(None, 0..3), [(0, M), (M, 2 * M), (2 * M, 3 * M)]);
        assert_eq!(SongLoop::new(5, 5), None);

        // Measures 2 and 3, whole.
        let whole = SongLoop::new(2 * M, 4 * M);
        assert_eq!(
            spans(whole, 0..7),
            [(0, M), (M, 2 * M), (2 * M, 3 * M), (3 * M, 4 * M), (2 * M, 3 * M), (3 * M, 4 * M), (2 * M, 3 * M)]
        );

        // From the middle of measure 1 to a quarter into measure 2.
        let q = M / 4;
        let cut = SongLoop::new(M + 2 * q, 2 * M + q);
        assert_eq!(
            spans(cut, 0..6),
            [(0, M), (M, 2 * M), (2 * M, 2 * M + q), (M + 2 * q, 2 * M), (2 * M, 2 * M + q), (M + 2 * q, 2 * M)]
        );

        // Inside one measure.
        let inside = SongLoop::new(q, 2 * q);
        assert_eq!(spans(inside, 0..3), [(0, 2 * q), (q, 2 * q), (q, 2 * q)]);
        // Started past the loop: in it at once.
        assert_eq!(spans(inside, 5..6), [(q, 2 * q)]);
    }
}
