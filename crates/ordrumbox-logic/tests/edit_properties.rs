//! Properties of the editing commands, on random command sequences
//! (seeded): every edit is one undo step, none when it changes nothing;
//! each undo gives back the library as it was before that step, each redo
//! as it was after it; undoing everything gives back the start, redoing
//! everything the end. Compacting tracks keeps what plays.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::collections::BTreeSet;

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::style::{COMPLEXITY, ENERGY, VARIATION};
use ordrumbox_core::{Library, Note, Pattern, Rng, Scales, SongClip, Styles, Track, expand_hits};
use ordrumbox_logic::Editor;
use ordrumbox_logic::editor::Nudge;
use ordrumbox_logic::generate::MelodicKind;

/// Random sequences tried.
const SEEDS: u64 = 120;
/// Commands per sequence, under the history's 50 steps so that undoing
/// everything reaches the start.
const COMMANDS: usize = 45;
/// Clock advance between two edits: inside the coalescing window (400 ms)
/// or past it.
const CLOSE_MS: f64 = 100.0;
const APART_MS: f64 = 1000.0;

fn library() -> Library {
    Library::parse(include_str!("../../ordrumbox-core/tests/fixtures/song.json")).expect("song.json parses").0
}

fn styles() -> Styles {
    Styles::from_json(&json::parse(include_str!("../../../assets/data/styles.json")).expect("styles.json parses")).0
}

/// Every command the sequences run that can change the library.
const COMMAND_NAMES: [&str; 52] = [
    "add_note",
    "add_pattern",
    "add_pattern_at_measure",
    "add_song",
    "add_song_clip",
    "add_track",
    "batch",
    "change_track_name",
    "change_track_sound",
    "clean",
    "compact_track",
    "delete_note",
    "delete_step",
    "drag",
    "duplicate_pattern",
    "generate_pattern",
    "incr_steps_per_beat",
    "load_library",
    "move_pattern",
    "move_song_clip",
    "move_track",
    "nudge_note",
    "paste_step_notes",
    "paste_track",
    "randomize_track",
    "regenerate_pattern",
    "regenerate_track",
    "remove_pattern",
    "remove_pattern_at_measure",
    "remove_pattern_clips",
    "remove_song",
    "remove_song_clips",
    "remove_track",
    "rename_pattern",
    "rename_song",
    "repeat_pattern_at_measure",
    "set_pattern_beat_count",
    "set_pattern_bpm",
    "set_pattern_description",
    "set_pattern_macro",
    "set_pattern_tags",
    "set_song_settings",
    "set_steps_per_beat",
    "shorten_loop",
    "toggle_accent",
    "toggle_drums",
    "toggle_melodic",
    "toggle_pattern_auto",
    "toggle_track",
    "update_note",
    "update_track",
    "update_tracks",
];

const TRACK_NAMES: [&str; 6] = ["KICK", "SNARE", "CHH", "BASS", "PIANO", "Perc"];
const NAMES: [&str; 5] = ["  Rock II ", "", "   ", "verse", "Untitled"];

/// An editor driven by random commands with random arguments, some of
/// them out of range.
struct Fuzz {
    editor: Editor,
    /// Picks the commands and their arguments.
    rng: Rng,
    /// What the generators draw.
    gen_rng: Rng,
    scales: Scales,
    styles: Styles,
    clock: f64,
}

impl Fuzz {
    fn new(seed: u64, library: Library, styles: Styles) -> Self {
        let mut editor = Editor::new(library);
        editor.set_clock(Some(0.0));
        Self {
            editor,
            rng: Rng::new(seed),
            gen_rng: Rng::new(seed ^ 0x5eed),
            scales: Scales::default(),
            styles,
            clock: 0.0,
        }
    }

    fn below(&mut self, n: usize) -> usize {
        self.rng.below(n.max(1) as u64) as usize
    }

    fn coin(&mut self) -> bool {
        self.rng.chance(0.5)
    }

    fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        *self.rng.pick(items).expect("not empty")
    }

    /// An index into a list of `len`, now and then one past it or far out.
    fn index(&mut self, len: usize) -> usize {
        match self.below(20) {
            0 => len,
            1 => 99,
            _ => self.below(len),
        }
    }

    fn library(&self) -> &Library {
        self.editor.library()
    }

    fn pattern(&mut self) -> usize {
        self.index(self.library().patterns.len())
    }

    fn track(&mut self, p: usize) -> usize {
        let len = self.library().patterns.get(p).map_or(0, |pattern| pattern.tracks.len());
        self.index(len)
    }

    fn note(&mut self, p: usize, t: usize) -> usize {
        let len = self.notes(p, t).len();
        self.index(len)
    }

    fn notes(&self, p: usize, t: usize) -> &[Note] {
        self.library().patterns.get(p).and_then(|pattern| pattern.tracks.get(t)).map_or(&[], |track| &track.notes)
    }

    fn song(&mut self) -> Option<usize> {
        if self.coin() { None } else { Some(self.index(self.library().songs.len())) }
    }

    /// A pattern reference: an id, a name in another case with spaces, or
    /// one no pattern has.
    fn reference(&mut self) -> String {
        let p = self.below(self.library().patterns.len());
        let pattern = &self.library().patterns[p];
        let (id, name) = (pattern.id.clone(), pattern.name.clone().unwrap_or_default());
        match self.below(4) {
            0 => id,
            1 => format!("  {} ", name.to_uppercase()),
            2 => "no such pattern".to_owned(),
            _ => String::new(),
        }
    }

    fn measure(&mut self) -> u32 {
        if self.below(10) == 0 { 999 } else { self.below(12) as u32 }
    }

    fn number(&mut self, min: f64, max: f64) -> f64 {
        min + self.rng.next_f64() * (max - min)
    }

    /// One random command (in a batch: no batch nor library load).
    fn command(&mut self, in_batch: bool) -> Outcome {
        let kind = self.below(if in_batch { 54 } else { 56 });
        let p = self.pattern();
        let t = self.track(p);
        match kind {
            0 | 1 => {
                let (beat, step, pitch) =
                    (self.below(5) as i32, self.below(4) as i32, self.pick(&[0.0, 0.0, 3.0, -5.0]));
                self.editor.add_note(p, t, beat, step, pitch);
                Outcome::new("add_note", false)
            }
            2 => {
                let n = self.note(p, t);
                let (beat, step, pitch) =
                    self.notes(p, t).get(n).map_or((1, 1, 0.0), |note| (note.beat, note.step, note.pitch));
                self.editor.delete_note(p, t, beat, step, pitch);
                Outcome::new("delete_note", false)
            }
            3 => {
                let (beat, step) = (self.below(5) as i32, self.below(4) as i32);
                self.editor.delete_step(p, t, beat, step);
                Outcome::new("delete_step", false)
            }
            4 | 5 => {
                let n = self.note(p, t);
                let key = self.pick(&["velocity", "pitch", "probability"]);
                let value = Value::Number((self.number(0.0, 1.0) * 10.0).round() / 10.0);
                let coalesce = self.coin();
                self.editor.update_note(p, t, n, &[(key, value)], None, coalesce);
                Outcome::new("update_note", coalesce)
            }
            6 => {
                let n = self.note(p, t);
                let (beat, step) = self.notes(p, t).get(n).map_or((0, 0), |note| (note.beat, note.step));
                self.editor.toggle_accent(p, t, beat, step);
                Outcome::new("toggle_accent", false)
            }
            7 => {
                let n = self.note(p, t);
                let nudge = self.pick(&[Nudge::VelocityUp, Nudge::VelocityDown, Nudge::PitchUp, Nudge::PitchDown]);
                self.editor.nudge_note(p, t, n, nudge);
                Outcome::new("nudge_note", true)
            }
            8 => {
                let source = self.track(p);
                let notes = self.notes(p, source).to_vec();
                let take = self.below(3).min(notes.len());
                let (beat, step) = (self.below(5) as i32, self.below(4) as i32);
                self.editor.paste_step_notes(p, t, beat, step, &notes[..take]);
                Outcome::new("paste_step_notes", false)
            }
            9 => {
                let (name, steps_per_beat) = (self.pick(&TRACK_NAMES), 1 + self.below(4) as i32);
                self.editor.add_track(p, name, steps_per_beat);
                Outcome::new("add_track", false)
            }
            10 => {
                self.editor.remove_track(p, t);
                Outcome::new("remove_track", false)
            }
            11 => {
                let to = self.track(p);
                self.editor.move_track(p, t, to);
                Outcome::new("move_track", false)
            }
            12 => {
                let source_pattern = self.below(self.library().patterns.len());
                let source = self.library().patterns[source_pattern].tracks.first().cloned();
                let index = self.track(p);
                if let Some(source) = source {
                    self.editor.paste_track(p, index, &source);
                }
                Outcome::new("paste_track", false)
            }
            13 | 14 => {
                let (key, min, max) = self.pick(&[
                    ("velocity", -1.0, 3.0),
                    ("pan", -2.0, 2.0),
                    ("filterCutoff", 0.0, 50_000.0),
                    ("pitch", -30.0, 30.0),
                    ("noSuchKey", 0.0, 1.0),
                ]);
                let value = Value::Number(self.number(min, max).round());
                let coalesce = self.coin();
                self.editor.update_track(p, t, &[(key, value)], None, coalesce);
                Outcome::new("update_track", coalesce)
            }
            15 => {
                let pan = self.number(-1.0, 1.0).round();
                self.editor.update_tracks(p, "Spread", |t, _| {
                    vec![("pan", Value::Number(if t % 2 == 0 { pan } else { -pan }))]
                });
                Outcome::new("update_tracks", false)
            }
            16 => {
                let key = self.pick(&["mute", "solo", "mono"]);
                self.editor.toggle_track(p, t, key);
                Outcome::new("toggle_track", false)
            }
            17 => {
                let (value, coalesce) = (self.number(0.0, 10.0).round(), self.coin());
                self.editor.set_steps_per_beat(p, t, value, coalesce);
                Outcome::new("set_steps_per_beat", coalesce)
            }
            18 => {
                self.editor.incr_steps_per_beat(p, t);
                Outcome::new("incr_steps_per_beat", false)
            }
            19 => {
                self.editor.shorten_loop(p, t);
                Outcome::new("shorten_loop", false)
            }
            20 => {
                self.editor.compact_track(p, t);
                Outcome::new("compact_track", false)
            }
            21 => {
                if self.coin() {
                    self.editor.clean_track(p, t);
                } else {
                    self.editor.clean_pattern(p);
                }
                Outcome::new("clean", false)
            }
            22 => {
                let sample = self.pick(&["kick_01", "snare_02", ""]);
                self.editor.change_track_sound(p, t, sample);
                Outcome::new("change_track_sound", false)
            }
            23 => {
                let name = self.pick(&TRACK_NAMES);
                self.editor.change_track_name(p, t, name);
                Outcome::new("change_track_name", false)
            }
            24 => {
                let name = if self.coin() { None } else { Some(self.pick(&NAMES)) };
                self.editor.add_pattern(name);
                Outcome::new("add_pattern", false)
            }
            25 => {
                self.editor.duplicate_pattern(p);
                Outcome::new("duplicate_pattern", false)
            }
            26 => {
                self.editor.remove_pattern(p);
                Outcome::new("remove_pattern", false)
            }
            27 => {
                let to = self.pattern();
                self.editor.move_pattern(p, to);
                Outcome::new("move_pattern", false)
            }
            28 => {
                let name = self.pick(&NAMES);
                self.editor.rename_pattern(p, name);
                Outcome::new("rename_pattern", false)
            }
            29 => {
                let (bpm, coalesce) = (self.pick(&[0.0, 90.0, 120.0, 174.0, f64::NAN]), self.coin());
                self.editor.set_pattern_bpm(p, bpm, coalesce);
                Outcome::new("set_pattern_bpm", coalesce)
            }
            30 => {
                let beats = self.number(-2.0, 20.0).round();
                self.editor.set_pattern_beat_count(p, beats);
                Outcome::new("set_pattern_beat_count", false)
            }
            31 => {
                let tags = [" rock ".to_owned(), " ".to_owned(), "live".to_owned()];
                let take = self.below(4);
                self.editor.set_pattern_tags(p, &tags[..take]);
                Outcome::new("set_pattern_tags", false)
            }
            32 => {
                let description = self.pick(&["", "intro", "fill"]);
                self.editor.set_pattern_description(p, description);
                Outcome::new("set_pattern_description", false)
            }
            33 => {
                let style = self.pick(&[None, Some("rap"), Some("funk")]);
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.generate_pattern(scales, styles, style, &mut || gen_rng.next_f64());
                Outcome::new("generate_pattern", false)
            }
            34 => {
                let style = self.pick(&[None, Some("rock")]);
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.regenerate_pattern(p, scales, styles, style, &mut || gen_rng.next_f64());
                Outcome::new("regenerate_pattern", false)
            }
            35 => {
                let (key, value) = (self.pick(&[ENERGY, COMPLEXITY, VARIATION]), self.number(-20.0, 120.0).round());
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.set_pattern_macro(p, key, value, (scales, styles), &mut || gen_rng.next_f64());
                Outcome::new("set_pattern_macro", true)
            }
            36 => {
                self.editor.toggle_pattern_auto(p);
                Outcome::new("toggle_pattern_auto", false)
            }
            37 => {
                let style = self.pick(&[None, Some("funk")]);
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.toggle_drums(p, scales, styles, style, &mut || gen_rng.next_f64());
                Outcome::new("toggle_drums", false)
            }
            38 => {
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.regenerate_track(p, t, scales, styles, &mut || gen_rng.next_f64());
                Outcome::new("regenerate_track", false)
            }
            39 => {
                let kind = self.pick(&[MelodicKind::Bass, MelodicKind::Chords]);
                let Self { editor, gen_rng, scales, styles, .. } = self;
                editor.toggle_melodic(p, kind, scales, styles, &mut || gen_rng.next_f64());
                Outcome::new("toggle_melodic", false)
            }
            40 => {
                let Self { editor, gen_rng, .. } = self;
                editor.randomize_track(p, t, &mut || gen_rng.next_f64());
                Outcome::new("randomize_track", false)
            }
            41 => {
                let (song, measure, count) = (self.song(), self.measure(), self.pick(&[0.0, -1.0, 0.75, 2.0]));
                let pattern_id = if self.below(10) == 0 { String::new() } else { self.reference() };
                self.editor.add_song_clip(song, SongClip { pattern_id, start_measure: measure, measure_count: count });
                Outcome::new("add_song_clip", false)
            }
            42 => {
                let song = self.song();
                let len = song
                    .or(Some(self.editor.selected_song()))
                    .and_then(|s| self.library().songs.get(s))
                    .map_or(0, |s| s.clips.len());
                let indices: Vec<usize> = (0..self.below(4)).map(|_| self.index(len)).collect();
                self.editor.remove_song_clips(song, &indices);
                Outcome::new("remove_song_clips", false)
            }
            43 => {
                let (song, index, measure) = (self.song(), self.below(6), self.measure());
                self.editor.move_song_clip(song, index, measure);
                Outcome::new("move_song_clip", false)
            }
            44 | 45 => {
                let (reference, measure, song) = (self.reference(), self.measure(), self.song());
                let count = self.pick(&[None, Some(0.75), Some(0.0), Some(f64::NAN), Some(3.0)]);
                let result = self.editor.add_pattern_at_measure(&reference, measure, count, song);
                Outcome::new("add_pattern_at_measure", false).refused_if(&result)
            }
            46 => {
                let (measure, song) = (self.measure(), self.song());
                let result = self.editor.repeat_pattern_at_measure(measure, song);
                Outcome::new("repeat_pattern_at_measure", false).refused_if(&result)
            }
            47 => {
                let (measure, song) = (self.measure(), self.song());
                let result = self.editor.remove_pattern_at_measure(measure, song);
                Outcome::new("remove_pattern_at_measure", false).refused_if(&result)
            }
            48 => {
                let (reference, song) = (self.reference(), self.song());
                let result = self.editor.remove_pattern_clips(&reference, song);
                Outcome::new("remove_pattern_clips", false).refused_if(&result)
            }
            49 => {
                let name = self.pick(&NAMES);
                if self.coin() {
                    self.editor.add_song(name);
                    Outcome::new("add_song", false)
                } else {
                    let song = self.song();
                    let result = self.editor.rename_song(song, name);
                    Outcome::new("rename_song", false).refused_if(&result)
                }
            }
            50 => {
                let song = self.song();
                let bpm = self.pick(&[None, Some(500.0), Some(f64::NAN), Some(98.0)]);
                let description = self.pick(&[None, Some(""), Some("intro")]);
                let loop_measures = self.pick(&[None, Some(0.0), Some(8.0), Some(f64::INFINITY)]);
                let result = self.editor.set_song_settings(song, bpm, description, loop_measures);
                Outcome::new("set_song_settings", false).refused_if(&result)
            }
            51 => {
                if self.coin() {
                    let index = self.index(self.library().songs.len());
                    self.editor.set_selected_song(index);
                    Outcome::new("set_selected_song", false)
                } else {
                    let song = self.song();
                    self.editor.remove_song(song);
                    Outcome::new("remove_song", false)
                }
            }
            52 | 53 => self.drag(p, t),
            54 => {
                let count = 1 + self.below(3);
                self.editor_batch(count);
                Outcome::new("batch", false)
            }
            _ => {
                // Another library loaded: always a step, so only a different one.
                let mut other = library();
                other.patterns.truncate(1 + self.below(3));
                if other != *self.library() {
                    let selected = self.below(3);
                    self.editor.load_library(other, selected);
                }
                Outcome::new("load_library", false)
            }
        }
    }

    /// A drag: 2 to 4 edits of the same thing, close in time, that make one
    /// undo step (or join the previous one).
    fn drag(&mut self, p: usize, t: usize) -> Outcome {
        let (what, n, count) = (self.below(5), self.note(p, t), 2 + self.below(3));
        let nudge = self.pick(&[Nudge::VelocityUp, Nudge::VelocityDown, Nudge::PitchUp, Nudge::PitchDown]);
        for i in 0..count {
            if i > 0 {
                self.clock += CLOSE_MS;
                self.editor.set_clock(Some(self.clock));
            }
            let x = self.rng.next_f64();
            match what {
                0 => {
                    let velocity = Value::Number((x * 100.0).round() / 100.0);
                    self.editor.update_track(p, t, &[("velocity", velocity)], Some("Volume"), true);
                }
                1 => {
                    let velocity = Value::Number((x * 100.0).round() / 100.0);
                    self.editor.update_note(p, t, n, &[("velocity", velocity)], None, true);
                }
                2 => {
                    self.editor.set_pattern_bpm(p, 60.0 + (x * 120.0).round(), true);
                }
                3 => {
                    self.editor.set_steps_per_beat(p, t, 1.0 + (x * 7.0).round(), true);
                }
                _ => {
                    self.editor.nudge_note(p, t, n, nudge);
                }
            }
        }
        Outcome::new("drag", true)
    }

    /// `count` random commands as one batch. The commands act on
    /// `self.editor`: the editor running the batch is swapped in for them.
    fn editor_batch(&mut self, count: usize) {
        let mut editor = std::mem::replace(&mut self.editor, Editor::new(Library::default()));
        editor.batch("Batch", |editor| {
            std::mem::swap(&mut self.editor, editor);
            for _ in 0..count {
                self.command(true);
            }
            std::mem::swap(&mut self.editor, editor);
        });
        self.editor = editor;
    }
}

/// What a command was.
#[derive(Debug)]
struct Outcome {
    name: &'static str,
    /// It may join the previous undo step.
    coalescing: bool,
    /// It returned an error.
    refused: bool,
}

impl Outcome {
    fn new(name: &'static str, coalescing: bool) -> Self {
        Self { name, coalescing, refused: false }
    }

    fn refused_if<T>(self, result: &Result<T, String>) -> Self {
        Self { refused: result.is_err(), ..self }
    }
}

/// The library before each step that can be undone, and after each that
/// can be redone, as the history should give them back.
#[derive(Default)]
struct Model {
    before: Vec<Library>,
    after: Vec<Library>,
}

/// Run one sequence; the commands that changed the library are added to `changed`.
fn check_sequence(seed: u64, start: &Library, styles: &Styles, changed: &mut BTreeSet<&'static str>) {
    let mut fuzz = Fuzz::new(seed, start.clone(), styles.clone());
    let mut model = Model::default();
    for i in 0..COMMANDS {
        let at = |what: &str| format!("seed {seed}, command {i} ({what})");
        let undo_len = fuzz.editor.history().undo_len();
        let redo_len = fuzz.editor.history().redo_len();
        let before = fuzz.library().clone();
        match fuzz.below(10) {
            0 => {
                let next = fuzz.editor.history().next_undo().map(str::to_owned);
                assert_eq!(fuzz.editor.undo(), next, "{}", at("undo"));
                if next.is_some() {
                    assert_eq!(fuzz.library(), &model.before.pop().unwrap(), "{}", at("undo"));
                    model.after.push(before);
                }
                continue;
            }
            1 => {
                let next = fuzz.editor.history().next_redo().map(str::to_owned);
                assert_eq!(fuzz.editor.redo(), next, "{}", at("redo"));
                if next.is_some() {
                    assert_eq!(fuzz.library(), &model.after.pop().unwrap(), "{}", at("redo"));
                    model.before.push(before);
                }
                continue;
            }
            _ => {}
        }
        fuzz.clock += if fuzz.coin() { CLOSE_MS } else { APART_MS };
        fuzz.editor.set_clock(Some(fuzz.clock));
        let outcome = fuzz.command(false);
        let at = at(outcome.name);
        let history = fuzz.editor.history();
        if outcome.refused {
            assert_eq!(fuzz.library(), &before, "{at}: a refused edit changes nothing");
        }
        if *fuzz.library() == before {
            assert_eq!((history.undo_len(), history.redo_len()), (undo_len, redo_len), "{at}: no change, no step");
            continue;
        }
        changed.insert(outcome.name);
        assert_eq!(history.redo_len(), 0, "{at}: an edit drops what was undone");
        model.after.clear();
        if history.undo_len() == undo_len + 1 {
            model.before.push(before);
        } else {
            assert!(
                outcome.coalescing && history.undo_len() == undo_len,
                "{at}: one step per edit, {undo_len} → {}",
                history.undo_len()
            );
        }
    }

    // The state after the most recent step, undone or not.
    let end = model.after.first().cloned().unwrap_or_else(|| fuzz.library().clone());
    while fuzz.editor.redo().is_some() {}
    assert_eq!(fuzz.library(), &end, "seed {seed}: redo everything");
    assert_eq!(model.before.first().unwrap_or(start), start, "seed {seed}: the first step starts at the start");
    while fuzz.editor.undo().is_some() {}
    assert_eq!(fuzz.library(), start, "seed {seed}: undo everything gives back the start");
    while fuzz.editor.redo().is_some() {}
    assert_eq!(fuzz.library(), &end, "seed {seed}: redo everything gives back the end");
}

#[test]
fn every_edit_undoes_and_redoes_exactly() {
    let (start, styles) = (library(), styles());
    let mut changed = BTreeSet::new();
    for seed in 0..SEEDS {
        check_sequence(seed, &start, &styles, &mut changed);
    }
    let never: Vec<_> = COMMAND_NAMES.iter().filter(|name| !changed.contains(*name)).collect();
    assert!(never.is_empty(), "commands that never changed anything: {never:?}");
}

/// The edits that are not undo steps, by design: the selected arrangement,
/// the library header, the lines auto mode makes every loop.
#[test]
fn some_changes_are_not_undo_steps() {
    let mut editor = Editor::new(library());
    assert_eq!(editor.set_selected_song(99), 1, "clamped to the list");
    assert!(editor.set_library_info("name", "Demo"));
    assert!(!editor.set_library_info("name", "Demo"));
    let mut tracks = vec![Track::new("KICK", 4, 4)];
    editor.swap_tracks(0, &mut tracks);
    assert_eq!(editor.library().patterns[0].tracks.len(), 1);
    assert_eq!(editor.history().undo_len(), 0);
}

// ─── Compact ─────────────────────────────────────────────────────────────────

/// What a pattern plays over a few passes, in a comparable order: tick,
/// track, pitch, velocity, pan.
fn plays(pattern: &Pattern) -> Vec<(u32, usize, u64, u64, u64)> {
    let mut hits: Vec<_> = (0..4)
        .flat_map(|pass| {
            let hits = expand_hits(pattern, pass, &mut Rng::new(1), &Scales::default());
            hits.into_iter().flat_map(move |(tick, hits)| {
                hits.into_iter().map(move |h| {
                    (tick + pass * 1_000_000, h.track, h.pitch.to_bits(), h.velocity.to_bits(), h.pan.to_bits())
                })
            })
        })
        .collect();
    hits.sort_unstable();
    hits
}

/// Steps a beat a track can have.
const GRIDS: std::ops::RangeInclusive<i32> = 1..=8;

fn gcd(mut a: i32, mut b: i32) -> i32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn lcm(a: i32, b: i32) -> i32 {
    a / gcd(a, b) * b
}

/// A pattern of random tracks: notes repeating every `period` steps (some
/// tracks broken by one more note), or none.
fn repeating_pattern(rng: &mut Rng) -> (Pattern, Vec<Option<i32>>) {
    let beat_count = 1 + rng.below(4) as i32;
    let mut periods = Vec::new();
    let tracks = (0..1 + rng.below(6))
        .map(|i| {
            let steps_per_beat = GRIDS.start() + rng.below(GRIDS.count() as u64) as i32;
            let total = beat_count * steps_per_beat;
            let mut track = Track::new(TRACK_NAMES[i as usize % 3], beat_count, steps_per_beat);
            let period = 1 + rng.below(total as u64) as i32;
            // Some tracks have one `stepPercent` for all their notes (as
            // files can): then swing does not tie the loop to the beat, and
            // a loop may last a fractional number of ticks.
            let step_percent = rng.chance(0.3).then_some(0.0);
            let motif: Vec<(i32, f64, f64)> = (0..period)
                .filter_map(|step| {
                    let note = (step, f64::from(rng.below(3) as i32), [0.5, 0.8, 1.0][rng.below(3) as usize]);
                    rng.chance(0.4).then_some(note)
                })
                .collect();
            for start in (0..total).step_by(period as usize) {
                for &(step, pitch, velocity) in &motif {
                    let at = start + step;
                    if at < total {
                        track.notes.push(Note {
                            beat: at / steps_per_beat,
                            step: at % steps_per_beat,
                            pitch,
                            velocity,
                            step_percent,
                            ..Note::default()
                        });
                    }
                }
            }
            let broken = rng.chance(0.2);
            if broken {
                let at = rng.below(total as u64) as i32;
                track.notes.push(Note {
                    beat: at / steps_per_beat,
                    step: at % steps_per_beat,
                    pitch: 7.0,
                    step_percent,
                    ..Note::default()
                });
            }
            periods.push((!broken && !motif.is_empty()).then_some(period));
            track
        })
        .collect();
    (Pattern { id: "p".into(), name: Some("p".into()), bpm: 120.0, beat_count, tracks, ..Pattern::default() }, periods)
}

#[test]
fn compacting_keeps_what_plays() {
    let mut rng = Rng::new(7);
    for case in 0..300 {
        let (pattern, periods) = repeating_pattern(&mut rng);
        let mut editor = Editor::new(Library { patterns: vec![pattern.clone()], ..Library::default() });
        let compacted = editor.compact_pattern(0);
        let after = &editor.library().patterns[0];

        assert_eq!(plays(after), plays(&pattern), "case {case}: the same notes play");
        assert_eq!(editor.history().undo_len(), compacted, "case {case}: one step per compacted track");
        for (t, (track, before)) in after.tracks.iter().zip(&pattern.tracks).enumerate() {
            let changed = track != before;
            assert!(!changed || track.notes.len() < before.notes.len(), "case {case}, track {t}: a note removed");
            // A step's position in its beat is part of what plays (swing,
            // unless `stepPercent` is set): the loop also falls on it.
            if let Some(period) = periods[t] {
                let spb = track.steps_per_beat;
                let swing = if before.notes.iter().all(|n| n.step_percent.is_some()) { 1 } else { spb };
                let bound = lcm(period, swing);
                let reaches = before.notes.iter().any(|n| n.beat * spb + n.step >= bound);
                if bound < before.beat_count * spb && reaches {
                    let loop_length = track.loop_length.unwrap();
                    assert!(changed && loop_length <= bound, "case {case}, track {t}: loop {loop_length} > {bound}");
                }
            }
        }
        assert_eq!(editor.compact_pattern(0), 0, "case {case}: compacting again changes nothing");
        while editor.undo().is_some() {}
        assert_eq!(editor.library().patterns[0], pattern, "case {case}: undone");
    }
}
