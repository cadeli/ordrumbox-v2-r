# AGENTS.md — orDrumbox

Drum machine and step sequencer: patterns of tracks of notes, songs arranged
from patterns, sample kits and a soft synth, played live or rendered to WAV
and MIDI. A native program, `ordrumbox.exe` on Windows (also runs on macOS),
and a CLAP/VST3 plugin, both with the same software-rendered UI. `README.md`
is the user documentation: the app, its keys, and where it reads and writes
its files. Everything for developers (building, tests, snapshots) is in
`DEVELOPMENT.md`, never in `README.md`.

## Commands

```bash
cargo run -p ordrumbox-app              # the app; finds assets/ from anywhere in the checkout
cargo test --workspace                  # all tests, snapshots included
cargo clippy --workspace --all-targets  # must be clean: CI sets RUSTFLAGS=-D warnings
cargo fmt --all                         # rustfmt.toml: max_width 120; CI checks it
cargo xtask bundle ordrumbox-plugin --release --features vst3   # target/bundled/orDrumbox.{clap,vst3}
```

CI (`.github/workflows/ci.yml`) runs fmt, clippy and tests on Windows and
macOS. For a `v*` tag or a manual run (`workflow_dispatch`) only, it then
builds the release `ordrumbox.exe` with `assets/` next to it, its Windows setup
(`installer/ordrumbox.iss`, Inno Setup), and the plugin bundles, checked by
`clap-validator`.

## Layout

```
ordrumbox-core    data model, JSON codec, pattern engine, RNG       no deps
ordrumbox-dsp     sample voice, synth voice, channel strip, master  no deps
ordrumbox-logic   editor with undo/redo, generators, MIDI files     → core
ordrumbox-engine  kits, WAV, passes, engine, UI ↔ audio link        → core, dsp
ordrumbox-probe   sound measurements (command line, not shipped)    → core, dsp, engine
ordrumbox-ui      session, views, input, MCP link, update check     → core, logic, engine, fontdue
ordrumbox-app     window, input translation, audio output, MIDI in  → core, engine, ui, winit, softbuffer, cpal, midir
ordrumbox-plugin  CLAP/VST3 instrument following the host           → core, engine, ui, nih-plug, baseview, softbuffer
xtask             `cargo xtask bundle`: plugin bundles, assets in   → nih_plug_xtask
assets/           data/*.json and kits/**.wav, read at runtime
```

- The four domain crates (`core`, `dsp`, `logic`, `engine`) take **no
  third-party dependency** and touch no file, socket or process: what they
  read comes in as bytes, text or a `read` function. Only `ordrumbox-app`
  opens a window, a sound device or a MIDI port, through `winit`,
  `softbuffer`, `cpal` and `midir`. `ordrumbox-ui` reaches the OS for the
  rest: files (`store.rs`), the MCP socket, the system font, `curl`.
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
  (`DEVELOPMENT.md`, "Commands").
- The MCP link (`ordrumbox-ui/src/mcp_link.rs`) listens on `127.0.0.1`
  only, on threads of its own; the tool calls run on the UI thread between
  frames (`session_mcp.rs`), each one undo step. The tools that only edit
  the library are `ordrumbox-logic/src/tools.rs`, tested without the app;
  `session_mcp.rs` keeps those that play, touch files, kits, synth presets
  or the master, or change what the window shows. `ordrumbox --mcp` is the
  stdio bridge an MCP client starts (`docs/mcp-guide.html`).
- An MCP client writes notes itself; it uses the automatic generation only
  when the user asks for it explicitly. The server says so in `INSTRUCTIONS`
  (`mcp.rs`), and starts the description of every tool of `GENERATION_TOOLS`
  with `GENERATION_RULE`: a new tool that generates, regenerates or
  randomizes notes joins that list.
- Dependencies only point down the table. Nothing depends on `ordrumbox-app`.
- Workspace lints (`Cargo.toml`): `unsafe_code = "forbid"`,
  `missing_debug_implementations = "warn"`, clippy `all` (`float_cmp`
  allowed) and the four lints of "Errors". `ordrumbox-plugin`
  repeats them with `unsafe_code = "deny"`: only the window handle adapters
  (`editor.rs`) take an `#[expect(unsafe_code)]`, each with a `SAFETY`
  comment (the export macros' `unsafe` comes from nih-plug, which the lint
  does not see).

## Kits and samples

- Kits are folders of `assets/kits/`, declared in `assets/data/kits.json`.
  A kit sample is 16- or 24-bit PCM, mono, 44.1 kHz, peak at −3 dB, starting at
  once and fading out to silence.
- `ordrumbox-probe` (not shipped) checks and repairs them:

  ```bash
  cargo run -p ordrumbox-probe -- kit human              # what is worth a look in a kit
  cargo run -p ordrumbox-probe -- fix assets/kits/human  # what fix would repair
  cargo run -p ordrumbox-probe -- fix assets/kits/human --write
  ```

  `fix` repairs what `kit` flags: a late start is trimmed to 1 ms before the
  sound, a loud end is faded out (130 ms, at most half the sample), DC is
  removed, the peak is set to −3 dB (`--peak keep` leaves it). `--highpass
  <hz>` and `--length <ms>` also filter and cut. It saves over the files only
  with `--write`; `--help` lists the options.
- A sample added to or removed from a kit changes which sample each track
  plays, in that kit and in the kits that borrow it: the table of
  `ordrumbox-ui/src/assign_tests/data.rs` is then rewritten, as a meant change.

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
  While the host is stopped, it posts a cue (the pass the host would start
  in), which the engine starts on the host's first rolling block by itself.
  In the plugin, a thread of its own (`player.rs`, `Ticker`) calls it, through `Session::update`, every
  10 ms, editor open or not; `process()` only calls `render_synced` and
  never takes the player's lock. Engines are made and dropped in
  `initialize()`, on the host's main thread.
- **Nothing reached from the audio callback allocates, frees, takes a
  blocking lock, or does I/O.** This covers `Engine::render` and every
  render path of `ordrumbox-dsp`. Buffers are sized before the stream starts.
  One exception, in the plugin only: when the host renders ahead of real
  time (a bounce, `ProcessMode::Offline`, or a measured lead of 250 ms:
  `lockstep.rs`, `Pace`), `process()` asks the `Ticker` for a tick and waits
  for it (`Steps`, with a timeout) before it renders, so the passes of the
  block are there however fast the host goes. It still never allocates. In
  real time it never waits.
- The render path is written for speed first, then for elegance; a modern
  idiom that costs a cycle per sample stays out. Concretely:
  - Work per block, not per sample. Coefficients that depend on parameters
    (`exp`, `pow`, `tan`, dB to gain) are computed once per block, or when
    the parameter changes, never inside the sample loop.
  - Hoist decisions out of the loop: `match` on the waveform, filter mode or
    interpolation once, then run one tight loop per case (a generic or a
    `const` parameter is fine; a per-sample `match` or closure is not).
  - Let the compiler drop bounds checks without `unsafe`: zip the buffers,
    or re-slice them to one length (`let left = &mut left[..frames];`)
    before the loop. No `get_unchecked`: `unsafe` stays forbidden.
  - Keep samples `f32` and buffers contiguous. Use `f64` where precision
    builds up (phase, time, long sums), as `ordrumbox-dsp` does. Changing a
    type's precision is a change of behaviour (snapshot, `DEVELOPMENT.md`).
  - No panic on the render path: no `unwrap`, no division without a guarded
    denominator, no index that the block length does not bound.
  - Guard feedback paths (delay, filters, reverb) against denormals.
  - `#[inline]` on small functions called per sample across crates
    (`dsp` → `engine`); `#[inline(always)]` only with a measurement.
- A change to a render path keeps the audio snapshots within 2e-5 and is
  timed in release (`cargo test --release`, or `ordrumbox-probe`), not in
  the `dev` profile.
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
- The layout's size (`view::min_window_size`, `TRACK_EDITOR_HEIGHT`) covers
  the tallest state, not the most common one. A window smaller than it
  (down to `MIN_WINDOW`, `scroll.rs`) scrolls; the layout never shrinks.
  A test that changes the state checks that the rects under it stay put
  (`editors/tests.rs`, `the_rows_stay_put_without_a_sample`).

## Files

- The library, kits, synth presets and settings are JSON, written with the
  in-house codec (`ordrumbox-core/src/json.rs`), camelCase keys. `README.md`
  gives the user data folder.
- A record's keys are declared once, in its `json_record!` table
  (`track.rs`, `note.rs`): field, default, key. Alternative spellings after
  `|` are read, never written; files from the web version use them. Add no new
  ones.
- Writes go through `store::write_atomic`. A file that cannot be read or parsed
  is reported, never replaced by defaults and saved over.

## Snapshots

- The `tests/fixtures/` of each crate are committed; tests need nothing outside
  this repository. `DEVELOPMENT.md`, "Snapshots", lists them.
- Expected outputs are Rust snapshots (`*_rust.txt`, a suffix kept from the
  time there were JS ones, and `*_renders.bin`), which
  each test writes with its `--ignored` test, tables in unit tests, and
  property tests. The JS app (ordrumbox-v2) is not the reference for
  behaviour: "the JS app does it" is no reason to keep or change one. Add no
  fixture from it.
- A snapshot that fails signals a change of behaviour: check whether it is
  meant. If not, the code is wrong. Audio outputs must match within 2e-5;
  everything else matches exactly. Never loosen a tolerance.
- Rewrite a snapshot only for a change of behaviour you mean to make, and
  note it in `DEVELOPMENT.md` ("Changes of behaviour"). Never edit one by hand;
  the inputs are named there.

## Modern Rust

- Edition 2024, `rust-version` 1.88: use what they give. `let … else`,
  `if let … && …` chains, `is_some_and`, `clamp`, `total_cmp`, `rem_euclid`
  (through `num::wrap`; `ordrumbox-dsp` has its own `wrap`), `div_euclid`,
  `checked_*`/`saturating_*`.
- No emulation of another language's numbers. No ToInt32/Uint8 tricks, no
  `((x % n) + n) % n`, no NaN as "missing": absence is an `Option`, a choice
  is an `enum`, not a `bool` pair or a magic integer.
- Conversions say their intent. Use `From`/`f64::from` when nothing is lost,
  and `try_from` when the value comes from a file or the user. Use `as` only
  when truncation or saturation is what you want, behind a named helper when
  it repeats (`data_byte`).
- Iterators and slice patterns over index loops (`zip`, `chunks_exact`,
  `windows`, `split_at_mut`); an index is for when the position itself
  matters.
- Generics and `impl Trait` over `dyn Trait`; `dyn` only where the set of
  types is open (plugin host, UI widgets), never on a render path.
- `#[expect(lint, reason = "…")]`, never a bare `#[allow]`.
- `#[must_use]` on functions that only compute a value; `const fn` and
  `const` tables where the value is known at compile time.

## Structure: do and do not

Decoupling, file size, naming, reuse. A rule here is checked in review; a
file or function that breaks one carries a comment saying why.

### Decoupling

- **Do** give each module one reason to change, and say it in its first doc
  line. **Do** put a type with the functions that act on it (`impl` in the
  module of the type), not in a "utils" or "helpers" module.
- **Do** depend on the narrowest thing: pass `&Pattern`, a slice, or a small
  struct of what the function reads, not the whole `Session`. A function that
  needs two fields of `Session` takes the two fields.
- **Do** keep logic that needs no window, no sound device and no file in the
  domain crates (`core`, `dsp`, `logic`, `engine`), where it is tested without the app.
  The UI calls it; it does not re-implement it.
- **Do** use a trait or a generic only at a seam that already exists (see
  Modern Rust); otherwise a plain function and a struct.
- **Do not** reach into another module's fields. Give the type a method or a
  getter instead; keep fields private (`pub(crate)` at most, never `pub` by
  habit).
- **Do not** add a dependency that points up the table of "Layout", or a
  cycle between modules of a crate. **Do** re-export (`pub use`) only once,
  to shape a crate's public surface in `lib.rs`; **do not** create internal
  aliases: one type, one home.
- **Do not** let a view mutate the model directly. A view reads state and
  returns a command; `Session` applies it as one undo step.
- **Do not** use a global, a `static mut`, a `thread_local` or a singleton to
  avoid passing a parameter.

### File and function size

- **Do** keep a file under about 500 lines of code (tests excluded) and a
  function under about 60. Past 800 lines, or when a file holds two
  separate ideas, split it before adding to it.
- **Do** split by concept, as a folder module (`foo.rs` + `foo/`), with a
  name that says what the part is: `session_roll.rs` and `session_song.rs`
  are parts of the session, not `session2.rs`. The parent keeps the type and
  the public surface; the parts keep the details.
- **Do** move tests that outgrow their file to `foo/tests.rs` or the crate's
  `tests/`; move big literal tables to a `data.rs` next to them.
- **Do not** grow an existing large file to add a feature: put the feature in
  a new module and call it from there. **Do not** split by size alone
  (`part1`, `part2`, `misc`, `more`): a split must follow a seam.
- **Do not** nest deeper than three levels of `if`/`match`/loop: extract a
  function, use an early `return`, `let … else`, or `?`.

### Naming

- **Do** give one concept one name across all crates, and look for it before
  coining another (`rg` the term). `pattern`, `track`, `note`, `kit`, `sample`,
  `synth`, `measure`, `clip` mean one thing each, as in "Code conventions".
- **Do** name an accessor after the field, with no `get_` prefix (`name()`,
  `set_name()`).
- **Do** use the same verb for the same action: `new` / `from_*` build,
  `to_*` / `as_*` / `into_*` convert, `is_*` / `has_*` ask a yes/no, `set_*`
  writes one value, `update` / `update_*` brings a state up to date,
  `apply` / `apply_*` runs a command, `draw` / `draw_*` paints, `key` /
  `*_key` / `pointer*` takes input, `load_*` / `save_*` touch files.
- **Do** name files after the main type or idea they hold; a prefix groups
  the parts of one subject (`session_*`, `editors/`). A test file is
  `foo/tests.rs`, ends in `_tests`, or lives in the crate's `tests/`.
- **Do** name a function for what it does, a type for what it is, a `bool`
  as a question (`is_playing`, not `playing_flag`).
- **Do not** abbreviate beyond the short names of "Code conventions" (`p`, `t`,
  `n`); no `mgr`, `ctx`, `tmp`, `util`, `helper`, and no bare `data` or
  `info`, as a name. A name that says which data is fine (`data_byte`);
  `data.rs` is only for the literal tables of "File and function size".
- **Do not** name the same thing two ways in two places (`step`/`slot`,
  `volume`/`gain`/`level` for the same value): rename one to match the other
  in the same change.

### Reuse

- **Do** search before writing: `rg` the verb and the type for an existing
  function, constant or test helper, and extend it if it is close.
- **Do** extract at the second copy of a rule that must stay the same
  everywhere (a clamp, a hit test, a unit conversion, a layout rect), and at
  the third copy of any other block of more than about 3 lines. **Do** put
  it in the lowest crate that can own it, so every layer above can call it.
  `core` and `dsp` share no crate: the rare helper both need (`wrap`) exists
  in each, under the same name.
- **Do** make a shared function small and general: parameters for what
  varies, no `bool` flag that switches two behaviours (that is two functions,
  or an `enum`).
- **Do** use the standard library and the iterator adapters before a loop of
  your own, and one shared test helper (`fixtures`, builders) before a new
  setup in each test.
- **Do not** add a near-duplicate function beside an existing one (a copy
  with a few edits, `draw_row_2`, `foo_alt`); change the original, or give
  both a shared core.
- **Do not** write a wrapper that only forwards a call, or a trait with one
  implementation and no seam. **Do not** abstract before the second use: three
  similar lines are better than a premature framework, but a rule repeated is
  a bug waiting.
- **Do not** leave dead code, `_unused` parameters or a commented-out block
  behind: delete it; git remembers.

## Code conventions

- English only, in code, comments and names.
- Rust naming: `snake_case` functions, fields and modules; `PascalCase`
  types; `SCREAMING_SNAKE_CASE` constants.
- A number of things ends in `_count` (`beat_count`, `track_count`,
  `retrigger_count`), never `nb_` or `num_`. A length or a duration ends in
  its unit (`pattern_ticks`, `decay_frames`, `delay_beats`, a song's
  `length_measures`). The beats a pattern is made of and the measures a
  clip is given are counts: `beat_count`, `measure_count`. An index ends in `_index`
  (`pattern_index`, `kit_index`), not `_idx`; in a short scope, `p` (pattern),
  `t` (track) and `n` (note) are fine.
- Music vocabulary: a measure is a **measure**, never a bar (`bar` is UI
  chrome only: `bar_height`, `slider_bar`). An audio file is a **sample**, a
  soft-synth sound is a **synth** preset, a set of samples is a **kit**.
- Doc comments say what a thing is or does, in plain sentences; comments in
  bodies say why. Match the comment density of the surrounding code.
- One shared function for a rule that repeats (a clamp range, a row hit
  test, a post-to-engine sequence), from its second copy ("Reuse"); a named
  constant instead of a repeated literal.
- Unit tests sit with the code (`#[cfg(test)] mod tests`); snapshot and
  end-to-end tests go in each crate's `tests/`.
