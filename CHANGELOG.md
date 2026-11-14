# Changelog

## Unreleased

### New
- **Synth unison**: a *Unison* card on the synth editor's OSC tab makes each oscillator sound as 1 to 8 detuned
  layers (*Layers*, *detune* in cents, stereo *spread*), each starting at its own phase: a supersaw is one saw
  with 7 layers (the factory preset `SYNTHLEAD_SUPERSAW`). With a spread, the filter runs once per side. A preset
  without a `unison` group plays as before; MCP: `unison.count`, `unison.detune` and `unison.spread` in
  `createSynthPreset` and `updateSynthPreset`.
- **Sample playback**: a sample has *Start*, *End*, *Attack*, *Hold*, *Reverse*, *Loop* and *Gate* (the sound holds for the note's
  length, then the decay is its release),
  next to its decay, in the kit editor and on the track editor's Sound tab. The waveform
  shows them: the start and end lines, the direction, the loop and the level curve. They
  are saved with the kit (`start`, `end`, `attack`, `hold`, `reverse`, `loop`, `gate`; times in ms); a
  sample without them plays as before. A loop crossfades its two ends (10 ms at most) so
  that it does not click.
- **Per-track sidechain**: the FX tab has a *Sidechain* strip. Pick another track of the
  pattern as the *Key*: its sound turns this track's volume down (*Duck*, with a *Release*
  time) and, when the compressor is on, is what the compressor listens to. The key is the
  track's sound before its own effects, so a muted or turned-down key still keys. MCP:
  `sidechainSource`, `duckAmount` and `duckReleaseMs` in `updateTrack`.
- **Plugin**: a MIDI program change chooses the pattern (program 1–128 → pattern 1–128),
  in the CLAP plugin.

### Changed
- The release ships the CLAP plugin: `orDrumbox.clap` in the Windows zip, next to
  `ordrumbox.exe`, and a zip of the macOS bundle.
- The release has a Windows setup (`ordrumbox-<version>-windows-x64-setup.exe`): it
  installs the app with a Start menu shortcut and, as an option, the CLAP plugin in
  `Common Files\CLAP\orDrumbox\`. The zip stays, for use without setup.

## 0.1.1

### New
- **CLAP / VST3 plugin**: orDrumbox runs as an instrument in a DAW and follows the
  host's transport and tempo, with automatable parameters. It has 17 stereo outputs:
  the mix, plus one per track when its Output is set to *Own*.
- **Per-track compressor** (normal or parallel) in each track's effects.
- **Kit editor** (Ctrl+K): kit title and notes, sample table, gain, transpose and decay,
  analysis and envelope.
- **Chords**: add minor, major, 7th, suspended… chords from a cell.
- **Retrigger fade**: repeats step down in velocity (Fade slider, 0.6 by default).
- **Zoom and resizable window**; compact view.
- **Save status** in the menu bar (saved, modified, save failed…); Help > About shows
  the library's path.
- Instrument picker, toast messages, F1 lists every shortcut.

### Changed
- Auto-assign: a track missing from the kit now plays the synth preset of its name
  (e.g. `CLAP_SYNTH`) instead of borrowing another kit's sample.
- New colour scheme; track and note editors separated; reworked LFO and steppers.
- Shortcuts work on any keyboard layout (AZERTY included).

### Fixed
- MIDI import: sysex events no longer shift the notes after them; program changes in
  format 0 files are read; a second track of the same instrument is kept (`PERC 2`).
- The triangle LFO stays within its min–max range.
- Some bug fixes.

## 0.1.0

First release.
