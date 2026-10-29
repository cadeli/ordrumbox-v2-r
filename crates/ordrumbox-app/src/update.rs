//! Whether a newer orDrumbox is published: the latest GitHub release of this
//! repository, read with the system's `curl` (in Windows 10 and later, and
//! macOS) on a thread of its own. Nothing is downloaded or installed: the
//! user is told, and Help opens the release page.
//!
//! The request carries only `User-Agent: orDrumbox/<version>`. The check runs
//! at start at most once a day (`settings.json`: `updates.lastCheck`), never
//! when `updates.check` is false, and on demand from Help.

use std::io;
use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use ordrumbox_core::json::{self, Object, Value};

/// This program's version.
pub const CURRENT: &str = env!("CARGO_PKG_VERSION");
/// The latest release, as JSON (`tag_name`, `html_url`).
const API_URL: &str = "https://api.github.com/repos/cadeli/ordrumbox-v2-r/releases/latest";
/// Seconds between two checks at start.
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
/// Seconds `curl` may take in all.
const TIMEOUT_SECS: &str = "10";

/// A published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    /// Its page, where the zip is downloaded.
    pub url: String,
}

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Newer(Release),
    UpToDate,
    /// The repository has no release yet.
    NoRelease,
    Failed(String),
}

/// The `updates` object of `settings.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// Check at start (Help checks anyway).
    pub check: bool,
    /// When the last check at start was made, in seconds since 1970.
    pub last_check: Option<u64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { check: true, last_check: None }
    }
}

impl Settings {
    /// Read from `settings.json`'s `updates`, with what was not taken.
    pub fn from_json(value: &Value) -> (Self, Vec<String>) {
        let mut settings = Self::default();
        let mut issues = Vec::new();
        let Some(object) = value.as_object() else {
            return (settings, vec![format!("updates: not an object ({value}), defaults used")]);
        };
        for (key, value) in object.iter() {
            match (key, value) {
                ("check", Value::Bool(check)) => settings.check = *check,
                ("lastCheck", value) if value.as_f64().is_some_and(|s| s >= 0.0 && s.fract() == 0.0) => {
                    settings.last_check = value.as_f64().map(|s| s as u64);
                }
                ("check" | "lastCheck", value) => issues.push(format!("updates.{key}: wrong type ({value}), ignored")),
                (key, _) => issues.push(format!("updates.{key}: unknown setting, ignored")),
            }
        }
        (settings, issues)
    }

    pub fn to_json(self) -> Value {
        let mut object = Object::new();
        object.insert("check", Value::Bool(self.check));
        if let Some(last) = self.last_check {
            object.insert("lastCheck", Value::from(last as f64));
        }
        Value::Object(object)
    }

    /// Whether the check at start is due at `now` (seconds since 1970).
    pub fn due(self, now: u64) -> bool {
        self.check && self.last_check.is_none_or(|last| now.saturating_sub(last) >= CHECK_INTERVAL_SECS)
    }
}

/// Seconds since 1970 (0 on a clock set before it).
pub fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `v1.2.3` or `1.2` as numbers (missing parts are 0); `None` for anything
/// else, a pre-release (`1.2.0-beta`) included.
fn parse_version(text: &str) -> Option<[u64; 3]> {
    let text = text.trim();
    let mut parts = text.strip_prefix('v').unwrap_or(text).split('.');
    let mut version = [0; 3];
    for (i, part) in parts.by_ref().take(3).enumerate() {
        version[i] = part.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

/// Whether release `latest` is newer than `current`, compared as numbers.
fn is_newer(latest: &str, current: &str) -> Result<bool, String> {
    let latest_number = parse_version(latest).ok_or_else(|| format!("release version \"{latest}\" not understood"))?;
    let current_number = parse_version(current).ok_or_else(|| format!("version \"{current}\" not understood"))?;
    Ok(latest_number > current_number)
}

/// The release in GitHub's answer.
fn release_from_json(text: &str) -> Result<Release, String> {
    let value = json::parse(text).map_err(|e| format!("answer not understood: {e}"))?;
    let field = |key: &str| {
        value.get(key).and_then(Value::as_str).map(str::to_owned).ok_or_else(|| format!("answer has no {key}"))
    };
    Ok(Release { version: field("tag_name")?, url: field("html_url")? })
}

/// GitHub's answer to the request.
#[derive(Debug)]
enum Answer {
    Release(String),
    /// 404: no release published.
    NotFound,
}

/// What GitHub's answer means for this version.
fn outcome(answer: Result<Answer, String>) -> Outcome {
    let text = match answer {
        Ok(Answer::Release(text)) => Ok(text),
        Ok(Answer::NotFound) => return Outcome::NoRelease,
        Err(e) => Err(e),
    };
    let checked = text.and_then(|text| release_from_json(&text)).and_then(|release| {
        let newer = is_newer(&release.version, CURRENT)?;
        Ok(if newer { Outcome::Newer(release) } else { Outcome::UpToDate })
    });
    checked.unwrap_or_else(Outcome::Failed)
}

/// `program` with `args`, without a console window flashing up on Windows
/// (the release build has none of its own).
fn command(program: &str, args: &[&str]) -> Command {
    let mut command = Command::new(program);
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// The HTTP status `curl` writes after the body.
const STATUS_MARK: &str = "\nHTTP status ";

/// GitHub's answer, read with `curl`.
fn fetch() -> Result<Answer, String> {
    let user_agent = format!("User-Agent: orDrumbox/{CURRENT}");
    let write_out = format!("{STATUS_MARK}%{{http_code}}");
    let args = [
        "--silent",
        "--show-error",
        "--location",
        "--write-out",
        &write_out,
        "--max-time",
        TIMEOUT_SECS,
        "--header",
        "Accept: application/vnd.github+json",
        "--header",
        &user_agent,
        API_URL,
    ];
    let output = command("curl", &args).output().map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => "curl not found".to_owned(),
        _ => format!("curl: {e}"),
    })?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() { format!("curl: {}", output.status) } else { error });
    }
    let text = String::from_utf8(output.stdout).map_err(|_| "answer is not UTF-8".to_owned())?;
    let (body, status) = text.rsplit_once(STATUS_MARK).ok_or("curl gave no HTTP status")?;
    match status.trim() {
        "200" => Ok(Answer::Release(body.to_owned())),
        "404" => Ok(Answer::NotFound),
        status => Err(format!("GitHub answered HTTP {status}")),
    }
}

/// Called from the check's thread once its outcome is sent, to wake the UI.
#[derive(Clone)]
pub struct Waker(Arc<dyn Fn() + Send + Sync>);

impl Waker {
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(wake))
    }
}

impl std::fmt::Debug for Waker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Waker")
    }
}

/// A check under way.
#[derive(Debug)]
pub struct Check {
    receiver: Receiver<Outcome>,
    /// Asked from Help (its outcome is always told).
    pub manual: bool,
}

impl Check {
    /// Start a check on a thread of its own.
    pub fn start(manual: bool, waker: &Waker) -> Self {
        let (sender, receiver) = mpsc::channel();
        let waker = waker.clone();
        thread::spawn(move || {
            // Unsent means the session is gone: no one left to tell.
            if sender.send(outcome(fetch())).is_ok() {
                (waker.0)();
            }
        });
        Self { receiver, manual }
    }

    /// Its outcome, once there is one.
    pub fn poll(&self) -> Option<Outcome> {
        match self.receiver.try_recv() {
            Ok(outcome) => Some(outcome),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Outcome::Failed("the check stopped".into())),
        }
    }
}

/// Open `url` in the default browser.
pub fn open_page(url: &str) -> Result<(), String> {
    let mut command = if cfg!(windows) {
        command("rundll32", &["url.dll,FileProtocolHandler", url])
    } else if cfg!(target_os = "macos") {
        command("open", &[url])
    } else {
        command("xdg-open", &[url])
    };
    let status = command.status().map_err(|e| format!("cannot open {url}: {e}"))?;
    if status.success() { Ok(()) } else { Err(format!("cannot open {url}: {status}")) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_numbers() {
        assert_eq!(parse_version("v0.10.2"), Some([0, 10, 2]));
        assert_eq!(parse_version("1.2"), Some([1, 2, 0]));
        assert_eq!(parse_version("1.2.0-beta"), None);
        assert_eq!(parse_version("1.2.3.4"), None);
        assert_eq!(is_newer("v0.10.0", "0.9.0"), Ok(true));
        assert_eq!(is_newer("v0.1.0", "0.1.0"), Ok(false));
        assert_eq!(is_newer("v0.0.9", "0.1.0"), Ok(false));
        assert!(is_newer("nightly", "0.1.0").is_err());
        assert!(parse_version(CURRENT).is_some(), "the crate's own version");
    }

    #[test]
    fn the_answer_gives_the_outcome() {
        let answer =
            |tag: &str| Ok(Answer::Release(format!(r#"{{"tag_name":"{tag}","html_url":"https://x/r","assets":[]}}"#)));
        let newer = Release { version: "v999.0.0".into(), url: "https://x/r".into() };
        assert_eq!(outcome(answer("v999.0.0")), Outcome::Newer(newer));
        assert_eq!(outcome(answer(&format!("v{CURRENT}"))), Outcome::UpToDate);
        assert_eq!(outcome(Ok(Answer::Release("{}".into()))), Outcome::Failed("answer has no tag_name".into()));
        assert_eq!(outcome(Ok(Answer::NotFound)), Outcome::NoRelease);
        assert_eq!(outcome(Err("offline".into())), Outcome::Failed("offline".into()));
    }

    #[test]
    fn settings_are_read_and_checked() {
        let read = |text: &str| Settings::from_json(&json::parse(text).unwrap());
        let (settings, issues) = read(r#"{"check":false,"lastCheck":100}"#);
        assert_eq!(settings, Settings { check: false, last_check: Some(100) });
        assert!(issues.is_empty());
        assert_eq!(read(&settings.to_json().to_json()).0, settings);
        let (settings, issues) = read(r#"{"check":"no","lastCheck":-1,"x":1}"#);
        assert_eq!(settings, Settings::default());
        assert_eq!(issues.len(), 3, "{issues:?}");
    }

    #[test]
    fn the_check_at_start_is_daily() {
        let day = CHECK_INTERVAL_SECS;
        assert!(Settings::default().due(5));
        assert!(!Settings { check: true, last_check: Some(10) }.due(10 + day - 1));
        assert!(Settings { check: true, last_check: Some(10) }.due(10 + day));
        assert!(!Settings { check: false, last_check: None }.due(5));
    }
}
