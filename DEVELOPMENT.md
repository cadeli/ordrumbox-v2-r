# orDrumbox — development

Building, testing and changing orDrumbox. The user documentation is
[`README.md`](README.md) and [`docs/`](docs/); the rules for code changes are in
[`AGENTS.md`](AGENTS.md).

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

## Commands

```sh
cargo run -p ordrumbox-app             # the app
cargo test --workspace                 # all tests
cargo clippy --workspace --all-targets # must be clean (CI: RUSTFLAGS=-D warnings)
cargo fmt --all                        # rustfmt (max_width 120)
cargo build --release -p ordrumbox-app # target/release/ordrumbox(.exe)
```

The app reads its `assets/` folder (`data/*.json` and `kits/**.wav`) and looks
for it in `$ORDRUMBOX_ASSETS`, next to the program, in the working folder, then
in this repository's `assets/`. `$ORDRUMBOX_LIBRARY` names a library file to use
instead of the one in the user data folder.

A release is a `v<version>` tag matching `Cargo.toml`: CI then builds the release
`ordrumbox.exe` with `assets/` next to it, and the plugin bundles. The release
holds the Windows zip (`ordrumbox.exe`, `orDrumbox.clap`, `assets/`, `LICENSE`)
and the macOS CLAP bundle, zipped. Both CLAPs are built without `vst3`, so they
stay MIT; the VST3 bundles are only artifacts of the CI run.

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

Building with `vst3` links GPLv3 bindings; the CLAP build stays MIT (`README.md`,
"License").

## Instrument catalog

`ordrumbox-core/data/instruments.json`, built into the program, holds what the app knows of instruments: canonical ids
(`KICK`, `OHH`, `CONGAS`…), their name patterns, words, substitutes, GM MIDI keys and
default pan. It is edited by hand; `ordrumbox-core/src/instruments.rs` only reads it.
The sample each track gets in each kit is the hand-written table of
`ordrumbox-ui/src/assign_tests.rs`.

## Snapshots

Tests need nothing outside the repository. Expected outputs are Rust snapshots in each
crate's `tests/fixtures/`, each written by an `--ignored` test of the file that checks
it; see `AGENTS.md` for the rules (never edit a snapshot by hand, rewrite one only for
a meant change of behaviour, and note that change below).

| Crate | Snapshots | Inputs |
|---|---|---|
| core | `flat_notes_rust.txt`, `library_rust.json`, `edge_library_rust.json`, `sub_positions_rust.txt` | `edge_library.json`, `engine_cases.json`, `song.json` |
| dsp | `effect_renders.bin`, `synth_renders.bin` | |
| engine | `analysis_rust.txt`, `mapping_rust.txt`, `synth_editor_rust.txt` | `generated_sounds.json`, `kick.wav` |
| logic | `compose_rust.txt`, `composition_rust.txt`, `generators_rust.txt`, `midi_rust.txt` | `generator_cases.json`, `midi/`, `midi_exports.json` |

### Changes of behaviour

The triangle LFO now stays between its min and max, starting at min like the
sine; it used to swing over twice that range. This rewrote `synth_renders.bin`
(dsp), `mapping_rust.txt` and `synth_editor_rust.txt` (engine) and
`midi_rust.txt` (logic).

A track's delay is now set up (time, mode, feedback) even when its amount is 0, and
the strip has a `reverb_on` and a `delay_on` flag (0 for the `none` types), so that a
plugin host can raise an effect the pattern leaves silent. This rewrote
`mapping_rust.txt` (engine); the audio renders are unchanged.

A retrigger now fades: a note's `retriggerFade` (0–1, 0.6 by default, the Fade slider of
the note editor's Retrigger tab) lowers each repeat's velocity by `fade × i / count`, so
at 1 the repeats step down towards silence and at 0 they all play at the note's
velocity. Arpeggio notes and euclidean fills do not fade. Saved notes without the key
take 0.6. This rewrote `flat_notes_rust.txt` (core) and `midi_rust.txt` (logic).

A MIDI file's sysex events (`F0`/`F7`) are now skipped by their length; they used
to be read as two-byte events, which shifted every later event of the track (a GM
reset at the start moved the notes after it, or lost them). The program changes of
a format 0 file, all in its one track, are now read. An instrument that already has
a track gets another, numbered (`PERC 2`, `BASS 2`), where its notes used to be
dropped; a channel left with no instrument is named in the import message. This
rewrote `midi_rust.txt` (logic).

A track with auto-assign on now plays the synth preset of its name (`CLAP_SYNTH`,
`COWBELL_SYNTH`, `BASS_FM`…) when the selected kit has neither its instrument nor a
variant of it, where it used to borrow the sample of another kit; another kit's sample
is taken only when no preset has its name. A variant in the kit (its tom for a `HI_TOM`
track) is kept before both. This rewrote the table of `ordrumbox-ui/src/assign_tests.rs`.

The strip's compressor has two cases of its own, `comp` and `comp_parallel`, added
after `delay_pingpong` to `effect_renders.bin` (dsp); the other cases are unchanged.
