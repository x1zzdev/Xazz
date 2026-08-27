import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import {
  codeLines,
  demoFillValue,
  filledRows,
  pipeline,
  resultRows,
  scenario,
  sourceRows,
} from '../src/data.js'

const here = dirname(fileURLToPath(import.meta.url))
const prototypeRoot = resolve(here, '..')
const repositoryRoot = resolve(prototypeRoot, '..')

const read = (path) => readFile(resolve(repositoryRoot, path), 'utf8')

assert.equal(sourceRows.length, 100, 'fixture must contain 100 deterministic rows')
assert.equal(demoFillValue, 31, 'the synthetic demo must use its documented fixed fill value')
assert.equal(
  scenario.fillValue,
  demoFillValue,
  'scenario metadata and the transform must share one fill-value source',
)
for (const [index, sourceRow] of sourceRows.entries()) {
  if (sourceRow.pm25 === null) {
    assert.equal(
      filledRows[index].pm25,
      demoFillValue,
      `source null at row ${index} must use the fixed demo value`,
    )
  }
}
assert.equal(
  resultRows.length,
  scenario.resultCount,
  'visible result count must come from the fixture',
)
assert.equal(
  resultRows.some((row) => row.pm25 === null),
  false,
  'result fixture must not retain null PM2.5 values',
)

const componentMap = await read('docs/design/component-map.md')
for (let index = 1; index <= 18; index += 1) {
  const requirement = `R-${String(index).padStart(3, '0')}`
  assert.match(componentMap, new RegExp(requirement), `${requirement} must be traced`)
}

const workspace = await read('visual-ide/src/components/Workspace.jsx')
for (const axis of ['Maturity', 'Process', 'Pipeline', 'Control', 'Integrity']) {
  assert.match(workspace, new RegExp(`axis="${axis}"`), `${axis} axis must be explicit`)
}
assert.match(workspace, /xazz-exec exited 0/)
assert.match(workspace, /pipeline is partial|pipeline exited with an error/i)
assert.match(workspace, /not persisted/)
assert.match(workspace, /Not available in current runtime/)
assert.match(workspace, /Connect to xazz-server to execute · not yet run/)
assert.doesNotMatch(workspace, /DEMO-0727-001/)
assert.match(workspace, /Full Run/i)
assert.match(workspace, /Not available in failed run/)
assert.match(workspace, /Runtime readiness · connected check/)
assert.match(workspace, /Verified by Full Run response/)
assert.match(workspace, /axis="Run confirmation"/)
assert.match(workspace, /Returned in browser · no file written/)
assert.match(workspace, /inert=\{runState === 'preflight'/)
assert.doesNotMatch(workspace, /Demo CSV ready|Ready · 0\.2\.8/)
assert.match(workspace, /executeCode/)
assert.match(workspace, /checkHealth/)
assert.match(workspace, /API_BASE_URL/)

const dataSource = await read('visual-ide/src/data.js')
assert.doesNotMatch(
  dataSource,
  /demo median/i,
  'a fixed synthetic fill value must not be presented as a calculated median',
)

const landing = await read('visual-ide/src/components/Landing.jsx')
assert.match(landing, /axis="Maturity" tone="success" compact>\s*Available/)
assert.match(landing, /Future Labs/)
assert.match(landing, /Research/)
assert.match(landing, /Planned/)
assert.match(landing, /Future contract/)
assert.doesNotMatch(landing, /Policy passed|Budget safe/)
assert.match(landing, /not <strong>Audited<\/strong>/)

const index = await read('visual-ide/index.html')
const css = await read('visual-ide/src/styles.css')
assert.doesNotMatch(index, /fonts\.googleapis|cdn\./i)
assert.doesNotMatch(css, /@import\s+url/i)
assert.doesNotMatch(css, /backdrop-filter|text-shadow/i)
assert.match(css, /\.preflight-warning__check/)
assert.match(css, /\.flow-node--relation-upstream/)

// ── ML compile band + monitoring (issue x1zzdev/Xazz#1) ─────────────────────
//
// These guard the one property the Monitor view exists to hold: a capability the
// backend does not implement must never be able to render as a measurement.

const mlNodes = ['compile', 'train', 'predict']
for (const id of mlNodes) {
  const node = pipeline.find((item) => item.id === id)
  assert.ok(node, `${id} node must exist in the pipeline`)
  assert.equal(node.band, 'ML COMPILE', `${id} must declare the ML COMPILE band`)
  assert.ok(
    codeLines[node.codeLine - 1] !== undefined,
    `${id} must point at a real source line`,
  )
}
assert.equal(
  pipeline.filter((node) => node.band === 'PREPROCESS').length,
  5,
  'the five preprocessing stages must keep their band',
)
for (const node of pipeline) {
  assert.ok(Array.isArray(node.from), `${node.id} must declare its edge sources`)
  assert.ok(node.position, `${node.id} must declare an explicit canvas position`)
  for (const source of node.from) {
    assert.ok(
      pipeline.some((item) => item.id === source),
      `${node.id} references unknown source ${source}`,
    )
  }
}

const monitor = await read('visual-ide/src/components/Monitor.jsx')
const implemented = JSON.parse(await read('visual-ide/src/mock/execute-response.json'))
const proposed = JSON.parse(await read('visual-ide/src/mock/telemetry-proposed.json'))

// The two mock files must stay honest about which contract they represent.
assert.equal(implemented._contract, 'implemented')
assert.equal(proposed._contract, 'proposed')
assert.equal(proposed._measured, false)

// TrainReport has no epoch history and no timing. Nothing may invent them.
const reportFields = Object.keys(implemented.training.report)
for (const forbidden of ['loss_history', 'epoch_losses', 'duration_ms', 'elapsed']) {
  assert.ok(
    !reportFields.includes(forbidden),
    `TrainReport does not emit ${forbidden}; the fixture must not add it`,
  )
}
assert.ok(implemented._absent_from_contract.per_epoch_loss_history)

// data.js ML evidence lines must agree with the mock contract they claim to mirror.
const trainNode = pipeline.find((node) => node.id === 'train')
assert.match(
  trainNode.evidence,
  new RegExp(String(implemented.training.report.epochs)),
  'train node evidence must cite the epochs the fixture reports',
)
assert.match(
  trainNode.evidence,
  new RegExp(String(implemented.training.report.final_train_loss)),
  'train node evidence must cite the loss the fixture reports',
)
const compileNode = pipeline.find((node) => node.id === 'compile')
assert.match(
  compileNode.evidence,
  new RegExp(String(implemented.training.report.num_params)),
  'compile node evidence must cite the parameter count the fixture reports',
)

// Implemented panels keep an honest "not yet measured" state; unimplemented
// panels stay proposed and hollow. No panel borrows a success colour.
assert.match(monitor, /contract="implemented"/)
assert.match(monitor, /contract="measured"/)
assert.match(monitor, /maturity="Real"/)
assert.match(monitor, /maturity="Planned"/)
assert.match(monitor, /contract="proposed"/)
assert.match(monitor, /Synthetic structure · not measured · proposed contract/)
assert.doesNotMatch(monitor, /tone="success"|tone="warning"|tone="danger"/)
for (const forbidden of [
  /budget safe/i,
  /policy passed/i,
  /\baudited\b/i,
  /\bsandboxed\b/i,
  /budget remaining/i,
]) {
  assert.doesNotMatch(monitor, forbidden, `forbidden claim ${forbidden} reappeared`)
}

// Unmeasured bars are hollow, not filled — the distinction cannot be colour-only.
assert.match(css, /\.monitor-bars__fill--proposed \{[^}]*background: transparent/)
assert.match(css, /\.monitor-bars__fill--proposed \{[^}]*border: 1px dashed/)
assert.doesNotMatch(
  css,
  /\.monitor-panel--proposed \{[^}]*var\(--success-dark\)/,
  'a proposed panel must not use the success token',
)

const workspaceMonitor = workspace
// The label moved into src/i18n.jsx when the view gained Korean; the view id and
// its icon are what this guards.
assert.match(workspaceMonitor, /\['monitor', Activity\]/)
assert.match(workspaceMonitor, /view === 'monitor'/)

console.log(
  `contract: ok; fixture=100→${scenario.resultCount}; requirements=18/18; forbidden CDN/effects=0; ml-nodes=${mlNodes.length}; monitor-panels=3`,
)
