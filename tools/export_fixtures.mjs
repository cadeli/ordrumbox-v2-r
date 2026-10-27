#!/usr/bin/env node
// Golden fixtures for the Rust port, produced by the JS app itself.
//
//   node tools/export_fixtures.mjs <path-to-ordrumbox-v2> [out-dir]
//
// out-dir defaults to crates/ordrumbox-core/tests/fixtures. It receives:
//   song.json          the library file, copied verbatim (parser input; it is
//                      itself JSON.stringify(…, null, 2) + "\n", the pretty writer reference)
//   song.min.json      JSON.stringify(JSON.parse(song.json)) (compact writer reference)
//   flat_notes/<id>.json  recomputeFlatNotes(fixPattern(p), loop) for loop 0..LOOP_COUNT-1
//   library.export.json   song.json loaded (fixPatterns + fixSongs) then exported
//                         (Exporter.export per pattern): the model round-trip reference
//   edge_library.export.json  the same for edge_library.json, a hand-written input
//                         (kept in out-dir) covering fixer/exporter paths song.json misses
//   slugify.json          [input, slugify(input)] pairs for the id helpers
//
// A pass is `deterministic: false` when Math.random was called while computing
// it (prob < 1, arp "random", variation…). The Rust golden tests compare only
// deterministic passes; the others are covered by property tests.

import { readFile, writeFile, mkdir, rm } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const LOOP_COUNT = 4

const [jsRepo, outArg] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_fixtures.mjs <path-to-ordrumbox-v2> [out-dir]\n')
    process.exit(2)
}

const here = dirname(fileURLToPath(import.meta.url))
const outDir = resolve(outArg ?? join(here, '..', 'crates', 'ordrumbox-core', 'tests', 'fixtures'))
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href

const { fixPatterns, fixSongs } = await import(srcUrl('patterns/fixer.js'))
const { recomputeFlatNotes } = await import(srcUrl('patterns/pattern_engine.js'))
const { Exporter } = await import(srcUrl('patterns/exporter.js'))
const { slugify } = await import(srcUrl('core/ids.js'))

const SLUGIFY_INPUTS = [
    'Rock 8a',
    '  Bossa Nova  ',
    'Ébauche n°1',
    'Ça déménage à Noël',
    'ÀÁÂÃÄÅàáâãäåÇçÈÉÊËèéêëÌÍÎÏìíîïÑñÒÓÔÕÖòóôõöÙÚÛÜùúûüÝýÿ',
    'ĀāĂăĄąĆćĈĉĊċČčĎďĒēĔĕĖėĘęĚěĜĝĞğĠġĢģĤĥĨĩĪīĬĭĮįİĴĵĶķĹĺĻļĽľŃńŅņŇňŌōŎŏŐőŔŕŖŗŘřŚśŜŝŞşŠšŢţŤťŨũŪūŬŭŮůŰűŲųŴŵŶŷŸŹźŻżŽž',
    'Ǎǎǐǒǔǖǘǚǜǧǩǫǭǰǵǹȁȃȅȇȉȋȍȏȑȓȕȗșțȟȧȩȫȭȯȱȳ',
    'ḀḁḂḃḄḅḆḇẠạẢảẤấẦầẪẫẬậẮắẰằẲẳẴẵẶặẸẹẺẻẼẽẾếỀềỂểỄễỆệỈỉỊịỌọỎỏỐốỒồỔổỖỗỘộỚớỜờỞởỠỡỢợỤụỦủỨứỪừỬửỮữỰựỲỳỴỵỶỷỸỹ',
    'Straße Œuvre Æsir Øresund Łódź Đà Nẵng ıi',
    'e\u0301te\u0300 combining',
    'Kelvin \u212a Ångström \u212b',
    'Ωmega Δelta 漢字 🥁 drums',
    '***',
    '',
    '---a---b---',
    'x'.repeat(60),
    'É'.repeat(60),
    'TR-808 / TR-909 (2026)',
]

let randomCalled = false
Math.random = () => {
    randomCalled = true
    return 0.5
}

/** One flat note, keyed back to its source: track index and note index (null for a clone). */
function encodeFlatNote(fn, pattern) {
    const trackIdx = pattern.tracks.indexOf(fn.track)
    const noteIdx = trackIdx < 0 ? -1 : fn.track.notes.indexOf(fn.note)
    return {
        track: trackIdx,
        note: noteIdx < 0 ? null : noteIdx,
        pitch: fn.note.pitch ?? 0,
        velocity: fn.note.velocity ?? null,
    }
}

function computePass(pattern, loop) {
    randomCalled = false
    const flat = recomputeFlatNotes(pattern, loop)
    const ticks = [...flat.keys()]
        .sort((a, b) => a - b)
        .map((tick) => [tick, flat.get(tick).map((fn) => encodeFlatNote(fn, pattern))])
    return { loop, deterministic: !randomCalled, ticks }
}

const songPath = resolve(jsRepo, 'assets', 'data', 'song.json')
const raw = await readFile(songPath, 'utf8')
const parsed = JSON.parse(raw)

await rm(join(outDir, 'flat_notes'), { recursive: true, force: true })
await mkdir(join(outDir, 'flat_notes'), { recursive: true })
await writeFile(join(outDir, 'song.json'), raw)
await writeFile(join(outDir, 'song.min.json'), JSON.stringify(parsed))

/** A library file loaded the way the app does, then written back by the exporter. */
function exportLibrary(lib) {
    const fixed = fixPatterns(structuredClone(lib.patterns ?? []))
    const { songs, dropped } = fixSongs(structuredClone(lib.songs), fixed)
    return { infos: lib.infos, songs, dropped, patterns: fixed.map((p) => Exporter.export(p)) }
}

await writeFile(join(outDir, 'library.export.json'), JSON.stringify(exportLibrary(parsed)) + '\n')
const edge = JSON.parse(await readFile(join(outDir, 'edge_library.json'), 'utf8'))
await writeFile(join(outDir, 'edge_library.export.json'), JSON.stringify(exportLibrary(edge)) + '\n')
await writeFile(
    join(outDir, 'slugify.json'),
    JSON.stringify(SLUGIFY_INPUTS.map((s) => [s, slugify(s)])) + '\n',
)

const patterns = fixPatterns(structuredClone(parsed.patterns))
let deterministicCount = 0
for (const pattern of patterns) {
    const passes = Array.from({ length: LOOP_COUNT }, (_, loop) => computePass(pattern, loop))
    deterministicCount += passes.filter((p) => p.deterministic).length
    const body = { id: pattern.id, beatCount: pattern.beatCount, passes }
    await writeFile(join(outDir, 'flat_notes', `${pattern.id}.json`), JSON.stringify(body) + '\n')
}

process.stdout.write(
    `${patterns.length} patterns, ${deterministicCount}/${patterns.length * LOOP_COUNT} deterministic passes -> ${outDir}\n`,
)
