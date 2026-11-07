//! What the host's project saves besides the parameters: the library, as
//! JSON text written with the in-house codec, and the window's size.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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

/// A window size as a project holds it (its own type: the host's
/// persistence is implemented for it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedSize {
    pub width: u32,
    pub height: u32,
}

/// The window's size in logical pixels, as the user left it: shared by the
/// window (which resizes it) and the host (which asks for it and saves it).
/// Both sides are one value, never read half updated.
#[derive(Debug)]
pub struct EditorSize(AtomicU64);

impl EditorSize {
    pub fn new(size: (u32, u32)) -> Self {
        Self(AtomicU64::new(pack(size)))
    }

    #[must_use]
    pub fn get(&self) -> (u32, u32) {
        unpack(self.0.load(Ordering::Acquire))
    }

    pub fn set(&self, size: (u32, u32)) {
        self.0.store(pack(size), Ordering::Release);
    }
}

const fn pack((width, height): (u32, u32)) -> u64 {
    (width as u64) << 32 | height as u64
}

/// The halves of [`pack`]'s value: the truncations are the unpacking.
const fn unpack(packed: u64) -> (u32, u32) {
    ((packed >> 32) as u32, packed as u32)
}

impl<'a> PersistentField<'a, SavedSize> for Arc<EditorSize> {
    fn set(&self, new_value: SavedSize) {
        EditorSize::set(self, (new_value.width, new_value.height));
    }

    fn map<F, R>(&self, f: F) -> R
    where
        F: Fn(&SavedSize) -> R,
    {
        let (width, height) = self.get();
        f(&SavedSize { width, height })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_editor_size_keeps_both_sides() {
        let size = Arc::new(EditorSize::new((1100, 680)));
        assert_eq!(size.get(), (1100, 680));
        PersistentField::set(&size, SavedSize { width: u32::MAX, height: 1 });
        assert_eq!(size.get(), (u32::MAX, 1));
        assert_eq!(size.map(|s| *s), SavedSize { width: u32::MAX, height: 1 });
    }

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
