//! The Model Context Protocol server: JSON-RPC 2.0 messages, one per line, and the tools an AI client may
//! call. The tools act on the running app: `ordrumbox --mcp` (`mcp_link.rs`) answers the
//! protocol and passes each tool call to the window, where the session runs
//! it (`session_mcp.rs`) as one undo step.
//!
//! Tool and parameter names stay as clients know them; the track and note settings
//! are named by their keys in the files this app writes (their `json_record!` tables).

use ordrumbox_core::json::{self, Object, Value};
use ordrumbox_logic::tools::object;

use crate::update;

pub use catalog::tools;
use renamed::{tool_arguments, tool_name};

mod catalog;
mod renamed;

/// Protocol versions this server speaks, newest first.
const PROTOCOL_VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];
/// The JSON-RPC errors this server answers with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ErrorCode {
    ParseError,
    InvalidRequest,
    MethodNotFound,
    InvalidParams,
}

impl ErrorCode {
    /// The code JSON-RPC 2.0 gives this error.
    #[must_use]
    const fn code(self) -> i32 {
        match self {
            Self::ParseError => -32700,
            Self::InvalidRequest => -32600,
            Self::MethodNotFound => -32601,
            Self::InvalidParams => -32602,
        }
    }
}

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

/// A JSON-RPC answer to request `id`.
fn reply(id: &Value, outcome: Result<Value, (ErrorCode, String)>) -> String {
    let mut message = Object::new();
    message.insert("jsonrpc", Value::from("2.0"));
    message.insert("id", id.clone());
    match outcome {
        Ok(result) => _ = message.insert("result", result),
        Err((code, text)) => {
            _ = message.insert("error", object([("code", Value::from(code.code())), ("message", Value::from(text))]));
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
        Err(e) => return Some(reply(&Value::Null, Err((ErrorCode::ParseError, format!("not JSON: {e}"))))),
    };
    let method = message.get("method").and_then(Value::as_str);
    let Some(id) = message.get("id") else {
        // A notification (`notifications/initialized`, a cancellation) or a
        // client's answer: nothing to say.
        return None;
    };
    let Some(method) = method else {
        return Some(reply(id, Err((ErrorCode::InvalidRequest, "no method".into()))));
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
                    Ok(tool_result(call(tool_name(name), &tool_arguments(arguments))))
                } else {
                    Err((ErrorCode::InvalidParams, "arguments: not an object".into()))
                }
            }
            None => Err((ErrorCode::InvalidParams, "no tool name".into())),
        },
        _ => Err((ErrorCode::MethodNotFound, format!("unknown method {method}"))),
    };
    Some(reply(id, outcome))
}

#[cfg(test)]
mod tests {
    use super::renamed::RENAMED_TOOLS;
    use super::*;

    fn run(line: &str) -> Value {
        let mut call = |name: &str, args: &Value| match name {
            "echo" => Ok(args.clone()),
            _ => Err(format!("Unknown tool: {name}")),
        };
        json::parse(&answer(line, &mut call).unwrap()).unwrap()
    }

    #[test]
    fn a_renamed_tool_answers_to_its_old_name() {
        let tools = tools();
        let tools = tools.as_array().unwrap();
        let named = |name: &str| tools.iter().any(|t| t.get("name").and_then(Value::as_str) == Some(name));
        for (old, new) in RENAMED_TOOLS {
            assert_eq!(tool_name(old), new);
            assert!(named(new) && !named(old), "{old} listed as {new} only");
        }
        assert_eq!(tool_name("play"), "play");
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
        assert_eq!(code(&run(r#"{"jsonrpc":"2.0","id":4,"method":"resources/list"}"#)), Some(f64::from(ErrorCode::MethodNotFound.code())));
        assert_eq!(code(&run("{not json")), Some(f64::from(ErrorCode::ParseError.code())));
        let bad = r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"echo","arguments":[]}}"#;
        assert_eq!(code(&run(bad)), Some(f64::from(ErrorCode::InvalidParams.code())));
        assert_eq!(run(r#"{"jsonrpc":"2.0","id":6,"method":"ping"}"#).get("result").unwrap().to_json(), "{}");
        assert_eq!(
            answer(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &mut |_, _| Ok(Value::Null)),
            None
        );
    }
}
