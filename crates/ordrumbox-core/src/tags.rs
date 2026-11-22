//! What a pattern's tags mean: a tag is a genre (rock, funk, reggae…),
//! which groups the patterns and teaches the styles, or a feature (a bass,
//! chords, a polyrhythm), which only describes the pattern.

use crate::Pattern;

/// The tags that say what a pattern has, not the genre it is in.
pub const FEATURE_TAGS: [&str; 13] = [
    "bass",
    "chords",
    "lead",
    "pad",
    "synth",
    "organ",
    "strings",
    "brass",
    "piano",
    "clavinet",
    "arpeggio",
    "polyrhythm",
    "polymeter",
];

/// Whether `tag` is a feature, not a genre (any case, around spaces).
#[must_use]
pub fn is_feature(tag: &str) -> bool {
    let tag = tag.trim();
    FEATURE_TAGS.iter().any(|feature| feature.eq_ignore_ascii_case(tag))
}

/// `tags` as they are kept: trimmed, in lower case, without empty ones or
/// repeats, in their order.
#[must_use]
pub fn normalize<S: AsRef<str>>(tags: &[S]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for tag in tags.iter().map(|tag| tag.as_ref().trim().to_lowercase()) {
        if !tag.is_empty() && !kept.contains(&tag) {
            kept.push(tag);
        }
    }
    kept
}

/// The tags written in `text`, separated by commas: `funk, bass`.
#[must_use]
pub fn parse(text: &str) -> Vec<String> {
    normalize(&text.split(',').collect::<Vec<_>>())
}

/// `tags` written as [`parse`] reads them back.
#[must_use]
pub fn to_text(tags: &[String]) -> String {
    tags.join(", ")
}

/// The patterns grouped by genre ([`Pattern::genre`]): each genre in
/// alphabetical order with the indices of its patterns, in their order; the
/// patterns without a genre last, under `None`.
#[must_use]
pub fn by_genre(patterns: &[Pattern]) -> Vec<(Option<&str>, Vec<usize>)> {
    let mut groups: Vec<(Option<&str>, Vec<usize>)> = Vec::new();
    for (index, pattern) in patterns.iter().enumerate() {
        let genre = pattern.genre();
        match groups.iter_mut().find(|(g, _)| *g == genre) {
            Some((_, indices)) => indices.push(index),
            None => groups.push((genre, vec![index])),
        }
    }
    // `None` sorts first: the patterns without a genre go last.
    groups.sort_by(|(a, _), (b, _)| a.is_none().cmp(&b.is_none()).then(a.cmp(b)));
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_group_by_their_first_genre() {
        let tagged =
            |tags: &[&str]| Pattern { tags: tags.iter().map(|t| (*t).to_owned()).collect(), ..Pattern::default() };
        let patterns = [tagged(&["rock"]), tagged(&["bass", "funk", "rock"]), tagged(&["synth"]), tagged(&["rock"])];
        assert_eq!(patterns[1].genre(), Some("funk"), "a feature first is passed over");
        assert_eq!(by_genre(&patterns), [(Some("funk"), vec![1]), (Some("rock"), vec![0, 3]), (None, vec![2])]);
    }

    #[test]
    fn features_are_told_from_genres() {
        assert!(is_feature("bass") && is_feature(" Synth ") && is_feature("polyrhythm"));
        assert!(!is_feature("funk") && !is_feature("reggae") && !is_feature(""));
    }

    #[test]
    fn tags_read_back_as_written() {
        let tags = parse(" Funk, bass,,funk , Chords ");
        assert_eq!(tags, ["funk", "bass", "chords"]);
        assert_eq!(to_text(&tags), "funk, bass, chords");
        assert_eq!(parse(&to_text(&tags)), tags);
        assert!(parse(" , ").is_empty());
    }
}
