//! The MCP tools (`mcp.rs`) run on the session: what the JS server did to
//! `song.json`, done to the library the window shows. The tools that only
//! edit the library are `ordrumbox_logic::tools`; the ones here play, read
//! and write files, change the kits, the synth presets or the master, or
//! change what the window shows. A tool that edits is one undo step and is
//! saved like any edit; its arguments are checked before anything changes.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_engine::synth_preset::tags::Facet;
use ordrumbox_logic::tools::{
    Tools, count, find_pattern, find_song, list, object, optional_bool, optional_text, optional_whole, pattern_name,
    pattern_summary, same_name, strings, text,
};

use super::Session;
use super::session_types::PlayMode;
use crate::editors::note_range;
use crate::picker::Export;
use crate::{master, output};
use crate::{mcp_link, store, update};
use ordrumbox_engine::{Mode, analysis};
use std::path::{Path, PathBuf};

/// A group of tools: its answer to tool `name`, or `None` when it has no such tool.
type ToolGroup = fn(&mut Session, &str, &Value) -> Result<Option<Value>, String>;

impl Session {
    /// Start the MCP link: its port goes to `mcp.json`. A failure is a
    /// status line; the app works without it.
    pub fn start_mcp(&mut self, waker: update::Waker) {
        let Some(endpoint) = store::mcp_endpoint_path() else {
            self.status.push("MCP not available: no user data folder".into());
            return;
        };
        self.start_mcp_at(&endpoint, waker);
    }

    /// Start the MCP link, its port written to `endpoint` (the file
    /// `ordrumbox --mcp` reads). A failure is a status line.
    pub fn start_mcp_at(&mut self, endpoint: &Path, waker: update::Waker) {
        match mcp_link::Server::start(endpoint, waker) {
            Ok(server) => self.mcp = Some(server),
            Err(e) => self.status.push(format!("MCP not available: {e}")),
        }
    }

    /// Run the MCP calls waiting, and show a connection that failed to
    /// open; whether there was either.
    pub fn poll_mcp(&mut self) -> bool {
        let mut called = false;
        while let Some(incoming) = self.mcp.as_ref().and_then(mcp_link::Server::poll) {
            match incoming {
                mcp_link::Incoming::Call(request) => {
                    let outcome = self.mcp_call(&request.name, &request.arguments);
                    request.answer(outcome);
                }
                mcp_link::Incoming::Failed(message) => self.tell(message),
            }
            called = true;
        }
        called
    }

    /// Run tool `name` on `args` (an object); what it answers.
    pub(super) fn mcp_call(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        // What waits for the user holds indexes that an edit would shift.
        if self.confirm_open() {
            return Err("A confirmation is open in the app: the user must answer it first".into());
        }
        // These run their own bookkeeping: nothing to settle afterwards.
        match name {
            "undo" => self.undo(),
            "redo" => self.redo(),
            _ => {}
        }
        if matches!(name, "undo" | "redo") {
            return Ok(object([("message", Value::from(self.message.as_str()))]));
        }
        let revision = self.editor.revision();
        let outcome = self.mcp_dispatch(name, args);
        if self.editor.revision() != revision {
            self.edited();
            self.clamp_selection();
            self.clamp_cursor_to_grid();
        }
        outcome
    }

    /// The groups of tools of `session_mcp_tools.rs`, in the order they are asked.
    const TOOL_GROUPS: [ToolGroup; 5] = [
        Session::mcp_arrangement_tool,
        Session::mcp_files_tool,
        Session::mcp_synth_tool,
        Session::mcp_kit_tool,
        Session::mcp_sound_tool,
    ];

    /// Run tool `name`: the group that has it, else a library tool.
    pub(super) fn mcp_dispatch(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        for group in Self::TOOL_GROUPS {
            if let Some(answer) = group(self, name, args)? {
                return Ok(answer);
            }
        }
        self.mcp_library_tool(name, args)
    }

    /// Run library tool `name` (`ordrumbox_logic::tools`) on the editor and
    /// what the generators read.
    pub(super) fn mcp_library_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let synth_presets = self.synths.presets.keys();
        let mut tools = Tools {
            editor: &mut self.editor,
            scales: &self.generators.scales,
            scale_list: &self.generators.scale_list,
            styles: &self.generators.styles,
            learned_styles: &mut self.generators.learned_styles,
            rng: &mut self.generators.rng,
            synth_presets: &synth_presets,
            note_range,
        };
        tools.call(name, args).unwrap_or_else(|| Err(format!("Unknown tool: {name}")))
    }

    pub(super) fn mcp_pattern(&self, reference: &str) -> Result<usize, String> {
        find_pattern(self.editor.library(), reference)
    }

    pub(super) fn mcp_remove_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let (name, clips) = self.remove_pattern_with_clips(p)?;
        Ok(object([("removed", Value::from(name)), ("clipsRemoved", count(clips))]))
    }

    pub(super) fn mcp_select_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let count = self.editor.library().patterns.len();
        let target = match (optional_text(args, "patternName")?, optional_whole(args, "index")?) {
            (Some(name), _) if !name.trim().is_empty() => Some(self.mcp_pattern(name)?),
            (_, Some(index)) if (index as usize) < count => Some(index as usize),
            (_, Some(index)) => return Err(format!("index: {index}, the library has {count} patterns")),
            _ => None,
        };
        if let Some(p) = target {
            self.show_pattern(p);
        }
        let pattern = self.pattern().ok_or("No pattern in the library")?;
        Ok(pattern_summary(self.pattern_index, pattern))
    }

    /// Where a tool writes `export` of pattern `p` or arrangement `s`: the
    /// exports folder, under the name the window would give.
    pub(super) fn mcp_export_path(&self, export: Export, p: usize, s: usize) -> Result<PathBuf, String> {
        let dir = store::exports_dir().ok_or("no user data folder to export to")?;
        let name = self.export_file_name_of(export, p, s).ok_or("nothing to export")?;
        Ok(dir.join(name))
    }

    pub(super) fn mcp_import_midi(&mut self, args: &Value) -> Result<Value, String> {
        let path = text(args, "filePath")?.trim();
        let before = self.editor.library().patterns.len();
        let message = self.import_midi_file(Path::new(path))?;
        let patterns = self.editor.library().patterns.iter().enumerate().skip(before);
        Ok(object([
            ("message", Value::from(message)),
            ("patterns", Value::Array(patterns.map(|(i, p)| pattern_summary(i, p)).collect())),
        ]))
    }

    /// Master settings by key, each inside its knob's range.
    pub(super) fn mcp_set_master(&mut self, args: &Value) -> Result<Value, String> {
        let settings = args.get("settings").and_then(Value::as_object).ok_or("settings: not an object")?;
        let mut changed = self.output_panel.master();
        for (key, value) in settings.iter() {
            let at = format!("settings.{key}");
            if key == "compBypass" {
                let bypass = value.as_bool().ok_or_else(|| format!("{at}: not true or false ({value})"))?;
                changed = master::SECTION.set(changed, key, if bypass { 1.0 } else { 0.0 });
                continue;
            }
            let param = output::find(key).ok_or_else(|| format!("{at}: not a master setting"))?;
            let number = value.as_f64().ok_or_else(|| format!("{at}: not a number ({value})"))?;
            let (min, max) = (param.spec.min, param.spec.max);
            if !(min..=max).contains(&number) {
                return Err(format!("{at}: {number} is outside {min} to {max}"));
            }
            changed = master::SECTION.set(changed, key, number);
        }
        self.set_master(changed, true);
        self.message = "master settings changed".into();
        Ok(master::SECTION.to_json(&self.output_panel.master()))
    }

    /// A synth preset's key, by key (else in any case).
    pub(super) fn mcp_synth(&self, name: &str) -> Result<String, String> {
        let keys = self.synths.presets.keys();
        let found = keys.iter().find(|k| **k == name.trim()).or_else(|| keys.iter().find(|k| same_name(k, name)));
        found
            .map(|k| (*k).to_owned())
            .ok_or_else(|| format!("No synth preset \"{}\" (listSynthPresets gives them)", name.trim()))
    }

    pub(super) fn mcp_create_synth(&mut self, args: &Value) -> Result<Value, String> {
        let key = text(args, "preset")?.trim();
        if key.is_empty() {
            return Err("preset is empty".into());
        }
        if self.mcp_synth(key).is_ok() {
            return Err(format!("There is a synth preset \"{key}\" already"));
        }
        let draft = match optional_text(args, "from")?.filter(|f| !f.trim().is_empty()) {
            Some(from) => {
                let from = self.mcp_synth(from)?;
                self.synths.presets.get(&from).and_then(Value::as_object).cloned().unwrap_or_default()
            }
            None => model::defaults(),
        };
        let settings = synth_settings(args, false)?;
        Ok(self.mcp_write_synth(key, draft, &settings))
    }

    /// Preset `key` as `draft` with `settings` set as the synth editor sets
    /// them, to the player and to the disk; what it is now.
    pub(super) fn mcp_write_synth(&mut self, key: &str, mut draft: Object, settings: &[(String, Value)]) -> Value {
        model::hydrate(&mut draft);
        for (path, value) in settings {
            model::implicit_enable(&mut draft, path);
            model::set(&mut draft, path, value.clone());
        }
        self.put_synth(key, Some(Value::Object(draft.clone())), format!("Set synth preset \"{key}\""), None);
        if self.synth_editor.edits(key) {
            self.load_synth(key);
        }
        self.message = format!("synth preset \"{key}\" set");
        object([("preset", Value::from(key)), ("settings", Value::Object(draft))])
    }

    pub(super) fn mcp_remove_synth(&mut self, args: &Value) -> Result<Value, String> {
        let key = self.mcp_synth(text(args, "preset")?)?;
        if self.synths.presets.len() <= 1 {
            return Err("The last synth preset stays".into());
        }
        let library = self.editor.library();
        let using = library.patterns.iter().flat_map(|p| {
            let playing = p.tracks.iter().filter(|t| t.use_synth && t.synth_preset.as_deref() == Some(key.as_str()));
            playing.map(move |t| format!("{}/{}", pattern_name(p), t.name))
        });
        let using: Vec<String> = using.collect();
        if self.synth_editor.edits(&key) {
            // The editor shows the next preset, as its Delete button does.
            self.remove_synth(&key);
        } else {
            self.put_synth(&key, None, format!("Delete synth preset \"{key}\""), None);
        }
        Ok(object([("removed", Value::from(key)), ("tracksUsing", strings(&using))]))
    }

    pub(super) fn mcp_kits(&self) -> Value {
        let kits = self.kits.kits().iter().map(|kit| {
            let samples = kit.samples.iter().map(|s| {
                object([
                    ("key", Value::from(s.key.as_str())),
                    ("name", Value::from(s.display_name.as_str())),
                    ("path", Value::from(s.path.as_str())),
                ])
            });
            object([("name", Value::from(kit.name.as_str())), ("samples", Value::Array(samples.collect()))])
        });
        let current = self.kit().map_or(Value::Null, |k| Value::from(k.name.as_str()));
        object([("kit", current), ("kits", Value::Array(kits.collect()))])
    }

    pub(super) fn mcp_analyze(&self, args: &Value) -> Result<Value, String> {
        let mut results = Vec::new();
        for item in list(args, "samples")? {
            let path = item.as_str().ok_or_else(|| format!("samples: {item} is not a string"))?;
            let result = match self.kits.buffer(path) {
                Some(buffer) => {
                    let a = analysis::analyze(buffer);
                    object([
                        ("samplePath", Value::from(path)),
                        ("durationSec", Value::from(a.duration_sec)),
                        ("peakDb", Value::from(a.peak_db)),
                        ("rmsDb", Value::from(a.rms_db)),
                        ("fundamentalHz", a.fundamental_hz.map_or(Value::Null, Value::from)),
                        ("pitchConfidence", Value::from(a.pitch_confidence)),
                        ("note", a.note.map_or(Value::Null, |n| Value::from(n.to_string()))),
                    ])
                }
                None => object([
                    ("samplePath", Value::from(path)),
                    ("error", Value::from("no sample of the kits has this path (listKitSamples gives them)")),
                ]),
            };
            results.push(result);
        }
        Ok(object([("results", Value::Array(results))]))
    }

    pub(super) fn mcp_remove_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(&self.editor, args)?;
        if self.playback.plays(Mode::Song(s)) {
            self.playback.stop();
        }
        let name = self.editor.library().songs[s].name.clone();
        self.editor.remove_song(Some(s));
        self.song_panel.scroll_to_start();
        Ok(object([("removed", Value::from(name)), ("arrangements", count(self.editor.library().songs.len()))]))
    }

    pub(super) fn mcp_play(&mut self, args: &Value) -> Result<Value, String> {
        let song = optional_bool(args, "song")?.unwrap_or(false);
        if self.playback.controller().is_none() {
            return Err(NO_AUDIO.into());
        }
        self.playback.stop();
        if song {
            if self.editor.library().songs.is_empty() {
                return Err("No arrangement to play: createArrangement".into());
            }
            self.transport.song_cursor = 0;
            self.mode = PlayMode::Song;
            self.toggle(Mode::Song(self.editor.selected_song()), 0);
        } else {
            self.mode = PlayMode::Pattern;
            self.toggle(Mode::Pattern(self.pattern_index), 0);
        }
        let library = self.editor.library();
        let playing = match song {
            true => library.songs.get(self.editor.selected_song()).map(|s| s.name.clone()),
            false => self.pattern().map(pattern_name),
        };
        self.message = format!("playing {}", playing.as_deref().unwrap_or_default());
        Ok(object([("playing", playing.map_or(Value::Null, Value::from))]))
    }
    pub(super) fn mcp_assign_sounds(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        // What the window says in its message is refused here.
        match text(args, "sounds")? {
            "kit" => self.auto_assign_all_tracks(p),
            "synth" if self.synths.presets.is_empty() => return Err("No synth preset (createSynthPreset)".into()),
            "synth" => self.convert_to_generated_sounds(p),
            "random" if self.kits.kits().iter().all(|k| k.samples.is_empty()) => {
                return Err("No sample in the kits".into());
            }
            "random" => self.assign_random_samples(p),
            other => return Err(format!("sounds: \"{other}\" is not kit, synth or random")),
        }
        let message = self.message.clone();
        let tracks = self.editor.library().patterns[p].tracks.iter().map(|track| {
            let sound = match (track.use_synth, track.auto_assign) {
                (true, _) => track.synth_preset.as_deref().map_or(Value::Null, |k| Value::from(format!("synth:{k}"))),
                (false, true) => Value::from("kit"),
                (false, false) => Value::from(track.sample_path.as_str()),
            };
            object([("trackName", Value::from(track.name.as_str())), ("sound", sound)])
        });
        Ok(object([("message", Value::from(message)), ("tracks", Value::Array(tracks.collect()))]))
    }

    pub(super) fn mcp_playback_state(&self) -> Value {
        let library = self.editor.library();
        let Some(controller) = self.playback.controller().filter(|c| c.is_playing()) else {
            return object([("playing", Value::Bool(false))]);
        };
        let position = controller.position();
        match controller.mode() {
            Mode::Pattern(p) => {
                let pattern = library.patterns.get(p);
                let beat = position
                    .zip(pattern)
                    .map(|(at, pattern)| (at.frame * pattern.beat_count.max(1) as u64 / at.frames.max(1)) as f64);
                object([
                    ("playing", Value::Bool(true)),
                    ("patternName", pattern.map_or(Value::Null, |p| Value::from(pattern_name(p)))),
                    ("loop", position.map_or(Value::Null, |at| Value::from(at.pass as f64))),
                    ("beat", beat.map_or(Value::Null, Value::from)),
                ])
            }
            Mode::Song(s) => {
                let measure =
                    if s == self.editor.selected_song() { Some(self.song_cursor_measure().floor()) } else { None };
                object([
                    ("playing", Value::Bool(true)),
                    ("arrangement", library.songs.get(s).map_or(Value::Null, |song| Value::from(song.name.as_str()))),
                    ("measure", measure.map_or(Value::Null, Value::from)),
                ])
            }
        }
    }

    pub(super) fn mcp_set_project_info(&mut self, args: &Value) -> Result<Value, String> {
        let name = optional_text(args, "name")?.map(str::trim);
        let description = optional_text(args, "description")?.map(str::trim);
        if name.is_some_and(str::is_empty) {
            return Err("name is empty".into());
        }
        if name.is_none() && description.is_none() {
            return Err("Give name, description, or both".into());
        }
        let changed = self.editor.batch("Project info", |editor| {
            let named = name.is_some_and(|name| editor.set_library_info("name", name));
            let described = description.is_some_and(|text| editor.set_library_info("description", text));
            named || described
        });
        if changed {
            self.edited();
        }
        Ok(object([("name", Value::from(self.project_name())), ("description", Value::from(self.info("description")))]))
    }

    /// The project written to `path`, stamped `stamp`.
    pub(super) fn mcp_write_project(&mut self, path: &Path, stamp: &str) -> Result<Value, String> {
        store::save_project(path, &self.project_record(stamp)).map_err(|e| format!("{}: {e}", path.display()))?;
        self.message = format!("Project \"{}\" written to {}", self.project_name(), path.display());
        exported(path, self.message.clone())
    }

    /// The projects of the songs folder: name and file.
    pub(super) fn mcp_saved_projects(&self) -> Result<Vec<(String, PathBuf)>, String> {
        let dir = self.saving.projects_dir.as_ref().ok_or("no user data folder: no saved projects")?;
        let files = store::list_projects(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(files.into_iter().map(|f| (f.file_stem().unwrap_or_default().to_string_lossy().into_owned(), f)).collect())
    }

    pub(super) fn mcp_load_project(&mut self, args: &Value) -> Result<Value, String> {
        let path = match (optional_text(args, "name")?, optional_text(args, "filePath")?) {
            (Some(name), None) => {
                let projects = self.mcp_saved_projects()?;
                let found = projects.into_iter().find(|(saved, _)| same_name(saved, name)).map(|(_, path)| path);
                found.ok_or_else(|| format!("No saved project \"{}\" (listSavedProjects gives them)", name.trim()))?
            }
            (None, Some(path)) => PathBuf::from(path.trim()),
            _ => return Err("Give name or filePath".into()),
        };
        let name = self.read_project_file(&path)?;
        let library = self.editor.library();
        Ok(object([
            ("name", Value::from(name)),
            ("patterns", count(library.patterns.len())),
            ("arrangements", count(library.songs.len())),
        ]))
    }

    /// Argument `kit` (a kit name, any case), else the kit in use.
    pub(super) fn mcp_kit(&self, args: &Value) -> Result<usize, String> {
        match optional_text(args, "kit")?.filter(|k| !k.trim().is_empty()) {
            Some(wanted) => self
                .kits
                .kits()
                .iter()
                .position(|k| same_name(&k.name, wanted))
                .ok_or_else(|| format!("No kit named \"{}\" (listKitSamples gives them)", wanted.trim())),
            None => Some(self.kit_index).filter(|&k| k < self.kits.kits().len()).ok_or_else(|| "No kit loaded".into()),
        }
    }

    /// Kit `k`, read from a folder or a mapping: the kits saved, its samples told.
    pub(super) fn mcp_kit_added(&mut self, k: usize, message: String) -> Result<Value, String> {
        self.kits_changed(message);
        let kit = &self.kits.kits()[k];
        let samples = kit
            .samples
            .iter()
            .map(|s| object([("key", Value::from(s.key.as_str())), ("path", Value::from(s.path.as_str()))]));
        Ok(object([
            ("kit", Value::from(kit.name.as_str())),
            ("samples", Value::Array(samples.collect())),
            ("message", Value::from(self.message.as_str())),
        ]))
    }
}

/// What a tool that plays answers without a sound device.
pub(super) const NO_AUDIO: &str = "no audio device: orDrumbox plays nothing";

/// Argument `settings` (`required`, else it may be absent): synth preset
/// settings by path, checked against the synth editor's controls.
pub(super) fn synth_settings(args: &Value, required: bool) -> Result<Vec<(String, Value)>, String> {
    let fields = match args.get("settings") {
        None | Some(Value::Null) if !required => return Ok(Vec::new()),
        Some(Value::Object(fields)) if !(required && fields.is_empty()) => fields,
        Some(Value::Object(_)) => return Err("settings is empty".into()),
        _ => return Err("settings: not an object".into()),
    };
    let mut checked = Vec::new();
    for (path, value) in fields.iter() {
        let at = format!("settings.{path}");
        let value = if let Some(meta) = model::find_meta(path) {
            let n = value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| format!("{at}: not a number ({value})"))?;
            if !(meta.min..=meta.max).contains(&n) {
                return Err(format!("{at}: {n} is outside {} to {}", meta.min, meta.max));
            }
            if meta.step >= 1.0 && n.fract() != 0.0 {
                return Err(format!("{at}: {n} is not a whole number"));
            }
            Value::from(n)
        } else if let Some(values) = model::choices(path) {
            let allowed = values.iter().map(|v| model::choice_value(path, v));
            if !allowed.clone().any(|v| v == *value) {
                return Err(format!("{at}: {value} is not one of {}", Value::Array(allowed.collect())));
            }
            value.clone()
        } else if model::BYPASS_FLAGS.iter().any(|(_, flag)| *flag == path) {
            Value::Bool(value.as_bool().ok_or_else(|| format!("{at}: not true or false ({value})"))?)
        } else if let Some(facet) = Facet::ALL.into_iter().find(|facet| facet.key() == path) {
            let tag = value.as_str().ok_or_else(|| format!("{at}: not a text ({value})"))?;
            Value::from(facet.parse(tag).map_err(|e| format!("{at}: {e}"))?.ok_or_else(|| format!("{at}: empty"))?)
        } else {
            return Err(format!("{at}: not a synth setting"));
        };
        checked.push((path.to_owned(), value));
    }
    Ok(checked)
}

/// The answer of a tool that wrote a file.
pub(super) fn exported(path: &Path, message: String) -> Result<Value, String> {
    Ok(object([("filePath", Value::from(path.display().to_string())), ("message", Value::from(message))]))
}

#[cfg(test)]
#[path = "session_mcp/tests.rs"]
mod tests;
