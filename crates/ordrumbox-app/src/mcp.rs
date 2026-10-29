//! The Model Context Protocol server (`ordrumboxMcpserver.mjs` of the JS
//! app): JSON-RPC 2.0 messages, one per line, and the tools an AI client may
//! call. Unlike the JS server, which edited `song.json` on its own, the tools
//! act on the running app: `ordrumbox --mcp` (`mcp_link.rs`) answers the
//! protocol and passes each tool call to the window, where the session runs
//! it (`session_mcp.rs`) as one undo step.
//!
//! Tool and parameter names are the JS server's; the track and note settings
//! are named by their keys in the files this app writes (`README.md`).

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_core::{Note, Track};

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
listAllInstrumentsNames); a note's place is an absolute step, beat * stepsPerBeat + step.";

/// Track settings a tool may not set: the track is named by `trackName`.
const NOT_EDITED: [&str; 1] = ["name"];
/// Note fields that are not settings: `step` gives the place.
const NOTE_PLACE: [&str; 2] = ["beat", "step"];

/// The tools, without the track and note settings (filled in by [`tools`]).
const TOOLS: &str = r#"[
  {"name": "listPatterns", "description": "The patterns of the library, in order, and the one shown.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "loadPattern", "description": "A pattern with its tracks (every setting) and notes (fields away from their default, and their absolute step).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string", "minLength": 1}}, "required": ["patternName"]}},
  {"name": "createNewPattern", "description": "Adds an empty pattern (4 beats, 120 BPM) at the end of the library.",
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
  {"name": "updateTrack", "description": "Changes the settings of a track (added when the pattern has none of that name) and, with noteUpdates, of all its notes. Numbers outside a setting's range are brought inside it: the answer gives the values used.",
   "inputSchema": {"type": "object", "properties": {
     "patternName": {"type": "string"},
     "trackName": {"type": "string"},
     "updates": {"type": "object", "properties": {}, "additionalProperties": false},
     "noteUpdates": {"type": "object", "properties": {}, "additionalProperties": false}},
     "required": ["patternName", "trackName", "updates"]}},
  {"name": "setPatternBpm", "description": "The tempo of a pattern, whole, 20 to 300.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "bpm": {"type": "number", "minimum": 20, "maximum": 300}}, "required": ["patternName", "bpm"]}},
  {"name": "setPatternBeatCount", "description": "The length of a pattern in beats; every track follows.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "beatCount": {"type": "integer", "minimum": 1, "maximum": 16}}, "required": ["patternName", "beatCount"]}},
  {"name": "setPatternTags", "description": "The tags (genre, mood...) of a pattern.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "tags": {"type": "array", "items": {"type": "string"}}}, "required": ["patternName", "tags"]}},
  {"name": "setPatternDescription", "description": "The description of a pattern.",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}, "description": {"type": "string"}}, "required": ["patternName", "description"]}},
  {"name": "savePatternToJson", "description": "Exports a pattern as JSON to the exports folder of the user data folder (as File > Export pattern).",
   "inputSchema": {"type": "object", "properties": {"patternName": {"type": "string"}}, "required": ["patternName"]}},
  {"name": "listAllInstrumentsNames", "description": "The instruments a track can be named after, with whether each is a drum and its default pan.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "listKitSamples", "description": "The kits and their samples (instrument key and path), and the kit in use.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "selectKit", "description": "Plays the library with a kit (by name, any case): every track's sound is assigned again.",
   "inputSchema": {"type": "object", "properties": {"kit": {"type": "string", "minLength": 1}}, "required": ["kit"]}},
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
  {"name": "play", "description": "Plays the pattern shown, or with song true the selected arrangement from its start.",
   "inputSchema": {"type": "object", "properties": {"song": {"type": "boolean"}}}},
  {"name": "stop", "description": "Stops playing.",
   "inputSchema": {"type": "object", "properties": {}}},
  {"name": "undo", "description": "Undoes the last edit, made in the window or by a tool.",
   "inputSchema": {"type": "object", "properties": {}}}
]"#;

/// `{key: value, ...}`.
pub fn object(entries: impl IntoIterator<Item = (&'static str, Value)>) -> Value {
    let mut object = Object::new();
    for (key, value) in entries {
        object.insert(key, value);
    }
    Value::Object(object)
}

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
        // Unset by default: an LFO (`{type, freq, min, max, phase}`), a synth
        // preset key, an arpeggio (intervals, "0,4,7", a direction); null unsets.
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
        (key, setting_schema(key, &default, Track::value_range(key)))
    });
    object(entries)
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
        assert_eq!(tools.len(), 22);
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
