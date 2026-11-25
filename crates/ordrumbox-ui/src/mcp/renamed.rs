//! The names of tools and arguments that changed: what clients written
//! before still call, and what it stands for now.

use std::borrow::Cow;

use ordrumbox_core::json::Value;

/// Tools renamed: the old name, still called by clients written before, and
/// the tool it stands for. A song file became a project file, then an
/// arrangement became a song.
pub(super) const RENAMED_TOOLS: [(&str, &str); 15] = [
    ("setSongInfo", "setProjectInfo"),
    ("saveSong", "saveProject"),
    ("exportSong", "exportProject"),
    ("listSavedSongs", "listSavedProjects"),
    ("loadSong", "openProject"),
    ("listArrangements", "listSongs"),
    ("createArrangement", "createSong"),
    ("addPatternToArrangement", "addPatternToSong"),
    ("removePatternFromArrangement", "removePatternFromSong"),
    ("updateArrangement", "updateSong"),
    ("removeArrangement", "removeSong"),
    ("renameArrangement", "renameSong"),
    ("selectArrangement", "selectSong"),
    ("exportArrangementWav", "exportSongWav"),
    ("exportArrangementStems", "exportSongStems"),
];

/// Arguments renamed with their tools: the old name, still given by clients
/// written before, and the argument it stands for.
const RENAMED_ARGUMENTS: [(&str, &str); 1] = [("arrangement", "song")];

/// The tool `name` stands for: itself, or the new name of a renamed tool.
pub(super) fn tool_name(name: &str) -> &str {
    RENAMED_TOOLS.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new)
}

/// `arguments` with each renamed argument under its new name (the new name,
/// when both are given, wins).
pub(super) fn tool_arguments(arguments: &Value) -> Cow<'_, Value> {
    let Some(object) = arguments.as_object() else { return Cow::Borrowed(arguments) };
    if !RENAMED_ARGUMENTS.iter().any(|(old, _)| object.contains_key(old)) {
        return Cow::Borrowed(arguments);
    }
    let mut renamed = object.clone();
    for (old, new) in RENAMED_ARGUMENTS {
        if let Some(value) = renamed.remove(old)
            && !renamed.contains_key(new)
        {
            renamed.insert(new, value);
        }
    }
    Cow::Owned(Value::Object(renamed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ordrumbox_core::json;

    #[test]
    fn an_old_argument_takes_its_new_name() {
        let old = json::parse(r#"{"arrangement":"intro","bpm":90}"#).unwrap();
        assert_eq!(tool_arguments(&old).to_json(), r#"{"bpm":90,"song":"intro"}"#);
        let both = json::parse(r#"{"arrangement":"a","song":"b"}"#).unwrap();
        assert_eq!(tool_arguments(&both).to_json(), r#"{"song":"b"}"#);
        let plain = json::parse(r#"{"song":"c"}"#).unwrap();
        assert!(matches!(tool_arguments(&plain), Cow::Borrowed(_)));
    }
}
