# orDrumbox — development

Building, testing and changing orDrumbox. The user documentation is
[`README.md`](README.md) and [`docs/`](docs/); the rules for code changes are in
[`AGENTS.md`](AGENTS.md).

## Crates

| Crate | Role | Dependencies |
|---|---|---|
| `ordrumbox-core` | data model, `library.json` codec, pattern engine, song clips, instrument catalog, JSON, RNG, decimal rounding | none |
| `ordrumbox-logic` | library being edited: commands with undo/redo; pattern generators; Standard MIDI Files | core |
| `ordrumbox-dsp` | sample voice, synth voice, channel strip, shared returns, master bus | none |
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

### Automation, measured

`ordrumbox-engine/tests/automation_renders.rs` renders songs whose lanes are read from their JSON as the WAV
export does (`offline::render_song`), through a 16-bit WAV file and back, and measures the measure where each
lane starts against the one before it: a mute and a probability of 0 silence, a solo silences the others,
a volume of 0.5 is −15 dB, a pan of 0 leaves the right side silent, a cutoff takes the highs off a noise, a
transpose of 0.75 plays a 440 Hz sine at 880 Hz, a swing of 0.25 beat moves an off-beat 0.125 s late, an echo
send feeds the echo, a saturation adds a third harmonic; lanes of an instrument the song lacks leave the file
the same, sample for sample. The sources are synthetic; the measures (`tests/signals/`) are shared with
`track_effects.rs`. Idempotence: `ordrumbox-core/tests/json_idempotence.rs` saves a song with a lane of every
control twice (the same text, the same lanes); `automation.rs` paints the same stroke twice (the same lane)
and repaints a lane's own values (no change); `ordrumbox-logic/src/arrangement/automation.rs` undoes and redoes
every kind of lane edit, twice over (the same library each time). No fixture, nothing to rewrite.

### Changes of behaviour

A double-click on a track's name in the grid opens the piano roll on that track (`press_track_name`,
`session_grid.rs`); a single press still selects the track and may drag it. No snapshot changed.

Tracks can send to two shared returns, a tape echo and a spring reverb (`ordrumbox-dsp/src/returns.rs`,
`ordrumbox-engine/src/engine/returns.rs`): `echoSend` and `reverbSend`, 0 by default, taken after the strip's volume
and pan; each return is a `Strip` with a `dry` level of 0 (a new strip setting, 1 by default: the strips play as
before). The returns' settings are `settings.returns` (`ReturnSettings`), the echo's time in beats at the tempo of the
pass playing. With every send at 0 the returns stay silent: no snapshot changed. The MIDI controls list has the echo's
feedback after the master volume, then six controls a track (`session_scenarios/midi_sync.rs` moved its indices).
The sends, the returns and the master bus are set in a main view of their own, the mixer (F12, View > Mixer, the
**MIX** button: `mixer.rs`, `session_mixer.rs`): a strip a track (sends, pan, mute, solo, a fader and a meter),
the echo's and the reverb's, the master's. It replaces the master strip along the bottom of the window (gone,
with View > Master and its Echo page) and keeps the track editor at 455 (`TRACK_EDITOR_HEIGHT`): at scale 1 the
smallest window is 64 pixels shorter (the strip) and 38 wider (the MIX button). A fader is a new control style
(`Style::Fader`); the master's low and high cut are knobs, no longer sliders. In a plugin the mixer shows the track
strips only: the host mixes and the returns and the master play at their defaults, as before.

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
measure of the loop. A pattern's loop and play cursor (the rulers of the grid and of the piano roll) are
made the same way (`Controller::set_pattern_loop`, `Controller::seek_pattern`): a pass is built whole, then cut
to its ticks (`controller/pattern_loop.rs`), and `Pass::offset_ticks` says where in the pattern it starts, which
`Controller::pattern_tick` and the playheads read. The metronome is built into the passes (`ordrumbox-engine/src/metronome.rs`),
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

A song's automation lanes can set a track's pan, filter cutoff and Q (`AutomationTarget::Pan`, `Cutoff`, `Q`,
keys `pan`, `cutoff`, `q`), as its volume: `AutomationTarget::to_control` and `from_control` convert a lane's 0 to 1
to the control's unit (the cutoff logarithmic, 20 Hz to 20 kHz). The engine sets them tick by tick over the track's
LFO where they change in a pass (`song_strips.rs`, `set_per_tick`); a cutoff lane switches on the filter of a track
whose filter is off. The mixer's pan knob and the track editor's pan, cutoff and Q knobs write or take back their
lanes as the fader does; any control held while recording writes on (only the grid's volume slider did). Without
lanes nothing changes: no snapshot changed.

Lanes also set a track's sends and effect amounts (`EchoSend`, `ReverbSend`, `ReverbMix`, `DelayMix`, `DelayFeedback`,
`Saturation`, `Bitcrush`, `HardClip`, `CompMix`), linear over the track field's range (`Track::value_range`). What each
target is moved to `ordrumbox-core/src/automation/target.rs`. The engine carries them tick by tick in
`StripLfo::controls` (`strip_control`, `set_strip_control`), which the strip approaches once a quantum as it does a
host's change: the render path is unchanged. Without lanes nothing changes: no snapshot changed.

Lanes also set a track's transpose (whole semitones), probability and swing amount (`Transpose`, `Probability`,
`Swing`). They act on each note of a song pass as it starts, at the lane's value on its step
(`ordrumbox-engine/src/pass/song_notes.rs`): a track with a probability lane is expanded with its probability at 1
(`lane_pattern`) and each hit is then drawn against the lane's (or the track's), from the pass's `Rng`; a swing lane on
a straight track swings pairs of steps. `note_frame` takes the swing in ticks (`swing_ticks`, with the note's own swing
in beats). The track editor's Generation and Loop sliders show what a lane sets, as the knob bar does. Without these
lanes nothing is drawn and nothing moves: no snapshot changed.

A level lane glides from a point to the next one on the next step only when they are part of a run of three
points or more on adjacent steps (a drawn line: `AutomationLane::value_at_position`); a level set on one step
alone, which leaves a point on that step and one on the next (where the lane goes back), now holds for its step
instead of gliding into the next value. A file's point with a fractional step is reported (`a.points[i].step`)
as well as rounded down. Painting a lane checks that the lane at its index is still the instrument and control
pressed (`Editor::paint_automation`), and a control held while recording finds its lane by them each time, so an
undo or an MCP edit during a drag no longer paints another lane. The lanes taken back are forgotten when another
song is selected. The automation renders keep their snapshots: no snapshot changed.

A song file (`.odbox`) is now called a project, in the UI, the code (`Library::project_record`, `ProjectRecord`,
`session_project_files.rs`, `Command::SaveProject`…) and the MCP tools (`saveProject`, `openProject`,
`exportProject`, `listSavedProjects`, `setProjectInfo`; `mcp.rs`, `RENAMED_TOOLS`, keeps the earlier names working).
A song is an arrangement, as before (`Song`, `library.songs`). The file format and the `songs/` folder are unchanged.
`listSavedProjects` lists them under `projects` (`songs` before). No snapshot changed.

A control taken back from its automation lane is marked, and each lane can be re-enabled on its own
(`session_automation_live.rs`): `mark_taken_back` puts a `Kind::Mark` (`theme::TAKEN_BACK`) over the mixer's, the
grid's and the track editor's controls whose lane is taken back, the grid's M S boxes get a corner mark
(`View::switches_taken_back`), and the song panel draws such a lane dim with an **OFF** badge
(`SongView::taken_back`, `SongHit::LaneBadge`) whose click, or the lane menu's **Re-enable lane**, calls
`reenable_lane`. The room for the badge is kept beside every lane's name, so a name stays put. No snapshot changed.

A pattern's tags are genres or features (`ordrumbox-core/src/tags.rs`, `FEATURE_TAGS`): `Pattern::genre` is its first
genre, `tags::by_genre` groups the patterns for the header's PATTERN list, now a menu of genres opening their patterns
(`session_pattern_tags.rs`, `Id::PatternMenu`). `compose::learn` learns a style from genre tags only, so the feature tags
no longer show as learned styles; `Editor::set_pattern_tags` keeps tags normalized (`tags::normalize`: lower case, no
repeats). The demo library's tags are cleaned: `default`, `song` and `sends` removed, `exp` renamed `experimental`, the
two `polyrhythm` patterns given `experimental` as genre. The song screen shows the tags on a line under the description
(`SongLayout::tags_line`, `SongHit::Tags`), always there, so the description keeps its place; its frame shows one line
less. No snapshot changed.

The help line follows the pointer (`session_hints.rs`: `Session::hint_hover`, `View::hint`): a text by `SongHit`, by the
grid's `Hit` and by panel control kind, else the view's keys as before. In the song panel the left button acts and the
right one opens the menus (`song_menu_click`): a click on an empty cell adds the row's pattern there, a click on a
clip selects it (`session_song_clip.rs`, known by its pattern and first measure; Delete removes it, Ctrl+D repeats it,
the menu's Next too), a click on a pattern of the list or a row's name shows that pattern. The song panel's tests were
changed to these clicks. No snapshot changed.

What has a menu shows it under the pointer: `SongPanel::hovered` (`SongView::hovered`) and the grid's hovered name
(`View::hovered_track`) draw the name lighter (`theme::highlight`, as a menu's line) with `panel::MENU_MARK` at its
end, whose room is kept so that a name never shortens under the pointer; a hovered clip is drawn lighter. A context
menu shows the key of its lines (`Dropdown::shortcuts`, `menu::shown_shortcuts`: Cmd on macOS, Alt in a plugin): the
grid's track and step menus, the clip's and the loop's. No snapshot changed.

A seventh colour scheme, High contrast (`theme::SCHEMES`, `colorScheme` 7): its ground and text only, the accents
being shared. `the_high_contrast_scheme_reads_everywhere` checks its WCAG ratios: 7:1 for text and labels on every
ground, 4.5:1 for what is greyed out, for a button's label on the line colour and for each accent on the window,
3:1 for borders. No snapshot changed.

Choke groups (`chokeGroup`, `Track::choke`, 1 to `MAX_CHOKE_GROUPS`): a pass's `Event` carries its track's group,
and `Engine::release_cut` (`engine/render.rs`) releases, as `mono` does for the track's own voices, the voices of
every track whose last note was in the same group (`Engine::choke_groups`, a fixed table: nothing allocated on the
audio thread). A track has no group by default, so no snapshot changed. A hi-hat added or renamed in the editor
(`Editor::add_track`, `change_track_name`) joins `HAT_CHOKE_GROUP`; `Track::new` gives none, so that the styles'
generation and MIDI import (and their snapshots) stay as they were.

The piano roll's drag moves a note (`note_drag.rs`, `NoteMove`; `session_roll_move.rs`): the steps and rows the pointer
crossed since the press, never onto another note, one undo step (`update_note` coalesced on beat, step and pitch); it
no longer edits the velocity, which the note editor and the grid's lane set. A click on the selected note selects it;
a second click on the same note within `DOUBLE_CLICK` deletes it (`RollState::last_note_click`, cleared by a click
that adds a note). The pointer shows it (`PointerShape::Move` on a note's body, `ResizeHorizontal` on its right
edge, `session_pointer.rs`), and keeps the shape while the note is held. No snapshot changed.

The first start (no `settings.json` in the user data folder: `session_resume::is_first_start`) opens in the simple
view, the grid alone (`Session::start_simple`, `Chrome::fold_editors`, as the plugin's compact view), with a message
saying how to show the rest; `resume` keeps the panels from then on. View > Simple view (`Command::SimpleView`) folds
both editors, or shows both when both are folded. No snapshot changed.

The kit is part of the library (`Library::kit`, the key `kit` of a library and of a project file, written when set):
`Editor::set_kit` is an undo step (`Change::Kit`). A kit chosen by the user (`Session::choose_kit`: the KIT list, the
kit keys, a random kit, `selectKit`, a kit folder imported) is played and kept; after an undo, a library or a project
loaded, `Session::follow_kit` plays the library's kit (once per library revision, as `refresh_synths`). A plugin
plays the kit of its host's parameter (`host_select_kit` keeps nothing). The project's name and description are undo
steps too (`set_library_info`, `Change::Info`; `setProjectInfo` one step for both). Deleting a synth preset, already
an undo step, no longer asks first. A file without `kit` keeps the kit in use: no snapshot changed.

A sample's loop has points of its own (`SampleShape::loop_start`, `loop_end`; `loopStart`, `loopEnd` in `kits.json`,
ms): the voice plays from the region's start (its end, reversed) into the loop, then wraps at the loop's edge ahead
(`SampleVoice`: the stop bounds are the loop's edge ahead and the region's edge behind, so the test per frame is the
same; `LoopSpan` wraps and crossfades). Without them the loop is the region, as before: no snapshot changed, the
voice's output bit-identical and its speed the same in release (5.1 ns a looping frame). The waveform's handles
(`session_wave_drag.rs`, `sample_overlay::Handle`) drag the start, the end, the loop's points and the end of the
decay; the points snap to the nearest zero crossing within 3 ms (`SampleBuffer::nearest_zero_crossing`). The kit
editor lays its nine knobs five a row (the track editor four), so that the waveform keeps its room.

Two kits rendered from synth presets, `808_like` and `909_like` (`assets/kits/`, `kits.json`), from the presets
`TR808_*` and `TR909_*` of `synth_presets.json`. Their names start with the machine so that auto-assign, which
matches a preset by its name (`assign::preset_match_key`), does not take them for the instrument a kit lacks: they
are heard as the kits' samples. Each was made with `ordrumbox-probe preset <names> --gate 1 --rate 44100 --velocity 1
--wav <folder>` (the new `--wav` option: mono, 16-bit PCM), the files renamed after their instrument, then
`ordrumbox-probe fix <folder> --write` (peak at −3 dB) and `fix <file> --length <end at −60 dB + 30 ms> --fade-out 20
--write`. A sound another kit lacks may now be borrowed from them: `assign_tests/data.rs` rewritten, as a meant change
(187 rows, the new kits' included).

Six more kits rendered the same way: `latin_perc`, `606_like`, `cr78_like`, `simmons_like`, `trap_808`, `minimal`
(presets `LAT_*`, `TR606_*`, `CR78_*`, `SDS_*`, `TRAP_*`, `MIN_*`; a prefix that names no instrument, as above). The
whistle is rendered with `--gate 0.35` (it sustains), the others with `--gate 1`. `assign_tests/data.rs` rewritten, as
a meant change (190 rows: the new kits' and what the others now borrow from them). The factory presets' checks of
`ordrumbox-probe/tests/synth_audio.rs` cover the new presets.

One more kit rendered the same way: `vintage60`, from the presets `SR601_*` (style `60s`), in the manner of the early
1960s rhythm boxes: ringing damped sines and soft, muffled noise. Drums rendered with `--gate 1`; `bass-c3` at C3 with
`--gate 0.5`, `lead-c4` and `pad-c4` at C4 with `--gate 0.6` and `--gate 1.2`, as for `8bits`. `SR601_BRUSH`, a second
snare, stays a preset only (a kit holds one sample per instrument). The `vintage` kit is removed. `assign_tests/data.rs`
rewritten, as a meant change (the `vintage60` rows, the `vintage` rows gone, and what the others now borrow from
`vintage60`).

The `8bits` kit is rendered again, from the chiptune presets: its drums and, new to it, melodic sounds (`BASS`,
`SYNTHLEAD`, `PIANO`, `BRASS`, `ORGAN`, `SYNTHPAD`, named after the note they
play, C3 or C4). `kick`, `snare`, `tom`, `chh`, `bass-c3`, `lead-c4` and `pulse-c4` are `CHIP_KICK`, `CHIP_SNARE`,
`CHIP_TOM`, `CHIP_HAT`, `CHIP_BASS`, `CHIP_LEAD` and `CHIP_PULSE`; the others are variants of them (pulse waves and
noise in long or short mode, no filter), not kept as presets. The melodic presets' release was lengthened (0.12 to
0.7 s) so they fade out. Rendered with `ordrumbox-probe preset … --rate 44100 --wav <folder>`, drums at C3 with the
default gate (`crash`: `--gate 0.3`), `bass-c3` at C3 with `--gate 0.5`, the C4 sounds with `--gate 0.6` (`pad-c4`: 1.2),
then `ordrumbox-probe fix <folder> --write`, with `--highpass 30` for the pulse sounds (a narrow pulse carries DC).
`assign_tests/data.rs` rewritten, as a meant change (the `8bits` rows).

The kit list is built from the folders of `assets/kits/`: each folder is a kit, each WAV file in it a sample
(`ordrumbox-engine/src/kit_folders.rs`, `kits_from_folders`). `kits.json`, the assets' and then the user's, only
describes them (instrument, root note, decay, texts, the user's edits) and orders them; a folder or a file it does not
list is added after the others, its instrument guessed from its file name. A kit or a sample it lists whose file is gone
is left out and reported. A kit of the user's `kits.json` takes the place of the shipped kit of its name, but the files
added to that kit's folder since still appear; a sample the user removes is kept in the kit's `removed` list so that it
is not added back. Before this change the user's `kits.json`, once written, replaced the shipped one whole: new
shipped kits and samples did not appear. A shipped sample removed before this change comes back once.

The `matt` kit has no crash any more (`CRASH.wav` removed): its `CRASH` tracks play the synth preset `CRASH_SYNTH`, as
its `COWBELL` tracks play `COWBELL_SYNTH`, and a track that only stands in for a crash borrows another kit's.
`assign_tests/data.rs` rewritten, as a meant change (the `matt` crash rows).
