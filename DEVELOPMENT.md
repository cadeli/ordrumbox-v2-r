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
holds the Windows zip (`ordrumbox.exe`, `orDrumbox.clap`, `assets/`, `LICENSE`),
the Windows setup made from the same files, the Linux zip (the same files, the
binary linked at run time against the system's ALSA and X11 libraries), and the
macOS and Linux CLAPs, zipped. Both CLAPs are built without `vst3`, so they stay
MIT; the VST3 bundles are only artifacts of the CI run.

The setup is `installer/ordrumbox.iss`, built by Inno Setup 6 (Windows only), with
the version of `Cargo.toml`:

```sh
ISCC.exe /DAppVersion=0.1.1 installer\ordrumbox.iss   # dist\ → setup\ordrumbox-v0.1.1-windows-x64-setup.exe
```

It reads `dist\` as CI makes it. Its `AppId` never changes: a new setup updates the
installed version through it. To try it without a release, run the workflow by
hand (Actions > CI > Run workflow): the setup is the `ordrumbox-windows-setup`
artifact.

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
`ordrumbox-ui/src/assign_tests/data.rs`.

## Snapshots

Tests need nothing outside the repository. Expected outputs are Rust snapshots in each
crate's `tests/fixtures/`, each written by an `--ignored` test of the file that checks
it; see `AGENTS.md` for the rules (never edit a snapshot by hand, rewrite one only for
a meant change of behaviour, and note that change below).

| Crate | Snapshots | Inputs |
|---|---|---|
| core | `flat_notes_rust.txt`, `library_rust.json`, `edge_library_rust.json`, `sub_positions_rust.txt` | `edge_library.json`, `engine_cases.json`, `song.json` |
| dsp | `effect_renders.bin`, `synth_renders.bin` | |
| engine | `analysis_rust.txt`, `mapping_rust.txt`, `pattern_renders.bin`, `synth_editor_rust.txt` | `generated_sounds.json`, `kick.wav` |
| logic | `compose_rust.txt`, `composition_rust.txt`, `generators_rust.txt`, `midi_rust.txt` | `generator_cases.json`, `midi/`, `midi_exports.json` |

### Changes of behaviour

The state-variable filter of the synth voice and of the channel strip is now the
textbook one. Its highpass (and so its notch) used to let some of the bandpass
through, more of it the higher the cutoff: a highpass now sounds a little thinner
and quieter (the synth presets `SN`, `CHH_SYNTH` and `OHH_SYNTH` lose up to 1 dB).
A synth cutoff moved by an LFO or an envelope now stops at 20 kHz like one that is
not; it used to reach 0.45 × the sample rate. This rewrote `synth_renders.bin` and
`effect_renders.bin` (dsp).

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
track) is kept before both. This rewrote the table of `ordrumbox-ui/src/assign_tests/data.rs`.

The strip's compressor has two cases of its own, `comp` and `comp_parallel`, added
after `delay_pingpong` to `effect_renders.bin` (dsp); the other cases are unchanged.

The whole chain has a snapshot of its own, `pattern_renders.bin` (engine): hand-written
patterns and a song rendered as the WAV export does, from the notes to the master bus.
It is new: no other snapshot changed.

A song's loop of less than one measure in a file (`"loopMeasures": 0.5`) is now no loop;
it used to be read as a loop of 0 measures, which played as none but was saved as `0`
and dropped by the save after. No snapshot changed.

A file's numbers are bounded when it loads. A setting past ±1e9 is reported and takes its
default. So is a length past what a file may hold: 1024 beats for a pattern or a track,
96 steps a beat, 98304 steps for a loop, beat 1024 and step 98304 for a note's place
(either side of 0), 64 hits for a retrigger. A song's clip that ends past measure 10000
is left out and reported, a loop past it is no loop. A synth preset's levels (master
volume, oscillator and sub gains, noise mix) are brought inside 0–1, the range of their
controls. These used to be taken as written: a track of a billion beats ran out of
memory, a level of 1e308 played samples that were not numbers. No snapshot changed.

The keys change: Ctrl+- / Ctrl+= (and the keypad's) zoom, as everywhere else, where
they used to change the pattern's length — the length has no key any more, the Song
panel and the menus change it. The letters that change every track (B, C, D, G, H, J,
K, V, X) now want Shift, as G H J K already did; S alone solos and M alone mutes the
track under the cursor, and Ctrl+1–7 run what F1–F7 do. Shift+arrows on a selected
note are up / down the pitch and left / right one step in time (they used to be up /
down the velocity and left / right the pitch), and dragging a note follows the piano
roll's axes: up / down the pitch, sideways the velocity. On macOS the menus show
Cmd+ (the key that runs them), redo is Cmd+Shift+Z, and export MIDI needs Shift
(Cmd+M minimizes the window). No snapshot changed; the unit tests of the keys and
the drags were rewritten with them.

Transport, selection and window keys. Shift+arrows in the grid now grow a zone from the
cursor; nudging the selected note (pitch, one step in time) moved to Alt+arrows, in a plugin
too. Ctrl+A selects the whole pattern, Ctrl+click on a step takes it into the zone, Ctrl+W
quits, F11 is full screen, F8 taps the tempo, F9 is the metronome, Ctrl+R records, Home and the
keypad's Enter rewind (the kit editor keeps its Enter). A song stopped keeps its place: the
cursor goes to the measure it was in, so Space goes on from there where it used to start over
from the cursor set by hand. A song's loop (Shift+click on the ruler) is made in the controller
(`Controller::set_song_loop`): a pass keeps the sequence number the engine counts and holds the
measure of the loop. The metronome is built into the passes (`ordrumbox-engine/src/metronome.rs`),
a synth click on a strip after the tracks' (none when the pattern has 64 tracks). The output
device is a setting (`audioDevice`) read by `ordrumbox-app` each time it starts the sound; Edit >
Preferences (`session_prefs.rs`) chooses it, and a new one reloads the session. The MIDI input
(`midiInput`, `ordrumbox-app/src/midi.rs`, `midir`) is opened by the program, which feeds the notes to
the audio callback through a lock-free ring (`AudioSide::trigger`, the same path as a plugin host's
MIDI) and to the UI thread through a channel, where `Session::midi_note` writes them when recording.
`Controller::set_live_midi` makes the controller post the pattern's key map without a host. No snapshot changed: the passes of a song without a loop or a
metronome are built as before.
