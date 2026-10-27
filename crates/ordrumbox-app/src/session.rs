//! Everything the window drives: the library being edited, the playback, the
//! generators, MIDI files, the grid cursor and selection, the clipboard, saving. Edits go through the
//! [`Editor`] (each one an undo step); the playing pass is rebuilt after each.
//!
//! The grid follows the JS pattern panel (`pointer_section.js`,
//! `keyboard_section.js`, `clipboard_section.js`): a click on an empty step
//! adds a note, on a note selects it, on the selected note deletes it.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ordrumbox_core::drum_taxonomy::{TrackType, detect_track_type};
use ordrumbox_core::{Library, Note, Pattern, Rng, Scales, Track};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{Controller, Kit, Mode, wav};
use ordrumbox_logic::generate::{self, AutoGen, MelodicKind};
use ordrumbox_logic::midi;
use ordrumbox_logic::{Editor, Nudge};

use crate::assets;
use crate::audio;
use crate::canvas::Canvas;
use crate::store;
use crate::view::{self, Cursor, Hit, View};

/// Save this long after the last edit.
const SAVE_DELAY: Duration = Duration::from_secs(1);

/// A direction (arrow key).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Debug, Clone)]
enum Clipboard {
    Step(Vec<Note>),
    Track(Box<Track>),
}

pub struct Session {
    editor: Editor,
    /// What the generators read (`scales.json`).
    scales: Scales,
    /// Random draws of the generators.
    rng: Rng,
    /// Root note of the loaded samples that have one (key, MIDI note), for MIDI files.
    roots: Vec<(String, f64)>,
    kits: Vec<Kit>,
    kit_index: usize,
    pattern_index: usize,
    song_index: usize,
    /// What offline renders (WAV exports) use; `None` without samples.
    render: Option<RenderSetup>,
    /// `None` when no audio device could be opened.
    controller: Option<Controller>,
    _output: Option<audio::Output>,
    /// Startup problems, shown until quit.
    status: Vec<String>,
    /// Outcome of the last action.
    message: String,
    cursor: Option<Cursor>,
    /// Selected note: track and index in its notes.
    selected: Option<(usize, usize)>,
    clipboard: Option<Clipboard>,
    /// Where the library is saved; `None`: not saved.
    library_path: Option<PathBuf>,
    saved_revision: u64,
    /// Editor revision the playing pass was built from.
    played_revision: u64,
    changed_at: Option<Instant>,
}

impl Session {
    pub fn load() -> Self {
        let Some(dir) = assets::find() else {
            let searched: Vec<String> = assets::candidates().iter().map(|d| d.display().to_string()).collect();
            return Self::new(
                Library::default(),
                Vec::new(),
                vec![
                    format!(
                        "assets folder not found: copy ordrumbox-v2/assets next to the program, or set {}",
                        assets::ENV_VAR
                    ),
                    format!("searched: {}", searched.join(", ")),
                ],
            );
        };
        let assets = match assets::load(&dir) {
            Ok(assets) => assets,
            Err(e) => return Self::new(Library::default(), Vec::new(), vec![e]),
        };
        let mut status = Vec::new();
        if let Some(first) = assets.sample_errors.first() {
            status.push(format!("{} sample(s) not loaded, first: {first}", assets.sample_errors.len()));
        }

        // The user's library when there is one, else the assets' song.json.
        let mut library = assets.library;
        let mut message = "demo library (assets/data/song.json)".to_owned();
        let library_path = match store::path() {
            None => {
                status.push("no user data folder: edits are not saved".into());
                None
            }
            Some(path) => match store::load(&path) {
                Ok(Some(saved)) => {
                    library = saved;
                    message = format!("library {}", path.display());
                    Some(path)
                }
                Ok(None) => Some(path),
                Err(e) => {
                    status.push(format!("library not loaded, not saved over: {e}"));
                    None
                }
            },
        };
        if library_path.is_none() && !assets.report.issues.is_empty() {
            status.push(format!("song.json: {} value(s) repaired", assets.report.issues.len()));
        }

        let mut session = Self::new(library, assets.kits, status);
        session.scales = assets.scales.clone();
        session.roots = assets
            .bank
            .samples
            .iter()
            .filter_map(|s| Some((s.instrument.key.clone(), s.instrument.root_midi?)))
            .collect();
        session.library_path = library_path;
        session.message = message;
        let bank = Arc::new(assets.bank);
        if !bank.samples.is_empty() {
            session.render = Some(RenderSetup {
                bank: bank.clone(),
                scales: assets.scales.clone(),
                synths: assets.synths.clone(),
                kit: session.kits.first().map(|k| k.name.clone()).unwrap_or_default(),
                sample_rate: offline::EXPORT_RATE,
                seed: 1,
            });
        }
        match audio::start(bank, assets.scales, assets.synths) {
            Ok((output, controller)) => {
                session.controller = Some(controller);
                session._output = Some(output);
            }
            Err(e) => session.status.push(format!("no sound: {e}")),
        }
        session
    }

    fn new(library: Library, kits: Vec<Kit>, status: Vec<String>) -> Self {
        Self {
            editor: Editor::new(library),
            scales: Scales::default(),
            rng: Rng::from_time(),
            roots: Vec::new(),
            kits,
            kit_index: 0,
            pattern_index: 0,
            song_index: 0,
            render: None,
            controller: None,
            _output: None,
            status,
            message: String::new(),
            cursor: None,
            selected: None,
            clipboard: None,
            library_path: None,
            saved_revision: 0,
            played_revision: 0,
            changed_at: None,
        }
    }

    pub fn pattern(&self) -> Option<&Pattern> {
        self.editor.library().patterns.get(self.pattern_index)
    }

    fn track(&self, t: usize) -> Option<&Track> {
        self.pattern()?.tracks.get(t)
    }

    pub fn status_lines(&self) -> usize {
        self.status.len()
    }

    pub fn playing(&self) -> bool {
        self.controller.as_ref().is_some_and(Controller::is_playing)
    }

    // ─── Transport ───────────────────────────────────────────────────────────

    /// Space: play or stop the selected pattern.
    pub fn toggle_play(&mut self) {
        self.toggle(Mode::Pattern(self.pattern_index));
    }

    /// Shift+Space: play or stop the selected song (its arrangement).
    pub fn toggle_play_song(&mut self) {
        if self.editor.library().songs.is_empty() {
            self.message = "no song in the library".into();
            return;
        }
        self.toggle(Mode::Song(self.song_index));
    }

    fn toggle(&mut self, mode: Mode) {
        let Some(controller) = &mut self.controller else { return };
        if controller.is_playing() {
            controller.stop();
        } else {
            if let Mode::Pattern(p) = mode {
                let rng = &mut self.rng;
                self.editor.auto_loop(p, 0, &self.scales, &mut || rng.next_f64());
            }
            controller.play(self.editor.library(), mode);
            self.played_revision = self.editor.revision();
        }
    }

    /// Shift+[ ]: previous / next song.
    pub fn step_song(&mut self, delta: isize) {
        let count = self.editor.library().songs.len();
        if count == 0 {
            self.message = "no song in the library".into();
            return;
        }
        self.song_index = (self.song_index as isize + delta).rem_euclid(count as isize) as usize;
        let song = &self.editor.library().songs[self.song_index];
        self.message = format!("song {} ({} measures)", song.name, song.length_measures());
    }

    /// Ctrl+E: the selected pattern (with Shift: the selected song) to a WAV
    /// file in the exports folder.
    pub fn export_wav(&mut self, song: bool) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        let Some(setup) = &self.render else {
            self.message = "nothing to render with (no samples loaded)".into();
            return;
        };
        let library = self.editor.library();
        let started = Instant::now();
        let (rendered, name) = if song {
            let Some(s) = library.songs.get(self.song_index) else {
                self.message = "no song in the library".into();
                return;
            };
            (offline::render_song(library, self.song_index, setup), format!("song-{}", s.id))
        } else {
            let Some(p) = library.patterns.get(self.pattern_index) else { return };
            // Tracks in auto mode get their new lines loop after loop, on the render's copy.
            let (scales, mut rng) = (&self.scales, Rng::new(setup.seed));
            let mut loop_start = |library: &mut Library, p: usize, number: u64| {
                if let Some(pattern) = library.patterns.get_mut(p).filter(|p| generate::plays_auto(p)) {
                    generate::run_loop(&mut AutoGen::new(scales, &mut || rng.next_f64()), pattern, number);
                }
            };
            (offline::render_pattern(library, self.pattern_index, 1, setup, Some(&mut loop_start)), p.id.clone())
        };
        let Some((left, right)) = rendered else {
            self.message = "nothing to export (no playable clip)".into();
            return;
        };
        let path = dir.join(format!("ordrumbox-{name}.wav"));
        let bytes = wav::encode(&left, &right, setup.sample_rate);
        let result = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, bytes));
        self.message = match result {
            Ok(()) => format!(
                "exported {} ({:.1} s of audio in {:.1} s)",
                path.display(),
                left.len() as f64 / f64::from(setup.sample_rate),
                started.elapsed().as_secs_f64()
            ),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// Ctrl+M: the selected pattern to a MIDI file in the exports folder.
    pub fn export_midi(&mut self) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        let Some(pattern) = self.pattern() else { return };
        let mut rng = Rng::new(1);
        let bytes = midi::export(pattern, 1, &self.roots, &mut rng, &self.scales);
        let path = dir.join(format!("ordrumbox-{}.mid", pattern.id));
        let result = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, &bytes));
        self.message = match result {
            Ok(()) => format!("exported {} ({} bytes)", path.display(), bytes.len()),
            Err(e) => format!("not exported: {}: {e}", path.display()),
        };
    }

    /// A MIDI file dropped on the window (or given on the command line):
    /// new patterns from it, the last one selected.
    pub fn import_midi(&mut self, path: &Path) {
        let name = path.file_name().map_or_else(|| "midi".into(), |n| n.to_string_lossy().into_owned());
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.message = format!("not imported: {}: {e}", path.display());
                return;
            }
        };
        match self.editor.import_midi(&name, &bytes, &self.roots) {
            Ok((index, message)) => {
                self.pattern_index = index;
                self.deselect();
                self.edited();
                self.message = message;
            }
            Err(reason) => self.message = format!("{name}: {reason}"),
        }
    }

    pub fn step_pattern(&mut self, delta: isize) {
        let count = self.editor.library().patterns.len();
        if count > 0 {
            self.pattern_index = (self.pattern_index as isize + delta).rem_euclid(count as isize) as usize;
            self.deselect();
            self.message = format!("pattern {}", self.pattern().and_then(|p| p.name.as_deref()).unwrap_or_default());
        }
    }

    pub fn step_kit(&mut self, delta: isize) {
        if self.kits.is_empty() {
            return;
        }
        self.kit_index = (self.kit_index as isize + delta).rem_euclid(self.kits.len() as isize) as usize;
        let name = &self.kits[self.kit_index].name;
        if let Some(controller) = &mut self.controller {
            controller.set_kit(name);
        }
        if let Some(render) = &mut self.render {
            render.kit.clone_from(name);
        }
        self.message = format!("kit {name}");
    }

    /// Keep the engine fed and the playing pass current; call every frame
    /// while playing. Before the next pass of a pattern is built, its tracks
    /// in auto mode get their new lines (the JS loop start).
    pub fn update(&mut self) {
        let Some(controller) = &mut self.controller else { return };
        controller.select_pattern(self.pattern_index);
        if self.editor.revision() != self.played_revision {
            controller.refresh(self.editor.library());
        }
        if let (Some(number), Mode::Pattern(p)) = (controller.pending_pass(), controller.mode()) {
            let rng = &mut self.rng;
            self.editor.auto_loop(p, number, &self.scales, &mut || rng.next_f64());
        }
        self.played_revision = self.editor.revision();
        controller.update(self.editor.library());
    }

    fn audition(&mut self, t: usize, note: Option<usize>) {
        if let Some(controller) = &mut self.controller {
            controller.preview(self.editor.library(), self.pattern_index, t, note);
        }
    }

    pub fn audition_row(&mut self, t: usize) {
        if self.track(t).is_some() {
            self.audition(t, None);
        }
    }

    // ─── Editing ─────────────────────────────────────────────────────────────

    /// After an edit: remember to save, show what happened.
    fn edited(&mut self) {
        self.changed_at = Some(Instant::now());
        if let Some(desc) = self.editor.history().next_undo() {
            self.message = desc.to_owned();
        }
    }

    /// Keep the pattern, cursor and selection inside the (changed) library.
    fn clamp_selection(&mut self) {
        let count = self.editor.library().patterns.len();
        self.pattern_index = self.pattern_index.min(count.saturating_sub(1));
        let tracks = self.pattern().map_or(0, |p| p.tracks.len());
        if let Some(cursor) = &mut self.cursor {
            if tracks == 0 {
                self.cursor = None;
            } else {
                cursor.track = cursor.track.min(tracks - 1);
            }
        }
        let valid = |&(t, n): &(usize, usize)| self.track(t).is_some_and(|t| n < t.notes.len());
        self.selected = self.selected.filter(valid);
    }

    pub fn undo(&mut self) {
        self.message = match self.editor.undo() {
            Some(desc) => format!("undone: {desc}"),
            None => "nothing to undo".into(),
        };
        self.after_history();
    }

    pub fn redo(&mut self) {
        self.message = match self.editor.redo() {
            Some(desc) => format!("redone: {desc}"),
            None => "nothing to redo".into(),
        };
        self.after_history();
    }

    fn after_history(&mut self) {
        self.selected = None;
        self.clamp_selection();
        self.changed_at = Some(Instant::now());
    }

    pub fn click(&mut self, hit: Hit) {
        match hit {
            Hit::Mute(t) => self.toggle_row(t, "mute"),
            Hit::Solo(t) => self.toggle_row(t, "solo"),
            Hit::Name(t) => {
                self.cursor = Some(Cursor { track: t, beat: self.cursor.map_or(0, |c| c.beat), step: 0 });
                self.selected = None;
                self.audition(t, None);
            }
            Hit::Cell { track, beat, step } => {
                self.cursor = Some(Cursor { track, beat, step });
                self.toggle_note(track, beat, step);
            }
        }
    }

    /// The JS grid click / Enter: add a note on an empty step, select the
    /// note of a step, delete the selected note.
    fn toggle_note(&mut self, t: usize, beat: i32, step: i32) {
        let p = self.pattern_index;
        let Some(track) = self.track(t) else { return };
        let at_step = track.notes.iter().position(|n| n.beat == beat && n.beat_step == step);
        match at_step {
            Some(n) if self.selected == Some((t, n)) => {
                let pitch = track.notes[n].pitch;
                self.editor.delete_note(p, t, beat, step, pitch);
                self.selected = None;
                self.edited();
            }
            Some(n) => {
                self.selected = Some((t, n));
                self.audition(t, Some(n));
            }
            None => {
                self.selected = self.editor.add_note(p, t, beat, step, 0.0).map(|n| (t, n));
                self.edited();
                self.audition(t, self.selected.map(|(_, n)| n));
            }
        }
    }

    pub fn enter(&mut self) {
        let Some(c) = self
            .cursor
            .or_else(|| self.pattern().filter(|p| !p.tracks.is_empty()).map(|_| Cursor { track: 0, beat: 0, step: 0 }))
        else {
            return;
        };
        self.cursor = Some(c);
        self.toggle_note(c.track, c.beat, c.step);
    }

    pub fn move_cursor(&mut self, direction: Move) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let tracks = pattern.tracks.len();
        if tracks == 0 {
            return;
        }
        let beats = pattern.beat_count.max(1);
        let Some(mut c) = self.cursor else {
            self.cursor = Some(Cursor { track: 0, beat: 0, step: 0 });
            return;
        };
        let spb = pattern.tracks[c.track.min(tracks - 1)].steps_per_beat.max(1);
        match direction {
            Move::Right => {
                c.step += 1;
                if c.step >= spb {
                    c.step = 0;
                    c.beat = (c.beat + 1) % beats;
                }
            }
            Move::Left => {
                c.step -= 1;
                if c.step < 0 {
                    c.step = spb - 1;
                    c.beat = (c.beat - 1).rem_euclid(beats);
                }
            }
            Move::Up => c.track = c.track.saturating_sub(1),
            Move::Down => c.track = (c.track + 1).min(tracks - 1),
        }
        let spb = pattern.tracks[c.track].steps_per_beat.max(1);
        c.step = c.step.min(spb - 1);
        self.cursor = Some(c);
        let note = pattern.tracks[c.track].notes.iter().position(|n| n.beat == c.beat && n.beat_step == c.step);
        self.selected = note.map(|n| (c.track, n));
        if let Some(n) = note {
            self.audition(c.track, Some(n));
        }
    }

    /// Shift+arrows on the selected note: ↑↓ velocity, ←→ pitch.
    pub fn nudge(&mut self, direction: Move) {
        let Some((t, n)) = self.selected else {
            self.message = "select a note first".into();
            return;
        };
        let nudge = match direction {
            Move::Up => Nudge::VelocityUp,
            Move::Down => Nudge::VelocityDown,
            Move::Right => Nudge::PitchUp,
            Move::Left => Nudge::PitchDown,
        };
        if self.editor.nudge_note(self.pattern_index, t, n, nudge) {
            self.edited();
            if let Some(note) = self.track(t).and_then(|track| track.notes.get(n)) {
                self.message = format!("velocity {}  pitch {}", note.velocity, note.pitch);
            }
            self.audition(t, Some(n));
        }
    }

    pub fn delete_step(&mut self) {
        let Some(c) = self.cursor else { return };
        if self.editor.delete_step(self.pattern_index, c.track, c.beat, c.step) {
            self.edited();
        }
        self.selected = None;
    }

    pub fn deselect(&mut self) {
        self.cursor = None;
        self.selected = None;
    }

    /// The track the cursor is on, else the selected note's.
    fn focus_track(&self) -> Option<usize> {
        self.cursor.map(|c| c.track).or(self.selected.map(|(t, _)| t)).filter(|&t| self.track(t).is_some())
    }

    pub fn toggle_row(&mut self, t: usize, key: &str) {
        if self.track(t).is_some() && self.editor.toggle_track(self.pattern_index, t, key) {
            self.edited();
        }
    }

    pub fn step_tempo(&mut self, delta: f64) {
        let Some(bpm) = self.pattern().map(|p| p.bpm) else { return };
        let bpm = (bpm + delta).round().clamp(20.0, 300.0);
        if self.editor.set_pattern_bpm(self.pattern_index, bpm, true) {
            self.edited();
        }
    }

    pub fn change_length(&mut self, delta: i32) {
        let Some(beats) = self.pattern().map(|p| p.beat_count) else { return };
        let beats = (beats + delta).clamp(1, ordrumbox_logic::editor::MAX_BEATS);
        if self.editor.set_pattern_beat_count(self.pattern_index, f64::from(beats)) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    fn clamp_cursor_to_grid(&mut self) {
        let Some(pattern) = self.editor.library().patterns.get(self.pattern_index) else { return };
        let beats = pattern.beat_count.max(1);
        if let Some(c) = &mut self.cursor {
            c.beat = c.beat.min(beats - 1);
            let spb = pattern.tracks.get(c.track).map_or(1, |t| t.steps_per_beat.max(1));
            c.step = c.step.min(spb - 1);
        }
        self.clamp_selection();
    }

    pub fn next_grid(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.incr_steps_per_beat(self.pattern_index, t) {
            self.edited();
            self.clamp_cursor_to_grid();
        }
    }

    pub fn loop_point_back(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.editor.decr_loop_point(self.pattern_index, t) {
            self.edited();
        }
    }

    /// A new track `T<n>` (the JS "add track" button).
    pub fn add_track(&mut self) {
        let Some(count) = self.pattern().map(|p| p.tracks.len()) else { return };
        if let Some(t) = self.editor.add_track(self.pattern_index, &format!("T{}", count + 1), 4) {
            self.cursor = Some(Cursor { track: t, beat: 0, step: 0 });
            self.selected = None;
            self.edited();
        }
    }

    /// Remove the focused track, never the last one.
    pub fn remove_track(&mut self) {
        let Some(t) = self.focus_track() else { return };
        if self.pattern().is_some_and(|p| p.tracks.len() > 1) && self.editor.remove_track(self.pattern_index, t) {
            self.selected = None;
            self.edited();
            self.clamp_selection();
        }
    }

    // ─── Generation ──────────────────────────────────────────────────────────

    /// B: a new pattern, generated (genre, tracks and lines).
    pub fn generate_pattern(&mut self) {
        let rng = &mut self.rng;
        self.pattern_index = self.editor.generate_pattern(&self.scales, &mut || rng.next_f64());
        self.deselect();
        self.edited();
        if let Some(pattern) = self.pattern() {
            let genre = pattern.extra.get("_autoGenGenre").and_then(|g| g.as_str()).unwrap_or_default();
            self.message = format!("generated {} ({genre})", pattern.name.as_deref().unwrap_or_default());
        }
    }

    /// D, S, A: ↻ Drum / Bass / Chords — new lines in auto mode, or auto mode off.
    pub fn toggle_generation(&mut self, part: Option<MelodicKind>) {
        let p = self.pattern_index;
        let rng = &mut self.rng;
        let rnd = &mut || rng.next_f64();
        let on = match part {
            None => self.editor.toggle_drums(p, &self.scales, rnd),
            Some(kind) => self.editor.toggle_melodic(p, kind, &self.scales, rnd),
        };
        let Some(on) = on else { return };
        self.clamp_selection();
        self.edited();
        let name = match part {
            None => "drums",
            Some(MelodicKind::Bass) => "bass",
            Some(MelodicKind::Chords) => "chords",
        };
        self.message = if on {
            let genre = self.pattern().and_then(|p| p.extra.get("_autoGenGenre")?.as_str()).unwrap_or_default();
            format!("{name}: generated, new lines every loop ({genre})")
        } else {
            format!("{name}: auto off")
        };
    }

    /// Z: auto mode of the focused track (a new line every loop).
    pub fn toggle_auto(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        if self.editor.toggle_track(self.pattern_index, t, "auto") {
            self.edited();
        }
    }

    /// X: random notes on the focused track.
    pub fn randomize_track(&mut self) {
        let Some(t) = self.focus_track() else {
            self.message = "select a track first".into();
            return;
        };
        let rng = &mut self.rng;
        if self.editor.randomize_track(self.pattern_index, t, &mut || rng.next_f64()) {
            self.selected = None;
            self.edited();
        }
    }

    pub fn new_pattern(&mut self) {
        self.pattern_index = self.editor.add_pattern(None);
        self.deselect();
        self.edited();
    }

    pub fn duplicate_pattern(&mut self) {
        if let Some(index) = self.editor.duplicate_pattern(self.pattern_index) {
            self.pattern_index = index;
            self.deselect();
            self.edited();
        }
    }

    /// Ctrl+C: the notes of the cursor step (with Shift, or no cursor: the track).
    pub fn copy(&mut self, track: bool) {
        let Some(t) = self.focus_track() else { return };
        let Some(source) = self.track(t).cloned() else { return };
        let name = source.name.clone();
        if track || self.cursor.is_none() {
            self.message = format!("copied track \"{name}\" ({} notes)", source.notes.len());
            self.clipboard = Some(Clipboard::Track(Box::new(source)));
        } else if let Some(c) = self.cursor {
            let notes: Vec<Note> =
                source.notes.iter().filter(|n| n.beat == c.beat && n.beat_step == c.step).cloned().collect();
            self.message = format!("copied {} note(s) — {name} @ {}.{}", notes.len(), c.beat + 1, c.step + 1);
            self.clipboard = Some(Clipboard::Step(notes));
        }
    }

    /// Ctrl+V: the copied step at the cursor, or the copied track after the
    /// focused one (with Shift, only a track).
    pub fn paste(&mut self, track_only: bool) {
        let p = self.pattern_index;
        match self.clipboard.clone() {
            None => self.message = "clipboard is empty".into(),
            Some(Clipboard::Track(track)) => {
                let after = self.focus_track().map_or_else(|| self.pattern().map_or(0, |p| p.tracks.len()), |t| t + 1);
                if let Some(t) = self.editor.paste_track(p, after, &track) {
                    self.cursor = Some(Cursor { track: t, beat: 0, step: 0 });
                    self.selected = None;
                    self.edited();
                }
            }
            Some(Clipboard::Step(_)) if track_only => self.message = "no track copied".into(),
            Some(Clipboard::Step(notes)) => {
                let c = self.cursor.unwrap_or(Cursor { track: 0, beat: 0, step: 0 });
                if self.editor.paste_step_notes(p, c.track, c.beat, c.step, &notes) {
                    self.cursor = Some(c);
                    self.selected = None;
                    self.edited();
                }
            }
        }
    }

    // ─── Saving ──────────────────────────────────────────────────────────────

    pub fn save(&mut self) {
        let Some(path) = &self.library_path else {
            self.message = "not saved: no library file (see the messages below)".into();
            return;
        };
        match store::save(path, self.editor.library()) {
            Ok(()) => {
                self.saved_revision = self.editor.revision();
                self.changed_at = None;
                self.message = format!("saved {}", path.display());
            }
            Err(e) => self.message = format!("not saved: {}: {e}", path.display()),
        }
    }

    pub fn save_if_changed(&mut self) {
        if self.library_path.is_some() && self.editor.revision() != self.saved_revision {
            self.save();
        }
    }

    /// Save once the edits have settled; when to call again, if a save is pending.
    pub fn autosave(&mut self) -> Option<Instant> {
        let changed = self.changed_at?;
        if self.library_path.is_none() || self.editor.revision() == self.saved_revision {
            self.changed_at = None;
            return None;
        }
        let due = changed + SAVE_DELAY;
        if Instant::now() >= due {
            self.save();
            return None;
        }
        Some(due)
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    pub fn draw(&self, canvas: &mut Canvas, scale: i32) {
        let position = self.controller.as_ref().and_then(Controller::position);
        let history = self.editor.history();
        let library = self.editor.library();
        let song_mode = self.controller.as_ref().and_then(|c| match c.mode() {
            Mode::Song(s) => library.songs.get(s),
            Mode::Pattern(_) => None,
        });
        let song = match (song_mode, position) {
            (Some(song), Some(p)) => {
                let total = song.length_measures().max(1.0);
                let measure = (p.pass as f64 + p.frame as f64 / p.frames.max(1) as f64) % total;
                Some(format!("SONG {}  measure {}/{}", song.name, measure.floor() + 1.0, total.ceil()))
            }
            _ => library.songs.get(self.song_index).map(|s| format!("song {}", s.name)),
        };
        let pattern = self.pattern();
        let generating = |kind: fn(TrackType) -> bool| {
            pattern
                .is_some_and(|p| p.tracks.iter().any(|t| generate::is_auto_on(t) && kind(detect_track_type(&t.name))))
        };
        let generation = [
            generating(TrackType::is_drum),
            generating(|k| k == TrackType::Bass),
            generating(|k| k == TrackType::Piano),
        ];
        let view = View {
            pattern,
            generation,
            genre: pattern.and_then(|p| p.extra.get("_autoGenGenre")?.as_str()),
            pattern_index: self.pattern_index,
            pattern_count: library.patterns.len(),
            kit: self.kits.get(self.kit_index).map_or("-", |k| k.name.as_str()),
            playing: self.playing(),
            playhead: position.filter(|_| song_mode.is_none()).map(|p| p.frame as f64 / p.frames.max(1) as f64),
            song: song.as_deref(),
            cursor: self.cursor,
            selected: self.selected,
            undo: history.next_undo(),
            redo: history.next_redo(),
            message: &self.message,
            status: &self.status,
        };
        view::draw(canvas, &view, scale);
    }
}
