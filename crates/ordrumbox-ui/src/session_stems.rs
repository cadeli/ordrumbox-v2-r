//! The stems export: each track of a pattern or a song as a WAV file of its
//! own, in a folder made for them.

use super::Session;
use super::session_files::NOTHING_TO_RENDER;
use crate::store;
use ordrumbox_core::Library;
use ordrumbox_engine::offline::{self, Stereo};
use ordrumbox_engine::{Mode, wav};
use std::path::Path;
use std::time::Instant;

/// The name of the stem of track `index` (from 0): its position, then its name.
fn stem_file_name(index: usize, track_name: &str) -> String {
    let name = store::safe_name(track_name);
    format!("{:02}-{}.wav", index + 1, if name.is_empty() { "track" } else { &name })
}

/// The name of each track played at its position: the first pattern played
/// that has a track there gives it.
fn track_names(library: &Library, mode: Mode) -> Vec<String> {
    let played: Vec<_> = match mode {
        Mode::Pattern(p) => library.patterns.get(p).into_iter().collect(),
        Mode::Song(s) => library
            .songs
            .get(s)
            .into_iter()
            .flat_map(|song| &song.clips)
            .filter_map(|c| library.patterns.iter().find(|p| p.id == c.pattern_id))
            .collect(),
    };
    let count = played.iter().map(|p| p.tracks.len()).max().unwrap_or(0);
    (0..count)
        .map(|i| played.iter().find_map(|p| p.tracks.get(i)).map(|t| t.name.clone()).unwrap_or_default())
        .collect()
}

impl Session {
    /// Ctrl-less: the selected pattern (a song with `song`) as stems in the
    /// folder at `dir`.
    pub(super) fn export_stems(&mut self, song: bool, dir: &Path) {
        let mode = if song { Mode::Song(self.editor.selected_song()) } else { Mode::Pattern(self.pattern_index) };
        self.message = self.write_stems(mode, dir).unwrap_or_else(|e| e);
    }

    /// What `mode` plays as one WAV per track in the folder `dir`, before the
    /// master bus; the tracks that stay silent
    /// are left out. What was written.
    pub(super) fn write_stems(&self, mode: Mode, dir: &Path) -> Result<String, String> {
        let setup = self.render.as_ref().ok_or(NOTHING_TO_RENDER)?;
        let library = self.editor.library();
        let started = Instant::now();
        let stems = match mode {
            Mode::Song(index) => {
                if library.songs.get(index).is_none() {
                    return Err("no song in the library".into());
                }
                offline::render_song_stems(library, index, setup)
            }
            Mode::Pattern(p) => {
                self.with_auto_loops(setup.seed, |hook| offline::render_pattern_stems(library, p, setup, Some(hook)))
            }
        };
        let stems = stems.ok_or("nothing to export (no playable clip)")?;
        let names = track_names(library, mode);
        let audible = |(left, right): &Stereo| left.iter().chain(right).any(|s| s.abs() > SILENCE);
        let mut written = 0;
        for (index, stem) in stems.iter().enumerate().filter(|(_, stem)| audible(stem)) {
            let path = dir.join(stem_file_name(index, names.get(index).map_or("", String::as_str)));
            let bytes = wav::encode(&stem.0, &stem.1, setup.sample_rate);
            store::write_atomic(&path, bytes).map_err(|e| format!("not exported: {}: {e}", path.display()))?;
            written += 1;
        }
        if written == 0 {
            return Err("nothing to export (every track is silent)".into());
        }
        Ok(format!("exported {written} stems to {} (in {:.1} s)", dir.display(), started.elapsed().as_secs_f64()))
    }
}

/// A stem whose samples all stay under this is silent (a track with no
/// sound in it), and is not written.
const SILENCE: f32 = 1e-6;
