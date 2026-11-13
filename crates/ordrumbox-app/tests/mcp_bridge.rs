//! An MCP client's way to the window, end to end: the `ordrumbox --mcp`
//! bridge started as a client starts it, speaking JSON-RPC on its standard
//! input and output, and a session in this process listening as the
//! window does, answering between its "frames".

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

mod common;

use common::scratch_dir;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use ordrumbox_core::json::{self, Value};
use ordrumbox_ui::{LibrarySource, Session, Waker, host_output};

/// The library the session starts with: never saved, as a plugin's.
const LIBRARY: &str = r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0}]}]}]}"#;
/// How long the bridge may take for the whole exchange.
const DEADLINE: Duration = Duration::from_secs(30);

fn call(id: u32, name: &str, arguments: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{arguments}}}}}"#
    )
}

/// The text of tool answer `answer`, and whether it is an error.
fn tool_text(answer: &Value) -> (String, bool) {
    let result = answer.get("result").unwrap_or_else(|| panic!("no result in {answer}"));
    let text = result.get("content").and_then(Value::as_array).and_then(|c| c[0].get("text")).and_then(Value::as_str);
    (text.unwrap().to_owned(), result.get("isError").and_then(Value::as_bool).unwrap())
}

#[test]
fn a_client_edits_the_session_through_the_bridge() {
    let dir = scratch_dir("mcp-bridge");
    let (start, _audio) = host_output(48_000.0);
    let mut session = Session::load_from(start, LibrarySource::Host(Some(LIBRARY.into())));
    // The bridge finds the window by the file next to the library.
    session.start_mcp_at(&dir.join("mcp.json"), Waker::new(|| {}));

    let mut bridge = Command::new(env!("CARGO_BIN_EXE_ordrumbox"))
        .arg("--mcp")
        .env("ORDRUMBOX_LIBRARY", dir.join("library.json"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let lines = [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_owned(),
        r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_owned(),
        call(3, "createNewPattern", r#"{"patternName":"house"}"#),
        call(4, "listPatterns", "{}"),
        call(5, "setPatternBeatCount", r#"{"patternName":"techno","beatCount":4}"#),
        call(6, "noSuchTool", "{}"),
        call(7, "undo", "{}"),
    ];
    let mut stdin = bridge.stdin.take().unwrap();
    for line in &lines {
        writeln!(stdin, "{line}").unwrap();
    }
    // End of input: the bridge stops once it has answered.
    drop(stdin);
    // Read on a thread of its own: the tool list alone may fill the pipe.
    let stdout = bridge.stdout.take().unwrap();
    let reader = thread::spawn(move || BufReader::new(stdout).lines().map(Result::unwrap).collect::<Vec<_>>());

    // The window: the calls waiting run between frames.
    let started = Instant::now();
    while bridge.try_wait().unwrap().is_none() {
        assert!(started.elapsed() < DEADLINE, "the bridge did not finish");
        session.poll_mcp();
        thread::sleep(Duration::from_millis(1));
    }
    assert!(bridge.wait().unwrap().success());
    let answers: Vec<Value> = reader.join().unwrap().iter().map(|line| json::parse(line).unwrap()).collect();
    std::fs::remove_dir_all(&dir).unwrap();

    // One answer per request, in order; none to the notification.
    let ids: Vec<f64> = answers.iter().map(|a| a.get("id").and_then(Value::as_f64).unwrap()).collect();
    assert_eq!(ids, [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    let info = answers[0].get("result").and_then(|r| r.get("serverInfo")).unwrap();
    assert_eq!(info.get("name").and_then(Value::as_str), Some("ordrumbox"));
    let tools = answers[1].get("result").and_then(|r| r.get("tools")).and_then(Value::as_array).unwrap();
    assert!(tools.iter().any(|t| t.get("name").and_then(Value::as_str) == Some("addNotesToPattern")));

    assert!(!tool_text(&answers[2]).1, "{}", answers[2]);
    let (listed, failed) = tool_text(&answers[3]);
    assert!(!failed);
    let listed = json::parse(&listed).unwrap();
    let names: Vec<&str> = listed
        .get("patterns")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|p| p.get("name").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(names, ["rock", "house"]);
    // A refused call is a tool error, not a protocol error.
    let (refused, failed) = tool_text(&answers[4]);
    assert!(failed && refused.contains("techno"), "{refused}");
    assert_eq!(tool_text(&answers[5]), ("Unknown tool: noSuchTool".to_owned(), true));
    let (undone, failed) = tool_text(&answers[6]);
    assert!(!failed && undone.contains("undone"), "{undone}");

    // What the client did is in the session: the new pattern, undone.
    let library = json::parse(&session.library_text()).unwrap();
    assert_eq!(library.get("patterns").and_then(Value::as_array).map(<[Value]>::len), Some(1));
}
