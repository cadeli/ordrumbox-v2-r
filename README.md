# orDrumbox v2 (Rust)

Note: the legacy orDrumbox V1, the old Java drum machine, has been largely
improved upon and superseded; it is no longer current. This Rust port is the
active version.

The JavaScript/web version (<https://github.com/cadeli/ordrumbox-v2>) is a web demo of this Rust version: it is less complete than the downloadable (native) version.

## Download

- Download page: <https://www.ordrumbox.com/download/download.html>
- GitHub releases: <https://github.com/cadeli/ordrumbox-v2-r/releases>

The app is a single executable with its `assets/` folder next to it.

On Windows, a release holds a setup and a zip:

- `ordrumbox-<version>-windows-x64-setup.exe` installs orDrumbox in `Program Files`, with a
  Start menu shortcut, and the CLAP plugin in `C:\Program Files\Common Files\CLAP\orDrumbox\`
  (an option, on by default). Running the setup of a newer version updates it; your library is kept, and the new demos are
  added to it at the first start. Help > Factory reset... restores all the demos as shipped,
  updated ones included (your files are backed up first). Uninstalling
  (Windows settings, or the Start menu) removes what the setup installed, never your library
  and settings in the user data folder. The setup is not signed: Windows may warn about an
  unknown publisher (More info > Run anyway).
- `ordrumbox-<version>-windows-x64.zip` is the same app without setup: unzip it anywhere
  and start `ordrumbox.exe`.
- On Linux, `ordrumbox-<version>-linux-x64.zip` holds the same files as the Windows zip:
  unzip it anywhere and run `./ordrumbox`. The binary is built against the system's ALSA
  and X11 libraries; on a Debian-like distribution, `sudo apt install libasound2
  libxkbcommon-x11-0 libxkbcommon0` installs what it links at run time. The window has
  the orDrumbox icon on its own; for the launcher entry and the icon in the application
  menu, copy `ordrumbox.desktop` to `~/.local/share/applications/` and `ordrumbox.png`
  to `~/.local/share/icons/`, and put `Exec=` in the `.desktop` file on the full path of
  `ordrumbox`.
- On macOS, `ordrumbox-<version>-macos.zip` holds `orDrumbox.app`, with the icon: unzip it,
  move it to Applications. It is not signed: the first time, right-click > Open.

The CLAP plugin comes with each release:

- **Windows**: the setup installs it. Without the setup, `orDrumbox.clap` is in the app's
  zip, next to `ordrumbox.exe`. It reads the `assets/` folder next to it: copy both,
  `orDrumbox.clap` and `assets/`, into a folder of their own, such as
  `C:\Program Files\Common Files\CLAP\orDrumbox\` (or any folder your host scans for CLAP
  plugins).
- **Linux**: `ordrumbox-<version>-linux-clap.zip` holds `orDrumbox.clap`, to use as on
  Windows: put it with an `assets/` folder in a folder your host scans for CLAP plugins,
  such as `~/.clap/`.
- **macOS**: `ordrumbox-<version>-macos-clap.zip` holds `orDrumbox.clap`, its assets inside.
  Unzip it into `~/Library/Audio/Plug-Ins/CLAP/`. It is not notarized: if the host refuses
  to load it, run `xattr -dr com.apple.quarantine ~/Library/Audio/Plug-Ins/CLAP/orDrumbox.clap`.

The VST3 plugin is a separate download, `ordrumbox-<version>-<system>-vst3.zip` (GPLv3,
see "License"). It is a folder, `orDrumbox.vst3`, with its assets inside: unzip it into
`C:\Program Files\Common Files\VST3\` (Windows), `~/.vst3/` (Linux) or
`~/Library/Audio/Plug-Ins/VST3/` (macOS). On macOS it is not notarized either: run
`xattr -dr com.apple.quarantine` on it if the host refuses to load it.

## Documentation

- User manual: [`docs/user-manual.html`](docs/user-manual.html), with its
  screenshots in `docs/images/`.
- AI clients (MCP): [`docs/mcp-guide.html`](docs/mcp-guide.html).
- Building from source, tests: [`DEVELOPMENT.md`](DEVELOPMENT.md).

## The library

User data is saved under the OS user data folder
(`%APPDATA%\orDrumbox` on Windows, `~/Library/Application Support/orDrumbox` on
macOS, `~/.local/share/ordrumbox` elsewhere).

The library's name and whether its edits are saved show at the right of the
menu bar: *saved*, *modified* (saved a moment after the last edit), *new*
(a copy of the demo, written at the first edit), *save failed*, *not saved*
(no user data folder), or *in host project* (the plugin). Help > About gives
the file's path.

## Plugin

The plugin follows the host transport and saves its library in the host project.
Its window has no play button and no tempo knob (the host's transport plays, at the host's
tempo): in their place, **HOST** shows whether the host plays (a triangle) or is stopped (a
square), and the tempo played. Its menus leave out what the host or the app does: save,
quit, play, tempo, songs, the user data folder, updates, factory reset. A host tempo
outside 20–300 BPM plays at the nearest bound, the tempo shown, and the status line says so.
Playback starts on the host's first block, on its first frame. An offline render (a
bounce, or a host computing ahead of real time) plays as in real time, every note in its
place: the plugin then waits for its pattern, kit and passes before each block.
Its window opens compact, the grid alone (1072×388 at scale 1), to leave the host's
arrangement in view: **F2** shows or folds the track editor, **F7** the note editor, and the
window grows or shrinks with them when it showed the layout whole. The grip in its
bottom-right corner resizes the window (down to 320×200, where the layout scrolls, its
scrollbars stopping short of the grip); the project keeps the size.

Parameters the host automates: **Pattern** and **Kit** (by number, from 1), **Volume**
(master, -60 to +6 dB), **Swing** (50 % straight to 75 %: in each pair of steps, where the
second one starts, on every track with an even number of steps per beat, on top of the
track's own swing), and for tracks 1–16 **Mute**, **Volume** (a gain on the track's own,
-60 to +6 dB), **Pan** (added to the track's own), **Cutoff** (moves the filter's
cutoff by up to ±4 octaves), **Q** (added to the filter's), **Reverb**, **Delay**,
**Saturation** and **Compressor** (added to the effect's amount, kept within 0 to 1; with
the effect's button off, the track's amount counts as 0), **Threshold** (dB added to the
compressor's threshold, kept within -60 to 0 dBFS), **Feedback** (added to the delay's, up
to 0.95; a slap delay keeps its single echo), **Echo send** and **Reverb send** (added to the
track's sends to the shared echo and reverb, kept within 0 to 1) and **Transpose** (±24 semitones on the notes played,
MIDI input included, not on the MIDI notes sent out), and **Echo feedback** (added to the shared
echo's, up to 0.99; the plugin plays the echo and the reverb at their default settings). At their defaults they change
nothing. A filter of type `allpass`, and a reverb or delay of type `none`, stay off.
They change what plays, not the library.
In the grid, a track's row shows what the host does to it, right of its volume slider: a
stripe when the host moves any of the track's settings away from their defaults, and a square
at the top of it when the track plays on its own output. Hover the mark for what the host does to the track, setting by setting. A host that automates a track's **Volume** moves its slider in the grid, in the colour of a modulated value; the track's own volume, the one saved, is left as it was. A mute from the host lights the
track's **M** box, dimmed.

The plugin has 17 stereo outputs: **Mix**, and **Track 1** to **Track 16**. Every track plays
in the mix until its **Output** parameter is set to *Own*: track 3 then leaves the mix and
plays on **Track 3**, with its own volume, pan, filter and effects, for the host to mix and
process apart. The master's compressor, filters, limit and volume act on the mix only. An
output with no track on it is silent. A host that gives the plugin a single stereo output
keeps every track in the mix, whatever the parameter says; tracks past the 16th always are.

MIDI goes both ways, with one place per track of the pattern playing (or selected):
a drum track is on channel 10 at its instrument's GM key (KICK 36, SNARE 38…), whatever
the note's pitch. Any other track (melodic, unknown name, a second track of one drum) has a
channel of its own (1–9, 11–15, in track order), its notes on the key of their pitch: the
track's pitch 0 is its sample's root note, C3 (48) without one and for a synth. Past the 14
channels, the tracks left take keys of channel 10 from 84 up.

- **In**: a note on a track's channel plays that track at the note's pitch; a note on any
  other channel plays the drum track with that key. At its velocity, whether the host plays
  or not.
- **Pattern by note** (in, the app too): a note on channel 16 chooses a pattern: key 0 (C-2,
  or C-1 by the keyboard's naming) is pattern 1, key 1 pattern 2… up to 128. It applies at
  once: a pattern playing is left for the new one from its first beat (a plugin keeps to the
  host's place in the pass). No track is on channel 16, so the melodic tracks use channels
  1–9 and 11–15, and the tracks left past them take keys of channel 10. A pattern the library
  does not have is refused, and the status line says so.
- **Program change** (in): program 1–128, on any channel, chooses pattern 1–128, as the
  **Pattern** parameter does: shown at once, played once the pattern playing ends its loop.
  A program the library has no pattern for is refused, and the status line says so. The
  host's Pattern parameter keeps its value. CLAP only: a VST3 host gives a plugin no program
  change (map a MIDI control to the Pattern parameter instead).
- **Out**: the notes the pattern plays go out where they come in, with their pitch (the
  track's transpose and pitch LFO included), as long as the note (a step of its track unless
  its **Length** says otherwise). Tracks the host mutes send nothing.
  A clip recorded from this output and played back into the plugin plays the same tracks at
  the same pitches.

## The grid

One row per track, a page of beats across: 4 beats, or 8 or 16 when the window is wide
enough for steps of a usable size. The bar beside **+ NEW TRACK** scrolls
the beats: drag its thumb, or the wheel with Shift held. The two ends of the thumb zoom (drag
one: the other stays), as does Ctrl+wheel around the pointer; down to one beat across. Under the rows, the **lane** shows one setting of the
track editor's track, a bar per note: **VELOCITY**, **CHANCE** (its probability), **RETRIG**
(how many times it plays), **PAN**, **PITCH** (whole semitones, the middle is no change), **LENGTH** (how long it is held, in steps) or **SHIFT**
(how early or late it plays, off the grid), picked by the tabs on its left. Click a bar to set
its value at that height, or drag across the bars to draw them; a drag is one undo step. A bubble gives the
value as you draw and when the pointer rests on a bar. Right-click a bar for a menu: **Reset this step** or
**Reset all notes** puts the notes back to the setting's default (a lock is cleared). Drag the lane's top edge (the strip just above the bars) to set its height; **Automatic height** in the same menu gives back the default, which fills the room the rows leave.

The last three tabs are **parameter locks**: **FILTER** (the track's filter cutoff, 20 Hz to
20 kHz), **REVERB** and **DELAY** (the track's reverb and delay mix) while the note plays. A note
without a bar has no lock and leaves the track's own setting; a lock lasts until the track's next
note. Paint a bar down to the bottom of the lane to clear it. A lock turns on what the track has
off (a lowpass filter, a room reverb); a delay of type none stays off.

The **note editor** (the selected note's velocity, pitch, pan, trigger, retrigger,
euclidean, arpeggio and timing settings) is a strip under the grid and under the piano roll, the
views that edit notes. The track editor, on the right, holds the track's settings only.

A note's **Length** (a knob beside Velocity, Pitch and Pan, 1 step by default, a quarter of a step at least) is how
long a synth note is held before its release, and how long the MIDI note sent or exported
lasts; a sample plays to its own end whatever the length. In the piano roll a note is as wide
as it is long: drag its right edge to draw it out. The bar under the piano roll scrolls and
zooms its beats like the grid's. Its **Shift** moves it off the grid, up
to half a step early or late, with its retriggers.

## Keys

Press **F1** in the app for the full keyboard shortcut list. Keys are the ones the
keyboard's layout marks (Ctrl Z is the key marked Z on AZERTY too), except the two rows
that play: 1–9 (mute, Shift: solo) and Q–P (hear a track) are read by their place. Common keys:

- **PATTERN** / **SONG** (the mode switch of the header): what **Space** plays, the
  pattern in a loop or the song. Only the switch changes it: the views (grid, piano roll,
  song panel, synth, mixer, and their keys) leave it as it is. In song mode the grid and
  the piano roll show the pattern the song is playing, with its playhead. Back from the
  song to the pattern, the pattern keeps the song's tempo.
- **The PATTERN list** of the header opens on the genres, each with how many patterns it
  has; a genre opens its patterns. A pattern's genre is its first tag that is not a feature
  (bass, chords, lead, pad, synth, organ, strings, brass, piano, clavinet, arpeggio,
  polyrhythm, polymeter). Its tags are edited on the song screen, under its description (a
  click), or with Pattern > Tags...: `funk, bass, chords`, the genre first. Genres teach the
  learned styles; features do not.
- **Space**: play / stop. A song stops where it is and **Space** goes on from that
  measure; **Home** (or the keypad's **Enter**) goes back to the first beat, or measure.
  A **drag** on the song panel's ruler selects the part of the song that plays in a loop:
  whole measures, beats or steps, as fine as the ruler is marked at that zoom. The ruler
  tints it green, with a mark at each end; drag a mark to move that end (on the same
  lines). Another drag selects a new loop. **Ctrl + L** (Play > Loop the song) turns the
  loop off and on again: off, it stays on the ruler in grey, its ends still movable. A
  **click** on the ruler only puts the cursor on the measure clicked. A **right-click** on
  the ruler opens the loop's menu: turn it on or off, or remove it (as Play > Clear the
  loop). **Shift + click** on the ruler loops the song from the cursor's measure to
  the one clicked.
  The rulers of the grid and of the piano roll work the same way for the pattern shown: a
  **drag** selects the steps that loop (the steps of the track whose cursor is in the grid,
  or of the roll's track), a **click** puts the play cursor on a step, which the pattern goes
  on from at once when it plays, and from which it plays next time (**Home** brings it back to
  the first beat), **Ctrl + L** and the **right-click** menu turn the loop off, on, or remove it.
  A **double-click** on a clip of the song panel opens its pattern in the grid, in
  pattern mode.
- **Song panel clicks**: the left button acts, a **right-click** opens the menu of what is
  under it. A click on an empty cell adds the row's pattern there; on a clip, selects it
  (**Delete** removes it, **Ctrl + D** repeats it right after itself); on a pattern of the
  list or a row's name, shows that pattern. The help line at the bottom of the window says
  what the mouse does on what is under the pointer, in every view. A name that has a menu
  is drawn lighter under the pointer, with a **v** at its end, and a menu line that has a
  key shows it at its right end.
- **Automation lanes** in the song panel: **+ NEW AUTOMATION**, under the arrangement's
  rows, lists the instruments the song's patterns play; pick one, then the control: **Mute**,
  **Solo**, **Volume**, **Pan**, **Cutoff**, **Q**, **Echo send**, **Reverb send**, **Reverb mix**,
  **Delay mix**, **Delay feedback**, **Saturation**, **Bitcrush**, **Hard clip**, **Comp mix**,
  **Transpose**, **Probability** or **Swing**, and a lane for it appears under the patterns. The panel opens zoomed in
  far enough to show each step (a sixteenth note). Press a step and **drag** along the
  lane to select steps. On a Mute or Solo lane, from a step that is not on, the selection
  is switched on; from a step that is on, it is switched off. On a Volume lane, the height
  you press at is the level, as on a fader: the top is 0 dB, the middle about −15 dB, the
  bottom silent. On a Pan lane the top is right, the middle the center, the bottom left; on a
  Cutoff lane the top is 20 kHz and the bottom 20 Hz, the middle about 630 Hz; on a Q lane the
  bottom is 0.71 and the top 18.7. On the other lanes the bottom is 0 and the top the knob's
  most (100 %, 95 % for the delay's feedback); a Transpose lane goes from −24 to +24
  semitones, in whole semitones. A drag draws a line from it to the pointer's height, which
  plays as a smooth glide; a level set on one step alone (a click) holds for its step. While you draw, and when the mouse is over a lane, a tooltip gives the
  value under it (`KICK Volume -4.5 dB at 2.3`). A press on the line of a level already
  drawn grabs its segment (from its change to the next): drag up or down to move it as one.
  A lane can be one to four rows tall: drag its bottom edge in the names column, or use
  **Taller** / **Shorter** in its menu (saved with the song). With the **right button**, the selection is erased, back to
  what the patterns say. The selection is made of whole measures, beats or steps, as fine
  as the ruler is marked at that zoom; hold **Shift** to select steps at any zoom. Only
  the steps selected change. Where a lane sets nothing, the instrument plays as its
  patterns say. A solo that is on silences every other instrument of the song; any other lane
  replaces the track's value (and its LFO) where it is set. A cutoff lane filters a track
  whose filter is off, as a lowpass; an effect lane plays its effect even with its light off,
  but a reverb or a delay of type `none` stays off, and a slap delay stays one echo.
  Transpose, Probability and Swing act on each note as it starts, at the value of its step:
  a transposed note keeps its pitch to its end (the track's transpose LFO still moves it), a
  probability lane decides each note on its own (a repeat too), and a swing lane swings a
  track whose swing grid is straight in pairs of steps. A click, left or
  right, on a lane's name opens its menu: clear all its points, delete the lane, or
  delete every lane. Lanes are saved with the song.
- **Automation and the mixer** while a song plays: the mixer shows what the lanes set (a
  fader moves with its Volume lane, a Pan or a send knob with its lane, M and S light with
  their lanes), and so do the grid's volume sliders and M S boxes and the track editor's
  knobs that have a lane (Volume, Pan, Cutoff, Q, the effects' amounts). Moving a control that
  has a lane takes it back: the lane is no longer played for that instrument, and the
  control keeps the value you gave it. It shows: the control gets an orange mark in its
  corner (the mixer's, the grid's, the track editor's), and the lane is drawn dim with an
  orange **OFF** badge beside its name. A click on the badge (or **Re-enable lane** in the
  lane's menu) plays that lane again; **Play > Re-enable automation** plays them all. With **REC**
  (beside PLAY in song mode) or **Play > Record automation** on, moving a mixer control while the song plays writes its lane
  instead (the lane is made if there is none; the track itself is left as it is): a fader
  or a knob that can have a lane writes its level on every step
  under the playhead for as long as you hold it; M or S
  writes its new state from the playhead's step to the lane's next change. Each gesture is
  one undo step.
- **Song panel zoom**, as on a DAW timeline: drag an end of the thumb of the
  arrangement's horizontal scroll bar (its light handles) to show more or fewer measures,
  the other end staying where it is; drag its middle to scroll. **Ctrl + wheel** over the
  arrangement zooms around the measure under the pointer.
- **The pointer** tells what a drag does: a hand over the thumb of any scroll bar (a
  drag scrolls; it closes while held), sideways arrows over the song panel's zoom
  handles. The plugin on macOS keeps the arrow.
- **F9**: metronome (a click on every beat, the first of a measure higher), **F8**: tap
  the tempo (tap on the beat; a pause of 2 seconds starts the count again)
- **Ctrl R**: record. While the pattern plays, **Q–P** also put a note on their track, on
  the step nearest to the playhead (one undo step each; a step that has a note keeps it).
  With **Play > Record off the grid** on, a recorded note also keeps how early or late it
  was played, as its Shift
- **F11**: full screen, **Ctrl W**: close the window (as **Ctrl Q**)
- **F1–F7** (also **Ctrl 1–7**, for the keyboards whose F keys want Fn): the shortcut list,
  track editor, song panel, piano roll, grid, synth editor, note editor (shown or folded)
- **View > Simple view**: the grid alone, both editors folded (again: both shown). The first
  start opens in it; later starts open as the window was left.
- **F12**: the mixer (View > Mixer, the **MIX** button): a strip a track with its echo and
  reverb sends, pan, mute, solo and fader, then the shared echo and reverb, then the master
  bus. **Left**/**Right** go from track to track; when the strips do not all fit, a scroll
  bar under them scrolls them (the wheel on a strip too). **Esc** goes back to the grid
- **Ctrl K**: kit editor, **Ctrl Shift M**: export MIDI (**Ctrl E**: export WAV)
- **File > Export pattern as WAV** (**Ctrl E**) and **as MIDI** (**Ctrl Shift M**) first ask how
  many loops of the pattern the file holds (1 to 64, **Left**/**Right**); the count is kept for
  the next export, of either
- **File > Export pattern stems / Export song stems**: one WAV per track in a folder of its own,
  taken before the master bus (as the plugin's track outputs); tracks with no sound are left out
- **Ctrl -** / **Ctrl =**: zoom out / in a quarter at a time (×1, ×1.25, ×1.5… up to ×4,
  the keypad's keys too), **Ctrl 0**: the display's scale, **View > Zoom: fit to
  window**: the biggest of these scales the window shows whole. The zoom does not depend
  on the window's size: a window too small for it gets scrollbars (drag them,
  click beside the thumb, or turn the wheel where nothing else takes it). A
  window goes down to 320×200, the app's and the plugin's. The
  zoom is kept in `settings.json` (`uiZoom`, scale units added to the
  display's, such as `0.5`; whole numbers from earlier versions still read). A
  display at 150 % shows at ×1.5. In the plugin, Alt stands for Ctrl (the host
  keeps Ctrl).
- **Shift + C** (View > Next colour scheme): the next of seven colour schemes, the last one
  **High contrast** (white on black, every text at least 7:1 against its ground), for low
  vision or a bright room. Kept in `settings.json` (`colorScheme`, 7 for high contrast).
- **Where you left off**: the app starts on the pattern and the arrangement you had
  chosen, in the mode and the view you had open (grid, piano roll, song panel, synth,
  mixer), with the track editor, the note editor, the VU meters and the kit editor shown
  or hidden as you left them. It is kept in `settings.json` (`resume`) when you quit. The
  plugin starts as the host's project left it.
- **M** / **S**: mute / solo the track under the cursor. The letters that change
  every track want **Shift** (B: the pattern again in its style, C: the next colour
  scheme, D: drums in auto, G, H, J, K, V: the VU meters, X: random notes), so a
  slip off the play rows does nothing; the pattern's length has no key (the
  Song panel and the menus change it).
- **Ctrl A**: select the whole pattern as a zone. **Shift + arrows** grow a zone from the
  cursor, **Ctrl + click** on a step takes it into the zone (it stays a rectangle); copy,
  cut and delete then act on it.
- **Edit > Quantize...** moves notes towards the lines of a grid (1/4, 1/8, 1/16 or 1/32 of a
  note) by a strength of 25, 50, 75 or 100 %. It takes the zone selected, else the track under
  the cursor, else the whole pattern. At 100 % on the track's own grid it takes the shift off
  notes recorded with *Record off the grid*; a coarser grid also moves their steps (a step
  halfway between two lines goes to the later one); a lower strength keeps what is left as a
  shift. A note pushed past the pattern's end or start wraps round it. One undo step.
- **Edit > Humanize...** plays the same notes (the zone, else the track under the cursor, else
  the whole pattern) a little early or late, and a little softer or louder, by a random amount:
  **Timing** up to ±5, 10, 20 or 30 % of a step (a note stays within half a step of its own),
  **Velocity** up to ±5, 10, 20 or 30 % of the note's (a ghost note stays a ghost); 0 leaves it as it is. Each run
  draws again, on top of the last; one undo step each.
- **Alt + arrows** on a selected note in the grid: up / down a semitone of pitch, left /
  right one step in time (in the piano roll, **Shift + arrows**, and its drag follows the
  same axes: up / down the pitch, sideways the time).
- **Edit > Preferences...** (**Ctrl ,**) gathers the sound output, the MIDI ports and clock,
  the metronome, the VU meters and the toasts in one window. Up / Down choose a line, Enter or
  Left / Right change it, **OK** applies and **Esc** leaves everything as it was. The sound
  output and the MIDI input are kept in `settings.json` (`audioDevice`, `midiInput`: empty
  is the system's device or the first port, `"off"` no MIDI). A new output starts the sound
  again; a device gone gives way to the system's, and the status line names the one in use.
  The MIDI output (`midiOutput`: a port's name, none by default), the clock it sends
  (`midiClockOut`, `true` or `false`) and the clock followed (`midiClockIn`) are kept there too.
- **MIDI keyboard or pad** (the app; a plugin takes its host's): the notes it plays sound
  the pattern's tracks, drums by their General MIDI key (kick 36, snare 38…), a melodic
  track on its own channel at the note's pitch. With **Ctrl R** on and the pattern playing,
  each note is also written to its track, on the step nearest to the playhead, at the
  velocity played (one undo step each).
- **MIDI output** (the app): the notes the pattern plays go out of the port chosen in the
  Preferences, as a plugin sends them: drums on channel 10 at their GM key, any other track
  on a channel of its own at its pitch, as long as the note. They leave when the block that
  plays them is rendered, so a sound device with a long buffer plays a few milliseconds after
  them.
- **MIDI clock out** (the app, with a MIDI output): the app sends 24 pulses a beat while it
  plays, **Start** from the first beat, and **Song position** then **Continue** when a song
  plays from a later measure, and **Stop**: a drum machine or a synth set to an external clock
  plays along. The clock follows the tempo, the pattern's length and the swing.
- **MIDI clock in** (the app, with a MIDI input): with *MIDI clock in: follow*, the machine
  sending to the input is the master. Its **Start**, **Continue**, **Stop** and **Song position**
  start, stop and place the transport; its pulses set the tempo, to a tenth of a beat a
  minute, and keep the playing in step with it. Nothing plays until it starts, so Space does
  nothing while following: choose *ignore* to play on its own again. A master that stops
  sending pulses for a second stops the transport.
- **Edit > MIDI controls...** binds the knobs of a MIDI controller (the app; a plugin has its
  host's parameters). Choose a control with Left / Right (the master volume, the shared echo's
  **feedback** (from the one set up to 0.99: a knob to make the echo build up), then each track
  of the pattern shown: **volume**, **pan**, **cutoff**, **mute**, **echo send**, **reverb send**
  (from the one set up to full: a knob to throw a sound into the echo); the first 16 tracks),
  go to **Learn**, press Enter and turn the knob: its control change (number and channel) is
  bound to it. One knob has one control and a control one knob; **Clear** lets it go. The
  bindings are kept in `settings.json` (`midiControls`). A knob moves its control on top of
  what the track has, as a plugin host's mix does, so nothing is saved with the library and
  no undo step is made: volumes go from silence (knob down) to the one set; pan and cutoff
  go from the one set (knob in the middle: no change) up to the end of the knob, the cutoff
  by four octaves; a mute is on from the middle of the knob up, and shows on the track's mute
  box. A knob takes its control to its place as soon as it is turned, wherever the control was.
- **On macOS** the menus show **Cmd** in place of Ctrl (the same key here); redo is
  **Cmd Shift Z** (also Cmd Y), and export MIDI is **Cmd Shift M**: Cmd M alone
  minimizes the window.

## Files

The app writes `library.json`, `kits.json`, `settings.json`, `songs/`, `samples/` and
`exports/` in the user data folder. A **project** is the library saved under a name, as an
`.odbox` file in `songs/` (File > Save project, Open project...); a song is one of its
arrangements. Pattern and track settings use camelCase keys; old (JS)
spellings are still read.

The app holds `ordrumbox.lock` there while it runs. Started a second time, it warns that
another orDrumbox is running and lets you quit or continue anyway (two windows can overwrite
each other's library). The plugin and `ordrumbox --mcp` are not concerned.

The synth presets edited (added, changed from the assets' ones, or deleted: `null`) are part
of the library, under `synthPresets` in `library.json`, in File > Export library, and in a
plugin's host project: each library, each project, has its own, and each change is an undo
step (a knob turned is one). A preset of the assets it does not name plays as the assets hold
it. A library saved before has none of its own: it takes those of `synth_presets.json` (or
`generated_sounds.json`), which earlier versions wrote and which is now only read. File >
Import library adds its presets, replacing those of the same name; a song loaded keeps them.

A synth preset has two tags: its **instrument** (an instrument name: BASS, SYNTHLEAD, KICK...)
and its **style** (dub, chiptune, acid...). The lists that choose a preset (the synth
editor's, the track editor's synth screen) open as the instruments, then the styles, each
opening its presets; the arrows beside them still step through every preset. The synth
editor's **Tags** button edits them as `BASS, dub` (the instrument first); a preset without
an instrument is listed under `other`. A preset edited before the presets had tags takes
those of the preset of the same name in `synth_presets.json`.

File > Export kit with its samples makes a folder named after the kit, holding `kit.json`
(the kit's mapping, each sample's path the name of its file there) and the samples' WAV
files. Kit > Import folder as kit (or the folder dropped on the window) reads it back: the
samples are copied to `samples/imported/<kit>/`, the kit replaces the kit of its name. A
folder without `kit.json` is imported as before, each WAV's instrument guessed from its name.

File > Back up user data copies all of it (the library with its synth presets, `kits.json`,
`settings.json`, `samples/`, `songs/`) to a folder `orDrumbox-backup-<date>-<time>` (UTC);
File > Restore user data puts such a folder back, the files there moved to `backups/` first.

## License

MIT: see `LICENSE`. The VST3 plugin links GPLv3 VST3 bindings, so that
`orDrumbox.vst3` is GPLv3 as a whole. The CLAP build stays
MIT.