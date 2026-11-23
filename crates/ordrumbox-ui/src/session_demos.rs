//! The new demos of an upgrade: at the start of the program, the patterns and songs of the assets'
//! library that the user's library has not been offered yet are added to it (one undo step). The
//! ids offered so far are the `demos` setting of `settings.json`; the first start only records
//! them. A demo that changed or went away in the assets is not touched: Help > Factory reset... does.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_logic::demos::{self, KnownDemos};

use super::Session;
use super::session_user_data::read_library;
use crate::store;

/// The setting holding the ids of the demos offered.
const DEMOS_KEY: &str = "demos";
const PATTERNS_KEY: &str = "patterns";
const SONGS_KEY: &str = "songs";

/// The ids `value` holds, those of the wrong type left out and reported.
fn ids(value: Option<&Value>, key: &str, issues: &mut Vec<String>) -> Vec<String> {
    let Some(items) = value.and_then(Value::as_array) else {
        issues.push(format!("settings.json: {DEMOS_KEY}.{key}: not a list, none known"));
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let id = item.as_str().map(str::to_owned);
            if id.is_none() {
                issues.push(format!("settings.json: {DEMOS_KEY}.{key}: {item} is not an id, ignored"));
            }
            id
        })
        .collect()
}

fn known_from_json(value: &Value, issues: &mut Vec<String>) -> KnownDemos {
    KnownDemos {
        pattern_ids: ids(value.get(PATTERNS_KEY), PATTERNS_KEY, issues),
        song_ids: ids(value.get(SONGS_KEY), SONGS_KEY, issues),
    }
}

fn known_to_json(known: &KnownDemos) -> Value {
    let list = |ids: &[String]| Value::from(ids.iter().map(|id| Value::from(id.as_str())).collect::<Vec<_>>());
    let mut object = Object::new();
    object.insert(PATTERNS_KEY, list(&known.pattern_ids));
    object.insert(SONGS_KEY, list(&known.song_ids));
    Value::from(object)
}

/// "3 new demos added: A, B, C" (the names of the first few).
fn added_line(added: &demos::AddedDemos) -> String {
    const SHOWN: usize = 4;
    let names: Vec<&str> = added.song_names.iter().chain(&added.pattern_names).map(String::as_str).collect();
    let more = names.len().saturating_sub(SHOWN);
    let shown = names.iter().take(SHOWN).copied().collect::<Vec<_>>().join(", ");
    let more = if more == 0 { String::new() } else { format!(" and {more} more") };
    format!("new demos added: {shown}{more}")
}

impl Session {
    /// At start of the program: add the demos the user's library has not been offered, and keep
    /// the ids of all those of the assets.
    pub fn add_new_demos(&mut self) {
        let (Some(settings), Some(factory_path)) = (store::settings_path(), self.demo_library_path()) else {
            return;
        };
        let factory = match read_library(&factory_path) {
            Ok((factory, _)) => factory,
            Err(e) => {
                self.status.push(format!("new demos not looked for: {e}"));
                return;
            }
        };
        // An unreadable file has been reported by `load_master`.
        let Ok(saved) = store::load_setting(&settings, DEMOS_KEY) else { return };
        let all = KnownDemos::of(&factory);
        if let Some(saved) = &saved {
            let mut issues = Vec::new();
            let known = known_from_json(saved, &mut issues);
            self.status.extend(issues);
            let mut library = self.editor.library().clone();
            let added = demos::add_new_demos(&mut library, &factory, &known);
            if !added.is_empty() {
                self.put_library(library, "Add the new demos");
                let line = added_line(&added);
                self.status.push(line.clone());
                self.message = line;
            }
        }
        if saved.as_ref().map(|saved| known_from_json(saved, &mut Vec::new())) != Some(all.clone())
            && let Err(e) = store::save_setting(&settings, DEMOS_KEY, known_to_json(&all))
        {
            self.status.push(format!("settings not saved: {}: {e}", settings.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_known_ids_survive_the_json() {
        let known = KnownDemos { pattern_ids: vec!["a".into(), "b".into()], song_ids: vec!["s".into()] };
        let mut issues = Vec::new();
        assert_eq!(known_from_json(&known_to_json(&known), &mut issues), known);
        assert!(issues.is_empty(), "{issues:?}");
    }

    #[test]
    fn a_wrong_setting_is_reported_and_nothing_is_known() {
        let mut issues = Vec::new();
        let known = known_from_json(&Value::from("x"), &mut issues);
        assert_eq!(known, KnownDemos::default());
        assert_eq!(issues.len(), 2);
    }

    #[test]
    fn the_line_names_the_first_demos_and_counts_the_rest() {
        let added = demos::AddedDemos {
            pattern_names: (1..=5).map(|i| format!("P{i}")).collect(),
            song_names: vec!["S".into()],
        };
        assert_eq!(added_line(&added), "new demos added: S, P1, P2, P3 and 2 more");
    }
}
