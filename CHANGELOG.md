# Changelog

## Unreleased

### New
- **File > Demos...**: the demos by style, as tiles; a song or a pattern is copied into your library and plays.
- Demo songs: Minimal House, Industrial Techno, Psytrance, Dubstep, Detroit Deep House, Amapiano, UK Garage,
  Lo-fi Hip-Hop, Blues Shuffle, Afrobeat, Electroclash, Electro-Punk.

### Changed
- A new library starts with the **starter library** (a funk song and a few beats), not every demo;
  File > Load the demo library... is now File > Load the starter library...
- The demo arrangement is named **Funk Song**.

## 0.1.3

### New
- **Mixer** (F12): a strip per track with echo and reverb sends, a tape echo, a spring reverb and the master bus.
- **Song automation**: lanes for mute, solo, volume, pan, filter, sends, effects and more, drawn or recorded.
- **Song loop** on the song panel's ruler (Ctrl+L), and a timeline that zooms.
- **New kits** rendered by the synth: 808, 909, 606, CR-78, Simmons, latin, trap, minimal, vintage 60s, punch.
- **Sample editing** on the waveform: start, end, decay and an inner loop.
- **Choke groups**: a closed hi-hat cuts an open one.
- **Piano roll zones** (copy, cut, paste, move) and **Edit > Humanize...**.
- **Simple view**, and the app starts where you left off.

### Changed
- A song file is now a **project** (File > Open project...); the `.odbox` files stay the same.
- Scroll bars with zoom replace the pages of the grid and the piano roll.
- Patterns are listed by genre; tags are edited on the song screen.
- Synth: the **rnd** wave of the oscillators reads the track's sample instead of random values.
- New pattern (Ctrl+N) has a track for each instrument of the kit; the kit chosen is kept.
- Kits `real`, `open`, `ropen`, `punchy` and `vintage` removed.
- A seventh colour scheme, **High contrast** (Shift C).

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
