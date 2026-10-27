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
use ordrumbox_engine::analysis::{self, Analysis};
use ordrumbox_engine::offline::{self, RenderSetup};
use ordrumbox_engine::{Controller, Mode, SynthPresets, wav};
use ordrumbox_logic::generate::{self, AutoGen, MelodicKind};
use ordrumbox_logic::midi;
use ordrumbox_logic::{Editor, Nudge};

use crate::assets;
use crate::audio;
use crate::canvas::Canvas;
use crate::kits::{Field, KitSet};
use crate::menu::{self, Command, MenuState};
use crate::picker::{self, Outcome, Picker, PickerLayout, Purpose};
use crate::store;
use crate::view::{self, Cursor, Hit, KitHit, KitView, View};

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

/// Keys a panel over the window reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayKey {
    Escape,
    Enter,
    /// Backspace: the parent folder.
    Back,
    Move(isize),
    /// Home (false) / End (true).
    Edge(bool),
    Letter(char),
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
    kits: KitSet,
    kit_index: usize,
    /// The kit editor, when open: selected sample of the current kit and field.
    kit_editor: Option<(usize, Field)>,
    /// Analysis of the sample the kit editor shows, by url.
    analysis: Option<(String, Analysis)>,
    synths: SynthPresets,
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
    /// The open menu, if any.
    menu: Option<menu::Open>,
    /// A panel over everything: the file picker, the key list, about.
    overlay: Option<Overlay>,
    /// Folder the file picker last picked in.
    last_dir: Option<PathBuf>,
    /// The assets folder in use.
    assets_dir: Option<PathBuf>,
    /// File > Quit was chosen.
    quit: bool,
}

/// What is shown over the window.
#[derive(Debug)]
enum Overlay {
    Picker(Picker),
    Shortcuts,
    About,
}

/// Every key, for Help > Keyboard shortcuts.
const SHORTCUTS: [&str; 30] = [
    "MENUS",
    "  F10 or Alt             open the menus: Arrows move, Enter runs, Esc closes",
    "  F1                     this list",
    "GRID",
    "  Arrows                 move the cursor",
    "  Enter / click          add a note, select it, delete the selected one",
    "  Shift+Arrows           velocity (up/down), pitch (left/right) of the selected note",
    "  Delete                 clear the cursor step        Esc  deselect",
    "  1-9 / Shift+1-9        mute / solo track 1-9        Q-P  hear track 1-10",
    "PLAY",
    "  Space / Shift+Space    play or stop the pattern / the song",
    "  [ ] / Shift+[ ]        previous / next pattern / song (also PageUp PageDown)",
    "  - = (Shift: x10)       tempo",
    "  , .                    previous / next kit",
    "FILE AND EDIT",
    "  Ctrl+S, Ctrl+O, Ctrl+Q save, import a MIDI file, quit",
    "  Ctrl+E, Ctrl+M         export WAV (Shift: the song), export MIDI",
    "  Ctrl+Z, Ctrl+Y         undo, redo (also Ctrl+Shift+Z)",
    "  Ctrl+C, Ctrl+V         copy, paste the step (Shift: the track)",
    "  Ctrl+N, Ctrl+D         new pattern, duplicate pattern",
    "  Ctrl+T, Ctrl+Backspace add, remove a track",
    "  Ctrl+G, Ctrl+L         next grid, loop point one step back",
    "  Ctrl+- Ctrl+=          pattern length",
    "GENERATE",
    "  B                      generate a pattern",
    "  D, S, A                drums, bass, chords in auto mode (again: off)",
    "  Z, X                   track in auto mode, random notes on the track",
    "KIT EDITOR (Ctrl+K)",
    "  Up Down, Left Right    sample, field     - = (Shift: finely)  change    Enter  hear",
    "  Delete, N, R, E        remove, normalize, assign again, export mapping   (drop files to add)",
];

impl Session {
    pub fn load() -> Self {
        let Some(dir) = assets::find() else {
            let searched: Vec<String> = assets::candidates().iter().map(|d| d.display().to_string()).collect();
            return Self::new(
                Library::default(),
                KitSet::empty(),
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
            Err(e) => return Self::new(Library::default(), KitSet::empty(), vec![e]),
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

        if let Some(e) = &assets.kits_error {
            status.push(format!("kits.json not loaded, kit edits not saved: {e}"));
        }
        let kits_file = if assets.kits_error.is_none() { store::kits_path() } else { None };
        let kits =
            KitSet::new(assets.kits, &assets.bank, Some(assets.sources.clone()), kits_file, store::samples_dir());
        let mut session = Self::new(library, kits, status);
        session.assets_dir = Some(dir.clone());
        session.scales = assets.scales.clone();
        session.synths = assets.synths.clone();
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
                kit: session.kits.kits.first().map(|k| k.name.clone()).unwrap_or_default(),
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

    fn new(library: Library, kits: KitSet, status: Vec<String>) -> Self {
        Self {
            editor: Editor::new(library),
            scales: Scales::default(),
            rng: Rng::from_time(),
            roots: Vec::new(),
            kits,
            kit_index: 0,
            kit_editor: None,
            analysis: None,
            synths: SynthPresets::default(),
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
            menu: None,
            overlay: None,
            last_dir: None,
            assets_dir: None,
            quit: false,
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
        let count = self.kits.kits.len();
        if count == 0 {
            return;
        }
        self.select_kit((self.kit_index as isize + delta).rem_euclid(count as isize) as usize);
        self.message = format!("kit {}", self.kits.kits[self.kit_index].name);
    }

    /// Use kit `index` (its sounds are assigned again).
    fn select_kit(&mut self, index: usize) {
        let Some(kit) = self.kits.kits.get(index) else { return };
        self.kit_index = index;
        if let Some(controller) = &mut self.controller {
            controller.set_kit(&kit.name);
        }
        if let Some(render) = &mut self.render {
            render.kit.clone_from(&kit.name);
        }
        if let Some((sample, field)) = self.kit_editor {
            self.kit_editor = Some((sample.min(kit.instruments.len().saturating_sub(1)), field));
        }
        self.update_analysis();
    }

    // ─── Menus and commands ──────────────────────────────────────────────────

    /// What the menus show now.
    fn menu_state(&self) -> MenuState {
        let history = self.editor.history();
        let pattern = self.pattern();
        let generating = |kind: fn(TrackType) -> bool| {
            pattern
                .is_some_and(|p| p.tracks.iter().any(|t| generate::is_auto_on(t) && kind(detect_track_type(&t.name))))
        };
        let focus = self.focus_track().and_then(|t| self.track(t)).map(|t| (t.auto, t.mute, t.solo));
        MenuState {
            can_save: self.library_path.is_some(),
            can_render: self.render.is_some(),
            has_pattern: pattern.is_some(),
            undo: history.next_undo().map(str::to_owned),
            redo: history.next_redo().map(str::to_owned),
            has_cursor: self.cursor.is_some(),
            focus_track: focus,
            can_remove_track: focus.is_some() && pattern.is_some_and(|p| p.tracks.len() > 1),
            clipboard_step: matches!(self.clipboard, Some(Clipboard::Step(_))),
            clipboard_track: matches!(self.clipboard, Some(Clipboard::Track(_))),
            has_selection: self.selected.is_some(),
            playing: self.playing(),
            has_songs: !self.editor.library().songs.is_empty(),
            generation: [
                generating(TrackType::is_drum),
                generating(|k| k == TrackType::Bass),
                generating(|k| k == TrackType::Piano),
            ],
            kits: self.kits.kits.iter().map(|k| k.name.clone()).collect(),
            kit_index: self.kit_index,
            kit_editor: self.kit_editor.is_some(),
            kit_sample: self.kit_list().is_some_and(|(s, n)| s < n),
        }
    }

    pub fn menus(&self) -> Vec<menu::Menu> {
        menu::menus(&self.menu_state())
    }

    /// The open menu, if any.
    pub fn open_menu(&self) -> Option<menu::Open> {
        self.menu
    }

    pub fn set_menu(&mut self, open: Option<menu::Open>) {
        self.menu = open;
    }

    /// File > Quit was chosen.
    pub fn quit_requested(&self) -> bool {
        self.quit
    }

    /// Run a menu command (or its shortcut).
    pub fn run(&mut self, command: Command) {
        use Command as C;
        self.menu = None;
        match command {
            C::Save => self.save(),
            C::ImportMidi => self.open_picker(Purpose::ImportMidi),
            C::ExportWav => self.export_wav(false),
            C::ExportSongWav => self.export_wav(true),
            C::ExportMidi => self.export_midi(),
            C::Quit => self.quit = true,
            C::Undo => self.undo(),
            C::Redo => self.redo(),
            C::CopyStep => self.copy(false),
            C::CopyTrack => self.copy(true),
            C::Paste => self.paste(false),
            C::PasteTrack => self.paste(true),
            C::ClearStep => self.delete_step(),
            C::Deselect => self.deselect(),
            C::NewPattern => self.new_pattern(),
            C::DuplicatePattern => self.duplicate_pattern(),
            C::PreviousPattern => self.step_pattern(-1),
            C::NextPattern => self.step_pattern(1),
            C::TempoDown => self.step_tempo(-1.0),
            C::TempoUp => self.step_tempo(1.0),
            C::LengthDown => self.change_length(-1),
            C::LengthUp => self.change_length(1),
            C::AddTrack => self.add_track(),
            C::RemoveTrack => self.remove_track(),
            C::NextGrid => self.next_grid(),
            C::LoopBack => self.loop_point_back(),
            C::GeneratePattern => self.generate_pattern(),
            C::GenerateDrums => self.toggle_generation(None),
            C::GenerateBass => self.toggle_generation(Some(MelodicKind::Bass)),
            C::GenerateChords => self.toggle_generation(Some(MelodicKind::Chords)),
            C::TrackAuto => self.toggle_auto(),
            C::RandomTrack => self.randomize_track(),
            C::PlayPattern => self.toggle_play(),
            C::PlaySong => self.toggle_play_song(),
            C::PreviousSong => self.step_song(-1),
            C::NextSong => self.step_song(1),
            C::Mute | C::Solo => {
                if let Some(t) = self.focus_track() {
                    self.toggle_row(t, if command == C::Mute { "mute" } else { "solo" });
                }
            }
            C::Kit(i) => {
                self.select_kit(i);
                if let Some(kit) = self.kits.kits.get(i) {
                    self.message = format!("kit {}", kit.name);
                }
            }
            C::PreviousKit => self.step_kit(-1),
            C::NextKit => self.step_kit(1),
            C::KitEditor => self.toggle_kit_editor(),
            C::NormalizeKit => self.kit_normalize(),
            C::ExportMapping => self.kit_export(),
            C::LoadMapping => self.open_picker(Purpose::LoadMapping),
            C::AddSample => self.open_picker(Purpose::AddSample),
            C::ReplaceSample => self.open_picker(Purpose::ReplaceSample),
            C::ImportFolder => self.open_picker(Purpose::ImportFolder),
            C::Reassign => self.kit_reassign(),
            C::Shortcuts => self.overlay = Some(Overlay::Shortcuts),
            C::About => self.overlay = Some(Overlay::About),
        }
    }

    fn open_picker(&mut self, purpose: Purpose) {
        self.overlay = Some(Overlay::Picker(Picker::new(purpose, self.last_dir.as_deref())));
    }

    /// A panel (file picker, key list, about) is over the window.
    pub fn overlay_open(&self) -> bool {
        self.overlay.is_some()
    }

    pub fn close_overlay(&mut self) {
        self.overlay = None;
    }

    /// A key while a panel is open: the file picker's keys, or any key closes it.
    pub fn overlay_key(&mut self, key: OverlayKey) {
        let Some(Overlay::Picker(picker)) = &mut self.overlay else {
            if matches!(key, OverlayKey::Escape | OverlayKey::Enter) {
                self.overlay = None;
            }
            return;
        };
        let outcome = match key {
            OverlayKey::Escape => {
                self.overlay = None;
                return;
            }
            OverlayKey::Enter => picker.activate(),
            OverlayKey::Back => {
                picker.up();
                Outcome::Browsing
            }
            OverlayKey::Move(delta) => {
                picker.step(delta);
                Outcome::Browsing
            }
            OverlayKey::Edge(end) => {
                picker.home_end(end);
                Outcome::Browsing
            }
            OverlayKey::Letter(c) => {
                picker.jump(c);
                Outcome::Browsing
            }
        };
        self.picker_outcome(outcome);
    }

    /// A click while a panel is open.
    pub fn overlay_click(&mut self, width: i32, height: i32, scale: i32, x: i32, y: i32) {
        let Some(Overlay::Picker(picker)) = &mut self.overlay else {
            self.overlay = None;
            return;
        };
        let layout = PickerLayout::new(width, height, scale);
        if !layout.contains(x, y) {
            self.overlay = None;
            return;
        }
        if let Some(i) = layout.hit(picker, x, y) {
            let outcome = picker.click(i);
            self.picker_outcome(outcome);
        }
    }

    fn picker_outcome(&mut self, outcome: Outcome) {
        let Outcome::Picked(path) = outcome else { return };
        let Some(Overlay::Picker(picker)) = self.overlay.take() else { return };
        self.last_dir = picker.dir.clone();
        match picker.purpose {
            Purpose::ImportMidi => self.import_midi(&path),
            Purpose::ReplaceSample => self.drop_path(&path, true),
            Purpose::AddSample | Purpose::LoadMapping | Purpose::ImportFolder => self.drop_path(&path, false),
        }
    }

    /// Lines of Help > About.
    fn about(&self) -> Vec<String> {
        let shown = |p: Option<PathBuf>| p.map_or_else(|| "-".to_owned(), |p| p.display().to_string());
        vec![
            format!("orDrumbox {} - native port of ordrumbox-v2", env!("CARGO_PKG_VERSION")),
            "GPL-3.0-only - https://www.ordrumbox.com".into(),
            String::new(),
            format!("library   {}", shown(self.library_path.clone())),
            format!("user data {}", shown(store::user_dir())),
            format!("exports   {}", shown(store::exports_dir())),
            format!("assets    {}", shown(self.assets_dir.clone())),
        ]
    }

    // ─── Kit editor ──────────────────────────────────────────────────────────

    pub fn kit_editor_open(&self) -> bool {
        self.kit_editor.is_some()
    }

    /// The kit editor's selected sample and the kit's sample count, when open.
    pub fn kit_list(&self) -> Option<(usize, usize)> {
        let (sample, _) = self.kit_editor?;
        Some((sample, self.kits.kits.get(self.kit_index).map_or(0, |k| k.instruments.len())))
    }

    /// Ctrl+K: the kit editor (the drumkit manager), open or closed.
    pub fn toggle_kit_editor(&mut self) {
        self.kit_editor = match self.kit_editor {
            Some(_) => None,
            None => Some((0, Field::Instrument)),
        };
        self.update_analysis();
    }

    /// The analysis of the sample shown, computed when it changes.
    fn update_analysis(&mut self) {
        let url = self.kit_editor.and_then(|(s, _)| self.kits.sample(self.kit_index, s)).map(|i| i.url.clone());
        match url {
            Some(url) if self.analysis.as_ref().is_some_and(|(u, _)| *u == url) => {}
            Some(url) => {
                self.analysis = self.kits.buffer(&url).map(|b| (url.clone(), analysis::analyze(b)));
            }
            None => self.analysis = None,
        }
    }

    /// The kits changed: a new bank for playback and exports, the root
    /// notes, the kits saved, the selection kept inside.
    fn kits_changed(&mut self, message: String) {
        let (bank, errors) = self.kits.bank();
        self.roots =
            bank.samples.iter().filter_map(|s| Some((s.instrument.key.clone(), s.instrument.root_midi?))).collect();
        let bank = Arc::new(bank);
        if let Some(controller) = &mut self.controller {
            controller.set_bank(bank.clone());
            controller.refresh(self.editor.library());
        }
        let kit = self.kits.kits.get(self.kit_index).map(|k| k.name.clone()).unwrap_or_default();
        self.render = (!bank.samples.is_empty()).then(|| RenderSetup {
            bank: bank.clone(),
            scales: self.scales.clone(),
            synths: self.synths.clone(),
            kit,
            sample_rate: offline::EXPORT_RATE,
            seed: 1,
        });
        self.message = match (self.kits.save(), errors.first()) {
            (Err(e), _) => e,
            (Ok(()), Some(first)) => format!("{message} ({} sample(s) not loaded, first: {first})", errors.len()),
            (Ok(()), None) => message,
        };
        self.kit_index = self.kit_index.min(self.kits.kits.len().saturating_sub(1));
        self.analysis = None;
        self.select_kit(self.kit_index);
    }

    /// ↑↓: the previous / next sample of the kit.
    pub fn kit_select(&mut self, delta: isize) {
        let Some((sample, field)) = self.kit_editor else { return };
        let count = self.kits.kits.get(self.kit_index).map_or(0, |k| k.instruments.len());
        if count > 0 {
            self.kit_editor = Some(((sample as isize + delta).rem_euclid(count as isize) as usize, field));
            self.update_analysis();
        }
    }

    /// ←→: the previous / next field.
    pub fn kit_field(&mut self, delta: isize) {
        let Some((sample, field)) = self.kit_editor else { return };
        let at = Field::ALL.iter().position(|f| *f == field).unwrap_or(0) as isize;
        // No wrap-around: a step past Decay must not land on Kit (where - = move the sample).
        let at = (at + delta).clamp(0, Field::ALL.len() as isize - 1) as usize;
        self.kit_editor = Some((sample, Field::ALL[at]));
    }

    /// A click in the kit editor.
    pub fn kit_click(&mut self, hit: KitHit) {
        let Some((sample, field)) = self.kit_editor else { return };
        self.kit_editor = Some(match hit {
            KitHit::Sample(s) => (s, field),
            KitHit::Field(f) => (sample, f),
        });
        self.update_analysis();
        if let KitHit::Sample(_) = hit {
            self.kit_audition();
        }
    }

    /// - =: the selected field down / up (Shift: finely).
    pub fn kit_adjust(&mut self, steps: i32, fine: bool) {
        let Some((sample, field)) = self.kit_editor else { return };
        if let Some(message) = self.kits.adjust(self.kit_index, sample, field, steps, fine) {
            self.kits_changed(message);
        }
    }

    /// Enter: the selected sample, as it is.
    pub fn kit_audition(&mut self) {
        let Some((sample, _)) = self.kit_editor else { return };
        let Some(url) = self.kits.sample(self.kit_index, sample).map(|i| i.url.clone()) else { return };
        if let Some(controller) = &mut self.controller
            && let Some(index) = controller.bank().find_url(&url)
        {
            controller.audition_sample(index);
        }
    }

    /// Delete: the selected sample out of the kit.
    pub fn kit_remove(&mut self) {
        let Some((sample, _)) = self.kit_editor else { return };
        if let Some(message) = self.kits.remove(self.kit_index, sample) {
            self.kits_changed(message);
        }
    }

    /// N: every sample of the kit to a 0 dB peak.
    pub fn kit_normalize(&mut self) {
        match self.kits.normalize(self.kit_index) {
            Ok(0) => self.message = "No samples to normalize".into(),
            Ok(n) => self.kits_changed(format!("Normalized {n} sample(s)")),
            Err(e) => self.message = e,
        }
    }

    /// R: the sounds of the tracks assigned again (`autoDetectAll`).
    pub fn kit_reassign(&mut self) {
        self.select_kit(self.kit_index);
        self.message = "Auto-detect complete".into();
    }

    /// E: the kit's mapping to a JSON file in the exports folder.
    pub fn kit_export(&mut self) {
        let Some(dir) = store::exports_dir() else {
            self.message = "no user data folder to export to".into();
            return;
        };
        self.message = self.kits.export_mapping(self.kit_index, &dir).unwrap_or_else(|e| e);
    }

    /// A file or folder dropped on the window (Shift: a WAV replaces the
    /// selected sample of the kit editor).
    pub fn drop_path(&mut self, path: &Path, shift: bool) {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        let result = if path.is_dir() {
            self.kits.import_folder(path).map(|(kit, message)| (kit, 0, message))
        } else {
            match ext.as_str() {
                "mid" | "midi" => return self.import_midi(path),
                "json" => self.kits.load_mapping(path).map(|(kit, message)| (kit, 0, message)),
                "wav" => match self.kit_editor {
                    Some((sample, _)) if shift => self
                        .kits
                        .replace_sample(self.kit_index, sample, path)
                        .map(|message| (self.kit_index, sample, message)),
                    _ => self
                        .kits
                        .add_sample(self.kit_index, path)
                        .map(|(sample, message)| (self.kit_index, sample, message)),
                },
                _ => Err(format!("{}: not a MIDI, WAV or kit file", path.display())),
            }
        };
        match result {
            Ok((kit, sample, message)) => {
                self.kit_index = kit;
                self.kit_editor = Some((sample, self.kit_editor.map_or(Field::Instrument, |(_, f)| f)));
                self.kits_changed(message);
            }
            Err(e) => self.message = e,
        }
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
            kit: self.kits.kits.get(self.kit_index).map_or("-", |k| k.name.as_str()),
            kit_view: self.kit_view(),
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
        menu::draw(canvas, &self.menus(), self.menu, scale);
        match &self.overlay {
            Some(Overlay::Picker(p)) => {
                let layout = PickerLayout::new(canvas.width(), canvas.height(), scale);
                picker::draw(canvas, p, &layout);
            }
            Some(Overlay::Shortcuts) => {
                let lines: Vec<String> = SHORTCUTS.iter().map(|l| (*l).to_owned()).collect();
                view::draw_notice(canvas, "Keyboard shortcuts", &lines, scale);
            }
            Some(Overlay::About) => view::draw_notice(canvas, "About orDrumbox", &self.about(), scale),
            None => {}
        }
    }

    /// What the kit editor shows.
    fn kit_view(&self) -> Option<KitView<'_>> {
        let (sample, field) = self.kit_editor?;
        let kit = self.kits.kits.get(self.kit_index)?;
        let samples = kit.instruments.iter().map(|i| (i.display_name.as_str(), i.key.as_str())).collect();
        let selected = kit.instruments.get(sample);
        let analysis =
            self.analysis.as_ref().filter(|(url, _)| selected.is_some_and(|i| i.url == *url)).map(|(_, a)| a);
        let values = selected.map(|i| {
            let decay = i.decay.map_or_else(|| format!("{} ms (default)", i.decay_ms()), |d| format!("{d} ms"));
            [kit.name.clone(), i.key.clone(), format!("{:+.1} dB", i.gain_db), format!("{:+.1} st", i.tune), decay]
        });
        let info = analysis.map(|a| {
            let db = |v: f64| if v.is_finite() { format!("{v:.1}") } else { "-".into() };
            let note = a.note.map_or_else(|| "-".to_owned(), |n| n.to_string());
            format!("{note}  {:.0} ms  {} dB peak  {} dB RMS", a.duration_sec * 1000.0, db(a.peak_db), db(a.rms_db))
        });
        let detected = selected.map(|i| {
            let name = if i.display_name.is_empty() { &i.url } else { &i.display_name };
            ordrumbox_core::instruments::catalog()
                .from_file_name(name)
                .map_or_else(|| "No instrument detected".to_owned(), |d| format!("Detected: {}", d.id))
        });
        let decay_marker = match (selected, analysis) {
            (Some(i), Some(a)) if a.duration_sec > 0.0 => Some((i.decay_ms() / 1000.0 / a.duration_sec).min(1.0)),
            _ => None,
        };
        Some(KitView {
            kit: &kit.name,
            samples,
            selected: sample,
            field,
            values,
            info,
            detected,
            envelope: analysis.map_or(&[][..], |a| &a.envelope),
            decay_marker,
        })
    }
}
