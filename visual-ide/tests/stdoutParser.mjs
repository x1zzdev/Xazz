/**
 * stdoutParser.mjs — unit tests for the stdout marker parser (issue #51).
 *
 * Covers the `[xazz:*]` marker contract consumed by the Visual IDE:
 *   - single-line `[xazz:chart] {JSON}` for bar/line/pie/scatter (current format)
 *   - legacy two-line `[xazz:chart]` + JSON (fallback)
 *   - `[xazz:train] {JSON}` / `[xazz:model] {JSON}` → train/model events
 *   - `[xazz:dp] {JSON}` is not a chart/error event — it falls through to text
 *   - `[xazz:result] {JSON}` is exposed as text
 *   - `[xazz:error]` + `AI_SUGGESTION:` legacy block → error event
 *   - `[xazz RUNTIME ERROR]` / `[xazz PARSER ERROR]` / `[xazz IO ERROR]` → error event
 *   - non-marker lines → text events
 *
 * Run:  cd visual-ide && npm run test:stdout
 *       (plain Node, ESM, no browser/server — no extra dependencies)
 */

import assert from 'node:assert/strict'
import {
  parseStdout,
  getChartEvents,
  getErrorEvents,
  getTextEvents,
} from '../src/transpiler/stdoutParser.js'

// ── empty / trivial input ────────────────────────────────────────────────────

assert.deepEqual(parseStdout([]), [], 'empty input yields no events')
assert.deepEqual(parseStdout(null), [], 'non-array input yields no events')

// ── single-line [xazz:chart] {JSON} — bar / line ─────────────────────────────

const barPayload = {
  chartType: 'bar',
  title: 'PM10 by station',
  x: 'station',
  y: 'pm10',
  data: [
    { station: 'A', pm10: 12 },
    { station: 'B', pm10: null },
  ],
}
const barEvents = parseStdout([`[xazz:chart] ${JSON.stringify(barPayload)}`])
assert.equal(barEvents.length, 1, 'a single-line chart marker yields one event')
const bar = barEvents[0]
assert.equal(bar.type, 'chart', 'single-line marker yields a chart event')
assert.equal(bar.chartType, 'bar', 'chartType is preserved')
assert.equal(bar.title, 'PM10 by station', 'title is preserved')
assert.equal(bar.data.length, 2, 'data rows are preserved')
assert.deepEqual(
  { label: bar.data[0].label, value: bar.data[0].value, x: bar.data[0].x, y: bar.data[0].y },
  { label: 'A', value: 12, x: 'A', y: 12 },
  'bar rows expose label/value and x/y',
)
assert.equal(bar.data[1].value, 0, 'null y values coerce to 0')
assert.equal(bar.data[1].y, 0, 'null y values coerce to 0 on the y axis')

const linePayload = {
  chartType: 'line',
  x: 'ts',
  y: 'value',
  data: [{ ts: 't0', value: '3.5' }],
}
const line = parseStdout([`[xazz:chart] ${JSON.stringify(linePayload)}`])[0]
assert.equal(line.chartType, 'line', 'line charts are supported')
assert.equal(line.data[0].value, 3.5, 'numeric strings are coerced on the y axis')

// ── single-line [xazz:chart] {JSON} — pie ────────────────────────────────────

const piePayload = {
  chartType: 'pie',
  title: 'Share',
  label: 'name',
  value: 'count',
  data: [{ name: 'x', count: 7 }],
}
const pie = parseStdout([`[xazz:chart] ${JSON.stringify(piePayload)}`])[0]
assert.equal(pie.type, 'chart', 'pie marker yields a chart event')
assert.equal(pie.chartType, 'pie', 'pie chartType is preserved')
assert.deepEqual(
  { label: pie.data[0].label, value: pie.data[0].value },
  { label: 'x', value: 7 },
  'pie rows expose label/value',
)
assert.ok(!('x' in pie.data[0]), 'pie rows do not get x/y fields')

// ── unknown chartType falls back to the whitelist default ────────────────────

const fallback = parseStdout([
  `[xazz:chart] ${JSON.stringify({ chartType: 'donut', x: 'a', y: 'b', data: [] })}`,
])[0]
assert.equal(fallback.chartType, 'bar', 'unknown chartType falls back to bar')

// ── single-line [xazz:chart] {JSON} — scatter ────────────────────────────────

const scatterPayload = {
  chartType: 'scatter',
  title: 'pm10 vs pm25',
  x: 'pm10',
  y: 'pm25',
  data: [{ pm10: '1.5', pm25: 2 }],
}
const scatter = parseStdout([`[xazz:chart] ${JSON.stringify(scatterPayload)}`])[0]
assert.equal(scatter.type, 'chart', 'scatter marker yields a chart event')
assert.equal(scatter.chartType, 'scatter', 'scatter chartType is preserved')
assert.deepEqual(
  { x: scatter.data[0].x, y: scatter.data[0].y },
  { x: 1.5, y: 2 },
  'scatter rows expose numeric x/y and coerce strings',
)
assert.ok(!('label' in scatter.data[0]), 'scatter rows do not get label/value fields')

// ── legacy two-line [xazz:chart] + JSON (fallback) ───────────────────────────

const legacy = parseStdout(['[xazz:chart]', JSON.stringify(piePayload)])
assert.equal(legacy.length, 1, 'legacy two-line chart yields one event')
assert.equal(legacy[0].type, 'chart', 'legacy two-line form still parses')
assert.equal(legacy[0].chartType, 'pie', 'legacy form preserves chartType')
assert.equal(legacy[0].data[0].label, 'x', 'legacy form transforms data')

const legacyBad = parseStdout(['[xazz:chart]', 'not json'])
assert.equal(legacyBad.length, 1, 'legacy chart with bad JSON yields one event')
assert.equal(legacyBad[0].type, 'text', 'unparseable legacy chart payload becomes text')

// ── [xazz:dp] {JSON} is not a chart/error event ──────────────────────────────

const dpLines = [`[xazz:dp] ${JSON.stringify({ mechanism: 'laplace', epsilon: 1 })}`]
const dpEvents = parseStdout(dpLines)
assert.equal(dpEvents.length, 1, 'a [xazz:dp] line yields exactly one event')
assert.equal(dpEvents[0].type, 'text', '[xazz:dp] is not parsed as a chart/error event')
assert.equal(dpEvents[0].text, dpLines[0], '[xazz:dp] falls through to a text event')
assert.equal(getChartEvents(dpEvents).length, 0, '[xazz:dp] emits no chart event')
assert.equal(getErrorEvents(dpEvents).length, 0, '[xazz:dp] emits no error event')

// ── [xazz:result] {JSON} is exposed as text ──────────────────────────────────

const resultLine = `[xazz:result] ${JSON.stringify({ rows: 3 })}`
const resultEvents = parseStdout([resultLine])
assert.equal(resultEvents.length, 1, 'a [xazz:result] line yields one event')
assert.equal(resultEvents[0].type, 'text', '[xazz:result] is surfaced as text')
assert.equal(resultEvents[0].text, resultLine, '[xazz:result] text keeps the raw line')

// ── [xazz:train] {JSON} → train event ────────────────────────────────────────

const trainPayload = {
  type: 'train_stmt',
  success: true,
  source_var: 'air',
  model_name: 'AirNet',
  report: { epochs: 10, final_loss: 0.05 },
}
const trainEvents = parseStdout([`[xazz:train] ${JSON.stringify(trainPayload)}`])
assert.equal(trainEvents.length, 1, 'a [xazz:train] line yields one event')
assert.equal(trainEvents[0].type, 'train', '[xazz:train] yields a train event')
assert.deepEqual(
  trainEvents[0].report,
  { epochs: 10, final_loss: 0.05 },
  'the nested report is unwrapped',
)
assert.equal(trainEvents[0].raw.model_name, 'AirNet', 'the raw payload is retained')

const bareTrain = parseStdout([`[xazz:train] ${JSON.stringify({ epochs: 3 })}`])[0]
assert.deepEqual(
  bareTrain.report,
  { epochs: 3 },
  'payload is used as report when the report key is absent',
)

// ── [xazz:model] {JSON} → model event ────────────────────────────────────────

const modelPayload = {
  type: 'model_decl',
  name: 'AirNet',
  layers: ['Linear(3, 16)', 'ReLU', 'Linear(16, 1)'],
}
const modelEvents = parseStdout([`[xazz:model] ${JSON.stringify(modelPayload)}`])
assert.equal(modelEvents.length, 1, 'a [xazz:model] line yields one event')
assert.equal(modelEvents[0].type, 'model', '[xazz:model] yields a model event')
assert.equal(modelEvents[0].model.name, 'AirNet', 'the model payload is exposed')
assert.equal(modelEvents[0].model.layers.length, 3, 'model layers are preserved')

// ── malformed inline train/model payloads fall back to text ──────────────────

const badTrain = parseStdout(['[xazz:train] {not json'])[0]
assert.equal(badTrain.type, 'text', 'unparseable [xazz:train] payload becomes text')
const badModel = parseStdout(['[xazz:model] {not json'])[0]
assert.equal(badModel.type, 'text', 'unparseable [xazz:model] payload becomes text')

// ── [xazz:error] + AI_SUGGESTION legacy block ────────────────────────────────

const errorEvents = parseStdout([
  '[xazz:error]',
  'ERROR[E42]: column not found',
  'AI_SUGGESTION: did you mean pm10?',
])
assert.equal(errorEvents.length, 1, 'the legacy error block yields one event')
assert.equal(errorEvents[0].type, 'error', 'legacy error block yields an error event')
assert.equal(errorEvents[0].code, 'E42', 'the error code is parsed')
assert.equal(errorEvents[0].message, 'column not found', 'the error message is parsed')
assert.equal(
  errorEvents[0].suggestion,
  'did you mean pm10?',
  'the AI_SUGGESTION line is attached',
)

const errorNoSuggestion = parseStdout(['[xazz:error]', 'ERROR: plain failure'])
assert.equal(errorNoSuggestion[0].suggestion, null, 'suggestion is null when absent')

// ── [xazz <KIND> ERROR] generic error lines ──────────────────────────────────

const runtimeErr = parseStdout(['[xazz RUNTIME ERROR] column pm10 not found'])[0]
assert.equal(runtimeErr.type, 'error', 'a [xazz RUNTIME ERROR] line yields an error event')
assert.equal(runtimeErr.code, 'RUNTIME', 'the error kind becomes the code')
assert.equal(runtimeErr.message, 'column pm10 not found', 'the trailing message is captured')
assert.ok(runtimeErr.suggestion, 'generic errors carry a re-run suggestion')

const parserErr = parseStdout(['[xazz PARSER ERROR] unexpected token `|>`'])[0]
assert.equal(parserErr.code, 'PARSER', 'the parser error kind is captured')
assert.equal(parserErr.message, 'unexpected token `|>`', 'the parser error message is captured')

// ── [xazz IO ERROR] line ─────────────────────────────────────────────────────

const ioErr = parseStdout(['[xazz IO ERROR] data file not found: data/x.csv'])[0]
assert.equal(ioErr.type, 'error', 'a [xazz IO ERROR] line yields an error event')
assert.equal(ioErr.code, 'IO_ERROR', 'IO errors use the IO_ERROR code')
assert.equal(ioErr.message, 'data file not found: data/x.csv', 'the IO message is captured')
assert.ok(ioErr.suggestion.includes('data/'), 'IO errors suggest checking the data/ path')

// ── random / non-marker lines ────────────────────────────────────────────────

const noise = parseStdout(['hello world', '', '  ', 'loading data.csv...'])
assert.deepEqual(
  noise.map((e) => e.type),
  ['text', 'text'],
  'non-empty non-marker lines become text; blank lines are dropped',
)
assert.equal(getTextEvents(noise).length, 2, 'getTextEvents keeps only text events')

// ── helper filters ───────────────────────────────────────────────────────────

const mixed = parseStdout([
  `[xazz:chart] ${JSON.stringify(barPayload)}`,
  'plain output',
  '[xazz:error]',
  'ERROR[E1]: boom',
])
assert.equal(getChartEvents(mixed).length, 1, 'getChartEvents isolates chart events')
assert.equal(getErrorEvents(mixed).length, 1, 'getErrorEvents isolates error events')
assert.equal(getTextEvents(mixed).length, 1, 'getTextEvents isolates text events')

console.log(
  'stdoutParser: ok; chart(bar,line,pie,scatter); train/model; error(io,runtime,parser,legacy); helpers=3',
)