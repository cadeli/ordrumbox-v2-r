#!/usr/bin/env node
// Golden fixtures for the piano roll, produced by the JS app's own code:
//
// - getNoteSubPositions() (patterns/note_positions.js): where a note's
//   retriggers, arpeggio steps and euclidean fills fall, as the piano roll
//   draws their ghosts and lights them while playing — on every track of
//   assets/data/song.json (loaded with fixPatterns) and on hand-made notes;
// - buildSequenceNotes() / getSequence() (logic/composition.js): the chord
//   progressions of the keyboard menu's "Add sequence";
// - NoteDrag (ui/components/note_drag.js): what dragging a note does
//   (velocity up and down, pitch sideways, the axis lock), driven through a
//   fake window;
// - midiName() (ui/piano_roll/piano_roll_constants.js).
//
//   node tools/export_piano_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-core/tests/fixtures/sub_positions.json,
// crates/ordrumbox-logic/tests/fixtures/composition.json and
// crates/ordrumbox-app/tests/fixtures/piano_roll.json.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_piano_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

// NoteDrag listens on window.
const windowListeners = {}
globalThis.window = {
    addEventListener: (type, fn) => ((windowListeners[type] ??= []).push(fn)),
    removeEventListener: (type, fn) => {
        windowListeners[type] = (windowListeners[type] ?? []).filter((f) => f !== fn)
    },
}
const fire = (type, event) => [...(windowListeners[type] ?? [])].forEach((fn) => fn(event))

const here = dirname(fileURLToPath(import.meta.url))
const crates = join(here, '..', 'crates')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href

const { fixPatterns } = await import(srcUrl('patterns/fixer.js'))
const { getNoteSubPositions } = await import(srcUrl('patterns/note_positions.js'))
const { SEQUENCES, getSequence, buildSequenceNotes } = await import(srcUrl('logic/composition.js'))
const { default: NoteDrag } = await import(srcUrl('ui/components/note_drag.js'))
const { midiName } = await import(srcUrl('ui/piano_roll/piano_roll_constants.js'))

async function write(dir, name, value) {
    const out = join(crates, dir, 'tests', 'fixtures')
    await mkdir(out, { recursive: true })
    await writeFile(join(out, name), JSON.stringify(value) + '\n')
}

// ─── Sub-positions ───────────────────────────────────────────────────────────

/** One track's notes and their sub-positions (a random arp's pitches are left out: they are drawn). */
function positionsOf(track) {
    const totalSteps = (track.stepsPerBeat ?? 4) * (track.beatCount ?? 4)
    return track.notes.map((note) => {
        const random = JSON.stringify(note.arp ?? '').toLowerCase().includes('random')
        return getNoteSubPositions(note, track, totalSteps).map(({ pos, type, pitchOffset }) => ({
            pos,
            type,
            pitchOffset: random ? null : pitchOffset,
        }))
    })
}

const lib = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'song.json'), 'utf8'))
const patterns = fixPatterns(structuredClone(lib.patterns))
const libraryCases = []
patterns.forEach((pattern, p) =>
    pattern.tracks.forEach((track, t) => {
        if (track.notes.length > 0) libraryCases.push({ p, t, positions: positionsOf(track) })
    }),
)

const note = (o) => ({ beat: 0, beatStep: 0, ...o })
const handTracks = [
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ retriggerCount: 4, rate: 2 }), note({ beat: 2, rate: 10, retriggerCount: 3 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ euclideanFill: 3 }), note({ beat: 2, euclideanFill: 5, euclideanRotation: 2 })] },
    { stepsPerBeat: 4, beatCount: 8, loopAtStep: 10, notes: [note({ euclideanFill: 4, euclideanRotation: 1 })] },
    { stepsPerBeat: 3, beatCount: 4, notes: [note({ arp: [0, 4, 7], retriggerCount: 6, rate: 4 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ arp: 'updown', retriggerCount: 9, rate: 2, euclideanFill: 2 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ arp: { intervals: [3, 7, 10], mode: 'down' }, retriggerCount: 5, rate: 3 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ arp: '0,5,9', retriggerCount: 40, rate: 1 })] },
    { stepsPerBeat: 2, beatCount: 3, notes: [note({ beat: 1, beatStep: 1, retriggerCount: 3, rate: 8, euclideanFill: 7 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ arp: 'random', retriggerCount: 4, rate: 2 })] },
    { stepsPerBeat: 4, beatCount: 4, notes: [note({ arp: 'nonsense', retriggerCount: 3, rate: 2 })] },
]
const handCases = handTracks.map((track) => ({ track, positions: positionsOf(track) }))
await write('ordrumbox-core', 'sub_positions.json', { library: libraryCases, hand: handCases })

// ─── Chord progressions ─────────────────────────────────────────────────────

const sequences = []
for (let index = -2; index < SEQUENCES.length + 2; index++) {
    const sequence = getSequence(index)
    for (const tonic of [0, -5, 7]) {
        for (const beatCount of [0, 1, 3, 4, 7, 8, 16]) {
            sequences.push({
                index,
                id: sequence.id,
                name: sequence.name,
                tonic,
                beatCount,
                notes: buildSequenceNotes(sequence, tonic, beatCount),
            })
        }
    }
}
await write('ordrumbox-logic', 'composition.json', { sequences })

// ─── Note drag ──────────────────────────────────────────────────────────────

/** A press at (0, 0) on a note, the moves (dx, dy in px, y down), the release. */
function drag(start, trackPitch, moves) {
    const note = { ...start }
    const calls = []
    const nd = new NoteDrag({
        onStart: (_, axis, value) => calls.push({ start: axis, value }),
        onValue: (n, axis, value, dir) => {
            calls.push({ axis, value, dir })
            n[axis] = value // the surface writes the value (applyNoteEdit)
        },
    })
    nd.begin({ clientX: 0, clientY: 0 }, note, trackPitch)
    for (const [x, y] of moves) fire('mousemove', { clientX: x, clientY: y })
    // The axis claimed, read before the release resets it. (The JS resets on
    // the window's mouseup, which comes before the click, so consumeClick()
    // never reports a drag: the port goes by the axis instead.)
    const axis = nd.axis
    fire('mouseup', {})
    return { start, trackPitch, moves, calls, axis }
}

const drags = [
    drag({ velocity: 0.8, pitch: 0 }, 0, [[1, -1], [2, -2], [0, -3], [0, -10], [0, -100], [5, 40], [0, 300]]),
    drag({ velocity: 0.5, pitch: 3 }, 0, [[3, 1], [11, 0], [12, -2], [30, 9], [-50, 0], [-900, 0], [900, 0]]),
    drag({ velocity: 0.8, pitch: 0 }, 40, [[4, 4], [200, 0]]),
    drag({ pitch: 2.6 }, 0, [[-3, 3], [-13, 0]]),
    drag({ velocity: 0.333 }, 0, [[2, 3], [2, 5], [2, 4]]),
    drag({ velocity: 0.8 }, 0, [[1, 1], [2, -2]]),
    drag({ velocity: 0.8 }, 0, []),
]

const midiNames = []
for (let midi = 0; midi <= 127; midi++) midiNames.push(midiName(midi))

await write('ordrumbox-app', 'piano_roll.json', { drags, midiNames })
process.stdout.write(
    `piano roll: ${libraryCases.length} library tracks, ${handCases.length} hand tracks, ` +
        `${sequences.length} sequences, ${drags.length} drags\n`,
)
