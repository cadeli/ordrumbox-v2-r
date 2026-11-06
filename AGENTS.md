# AGENTS.md — orDrumbox

Drum machine and step sequencer: patterns of tracks of notes, songs arranged
from patterns, sample kits and a soft synth, played live or rendered to WAV
and MIDI. One native program, `ordrumbox.exe` on Windows (also runs on macOS),
with a software-rendered UI. `README.md` describes the app, its keys, and
where it reads and writes its files.

## Commands

```bash
cargo run -p ordrumbox-app              # the app; finds assets/ from anywhere in the checkout
cargo test --workspace                  # all tests, golden fixtures included
cargo clippy --workspace --all-targets  # must be clean: CI sets RUSTFLAGS=-D warnings
cargo fmt --all                         # rustfmt.toml: max_width 120; CI checks it
cargo xtask bundle ordrumbox-plugin --release --features vst3   # target/bundled/orDrumbox.{clap,vst3}
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on Windows and
macOS. For a `v*` tag or a manual run (`workflow_dispatch`) only, it then
builds the release `ordrumbox.exe` with `assets/` next to it, and the plugin
bundles, checked by `clap-validator`.

## Layout

```
ordrumbox-core    data model, JSON codec, pattern engine, RNG       no deps
ordrumbox-dsp     sample voice, synth voice, channel strip, master  no deps
ordrumbox-logic   editor with undo/redo, generators, MIDI files     → core
ordrumbox-engine  kits, WAV, passes, engine, UI ↔ audio link        → core, dsp
ordrumbox-probe   sound measurements (command line, not shipped)    → core, dsp, engine
ordrumbox-ui      session, views, input, MCP link, update check     → core, logic, engine, fontdue
ordrumbox-app     window, input translation, audio output           → core, engine, ui, winit, softbuffer, cpal
ordrumbox-plugin  CLAP/VST3 instrument following the host           → engine, ui, nih-plug, baseview, softbuffer
xtask             `cargo xtask bundle`: plugin bundles, assets in   → nih_plug_xtask
assets/           data/*.json and kits/**.wav, read at runtime
tools/            scripts that regenerate the golden fixtures
```

- The four domain crates take **no third-party dependency**. Only
  `ordrumbox-app` talks to the OS (window, screen, sound device), through
  `winit`, `softbuffer` and `cpal`.
  `ordrumbox-plugin` talks to a plugin host instead, through `nih-plug`
  (pinned git revision) and `baseview`; the `vst3` feature links GPLv3
  bindings (`README.md`, "License"), so it stays off by default.
- `ordrumbox-ui` is the whole UI without a window or a sound device:
  `Session`, the views drawn into a pixel buffer, and `Ui`, which takes the
  neutral input of `ui_input.rs` (keys, modifiers, pointer, wheel). Its only
  third-party dependency is `fontdue` (text in the system's monospace font,
  read at start: nothing bundled). The program gives it its sound output
  (`AudioOutput`, started by a `StartOutput`) and translates its window
  events; `main.rs` does nothing else.
- The update check (`ordrumbox-ui/src/update.rs`) runs the system's `curl`
  on a thread of its own, never on the UI or audio thread; it only tells,
  never downloads. A release is a `v<version>` tag matching `Cargo.toml`
  (`README.md`, "Commands").
- The MCP link (`ordrumbox-ui/src/mcp_link.rs`) listens on `127.0.0.1`
  only, on threads of its own; the tool calls run on the UI thread between
  frames (`session_mcp.rs`), each one undo step. The tools that only edit
  the library are `ordrumbox-logic/src/tools.rs`, tested without the app;
  `session_mcp.rs` keeps those that play, touch files, kits, synth presets
  or the master, or change what the window shows. `ordrumbox --mcp` is the
  stdio bridge an MCP client starts (`README.md`, "AI clients").
- Dependencies only point down the table. Nothing depends on
  `ordrumbox-app`; `dsp` uses `core` only as a dev-dependency.
- Workspace lints (`Cargo.toml`): `unsafe_code = "forbid"`,
  `missing_debug_implementations = "warn"`, clippy `all`. `ordrumbox-plugin`
  repeats them with `unsafe_code = "deny"`: only the export macros and the
  window handle adapters (`editor.rs`) allow it, each with a `SAFETY` comment.

## Audio thread

- Playback runs on two threads, which share state only through
  `ordrumbox-engine/src/controller.rs`:
  - `Controller`, on the UI thread, builds passes (this allocates) and posts
    them ahead of time;
  - `AudioSide`, in the cpal callback, takes them with `try_lock`, renders,
    and publishes its position.
- Under a plugin host, `AudioSide::render_synced` follows the host's
  transport (`HostTransport`): it stops with the host, and seeks inside the pass it
  plays. `Controller::update`, called even while stopped, starts playback,
  moves it to the host's pass and rebuilds passes at the host's tempo.
  In the plugin, a thread of its own (`player.rs`, `Ticker`) calls it, through `Session::update`, every
  10 ms, editor open or not; `process()` only calls `render_synced` and
  never takes the player's lock. Engines are made and dropped in
  `initialize()`, on the host's main thread.
- **Nothing reached from the audio callback allocates, frees, takes a
  blocking lock, or does I/O.** This covers `Engine::render` and every
  render path of `ordrumbox-dsp`. Buffers are sized before the stream starts.
- The UI side holds a reference to everything it posted (`Arc<Pass>`,
  `Arc<SampleBank>`) until the engine is surely done with it, so the last
  reference is never dropped on the audio thread.
- Values the UI reads together (pass, frame, frame count) are published as one
  consistent snapshot, not as independent atomics.

## Errors

- No failure is swallowed: no `let _ =` on a `Result`, no `.ok()` or
  `unwrap_or_default()` that turns an I/O or parse error into "nothing".
- Where an error goes:
  - loading a library, pattern or kit: an `Issue`
    (`ordrumbox-core/src/record.rs`) with its JSON path; the value takes its
    default and the issue is shown;
  - startup: the session's `status` list; an action: `Session::message`;
  - in between: `Result<_, String>` or `io::Result`, up to the app.
- `eprintln!` is not a report: the Windows release build has no console.
- A clamp or a fallback that changes what the user asked for (tempo range,
  track count, steps per beat) is visible: the UI shows the value used, or the
  input is refused with a message.
- `expect`/`unwrap` only for invariants (a built-in constant that must parse,
  covered by a test), never on files or user input.
- Clippy checks these rules (`unwrap_used`, `expect_used`, `print_stderr`,
  `let_underscore_must_use` in `Cargo.toml`). An invariant is marked where it
  is, `#[expect(clippy::expect_used, reason = "…")]` on its function. Tests are
  exempt: `clippy.toml` for `#[test]` and `#[cfg(test)]` code, a
  `#![allow(…)]` at the top of each file in `tests/`.

## UI layout

- No layout shift, anywhere. A control or a row never moves because of a
  state change: a sample replaced by a synth, a value that
  appears or goes, an analysis still running, a playing or stopped transport.
  A block that can be empty keeps its place (drawn empty or greyed out); a
  text that varies in length gets a fixed width and is fitted to it.
- The window's minimum size (`view::min_window_size`,
  `TRACK_EDITOR_HEIGHT`) covers the tallest state, not the most common one.
  A test that changes the state checks that the rects under it stay put
  (`editors.rs`, `the_rows_stay_put_without_a_sample`).

## Files

- The library, kits, synth presets and settings are JSON, written with the
  in-house codec (`ordrumbox-core/src/json.rs`), camelCase keys. `README.md`
  lists the key names and the user data folder.
- A record's keys are declared once, in its `json_record!` table
  (`track.rs`, `note.rs`): field, default, key. Alternative spellings after
  `|` are read, never written; files from the web version use them. Add no new
  ones.
- Writes go through `store::write_atomic`. A file that cannot be read or parsed
  is reported, never replaced by defaults and saved over.

## Golden fixtures

- The `tests/fixtures/` of each crate are committed; tests need nothing outside
  this repository.
- The JS app (ordrumbox-v2) is no longer the reference for behaviour. The
  fixtures the scripts in `tools/` generated from it (`README.md`, "Golden
  fixtures") are non-regression tests only, to be replaced over time by pure
  Rust tests (snapshots, property tests). New tests are written in Rust; add no
  JS fixture, and don't re-run a script to follow the JS app.
- Never edit a generated fixture by hand; the hand-written ones are named in
  the README.
- A test against a fixture that fails signals a change of behaviour: check
  whether it is meant. If not, the code is wrong. DSP outputs must match within
  2e-5; everything else matches exactly.
- A change meant on purpose moves the cases concerned to a Rust snapshot or a
  property test, and the README notes the departure. Never loosen a tolerance
  or regenerate a JS fixture to make a test pass. "The JS app does it" is no
  reason to keep a behaviour.
- The synth voice and some effects are such departures (`README.md`,
  "Golden fixtures", "Effects"): their cases are checked against
  `synth_renders.bin` and `effect_renders.bin`, written from the Rust
  processors. Rewrite them only for a change of sound you mean to make.

## Code conventions

- English only, in code, comments and names.
- Rust naming: `snake_case` functions, fields and modules; `PascalCase`
  types; `SCREAMING_SNAKE_CASE` constants.
- A number of things ends in `_count` (`beat_count`, `track_count`,
  `retrigger_count`), never `nb_` or `num_`. A length or a duration ends in
  its unit (`pattern_ticks`, `decay_frames`, `delay_beats`,
  `length_measures`); a clip's `measure_count` is both. An index ends in `_index`
  (`pattern_index`, `kit_index`), not `_idx`; in a short scope, `p` (pattern),
  `t` (track) and `n` (note) are fine.
- Music vocabulary: a measure is a **measure**, never a bar (`bar` is UI
  chrome only: `bar_height`, `slider_bar`). An audio file is a **sample**, a
  soft-synth sound is a **synth** preset, a set of samples is a **kit**.
- Doc comments say what a thing is or does, in plain sentences; comments in
  bodies say why. Match the comment density of the surrounding code.
- One shared function instead of a repeated expression (a clamp range, a row
  hit test, a post-to-engine sequence); a named constant instead of a repeated
  literal.
- Unit tests sit with the code (`#[cfg(test)] mod tests`); golden and
  end-to-end tests go in each crate's `tests/`.
