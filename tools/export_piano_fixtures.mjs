#!/usr/bin/env node
// Golden fixtures for the piano roll, produced by the JS app's own code:
//
// - NoteDrag (ui/components/note_drag.js): what dragging a note does
//   (velocity up and down, pitch sideways, the axis lock), driven through a
//   fake window;
// - midiName() (ui/piano_roll/piano_roll_constants.js).
//
//   node tools/export_piano_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-ui/tests/fixtures/piano_roll.json.

import { writeFile, mkdir } from 'node:fs/promises'
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

const { default: NoteDrag } = await import(srcUrl('ui/components/note_drag.js'))
const { midiName } = await import(srcUrl('ui/piano_roll/piano_roll_constants.js'))

async function write(dir, name, value) {
    const out = join(crates, dir, 'tests', 'fixtures')
    await mkdir(out, { recursive: true })
    await writeFile(join(out, name), JSON.stringify(value) + '\n')
}

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

await write('ordrumbox-ui', 'piano_roll.json', { drags, midiNames })
process.stdout.write(`piano roll: ${drags.length} drags\n`)
