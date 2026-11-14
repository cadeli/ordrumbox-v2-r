//! The Model Context Protocol server (`ordrumboxMcpserver.mjs` of the JS
//! app): JSON-RPC 2.0 messages, one per line, and the tools an AI client may
//! call. Unlike the JS server, which edited `song.json` on its own, the tools
//! act on the running app: `ordrumbox --mcp` (`mcp_link.rs`) answers the
//! protocol and passes each tool call to the window, where the session runs
//! it (`session_mcp.rs`) as one undo step.
//!
//! Tool and parameter names are the JS server's; the track and note settings
//! are named by their keys in the files this app writes (their `json_record!` tables).

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Note, Track};
use ordrumbox_engine::synth_model;
use ordrumbox_logic::tools::object;

use crate::editors::track_select_options;
use crate::update;

/// Protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// JSON-RPC error codes.
const PARSE_ERROR: f64 = -32700.0;
const INVALID_REQUEST: f64 = -32600.0;
const METHOD_NOT_FOUND: f64 = -32601.0;
const INVALID_PARAMS: f64 = -32602.0;

/// What the client is told about the server at `initialize`.
const INSTRUCTIONS: &str = "Controls the orDrumbox window running on this computer: every edit is \
shown at once, is one step of Edit > Undo, and is saved with the library. Patterns are named by \
their name (any case) and hold tracks named by their instrument (KICK, SNARE, CHH...: \
listAllInstrumentsNames); a note's place is an absolute step, beat * stepsPerBeat + step. \
Write patterns yourself, note by note (addNotesToPattern, updateTrack). orDrumbox's automatic \
generation (generatePattern, regeneratePattern, regenerateTrack, randomizeTrack, toggleGeneration, \
togglePatternAuto, setPatternMacro, and a track's autoMode) is only for when the user explicitly \
asks for it, in words such as \"generate\", \"auto mode\" or \"random\": a request for a style, a \
groove or a song is not such a request.";

/// The tools of orDrumbox's automatic generation, which a client calls
/// only when the user asks for generation ([`GENERATION_RULE`]).
const GENERATION_TOOLS: [&str; 7] = [
    "generatePattern",
    "regeneratePattern",
    "regenerateTrack",
    "randomizeTrack",
    "toggleGeneration",
    "togglePatternAuto",
    "setPatternMacro",
];
/// What the description of each generation tool, and of a track's
/// `autoMode`, starts with.
const GENERATION_RULE: &str = "Automatic generation: call only when the user explicitly asks for generation, \
auto mode or random notes, never to fulfil a request for a style, a groove or a song (write the notes instead).";

/// Track settings a tool may not set: the track is named by `trackName`.
const NOT_EDITED: [&str; 1] = ["name"];
/// Note fields that are not settings: `step` gives the place.
const NOTE_PLACE: [&str; 2] = ["beat", "step"];

/// The tools, without the track and note settings (filled in by [`tools`]).
const TOOLS: &str = r#"[
  {"name": "listPatterns", "description": "The session tempo, the patterns of the library, in order, and the one shown.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "loadPattern", "description": "A pattern with its tracks (every setting) and notes (fields away from their default, and their absolute step).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string", "minLength": 1}}, "required": ["patternName"]}},
  {"name": "createNewPattern", "description": "Adds an empty pattern (4 beats, no track) at the end of the library; addNotesToPattern or updateTrack add its tracks.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string", "minLength": 1}}, "required": ["patternName"]}},
  {"name": "selectPattern", "description": "Shows a pattern in the window (by name or index; a playing pattern switches at once). With neither, tells which one is shown.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "index": {"type": "integer", "minimum": 0}}}},
  {"name": "addNotesToPattern", "description": "Adds notes, or changes the note already at their step. A track missing from the pattern is added; a pattern too short grows to the next multiple of 4 beats (16 at most), and a track that looped over its whole length keeps doing so.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "notes": {"type": "array", "items": {"type": "object", "properties": {
       "trackName": {"type": "string", "description": "Instrument name (KICK, SNARE...)"},
       "step": {"type": "integer", "minimum": 0, "description": "Absolute step, 0-based: beat * stepsPerBeat + step in the beat"}},
       "required": ["trackName", "step"]}}},
     "required": ["patternName", "notes"]}},
  {"name": "deleteNotes", "description": "Deletes every note at the given steps of the given tracks.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "notes": {"type": "array", "items": {"type": "object", "properties": {
       "trackName": {"type": "string", "description": "Instrument name (KICK, SNARE...)"},
       "step": {"type": "integer", "minimum": 0, "description": "Absolute step, 0-based: beat * stepsPerBeat + step in the beat"}},
       "required": ["trackName", "step"]}}},
     "required": ["patternName", "notes"]}},
  {"name": "removeTrack", "description": "Removes a track and its notes from a pattern; the last track stays.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "trackName": {"type": "string"}}, "required": ["patternName", "trackName"]}},
  {"name": "copyTrack", "description": "Copies a track (settings and notes) to the end of a pattern, the same one or another.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "trackName": {"type": "string"},
     "toPatternName": {"type": "string", "description": "Destination pattern (default: patternName)"},
     "newName": {"type": "string", "description": "Name of the copy (default: the track's; required when the destination has a track of that name)"}},
     "required": ["patternName", "trackName"]}},
  {"name": "updateTrack", "description": "Changes the settings of a track (added when the pattern has none of that name) and, with noteUpdates, of all its notes. Numbers outside a setting's range are brought inside it: the answer gives the values used.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "trackName": {"type": "string"},
     "updates": {"type": "object", "properties": {}, "additionalProperties": false},
     "noteUpdates": {"type": "object", "properties": {}, "additionalProperties": false}},
     "required": ["patternName", "trackName", "updates"]}},
  {"name": "setBpm", "description": "The session tempo every pattern plays at, whole, 20 to 300 (an arrangement plays at its own: updateArrangement).",
   "inputSchema": {"type": "object", "properties": {"bpm": {"type": "number", "minimum": 20, "maximum": 300}}, "required": ["bpm"]}},
  {"name": "setPatternBeatCount", "description": "The length of a pattern in beats; every track follows.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "beatCount": {"type": "integer", "minimum": 1, "maximum": 16}}, "required": ["patternName", "beatCount"]}},
  {"name": "setPatternTags", "description": "The tags (genre, mood...) of a pattern.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}}, "required": ["patternName", "tags"]}},
  {"name": "setPatternDescription", "description": "The description of a pattern.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "description": {"type": "string"}}, "required": ["patternName", "description"]}},
  {"name": "renamePattern", "description": "Renames a pattern; its id, which arrangements refer to, stays.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "newName": {"type": "string", "minLength": 1}}, "required": ["patternName", "newName"]}},
  {"name": "duplicatePattern", "description": "Adds a copy of a pattern at the end of the library, named newName (default: \"<name> copy\").",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "newName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "removePattern", "description": "Removes a pattern and its clips in every arrangement; the last pattern stays.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "savePatternToJson", "description": "Exports a pattern as JSON to the exports folder of the user data folder (as File > Export pattern).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "exportPatternWav", "description": "Renders a pattern once to a WAV file in the exports folder of the user data folder (as File > Export pattern as WAV).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "exportPatternMidi", "description": "Exports a pattern as a MIDI file to the exports folder of the user data folder (as File > Export pattern as MIDI).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "importMidi", "description": "New patterns from a MIDI file (as File > Import MIDI file); the last one is shown.",
   "inputSchema": {"type": "object", "properties": {"filePath": {"type": "string", "minLength": 1, "description": "Absolute path of a .mid file"}}, "required": ["filePath"]}},
  {"name": "listStyles", "description": "The styles of styles.json and those learned from the library's tags: meter, tempo range, swing, tracks.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listScales", "description": "The scales: name and degrees (semitones from the root), as a note's arp or pitch can use them.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listSynthPresets", "description": "The synth presets a track plays with updateTrack {useSynth: true, synthPreset: <key>}.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listAllInstrumentsNames", "description": "The instruments a track can be named after, with whether each is a drum and its default pan.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listKitSamples", "description": "The kits and their samples (instrument key and path), and the kit in use.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "selectKit", "description": "Plays the library with a kit (by name, any case): every track's sound is assigned again.",
   "inputSchema": {"type": "object", "properties": {"kit": {"type": "string", "minLength": 1}}, "required": ["kit"]}},
  {"name": "getSynthPreset", "description": "Every setting of a synth preset, by group (vco1, filter, envelope...).",
   "inputSchema": {"type": "object", "properties": {"preset": {"type": "string", "minLength": 1}}, "required": ["preset"]}},
  {"name": "createSynthPreset", "description": "A new synth preset: the default one (a sine) or a copy of another, then settings by path (\"filter.freq\"). Not an undo step.",
   "inputSchema": {"type": "object", "properties": {
     "preset": {"type": "string", "minLength": 1, "description": "Key of the new preset"},
     "from": {"type": "string", "description": "Preset to copy (default: the default preset)"},
     "settings": {"type": "object", "properties": {}, "additionalProperties": false}},
     "required": ["preset"]}},
  {"name": "updateSynthPreset", "description": "Changes settings of a synth preset by path (\"filter.freq\", \"vco2.wave\"); every track playing it hears it. A group off is switched on when one of its controls is set, as in the synth editor. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {
     "preset": {"type": "string", "minLength": 1},
     "settings": {"type": "object", "properties": {}, "additionalProperties": false}},
     "required": ["preset", "settings"]}},
  {"name": "removeSynthPreset", "description": "Removes a synth preset; the last one stays. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {"preset": {"type": "string", "minLength": 1}}, "required": ["preset"]}},
  {"name": "analyzeSamples", "description": "Length, peak and RMS levels and pitch of samples, by their path in listKitSamples.",
   "inputSchema": {"type": "object", "properties": {"samples": {"type": "array", "items": {"type": "string"}}}, "required": ["samples"]}},
  {"name": "listArrangements", "description": "The arrangements (songs) with their clips, and the one selected. Measures are 0-based.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "createArrangement", "description": "Adds an arrangement, selects it and fills it with clips. A clip lasts as long as its pattern unless measureCount says otherwise.",
   "inputSchema": {"type": "object", "properties": {
     "name": {"type": "string", "minLength": 1},
     "description": {"type": "string"},
     "bpm": {"type": "number", "minimum": 20, "maximum": 300, "description": "Tempo of the whole arrangement"},
     "loopMeasures": {"type": "number", "minimum": 0, "description": "Loop length in measures (0: up to the end of the last clip)"},
     "clips": {"type": "array", "items": {"type": "object", "properties": {
       "patternName": {"type": "string", "description": "Pattern name or id"},
       "startMeasure": {"type": "integer", "minimum": 0},
       "measureCount": {"type": "number", "exclusiveMinimum": 0}},
       "required": ["patternName"]}}},
     "required": ["name"]}},
  {"name": "addPatternToArrangement", "description": "Places a pattern in an arrangement at a measure. Clips may overlap; a pattern may be placed several times.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string", "description": "Pattern name or id"},
     "startMeasure": {"type": "integer", "minimum": 0, "description": "0-based measure (default 0)"},
     "measureCount": {"type": "number", "exclusiveMinimum": 0, "description": "Clip length (default: the pattern's)"},
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}},
     "required": ["patternName"]}},
  {"name": "removePatternFromArrangement", "description": "Removes from an arrangement every clip starting at startMeasure, every clip of patternName, or both.",
   "inputSchema": {"type": "object", "properties": {
     "startMeasure": {"type": "integer", "minimum": 0},
     "patternName": {"type": "string", "description": "Pattern name or id"},
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}}}},
  {"name": "updateArrangement", "description": "Changes the tempo, description or loop length of an arrangement; what is not given stays.",
   "inputSchema": {"type": "object", "properties": {
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"},
     "description": {"type": "string"},
     "bpm": {"type": "number", "minimum": 20, "maximum": 300, "description": "Tempo of the whole arrangement"},
     "loopMeasures": {"type": "number", "minimum": 0, "description": "Loop length in measures (0: up to the end of the last clip)"}}}},
  {"name": "removeArrangement", "description": "Removes an arrangement and its clips (its patterns stay).",
   "inputSchema": {"type": "object", "properties": {
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}}}},
  {"name": "renameArrangement", "description": "Renames an arrangement; its id stays.",
   "inputSchema": {"type": "object", "properties": {
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"},
     "newName": {"type": "string", "minLength": 1}},
     "required": ["newName"]}},
  {"name": "moveClip", "description": "Moves the clip starting at startMeasure to start at toMeasure.",
   "inputSchema": {"type": "object", "properties": {
     "startMeasure": {"type": "integer", "minimum": 0},
     "toMeasure": {"type": "integer", "minimum": 0},
     "patternName": {"type": "string", "description": "Pattern name or id, when several clips start at startMeasure"},
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}},
     "required": ["startMeasure", "toMeasure"]}},
  {"name": "exportArrangementWav", "description": "Renders an arrangement whole to a WAV file in the exports folder of the user data folder (as File > Export song as WAV).",
   "inputSchema": {"type": "object", "properties": {
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}}}},
  {"name": "getMaster", "description": "The master bus settings: volume, pre-gain, compressor, low and high cut.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "setMaster", "description": "Changes master bus settings; what is not given stays. Saved with the settings, not an undo step.",
   "inputSchema": {"type": "object", "properties": {
     "settings": {"type": "object", "properties": {"compBypass": {"type": "boolean", "description": "Skips pre-gain, compressor and filters"}}, "additionalProperties": false}},
     "required": ["settings"]}},
  {"name": "play", "description": "Plays the pattern shown, or with song true the selected arrangement from its start.",
   "inputSchema": {"type": "object", "properties": {"song": {"type": "boolean"}}}},
  {"name": "stop", "description": "Stops playing.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "undo", "description": "Undoes the last edit, made in the window or by a tool.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "redo", "description": "Redoes the last edit undone.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "renameTrack", "description": "Renames a track (an instrument name: KICK, SNARE...); its sound follows the name when it is auto-assigned.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"}, "trackName": {"type": "string"}, "newName": {"type": "string", "minLength": 1}},
     "required": ["patternName", "trackName", "newName"]}},
  {"name": "compactPattern", "description": "Shortens every track of a pattern to its shortest loop, when its notes repeat one (as Pattern > Compact tracks).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "generatePattern", "description": "Adds a pattern made in a style (listStyles; default: a drawn one): its tracks, meter and lines, the session tempo drawn in the style's range (as Pattern > New pattern in the style).",
   "inputSchema": {"type": "object", "properties": {"style": {"type": "string"}}}},
  {"name": "regeneratePattern", "description": "Makes a pattern again in a style (default: its own, else a drawn one); locked tracks (composeLock) stay as they are.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "style": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "toggleGeneration", "description": "Drums, bass or chords of a pattern generated, their tracks in auto mode (a new line every loop), or that auto mode off; the answer tells which.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "part": {"type": "string", "enum": ["drums", "bass", "chords"]},
     "style": {"type": "string", "description": "For drums: the style (default: the pattern's)"}},
     "required": ["patternName", "part"]}},
  {"name": "togglePatternAuto", "description": "Every unlocked track of a pattern in auto mode (a new line every loop), or all out of it; the answer tells which.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "setPatternMacro", "description": "A generation macro of a pattern (0 to 100, 50 changes nothing); its lines are made again from its seed.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "macro": {"type": "string", "enum": ["energy", "complexity", "variation"]},
     "value": {"type": "number", "minimum": 0, "maximum": 100}},
     "required": ["patternName", "macro", "value"]}},
  {"name": "regenerateTrack", "description": "A new line on a track, in the pattern's style; a locked track (composeLock) is refused.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "trackName": {"type": "string"}}, "required": ["patternName", "trackName"]}},
  {"name": "randomizeTrack", "description": "Random notes on a track, in place of its notes.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "trackName": {"type": "string"}}, "required": ["patternName", "trackName"]}},
  {"name": "assignSounds", "description": "The sound of every track of a pattern: kit (the kit's sample for its name), synth (the synth preset matching its name, else a random one) or random (a random sample of any kit).",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"}, "sounds": {"type": "string", "enum": ["kit", "synth", "random"]}},
     "required": ["patternName", "sounds"]}},
  {"name": "auditionTrack", "description": "Plays a track's sound once, as Q-P in the window.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "trackName": {"type": "string"}}, "required": ["patternName", "trackName"]}},
  {"name": "getPlaybackState", "description": "Whether the app plays, what (a pattern or an arrangement) and where: the loop of a pattern and its beat, the measure of an arrangement.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "selectArrangement", "description": "Selects an arrangement (by name or index): the one play song plays and the arrangement tools edit by default. A song playing switches to it.",
   "inputSchema": {"type": "object", "properties": {
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index"}},
     "required": ["arrangement"]}},
  {"name": "repeatClip", "description": "Repeats the clip starting at startMeasure right after itself, as long.",
   "inputSchema": {"type": "object", "properties": {
     "startMeasure": {"type": "integer", "minimum": 0},
     "arrangement": {"type": ["string", "integer"], "description": "Arrangement name or index (default: the selected one)"}},
     "required": ["startMeasure"]}},
  {"name": "setSongInfo", "description": "The name and description of the song: the library as saveSong writes it. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {"name": {"type": "string", "minLength": 1}, "description": {"type": "string"}}}},
  {"name": "saveSong", "description": "Saves the song (patterns, arrangements) to the songs folder of the user data folder as <name>.odbox (as Song > Save).",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "exportSong", "description": "Exports the song as <name>.odbox to the exports folder of the user data folder (as Song > Export song).",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listSavedSongs", "description": "The songs saved in the songs folder, by name.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "loadSong", "description": "Puts a song file in the place of the library, one undo step: a saved song by name (listSavedSongs), or an .odbox file by path.",
   "inputSchema": {"type": "object", "properties": {"name": {"type": "string"}, "filePath": {"type": "string"}}}},
  {"name": "normalizeKit", "description": "Brings every sample of a kit to a 0 dB peak, on disk. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {"kit": {"type": "string", "description": "Kit name (default: the kit in use)"}}}},
  {"name": "addSampleToKit", "description": "Adds a WAV file to a kit, its instrument guessed from its file name. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {
     "filePath": {"type": "string"}, "kit": {"type": "string", "description": "Kit name (default: the kit in use)"}},
     "required": ["filePath"]}},
  {"name": "replaceKitSample", "description": "New audio, from a WAV file, for a sample of a kit (by its path in listKitSamples). Not an undo step.",
   "inputSchema": {"type": "object", "properties": {
     "samplePath": {"type": "string"}, "filePath": {"type": "string"},
     "kit": {"type": "string", "description": "Kit name (default: the kit in use)"}},
     "required": ["samplePath", "filePath"]}},
  {"name": "importKitFolder", "description": "The WAV files of a folder as a kit named after it, replacing a kit of that name. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {"folderPath": {"type": "string"}}, "required": ["folderPath"]}},
  {"name": "loadKitMapping", "description": "A kit mapping file (JSON, as exportKitMapping writes), replacing the kit of its name. Not an undo step.",
   "inputSchema": {"type": "object", "properties": {"filePath": {"type": "string"}}, "required": ["filePath"]}},
  {"name": "exportKitMapping", "description": "Exports a kit's mapping (no audio) as JSON to the exports folder of the user data folder.",
   "inputSchema": {"type": "object", "properties": {"kit": {"type": "string", "description": "Kit name (default: the kit in use)"}}}}
]"#;

/// The schema of a setting whose default is `default`: its type, its range,
/// its choices.
fn setting_schema(key: &str, default: &Value, range: Option<(f64, f64)>) -> Value {
    let types = |names: &[&str]| Value::Array(names.iter().map(|n| Value::from(*n)).collect());
    let mut schema = Object::new();
    match default {
        Value::Bool(_) => _ = schema.insert("type", Value::from("boolean")),
        Value::Number(_) => {
            schema.insert("type", Value::from("number"));
            if let Some((min, max)) = range {
                schema.insert("minimum", Value::from(min));
                schema.insert("maximum", Value::from(max));
            }
        }
        Value::String(_) => {
            schema.insert("type", Value::from("string"));
            let (choices, _) = track_select_options(key);
            if !choices.is_empty() {
                schema.insert("enum", Value::Array(choices.into_iter().map(Value::from).collect()));
            }
        }
        // Unset by default, a number when set: the compose settings, the variation amounts.
        _ if range.is_some() => {
            schema.insert("type", types(&["number", "null"]));
            if let Some((min, max)) = range {
                schema.insert("minimum", Value::from(min));
                schema.insert("maximum", Value::from(max));
            }
        }
        // Unset by default: an LFO (`{type, freq, min, max, phase}`), a synth
        // preset key, an arpeggio (intervals, "0,4,7", a direction, octaves 1-4); null unsets.
        _ if key.ends_with("Lfo") => _ = schema.insert("type", types(&["object", "null"])),
        _ if key == "arp" => _ = schema.insert("type", types(&["string", "array", "object", "null"])),
        _ => _ = schema.insert("type", types(&["string", "null"])),
    }
    Value::Object(schema)
}

/// The track settings a tool may set, as schema properties.
fn track_properties() -> Value {
    let defaults = Track::default();
    let keys = Track::KEYS.iter().filter(|k| Track::is_setting(k) && !NOT_EDITED.contains(k));
    let entries = keys.map(|&key| {
        let default = defaults.field(key).unwrap_or_default();
        let mut schema = setting_schema(key, &default, Track::value_range(key));
        if key == "autoMode"
            && let Some(schema) = schema.as_object_mut()
        {
            schema.insert("description", Value::from(format!("{GENERATION_RULE} A new line every loop.")));
        }
        (key, schema)
    });
    object(entries)
}

/// Start the description of tool `tool` with [`GENERATION_RULE`].
fn mark_generation(tool: &mut Value) {
    let Some(tool) = tool.as_object_mut() else { return };
    let description = tool.get("description").and_then(Value::as_str).unwrap_or_default();
    let marked = format!("{GENERATION_RULE} {description}");
    tool.insert("description", Value::from(marked));
}

/// The note fields a tool may set, as schema properties.
fn note_properties() -> Value {
    let defaults = Note::default();
    let keys = Note::KEYS.iter().filter(|k| !NOTE_PLACE.contains(k));
    object(keys.map(|&key| {
        let default = defaults.field(key).unwrap_or_default();
        (key, setting_schema(key, &default, crate::editors::note_range(key)))
    }))
}

/// The master bus settings a tool may set (but `compBypass`), with the
/// ranges of their knobs.
fn master_properties() -> Value {
    object(crate::output::params().map(|param| {
        let schema = object([
            ("type", Value::from("number")),
            ("minimum", Value::from(param.spec.min)),
            ("maximum", Value::from(param.spec.max)),
        ]);
        (param.key, schema)
    }))
}

/// The synth preset settings a tool may set, by path: the knobs with their
/// ranges, the lists with their values, the bypass flags.
fn synth_properties() -> Value {
    let knobs = synth_model::metas().map(|meta| {
        let kind = if meta.step >= 1.0 { "integer" } else { "number" };
        let schema = object([
            ("type", Value::from(kind)),
            ("minimum", Value::from(meta.min)),
            ("maximum", Value::from(meta.max)),
        ]);
        (meta.path, schema)
    });
    let lists = synth_model::CHOICE_PATHS.iter().map(|&path| {
        let values = synth_model::choices(path).into_iter().flatten();
        let values = values.map(|value| synth_model::choice_value(path, value)).collect();
        (path, object([("enum", Value::Array(values))]))
    });
    let flags = synth_model::BYPASS_FLAGS.iter().map(|(_, flag)| (*flag, object([("type", Value::from("boolean"))])));
    object(knobs.chain(lists).chain(flags))
}

/// Merge `properties` into the object at `path` of `value`.
fn add_properties(value: &mut Value, path: &[&str], properties: &Value) {
    let mut at = value;
    for key in path {
        let Some(next) = at.as_object_mut().and_then(|o| o.get_mut(key)) else { return };
        at = next;
    }
    let (Some(target), Some(source)) = (at.as_object_mut(), properties.as_object()) else { return };
    for (key, value) in source.iter() {
        target.insert(key, value.clone());
    }
}

/// The tool list of `tools/list`.
#[expect(clippy::expect_used, reason = "a constant of this file, parsed by a test")]
pub fn tools() -> Value {
    let mut tools = json::parse(TOOLS).expect("the tool list parses");
    let (track, note) = (track_properties(), note_properties());
    for tool in tools.as_array_mut().into_iter().flatten() {
        let name = tool.get("name").and_then(Value::as_str).unwrap_or_default().to_owned();
        let schema = ["inputSchema", "properties"];
        match name.as_str() {
            "updateTrack" => {
                add_properties(tool, &[schema[0], schema[1], "updates", "properties"], &track);
                add_properties(tool, &[schema[0], schema[1], "noteUpdates", "properties"], &note);
            }
            "addNotesToPattern" => add_properties(tool, &[schema[0], schema[1], "notes", "items", "properties"], &note),
            "createSynthPreset" | "updateSynthPreset" => {
                add_properties(tool, &[schema[0], schema[1], "settings", "properties"], &synth_properties());
            }
            "setMaster" => {
                add_properties(tool, &[schema[0], schema[1], "settings", "properties"], &master_properties())
            }
            name if GENERATION_TOOLS.contains(&name) => mark_generation(tool),
            _ => {}
        }
    }
    tools
}

/// A JSON-RPC answer to request `id`.
fn reply(id: &Value, outcome: Result<Value, (f64, String)>) -> String {
    let mut message = Object::new();
    message.insert("jsonrpc", Value::from("2.0"));
    message.insert("id", id.clone());
    match outcome {
        Ok(result) => _ = message.insert("result", result),
        Err((code, text)) => {
            _ = message.insert("error", object([("code", Value::from(code)), ("message", Value::from(text))]));
        }
    }
    Value::Object(message).to_json()
}

/// The result of a tool call: what the tool returned, or why it failed, as text.
fn tool_result(outcome: Result<Value, String>) -> Value {
    let (text, failed) = match outcome {
        Ok(value) => (value.to_json(), false),
        Err(message) => (message, true),
    };
    let content = object([("type", Value::from("text")), ("text", Value::from(text))]);
    object([("content", Value::Array(vec![content])), ("isError", Value::Bool(failed))])
}

/// The answer to `initialize`: the client's protocol version when this
/// server speaks it, else the newest.
fn initialize(params: Option<&Value>) -> Value {
    let asked = params.and_then(|p| p.get("protocolVersion")).and_then(Value::as_str);
    let version = asked.filter(|v| PROTOCOL_VERSIONS.contains(v)).unwrap_or(PROTOCOL_VERSIONS[0]);
    object([
        ("protocolVersion", Value::from(version)),
        ("capabilities", object([("tools", Value::Object(Object::new()))])),
        ("serverInfo", object([("name", Value::from("ordrumbox")), ("version", Value::from(update::CURRENT))])),
        ("instructions", Value::from(INSTRUCTIONS)),
    ])
}

/// The answer to one message (`None` for a notification, which gets
/// none); `call` runs a tool, by name, on its arguments.
pub fn answer(line: &str, call: &mut dyn FnMut(&str, &Value) -> Result<Value, String>) -> Option<String> {
    let message = match json::parse(line) {
        Ok(message) => message,
        Err(e) => return Some(reply(&Value::Null, Err((PARSE_ERROR, format!("not JSON: {e}"))))),
    };
    let method = message.get("method").and_then(Value::as_str);
    let Some(id) = message.get("id") else {
        // A notification (`notifications/initialized`, a cancellation) or a
        // client's answer: nothing to say.
        return None;
    };
    let Some(method) = method else {
        return Some(reply(id, Err((INVALID_REQUEST, "no method".into()))));
    };
    let params = message.get("params");
    let outcome = match method {
        "initialize" => Ok(initialize(params)),
        "ping" => Ok(Value::Object(Object::new())),
        "tools/list" => Ok(object([("tools", tools())])),
        "tools/call" => match params.and_then(|p| p.get("name")).and_then(Value::as_str) {
            Some(name) => {
                let empty = Value::Object(Object::new());
                let arguments = params.and_then(|p| p.get("arguments")).filter(|a| !a.is_null()).unwrap_or(&empty);
                if arguments.as_object().is_some() {
                    Ok(tool_result(call(name, arguments)))
                } else {
                    Err((INVALID_PARAMS, "arguments: not an object".into()))
                }
            }
            None => Err((INVALID_PARAMS, "no tool name".into())),
        },
        _ => Err((METHOD_NOT_FOUND, format!("unknown method {method}"))),
    };
    Some(reply(id, outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(line: &str) -> Value {
        let mut call = |name: &str, args: &Value| match name {
            "echo" => Ok(args.clone()),
            _ => Err(format!("Unknown tool: {name}")),
        };
        json::parse(&answer(line, &mut call).unwrap()).unwrap()
    }

    #[test]
    fn the_tool_list_parses_with_the_settings() {
        let tools = tools();
        let tools = tools.as_array().unwrap();
        assert_eq!(tools.len(), 71);
        let update = tools.iter().find(|t| t.get("name").and_then(Value::as_str) == Some("updateTrack")).unwrap();
        let updates = &update.get("inputSchema").unwrap().get("properties").unwrap().get("updates").unwrap();
        let properties = updates.get("properties").unwrap();
        let volume = properties.get("volume").unwrap();
        assert_eq!(volume.to_json(), r#"{"type":"number","minimum":0,"maximum":1}"#);
        assert_eq!(properties.get("mute").unwrap().to_json(), r#"{"type":"boolean"}"#);
        assert_eq!(properties.get("volumeLfo").unwrap().to_json(), r#"{"type":["object","null"]}"#);
        assert!(properties.get("delayType").unwrap().get("enum").is_some());
        assert!(properties.get("name").is_none());
        assert!(properties.get("velocity").is_none(), "web-version keys are not offered");
        let notes = update.get("inputSchema").unwrap().get("properties").unwrap().get("noteUpdates").unwrap();
        let cycle = notes.get("properties").unwrap().get("cycle").unwrap();
        assert_eq!(cycle.to_json(), r#"{"type":"number","minimum":1,"maximum":16}"#);
        assert!(notes.get("properties").unwrap().get("beat").is_none());
    }

    #[test]
    fn generation_is_only_for_when_the_user_asks_for_it() {
        let tools = tools();
        let tools = tools.as_array().unwrap();
        let description = |tool: &Value| tool.get("description").and_then(Value::as_str).unwrap().to_owned();
        for tool in tools {
            let name = tool.get("name").and_then(Value::as_str).unwrap();
            let marked = description(tool).starts_with(GENERATION_RULE);
            assert_eq!(marked, GENERATION_TOOLS.contains(&name), "{name}");
        }
        let named = |name: &str| tools.iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name));
        assert!(GENERATION_TOOLS.iter().all(|name| named(name)), "a generation tool is gone");
        assert!(GENERATION_TOOLS.iter().all(|name| INSTRUCTIONS.contains(name)));
        let update = tools.iter().find(|t| t.get("name").and_then(Value::as_str) == Some("updateTrack")).unwrap();
        let updates = update.get("inputSchema").unwrap().get("properties").unwrap().get("updates").unwrap();
        let auto_mode = updates.get("properties").unwrap().get("autoMode").unwrap();
        assert!(description(auto_mode).starts_with(GENERATION_RULE));
    }

    #[test]
    fn initialize_agrees_on_a_version() {
        let answer = run(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#);
        let result = answer.get("result").unwrap();
        assert_eq!(result.get("protocolVersion").unwrap().as_str(), Some("2024-11-05"));
        assert_eq!(answer.get("id").unwrap().as_f64(), Some(1.0));
        let answer = run(r#"{"jsonrpc":"2.0","id":"a","method":"initialize","params":{"protocolVersion":"1999"}}"#);
        assert_eq!(answer.get("result").unwrap().get("protocolVersion").unwrap().as_str(), Some(PROTOCOL_VERSIONS[0]));
        assert_eq!(answer.get("id").unwrap().as_str(), Some("a"));
    }

    #[test]
    fn calls_and_errors() {
        let ok = run(r#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"echo","arguments":{"a":1}}}"#);
        let result = ok.get("result").unwrap();
        assert_eq!(result.get("isError").unwrap().as_bool(), Some(false));
        assert_eq!(
            result.get("content").unwrap().as_array().unwrap()[0].get("text").unwrap().as_str(),
            Some(r#"{"a":1}"#)
        );
        let failed = run(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"nope"}}"#);
        let result = failed.get("result").unwrap();
        assert_eq!(result.get("isError").unwrap().as_bool(), Some(true));
        assert_eq!(
            result.get("content").unwrap().as_array().unwrap()[0].get("text").unwrap().as_str(),
            Some("Unknown tool: nope")
        );
        let code = |v: &Value| v.get("error").unwrap().get("code").unwrap().as_f64();
        assert_eq!(code(&run(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#)), Some(METHOD_NOT_FOUND));
        assert_eq!(code(&run("{not json")), Some(PARSE_ERROR));
        let bad = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"echo","arguments":[]}}"#;
        assert_eq!(code(&run(bad)), Some(INVALID_PARAMS));
        assert_eq!(run(r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#).get("result").unwrap().to_json(), "{}");
        assert_eq!(
            answer(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &mut |_, _| Ok(Value::Null)),
            None
        );
    }
}
