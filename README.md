# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `song.json` codec, pattern engine, JSON, RNG | none |
| `ordrumbox-logic` | the library being edited: commands with undo/redo (generators and MIDI files to come) | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip (filter, saturation, reverb, delay), master bus; no allocation in render paths | none |
| `ordrumbox-engine` | WAV decoding, kits, synth presets, passes (events, strip settings, LFO steps), sample-accurate engine, UI ↔ audio link | core, dsp |
| `ordrumbox-app` | window, software-rendered UI, audio output → `ordrumbox.exe` | winit, softbuffer, cpal |

The four domain crates have no third-party dependency. Only the app talks to
the OS, through small bridges (`winit`, `softbuffer`, `cpal`; `midir` with MIDI).

## Running

The app reads the `assets` folder of ordrumbox-v2 (`data/song.json`,
`data/drumkits.json`, `data/scales.json`, `data/generated_sounds.json`,
`kits/**.wav`). It looks, in order,
in `$ORDRUMBOX_ASSETS`, in `assets/` next to the program, in `assets/` in the
working folder, then in `../ordrumbox-v2/assets` (so `cargo run` works from
this repository when both checkouts sit side by side).

The demo `song.json` is only the starting point: the library being edited is
saved (one second after each edit, and on quit) to `library.json` in the user
data folder — `%APPDATA%\orDrumbox` on Windows, `~/Library/Application
Support/orDrumbox` on macOS, `~/.local/share/ordrumbox` elsewhere, or the file
named by `$ORDRUMBOX_LIBRARY` — and loaded from there next time. Delete that
file to start again from the demo.

Mouse: a click on an empty step adds a note, on a note selects it (and plays
it), on the selected note deletes it; **M**/**S** boxes mute/solo; a click on
a name selects the track (and plays it).

| Keys | |
|---|---|
| **Space** | play / stop |
| **[ ]** | previous / next pattern |
| **- =** | tempo −1 / +1 (**Shift** ×10) |
| **, .** | previous / next kit |
| **1–9** | mute track 1–9 (**Shift**: solo) |
| **Q–P** | play track 1–10 |
| **arrows** | move the grid cursor |
| **Enter** | like a click at the cursor |
| **Shift + ↑↓ / ←→** | velocity ±0.05 / pitch ±1 of the selected note |
| **Delete** | clear the cursor step |
| **Esc** | deselect |
| **Ctrl Z**, **Ctrl Y** (or **Ctrl Shift Z**) | undo, redo (50 steps) |
| **Ctrl C / V** | copy / paste the cursor step (**Shift**: the track) |
| **Ctrl T**, **Ctrl Backspace** | add a track, remove the focused track |
| **Ctrl G**, **Ctrl L** | next grid (steps per beat), loop point one step back |
| **Ctrl - =** | pattern length −1 / +1 beat |
| **Ctrl N**, **Ctrl D** | new pattern, duplicate pattern |
| **Ctrl S** | save now |

Keys are read by position (like the JS app's shortcuts): on an AZERTY
keyboard, **[ ]** are the two keys right of **P**, **Q–P** start at **A**.
**PageUp/PageDown** and the keypad **- +** work too. **Cmd** works for
**Ctrl** on macOS.

## Commands

```sh
cargo run -p ordrumbox-app            # the app (debug, with console)
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo build --release -p ordrumbox-app  # target/release/ordrumbox(.exe)
# a pattern rendered offline to a WAV file, with its speed
cargo run --release -p ordrumbox-engine --example render -- ../ordrumbox-v2/assets rock 8 rock.wav
```

Windows `.exe`: every push to `main` builds it in CI (artifact
`ordrumbox-windows-x64`); put the `assets` folder next to it. Locally from
macOS: `cargo install cargo-xwin`, then
`cargo xwin build --release -p ordrumbox-app --target x86_64-pc-windows-msvc`.

## Golden fixtures

The JS app is the reference. `tools/export_fixtures.mjs` runs its own code on
`assets/data/song.json` and writes `crates/ordrumbox-core/tests/fixtures/`:

```sh
node tools/export_fixtures.mjs ../ordrumbox-v2
```

- `song.json` / `song.min.json`: the JSON writer must reproduce both byte for byte.
- `library.export.json`: `song.json` loaded by the app (`fixPatterns`, `fixSongs`)
  and saved by its exporter; `Library` must hold the same values.
- `edge_library.json` (hand-written, kept by the script) and
  `edge_library.export.json`: the same check on cases `song.json` does not reach.
- `slugify.json`: pattern/song id slugs.
- `flat_notes/<id>.json`: `recomputeFlatNotes()` for passes 0–3 of every
  pattern of `song.json` and of `engine_cases.json` (hand-written, kept by the
  script), loaded the way the app boots (`fixPatterns` then
  `importPatternFromJson`). A pass is deterministic when three different
  `Math.random` streams give the same notes: Rust must give exactly those notes
  and draw no randomness. The other passes are checked by properties.

Re-run the script whenever the JS engine or `song.json` changes.

The audio processors have their own script, which runs the JS AudioWorklet
processors in Node (a minimal worklet scope, 128-frame quanta):

```sh
node tools/export_dsp_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-dsp/tests/fixtures/renders.{json,bin}`: strip, master bus and
  synth voice outputs (every preset of `generated_sounds.json`, every LFO
  target, FM algorithm and modulation target) for inputs both sides compute
  bit for bit. The Rust processors must match within 2e-5.
- `ordrumbox-engine/tests/fixtures/strip_settings.json`: the strip parameters
  `applyTrackToStrip()` sets for every track setup of `song.json`.
- `ordrumbox-engine/tests/fixtures/lfo_values.json`: `computeLfoValue()` over
  a range of ticks.
- `ordrumbox-engine/tests/fixtures/synth_patches.json`: the message
  `WorkletSynthVoice` sends to its processor for each preset.

The editing commands have a third script, which drives the JS `Commander`
and `HistoryManager`:

```sh
node tools/export_edit_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/edits.json`: 29 edits on `song.json`
  (notes, tracks, patterns), then undo all, then redo all; after each, the
  undo/redo sizes, the step description, the pattern ids and the saved
  patterns.

## License

GPL-3.0-only, like ordrumbox-v2.
