# Changelog

## Unreleased

### New
- **Plugin**: a MIDI program change chooses the pattern (program 1–128 → pattern 1–128),
  in the CLAP plugin.

### Changed
- The release ships the CLAP plugin: `orDrumbox.clap` in the Windows zip, next to
  `ordrumbox.exe`, and a zip of the macOS bundle.

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
