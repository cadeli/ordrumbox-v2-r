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
| `xtask` | plugin bundling; `audit`, the checks of `AGENTS.md` | nih-plug's bundler |

The domain crates take no third-party dependency.

## Commands

```sh
cargo run -p ordrumbox-app             # the app
cargo test --workspace                 # all tests
cargo clippy --workspace --all-targets # must be clean (CI: RUSTFLAGS=-D warnings)
cargo fmt --all                        # rustfmt (max_width 120)
cargo xtask audit                      # AGENTS.md's rules that a tool can check
cargo build --release -p ordrumbox-app # target/release/ordrumbox(.exe)
```

`cargo xtask audit` checks, over every crate: the dependencies against the
table above, the banned names (`nb_`, `num_`, `_idx`, `get_`, `ctx`, `bar` for
a measure…), `#[allow]`, `static mut` and `thread_local`, the size of files
(about 500 lines of code, 800 at most), the nesting of `if`/`match`/loops
(read from the indentation), duplicated blocks, and, through clippy, functions
over 60 lines (`clippy.toml`) or too complex, a value taken by value and not
used, a useless clone. Errors fail it (CI runs it with `--no-clippy`);
warnings are measures with known exceptions, and fail it with `--strict`.
`--all` lists every finding. A file exempt from a rule says why in a line
`// audit: allow(<rule>): <why>`, the rule named as the report names it.

The app reads its `assets/` folder (`data/*.json` and `kits/**.wav`) and looks
for it in `$ORDRUMBOX_ASSETS`, next to the program, in the working folder, then
in this repository's `assets/`. `$ORDRUMBOX_LIBRARY` names a library file to use
instead of the one in the user data folder.

A release is a `v<version>` tag matching `Cargo.toml`: CI then builds the release
`ordrumbox.exe` with `assets/` next to it, and the plugin bundles. The release
holds the Windows zip (`ordrumbox.exe`, `orDrumbox.clap`, `assets/`, `LICENSE`),
the Windows setup made from the same files, the Linux zip (the same files, the
binary linked at run time against the system's ALSA and X11 libraries, plus the
`.desktop` file and icon), the macOS `orDrumbox.app` (`installer/Info.plist` and
`ordrumbox.icns`; the binary and `assets/` in `Contents/MacOS`), and the macOS and
Linux CLAPs, zipped, and the VST3 bundles of the three systems, one zip each
(`ordrumbox-<tag>-<os>-vst3.zip`, `assets/` inside, with `LICENSE`, `LICENSE-GPL-3.0` and `installer/vst3-NOTICE.txt`). The icon of the window comes from `installer/ordrumbox-setup.png`,
decoded by `crates/ordrumbox-app/build.rs`; the `.exe` carries `ordrumbox.ico`. Both CLAPs are built without `vst3`, so they stay
MIT; the VST3 bundles are built with `vst3`, so they are GPLv3, and shipped
as separate zips: never ship the `.clap` that build leaves beside them.

The setup is `installer/ordrumbox.iss`, built by Inno Setup 6 (Windows only), with
the version of `Cargo.toml`:

```sh
ISCC.exe /DAppVersion=0.1.2 installer\ordrumbox.iss   # dist\ → setup\ordrumbox-v0.1.2-windows-x64-setup.exe
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

## Licenses

`THIRD-PARTY-LICENSES.txt` lists the licenses of every dependency, for every target and
feature of the releases (`about.toml`, `about.hbs`). It is generated, committed, and shipped
in each zip, the setup and the macOS app; CI fails when it is out of date. After a change to
`Cargo.lock`:

```sh
cargo install --locked cargo-about --features cli   # once
cargo about generate --all-features --workspace about.hbs -o THIRD-PARTY-LICENSES.txt
```

A new license must be added to `accepted` in `about.toml` on purpose.

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

### The synth, measured

A snapshot says the sound did not change, not that it is right. `ordrumbox-probe/tests/synth_audio.rs`
renders notes with the real voice (and, for the last test, a whole pattern through the engine, a
WAV file and back), measures them and compares the measures with what the settings say: pitch,
the harmonics of each wave, aliasing against a naive saw, the slope, notch, bandpass and resonance
of the filter against the filter's own formula, the envelope's times and levels, an LFO's rate and
depth, unison lines and stereo spread, and each source and target of the modulation matrix. Every
factory preset also plays a finite, silent-ended note at three pitches, the same one every time,
whatever the size of the blocks it is rendered in. No fixture, nothing to rewrite: a failure is a
wrong sound or a wrong threshold.

### Changes of behaviour

Choosing an effect's type (its screen's list or its ◂ ▸ arrows) switches the effect on, as it already did for
the filter and an LFO's wave: a reverb or a delay without an amount goes on at its `on_value`, one without a
type (`none`) at the type chosen. The type arrows of a reverb or a delay that was off used to leave it off.
Scenario tests only (`session_scenarios/cartridges.rs`); no snapshot changed.

An LFO knob shows what the LFO plays for a setting its object lacks or holds as a numeric string
(`lfo::setting`: `freq` and `max` 1, `min` and `phase` 0), where it showed NaN. The synth cartridge's
screen no longer panics on the wheel or an arrow key with no preset loaded. No snapshot changed.

A synth note is 3 dB quieter: its level (`peak_velocity`, `synth_preset.rs`) is trimmed by `SYNTH_TRIM`,
−3 dB, the peak a kit sample is normalised to, so a synth at full velocity peaks where a sample does instead
of 3 dB above it. Presets and patterns are unchanged. This rewrote `mapping_rust.txt` (engine: the `velocity` of
each `synth` line, ×0.708, nothing else) and `pattern_renders.bin` (engine); `synth_renders.bin` (dsp) renders
patches directly and is unchanged.

The synth has three chip controls, read from a preset when present and written only once set (like
`filter.slope`): `vco1`..`vco3` `width` (the pulse width of a square wave, 0.5 the plain square) and, in
`noise`, `mode` (`white`, `long`, `short`: the 15- and 7-bit shift registers of an 8-bit console) and `rate`
(its clock, Hz). A preset without them plays as before: the audio snapshots are unchanged. The factory presets
`CHIP_*` use them; `vco1`..`vco3` `width` and `noise.rate` are also LFO and matrix targets (`vcoN.width`, `noise.rate`,
after the others in `LfoTarget::ALL`, so no code moved), which rewrote `synth_editor_rust.txt` (engine: more targets
in the LFO lines; the audio renders are unchanged). Their names keep them out of the auto-assign, so no row of `assign_tests/data.rs` changed.

The synth editor has a `matrix` group (`source1`..`source8`, `target1`..`target8`,
`amount1`..`amount8`) in its defaults, so a hydrated preset holds it and the MOD tab has a
Matrix card. This rewrote `synth_editor_rust.txt` (engine); the audio renders are unchanged,
a preset whose lines are all `off` adds nothing to any control.

Four factory presets use the matrix (`BASS_TOUCH`, `BASS_WOBBLE`, `SYNTHPAD_DRIFT`, `PIANO_KEYBELL`). They
sort last in their families, so the auto-assign picks them as the last candidate of `BASS`, `SYNTHPAD`
and `PIANO` tracks: those rows of `assign_tests/data.rs` were rewritten.

The synth editor has a `unison` group (`count`, `detune`, `spread`) in its defaults, so a
hydrated preset holds it and the OSC tab has a Unison card. This rewrote
`synth_editor_rust.txt` (engine); the audio renders are unchanged, a preset without the
group has one layer. The factory preset `SYNTHLEAD_SUPERSAW` is the second `SYNTHLEAD` the
auto-assign picks, so the `SYNTHLEAD` rows of `assign_tests/data.rs` were rewritten.

A MIDI file's import now reads note lengths: a note lasts until the note-off
its file pairs it with (first in first out on its key, a velocity-0 note-on
counts as an off), written as `lengthSteps` to 2 decimals; a note with no
note-off plays one step, as before. This rewrote `midi_rust.txt` (logic).

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

The per-track sidechain (`Strip::process`'s `key`, `Pass::keys`) changes no snapshot: a
track with no key and no `duckAmount` renders as before.

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

The app's MIDI also goes out, and a clock comes in. Output (`midiOutput`, `midiClockOut`;
`ordrumbox-app/src/midi_out.rs`): after each chunk the callback takes the engine's notes
(`take_notes`, which a plugin already sends) and clock (`Engine::set_clock_out`, `take_clock`,
`engine/clock_out.rs`: 24 pulses a beat at the pass's frames, Start or Position + Continue on
`Engine::play`, Stop on `stop`, none under a plugin) and writes them, with their offset, to a ring;
a thread reads it every half millisecond, holds each message until its time and sends it. The
device's latency is not compensated. Input clock (`midiClockIn`, `midi_sync.rs`): the MIDI thread
feeds `ClockFollower` (`ordrumbox-engine/src/clock_in.rs`, a pure state machine, tested without a
port), the callback asks it for a `HostTransport` and calls `render_synced`, so the transport follows
a MIDI clock exactly as it follows a plugin host: the controller posts a cue while stopped, plays
at the tempo (a tenth of a beat a minute, with hysteresis, so jitter does not rebuild the passes)
and seeks past 32 frames of drift. `Session::set_clock_follow` and `Ui::idle` keep `update` running
while it follows; `Controller::leave_host` gives the library's tempo back when it stops.
The knobs (`ordrumbox-engine/src/controls.rs`, `ordrumbox-ui/src/session_controls.rs`, the
`midiControls` setting) make a `HostMix` the controller posts (`Controller::set_live_mix`): the
engine's host path, so no pass is rebuilt and nothing is an undo step. No snapshot changed: the
engine sends no clock unless asked, and `AudioSide::render` now sets the block it takes MIDI
from, as `render_synced` did.

A note has a length and a shift. `lengthSteps` (1 by default, in steps of its track, may be a
fraction) is how long it is held: a synth note before its release, the MIDI note the plugin
sends, the note a MIDI export writes; a sample plays to its own end. `shiftSteps` (0 by
default, half a step either way) moves the note and its repeats off the grid, in the hits
themselves (`expand_hits`), so every reader of the hits gets it. A MIDI note sent or exported
used to last a 16th whatever the track's grid; it now lasts a step of its track, so the notes
of tracks that are not 4 steps a beat changed length. This rewrote the export lines of
`midi_rust.txt` (logic); the audio renders and `flat_notes_rust.txt` are unchanged. The note
editor has a Timing tab (Length, Shift), the lane two more settings (LENGTH, SHIFT, its tabs
now in two columns), the piano roll draws a note as wide as it is long and its right edge
drags (`session_roll_length.rs`); Play > Record off the grid keeps a recorded note's timing as
its shift (off by default). A MIDI import still reads neither lengths nor timing.

A note has parameter locks: `filterLock` (0–1, 20 Hz to 20 kHz by octaves), `reverbLock` and
`delayLock` (0–1, the track's mix), each an `Option`, absent by default. `build_pass` gives the
events of a track that has any lock a `StripLock` (`strip_settings.rs`): all three values, each
the note's lock or the strip's own, so a lock lasts until the track's next note. The engine
applies it at the event's frame to the strip's target and to its current settings at once (a
voice start, not a ramp); a lock switches on the lowpass filter or the room reverb a track has
off, not a delay of type none. A cutoff LFO overrides the lock at the next quantum. The lane has
three more tabs (FILTER, REVERB, DELAY): a note without a lock has no bar, and a bar painted to
the bottom clears it. No snapshot changed.

Quantize (Edit > Quantize, `ordrumbox-logic/src/quantize.rs`, `session_note_tool.rs`) is a pure function of a pattern, a scope (zone, track, pattern), a grid and a strength; a note's place is its step plus `shiftSteps`, and what a strength leaves is kept as the shift. No snapshot changed.

Humanize (Edit > Humanize, `ordrumbox-logic/src/humanize.rs`, the same modal as Quantize in `session_note_tool.rs`) adds to each note of the scope a random shift (±0–30 % of a step, kept within half a step) and a random change of velocity (±0–30 % of it, at most 1), drawn from the session's generator `Rng`; the steps stay. The same seed gives the same notes (unit tests). No snapshot changed.
