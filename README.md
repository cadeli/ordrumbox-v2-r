# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

Note: the legacy orDrumbox V1, the old Java drum machine, has been largely
improved upon and superseded; it is no longer current. This Rust port is the
active version.

## Download

- Download page: <https://www.ordrumbox.com/download/download.html>
- GitHub releases: <https://github.com/cadeli/ordrumbox-v2-r/releases>

The app is a single executable with its `assets/` folder next to it.

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `library.json` codec, pattern engine, song clips, instrument catalog, JSON, RNG, decimal rounding | none |
| `ordrumbox-logic` | library being edited: commands with undo/redo; pattern generators; Standard MIDI Files | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip, master bus | none |
| `ordrumbox-engine` | WAV, kits, sample analysis, synth presets, passes, engine, UI link, offline renders | core, dsp |
| `ordrumbox-probe` | sound measurements, command-line only | core, dsp, engine |
| `ordrumbox-ui` | session, views, input, MCP link, update check | core, logic, engine, fontdue |
| `ordrumbox-app` | window, input translation, audio output (`ordrumbox.exe`) | core, engine, ui, winit, softbuffer, cpal |
| `ordrumbox-plugin` | CLAP and VST3 instrument | engine, ui, nih-plug, baseview, softbuffer |
| `xtask` | plugin bundling | nih-plug's bundler |

The domain crates take no third-party dependency.

## Running

```sh
cargo run -p ordrumbox-app
```

The app reads its `assets/` folder (`data/*.json` and `kits/**.wav`) and looks
for it in `$ORDRUMBOX_ASSETS`, next to the program, in the working folder, then
in this repository's `assets/`. User data is saved under the OS user data folder
(`%APPDATA%\orDrumbox` on Windows, `~/Library/Application Support/orDrumbox` on
macOS, `~/.local/share/ordrumbox` elsewhere), or the file named by
`$ORDRUMBOX_LIBRARY`.

## Commands

```sh
cargo run -p ordrumbox-app            # the app
cargo test --workspace                # all tests
cargo clippy --workspace --all-targets # must be clean (CI: RUSTFLAGS=-D warnings)
cargo fmt --all                       # rustfmt (max_width 120)
cargo build --release -p ordrumbox-app # target/release/ordrumbox(.exe)
```

Offline render:

```sh
cargo run --release -p ordrumbox-engine --example render -- assets rock 2 rock.wav
cargo run --release -p ordrumbox-engine --example render -- assets song:demo demo.wav
```

## Plugin

```sh
cargo xtask bundle ordrumbox-plugin --release                 # target/bundled/orDrumbox.clap
cargo xtask bundle ordrumbox-plugin --release --features vst3 # and orDrumbox.vst3
```

The plugin follows the host transport and saves its library in the host project.
Building with `vst3` links GPLv3 bindings; the CLAP build stays MIT.

## Keys

Press **F1** in the app for the full keyboard shortcut list. Common keys:

- **Space**: play / stop
- **F2**: track editor, **F3**: song panel, **F4**: piano roll, **F5**: grid, **F6**: synth editor
- **Ctrl K**: kit editor, **Ctrl M**: export MIDI, **Ctrl E**: export WAV

## Files

The app writes `library.json`, `kits.json`, `settings.json`, `synth_presets.json`,
`songs/`, `samples/` and `exports/` in the user data folder. Pattern and track
settings use camelCase keys; old (JS) spellings are still read.

## Tests and snapshots

Tests need nothing outside the repository. Expected outputs are Rust snapshots
in each crate's `tests/fixtures/`; see `AGENTS.md` for the rules (never edit a
snapshot by hand, rewrite only for a meant change of behaviour).

## License

MIT: see `LICENSE`. A plugin built with the `vst3` feature links GPLv3 VST3
bindings, so that `orDrumbox.vst3` is GPLv3 as a whole. The CLAP build stays
MIT.