//! An MCP client composing a song, end to end: what an AI client does when
//! asked for "a 64-measure arrangement of afrobeat patterns, with a bass,
//! melodic progressions and breaks". The `ordrumbox --mcp` bridge is started
//! as a client starts it; each call goes through it to a session in this
//! process, which answers between its "frames", and the client reads each
//! answer before its next call, as a client does.

#![allow(clippy::unwrap_used, clippy::expect_used, reason = "a test: a failed expectation fails the test")]

use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use ordrumbox_core::json::{self, Value};
use ordrumbox_core::{Library, Pattern};
use ordrumbox_ui::{LibrarySource, Session, Waker, host_output};

/// The library the session starts with: never saved, as a plugin's.
const LIBRARY: &str = r#"{"patterns":[{"name":"rock","bpm":100,"tracks":[{"name":"KICK","notes":[{"beat":0}]}]}]}"#;
/// How long one answer may take.
const DEADLINE: Duration = Duration::from_secs(30);

const ARRANGEMENT: &str = "afro song";
const MEASURE_COUNT: u32 = 64;
const BPM: f64 = 112.0;
const GROOVE_A: &str = "afro groove A";
const GROOVE_B: &str = "afro groove B";
const VERSE_CHORDS: &str = "afro verse chords";
const CHORUS_CHORDS: &str = "afro chorus chords";
const FILL: &str = "afro fill";
const STOP: &str = "afro stop";
/// Measures of each harmony pattern: 16 beats.
const HARMONY_MEASURES: u32 = 4;

/// A part of the song: its first measure, its length, its drums, and the
/// harmony (bass and chords) over them.
struct Section {
    start: u32,
    measures: u32,
    drums: &'static str,
    harmony: Option<&'static str>,
}

const fn section(start: u32, measures: u32, drums: &'static str, harmony: Option<&'static str>) -> Section {
    Section { start, measures, drums, harmony }
}

/// The song: an intro of drums alone, verses and choruses, each section
/// closed by a break, the last one ending it.
const SECTIONS: [Section; 11] = [
    section(0, 8, GROOVE_A, None),
    section(8, 7, GROOVE_A, Some(VERSE_CHORDS)),
    section(15, 1, FILL, None),
    section(16, 7, GROOVE_B, Some(CHORUS_CHORDS)),
    section(23, 1, STOP, None),
    section(24, 15, GROOVE_A, Some(VERSE_CHORDS)),
    section(39, 1, FILL, None),
    section(40, 15, GROOVE_B, Some(CHORUS_CHORDS)),
    section(55, 1, STOP, None),
    section(56, 7, GROOVE_B, Some(VERSE_CHORDS)),
    section(63, 1, FILL, None),
];

/// A chord progression, one chord a measure: its root, in semitones.
const VERSE_ROOTS: [i32; 4] = [0, 0, 5, 7]; // i i iv v
const CHORUS_ROOTS: [i32; 4] = [-4, -2, 0, 0]; // VI VII i i

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ordrumbox-mcp-afrobeat-{}", std::process::id()));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).unwrap();
    }
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The client: the bridge it started, and the answers read from it.
struct Client {
    bridge: Child,
    stdin: Option<ChildStdin>,
    answers: Receiver<String>,
    next_id: u32,
}

impl Client {
    fn start(dir: &std::path::Path) -> Self {
        let mut bridge = Command::new(env!("CARGO_BIN_EXE_ordrumbox"))
            .arg("--mcp")
            .env("ORDRUMBOX_LIBRARY", dir.join("library.json"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let stdin = bridge.stdin.take();
        let stdout = bridge.stdout.take().unwrap();
        let (sender, answers) = mpsc::channel();
        // Read on a thread of its own: the tool list alone may fill the pipe.
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line.unwrap()).is_err() {
                    break;
                }
            }
        });
        Self { bridge, stdin, answers, next_id: 1 }
    }

    fn send(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    /// Request `method`, the window running between frames until it is answered.
    fn request(&mut self, session: &mut Session, method: &str, params: &str) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#));
        let started = Instant::now();
        let line = loop {
            session.poll_mcp();
            match self.answers.try_recv() {
                Ok(line) => break line,
                Err(mpsc::TryRecvError::Empty) => {}
                Err(mpsc::TryRecvError::Disconnected) => panic!("the bridge stopped before answering {method}"),
            }
            assert!(started.elapsed() < DEADLINE, "no answer to {method} {params}");
            thread::sleep(Duration::from_millis(1));
        };
        let answer = json::parse(&line).unwrap();
        assert_eq!(answer.get("id").and_then(Value::as_f64), Some(f64::from(id)), "{line}");
        answer
    }

    /// Call tool `name`, which must succeed; its answer, parsed.
    fn call(&mut self, session: &mut Session, name: &str, arguments: &str) -> Value {
        json::parse(arguments).unwrap_or_else(|e| panic!("{name}: arguments not JSON ({e}): {arguments}"));
        let answer = self.request(session, "tools/call", &format!(r#"{{"name":"{name}","arguments":{arguments}}}"#));
        let result = answer.get("result").unwrap_or_else(|| panic!("{name}: no result in {answer}"));
        let content = result.get("content").and_then(Value::as_array).unwrap();
        let text = content[0].get("text").and_then(Value::as_str).unwrap();
        assert_eq!(result.get("isError").and_then(Value::as_bool), Some(false), "{name} {arguments}: {text}");
        json::parse(text).unwrap_or_else(|e| panic!("{name}: answer not JSON ({e}): {text}"))
    }

    /// Close the bridge's input: it stops once it has answered.
    fn finish(mut self) {
        drop(self.stdin.take());
        let started = Instant::now();
        while self.bridge.try_wait().unwrap().is_none() {
            assert!(started.elapsed() < DEADLINE, "the bridge did not finish");
            thread::sleep(Duration::from_millis(1));
        }
        assert!(self.bridge.wait().unwrap().success());
    }
}

fn str_field<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or_else(|| panic!("no {key} in {value}"))
}

fn number_field(value: &Value, key: &str) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or_else(|| panic!("no {key} in {value}"))
}

/// Notes as the `notes` argument of `addNotesToPattern`.
fn notes_json(notes: &[String]) -> String {
    format!("[{}]", notes.join(","))
}

fn note(track: &str, step: i32, fields: &str) -> String {
    format!(r#"{{"trackName":"{track}","step":{step}{fields}}}"#)
}

/// A bass line and a chord progression over `roots`, one chord a measure,
/// 4 steps a beat: the bass on the root, its octave and its fifth in an
/// afrobeat push; the chords as stabs on the off-beats, each spelled as an
/// arpeggio of the triad.
fn harmony_notes(roots: &[i32; 4]) -> String {
    let mut notes = Vec::new();
    for (measure, &root) in (0..).zip(roots) {
        let at = measure * 16;
        // Bass: 1, the "and" of 1, the push before 3, 3, a fifth to lead on.
        for (step, interval, velocity) in [(0, 0, 1.0), (3, 12, 0.7), (6, 0, 0.8), (8, 0, 0.9), (14, 7, 0.75)] {
            notes.push(note("BASS", at + step, &format!(r#","pitch":{},"velocity":{velocity}"#, root + interval)));
        }
        for step in [2, 6, 10, 14] {
            let fields = format!(r#","pitch":{root},"velocity":0.7,"arp":"0,3,7","retriggerCount":3"#);
            notes.push(note("PIANO", at + step, &fields));
        }
    }
    notes_json(&notes)
}

/// A one-measure fill: the kick on 1 and the push, a snare roll growing
/// louder, the toms leading into the next section.
fn fill_notes() -> String {
    let mut notes = vec![note("KICK", 0, r#","velocity":1"#), note("KICK", 6, "")];
    notes.extend(
        (8..12).map(|step| note("SNARE", step, &format!(r#","velocity":{}"#, 0.5 + 0.1 * f64::from(step - 8)))),
    );
    notes.extend([12, 13].map(|step| note("HTOM", step, r#","velocity":0.9"#)));
    notes.extend([14, 15].map(|step| note("LTOM", step, r#","velocity":1"#)));
    notes_json(&notes)
}

/// A stop: everything hits on 1 and on the push before 3, then silence.
fn stop_notes() -> String {
    let hits =
        ["KICK", "SNARE", "CYM"].iter().flat_map(|&track| [note(track, 0, r#","velocity":1"#), note(track, 6, "")]);
    notes_json(&hits.collect::<Vec<_>>())
}

fn pattern<'a>(library: &'a Library, name: &str) -> &'a Pattern {
    library.patterns.iter().find(|p| p.name.as_deref() == Some(name)).unwrap_or_else(|| panic!("no pattern {name}"))
}

/// The tracks of a pattern that have notes, by name.
fn sounding_tracks(pattern: &Pattern) -> BTreeSet<&str> {
    pattern.tracks.iter().filter(|t| !t.notes.is_empty()).map(|t| t.name.as_str()).collect()
}

#[test]
fn a_client_composes_an_afrobeat_song_of_64_measures() {
    let dir = scratch_dir();
    let (start, _audio) = host_output(48_000.0);
    let mut session = Session::load_from(start, LibrarySource::Host(Some(LIBRARY.into())));
    // The bridge finds the window by the file next to the library.
    session.start_mcp_at(&dir.join("mcp.json"), Waker::new(|| {}));
    let mut client = Client::start(&dir);

    let init = client.request(&mut session, "initialize", r#"{"protocolVersion":"2024-11-05"}"#);
    assert!(init.get("result").and_then(|r| r.get("serverInfo")).is_some(), "{init}");
    client.send(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#);
    let tools = client.request(&mut session, "tools/list", "{}");
    let tools = tools.get("result").and_then(|r| r.get("tools")).and_then(Value::as_array).unwrap();
    let names: BTreeSet<&str> = tools.iter().filter_map(|t| t.get("name").and_then(Value::as_str)).collect();
    for needed in ["listStyles", "generatePattern", "addNotesToPattern", "createArrangement", "addPatternToArrangement"]
    {
        assert!(names.contains(needed), "no tool {needed}");
    }

    // The client looks for the style, then has two grooves made in it.
    let styles = client.call(&mut session, "listStyles", "{}");
    assert!(styles.to_json().contains("afrobeat"), "no afrobeat style: {styles}");
    for name in [GROOVE_A, GROOVE_B] {
        let made = client.call(&mut session, "generatePattern", r#"{"style":"afrobeat"}"#);
        assert_eq!(str_field(&made, "style"), "afrobeat", "{made}");
        let generated = str_field(&made, "name").to_owned();
        let arguments = format!(r#"{{"patternName":"{generated}","newName":"{name}"}}"#);
        client.call(&mut session, "renamePattern", &arguments);
    }

    // The harmony: a bass and chords over a progression, 4 measures long.
    for (name, roots) in [(VERSE_CHORDS, &VERSE_ROOTS), (CHORUS_CHORDS, &CHORUS_ROOTS)] {
        client.call(&mut session, "createNewPattern", &format!(r#"{{"patternName":"{name}"}}"#));
        let arguments = format!(r#"{{"patternName":"{name}","notes":{}}}"#, harmony_notes(roots));
        let added = client.call(&mut session, "addNotesToPattern", &arguments);
        assert_eq!(number_field(&added, "created"), 36.0, "{added}");
        assert_eq!(number_field(&added, "beatCount"), f64::from(HARMONY_MEASURES * 4), "{added}");
        // The chords under the bass, a little to the side.
        let arguments =
            format!(r#"{{"patternName":"{name}","trackName":"PIANO","updates":{{"volume":0.7,"pan":0.3}}}}"#);
        let updated = client.call(&mut session, "updateTrack", &arguments);
        assert_eq!(str_field(&updated, "action"), "updated", "{updated}");
    }

    // The breaks.
    for (name, notes) in [(FILL, fill_notes()), (STOP, stop_notes())] {
        client.call(&mut session, "createNewPattern", &format!(r#"{{"patternName":"{name}"}}"#));
        client.call(&mut session, "addNotesToPattern", &format!(r#"{{"patternName":"{name}","notes":{notes}}}"#));
    }
    for name in [GROOVE_A, GROOVE_B, VERSE_CHORDS, CHORUS_CHORDS, FILL, STOP] {
        client.call(&mut session, "setPatternTags", &format!(r#"{{"patternName":"{name}","tags":["afrobeat"]}}"#));
    }

    // The arrangement: the grooves and the harmony over them at once, the
    // breaks added after, as a client adds them once it hears the song.
    let clips: Vec<String> = SECTIONS
        .iter()
        .filter(|s| s.harmony.is_some() || s.measures > 1)
        .flat_map(|s| {
            let drums =
                format!(r#"{{"patternName":"{}","startMeasure":{},"measureCount":{}}}"#, s.drums, s.start, s.measures);
            let harmony = s.harmony.map(|h| {
                format!(r#"{{"patternName":"{h}","startMeasure":{},"measureCount":{}}}"#, s.start, s.measures)
            });
            std::iter::once(drums).chain(harmony)
        })
        .collect();
    let arguments = format!(
        r#"{{"name":"{ARRANGEMENT}","description":"afrobeat: intro, verses, choruses, breaks","bpm":{BPM},"clips":[{}]}}"#,
        clips.join(",")
    );
    client.call(&mut session, "createArrangement", &arguments);
    for s in SECTIONS.iter().filter(|s| s.harmony.is_none() && s.measures == 1) {
        let arguments =
            format!(r#"{{"patternName":"{}","startMeasure":{},"arrangement":"{ARRANGEMENT}"}}"#, s.drums, s.start);
        let added = client.call(&mut session, "addPatternToArrangement", &arguments);
        assert_eq!(added.get("clip").map(|c| number_field(c, "measureCount")), Some(1.0), "{added}");
    }

    // What the client reads back.
    let listed = client.call(&mut session, "listArrangements", "{}");
    let songs = listed.get("arrangements").and_then(Value::as_array).unwrap();
    let song = songs.iter().find(|s| str_field(s, "name") == ARRANGEMENT).unwrap_or_else(|| panic!("{listed}"));
    assert_eq!(number_field(song, "lengthMeasures"), f64::from(MEASURE_COUNT), "{song}");
    assert_eq!(number_field(song, "bpm"), BPM);
    let groove = client.call(&mut session, "loadPattern", &format!(r#"{{"patternName":"{GROOVE_A}"}}"#));
    let tracks = groove.get("tracks").and_then(Value::as_array).unwrap();
    let track_names: BTreeSet<&str> = tracks.iter().map(|t| str_field(t, "name")).collect();
    assert!(["KICK", "SNARE", "BASS", "ORGAN"].iter().all(|t| track_names.contains(t)), "{track_names:?}");

    // It plays.
    let played = client.call(&mut session, "play", r#"{"song":true}"#);
    assert_eq!(str_field(&played, "playing"), ARRANGEMENT);
    let state = client.call(&mut session, "getPlaybackState", "{}");
    assert_eq!(str_field(&state, "arrangement"), ARRANGEMENT, "{state}");
    client.call(&mut session, "stop", "{}");
    client.finish();
    std::fs::remove_dir_all(&dir).unwrap();

    // The song in the session, as the engine reads it.
    let (library, report) = Library::parse(&session.library_text()).unwrap();
    assert!(report.issues.is_empty(), "{:?}", report.issues);
    let song = library.songs.iter().find(|s| s.name == ARRANGEMENT).unwrap();
    for name in [GROOVE_A, GROOVE_B] {
        let groove = pattern(&library, name);
        assert_eq!(groove.tags, ["afrobeat"]);
        let sounding = sounding_tracks(groove);
        assert!(["KICK", "SNARE"].iter().all(|t| sounding.contains(t)), "{name}: {sounding:?}");
    }
    // The harmony moves: the bass follows the progression, the chords too.
    for (name, roots) in [(VERSE_CHORDS, &VERSE_ROOTS), (CHORUS_CHORDS, &CHORUS_ROOTS)] {
        let harmony = pattern(&library, name);
        assert_eq!(sounding_tracks(harmony), BTreeSet::from(["BASS", "PIANO"]), "{name}");
        assert!(harmony.tracks.iter().all(|t| !t.notes.is_empty()), "{name}: an empty track is left");
        let track = |n: &str| harmony.tracks.iter().find(|t| t.name == n).unwrap();
        for (measure, &root) in (0..).zip(roots) {
            let downbeat = track("BASS").notes.iter().find(|n| n.beat == measure * 4 && n.step == 0).unwrap();
            assert_eq!(downbeat.pitch, f64::from(root), "{name}, measure {measure}");
            let chord = track("PIANO").notes.iter().find(|n| n.beat / 4 == measure).unwrap();
            assert_eq!(chord.pitch, f64::from(root), "{name}, measure {measure}");
        }
    }

    // Measure by measure, what sounds: one drum pattern always, the harmony
    // wherever a section has it, a break closing each section.
    let pattern_name = |index: usize| library.patterns[index].name.as_deref().unwrap_or_default();
    for s in &SECTIONS {
        for measure in s.start..s.start + s.measures {
            let sounding: BTreeSet<&str> = song
                .active_clips_at(&library.patterns, u64::from(measure) * 4, 1)
                .iter()
                .map(|clip| pattern_name(clip.pattern))
                .collect();
            let expected: BTreeSet<&str> = std::iter::once(s.drums).chain(s.harmony).collect();
            assert_eq!(sounding, expected, "measure {measure}");
        }
    }
    let covered: u32 = SECTIONS.iter().map(|s| s.measures).sum();
    assert_eq!(covered, MEASURE_COUNT);
    let breaks = SECTIONS.iter().filter(|s| [FILL, STOP].contains(&s.drums)).count();
    assert_eq!(breaks, 5);
    // The harmony loops inside its sections: a 7-measure verse plays its
    // 4 measures, then 3 of them again.
    let verse = song.active_clips_at(&library.patterns, (8 + u64::from(HARMONY_MEASURES)) * 4, 1);
    let harmony = verse.iter().find(|c| pattern_name(c.pattern) == VERSE_CHORDS).unwrap();
    assert_eq!((harmony.loop_index, harmony.local_tick), (1, 0));
}
