//! The library as the host's project saves it: JSON text, written with the
//! in-house codec.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use nih_plug::params::persist::PersistentField;
use serde::{Deserialize, Serialize};

/// The text a project holds (its own type: the host's persistence is
/// implemented for it).
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LibraryText(pub String);

/// The library's JSON, shared by the host (which saves and restores it) and
/// the player (which plays it and publishes its edits).
#[derive(Debug, Default)]
pub struct LibraryState {
    json: RwLock<LibraryText>,
    /// Set when the host restored a project: the player reloads the library.
    restored: AtomicBool,
}

impl LibraryState {
    /// The JSON a restored project gave, once.
    pub fn take_restored(&self) -> Option<String> {
        self.restored.swap(false, Ordering::AcqRel).then(|| self.read())
    }

    /// The library to save with the project from now on.
    pub fn publish(&self, json: String) {
        *self.json.write().unwrap_or_else(|poisoned| poisoned.into_inner()) = LibraryText(json);
    }

    /// Nothing saved yet (a new instance, not a restored project).
    pub fn is_empty(&self) -> bool {
        self.json.read().unwrap_or_else(|poisoned| poisoned.into_inner()).0.is_empty()
    }

    fn read(&self) -> String {
        self.json.read().unwrap_or_else(|poisoned| poisoned.into_inner()).0.clone()
    }
}

impl<'a> PersistentField<'a, LibraryText> for Arc<LibraryState> {
    fn set(&self, new_value: LibraryText) {
        self.publish(new_value.0);
        self.restored.store(true, Ordering::Release);
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&LibraryText) -> R,
    {
        f(&self.json.read().unwrap_or_else(|poisoned| poisoned.into_inner()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restore_is_taken_once() {
        let state = Arc::new(LibraryState::default());
        assert!(state.is_empty() && state.take_restored().is_none());
        state.publish("{}".into());
        assert!(state.take_restored().is_none(), "a publish is not a restore");
        state.set(LibraryText(r#"{"patterns":[]}"#.into()));
        assert_eq!(state.take_restored().as_deref(), Some(r#"{"patterns":[]}"#));
        assert!(state.take_restored().is_none());
        assert_eq!(state.map(|t| t.0.len()), 15);
    }
}
