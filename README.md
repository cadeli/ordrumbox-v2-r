# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `song.json` codec, pattern engine, JSON, RNG | none |
| `ordrumbox-logic` | commands + undo, generators, MIDI files | core |
| `ordrumbox-dsp` | sample voice, channel strip (no allocation in render paths) | none |
| `ordrumbox-engine` | WAV decoding, kits, passes, sample-accurate engine, UI ↔ audio link | core, dsp |
| `ordrumbox-app` | window, software-rendered UI, audio output → `ordrumbox.exe` | winit, softbuffer, cpal |

The four domain crates have no third-party dependency. Only the app talks to
the OS, through small bridges (`winit`, `softbuffer`, `cpal`; `midir` with MIDI).

## Running

The app reads the `assets` folder of ordrumbox-v2 (`data/song.json`,
`data/drumkits.json`, `data/scales.json`, `kits/**.wav`). It looks, in order,
in `$ORDRUMBOX_ASSETS`, in `assets/` next to the program, in `assets/` in the
working folder, then in `../ordrumbox-v2/assets` (so `cargo run` works from
this repository when both checkouts sit side by side).

Keys: **Space** play/stop, **←/→** previous/next pattern, **↑/↓** tempo
(**Shift** ×10), **K** next kit.

## Commands

```sh
cargo run -p ordrumbox-app            # the app (debug, with console)
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo build --release -p ordrumbox-app  # target/release/ordrumbox(.exe)
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

## License

GPL-3.0-only, like ordrumbox-v2.
