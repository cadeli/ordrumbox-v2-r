# Changelog

## Unreleased

### Changed
- A song file is now a **project**: File > Open project (Ctrl O), Save project, Save project as,
  Export project, Import project, Rename project and Project description, all in the File menu.
  A song is an arrangement, as before. The files stay `.odbox` in `songs/`: nothing to convert.
- A control taken back from its automation lane shows it (an orange mark), and its lane too (dim, with an
  orange OFF badge); a click on the badge, or Re-enable lane in the lane's menu, plays that lane alone again.
- MCP: `saveProject`, `openProject`, `exportProject`, `listSavedProjects` (its list under `projects`)
  and `setProjectInfo`; the earlier `*Song*` names still work.

## 0.1.2

### New
- **Synth**: unison, modulation matrix, chip controls (pulse width, noise modes) and new factory presets.
- **Samples**: start, end, attack, hold, reverse, loop and gate.
- **Per-track sidechain**, and **parameter locks** (filter, reverb, delay) per note.
- **Quantize** (Edit > Quantize...) and **zone selection** in the grid.
- **MIDI**: input and output, MIDI learn, clock in and out, pattern choice by program change or note.
- **Stems export** of a pattern or a song (File menu, MCP).
- **Preferences** window (Ctrl ,), metronome, tap tempo.
- About twenty new demo patterns and a full demo song.
- Windows setup, CLAP plugin in the release, Linux build.
- VST3 plugin in the release, a separate GPLv3 zip for Windows, Linux and macOS.

### Changed
- New keyboard shortcuts (F1 lists them); PATTERN / SONG mode switch.
- MIDI import and export keep note lengths.
- The filter's highpass is now textbook: a little thinner and quieter.

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
