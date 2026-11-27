//! The symbols the views draw that the built-in font lacks (arrows,
//! triangles, a note), drawn here in pixels on the font's cell and scaled
//! like its glyphs. A font that has one draws its own.

use super::{Glyph, SIZE, advance};

/// Each symbol's cell, `WIDTH` font pixels by `SIZE` rows, `#` drawn. Letters
/// fill columns 0–4 and stand on row 6, a dash on row 3.
const SYMBOLS: &[(char, [&str; SIZE as usize])] = &[
    ('→', ["......", "..#...", "...#..", "#####.", "...#..", "..#...", "......", "......"]),
    ('←', ["......", "..#...", ".#....", "#####.", ".#....", "..#...", "......", "......"]),
    ('↔', ["......", "......", ".#.#..", "#####.", ".#.#..", "......", "......", "......"]),
    ('►', ["......", ".#....", ".##...", ".###..", ".##...", ".#....", "......", "......"]),
    ('◄', ["......", "...#..", "..##..", ".###..", "..##..", "...#..", "......", "......"]),
    ('▼', ["......", "......", "#####.", ".###..", "..#...", "......", "......", "......"]),
    ('♫', [".####.", ".#..#.", ".#..#.", ".#..#.", "##.##.", "##.##.", "......", "......"]),
];

/// The glyph of `c` in cells of `quarters` quarters of a scale unit, if it
/// is one of the symbols drawn here: each font pixel covers `quarters / 4`
/// pixels, partly covered pixels grey as a font's edges are.
pub(super) fn glyph(c: char, quarters: i32) -> Option<Glyph> {
    let (_, rows) = SYMBOLS.iter().find(|(symbol, _)| *symbol == c)?;
    let width = usize::try_from(advance(quarters)).ok()?.max(1);
    let height = usize::try_from((SIZE * quarters + 2) / 4).ok()?.max(1);
    let px = quarters as f32 / 4.0;
    let mut area = vec![0.0f32; width * height];
    for (y, row) in rows.iter().enumerate() {
        for (x, _) in row.bytes().enumerate().filter(|&(_, b)| b == b'#') {
            cover(&mut area, width, (x as f32 * px, y as f32 * px), px);
        }
    }
    let coverage = area.iter().map(|a| (a.min(1.0) * 255.0).round() as u8).collect();
    Some(Glyph { width, coverage, left: 0, top: 0 })
}

/// Adds to `area` (rows of `width` pixels) the part of each pixel that the
/// square of side `side` at `corner` covers.
fn cover(area: &mut [f32], width: usize, corner: (f32, f32), side: f32) {
    let height = area.len() / width;
    let span = |start: f32, len: usize| {
        let first = start.floor() as usize;
        let last = ((start + side).ceil() as usize).min(len);
        (first..last).map(move |i| (i, (start + side).min(i as f32 + 1.0) - start.max(i as f32)))
    };
    for (y, dy) in span(corner.1, height) {
        for (x, dx) in span(corner.0, width) {
            area[y * width + x] += dx * dy;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::WIDTH;
    use super::*;

    #[test]
    fn every_symbol_fills_one_cell() {
        for (symbol, rows) in SYMBOLS {
            assert!(rows.iter().all(|row| row.len() == WIDTH as usize), "{symbol}");
        }
    }

    #[test]
    fn a_symbol_is_its_pixels_at_whole_scales() {
        let arrow = glyph('→', 8).unwrap();
        let row = |y: usize| &arrow.coverage[y * arrow.width..][..arrow.width];
        assert_eq!(arrow.width, 2 * WIDTH as usize);
        assert_eq!(row(6), [255; 10].iter().chain(&[0, 0]).copied().collect::<Vec<_>>(), "the shaft, 2 px high");
        assert_eq!(row(0), [0; 12]);
        assert!(glyph('A', 4).is_none());
    }

    #[test]
    fn a_symbol_greys_its_edges_at_three_quarters() {
        let arrow = glyph('→', 3).unwrap();
        assert!(arrow.coverage.iter().any(|&c| c > 0 && c < 255), "{arrow:?}");
    }
}
