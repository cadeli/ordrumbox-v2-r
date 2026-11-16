//! The MCP tools that run on the session, in groups: `mcp_call`
//! (`session_mcp.rs`) asks each group in turn, and a library tool of
//! `ordrumbox_logic::tools` answers what no group takes.

use ordrumbox_core::json::Value;
use ordrumbox_engine::synth_model as model;
use ordrumbox_logic::tools::{count, find_song, find_track, object, same_name, song_json, text};

use super::Session;
use super::session_mcp::{NO_AUDIO, exported, synth_settings};
use crate::picker::Export;
use crate::{master, store};
use ordrumbox_engine::Mode;
use std::path::Path;

impl Session {
    /// Files: the exports, MIDI import and the songs saved. `Ok(None)` when `name` is not one of them.
    pub(super) fn mcp_files_tool(&mut self, name: &str, args: &Value) -> Result<Option<Value>, String> {
        match name {
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
            "exportPatternStems" => {
                let p = self.mcp_pattern(text(args, "patternName")?)?;
                let path = self.mcp_export_path(Export::PatternStems, p, 0)?;
                exported(&path, self.write_stems(Mode::Pattern(p), &path)?)
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
            "exportArrangementStems" => {
                let s = find_song(&self.editor, args)?;
                let path = self.mcp_export_path(Export::SongStems, 0, s)?;
                exported(&path, self.write_stems(Mode::Song(s), &path)?)
            }
            "importMidi" => self.mcp_import_midi(args),
            "saveSong" => {
                let dir = self.saving.songs_dir.as_ref().ok_or("no user data folder to save to")?;
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
            _ => return Ok(None),
        }
        .map(Some)
    }

    /// Which pattern or arrangement is shown, and the arrangements. `Ok(None)` when `name` is not one of them.
    pub(super) fn mcp_arrangement_tool(&mut self, name: &str, args: &Value) -> Result<Option<Value>, String> {
        match name {
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
            "removeArrangement" => self.mcp_remove_arrangement(args),
            "selectArrangement" => {
                if args.get("arrangement").is_none_or(|a| *a == Value::Null) {
                    return Err("arrangement is required".into());
                }
                let s = find_song(&self.editor, args)?;
                self.select_song(s);
                Ok(song_json(self.editor.library(), s))
            }
            "setSongInfo" => self.mcp_set_song_info(args),
            _ => return Ok(None),
        }
        .map(Some)
    }

    /// The synth presets. `Ok(None)` when `name` is not one of them.
    pub(super) fn mcp_synth_tool(&mut self, name: &str, args: &Value) -> Result<Option<Value>, String> {
        match name {
            "getSynthPreset" => {
                let key = self.mcp_synth(text(args, "preset")?)?;
                let mut draft = self.synths.presets.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
                model::hydrate(&mut draft);
                Ok(object([("preset", Value::from(key)), ("settings", Value::Object(draft))]))
            }
            "createSynthPreset" => self.mcp_create_synth(args),
            "updateSynthPreset" => {
                let key = self.mcp_synth(text(args, "preset")?)?;
                let settings = synth_settings(args, true)?;
                let draft = self.synths.presets.get(&key).and_then(Value::as_object).cloned().unwrap_or_default();
                Ok(self.mcp_write_synth(&key, draft, &settings))
            }
            "removeSynthPreset" => self.mcp_remove_synth(args),
            "listSynthPresets" => {
                let keys = self.synths.presets.keys().into_iter().map(Value::from).collect::<Vec<_>>();
                Ok(object([("count", count(keys.len())), ("presets", Value::Array(keys))]))
            }
            _ => return Ok(None),
        }
        .map(Some)
    }

    /// The kits and their samples. `Ok(None)` when `name` is not one of them.
    pub(super) fn mcp_kit_tool(&mut self, name: &str, args: &Value) -> Result<Option<Value>, String> {
        match name {
            "listKitSamples" => Ok(self.mcp_kits()),
            "selectKit" => {
                let wanted = text(args, "kit")?;
                let found = self.kits.kits().iter().position(|k| same_name(&k.name, wanted));
                let index = found.ok_or_else(|| format!("No kit named \"{wanted}\" (listKitSamples gives them)"))?;
                self.use_kit(index);
                Ok(object([("kit", Value::from(self.kits.kits()[index].name.as_str()))]))
            }
            "analyzeSamples" => self.mcp_analyze(args),
            "normalizeKit" => {
                let k = self.mcp_kit(args)?;
                let n = self.kits.normalize(k)?;
                self.kits_changed(format!("Normalized {n} sample(s)"));
                Ok(object([("kit", Value::from(self.kits.kits()[k].name.as_str())), ("normalized", count(n))]))
            }
            "addSampleToKit" => {
                let k = self.mcp_kit(args)?;
                let (sample, message) = self.kits.add_sample(k, Path::new(text(args, "filePath")?.trim()))?;
                self.kits_changed(message);
                let added = &self.kits.kits()[k].samples[sample];
                Ok(object([
                    ("kit", Value::from(self.kits.kits()[k].name.as_str())),
                    ("key", Value::from(added.key.as_str())),
                    ("path", Value::from(added.path.as_str())),
                ]))
            }
            "replaceKitSample" => {
                let k = self.mcp_kit(args)?;
                let path = text(args, "samplePath")?.trim();
                let sample = self.kits.kits()[k].samples.iter().position(|s| s.path == path);
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
            _ => return Ok(None),
        }
        .map(Some)
    }

    /// Playback, the master and the sounds of the tracks. `Ok(None)` when `name` is not one of them.
    pub(super) fn mcp_sound_tool(&mut self, name: &str, args: &Value) -> Result<Option<Value>, String> {
        match name {
            "getMaster" => Ok(master::to_json(&self.output_panel.master())),
            "setMaster" => self.mcp_set_master(args),
            "play" => self.mcp_play(args),
            "stop" => {
                self.playback.stop();
                self.message = "stopped".into();
                Ok(object([("playing", Value::Bool(false))]))
            }
            "assignSounds" => self.mcp_assign_sounds(args),
            "auditionTrack" => {
                let (p, t) = find_track(self.editor.library(), args)?;
                let controller = self.playback.controller_mut().ok_or(NO_AUDIO)?;
                controller.preview(self.editor.library(), p, t, None);
                Ok(object([("trackName", Value::from(self.editor.library().patterns[p].tracks[t].name.as_str()))]))
            }
            "getPlaybackState" => Ok(self.mcp_playback_state()),
            _ => return Ok(None),
        }
        .map(Some)
    }
}
