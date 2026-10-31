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
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on Windows and
macOS, then builds the release `ordrumbox.exe` with `assets/` next to it.

## Layout

```
ordrumbox-core    data model, JSON codec, pattern engine, RNG       no deps
ordrumbox-dsp     sample voice, synth voice, channel strip, master  no deps
ordrumbox-logic   editor with undo/redo, generators, MIDI files     → core
ordrumbox-engine  kits, WAV, passes, engine, UI ↔ audio link        → core, dsp
ordrumbox-probe   sound measurements (command line, not shipped)    → core, dsp, engine
ordrumbox-app     window, UI, audio output                          → core, logic, engine, winit, softbuffer, cpal, fontdue
assets/           data/*.json and kits/**.wav, read at runtime
tools/            scripts that regenerate the golden fixtures
```

- The four domain crates take **no third-party dependency**. Only
  `ordrumbox-app` talks to the OS (window, screen, sound device), through
  `winit`, `softbuffer`, `cpal` and `fontdue` (text in the system's monospace font, read at start: nothing bundled).
- The update check (`ordrumbox-app/src/update.rs`) runs the system's `curl`
  on a thread of its own, never on the UI or audio thread; it only tells,
  never downloads. A release is a `v<version>` tag matching `Cargo.toml`
  (`README.md`, "Commands").
- The MCP link (`ordrumbox-app/src/mcp_link.rs`) listens on `127.0.0.1`
  only, on threads of its own; the tool calls run on the UI thread between
  frames (`session_mcp.rs`), each one undo step. `ordrumbox --mcp` is the
  stdio bridge an MCP client starts (`README.md`, "AI clients").
- Dependencies only point down the table. Nothing depends on
  `ordrumbox-app`; `dsp` uses `core` only as a dev-dependency.
- Workspace lints (`Cargo.toml`): `unsafe_code = "forbid"`,
  `missing_debug_implementations = "warn"`, clippy `all`.

## Audio thread

- Playback runs on two threads, which share state only through
  `ordrumbox-engine/src/controller.rs`:
  - `Controller`, on the UI thread, builds passes (this allocates) and posts
    them ahead of time;
  - `AudioSide`, in the cpal callback, takes them with `try_lock`, renders,
    and publishes its position.
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
- They are generated by the scripts in `tools/` (`README.md`, "Golden
  fixtures"). Never edit a generated fixture by hand; the hand-written ones are
  named in the README.
- A test against a fixture that fails signals a change of behaviour. By default
  the code is wrong, not the fixture. DSP outputs must match within 2e-5;
  everything else matches exactly.
- A change meant on purpose moves the cases concerned to a Rust snapshot or a
  property test, and the README notes the departure. Never loosen a tolerance
  or regenerate a JS fixture to make a test pass.
- The synth voice is such a departure (`README.md`, "Golden fixtures"): its
  cases are checked against `synth_renders.bin`, written from the Rust voice.
  Rewrite it only for a change of sound you mean to make.

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
