# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `song.json` codec, pattern engine, song clips, instrument catalog, JSON, RNG, the JS sort and number formatting | none |
| `ordrumbox-logic` | the library being edited: commands with undo/redo; pattern generators (every instrument's variants, genre templates, auto mode loop by loop); Standard MIDI Files (read, import, export) | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip (filter, saturation, reverb, delay), master bus; no allocation in render paths | none |
| `ordrumbox-engine` | WAV decoding/encoding, kits and their files, sample analysis, sound auto-assignment, synth presets, passes (pattern and song: events, strip settings, LFO steps), sample-accurate engine, UI ↔ audio link, offline renders | core, dsp |
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

Sounds: a track with auto-assign gets a sample of the selected kit by name,
then of another kit, then a catalog substitute, then any sample (each a
random pick among the candidates, made at play and on a kit change); a synth
track without a known preset gets a random one.

WAV exports (16-bit, 44.1 kHz, the pattern once or the whole song, no tail) go
to `exports/` next to `library.json`.

Generation (the JS generators, same variants and same choices): **B** makes a
new pattern from a random genre's template (tracks, lines, key, scale);
**D**, **S**, **A** are the ↻ Drum, ↻ Bass and ↻ Chords buttons — new lines
for the drum tracks (the bass and chord tracks, created on the synth when
missing) and auto mode for them, or auto mode off. A track in auto mode (name
in green, **Z** toggles it) gets a new line at every loop of playback,
following the song form (intro, chorus, verse, break…: fills on the last loop
of a section, a snare crescendo on breaks); those lines are saved but are not
undo steps. Exports regenerate them on their own copy. **X** puts random notes
on the focused track.

MIDI files (the JS exporter and import, byte for byte): **Ctrl M** writes the
pattern as a type 1 file (96 PPQN; drums on channel 10 at their GM keys,
melodic instruments on their own channels at the sample's root note) to
`exports/`. A `.mid` dropped on the window becomes new 32-beat patterns (up to
16), one track per instrument found (by GM program, drum key, then track
name), in one undo step. As in the JS app, the file's tempo is not read
(120 BPM).

Kit editor (**Ctrl K**, the JS drumkit manager): the current kit's samples
(**↑↓**), and for the selected one its kit, instrument, gain (dB), tune
(semitones) and decay (**←→** to pick, **- =** to change, **Shift** finely),
its note, length, peak and RMS levels and envelope with the decay end;
**Enter** plays it, **Delete** removes it, **N** normalizes the kit to 0 dB,
**E** exports the kit mapping (`.json`), **R** assigns the tracks' sounds
again. Dropped on the window, a `.wav` joins the kit (**Shift**: replaces the
selected sample), a folder of WAV files becomes a kit, a kit mapping is
loaded. Gain and tune now play (the JS sample voice uses them). Unlike the
JS app, where these edits are lost on reload, the kits are saved to
`kits.json` and the audio of added, replaced or normalized samples to
`samples/` (32-bit float WAV), both in the user data folder; `samples/`
wins over the assets' `kits/`. Delete `kits.json` to start again from the
assets' kits.

Menus: the menu bar at the top of the window (File, Edit, Pattern, Generate,
Play, Kit, Help) lists every command with its shortcut; commands that cannot
run are greyed out, on/off ones (auto modes, mute, solo, the current kit, the
kit editor) show a box. Click a title, or press **F10** or **Alt** alone, then
**←→** between menus, **↑↓** between items, **Enter** to run, **Esc** to
close. **Help › Keyboard shortcuts** (**F1**) lists every key; **Help › About**
shows the version and the folders in use.

Commands ending in "..." (import a MIDI file, add or replace a sample, import a
folder as a kit, load a kit mapping) open the built-in file picker: **↑↓**,
**PageUp/PageDown**, **Home/End** or the mouse wheel to move, a letter to jump,
**Enter** (or a second click) to open a folder or choose, **Backspace** for the
parent folder, **Esc** to cancel. It lists only the files the command takes;
on Windows the drives are listed above a drive's root. It starts in the home
folder, then where it last chose.

Mouse: a click on an empty step adds a note, on a note selects it (and plays
it), on the selected note deletes it; **M**/**S** boxes mute/solo; a click on
a name selects the track (and plays it).

| Keys | |
|---|---|
| **Space** | play / stop the pattern (**Shift**: the song) |
| **[ ]** | previous / next pattern (**Shift**: song) |
| **- =** | tempo −1 / +1 (**Shift** ×10) |
| **, .** | previous / next kit |
| **B** | generate a new pattern |
| **D**, **S**, **A** | drums, bass, chords: generate in auto mode / auto mode off |
| **Z**, **X** | auto mode of the focused track, random notes on it |
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
| **Ctrl E** | export the pattern to WAV (**Shift**: the song) |
| **Ctrl M** | export the pattern to a MIDI file |
| **Ctrl K** | kit editor (open / close) |
| **Ctrl O** | import a MIDI file (file picker) |
| **Ctrl Q** | save and quit |
| **F10**, **Alt** | open the menus |
| **F1** | the list of keys |
| drop a `.mid` file on the window | import it (also a `.mid` given on the command line) |

Keys are read by position (like the JS app's shortcuts): on an AZERTY
keyboard, **[ ]** are the two keys right of **P**, **Q–P** start at **A**,
**A** and **Z** are **Q** and **W**.
**PageUp/PageDown** and the keypad **- +** work too. **Cmd** works for
**Ctrl** on macOS.

## Commands

```sh
cargo run -p ordrumbox-app            # the app (debug, with console)
cargo test --workspace
cargo clippy --workspace --all-targets
cargo fmt --all
cargo build --release -p ordrumbox-app  # target/release/ordrumbox(.exe)
# a pattern (2 loops) or a song rendered offline to a WAV file, with its speed
cargo run --release -p ordrumbox-engine --example render -- ../ordrumbox-v2/assets rock 2 rock.wav
cargo run --release -p ordrumbox-engine --example render -- ../ordrumbox-v2/assets song:demo demo.wav
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

Sound assignment, song playback and WAV encoding have a fourth script; it
also writes the instrument catalog the port reads (`ordrumbox-core/data/instruments.json`):

```sh
node tools/export_assign_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-core/tests/fixtures/instruments.json`: catalog lookups
  (`findById`, `findByName`, `findInstrumentFromFileName`, MIDI note and program).
- `ordrumbox-core/tests/fixtures/song_sources.json`: `resolveSongSources()`
  tick by tick for the demo song and a hand-made one.
- `ordrumbox-engine/tests/fixtures/assign.json`: `AutoAssign` for every track
  name in every kit, with `Math.random` pinned to the first and to the last
  candidate.
- `ordrumbox-engine/tests/fixtures/wav.json`: `bufferToWav()` bytes.

The generators have a fifth script; it also writes the variant tables the
port reads (`ordrumbox-logic/data/generators.json`):

```sh
node tools/export_gen_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/generators.json`, with `Math.random`
  replaced by a seeded stream (mulberry32, also in the Rust test): every
  variant of every generator on several track setups, densities and seeds;
  `generateRandom`; `generatePattern` on new and existing patterns; the
  player's loop-start regeneration over 30–46 loops (auto tracks, `autoGen`
  patterns); the ↻ Drum / Bass / Chords toggles. Rust must give the same
  notes, track settings and number of draws.

MIDI files have a sixth script:

```sh
node tools/export_midi_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/midi.json`: `parseMidi()` (events, names,
  note-ons, program changes) of hand-made files covering the parser's paths
  (running status, sysex, truncation, garbage) and of exports; the
  exporter's bytes for every song.json pattern it plays the same with any
  random stream, 1 and 3 loops; the patterns the import makes of each file;
  and `Array.prototype.sort` permutations — the exporter sorts with a
  comparator that contradicts itself, so the bytes depend on V8's TimSort,
  which `ordrumbox_core::js::sort_by` reproduces step by step.

The kit editor's sample analysis has a seventh script:

```sh
node tools/export_kit_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-engine/tests/fixtures/analysis.json`: `Analyzer.analyzeWavBuffer()`
  (envelope, length, peak and RMS dB, fundamental and its confidence) and the
  note of WAV files in every format the JS decodes (tones, noise, decays,
  silence, very short files, stereo, the kick.wav fixture). Rust decodes the
  same bytes and must report the same values exactly.

## License

GPL-3.0-only, like ordrumbox-v2.
