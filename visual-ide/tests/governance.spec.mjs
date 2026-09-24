// IDE workstream #104 — every new panel against a mocked xazz-server. The preview
// build calls the API on its own origin, so page.route answers for the server.
// The audit fixture is a verbatim response from a locally built xazz-server.
import { expect, test } from '@playwright/test'
import { auditFixture, defaults, HASH, mockServer } from './mockServer.mjs'

async function openMonitor(page) {
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Monitor' }).click()
  await expect(page.locator('.gov-section')).toBeVisible()
}

test('local policy demo compares safe and unsafe sources without running the pipeline', async ({ page }) => {
  const requests = await mockServer(page, {
    'POST /security/policy/check': (request) => {
      const code = request.postDataJSON().code
      const blocked = code.includes('name, patient_id')
      return {
        safe_to_execute: !blocked,
        policy_origin: 'builtin',
        policy: {
          policy_id: 'builtin', policy_version: '1', domain: 'common', risk_level: 'medium',
          safe_to_execute: !blocked, scanned_statements: 1,
          violations: blocked ? [{ rule_id: 'XZP001', rule_name: 'DIRECT_IDENTIFIER_EXPOSED', severity: 'block', message: 'Direct identifier exposure', columns: ['name', 'patient_id'] }] : [],
          warnings: [],
        },
      }
    },
  })
  await openMonitor(page)
  await page.getByRole('button', { name: 'Check safe example' }).click()
  await expect(page.getByText('Guardrail check passed')).toBeVisible()
  await page.getByRole('button', { name: 'Check unsafe example' }).click()
  await expect(page.getByText('Policy check blocked execution')).toBeVisible()
  await expect(page.getByLabel('unsafe example source')).toContainText('name, patient_id')
  await expect(page.getByRole('button', { name: 'Full Run' })).toBeEnabled()
  expect(requests.filter((request) => request.path === '/security/policy/check')).toHaveLength(2)
  expect(requests.filter((request) => request.path === '/execute')).toHaveLength(0)
})

// ── #107 Run history ────────────────────────────────────────────────────────

test('run history lists server runs and reopens one on the Process axis', async ({ page }) => {
  await mockServer(page, {
    'GET /runs': {
      tenant: '',
      runs: [
        { id: 2, code_hash: HASH, status: 'failed', rows: 0, error: 'boom', created_at: 1790000000 },
        { id: 1, code_hash: 'f'.repeat(64), status: 'success', rows: 5, created_at: 1789990000 },
      ],
    },
    'GET /runs/2': { id: 2, code_hash: HASH, status: 'failed', rows: 0, error: 'boom', created_at: 1790000000 },
    [`GET /security/audit/log/${HASH}`]: { hash: HASH, matches: 2, records: auditFixture.records.slice(0, 2) },
    'GET /runs/1': { http: 404, text: "run 1 not found (or not in tenant '')" },
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('tab', { name: 'History' }).click()
  const list = page.getByRole('list', { name: 'Recorded runs, newest first' })
  await expect(list.getByRole('button')).toHaveCount(2)
  await expect(list).toContainText('Exit failed')
  await expect(page.locator('.run-history')).not.toContainText('Succeeded')

  await list.getByRole('button', { name: /#2/ }).click()
  const detail = page.getByLabel('Run 2 receipt')
  await expect(detail.getByLabel('Process: Exit failed')).toBeVisible()
  await expect(detail).toContainText('2 audit record(s) carry this code hash')
  await expect(detail).toContainText('boom')
  await expect(detail).toContainText('xazz-server keeps run metadata only')

  await list.getByRole('button', { name: /#1/ }).click()
  await expect(page.getByText('Run 1 was not found for this tenant')).toBeVisible()
})

test('run history shows empty and offline states instead of rows', async ({ page }) => {
  await mockServer(page)
  await page.goto('/?screen=workspace')
  await page.getByRole('tab', { name: 'History' }).click()
  await expect(page.getByText('No runs recorded for this tenant yet')).toBeVisible()

  await page.unrouteAll({ behavior: 'ignoreErrors' })
  await mockServer(page, { 'GET /runs': 'abort' })
  await page.goto('/?screen=workspace')
  await page.getByRole('tab', { name: 'History' }).click()
  await expect(page.getByText('xazz-server is not reachable').first()).toBeVisible()
})

test('a run from this session restores its rows from History', async ({ page }) => {
  const rows = [
    { district: 'Mapo', pm25: 21.9 },
    { district: 'Jongno', pm25: 27.8 },
  ]
  const execute = {
    success: true,
    rows,
    schema: [
      { name: 'district', type: 'str' },
      { name: 'pm25', type: 'f64' },
    ],
    logs: [],
    stdout: '',
    run_id: 7,
  }
  await mockServer(page, {
    'POST /execute': execute,
    'GET /runs': { tenant: '', runs: [{ id: 7, code_hash: HASH, status: 'success', rows: 2, created_at: 1790000000 }] },
    'GET /runs/7': { id: 7, code_hash: HASH, status: 'success', rows: 2, created_at: 1790000000 },
    [`GET /security/audit/log/${HASH}`]: { http: 404, text: 'none' },
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('7')

  await page.getByRole('tab', { name: 'History' }).click()
  await page.getByRole('button', { name: /#7/ }).click()
  await expect(page.getByLabel('Run 7 receipt')).toContainText('0 audit record(s)')
  await page.getByRole('button', { name: 'Restore result in Preview' }).click()
  await expect(page.locator('.result-summary')).toContainText('2')
  await expect(page.getByRole('cell', { name: 'Mapo' })).toBeVisible()
})

// ── #108 Audit chain ────────────────────────────────────────────────────────

test('audit chain shows the server verdict and names a tampered record', async ({ page }) => {
  await mockServer(page)
  await openMonitor(page)
  const panel = page.getByRole('region', { name: 'Audit hash chain' })
  await expect(panel.getByLabel('Integrity: Verified')).toBeVisible()
  await expect(panel.locator('tbody tr')).toHaveCount(auditFixture.records.length)
  await expect(panel.getByText('inference', { exact: true })).toBeVisible()

  const tampered = structuredClone(auditFixture)
  tampered.records[1].outcome = 'tampered'
  await page.unrouteAll({ behavior: 'ignoreErrors' })
  await mockServer(page, {
    'GET /security/audit/log': tampered,
    'GET /security/audit/chain': { intact: false, records: tampered.records.length },
  })
  await panel.getByRole('button', { name: 'Verify chain' }).click()
  await expect(panel.getByLabel('Integrity: Mismatch')).toBeVisible()
  await expect(panel.getByRole('alert')).toContainText(
    `record #${tampered.records[1].index}: its contents changed after it was written`,
  )
  await expect(panel.locator('tr.is-broken')).toHaveCount(1)
})

// ── #109 Policy packs ───────────────────────────────────────────────────────

test('policy packs install, reject bad JSON, and remove only after confirmation', async ({ page }) => {
  let pack = null // the tenant pack the mock server holds
  const builtin = defaults()['GET /security/policy']
  const requests = await mockServer(page, {
    'GET /security/policy': () => (pack ? { tenant: '', origin: 'tenant:<default>', policy: pack } : builtin),
    'PUT /security/policy': (request) => {
      pack = request.postDataJSON()
      return { tenant: '', origin: 'tenant:<default>', policy: pack }
    },
    'DELETE /security/policy': () => {
      const deleted = pack !== null
      pack = null
      return { tenant: '', deleted }
    },
  })
  await openMonitor(page)
  const panel = page.getByRole('region', { name: 'Policy packs' })
  await expect(panel).toContainText('Applies to the default tenant only')
  await expect(panel.getByRole('button', { name: 'Remove pack' })).toBeDisabled()

  const json = panel.getByLabel('Policy pack JSON')
  await json.fill('{ not json')
  await panel.getByRole('button', { name: 'Install pack' }).click()
  await expect(panel.locator('p.gov-notice[role="status"]')).toContainText('Not valid JSON')
  expect(requests.filter((r) => r.method === 'PUT')).toHaveLength(0)

  await json.fill('{"id":"demo-pack","version":"2.0.0"}')
  await panel.getByRole('button', { name: 'Install pack' }).click()
  await expect(panel.locator('p.gov-notice[role="status"]')).toContainText('Installed demo-pack@2.0.0')
  await expect(panel).toContainText('Tenant pack · tenant:<default>')
  expect(JSON.parse(requests.find((r) => r.method === 'PUT').body)).toEqual({ id: 'demo-pack', version: '2.0.0' })

  await panel.getByRole('button', { name: 'Remove pack' }).click()
  const dialog = page.getByRole('dialog', { name: 'Remove this tenant’s policy pack?' })
  await expect(dialog).toBeVisible()
  await expect(dialog.getByRole('button', { name: 'Cancel' })).toBeFocused()
  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  expect(requests.filter((r) => r.method === 'DELETE')).toHaveLength(0)

  await panel.getByRole('button', { name: 'Remove pack' }).click()
  await dialog.getByRole('button', { name: 'Remove pack' }).click()
  await expect(panel.locator('p.gov-notice[role="status"]')).toContainText('Pack removed')
  expect(requests.filter((r) => r.method === 'DELETE')).toHaveLength(1)
  await expect(panel).toContainText('Global policy · builtin')
})

test('a policy that fails to load is shown as fail-closed', async ({ page }) => {
  await mockServer(page, {
    'GET /security/policy': { http: 500, json: { error: 'policy parse failed: rules[0]' } },
  })
  await openMonitor(page)
  const panel = page.getByRole('region', { name: 'Policy packs' })
  await expect(panel.getByRole('alert')).toContainText('denies every execution')
  await expect(panel.getByRole('alert')).toContainText('policy parse failed: rules[0]')
})

// ── #110 DP ledger ──────────────────────────────────────────────────────────

test('DP ledger reset needs confirmation and shows the re-read value', async ({ page }) => {
  let spent = 2.5
  const requests = await mockServer(page, {
    'GET /dp/budget': () => ({ ...defaults()['GET /dp/budget'], spent_epsilon: spent, remaining_epsilon: 10 - spent }),
    'POST /dp/budget/reset': () => {
      spent = 0
      return { ...defaults()['GET /dp/budget'], spent_epsilon: 0, remaining_epsilon: 10 }
    },
  })
  await openMonitor(page)
  const panel = page.getByRole('region', { name: 'Differential-privacy ledger' })
  await expect(panel.getByRole('img')).toHaveAccessibleName('2.5 of 10 epsilon spent by this tenant.')
  await expect(panel).toContainText('No rolling window')
  await expect(panel).toContainText('Not available in this version (#123)')

  await panel.getByRole('button', { name: 'Reset budget' }).click()
  const dialog = page.getByRole('dialog', { name: 'Reset this tenant’s privacy budget?' })
  await dialog.getByRole('button', { name: 'Cancel' }).click()
  expect(requests.filter((r) => r.path === '/dp/budget/reset')).toHaveLength(0)

  await panel.getByRole('button', { name: 'Reset budget' }).click()
  await dialog.getByRole('button', { name: 'Reset budget' }).click()
  await expect(panel.locator('p.gov-notice[role="status"]')).toContainText('spent ε now 0')
  await expect(panel.getByRole('img')).toHaveAccessibleName('0 of 10 epsilon spent by this tenant.')
})

// ── Server access: headers sent, token never stored ────────────────────────

test('server access headers reach every request and the token is never stored', async ({ page }) => {
  const requests = await mockServer(page)
  await openMonitor(page)
  await page.getByText('Server access').click()
  await page.getByLabel('Tenant (X-Xazz-Tenant)').fill('tenant-a')
  await page.getByLabel('Bearer token').fill('s3cret-token')
  await page.getByLabel('Admin actor (X-Xazz-Actor)').fill('alice')
  const before = requests.length
  await page.getByRole('button', { name: 'Apply and reload panels' }).click()
  await expect.poll(() => requests.length).toBeGreaterThan(before + 3)
  for (const request of requests.slice(before)) {
    expect(request.headers['x-xazz-tenant']).toBe('tenant-a')
    expect(request.headers.authorization).toBe('Bearer s3cret-token')
    expect(request.headers['x-xazz-actor']).toBe('alice')
  }
  const stored = await page.evaluate(() => JSON.stringify({ ...localStorage }) + JSON.stringify({ ...sessionStorage }))
  expect(stored).not.toContain('s3cret-token')
})

// ── #111 Error boundary ─────────────────────────────────────────────────────

test('a crashing panel is contained and a corrupt saved DAG can be discarded', async ({ page }) => {
  await mockServer(page)
  await page.addInitScript(() => {
    if (sessionStorage.getItem('seeded')) return
    sessionStorage.setItem('seeded', '1')
    // An object label cannot render as a React child — a realistic corrupt save.
    localStorage.setItem(
      'xazz_dag',
      JSON.stringify({ nodes: [{ id: 'a', type: 'filter', position: { x: 0, y: 0 }, data: { label: { bad: true } } }], edges: [] }),
    )
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()
  const fallback = page.getByRole('alert').filter({ hasText: 'Canvas stopped working' })
  await expect(fallback).toBeVisible()
  await expect(page.getByRole('button', { name: 'Full Run' })).toBeEnabled()
  await page.getByRole('tab', { name: 'Receipt' }).click()
  await expect(page.getByText('No full-run receipt yet')).toBeVisible()

  await fallback.getByRole('button', { name: 'Discard saved DAG' }).click()
  await expect(page.locator('.dag-editor')).toBeVisible()
  expect(await page.evaluate(() => localStorage.getItem('xazz_dag'))).toBeNull()
})

// ── #113 Run progress ───────────────────────────────────────────────────────

test('a pending run shows browser-measured progress and can stop waiting', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': () => new Promise(() => {}), // never answers
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  const overlay = page.locator('.run-overlay')
  await expect(overlay).toContainText('measured in this browser')
  await expect(overlay).toContainText('Executing on xazz-server')
  await expect(overlay).toContainText('Epoch progress · Not available in this version')
  await expect(overlay).toContainText('not recorded in History')
  await expect(page.getByLabel('Waiting for rows from xazz-server').first()).toBeVisible()
  await expect(overlay).toContainText(/Elapsed [1-9]\d*s/, { timeout: 4000 })

  await overlay.getByRole('button', { name: 'Stop waiting' }).click()
  await expect(overlay).toBeHidden()
  await page.getByRole('tab', { name: 'Preview' }).click()
  await expect(page.getByText('Stopped waiting for xazz-server').first()).toBeVisible()
  await page.getByRole('button', { name: 'Run again' }).click()
  await expect(page.getByRole('dialog', { name: /Review what will execute/ })).toBeVisible()
})

// ── #114 Schema inference ───────────────────────────────────────────────────

async function openFileInputNode(page) {
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()
  await page.locator('.react-flow__node').filter({ hasText: 'File Input' }).first().click()
}

const csv = (name, buffer) => ({ name, mimeType: 'text/csv', buffer })

test('a CSV goes to POST /schema and fills the node from the server answer', async ({ page }) => {
  const requests = await mockServer(page, {
    'POST /schema': {
      schema: [
        { name: 'district', type: 'string' },
        { name: 'pm25', type: 'float' },
      ],
      filePath: 'uploads/abc_air.csv',
    },
  })
  await openFileInputNode(page)
  await page.locator('.dag-params input[type=file]').setInputFiles(csv('air.csv', Buffer.from('district,pm25\nMapo,21\n')))
  await expect(page.getByRole('status').filter({ hasText: 'inferred by xazz-server' })).toBeVisible()
  await expect(page.locator('.dag-schema-tags')).toContainText('pm25float')
  await expect(page.locator('.dag-code')).toContainText('uploads/abc_air.csv')
  expect(requests.find((r) => r.path === '/schema').headers['content-type']).toContain('multipart/form-data')
})

test('a rejected upload is reported and an offline server falls back to EUC-KR detection', async ({ page }) => {
  await mockServer(page, { 'POST /schema': { http: 413, text: '파일이 너무 큽니다. 최대 50 MB 까지 허용됩니다.' } })
  await openFileInputNode(page)
  const input = page.locator('.dag-params input[type=file]')
  await input.setInputFiles(csv('big.csv', Buffer.from('a,b\n1,2\n')))
  await expect(page.getByRole('status').filter({ hasText: 'xazz-server rejected the file' })).toContainText('50 MB')

  await page.unrouteAll({ behavior: 'ignoreErrors' })
  await mockServer(page, { 'POST /schema': 'abort' })
  // "구,미세먼지\n강남구,31" encoded as EUC-KR.
  const eucKr = Buffer.from('b1b82cb9ccbcbcb8d5c1f60ab0adb3b2b1b82c33310a', 'hex')
  await input.setInputFiles(csv('kr.csv', eucKr))
  await expect(page.getByRole('status').filter({ hasText: 'detected in this browser' })).toBeVisible()
  await expect(page.locator('.dag-schema-tags')).toContainText('미세먼지int')
})

// ── #115 Language ───────────────────────────────────────────────────────────

test('the DAG editor has no Korean in English and follows the toggle to Korean', async ({ page }) => {
  await mockServer(page)
  await openFileInputNode(page)
  const hangul = /[가-힣]/
  const editorText = () =>
    page.locator('.dag-palette, .dag-side__section--params, .dag-side__actions').allInnerTexts()
  expect((await editorText()).join(' ')).not.toMatch(hangul)
  const titles = await page.locator('.dag-palette__tool').evaluateAll((els) => els.map((el) => el.title))
  expect(titles.join(' ')).not.toMatch(hangul)

  await page.getByRole('button', { name: '한국어' }).click()
  await expect(page.locator('.dag-side__section--params')).toContainText('파일 경로')
  await page.locator('.react-flow__node').filter({ hasText: 'Filter' }).first().click()
  await expect(page.locator('.dag-side__section--params')).toContainText('연산자')
  await expect(page.getByRole('button', { name: '삭제' })).toBeVisible()
})

test('the mobile note renders only on a narrow viewport', async ({ page }) => {
  await mockServer(page)
  await page.goto('/?screen=workspace')
  await expect(page.locator('.workspace-mobile-note')).toHaveCount(0)
  await page.setViewportSize({ width: 800, height: 900 })
  await expect(page.getByRole('heading', { name: 'Compiler Canvas is a desktop tool.' })).toBeVisible()
  // The toggle lives in the hidden topbar on mobile, so switch through the URL.
  await page.goto('/?screen=workspace&lang=ko')
  await expect(page.getByRole('heading', { name: '컴파일러 캔버스는 데스크톱 도구입니다.' })).toBeVisible()
})

// ── #116 Lineage ────────────────────────────────────────────────────────────

test('column lineage traces an aggregate back to its source column', async ({ page }) => {
  const requests = await mockServer(page, {
    'POST /catalog': {
      catalog: {
        pipelines: [
          {
            id: 0,
            name: 'dp_summary',
            input_columns: ['district', 'pm25'],
            output_columns: ['district', 'pm25', 'label'],
            lineage: [
              { column: 'district', from: ['district'], created_by_step: 0 },
              { column: 'pm25', from: ['pm25'], created_by_step: 2 },
              { column: 'label', created_by_step: 3 },
            ],
          },
        ],
      },
    },
  })
  await page.goto('/?screen=workspace')
  const panel = page.getByRole('region', { name: 'Column lineage' })
  await panel.getByRole('button', { name: 'Trace columns' }).click()
  await expect(panel.getByRole('row', { name: /pm25 pm25 2/ })).toBeVisible()
  await expect(panel.getByRole('row', { name: /label derived · no source column 3/ })).toBeVisible()
  expect(JSON.parse(requests.find((r) => r.path === '/catalog').body).code).toContain('load(')

  await panel.getByRole('button', { name: 'Diagram' }).click()
  await panel.getByRole('button', { name: 'pm25', exact: true }).click()
  await expect(panel.locator('code.is-source')).toHaveText('pm25')
})

test('a lineage compile error is shown verbatim', async ({ page }) => {
  await mockServer(page, { 'POST /catalog': { http: 422, text: 'compile error: unknown column foo' } })
  await page.goto('/?screen=workspace')
  const panel = page.getByRole('region', { name: 'Column lineage' })
  await panel.getByRole('button', { name: 'Trace columns' }).click()
  await expect(panel).toContainText('xazz-server answered 422')
  await expect(panel).toContainText('compile error: unknown column foo')
})

// ── Review follow-ups: states that returned no pipeline evidence ────────────

async function startFullRun(page) {
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
}

test('a server that refuses the run is shown as its answer, not as offline', async ({ page }) => {
  await mockServer(page, { 'POST /execute': { http: 401, text: 'invalid tenant token' } })
  await page.goto('/?screen=workspace')
  await startFullRun(page)
  await page.getByRole('tab', { name: 'Preview' }).click()
  await expect(page.getByText('xazz-server answered 401').first()).toBeVisible()
  await expect(page.getByText('invalid tenant token').first()).toBeVisible()
  await expect(page.getByLabel('Location: xazz-server connected')).toBeVisible()
  await expect(page.getByLabel('Process: Not started')).toBeVisible()
  await expect(page.locator('.flow-node--failed')).toHaveCount(0)
})

test('a dropped request marks termination unknown and no failed step', async ({ page }) => {
  await mockServer(page, { 'POST /execute': () => new Promise(() => {}) })
  await page.goto('/?screen=workspace')
  await startFullRun(page)
  await page.getByRole('button', { name: 'Stop waiting' }).click()
  await expect(page.getByLabel('Process: Termination unknown').first()).toBeVisible()
  await expect(page.locator('.flow-node--failed')).toHaveCount(0)
  await expect(page.locator('.code-pane li.has-error')).toHaveCount(0)
})

test('removing the blocking pack clears the stale verdict and unlocks Full Run', async ({ page }) => {
  let pack = { id: 'strict', version: '1' }
  const blocked = {
    safe_to_execute: false,
    policy_origin: 'tenant:<default>',
    policy: {
      policy_id: 'strict',
      policy_version: '1',
      domain: 'finance',
      risk_level: 'high',
      safe_to_execute: false,
      scanned_statements: 1,
      violations: [{ rule_id: 'XZP001', rule_name: 'DIRECT_IDENTIFIER_EXPOSED', severity: 'block', message: 'phone', columns: ['phone'] }],
      warnings: [],
    },
  }
  await mockServer(page, {
    'GET /security/policy': () => ({ tenant: '', origin: pack ? 'tenant:<default>' : 'builtin', policy: pack ?? { id: 'builtin', version: '1' } }),
    'POST /security/policy/check': blocked,
    'DELETE /security/policy': () => {
      pack = null
      return { tenant: '', deleted: true }
    },
  })
  await openMonitor(page)
  await page.getByRole('button', { name: 'Check policy' }).click()
  await expect(page.getByRole('button', { name: 'Full Run' })).toBeDisabled()
  const panel = page.getByRole('region', { name: 'Policy packs' })
  await panel.getByRole('button', { name: 'Remove pack' }).click()
  await page.getByRole('dialog').getByRole('button', { name: 'Remove pack' }).click()
  await expect(panel.locator('p.gov-notice[role="status"]')).toContainText('Pack removed')
  await expect(page.getByRole('button', { name: 'Full Run' })).toBeEnabled()
})

test('an empty retention field cannot silently save "keep forever"', async ({ page }) => {
  const requests = await mockServer(page)
  await openMonitor(page)
  const save = page.getByRole('button', { name: 'Save retention' })
  await expect(save).toBeDisabled()
  await page.getByPlaceholder('seconds · 0 keeps forever').fill('3600')
  await expect(save).toBeEnabled()
  await page.getByPlaceholder('seconds · 0 keeps forever').fill('')
  await expect(save).toBeDisabled()
  expect(requests.filter((r) => r.method === 'PUT')).toHaveLength(0)
})

test('without Web Crypto the server verdict still shows and nothing throws', async ({ page }) => {
  const errors = []
  page.on('pageerror', (error) => errors.push(error.message))
  await page.addInitScript(() => Object.defineProperty(window.crypto, 'subtle', { value: undefined }))
  await mockServer(page, { 'GET /security/audit/chain': { intact: false, records: auditFixture.records.length } })
  await openMonitor(page)
  const panel = page.getByRole('region', { name: 'Audit hash chain' })
  await expect(panel.getByLabel('Integrity: Mismatch')).toBeVisible()
  await expect(panel.getByRole('alert')).toContainText('needs HTTPS or localhost')
  expect(errors).toEqual([])
})

test('the ledger re-reads the server when its window rolls over', async ({ page }) => {
  let reads = 0
  await mockServer(page, {
    'GET /dp/budget': () => {
      reads += 1
      const now = Math.floor(Date.now() / 1000)
      return { ...defaults()['GET /dp/budget'], window_secs: 60, window_source: 'tenant', resets_at: reads === 1 ? now + 2 : now + 60 }
    },
  })
  await openMonitor(page)
  await expect(page.getByRole('region', { name: 'Differential-privacy ledger' })).toContainText('60s · tenant')
  await expect.poll(() => reads, { timeout: 6000 }).toBeGreaterThanOrEqual(2)
})

test('a server clock behind the browser does not make the ledger re-read in a loop', async ({ page }) => {
  let reads = 0
  const stuck = Math.floor(Date.now() / 1000) - 5 // server has not rolled yet
  await mockServer(page, {
    'GET /dp/budget': () => {
      reads += 1
      return { ...defaults()['GET /dp/budget'], window_secs: 60, window_source: 'tenant', resets_at: stuck }
    },
  })
  await openMonitor(page)
  await page.waitForTimeout(2500)
  expect(reads).toBeLessThanOrEqual(2)
})

test('a refused policy check reports the server answer, not an offline server', async ({ page }) => {
  await mockServer(page, { 'POST /security/policy/check': { http: 401, text: 'invalid tenant token' } })
  await openMonitor(page)
  await page.getByRole('button', { name: 'Check policy' }).click()
  await expect(page.locator('.live-message')).toContainText('xazz-server answered 401: invalid tenant token')
})
