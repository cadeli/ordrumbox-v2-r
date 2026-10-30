# orDrumbox (Rust)

Native port of [ordrumbox-v2](https://github.com/cadeli/ordrumbox-v2), the web
drum machine. Target: a single Windows `.exe` (developed on macOS, built for
Windows by CI).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `library.json` codec, pattern engine, song clips, instrument catalog, JSON, RNG, the JS sort and number formatting | none |
| `ordrumbox-logic` | the library being edited: commands with undo/redo; pattern generators (every instrument's variants, genre templates, auto mode loop by loop); Standard MIDI Files (read, import, export) | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip (filter, saturation, reverb, delay), master bus; no allocation in render paths | none |
| `ordrumbox-engine` | WAV decoding/encoding, kits and their files, sample analysis, sound auto-assignment, synth presets, passes (pattern and song: events, strip settings, LFO steps), sample-accurate engine, UI ↔ audio link, offline renders | core, dsp |
| `ordrumbox-app` | window, software-rendered UI (text in the system monospace font), audio output → `ordrumbox.exe` | winit, softbuffer, cpal, fontdue |

The four domain crates have no third-party dependency. Only the app talks to
the OS, through small bridges (`winit`, `softbuffer`, `cpal`, `fontdue`; `midir` with MIDI).

## Running

The app reads its `assets` folder, part of this repository: `data/library.json`
(the demo patterns and arrangement), `data/kits.json` (the kits and their
samples), `data/scales.json`, `data/synth_presets.json` and `kits/**.wav`. It
looks, in order, in `$ORDRUMBOX_ASSETS`, in `assets/` next to the program, in
`assets/` in the working folder, then in this repository's `assets/` (so
`cargo run` works from anywhere in the checkout).

The demo `library.json` is only the starting point: the library being edited is
saved (one second after each edit, and on quit) to `library.json` in the user
data folder — `%APPDATA%\orDrumbox` on Windows, `~/Library/Application
Support/orDrumbox` on macOS, `~/.local/share/ordrumbox` elsewhere, or the file
named by `$ORDRUMBOX_LIBRARY` — and loaded from there next time. Delete that
file to start again from the demo.

Key names: the files this app writes (`library.json`, `kits.json`, pattern
exports) name their settings after what they hold, in camelCase, and differ
from the JS app's (old name in brackets):

- track: `volume`/`volumeLfo` (`velocity`/`velocityLfo`), `transpose`/
  `transposeLfo` (`pitch`/`pitchLfo`), `samplePath` (`sampleId`),
  `autoAssign` (`useAutoAssignSound`), `useSynth` (`useSoftSynth`),
  `synthPreset` (`synthSoundKey`), `loopLength` (`loopAtStep`),
  `filterCutoff`/`filterCutoffLfo` (`filterFreq`…), `reverbMix`,
  `delayMix`, `saturationMix` (`reverbAmount`, `delayDepth`,
  `saturationAmount`), `delayBeats` (`delayTime`), `saturationOn` (`sat`),
  `hitVariation`/`noteVariation` (`variation`/`variation2`); auto mode:
  `autoMode` (`auto`), its chances (0–100) `autoPitchChance`,
  `autoVelocityChance`, `autoSilenceChance`, `autoFillChance`,
  `autoGhostChance`, `autoRetriggerChance`, `autoEuclideanChance`,
  `autoArpChance`, `autoNoteProbabilityChance`/`autoNoteProbabilityRange`
  (`prob_pitch`… `prob_note_depth`), `autoVariant`, `autoDensity`,
  `autoHitVariation`, `autoNoteVariation`, `pitchRange`, `pitchScaleLock`
  (`auto_variant`…); `fxSelected` (an editor tab) is dropped;
- note: `step` (`beatStep`), `cycle`/`cycleOffset` (`every`/`pos`),
  `probability` (`prob`), `retriggerRate` (`rate`), `retriggerProbability`
  (`arpTriggerProbability`), `euclideanPulses` (`euclideanFill`),
  `arpScale`, `arpDirection` (`_arpScale`, `_arpType`), and an arpeggio's
  `direction` (`mode`);
- pattern: `autoGenre`, `autoKeyOffset`, `autoScale` (`_autoGen…`), no
  more `application`/`url` header; song: `loopMeasures`
  (`loopMeasureCount`), a clip's `patternId` (`pattern`); library: `info`
  (`infos`), `selectedPattern`/`selectedSong` (`…Idx`);
- kit: `samples` (`instruments`), and for a sample `path`, `displayName`,
  `transpose` (`url`, `display_name`, `tune`).

The old names are still read, so the JS assets and older saves load as
before; the JS app does not know the new ones and drops those settings from
a file written here.

Sounds: a track with auto-assign gets a sample of the selected kit by name,
then of another kit, then a catalog substitute, then any sample (each a
random pick among the candidates, made at play and on a kit change); a synth
track without a known preset gets a random one.

WAV exports (16-bit, 44.1 kHz, the pattern once or the whole song, no tail) go
to the folder picked when exporting. Every export asks for its folder: the last
one picked, at first `exports/` next to `library.json` if it exists, else the
home folder; **Enter** on `[ Use this folder ]` writes there. A file of the same
name is replaced only once confirmed.

What removes or replaces work asks first (**Enter** goes on, **Esc** or a click
cancels): deleting a track, a clip, a row, an arrangement, a kit's sample or a
synth preset; clearing a track or a step of several notes; random notes over a
track's notes; compacting tracks; loading or importing a song. A single note goes
at once. While a question is open, the MCP tools are refused.

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
melodic instruments on their own channels at the sample's root note) to a
folder picked. A `.mid` dropped on the window becomes new 32-beat patterns (up to
16), one track per instrument found (by GM program, drum key, then track
name), in one undo step. As in the JS app, the file's tempo is not read
(120 BPM).

The window keeps the same three parts whatever is edited: one main editor in
the top-left (the grid, the piano roll, the song panel, the synth editor, or
the kit editor over any of them — the **VIEW** buttons and the View menu pick
one), the track editor down the right edge (**F2** shows or hides it), and the
output panel as a strip along the bottom, always shown.

Kit editor (**Ctrl K**, the JS drumkit manager): the current kit's samples
(**↑↓**), and for the selected one its kit and instrument (lists), its gain
(dB), transpose (semitones) and decay (knobs), its note, length, peak and RMS
levels and envelope with the decay end; **Enter** plays it, **Delete**
removes it, **N** normalizes the kit to 0 dB, **E** exports the kit mapping
(`.json`), **R** assigns the tracks' sounds again. Dropped on the window, a
`.wav` joins the kit (**Shift**: replaces the selected sample), a folder of
WAV files becomes a kit, a kit mapping is loaded. Gain and transpose now play (the
JS sample voice uses them). Unlike the JS app, where these edits are lost on
reload, the kits are saved to `kits.json` and the audio of added, replaced or
normalized samples to `samples/` (32-bit float WAV), both in the user data
folder; `samples/` wins over the assets' `kits/`. Delete `kits.json` to start
again from the assets' kits.

Track editor (on the right, **F2** or **View › Track editor** shows or hides
it), for the track of the cursor or of the selected note: the sample bar
(note, length, peak, envelope with the decay end, **Load** replaces the
sample), the knob bar (volume, pan, transpose, the sample's decay) and the JS
tabs — **FX** (reverb, delay, saturation, filter, each with its on/off LED),
**Sound** (mono, auto-assign, instrument, sample, synth preset), **LFO** (an
LFO per setting: LED, wave, frequency, range, phase), **Loop** (steps per
beat, loop length, swing), **Gen** (auto mode; the manual variation settings,
or the Groove / Engine settings in auto mode). Under it, the note editor: the
selected note (else the step under the cursor, else the track's first note)
— velocity, pitch, pan, and the Trig, Retr, Eucl and Arp tabs. While
playing, a setting with an LFO shows the LFO's value. The grid has a volume
slider per track and a master volume (the output panel's volume, saved
with it). Right-click a track's name: rename, copy, paste, duplicate,
delete, randomize or clear it; right-click a step: copy, paste or delete its
notes, or add a note at a random pitch.

Knobs and sliders work as in the JS (`OrKnob`, `OrSlider`, checked against
them): drag a knob up or down (200 px for the whole range), click or drag a
slider; **Shift** drags finely (sliders too, which the JS range input cannot);
the mouse wheel and the arrow keys (or **- =**) step the value (**Shift** or
**Alt**: a tenth of a step, **Ctrl**: ten steps); a double-click restores the
default; a right-click (or **Enter**) asks for a number. **Tab** moves the
keyboard focus through the controls, **Esc** gives the keys back to the grid.
A list opens on a click (or **Enter**): arrows, a letter to jump, the wheel,
**Enter** to choose, **Esc** to close.

Song panel (**F3**, **View › Song** or **Song › Song view**, the JS song
view; **Esc** goes back to the grid): in the place of the grid, the
selected arrangement across the whole width — a ruler of measures, one row
per pattern it uses (named on the left; a pattern gone from the library keeps
its row, in red), each clip a bar over its measures — and under it the
library's patterns on the left and on the right the song's name and
description (a click edits them), its date, the arrangement list, and
**Save**, **Load**, **Save as**, **Export**, **Import**. A click on the ruler aims the cursor, where the song plays from
(**←→** move it a measure; while playing, it jumps there at once); while the
panel is shown **Space** plays the song. Right-click a pattern of the list
or a row's name: add it at the playhead (stopped: the cursor), or remove its
row; a clip: **Next** (the same clip right after it) or **Delete**; the bare
grid: add the row's pattern there. Drag a pattern of the list onto the
arrangement to add it at that measure, on its own row; drag a clip sideways to
move it, whole measures at a time (the wheel scrolls during the drag; the grid
shows a page past the end to drop on). A click on a pattern selects it, a
double-click renames it (**↑↓** step through them). The wheel scrolls the
measures (**Shift**: the rows); while the song plays the grid follows it page
by page. Every edit is an undo step, and moves the loop onto the end of the
last clip, as in the JS (`song_commands.js`, checked against it). **Save**
writes the song (patterns, arrangements, what is selected) to
`songs/<name>.odbox` in the user data folder, where the JS app uses
IndexedDB; **Load** lists them; **Save as** and **Export** write the same file
to a folder picked; **Import** (or a `.odbox` dropped on the window) reads one; loading
replaces the library in one undo step. Beyond the JS panel, **New** and
**Delete** add and remove arrangements — the JS `addSong` / `removeSong`
commands, which only its MCP tools reach. The **Song** menu has all of these
too, with or without the panel shown: play the song, new, remove, previous and
next arrangement, rename the song, its description; save, load, export and
import are in the **File** menu.

Piano roll (**F4**, **View › Piano roll**, the JS piano roll; **Esc**
clears the selection, then goes back to the grid): in the place of the
grid, the track editor's track (the list next to the title picks another)
on a grid of steps, a page of 4 beats at a time (**< >**, or **Shift** +
wheel), and pitches from MIDI 12 to 108 with a keyboard on the left (the
wheel scrolls it; it opens on the track's pitch). Notes are drawn by
velocity, a bar on top marks a probability (purple) or a cycle (blue), the
retriggers, arpeggio steps and euclidean fills are ghosts at their steps
and pitches, and while playing the playhead moves (the page follows) and
the notes sounding light up. A click on an empty cell adds a note, on a
note selects it (and plays it), on the selected note deletes it; dragging a
note changes its velocity (up, down) or its pitch (sideways, a semitone
every 12 px), the first movement choosing which, one undo step per drag,
its value shown over the note. A click on a key plays it. Right-click a key:
**Clear all**, or **Add sequence** — the track replaced by a chord
progression from that key, one chord per measure, the next progression
each time (I-V-vi-IV, I-vi-IV-V, …); a cell: **Add note** or **Delete
note**, **Add minor chord**, **Add major chord**. **Arrows** move the
cursor (wrapping; the note there is selected), **Shift + arrows** change the
selected note's velocity (up, down) and pitch (left, right), **Enter**
adds, selects or deletes at the cursor, **Delete** deletes the selected
note. The note editor follows the selected note. The ghosts, the
progressions and the drag are the JS code's (checked against it). Unlike
the JS, a drag never counts as a click: there the window's mouseup resets
the drag before the click arrives, so `consumeClick()` never sees it.

Synth editor (**F6**, **View › Synth**, the JS soft-synth editor; **Esc**
goes back to the grid): the preset of the track editor's track (else the
first one), in the place of the grid, the track editor beside it. The preset
bar picks another preset and deletes, creates, duplicates, reverts (to the
preset as it was when opened), exports (`ordrumbox-synth-sounds.json`, to a
folder picked) and imports presets. Four tabs of cards — **OSC** (the scope: four cycles of
the oscillators, with their FM, sub and drive; VCO1–3; FM), **FLT** (the
filter and its response curve, the filter envelope, the mod envelope),
**MOD** (LFO1, LFO2, noise), **ENV** (master, the envelope and its shape) —
each with its wave, filter type or FM algorithm choices, its power button,
its knobs, lists and switches, as the JS lays them out. A knob an LFO moves
is purple and follows it. Touching a control of a group that is off switches
it on (an oscillator to 0.5, FM to 0.3, the noise to 0.15, an LFO onto the
filter, the mod envelope onto the filter), as the JS does. Every change
plays at once and is saved half a second later to `synth_presets.json`
in the user data folder (an older `generated_sounds.json` there is read when
it is missing; the JS keeps them in IndexedDB), read at start over the
assets' presets. The pills under the cards show the modules in use.

Output panel (a strip along the bottom of the window, always shown — the JS
output panel): the master bus in three tabs — **Vol**: the master
volume and the spectrum of what plays (the JS analyser's
`getByteFrequencyData`, computed as the Web Audio specification describes
it: 4096 samples, Blackman window, smoothing 0.8, -100 to -30 dB, the first
256 bins); **Comp**: the pre-gain, the compressor's transfer curve
(-60 to 0 dB in, the unity line, the threshold), its on/off button and its
threshold, ratio, attack, release, knee and make-up; **Flt**: the low cut
and the high cut. They play at once and are saved half a second after the
last change to `settings.json` in the user data folder (`settings.master`,
as the JS keeps them; the file's other settings are kept), and loaded at
start. The grid's master volume is this volume.

The header, under the menu bar, is a row of labelled groups after the logo
(the JS toolbar and its pattern settings panel): **PLAY** / **STOP**, **BPM**
(the tempo knob, 20–250 as the JS slider), **PATTERN** (the pattern list,
and where it is in the library), **BEATS** (1–16), **KIT** (the kit list),
**GENERATE** (**DRUM**, **BASS**, **CHORDS**, lit while their tracks are in
auto mode: the D, S, A keys; then the genre) and **VIEW** (**GRID**,
**ROLL**, **SONG**, **SYNTH**, **KIT**: the main editor shown is lit, as
F3–F6 do; **KIT** toggles the kit editor, as Ctrl+K does).
A list opens on a click; the wheel or the arrows move, **Enter** picks. The
line under it shows the last action, undo and redo, and the song.

The look (`theme.rs`, every colour of the app): a deep indigo ground,
panels a step lighter, and neon accents with one role each — pink for
titles, the selection and slider handles, cyan for labels, sliders and
knobs, lime for play, what is on and the playhead, yellow for values, the
active tab and button; each track has its colour (pink, cyan, yellow,
violet, orange, lime, round again): its stripe in the grid, its notes (by
velocity), its notes in the piano roll, its clips in the song panel. Flat
colours, no glow. This is a redesign: the JS app looks otherwise. **Pattern › Rename pattern...** and **Pattern
description...** edit the pattern's name and description; **Pattern ›
Compact tracks** (the JS tools panel's button) shortens every track whose
notes repeat to its shortest loop — the loop length set, the notes past it
removed, one undo step per track (`compactTrack`, checked against the JS).

Menus: the menu bar at the top of the window (File, Edit, View, Pattern,
Generate, Play, Song, Kit, Help) lists every command with its shortcut; commands that cannot
run are greyed out, on/off ones (auto modes, mute, solo, the current kit, the
kit editor) show a box. Every command that reads or writes a file (songs,
MIDI, WAV, samples, kit folders and mappings) is in **File**. Click a title, or press **F10** or **Alt** alone, then
**←→** between menus, **↑↓** between items, **Enter** to run, **Esc** to
close. **Help › Keyboard shortcuts** (**F1**) lists every key; **Help › About**
shows the version and the folders in use. A click on the status line (under
the controls) lists what was done since the app started, newest first; the
wheel, the arrows, **Page Up/Down** and **Home/End** scroll it.

Updates: at start, at most once a day, the app asks GitHub for the latest
release of this repository (with the system's `curl`, in Windows 10 and
later and in macOS; the request carries only `User-Agent:
orDrumbox/<version>`). A newer one is shown in the status lines, and **Help ›
Download orDrumbox …** opens its page; nothing is downloaded or installed by
the app. **Help › Check for updates** checks now. `settings.json` keeps it
under `updates`: `"check": false` stops the check at start, `lastCheck` is
when it last ran (seconds since 1970).

AI clients (MCP): an MCP client (Claude Desktop, Claude Code, Cursor…) can
drive the running app, as the JS app's `ordrumboxMcpserver.mjs` did its
files. The client starts `ordrumbox --mcp`, which speaks the Model Context
Protocol on its standard input and output and passes each tool call to the
window: the edit is shown at once, is one step of **Edit › Undo**, and is
saved with the library. The window listens on `127.0.0.1` only, on a port
it picks at start and writes to `mcp.json` in the user data folder with a
token a connection must give; with the window closed, a call answers that
orDrumbox is not running. In the client's settings (the path is the
program's, `ordrumbox.exe` on Windows):

```json
{ "mcpServers": { "ordrumbox": { "command": "/path/to/ordrumbox", "args": ["--mcp"] } } }
```

The tools keep the JS names: `listPatterns`, `loadPattern`,
`createNewPattern`, `selectPattern` (shows it in the window),
`addNotesToPattern`, `updateTrack`, `setPatternBpm`, `setPatternBeatCount`,
`setPatternTags`, `setPatternDescription`, `savePatternToJson` (to
`exports/`), `listAllInstrumentsNames`, `listKitSamples`, `analyzeSamples`,
`listArrangements`, `createArrangement`, `addPatternToArrangement`,
`removePatternFromArrangement`; and for the live app `selectKit`, `play`,
`stop`, `undo`. `setColorScheme` is gone: this app has one theme. Track and
note settings take the key names of the files (`volume`, `transpose`,
`cycle`, `probability`…; the JS names are read too), and a value outside its
range is refused (notes) or brought inside it, the answer saying the value
used (tracks, tempo).

Commands ending in "..." (import a MIDI file or a song, add or replace a
sample, import a folder as a kit, load a kit mapping) open the built-in file picker: **↑↓**,
**PageUp/PageDown**, **Home/End** or the mouse wheel to move, a letter to jump,
**Enter** (or a second click) to open a folder or choose, **Backspace** for the
parent folder, **Esc** to cancel. It lists only the files the command takes;
on Windows the drives are listed above a drive's root. It starts in the home
folder, then where it last chose.

Under each track's name, the grid shows what it plays: its sample (`kit/file`,
as assigned, perhaps from another kit) or its synth preset (`synth BASS5`).

Mouse: a click on an empty step adds a note, on a note selects it (and plays
it), on the selected note deletes it; **M**/**S** boxes mute/solo; a click on
a name selects the track (and plays it). Over a note, a bubble shows its
pitch, MIDI number and the settings that differ from the defaults (the JS
`formatNoteTooltip`, checked against it).

| Keys | |
|---|---|
| **Space** | play / stop the pattern (**Shift**, or in the song panel: the song, from its cursor) |
| **[ ]** | previous / next pattern (**Shift**: song) |
| **- =** | tempo −1 / +1 (**Shift** ×10) |
| **, .** | previous / next kit |
| **B** | generate a new pattern |
| **F**, **G** | a random pattern, a random kit |
| **H** | every track on a synth patch (by its type: KICK → BASS0, SNARE → SN…; random for the others) |
| **J** | every track back on auto-assign |
| **K** | a random sample (of any kit) on every track |
| **V** | VU meters on the track stripes (on / off) |
| **D**, **S**, **A** | drums, bass, chords: generate in auto mode / auto mode off |
| **Z**, **X** | auto mode of the focused track, random notes on it |
| **1–9** | mute track 1–9 (**Shift**: solo), in every view |
| **Q–P** | play track 1–10, in every view (the kit editor keeps N, R, E) |
| **arrows** | move the grid cursor |
| **Enter** | like a click at the cursor |
| **Shift + ↑↓ / ←→** | velocity ±0.05 / pitch ±1 of the selected note |
| **Delete** | clear the cursor step |
| **Esc** | deselect |
| **Ctrl Z**, **Ctrl Y** (or **Ctrl Shift Z**) | undo, redo (50 steps) |
| **Ctrl C / V** | copy / paste the cursor step (**Shift**: the track) |
| **Ctrl T**, **Ctrl Backspace** | add a track, remove the focused track |
| **Ctrl G**, **Ctrl L** | next grid (steps per beat), loop one step shorter |
| **Ctrl - =** | pattern length −1 / +1 beat |
| **Ctrl N**, **Ctrl D** | new pattern, duplicate pattern |
| **Ctrl S** | export the pattern as JSON (`ordrumbox-<name>.json` in a folder picked, as the JS); the library is saved on its own, **File › Save** saves now |
| **Ctrl E** | export the pattern to WAV (**Shift**: the song) |
| **Ctrl M** | export the pattern to a MIDI file |
| **Ctrl K** | kit editor (open / close) |
| **Ctrl O** | import a MIDI file (file picker) |
| **Ctrl Q** | save and quit |
| **F10**, **Alt** | open the menus |
| **F1** | the list of keys |
| **F2** | track editor (shown / hidden) |
| **F3** | song panel (shown / hidden) |
| **F4** | piano roll (shown / hidden) |
| **F5** | grid (back to the main editor) |
| **F6** | synth editor (shown / hidden) |
| **Tab**, **Shift Tab** | focus the next / previous control of the editors |
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
cargo run --release -p ordrumbox-engine --example render -- assets rock 2 rock.wav
cargo run --release -p ordrumbox-engine --example render -- assets song:demo demo.wav
```

Windows `.exe`: every push to `main` builds it in CI (artifact
`ordrumbox-windows-x64`, the program with its `assets` folder next to it);
a local build needs the `assets` folder copied next to it.

A release: set `version` in the root `Cargo.toml` (`0.2.0`), commit, then
`git tag v0.2.0 && git push origin v0.2.0`. CI checks that the tag is the
version, then publishes a GitHub release with
`ordrumbox-v0.2.0-windows-x64.zip`; the apps already installed find it
through their update check.

Locally from
macOS: `cargo install cargo-xwin`, then
`cargo xwin build --release -p ordrumbox-app --target x86_64-pc-windows-msvc`.

## Golden fixtures

The fixtures are committed: the tests need nothing outside this repository.
They were produced by the JS app (ordrumbox-v2), the reference of the port,
with the scripts of `tools/`, which need a checkout of it only to make them
again. They hold the JS names of the keys; the tests compare after
`web_keys::current_keys` (`crates/ordrumbox-core/tests/support/`).

The JS app is the reference. `tools/export_fixtures.mjs` runs its own code on
`assets/data/song.json` and writes `crates/ordrumbox-core/tests/fixtures/`:

```sh
node tools/export_fixtures.mjs ../ordrumbox-v2
```

- `song.json` / `song.min.json`: the JSON writer must reproduce both byte for byte.
- `library.export.json`: `song.json` loaded by the app (`fixPatterns`, `fixSongs`)
  and saved by its exporter; `Library` must hold the same values (compared
  after `web_keys::current_keys` (`crates/ordrumbox-core/tests/support/`), the JS key names being the web version's).
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
- `ordrumbox-logic/tests/fixtures/compact.json`: `compactTrack` on every
  track of every pattern of `song.json` and of hand-made patterns that
  repeat, loaded as the app boots (`importPatternFromJson`: every note has a
  `stepPercent`, which the comparison of notes includes): the steps recorded,
  then each track's loop point and notes.

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

The knobs and sliders have an eighth:

```sh
node tools/export_control_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-app/tests/fixtures/controls.json`: the JS `OrKnob` and `OrSlider`
  driven through a minimal fake DOM — drags (normal and fine, linear and
  logarithmic), arrow keys with every modifier, typed numbers, the knob's arc
  — and the value texts of every formatter (`fmt`, `knobFormat`, `fmtVal`,
  `formatSignedDb`, the loop point label), and `formatNoteTooltip` for a set of
  notes and track pitches.

The piano roll has a tenth:

```sh
node tools/export_piano_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-core/tests/fixtures/sub_positions.json`: `getNoteSubPositions()`
  (the ghosts) on every track of `song.json` and on hand-made notes
  (retriggers at every rate, euclidean fills and rotations, loop points,
  every arpeggio form; a random arpeggio's pitches are drawn, so only its
  steps are compared).
- `ordrumbox-logic/tests/fixtures/composition.json`: `getSequence()` and
  `buildSequenceNotes()` for every progression, tonic and length.
- `ordrumbox-app/tests/fixtures/piano_roll.json`: `NoteDrag` driven through a
  fake window (velocity, pitch, the axis lock, the bounds), and `midiName()`.

The synth editor has an eleventh:

```sh
node tools/export_synth_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-app/tests/fixtures/synth_editor.json`: `SynthPresetModel`
  on every preset of `generated_sounds.json` and hand-made ones (the defaults
  filled in, the cards bypassed), `getOrderedGroupNames()`, the LFO targets
  and their scales, and `computeLfoMod()` for every target, wave and sync at
  several times.

The song panel's commands have a ninth:

```sh
node tools/export_song_fixtures.mjs ../ordrumbox-v2
```

- `ordrumbox-logic/tests/fixtures/song_edits.json`: the JS `SongCommands`
  (clips added by id or name, repeated, removed one by one, by measure or by
  row; arrangements added, removed and selected; what each one refuses) on
  `song.json`'s arrangements, then undo all, then redo all; after each, the
  undo/redo sizes, the step's description, what the command returned, the
  selected arrangement and every arrangement (with its loop length, which an
  edit may leave fractional). Also `songMeasureAtTick()`.

## License

MIT: see `LICENSE`.
