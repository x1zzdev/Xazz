import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import {
  demoFillValue,
  filledRows,
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

const componentMap = await read('design/component-map.md')
for (let index = 1; index <= 18; index += 1) {
  const requirement = `R-${String(index).padStart(3, '0')}`
  assert.match(componentMap, new RegExp(requirement), `${requirement} must be traced`)
}

const workspace = await read('ui-prototype/src/components/Workspace.jsx')
for (const axis of ['Maturity', 'Process', 'Pipeline', 'Control', 'Integrity']) {
  assert.match(workspace, new RegExp(`axis="${axis}"`), `${axis} axis must be explicit`)
}
assert.match(workspace, /Exited with code 0/)
assert.match(workspace, /pipeline is partial/i)
assert.match(workspace, /not persisted/i)
assert.match(workspace, /Not available in current runtime/)
assert.match(workspace, /Live Check demo · Future contract/)
assert.doesNotMatch(workspace, /DEMO-0727-001/)
assert.match(workspace, /Fixture ID/)
assert.match(workspace, /Not available in failed run/)
assert.match(workspace, /Runtime readiness · synthetic state/)
assert.match(workspace, /Future contract · not verified/)
assert.match(workspace, /axis="Run confirmation"/)
assert.match(workspace, /Not requested by run · optional export after result/)
assert.match(workspace, /inert=\{runState === 'preflight'/)
assert.doesNotMatch(workspace, /Demo CSV ready|Ready · 0\.2\.8/)

const dataSource = await read('ui-prototype/src/data.js')
assert.doesNotMatch(
  dataSource,
  /demo median/i,
  'a fixed synthetic fill value must not be presented as a calculated median',
)

const landing = await read('ui-prototype/src/components/Landing.jsx')
assert.match(landing, /axis="Maturity" tone="success" compact>\s*Available/)
assert.match(landing, /Future Labs/)
assert.match(landing, /Research/)
assert.match(landing, /Planned/)
assert.match(landing, /Future contract/)
assert.doesNotMatch(landing, /Policy passed|Budget safe/)
assert.match(landing, /not <strong>Audited<\/strong>/)

const index = await read('ui-prototype/index.html')
const css = await read('ui-prototype/src/styles.css')
assert.doesNotMatch(index, /fonts\.googleapis|cdn\./i)
assert.doesNotMatch(css, /@import\s+url/i)
assert.doesNotMatch(css, /backdrop-filter|text-shadow/i)
assert.match(css, /\.preflight-warning__check/)
assert.match(css, /\.flow-node--relation-upstream/)

console.log(
  `contract: ok; fixture=100→${scenario.resultCount}; requirements=18/18; forbidden CDN/effects=0`,
)
