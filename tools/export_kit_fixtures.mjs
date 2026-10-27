#!/usr/bin/env node
// Golden fixtures for the kit editor's sample analysis, produced by the JS
// app's own Analyzer (audio/analyzer.js) and hzToNote (core/hz_to_note.js).
//
//   node tools/export_kit_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-engine/tests/fixtures/analysis.json: WAV files
// (synthetic tones, noise, decays, silence, very short ones, 8/16/24/32-bit
// PCM and 32-bit float, mono and stereo, and the kick.wav fixture) with what
// Analyzer.analyzeWavBuffer() reports and the note of its fundamental.

import { readFile, writeFile } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_kit_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const root = join(here, '..')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href

const { default: Analyzer } = await import(srcUrl('audio/analyzer.js'))
const { hzToNote, formatNote } = await import(srcUrl('core/hz_to_note.js'))

const mulberry32 = (seed) => () => {
    seed = (seed + 0x6d2b79f5) >>> 0
    let t = seed
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}

/** A WAV file of `channels` (arrays of -1..1) as `format` 1 (PCM) or 3 (float), `bits` deep. */
function wav(channels, rate, format, bits) {
    const frames = channels[0].length
    const bytesPer = bits / 8
    const data = new DataView(new ArrayBuffer(frames * channels.length * bytesPer))
    let p = 0
    for (let i = 0; i < frames; i++) {
        for (const ch of channels) {
            const s = Math.max(-1, Math.min(1, ch[i]))
            if (format === 3) data.setFloat32(p, s, true)
            else if (bits === 8) data.setUint8(p, Math.round(s * 127) + 128)
            else if (bits === 16) data.setInt16(p, Math.round(s * 32767), true)
            else if (bits === 24) {
                const v = Math.round(s * 8388607)
                data.setUint8(p, v & 0xff)
                data.setUint8(p + 1, (v >> 8) & 0xff)
                data.setInt8(p + 2, v >> 16)
            } else data.setInt32(p, Math.round(s * 2147483647), true)
            p += bytesPer
        }
    }
    const header = new DataView(new ArrayBuffer(44))
    const text = (o, s) => [...s].forEach((c, i) => header.setUint8(o + i, c.charCodeAt(0)))
    text(0, 'RIFF')
    header.setUint32(4, 36 + data.byteLength, true)
    text(8, 'WAVE')
    text(12, 'fmt ')
    header.setUint32(16, 16, true)
    header.setUint16(20, format, true)
    header.setUint16(22, channels.length, true)
    header.setUint32(24, rate, true)
    header.setUint32(28, rate * channels.length * bytesPer, true)
    header.setUint16(32, channels.length * bytesPer, true)
    header.setUint16(34, bits, true)
    text(36, 'data')
    header.setUint32(40, data.byteLength, true)
    return [...new Uint8Array(header.buffer), ...new Uint8Array(data.buffer)]
}

const tone = (hz, rate, n, amp = 0.5, decay = 0) =>
    Array.from({ length: n }, (_, i) => amp * Math.sin((2 * Math.PI * hz * i) / rate) * Math.exp(-decay * i / rate))
const rnd = mulberry32(3)
const noise = (n, amp) => Array.from({ length: n }, () => (rnd() * 2 - 1) * amp)

const CASES = {
    'a220-16bit': wav([tone(220, 44100, 9000)], 44100, 1, 16),
    'a440-float-48k': wav([tone(440, 48000, 6000, 0.8)], 48000, 3, 32),
    'c2-decay-24bit': wav([tone(65.41, 44100, 20000, 0.9, 6)], 44100, 1, 24),
    'high-1500-32bit': wav([tone(1500, 44100, 5000, 0.3)], 44100, 1, 32),
    'stereo-detuned': wav([tone(330, 44100, 7000, 0.6), tone(331, 44100, 7000, 0.4, 3)], 44100, 1, 16),
    'noise-8bit': wav([noise(5000, 0.7)], 22050, 1, 8),
    'noisy-tone': wav([tone(196, 44100, 9000, 0.5).map((v, i) => v + noise(1, 0.2)[0])], 44100, 1, 16),
    silence: wav([new Array(3000).fill(0)], 44100, 1, 16),
    'very-short': wav([[0.1, -0.5, 0.25, 0.9, -0.2]], 44100, 1, 16),
    'shorter-than-envelope': wav([tone(1000, 8000, 100, 0.5)], 8000, 3, 32),
    'dc-offset': wav([tone(100, 44100, 6000, 0.3).map((v) => v + 0.4)], 44100, 1, 16),
}
CASES['kick.wav'] = [...(await readFile(join(root, 'crates/ordrumbox-engine/tests/fixtures/kick.wav')))]

const analyzer = new Analyzer()
const cases = []
for (const [name, bytes] of Object.entries(CASES)) {
    const a = analyzer.analyzeWavBuffer(new Uint8Array(bytes))
    const note = a.fundamentalHz ? hzToNote(a.fundamentalHz) : null
    cases.push({
        name,
        bytes,
        envelope: a.envelope,
        durationSec: a.durationSec,
        peakLinear: a.peakLinear ?? 0,
        peakDb: Number.isFinite(a.peakDb) ? a.peakDb : null,
        rmsDb: Number.isFinite(a.rmsDb) ? a.rmsDb : null,
        fundamentalHz: a.fundamentalHz,
        pitchConfidence: a.pitchConfidence,
        note: note ? formatNote(note) : null,
    })
}
const out = join(root, 'crates/ordrumbox-engine/tests/fixtures/analysis.json')
await writeFile(out, JSON.stringify({ cases }) + '\n')
process.stdout.write(`${cases.length} analyses\n`)
