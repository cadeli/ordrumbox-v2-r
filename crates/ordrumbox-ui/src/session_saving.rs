//! Where the session's files go and when they are written: the library (and
//! the synth presets saved with it) a moment after each edit, kit knobs and
//! the master once the edits have settled.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::menu::LibraryState;

/// The library is saved this long after the last edit.
const LIBRARY_DELAY: Duration = Duration::from_secs(1);
/// Kit knob edits reach the player and the disk this long after the last one.
const KITS_DELAY: Duration = Duration::from_millis(200);
/// Master changes reach `settings.json` this long after the last one.
const MASTER_DELAY: Duration = Duration::from_millis(500);

/// Changes written once they have settled: `delay` after the last one.
#[derive(Debug, Clone, Copy)]
pub(super) struct Settle {
    delay: Duration,
    /// When the last change not written yet was made.
    since: Option<Instant>,
}

impl Settle {
    const fn new(delay: Duration) -> Self {
        Self { delay, since: None }
    }

    /// A change now: the write waits `delay` more.
    pub(super) fn touch(&mut self) {
        self.since = Some(Instant::now());
    }

    /// A change waits to be written.
    #[cfg(test)]
    pub(super) fn pending(&self) -> bool {
        self.since.is_some()
    }

    /// Nothing waits any more (written another way, or given up).
    pub(super) fn clear(&mut self) {
        self.since = None;
    }

    /// `Ok` when the write is due, else when it will be (`None`: nothing waits).
    fn due(&self) -> Result<(), Option<Instant>> {
        let due = self.since.ok_or(None)? + self.delay;
        if Instant::now() < due { Err(Some(due)) } else { Ok(()) }
    }

    /// [`Self::due`], and a write due is no longer pending.
    pub(super) fn ready(&mut self) -> Result<(), Option<Instant>> {
        self.due()?;
        self.since = None;
        Ok(())
    }

    /// As if the delay had passed since the last change.
    pub(super) fn expire(&mut self) {
        self.since = self.since.map(|_| Instant::now() - self.delay);
    }
}

/// The session's files, and the changes waiting to be written to them.
#[derive(Debug)]
pub(super) struct Saving {
    /// Where the library is saved; `None`: not saved.
    pub(super) library_path: Option<PathBuf>,
    /// Editor revision the library file holds.
    saved_revision: u64,
    /// The library file exists: read at start, or written since.
    pub(super) on_disk: bool,
    /// The last library save failed.
    pub(super) failed: bool,
    /// Library edits (synth presets included) since the last save.
    pub(super) library: Settle,
    /// Kit knob edits not yet applied to the bank and saved.
    pub(super) kits: Settle,
    /// Master bus changes not yet saved.
    pub(super) master: Settle,
    /// The settings file the colour scheme and the zoom are saved to (`None`: not saved).
    pub(super) settings_file: Option<PathBuf>,
    /// Where the song panel saves songs; `None`: no user data folder.
    pub(super) songs_dir: Option<PathBuf>,
}

impl Default for Saving {
    fn default() -> Self {
        Self {
            library_path: None,
            saved_revision: 0,
            on_disk: false,
            failed: false,
            library: Settle::new(LIBRARY_DELAY),
            kits: Settle::new(KITS_DELAY),
            master: Settle::new(MASTER_DELAY),
            settings_file: None,
            songs_dir: None,
        }
    }
}

impl Saving {
    /// Every write waiting is due now (before the files are moved or copied).
    pub(super) fn expire_all(&mut self) {
        for settle in [&mut self.library, &mut self.kits, &mut self.master] {
            settle.expire();
        }
    }

    /// The library has a file, and editor revision `revision` is not in it.
    pub(super) fn library_unsaved(&self, revision: u64) -> bool {
        self.library_path.is_some() && revision != self.saved_revision
    }

    /// The library file now holds editor revision `revision`.
    pub(super) fn library_saved(&mut self, revision: u64) {
        self.saved_revision = revision;
        self.on_disk = true;
        self.failed = false;
        self.library.clear();
    }

    /// Where the library's edits stand at editor revision `revision`, a
    /// plugin's when `hosted`.
    pub(super) fn library_state(&self, revision: u64, hosted: bool) -> LibraryState {
        if hosted {
            LibraryState::Host
        } else if self.library_path.is_none() {
            LibraryState::NotSaved
        } else if self.failed {
            LibraryState::Failed
        } else if self.library_unsaved(revision) {
            LibraryState::Modified
        } else if self.on_disk {
            LibraryState::Saved
        } else {
            LibraryState::New
        }
    }

    /// `Ok` when the library, at editor revision `revision`, is due to be
    /// saved; else when it will be (`None`: nothing to save).
    pub(super) fn library_ready(&mut self, revision: u64) -> Result<(), Option<Instant>> {
        if !self.library_unsaved(revision) {
            self.library.clear();
            return Err(None);
        }
        // Still pending once due: a save clears it, a failed one is tried again.
        self.library.due()
    }
}
