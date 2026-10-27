#!/usr/bin/env node
// Golden fixtures for the value controls, produced by the JS app's own OrKnob
// and OrSlider (ui/components/or_knob.js, or_slider.js) driven through a
// minimal fake DOM, and by its formatters (ui_utils.js knobFormat / fmt /
// formatSignedDb, track_editor_constants.js fmtVal, the loop section's loop
// point label).
//
//   node tools/export_control_fixtures.mjs <path-to-ordrumbox-v2>
//
// Writes crates/ordrumbox-app/tests/fixtures/controls.json.

import { writeFile, mkdir } from 'node:fs/promises'
import { join, resolve, dirname } from 'node:path'
import { pathToFileURL, fileURLToPath } from 'node:url'

const [jsRepo] = process.argv.slice(2)
if (!jsRepo) {
    process.stderr.write('usage: node tools/export_control_fixtures.mjs <path-to-ordrumbox-v2>\n')
    process.exit(2)
}
for (const level of ['warn', 'error', 'info', 'debug', 'log']) console[level] = () => {}

// ─── A fake DOM, just enough for OrKnob / OrSlider ───────────────────────────

class ClassList {
    #set = new Set()
    add(...c) {
        c.forEach((x) => this.#set.add(x))
    }
    remove(...c) {
        c.forEach((x) => this.#set.delete(x))
    }
    toggle(c, on) {
        if (on ?? !this.#set.has(c)) this.#set.add(c)
        else this.#set.delete(c)
    }
    contains(c) {
        return this.#set.has(c)
    }
    set(text) {
        this.#set = new Set(String(text).split(/\s+/).filter(Boolean))
    }
    toString() {
        return [...this.#set].join(' ')
    }
}

class Listeners {
    listeners = {}
    addEventListener(type, fn) {
        ;(this.listeners[type] ??= []).push(fn)
    }
    removeEventListener(type, fn) {
        this.listeners[type] = (this.listeners[type] ?? []).filter((f) => f !== fn)
    }
    dispatch(type, event) {
        for (const fn of [...(this.listeners[type] ?? [])]) fn(event)
    }
}

class El extends Listeners {
    constructor(tag) {
        super()
        this.tagName = tag.toUpperCase()
        this.children = []
        this.dataset = {}
        this.classList = new ClassList()
        this.attributes = {}
        const props = {}
        this.style = { props, setProperty: (k, v) => (props[k] = v) }
        this.textContent = ''
        this.parentNode = null
    }
    get className() {
        return this.classList.toString()
    }
    set className(v) {
        this.classList.set(v)
    }
    append(...c) {
        c.forEach((x) => this.appendChild(x))
    }
    appendChild(c) {
        c.parentNode = this
        this.children.push(c)
        return c
    }
    setAttribute(k, v) {
        this.attributes[k] = String(v)
    }
    removeAttribute(k) {
        delete this.attributes[k]
    }
    #all() {
        return this.children.flatMap((c) => [c, ...c.#all()])
    }
    querySelector(sel) {
        const match = (e) => {
            if (sel.startsWith('.')) return e.classList.contains(sel.slice(1))
            if (sel.startsWith('input')) return e.tagName === 'INPUT' && e.type === 'range'
            return false
        }
        return this.#all().find(match) ?? null
    }
}

const win = new Listeners()
globalThis.window = win
globalThis.document = { createElement: (tag) => new El(tag) }

const srcUrl = (rel) => pathToFileURL(resolve(jsRepo, 'src', rel)).href
const { OrKnob } = await import(srcUrl('ui/components/or_knob.js'))
const { OrSlider } = await import(srcUrl('ui/components/or_slider.js'))
const { knobFormat, knobUnit, fmt, formatSignedDb } = await import(srcUrl('ui/components/ui_utils.js'))
const { fmtVal, GEN_ENGINE_PROPS } = await import(srcUrl('ui/track_editor/track_editor_constants.js'))
const { default: LoopSection } = await import(srcUrl('ui/track_editor/loop_section.js'))

// ─── Cases ───────────────────────────────────────────────────────────────────

const SPECS = [
    { name: 'velocity', min: 0, max: 1, step: 0.01 },
    { name: 'pan', min: -1, max: 1, step: 0.01 },
    { name: 'pitch', min: -24, max: 24, step: 1 },
    { name: 'decay', min: 20, max: 5000, step: 10, scale: 'log' },
    { name: 'freq', min: 20, max: 20000, step: 1 },
    { name: 'q', min: 0.707, max: 18.707, step: 0.01 },
    { name: 'gain', min: -24, max: 6, step: 0.1 },
    { name: 'prob', min: 0, max: 100, step: 1 },
    { name: 'lfoFreq', min: 0.1, max: 2, step: 0.1 },
    { name: 'linearLog', min: 0, max: 10, step: 0.5, scale: 'log' },
]
const DY = [-250, -120, -37, -5, -1, 0, 1, 3, 7, 50, 133, 250]
const MODS = [{}, { shiftKey: true }, { altKey: true }, { ctrlKey: true }]
const TYPED = [-1e6, -30, -0.5, 0, 0.004, 0.333, 1.26, 7.77, 19.5, 333.3, 1e6]

const starts = (s) => [s.min, s.max, (s.min + s.max) / 2, s.min + (s.max - s.min) * 0.137, s.min + s.step * 3]
const ev = (extra = {}) => ({ button: 0, preventDefault() {}, stopPropagation() {}, ...extra })

function knob(spec, value) {
    const k = new OrKnob({ key: 'k', label: 'K', min: spec.min, max: spec.max, step: spec.step, value, scale: spec.scale })
    k.createElement()
    return k
}

function slider(spec, value) {
    const s = new OrSlider({ key: 's', label: 'S', min: spec.min, max: spec.max, step: spec.step, value })
    const row = s.createElement()
    return { s, input: row.querySelector('input') }
}

const cases = SPECS.map((spec) => {
    const out = { ...spec, scale: spec.scale ?? 'linear', drags: [], knobKeys: [], arcs: [], knobTyped: [], sliderKeys: [], sliderTyped: [] }
    for (const start of starts(spec)) {
        for (const dy of DY) {
            for (const fine of [false, true]) {
                const k = knob(spec, start)
                const knobEl = k.el.querySelector('.or-knob')
                knobEl.dispatch('pointerdown', ev({ clientY: 500 }))
                win.dispatch('pointermove', ev({ clientY: 500 - dy, shiftKey: fine }))
                out.drags.push([start, dy, fine, k.getValue()])
                win.dispatch('pointerup', ev())
            }
        }
        for (const key of ['ArrowUp', 'ArrowDown', 'ArrowRight', 'ArrowLeft']) {
            for (const mods of MODS) {
                const k = knob(spec, start)
                k.el.querySelector('.or-knob').dispatch('keydown', ev({ key, ...mods }))
                const { s, input } = slider(spec, start)
                input.dispatch('keydown', ev({ key, ...mods }))
                const m = [!!mods.shiftKey, !!mods.altKey, !!mods.ctrlKey]
                const up = key === 'ArrowUp' || key === 'ArrowRight'
                out.knobKeys.push([start, up, ...m, k.getValue()])
                out.sliderKeys.push([start, up, ...m, s.getValue()])
            }
        }
    }
    for (let i = 0; i <= 20; i++) {
        const v = spec.min - spec.step + ((spec.max - spec.min + 2 * spec.step) * i) / 20
        const k = knob(spec, spec.min)
        k.setValue(v)
        out.arcs.push([v, parseFloat(k.el.querySelector('.or-knob').style.props['--arc-deg']) / 270])
    }
    for (const n of TYPED) {
        out.knobTyped.push([n, knob(spec, spec.min).sanitizePromptInput(n)])
        out.sliderTyped.push([n, slider(spec, spec.min).s.sanitizePromptInput(n)])
    }
    return out
})

// Formats: the value text of a control, unit included, as formatValue shows it.
const VALUES = [-1234.5, -24, -3.14159, -1, -0.505, -0.04, 0, 0.005, 0.01, 0.125, 0.333333, 0.505, 0.707,
    0.999, 1, 1.005, 2.5, 7, 12, 99.995, 100, 499.5, 999.4, 999.6, 1000, 1234.56, 5000, 12345.678, 20000]
const density = GEN_ENGINE_PROPS.find((p) => p.key === 'auto_density').format
const FORMATS = {
    Fmt: { format: fmt },
    Plain: { format: (v) => fmtVal('swingAmount', v) },
    Percent: { format: knobFormat({ key: 'velocity' }), unit: knobUnit({ key: 'velocity' }) },
    SignedPlain: { format: knobFormat({ key: 'pitch' }), unit: knobUnit({ key: 'pitch' }) },
    Millis: { format: knobFormat({ key: 'decay' }), unit: knobUnit({ key: 'decay' }) },
    Freq: { format: (v) => fmtVal('filterFreq', v) },
    Fixed2: { format: (v) => fmtVal('filterQ', v) },
    Fixed1: { format: (v) => v.toFixed(1), unit: 'dB' },
    SignedFixed1: { format: formatSignedDb, unit: 'st' },
    Density: { format: density },
}
const formats = Object.entries(FORMATS).map(([name, { format, unit }]) => {
    const k = new OrKnob({ key: 'f', label: 'F', min: -1e9, max: 1e9, step: 1, format, unit })
    return { name, unit: unit ?? '', values: VALUES.map((v) => [v, k.formatValue(v)]) }
})

// The loop point label, from the loop section itself.
const editor = { track: { stepsPerBeat: 4 }, onLoopSlider() {} }
const loop = new LoopSection(editor)
loop.mount(new El('div'))
const loopSlider = loop.controls.get('loopAtStep')
const loopPoints = []
for (const spb of [1, 3, 4, 8]) {
    editor.track.stepsPerBeat = spb
    for (const step of [1, 2, 3, 4, 5, 8, 9, 16, 17, 31, 64, 128]) loopPoints.push([spb, step, loopSlider.formatValue(step)])
}

const here = dirname(fileURLToPath(import.meta.url))
const dir = join(here, '..', 'crates/ordrumbox-app/tests/fixtures')
await mkdir(dir, { recursive: true })
await writeFile(join(dir, 'controls.json'), JSON.stringify({ cases, formats, loopPoints }) + '\n')
process.stdout.write(`${cases.length} control specs, ${formats.length} formats\n`)
