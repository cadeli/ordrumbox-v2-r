//! How an MCP client reaches the running app. The client starts
//! `ordrumbox --mcp`, the bridge: it answers the protocol on its standard
//! input and output (`mcp.rs`) and passes each tool call to the window over
//! a TCP connection to `127.0.0.1`, on the port the app wrote at start to
//! `mcp.json` in the user data folder, with the token written there.
//!
//! Between the two, one JSON object per line: first `{"token"}`, then for
//! each call `{"name", "arguments"}`; each answered by `{"result"}` or
//! `{"error"}`. The window runs the calls one at a time, between frames;
//! the threads here only carry them.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SendError, Sender};
use std::thread;
use std::time::Duration;

use ordrumbox_core::json::{self, Value};

use crate::mcp::{self, object};
use crate::store;
use crate::update::Waker;

/// Longest line read, bytes: a call is a few kilobytes.
const MAX_LINE_BYTES: u64 = 16 << 20;
/// How long the bridge waits for the app to accept, and to answer a call.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const REPLY_TIMEOUT: Duration = Duration::from_secs(60);

/// A tool call from the bridge, waiting for the window's answer.
#[derive(Debug)]
pub struct Request {
    pub name: String,
    pub arguments: Value,
    reply: Sender<Result<Value, String>>,
}

impl Request {
    pub fn answer(self, outcome: Result<Value, String>) {
        match self.reply.send(outcome) {
            // Unsent: the bridge hung up, no one is left to tell.
            Ok(()) | Err(SendError(_)) => {}
        }
    }
}

/// The app's end: a listener on a thread of its own, the calls it took.
#[derive(Debug)]
pub struct Server {
    requests: Receiver<Request>,
}

impl Server {
    /// Listen on a free port of `127.0.0.1` and write it, with a new token,
    /// to `endpoint`; `waker` is called once a call is waiting.
    pub fn start(endpoint: &Path, waker: Waker) -> Result<Self, String> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).map_err(|e| format!("listen: {e}"))?;
        let port = listener.local_addr().map_err(|e| format!("listen: {e}"))?.port();
        let token = new_token();
        let text = object([("port", Value::from(u32::from(port))), ("token", Value::from(token.as_str()))]);
        store::write_atomic(endpoint, text.to_json_pretty(2) + "\n")
            .map_err(|e| format!("{}: {e}", endpoint.display()))?;
        let (sender, requests) = mpsc::channel();
        thread::spawn(move || accept(&listener, &token, &sender, &waker));
        Ok(Self { requests })
    }

    /// The next call waiting, if any.
    pub fn poll(&self) -> Option<Request> {
        // Disconnected: the listener stopped, and so did the calls.
        self.requests.try_recv().ok()
    }
}

/// A token no other program can guess: the standard library's hash keys,
/// drawn from the system's randomness.
fn new_token() -> String {
    use std::hash::{BuildHasher, RandomState};
    let [a, b] = [0u8, 1].map(|i| RandomState::new().hash_one((i, std::process::id())));
    format!("{a:016x}{b:016x}")
}

/// Serve each connection on a thread of its own.
fn accept(listener: &TcpListener, token: &str, sender: &Sender<Request>, waker: &Waker) {
    for stream in listener.incoming() {
        // A connection that failed to open: the bridge sees it and tells its client.
        let Ok(stream) = stream else { continue };
        let (token, sender, waker) = (token.to_owned(), sender.clone(), waker.clone());
        thread::spawn(move || {
            match serve(stream, &token, &sender, &waker) {
                // An I/O error ends the connection: the bridge reports it.
                Ok(()) | Err(_) => {}
            }
        });
    }
}

/// One line, up to [`MAX_LINE_BYTES`]; `None` at the end of the stream.
fn read_line(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = String::new();
    let count = reader.by_ref().take(MAX_LINE_BYTES).read_line(&mut line)?;
    if count == 0 {
        return Ok(None);
    }
    if !line.ends_with('\n') && count as u64 == MAX_LINE_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "line too long"));
    }
    Ok(Some(line))
}

fn write_line(writer: &mut impl Write, value: &Value) -> io::Result<()> {
    writeln!(writer, "{}", value.to_json())?;
    writer.flush()
}

/// `{"result": value}` or `{"error": message}`.
fn outcome_json(outcome: Result<Value, String>) -> Value {
    match outcome {
        Ok(value) => object([("result", value)]),
        Err(message) => object([("error", Value::from(message))]),
    }
}

/// A call line: the tool's name and arguments.
fn parse_call(line: &str) -> Result<(String, Value), String> {
    let value = json::parse(line).map_err(|e| format!("not JSON: {e}"))?;
    let name = value.get("name").and_then(Value::as_str).ok_or("no tool name")?;
    let arguments = value.get("arguments").cloned().unwrap_or_default();
    Ok((name.to_owned(), arguments))
}

/// The token, then calls until the bridge hangs up.
fn serve(stream: TcpStream, token: &str, sender: &Sender<Request>, waker: &Waker) -> io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let Some(hello) = read_line(&mut reader)? else { return Ok(()) };
    let given = match json::parse(&hello) {
        Ok(value) => value.get("token").and_then(Value::as_str).map(str::to_owned),
        Err(e) => return write_line(&mut writer, &outcome_json(Err(format!("not JSON: {e}")))),
    };
    if given.as_deref() != Some(token) {
        return write_line(&mut writer, &outcome_json(Err("wrong token: start orDrumbox again".into())));
    }
    write_line(&mut writer, &outcome_json(Ok(Value::from(crate::update::CURRENT))))?;
    while let Some(line) = read_line(&mut reader)? {
        let outcome = match parse_call(&line) {
            Ok((name, arguments)) => {
                let (reply, answer) = mpsc::channel();
                if sender.send(Request { name, arguments, reply }).is_err() {
                    return Ok(()); // the window is gone
                }
                waker.wake();
                answer.recv().unwrap_or_else(|_| Err("orDrumbox is quitting".into()))
            }
            Err(e) => Err(e),
        };
        write_line(&mut writer, &outcome_json(outcome))?;
    }
    Ok(())
}

// ─── The bridge ──────────────────────────────────────────────────────────────

/// A connection to the app.
#[derive(Debug)]
struct Connection {
    reader: BufReader<TcpStream>,
    writer: TcpStream,
}

impl Connection {
    /// Connect to the app `endpoint` names, and give its token.
    fn open(endpoint: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(endpoint).map_err(|e| format!("{}: {e}", endpoint.display()))?;
        let value = json::parse(&text).map_err(|e| format!("{}: {e}", endpoint.display()))?;
        let port = value.get("port").and_then(Value::as_f64).filter(|p| (1.0..=65535.0).contains(p));
        let token = value.get("token").and_then(Value::as_str);
        let (Some(port), Some(token)) = (port, token) else {
            return Err(format!("{}: no port or no token", endpoint.display()));
        };
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port as u16));
        let stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT).map_err(|e| format!("{address}: {e}"))?;
        stream.set_read_timeout(Some(REPLY_TIMEOUT)).map_err(|e| e.to_string())?;
        let mut connection =
            Self { reader: BufReader::new(stream.try_clone().map_err(|e| e.to_string())?), writer: stream };
        match connection.exchange(&object([("token", Value::from(token))])).map_err(|e| e.to_string())? {
            Ok(_) => Ok(connection),
            Err(e) => Err(e),
        }
    }

    /// Send `message`, read the answer.
    fn exchange(&mut self, message: &Value) -> io::Result<Result<Value, String>> {
        write_line(&mut self.writer, message)?;
        let line = read_line(&mut self.reader)?
            .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "the app closed the connection"))?;
        let answer = json::parse(&line).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(match (answer.get("result"), answer.get("error").and_then(Value::as_str)) {
            (Some(result), _) => Ok(result.clone()),
            (None, Some(error)) => Err(error.to_owned()),
            (None, None) => Err(format!("unexpected answer: {line}")),
        })
    }
}

/// What the bridge keeps between calls.
#[derive(Debug)]
struct Link {
    endpoint: PathBuf,
    connection: Option<Connection>,
}

impl Link {
    /// Run tool `name` in the window.
    fn call(&mut self, name: &str, arguments: &Value) -> Result<Value, String> {
        let message = object([("name", Value::from(name)), ("arguments", arguments.clone())]);
        // A connection kept from an earlier call may lead to an app since
        // quit or restarted: the call never reached it, so it is made once
        // more on a new connection.
        if let Some(connection) = &mut self.connection {
            match connection.exchange(&message) {
                Ok(outcome) => return outcome,
                Err(_stale) => self.connection = None,
            }
        }
        let not_running = |e: String| format!("orDrumbox is not running: start it, then call the tool again ({e})");
        let mut connection = Connection::open(&self.endpoint).map_err(not_running)?;
        let outcome = connection.exchange(&message).map_err(|e| format!("orDrumbox did not answer: {e}"))?;
        self.connection = Some(connection);
        outcome
    }
}

/// `ordrumbox --mcp`: answer the client on the standard input and output
/// until it closes them.
pub fn run_bridge() -> Result<(), String> {
    let endpoint = store::mcp_endpoint_path().ok_or("no user data folder: the app cannot be found")?;
    let mut link = Link { endpoint, connection: None };
    let mut stdout = io::stdout().lock();
    for line in io::stdin().lock().lines() {
        let line = line.map_err(|e| format!("standard input: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(answer) = mcp::answer(&line, &mut |name, arguments| link.call(name, arguments)) {
            writeln!(stdout, "{answer}").and_then(|()| stdout.flush()).map_err(|e| format!("standard output: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_goes_to_the_window_and_back() {
        let endpoint = std::env::temp_dir().join(format!("ordrumbox_mcp_{}.json", std::process::id()));
        let server = Server::start(&endpoint, Waker::new(|| {})).unwrap();
        // The window: answers each call with its name and arguments.
        let window = thread::spawn(move || {
            for _ in 0..2 {
                let request = loop {
                    if let Some(request) = server.poll() {
                        break request;
                    }
                    thread::sleep(Duration::from_millis(5));
                };
                let outcome = match request.name.as_str() {
                    "fail" => Err("it failed".to_owned()),
                    _ => {
                        Ok(object([("name", Value::from(request.name.as_str())), ("args", request.arguments.clone())]))
                    }
                };
                request.answer(outcome);
            }
        });
        let mut link = Link { endpoint: endpoint.clone(), connection: None };
        let args = json::parse(r#"{"bpm":128}"#).unwrap();
        assert_eq!(
            link.call("setPatternBpm", &args).unwrap().to_json(),
            r#"{"name":"setPatternBpm","args":{"bpm":128}}"#
        );
        assert_eq!(link.call("fail", &Value::Null), Err("it failed".to_owned()));
        window.join().unwrap();

        // A wrong token is refused.
        let text = std::fs::read_to_string(&endpoint).unwrap();
        let wrong = text.replace("\"token\": \"", "\"token\": \"x");
        std::fs::write(&endpoint, wrong).unwrap();
        let error = Connection::open(&endpoint).unwrap_err();
        assert!(error.contains("wrong token"), "{error}");

        // No app: the client is told to start it.
        std::fs::remove_file(&endpoint).unwrap();
        let mut link = Link { endpoint, connection: None };
        let error = link.call("listPatterns", &Value::Null).unwrap_err();
        assert!(error.starts_with("orDrumbox is not running"), "{error}");
    }
}
