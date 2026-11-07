# orDrumbox v2 (Rust)

Note: the legacy orDrumbox V1, the old Java drum machine, has been largely
improved upon and superseded; it is no longer current. This Rust port is the
active version.

The JavaScript/web v(ersion https://github.com/cadeli/ordrumbox-v2), is a web demo of this Rust version: it is less complete than the downloadable (native) version.

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
The grip in its window's bottom-right corner resizes the window (down to 320×200, where
the layout scrolls); the project keeps the size.

MIDI goes both ways, with one place per track of the pattern playing (or selected):
a drum track is on channel 10 at its instrument's GM key (KICK 36, SNARE 38…), whatever
the note's pitch. Any other track (melodic, unknown name, a second track of one drum) has a
channel of its own (1–9, 11–16, in track order), its notes on the key of their pitch: the
track's pitch 0 is its sample's root note, C3 (48) without one and for a synth. Past the 15
channels, the tracks left take keys of channel 10 from 84 up.

- **In**: a note on a track's channel plays that track at the note's pitch; a note on any
  other channel plays the drum track with that key. At its velocity, whether the host plays
  or not.
- **Out**: the notes the pattern plays go out where they come in, with their pitch (the
  track's transpose and pitch LFO included), a 16th long. Tracks the host mutes send nothing.
  A clip recorded from this output and played back into the plugin plays the same tracks at
  the same pitches.
Building with `vst3` links GPLv3 bindings; the CLAP build stays MIT.

## Keys

Press **F1** in the app for the full keyboard shortcut list. Common keys:

- **Space**: play / stop
- **F2**: track editor, **F3**: song panel, **F4**: piano roll, **F5**: grid, **F6**: synth editor
- **Ctrl K**: kit editor, **Ctrl M**: export MIDI, **Ctrl E**: export WAV
- **Ctrl +** / **Ctrl Shift -**: zoom in / out a quarter at a time (×1, ×1.25,
  ×1.5… up to ×4), **Ctrl 0**: the display's scale, **View > Zoom: fit to
  window**: the biggest of these scales the window shows whole. The zoom does not depend
  on the window's size: a window too small for it gets scrollbars (drag them,
  click beside the thumb, or turn the wheel where nothing else takes it). A
  window goes down to 320×200, the app's and the plugin's. The
  zoom is kept in `settings.json` (`uiZoom`, scale units added to the
  display's, such as `0.5`; whole numbers from earlier versions still read). A
  display at 150 % shows at ×1.5. In the plugin, Alt stands for Ctrl (the host
  keeps Ctrl).

## Files

The app writes `library.json`, `kits.json`, `settings.json`, `synth_presets.json`,
`songs/`, `samples/` and `exports/` in the user data folder. Pattern and track
settings use camelCase keys; old (JS) spellings are still read.

## Tests and snapshots

Tests need nothing outside the repository. Expected outputs are Rust snapshots
in each crate's `tests/fixtures/`; see `AGENTS.md` for the rules (never edit a
snapshot by hand, rewrite only for a meant change of behaviour).

The triangle LFO now stays between its min and max, starting at min like the
sine; it used to swing over twice that range. This rewrote `synth_renders.bin`
(dsp), `mapping_rust.txt` and `synth_editor_rust.txt` (engine) and
`midi_rust.txt` (logic).

## License

MIT: see `LICENSE`. A plugin built with the `vst3` feature links GPLv3 VST3
bindings, so that `orDrumbox.vst3` is GPLv3 as a whole. The CLAP build stays
MIT.