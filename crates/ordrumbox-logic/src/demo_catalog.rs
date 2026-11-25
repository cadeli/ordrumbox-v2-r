//! The demo catalog: the factory's demos, one library per style, browsed by
//! style and copied into the user's library one song or one pattern at a time.

use ordrumbox_core::Library;
use ordrumbox_core::json::Value;

/// The demos of one style: its name and description (the library's `info`),
/// and its songs and patterns.
#[derive(Debug, Clone, PartialEq)]
pub struct DemoStyle {
    name: String,
    description: String,
    library: Library,
}

/// A demo of a style: one of its songs or one of its patterns, by index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DemoItem {
    Song(usize),
    Pattern(usize),
}

/// What [`copy_demo`] did: the index of the song or pattern in the user's
/// library, and the patterns it added there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Copied {
    pub item: DemoItem,
    /// The patterns added, a song's included.
    pub added_pattern_count: usize,
    /// The library already held it (same id): nothing of it was copied.
    pub was_there: bool,
}

impl DemoStyle {
    /// The style a demo `library` file holds; `fallback` names it when its
    /// `info` has no name.
    #[must_use]
    pub fn new(library: Library, fallback: &str) -> Self {
        let text = |key: &str| library.info.get(key).and_then(Value::as_str).map(str::to_owned);
        let name = text("name").unwrap_or_else(|| fallback.to_owned());
        let description = text("description").unwrap_or_default();
        Self { name, description, library }
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn description(&self) -> &str {
        &self.description
    }

    #[must_use]
    pub fn library(&self) -> &Library {
        &self.library
    }

    /// How many demos it has: its songs and its patterns.
    #[must_use]
    pub fn item_count(&self) -> usize {
        self.library.songs.len() + self.library.patterns.len()
    }

    /// Its demo at `index` as the browser lists them: the songs, then the
    /// patterns; `None` past the last.
    #[must_use]
    pub fn item(&self, index: usize) -> Option<DemoItem> {
        let song_count = self.library.songs.len();
        match index.checked_sub(song_count) {
            None => Some(DemoItem::Song(index)),
            Some(p) => (p < self.library.patterns.len()).then_some(DemoItem::Pattern(p)),
        }
    }

    /// The name of `item`; empty for an item it does not have.
    #[must_use]
    pub fn item_name(&self, item: DemoItem) -> &str {
        match item {
            DemoItem::Song(s) => self.library.songs.get(s).map_or("", |song| song.name.as_str()),
            DemoItem::Pattern(p) => self.pattern_name(p),
        }
    }

    /// The description of `item`; empty for an item it does not have.
    #[must_use]
    pub fn item_description(&self, item: DemoItem) -> &str {
        match item {
            DemoItem::Song(s) => self.library.songs.get(s).map_or("", |song| song.description.as_str()),
            DemoItem::Pattern(p) => self.library.patterns.get(p).map_or("", |pattern| pattern.description.as_str()),
        }
    }

    fn pattern_name(&self, p: usize) -> &str {
        self.library.patterns.get(p).map_or("", |pattern| pattern.name.as_deref().unwrap_or(&pattern.id))
    }
}

/// Copy `item` of `style` into `library`: a song with the patterns it plays
/// that `library` lacks, or a pattern. What `library` already holds under
/// the same id is kept, never replaced (the user may have changed it).
/// `None` for an item `style` does not have.
pub fn copy_demo(library: &mut Library, style: &Library, item: DemoItem) -> Option<Copied> {
    match item {
        DemoItem::Song(s) => {
            let song = style.songs.get(s)?;
            if let Some(index) = library.song_index(&song.id) {
                return Some(Copied { item: DemoItem::Song(index), added_pattern_count: 0, was_there: true });
            }
            let added_pattern_count = library.add_song_from(song, style).len();
            let item = DemoItem::Song(library.songs.len() - 1);
            Some(Copied { item, added_pattern_count, was_there: false })
        }
        DemoItem::Pattern(p) => {
            let pattern = style.patterns.get(p)?;
            if let Some(index) = library.pattern_index(&pattern.id) {
                return Some(Copied { item: DemoItem::Pattern(index), added_pattern_count: 0, was_there: true });
            }
            library.patterns.push(pattern.clone());
            let item = DemoItem::Pattern(library.patterns.len() - 1);
            Some(Copied { item, added_pattern_count: 1, was_there: false })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library(text: &str) -> Library {
        Library::parse(text).expect("a library").0
    }

    const STYLE: &str = r#"{"info":{"name":"Funk","description":"Grooves."},
        "patterns":[{"id":"a","name":"A","description":"The A."},{"id":"b","name":"B"},{"id":"c"}],
        "songs":[{"id":"s","name":"S","description":"The song.","clips":[
            {"patternId":"a","startMeasure":0,"measureCount":1},{"patternId":"b","startMeasure":1,"measureCount":1},
            {"patternId":"a","startMeasure":2,"measureCount":1}]}]}"#;

    #[test]
    fn a_style_lists_its_songs_then_its_patterns() {
        let style = DemoStyle::new(library(STYLE), "funk");
        assert_eq!((style.name(), style.description()), ("Funk", "Grooves."));
        assert_eq!(style.item_count(), 4);
        assert_eq!((style.item(0), style.item(1)), (Some(DemoItem::Song(0)), Some(DemoItem::Pattern(0))));
        assert_eq!(style.item(4), None);
        let names: Vec<&str> = (0..4).filter_map(|i| style.item(i)).map(|i| style.item_name(i)).collect();
        assert_eq!(names, ["S", "A", "B", "c"], "a pattern without a name shows its id");
        assert_eq!(style.item_description(DemoItem::Song(0)), "The song.");
        assert_eq!(style.item_description(DemoItem::Pattern(0)), "The A.");
        assert_eq!(style.item_name(DemoItem::Pattern(9)), "");
    }

    #[test]
    fn a_style_without_a_name_takes_the_fallback() {
        assert_eq!(DemoStyle::new(library(r#"{"patterns":[]}"#), "rock_pop").name(), "rock_pop");
    }

    #[test]
    fn a_song_brings_the_patterns_it_plays_once_each() {
        let style = library(STYLE);
        let mut mine = library(r#"{"patterns":[{"id":"x"}]}"#);
        let copied = copy_demo(&mut mine, &style, DemoItem::Song(0)).unwrap();
        assert_eq!(copied, Copied { item: DemoItem::Song(0), added_pattern_count: 2, was_there: false });
        let ids: Vec<&str> = mine.patterns.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["x", "a", "b"], "c is not played by the song");
    }

    #[test]
    fn what_the_library_holds_is_kept() {
        let style = library(STYLE);
        let mut mine = library(r#"{"patterns":[{"id":"x"},{"id":"a","name":"My A"}]}"#);
        let copied = copy_demo(&mut mine, &style, DemoItem::Pattern(0)).unwrap();
        assert_eq!(copied, Copied { item: DemoItem::Pattern(1), added_pattern_count: 0, was_there: true });
        copy_demo(&mut mine, &style, DemoItem::Song(0)).unwrap();
        assert_eq!(mine.pattern("a").and_then(|p| p.name.as_deref()), Some("My A"), "the user's A is kept");
        let again = copy_demo(&mut mine, &style, DemoItem::Song(0)).unwrap();
        assert_eq!(again, Copied { item: DemoItem::Song(0), added_pattern_count: 0, was_there: true });
        assert_eq!(mine.songs.len(), 1);
    }

    #[test]
    fn a_pattern_is_added_at_the_end() {
        let style = library(STYLE);
        let mut mine = library(r#"{"patterns":[{"id":"x"}]}"#);
        let copied = copy_demo(&mut mine, &style, DemoItem::Pattern(2)).unwrap();
        assert_eq!(copied, Copied { item: DemoItem::Pattern(1), added_pattern_count: 1, was_there: false });
        assert_eq!(copy_demo(&mut mine, &style, DemoItem::Pattern(3)), None);
    }
}
