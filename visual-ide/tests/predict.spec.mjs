// D3 — predict-path embedding diagnostics surfaced in the Monitor view.
//
// The diagnostics are persisted on the run and served by GET /runs/{id}/prediction,
// so the Monitor fetches them per recorded run (like resource telemetry). The
// /execute response also carries `prediction` for the run it just returned and is
// kept as a fallback. The panel must show measured facts, an honest empty state when
// a run recorded none, and "couldn't load" when the fetch itself fails.
import { expect, test } from '@playwright/test'
import { mockServer } from './mockServer.mjs'

const prediction = {
  type: 'predict_stmt',
  success: true,
  model_name: 'AirPredictor',
  report: { embedding_out_of_range: 3, embedding_non_integer: 1 },
}

async function fullRun(page) {
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('7')
  await page.getByRole('button', { name: 'Monitor' }).click()
}

test('monitor shows predict diagnostics from GET /runs/{id}/prediction', async ({ page }) => {
  await mockServer(page, {
    // The /execute response omits `prediction`: the panel must come from the
    // persisted endpoint, proving the wiring rather than the response echo.
    'POST /execute': {
      success: true,
      rows: [{ pm25_pred: 24.8 }],
      schema: [
        { name: 'pm25', type: 'f64' },
        { name: 'pm25_pred', type: 'f64' },
      ],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/prediction': {
      run_id: 7,
      tenant: '',
      available: true,
      prediction,
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Predict embedding input' })
  await expect(panel.getByLabel('Maturity: Real')).toBeVisible()
  await expect(panel.getByText('AirPredictor', { exact: true })).toBeVisible()
  await expect(
    panel.getByText('Out-of-range indices (clamped)').locator('..'),
  ).toContainText('3')
  await expect(
    panel.getByText('Non-integer inputs (truncated)').locator('..'),
  ).toContainText('1')
})

test('monitor falls back to the /execute prediction when the endpoint fails', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25_pred: 24.8 }],
      schema: [{ name: 'pm25_pred', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
      prediction,
    },
    // A run that could not be recorded (store failure) returns run_id 0 / a 404;
    // diagnostics already in the response must still be shown, not hidden.
    'GET /runs/7/prediction': { http: 404, text: 'run 7 not found' },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Predict embedding input' })
  await expect(panel.getByLabel('Maturity: Real')).toBeVisible()
  await expect(panel.getByText('AirPredictor', { exact: true })).toBeVisible()
})

test('monitor stays an honest empty predict panel when a run recorded none', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/prediction': {
      run_id: 7,
      tenant: '',
      available: false,
      reason: 'no predict diagnostics were recorded for this run',
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Predict embedding input' })
  await expect(panel.getByLabel('Maturity: Beta')).toBeVisible()
  await expect(
    panel.getByText('no predict diagnostics were recorded for this run'),
  ).toBeVisible()
})

test('monitor says "couldn\'t load" when the predict fetch fails', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/prediction': { http: 404, text: 'run 7 not found' },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Predict embedding input' })
  await expect(panel.getByLabel('Maturity: Unavailable')).toBeVisible()
  await expect(panel.getByText('input diagnostics · couldn\'t load')).toBeVisible()
  await expect(
    panel.getByText('xazz-server answered HTTP 404 · run 7 not found'),
  ).toBeVisible()
})
