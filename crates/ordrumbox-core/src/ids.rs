//! Stable identifiers (`src/core/ids.js`): pattern and song ids are slugs of
//! their names, made unique within a library, and never rewritten afterwards.

use std::collections::HashSet;

/// Maximum slug length, like `slice(0, 48)` in ids.js.
const MAX_SLUG_LEN: usize = 48;

/// Lowercase alphanumerics and single dashes, no dash at either end, at most 48
/// characters. Accented Latin letters lose their accent (`Ébauche` → `ebauche`);
/// any other character separates words. `""` when nothing is sluggable.
///
/// Same output as `slugify()` in ids.js (`normalize('NFD')`, strip
/// U+0300–U+036F, lowercase, `[^a-z0-9]+` → `-`), without a Unicode table
/// dependency: see [`fold_char`].
pub fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in value.chars() {
        match fold_char(c) {
            Fold::Drop => {}
            Fold::Keep(ascii) => {
                if pending_dash && !slug.is_empty() {
                    slug.push('-');
                }
                pending_dash = false;
                slug.push(ascii);
            }
            Fold::Separator => pending_dash = true,
        }
    }
    slug.truncate(MAX_SLUG_LEN);
    slug
}

/// `candidate`, or the first free `candidate-2`, `candidate-3`…; the result is
/// added to `taken`. An empty candidate becomes `pattern`.
pub fn unique_id(candidate: &str, taken: &mut HashSet<String>) -> String {
    let base = if candidate.is_empty() { "pattern" } else { candidate };
    let id = if taken.contains(base) {
        (2..).map(|n| format!("{base}-{n}")).find(|id| !taken.contains(id)).unwrap_or_default()
    } else {
        base.to_owned()
    };
    taken.insert(id.clone());
    id
}

enum Fold {
    /// A combining accent (U+0300–U+036F): removed without a trace.
    Drop,
    /// Lowercase ASCII letter or digit.
    Keep(char),
    /// Anything else: a word boundary.
    Separator,
}

fn fold_char(c: char) -> Fold {
    if c.is_ascii_alphanumeric() {
        return Fold::Keep(c.to_ascii_lowercase());
    }
    let code = u32::from(c);
    if (0x300..=0x36F).contains(&code) {
        return Fold::Drop;
    }
    let base = FOLD_RANGES.iter().find_map(|&(start, letters)| {
        let offset = code.checked_sub(start)? as usize;
        letters.as_bytes().get(offset).copied()
    });
    match base {
        Some(b) if b != b'.' => Fold::Keep(char::from(b)),
        _ => Fold::Separator,
    }
}

/// Base letter of every precomposed character whose canonical decomposition is
/// one ASCII letter plus U+0300–U+036F accents (`.` = none), by code point from
/// the range start. Generated from the Unicode 15.1 data (Node 22's version):
/// 490 characters, all in these ranges.
const FOLD_RANGES: [(u32, &str); 3] = [
    (
        0x00C0,
        concat!(
            "aaaaaa.ceeeeiiii.nooooo..uuuuy..aaaaaa.ceeeeiiii.nooooo..uuuuy.y", // U+00C0
            "aaaaaaccccccccdd..eeeeeeeeeegggggggghh..iiiiiiiii...jjkk.llllll.", // U+0100
            "...nnnnnn...oooooo..rrrrrrsssssssstttt..uuuuuuuuuuuuwwyyyzzzzzz.", // U+0140
            "................................oo.............uu...............", // U+0180
            ".............aaiioouuuuuuuuuu.aaaa....ggkkoooo..j...gg..nnaa....", // U+01C0
            "aaaaeeeeiiiioooorrrruuuusstt..hh......aaeeooooooooyy............", // U+0200
            "................",                                                 // U+0240
        ),
    ),
    (
        0x1E00,
        concat!(
            "aabbbbbbccddddddddddeeeeeeeeeeffgghhhhhhhhhhiiiikkkkkkllllllllmm", // U+1E00
            "mmmmnnnnnnnnoooooooopppprrrrrrrrssssssssssttttttttuuuuuuuuuuvvvv", // U+1E40
            "wwwwwwwwwwxxxxyyzzzzzzhtwy......aaaaaaaaaaaaaaaaaaaaaaaaeeeeeeee", // U+1E80
            "eeeeeeeeiiiioooooooooooooooooooooooouuuuuuuuuuuuuuyyyyyyyy......", // U+1EC0
        ),
    ),
    (0x212A, "ka"), // KELVIN SIGN, ANGSTROM SIGN
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fold_table_covers_490_characters() {
        let count: usize = FOLD_RANGES.iter().map(|(_, letters)| letters.bytes().filter(|&b| b != b'.').count()).sum();
        assert_eq!(count, 490);
    }

    #[test]
    fn unique_id_appends_the_first_free_suffix() {
        let mut taken = HashSet::new();
        assert_eq!(unique_id("rock", &mut taken), "rock");
        assert_eq!(unique_id("rock", &mut taken), "rock-2");
        assert_eq!(unique_id("rock", &mut taken), "rock-3");
        assert_eq!(unique_id("", &mut taken), "pattern");
        assert_eq!(unique_id("", &mut taken), "pattern-2");
    }
}
