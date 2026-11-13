//! What the end-to-end tests share.

#![allow(clippy::unwrap_used, reason = "a test: a failed expectation fails the test")]

use std::path::PathBuf;

/// A folder of its own for test `name`, empty: removed first if an earlier run left it.
pub fn scratch_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-{name}-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
