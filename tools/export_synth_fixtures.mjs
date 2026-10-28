#!/usr/bin/env node
// Golden fixtures for the synth editor, produced by the JS app's own code:
//
// - SynthPresetModel.loadPreset() (ui/synth_editor/synth_preset_model.js):
//   every preset of assets/data/generated_sounds.json and hand-made ones,
//   filled with the group defaults (hydrate), and the cards it shows
//   bypassed (hydrateCardBypassed);
// - GroupsSection.getOrderedGroupNames(): the cards, in order;
// - SYNTH_LFO_TARGETS: what an LFO can modulate;
// - SynthPresetModel.computeLfoMod(): an LFO's modulation at several times,
//   for every target, wave and sync, at 120 BPM.
//
//   node tools/export_synth_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-app/tests/fixtures/synth_editor.json.

import { readFile, writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_synth_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

const here = dirname(fileURLToPath(import.meta.url))
const outDir = join(here, '..', 'crates', 'ordrumbox-app', 'tests', 'fixtures')
const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href

const { default: SynthPresetModel } = await import(srcUrl('ui/synth_editor/synth_preset_model.js'))
const { default: GroupsSection } = await import(srcUrl('ui/synth_editor/groups_section.js'))
const { SYNTH_LFO_TARGETS, LFO_TARGET_SCALE } = await import(srcUrl('ui/synth_editor/synth_editor_constants.js'))
const { WAVE_TYPES } = await import(srcUrl('audio/fx_values.js'))

const sounds = JSON.parse(await readFile(resolve(jsRepo, 'assets', 'data', 'generated_sounds.json'), 'utf8'))
const hand = {
    empty: {},
    partial: { vco2: { gain: 0.4 }, filter: 'broken', bypassLfo1: true, bypassFm: 1, extra: { a: 1 }, zeta: 3 },
    bypassed: { vco1: { gain: 0 }, bypassNoise: true, bypassFilter: true, bypassEnv: false, bypassModEnv: true },
}
const soundRegistry = { generatedSounds: { ...structuredClone(sounds), ...structuredClone(hand) } }
const model = new SynthPresetModel({ soundRegistry, serviceRegistry: { transport: { bpm: 120 } } })
const groups = new GroupsSection(model)

const presets = Object.keys(soundRegistry.generatedSounds).map((key) => {
    model.loadPreset(key)
    model.hydrateCardBypassed()
    return {
        key,
        source: soundRegistry.generatedSounds[key],
        draft: structuredClone(model.draft),
        bypassed: { ...model.cardBypassed },
        groups: groups.getOrderedGroupNames(),
    }
})

const lfoMods = []
const targets = [...SYNTH_LFO_TARGETS, 'unknown']
for (const target of targets) {
    for (const wave of [...WAVE_TYPES, 'bogus']) {
        for (const [freq, depth, sync] of [
            [1.3, 0.5, 'off'],
            [0.25, 1, '1/8T'],
            [0, 0.5, 'off'],
            [2, 0, 'off'],
        ]) {
            const lfo = { target, wave, freq, depth, sync }
            const at = [0, 0.1, 0.37, 1.5, 7.77].map((t) => model.computeLfoMod(lfo, t))
            lfoMods.push({ lfo, at })
        }
    }
}

await mkdir(outDir, { recursive: true })
await writeFile(
    join(outDir, 'synth_editor.json'),
    JSON.stringify({ presets, lfoTargets: SYNTH_LFO_TARGETS, lfoScale: LFO_TARGET_SCALE, lfoMods }) + '\n',
)
process.stdout.write(`synth editor: ${presets.length} presets, ${lfoMods.length} LFO cases\n`)
