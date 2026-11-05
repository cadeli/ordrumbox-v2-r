//! The MCP tools (`mcp.rs`) run on the session: what the JS server did to
//! `song.json`, done to the library the window shows. The tools that only
//! edit the library are `ordrumbox_logic::tools`; the ones here play, read
//! and write files, change the kits, the synth presets or the master, or
//! change what the window shows. A tool that edits is one undo step and is
//! saved like any edit; its arguments are checked before anything changes.

use ordrumbox_core::json::{Object, Value};
use ordrumbox_engine::synth_model as model;
use ordrumbox_logic::tools::{
    Tools, count, find_pattern, find_song, find_track, list, object, optional_bool, optional_text, optional_whole,
    pattern_name, pattern_summary, same_name, song_json, strings, text,
};

use super::*;
use crate::editors::note_range;
use crate::{master, output};

impl Session {
    /// Start the MCP link: its port goes to `mcp.json`. A failure is a
    /// status line; the app works without it.
    pub fn start_mcp(&mut self, waker: update::Waker) {
        let Some(endpoint) = store::mcp_endpoint_path() else {
            self.status.push("MCP not available: no user data folder".into());
            return;
        };
        match mcp_link::Server::start(&endpoint, waker) {
            Ok(server) => self.mcp = Some(server),
            Err(e) => self.status.push(format!("MCP not available: {e}")),
        }
    }

    /// Run the MCP calls waiting; whether there was one.
    pub fn poll_mcp(&mut self) -> bool {
        let mut called = false;
        while let Some(request) = self.mcp.as_ref().and_then(mcp_link::Server::poll) {
            let outcome = self.mcp_call(&request.name, &request.arguments);
            request.answer(outcome);
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
        let revision = self.editor.revision();
        let outcome = match name {
            "listPatterns" => self.mcp_library_tool(name, args).map(|mut answer| {
                if let Some(object) = answer.as_object_mut() {
                    object.insert("shown", count(self.pattern_index));
                }
                answer
            }),
            "createArrangement" => {
                let answer = self.mcp_library_tool(name, args)?;
                self.song_panel.scroll_to_start();
                Ok(answer)
            }
            "selectPattern" => self.mcp_select_pattern(args),
            "removePattern" => self.mcp_remove_pattern(args),
            "savePatternToJson" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let path = self.mcp_export_path(Export::PatternJson, p, 0)?;
                self.write_pattern_json(p, &path)?;
                Ok(object([("filePath", Value::from(path.display().to_string()))]))
            }
            "exportPatternWav" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let path = self.mcp_export_path(Export::PatternWav, p, 0)?;
                exported(&path, self.write_wav(Mode::Pattern(p), &path)?)
            }
            "exportPatternMidi" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let path = self.mcp_export_path(Export::PatternMidi, p, 0)?;
                exported(&path, self.write_midi(p, &path)?)
            }
            "exportArrangementWav" => {
                let s = find_song(&self.editor, args)?;
                let path = self.mcp_export_path(Export::SongWav, 0, s)?;
                exported(&path, self.write_wav(Mode::Song(s), &path)?)
            }
            "importMidi" => self.mcp_import_midi(args),
            "getSynthPreset" => {
                let key = self.mcp_synth(text(args, "preset")?)?;
                let mut draft = self.synths.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
                model::hydrate(&mut draft);
                Ok(object([("preset", Value::from(key)), ("settings", Value::Object(draft))]))
            }
            "createSynthPreset" => self.mcp_create_synth(args),
            "updateSynthPreset" => {
                let key = self.mcp_synth(text(args, "preset")?)?;
                let settings = synth_settings(args, true)?;
                let draft = self.synths.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
                Ok(self.mcp_write_synth(&key, draft, &settings))
            }
            "removeSynthPreset" => self.mcp_remove_synth(args),
            "listSynthPresets" => {
                let keys = self.synths.keys().into_iter().map(Value::from).collect::<Vec<_>>();
                Ok(object([("count", count(keys.len())), ("presets", Value::Array(keys))]))
            }
            "listKitSamples" => Ok(self.mcp_kits()),
            "selectKit" => {
                let wanted = text(args, "kit")?;
                let found = self.kits.kits.iter().position(|k| same_name(&k.name, wanted));
                let index = found.ok_or_else(|| format!("No kit named \"{wanted}\" (listKitSamples gives them)"))?;
                self.select_kit(index);
                self.message = format!("kit {}", self.kits.kits[index].name);
                Ok(object([("kit", Value::from(self.kits.kits[index].name.as_str()))]))
            }
            "analyzeSamples" => self.mcp_analyze(args),
            "removeArrangement" => self.mcp_remove_arrangement(args),
            "getMaster" => Ok(master::to_json(&self.output_panel.master())),
            "setMaster" => self.mcp_set_master(args),
            "play" => self.mcp_play(args),
            "stop" => {
                if let Some(controller) = &mut self.controller {
                    controller.stop();
                }
                self.message = "stopped".into();
                Ok(object([("playing", Value::Bool(false))]))
            }
            "undo" => {
                self.undo();
                return Ok(object([("message", Value::from(self.message.as_str()))]));
            }
            "redo" => {
                self.redo();
                return Ok(object([("message", Value::from(self.message.as_str()))]));
            }
            "assignSounds" => self.mcp_assign_sounds(args),
            "auditionTrack" => {
                let (p, t) = find_track(self.editor.library(), args)?;
                let controller = self.controller.as_mut().ok_or(NO_AUDIO)?;
                controller.preview(self.editor.library(), p, t, None);
                Ok(object([("trackName", Value::from(self.editor.library().patterns[p].tracks[t].name.as_str()))]))
            }
            "getPlaybackState" => Ok(self.mcp_playback_state()),
            "selectArrangement" => {
                if args.get("arrangement").is_none_or(|a| *a == Value::Null) {
                    return Err("arrangement is required".into());
                }
                let s = find_song(&self.editor, args)?;
                self.select_song(s);
                Ok(song_json(self.editor.library(), s))
            }
            "setSongInfo" => self.mcp_set_song_info(args),
            "saveSong" => {
                let dir = self.songs_dir.as_ref().ok_or("no user data folder to save to")?;
                let path = dir.join(self.song_file_name());
                self.mcp_write_song(&path, "savedAt")
            }
            "exportSong" => {
                let path = store::exports_dir().ok_or("no user data folder to export to")?.join(self.song_file_name());
                self.mcp_write_song(&path, "exportedAt")
            }
            "listSavedSongs" => {
                let names = self.mcp_saved_songs()?.into_iter().map(|(name, _)| Value::from(name)).collect();
                Ok(object([("songs", Value::Array(names))]))
            }
            "loadSong" => self.mcp_load_song(args),
            "normalizeKit" => {
                let k = self.mcp_kit(args)?;
                let n = self.kits.normalize(k)?;
                self.kits_changed(format!("Normalized {n} sample(s)"));
                Ok(object([("kit", Value::from(self.kits.kits[k].name.as_str())), ("normalized", count(n))]))
            }
            "addSampleToKit" => {
                let k = self.mcp_kit(args)?;
                let (sample, message) = self.kits.add_sample(k, Path::new(text(args, "filePath")?.trim()))?;
                self.kits_changed(message);
                let added = &self.kits.kits[k].samples[sample];
                Ok(object([
                    ("kit", Value::from(self.kits.kits[k].name.as_str())),
                    ("key", Value::from(added.key.as_str())),
                    ("path", Value::from(added.path.as_str())),
                ]))
            }
            "replaceKitSample" => {
                let k = self.mcp_kit(args)?;
                let path = text(args, "samplePath")?.trim();
                let sample = self.kits.kits[k].samples.iter().position(|s| s.path == path);
                let sample =
                    sample.ok_or_else(|| format!("No sample \"{path}\" in that kit (listKitSamples gives them)"))?;
                let message = self.kits.replace_sample(k, sample, Path::new(text(args, "filePath")?.trim()))?;
                self.kits_changed(message);
                Ok(object([("message", Value::from(self.message.as_str()))]))
            }
            "importKitFolder" => {
                let (k, message) = self.kits.import_folder(Path::new(text(args, "folderPath")?.trim()))?;
                self.mcp_kit_added(k, message)
            }
            "loadKitMapping" => {
                let (k, message) = self.kits.load_mapping(Path::new(text(args, "filePath")?.trim()))?;
                self.mcp_kit_added(k, message)
            }
            "exportKitMapping" => {
                let k = self.mcp_kit(args)?;
                let name = self.kits.mapping_file_name(k).ok_or("nothing to export")?;
                let path = store::exports_dir().ok_or("no user data folder to export to")?.join(name);
                exported(&path, self.kits.export_mapping(k, &path)?)
            }
            _ => self.mcp_library_tool(name, args),
        };
        if self.editor.revision() != revision {
            self.edited();
            self.clamp_selection();
            self.clamp_cursor_to_grid();
        }
        outcome
    }

    /// Run library tool `name` (`ordrumbox_logic::tools`) on the editor and
    /// what the generators read.
    fn mcp_library_tool(&mut self, name: &str, args: &Value) -> Result<Value, String> {
        let synth_presets = self.synths.keys();
        let mut tools = Tools {
            editor: &mut self.editor,
            scales: &self.scales,
            scale_list: &self.scale_list,
            styles: &self.styles,
            learned_styles: &mut self.learned_styles,
            rng: &mut self.rng,
            synth_presets: &synth_presets,
            note_range,
        };
        tools.call(name, args).unwrap_or_else(|| Err(format!("Unknown tool: {name}")))
    }

    fn mcp_pattern(&self, reference: &str) -> Result<usize, String> {
        find_pattern(self.editor.library(), reference)
    }

    fn mcp_remove_pattern(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        let (name, clips) = self.remove_pattern_with_clips(p)?;
        Ok(object([("removed", Value::from(name)), ("clipsRemoved", count(clips))]))
    }

    fn mcp_select_pattern(&mut self, args: &Value) -> Result<Value, String> {
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
    fn mcp_export_path(&self, export: Export, p: usize, s: usize) -> Result<PathBuf, String> {
        let dir = store::exports_dir().ok_or("no user data folder to export to")?;
        let name = self.export_file_name_of(export, p, s).ok_or("nothing to export")?;
        Ok(dir.join(name))
    }

    fn mcp_import_midi(&mut self, args: &Value) -> Result<Value, String> {
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
    fn mcp_set_master(&mut self, args: &Value) -> Result<Value, String> {
        let settings = args.get("settings").and_then(Value::as_object).ok_or("settings: not an object")?;
        let mut changed = self.output_panel.master();
        for (key, value) in settings.iter() {
            let at = format!("settings.{key}");
            if key == "compBypass" {
                let bypass = value.as_bool().ok_or_else(|| format!("{at}: not true or false ({value})"))?;
                changed = master::set(changed, key, if bypass { 1.0 } else { 0.0 });
                continue;
            }
            let param = output::find(key).ok_or_else(|| format!("{at}: not a master setting"))?;
            let number = value.as_f64().ok_or_else(|| format!("{at}: not a number ({value})"))?;
            let (min, max) = (param.spec.min, param.spec.max);
            if !(min..=max).contains(&number) {
                return Err(format!("{at}: {number} is outside {min} to {max}"));
            }
            changed = master::set(changed, key, number);
        }
        self.set_master(changed, true);
        self.message = "master settings changed".into();
        Ok(master::to_json(&self.output_panel.master()))
    }

    /// A synth preset's key, by key (else in any case).
    fn mcp_synth(&self, name: &str) -> Result<String, String> {
        let keys = self.synths.keys();
        let found = keys.iter().find(|k| **k == name.trim()).or_else(|| keys.iter().find(|k| same_name(k, name)));
        found
            .map(|k| (*k).to_owned())
            .ok_or_else(|| format!("No synth preset \"{}\" (listSynthPresets gives them)", name.trim()))
    }

    fn mcp_create_synth(&mut self, args: &Value) -> Result<Value, String> {
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
                self.synths.get(&from).and_then(Value::as_object).cloned().unwrap_or_default()
            }
            None => model::defaults(),
        };
        let settings = synth_settings(args, false)?;
        Ok(self.mcp_write_synth(key, draft, &settings))
    }

    /// Preset `key` as `draft` with `settings` set as the synth editor sets
    /// them, to the player and to the disk; what it is now.
    fn mcp_write_synth(&mut self, key: &str, mut draft: Object, settings: &[(String, Value)]) -> Value {
        model::hydrate(&mut draft);
        for (path, value) in settings {
            model::implicit_enable(&mut draft, path);
            model::set(&mut draft, path, value.clone());
        }
        self.synths.set(key, Value::Object(draft.clone()));
        self.synths_changed();
        if self.synth_editor.edits(key) {
            self.load_synth(key);
        }
        self.message = format!("synth preset \"{key}\" set");
        object([("preset", Value::from(key)), ("settings", Value::Object(draft))])
    }

    fn mcp_remove_synth(&mut self, args: &Value) -> Result<Value, String> {
        let key = self.mcp_synth(text(args, "preset")?)?;
        if self.synths.len() <= 1 {
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
            self.synths.remove(&key);
            self.synths_changed();
            self.message = format!("Deleted \"{key}\"");
        }
        Ok(object([("removed", Value::from(key)), ("tracksUsing", strings(&using))]))
    }

    fn mcp_kits(&self) -> Value {
        let kits = self.kits.kits.iter().map(|kit| {
            let samples = kit.samples.iter().map(|s| {
                object([
                    ("key", Value::from(s.key.as_str())),
                    ("name", Value::from(s.display_name.as_str())),
                    ("path", Value::from(s.path.as_str())),
                ])
            });
            object([("name", Value::from(kit.name.as_str())), ("samples", Value::Array(samples.collect()))])
        });
        let current = self.kits.kits.get(self.kit_index).map_or(Value::Null, |k| Value::from(k.name.as_str()));
        object([("kit", current), ("kits", Value::Array(kits.collect()))])
    }

    fn mcp_analyze(&self, args: &Value) -> Result<Value, String> {
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

    fn mcp_remove_arrangement(&mut self, args: &Value) -> Result<Value, String> {
        let s = find_song(&self.editor, args)?;
        if let Some(controller) = &mut self.controller
            && controller.is_playing()
            && controller.mode() == Mode::Song(s)
        {
            controller.stop();
        }
        let name = self.editor.library().songs[s].name.clone();
        self.editor.remove_song(Some(s));
        self.song_panel.scroll_to_start();
        Ok(object([("removed", Value::from(name)), ("arrangements", count(self.editor.library().songs.len()))]))
    }

    fn mcp_play(&mut self, args: &Value) -> Result<Value, String> {
        let song = optional_bool(args, "song")?.unwrap_or(false);
        let Some(controller) = &mut self.controller else {
            return Err(NO_AUDIO.into());
        };
        controller.stop();
        if song {
            if self.editor.library().songs.is_empty() {
                return Err("No arrangement to play: createArrangement".into());
            }
            self.song_cursor = 0;
            self.toggle(Mode::Song(self.editor.selected_song()), 0);
        } else {
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
    fn mcp_assign_sounds(&mut self, args: &Value) -> Result<Value, String> {
        let p = self.mcp_pattern(text(args, "patternName")?)?;
        // What the window says in its message is refused here.
        match text(args, "sounds")? {
            "kit" => self.auto_assign_all_tracks(p),
            "synth" if self.synths.is_empty() => return Err("No synth preset (createSynthPreset)".into()),
            "synth" => self.convert_to_generated_sounds(p),
            "random" if self.kits.kits.iter().all(|k| k.samples.is_empty()) => {
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

    fn mcp_playback_state(&self) -> Value {
        let library = self.editor.library();
        let Some(controller) = self.controller.as_ref().filter(|c| c.is_playing()) else {
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

    fn mcp_set_song_info(&mut self, args: &Value) -> Result<Value, String> {
        let name = optional_text(args, "name")?.map(str::trim);
        let description = optional_text(args, "description")?.map(str::trim);
        if name.is_some_and(str::is_empty) {
            return Err("name is empty".into());
        }
        if name.is_none() && description.is_none() {
            return Err("Give name, description, or both".into());
        }
        if let Some(name) = name {
            self.editor.set_library_info("name", name);
        }
        if let Some(description) = description {
            self.editor.set_library_info("description", description);
        }
        Ok(object([("name", Value::from(self.song_name())), ("description", Value::from(self.info("description")))]))
    }

    /// The song written to `path`, stamped `stamp`.
    fn mcp_write_song(&mut self, path: &Path, stamp: &str) -> Result<Value, String> {
        store::save_song(path, &self.song_record(stamp)).map_err(|e| format!("{}: {e}", path.display()))?;
        self.message = format!("Song \"{}\" written to {}", self.song_name(), path.display());
        exported(path, self.message.clone())
    }

    /// The songs of the songs folder: name and file.
    fn mcp_saved_songs(&self) -> Result<Vec<(String, PathBuf)>, String> {
        let dir = self.songs_dir.as_ref().ok_or("no user data folder: no saved songs")?;
        let files = store::list_songs(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(files.into_iter().map(|f| (f.file_stem().unwrap_or_default().to_string_lossy().into_owned(), f)).collect())
    }

    fn mcp_load_song(&mut self, args: &Value) -> Result<Value, String> {
        let path = match (optional_text(args, "name")?, optional_text(args, "filePath")?) {
            (Some(name), None) => {
                let songs = self.mcp_saved_songs()?;
                let found = songs.into_iter().find(|(saved, _)| same_name(saved, name)).map(|(_, path)| path);
                found.ok_or_else(|| format!("No saved song \"{}\" (listSavedSongs gives them)", name.trim()))?
            }
            (None, Some(path)) => PathBuf::from(path.trim()),
            _ => return Err("Give name or filePath".into()),
        };
        let name = self.read_song_file(&path)?;
        let library = self.editor.library();
        Ok(object([
            ("name", Value::from(name)),
            ("patterns", count(library.patterns.len())),
            ("arrangements", count(library.songs.len())),
        ]))
    }

    /// Argument `kit` (a kit name, any case), else the kit in use.
    fn mcp_kit(&self, args: &Value) -> Result<usize, String> {
        match optional_text(args, "kit")?.filter(|k| !k.trim().is_empty()) {
            Some(wanted) => self
                .kits
                .kits
                .iter()
                .position(|k| same_name(&k.name, wanted))
                .ok_or_else(|| format!("No kit named \"{}\" (listKitSamples gives them)", wanted.trim())),
            None => Some(self.kit_index).filter(|&k| k < self.kits.kits.len()).ok_or_else(|| "No kit loaded".into()),
        }
    }

    /// Kit `k`, read from a folder or a mapping: the kits saved, its samples told.
    fn mcp_kit_added(&mut self, k: usize, message: String) -> Result<Value, String> {
        self.kits_changed(message);
        let kit = &self.kits.kits[k];
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
const NO_AUDIO: &str = "no audio device: orDrumbox plays nothing";

/// Argument `settings` (`required`, else it may be absent): synth preset
/// settings by path, checked against the synth editor's controls.
fn synth_settings(args: &Value, required: bool) -> Result<Vec<(String, Value)>, String> {
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
        } else {
            return Err(format!("{at}: not a synth setting"));
        };
        checked.push((path.to_owned(), value));
    }
    Ok(checked)
}

/// The answer of a tool that wrote a file.
fn exported(path: &Path, message: String) -> Result<Value, String> {
    Ok(object([("filePath", Value::from(path.display().to_string())), ("message", Value::from(message))]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kits::KitSet;
    use ordrumbox_core::json;

    fn session() -> Session {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0},{"beat":2}]}]},
                {"name":"jazz"}]}"#,
        )
        .unwrap();
        Session::new(library, KitSet::empty(), Vec::new())
    }

    fn call(session: &mut Session, name: &str, args: &str) -> Result<Value, String> {
        session.mcp_call(name, &json::parse(args).unwrap())
    }

    fn get<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
        path.iter().fold(value, |v, key| v.get(key).unwrap_or_else(|| panic!("no {key} in {v}")))
    }

    #[test]
    fn library_tools_run_on_the_session() {
        let mut s = session();
        let before = s.editor.library().clone();
        let args = r#"{"patternName":"rock","notes":[{"trackName":"KICK","step":4,"velocity":2}]}"#;
        // The note editor's ranges are the session's.
        assert_eq!(call(&mut s, "addNotesToPattern", args), Err("notes[0].velocity: 2 is outside 0 to 1".into()));
        let args = r#"{"patternName":"rock","notes":[{"trackName":"snare","step":4},{"trackName":"KICK","step":8}]}"#;
        call(&mut s, "addNotesToPattern", args).unwrap();
        assert_eq!(s.message, "Add 2 notes to \"rock\"");
        let undone = call(&mut s, "undo", "{}").unwrap();
        assert_eq!(get(&undone, &["message"]).as_str(), Some("undone: Add 2 notes to \"rock\""));
        assert_eq!(s.editor.library(), &before);
        call(&mut s, "redo", "{}").unwrap();
        assert_eq!(s.editor.library().patterns[0].tracks.len(), 2);
        assert_eq!(call(&mut s, "setColorScheme", r#"{"scheme":2}"#), Err("Unknown tool: setColorScheme".into()));
    }

    #[test]
    fn export_file_names_stay_in_their_folder() {
        let (library, _) = Library::parse(
            r#"{"patterns":[{"id":"/../../x","name":"a"}],"songs":[{"id":"..\\..\\y","name":"b","clips":[]}]}"#,
        )
        .unwrap();
        let s = Session::new(library, KitSet::empty(), Vec::new());
        assert_eq!(s.export_file_name_of(Export::PatternWav, 0, 0).as_deref(), Some("ordrumbox-_______x.wav"));
        assert_eq!(s.export_file_name_of(Export::PatternMidi, 0, 0).as_deref(), Some("ordrumbox-_______x.mid"));
        assert_eq!(s.export_file_name_of(Export::SongWav, 0, 0).as_deref(), Some("ordrumbox-song-______y.wav"));
    }

    #[test]
    fn patterns_are_shown_and_removed() {
        let mut s = session();
        call(&mut s, "createNewPattern", r#"{"patternName":"house"}"#).unwrap();
        let shown = call(&mut s, "selectPattern", r#"{"patternName":"house"}"#).unwrap();
        assert_eq!((s.pattern_index, get(&shown, &["name"]).as_str()), (2, Some("house")));
        call(&mut s, "selectPattern", r#"{"index":1}"#).unwrap();
        assert_eq!(s.pattern_index, 1);
        assert!(call(&mut s, "selectPattern", r#"{"index":3}"#).is_err());
        assert_eq!(get(&call(&mut s, "listPatterns", "{}").unwrap(), &["shown"]).as_f64(), Some(1.0));
        call(&mut s, "createArrangement", r#"{"name":"song","clips":[{"patternName":"jazz"},{"patternName":"rock"}]}"#)
            .unwrap();
        call(&mut s, "selectPattern", r#"{"patternName":"house"}"#).unwrap();
        let removed = call(&mut s, "removePattern", r#"{"patternName":"jazz"}"#).unwrap();
        assert_eq!(removed.to_json(), r#"{"removed":"jazz","clipsRemoved":1}"#);
        // The pattern shown stays the same one.
        assert_eq!(s.pattern().and_then(|p| p.name.as_deref()), Some("house"));
        assert_eq!(s.editor.library().songs[0].clips.len(), 1);
        call(&mut s, "removePattern", r#"{"patternName":"rock"}"#).unwrap();
        assert!(call(&mut s, "removePattern", r#"{"patternName":"house"}"#).is_err(), "the last pattern stays");
    }

    #[test]
    fn arrangements_are_selected_and_removed() {
        let mut s = session();
        call(&mut s, "createArrangement", r#"{"name":"A","clips":[{"patternName":"rock","measureCount":2}]}"#).unwrap();
        call(&mut s, "createArrangement", r#"{"name":"B"}"#).unwrap();
        assert_eq!(s.editor.selected_song(), 1);
        assert!(call(&mut s, "selectArrangement", "{}").is_err());
        let selected = call(&mut s, "selectArrangement", r#"{"arrangement":"a"}"#).unwrap();
        assert_eq!((s.editor.selected_song(), get(&selected, &["name"]).as_str()), (0, Some("A")));
        let info = call(&mut s, "setSongInfo", r#"{"name":" Live set ","description":"two parts"}"#).unwrap();
        assert_eq!(info.to_json(), r#"{"name":"Live set","description":"two parts"}"#);
        assert!(call(&mut s, "setSongInfo", r#"{"name":""}"#).is_err());
        let removed = call(&mut s, "removeArrangement", r#"{"arrangement":"B"}"#).unwrap();
        assert_eq!(removed.to_json(), r#"{"removed":"B","arrangements":1}"#);
    }

    #[test]
    fn the_master_is_read_and_set() {
        let mut s = session();
        let master = call(&mut s, "setMaster", r#"{"settings":{"volume":0.5,"compBypass":true}}"#).unwrap();
        assert_eq!(
            (get(&master, &["volume"]).as_f64(), get(&master, &["compBypass"])),
            (Some(0.5), &Value::Bool(true))
        );
        for refused in
            [r#"{"settings":{"volume":3}}"#, r#"{"settings":{"loudness":1}}"#, r#"{"settings":{"compBypass":1}}"#]
        {
            assert!(call(&mut s, "setMaster", refused).is_err(), "{refused}");
        }
        assert_eq!(call(&mut s, "getMaster", "{}").unwrap(), master);
        assert_eq!(s.editor.history().undo_len(), 0);
    }

    #[test]
    fn lists_exports_and_imports() {
        let mut s = session();
        assert_eq!(call(&mut s, "listSynthPresets", "{}").unwrap().to_json(), r#"{"count":0,"presets":[]}"#);
        let preset = r#"{"patternName":"rock","trackName":"KICK","updates":{"useSynth":true,"synthPreset":"B1"}}"#;
        assert!(call(&mut s, "updateTrack", preset).is_err(), "no such preset");
        assert!(get(&call(&mut s, "listScales", "{}").unwrap(), &["scales"]).as_array().is_some());
        assert!(get(&call(&mut s, "listStyles", "{}").unwrap(), &["styles"]).as_array().is_some());
        assert_eq!(call(&mut s, "exportPatternWav", r#"{"patternName":"rock"}"#), Err(NOTHING_TO_RENDER.into()));
        let path = std::env::temp_dir().join(format!("ordrumbox_mcp_{}.mid", std::process::id()));
        s.write_midi(0, &path).unwrap();
        let args = object([("filePath", Value::from(path.display().to_string()))]);
        let imported = s.mcp_call("importMidi", &args).unwrap();
        std::fs::remove_file(&path).unwrap();
        let patterns = get(&imported, &["patterns"]).as_array().unwrap();
        assert_eq!(get(&patterns[0], &["index"]).as_f64(), Some(2.0));
        assert_eq!(s.pattern_index, s.editor.library().patterns.len() - 1);
        assert!(call(&mut s, "importMidi", r#"{"filePath":"/no/such/file.mid"}"#).is_err());
    }

    #[test]
    fn synth_presets_are_designed() {
        let mut s = session();
        let args = r#"{"preset":"acid","settings":{"vco1.wave":"sawtooth","filter.freq":800,"filter.slope":24,
            "lfo.depth":0.3,"bypassFilter":false}}"#;
        let made = call(&mut s, "createSynthPreset", args).unwrap();
        assert_eq!(get(&made, &["settings", "vco1", "wave"]).as_str(), Some("sawtooth"));
        assert_eq!(get(&made, &["settings", "filter", "slope"]).as_f64(), Some(24.0));
        // Setting the LFO's depth aims it at the filter, as the editor does.
        assert_eq!(get(&made, &["settings", "lfo", "target"]).as_str(), Some("filter.freq"));
        assert!(call(&mut s, "createSynthPreset", r#"{"preset":"ACID"}"#).is_err(), "the key is taken");
        let refused = [
            r#"{"preset":"acid","settings":{"filter.freq":5}}"#,
            r#"{"preset":"acid","settings":{"vco1.octave":1.5}}"#,
            r#"{"preset":"acid","settings":{"vco1.wave":"saw"}}"#,
            r#"{"preset":"acid","settings":{"filter.slope":"24"}}"#,
            r#"{"preset":"acid","settings":{"vco9.gain":1}}"#,
            r#"{"preset":"acid","settings":{}}"#,
            r#"{"preset":"bass","settings":{"vco1.gain":1}}"#,
        ];
        for args in refused {
            assert!(call(&mut s, "updateSynthPreset", args).is_err(), "{args}");
        }
        let copy =
            call(&mut s, "createSynthPreset", r#"{"preset":"acid 2","from":"Acid","settings":{"vco2.wave":"square"}}"#);
        let copy = copy.unwrap();
        assert_eq!(get(&copy, &["settings", "filter", "freq"]).as_f64(), Some(800.0));
        assert_eq!(get(&copy, &["settings", "vco2", "gain"]).as_f64(), Some(0.5), "vco2 switched on");
        call(&mut s, "updateSynthPreset", r#"{"preset":"acid","settings":{"envelope.decay":0.2}}"#).unwrap();
        let read = call(&mut s, "getSynthPreset", r#"{"preset":"acid"}"#).unwrap();
        assert_eq!(get(&read, &["settings", "envelope", "decay"]).as_f64(), Some(0.2));
        let preset = r#"{"patternName":"rock","trackName":"KICK","updates":{"useSynth":true,"synthPreset":"acid"}}"#;
        call(&mut s, "updateTrack", preset).unwrap();
        let removed = call(&mut s, "removeSynthPreset", r#"{"preset":"acid"}"#).unwrap();
        assert_eq!(removed.to_json(), r#"{"removed":"acid","tracksUsing":["rock/KICK"]}"#);
        assert!(call(&mut s, "removeSynthPreset", r#"{"preset":"acid 2"}"#).is_err(), "the last preset stays");
        assert_eq!(s.synths.keys(), ["acid 2"]);
    }
    #[test]
    fn songs_kits_and_playback_without_files_or_sound() {
        let mut s = session();
        assert_eq!(call(&mut s, "getPlaybackState", "{}").unwrap().to_json(), r#"{"playing":false}"#);
        assert_eq!(call(&mut s, "auditionTrack", r#"{"patternName":"rock","trackName":"KICK"}"#), Err(NO_AUDIO.into()));
        // A session of the tests has no user data folder and no kit.
        assert!(call(&mut s, "saveSong", "{}").is_err());
        assert!(call(&mut s, "listSavedSongs", "{}").is_err());
        assert!(call(&mut s, "loadSong", "{}").is_err());
        assert!(call(&mut s, "loadSong", r#"{"filePath":"/no/such/song.odbox"}"#).is_err());
        assert!(call(&mut s, "normalizeKit", "{}").is_err());
        assert!(call(&mut s, "exportKitMapping", r#"{"kit":"808"}"#).is_err());
        assert!(call(&mut s, "importKitFolder", r#"{"folderPath":"/no/such/folder"}"#).is_err());
        assert!(call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"random"}"#).is_err());
        assert!(call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"synth"}"#).is_err());
        let kit = call(&mut s, "assignSounds", r#"{"patternName":"rock","sounds":"kit"}"#).unwrap();
        assert_eq!(get(&kit, &["tracks"]).to_json(), r#"[{"trackName":"KICK","sound":"kit"}]"#);
        assert_eq!(s.editor.history().undo_len(), 0);
    }

    #[test]
    fn a_song_file_is_loaded_by_path() {
        let mut s = session();
        let path = std::env::temp_dir().join(format!("ordrumbox_mcp_{}.odbox", std::process::id()));
        store::save_song(&path, &s.song_record("savedAt")).unwrap();
        call(&mut s, "removePattern", r#"{"patternName":"jazz"}"#).unwrap();
        let args = object([("filePath", Value::from(path.display().to_string()))]);
        let loaded = s.mcp_call("loadSong", &args).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(get(&loaded, &["patterns"]).as_f64(), Some(2.0));
        assert_eq!(s.editor.library().patterns.len(), 2);
    }
}
