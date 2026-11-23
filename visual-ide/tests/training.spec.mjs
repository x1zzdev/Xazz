// D3 — training report surfaced in the Monitor view.
//
// The report is persisted on the run and served by GET /runs/{id}/training, so the
// Monitor fetches it per recorded run (like resource telemetry and predict
// diagnostics). The /execute response also carries `training` for the run it just
// returned and is kept as a fallback. The panel must show measured facts, an honest
// empty state when a run recorded none, and "couldn't load" when the fetch fails.
import { expect, test } from '@playwright/test'
import { mockServer } from './mockServer.mjs'

const training = {
  type: 'train_stmt',
  success: true,
  model_name: 'AirPredictor',
  report: {
    model_name: 'AirPredictor',
    target: 'pm25',
    input_dim: 3,
    output_dim: 1,
    num_params: 209,
    epochs: 40,
    batch_size: 16,
    learning_rate: 0.01,
    final_train_loss: 0.0417,
    final_val_loss: 0.0538,
    checkpoint_path: 'artifacts/airnet.mpk',
  },
}

async function fullRun(page) {
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('7')
  await page.getByRole('button', { name: 'Monitor' }).click()
}

test('monitor shows the training report from GET /runs/{id}/training', async ({ page }) => {
  await mockServer(page, {
    // The /execute response omits `training`: the panel must come from the
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
    'GET /runs/7/training': {
      run_id: 7,
      tenant: '',
      available: true,
      training,
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Burn compile and training' })
  await expect(panel.getByLabel('Maturity: Real')).toBeVisible()
  await expect(panel.getByText('AirPredictor', { exact: true })).toBeVisible()
  await expect(
    panel.locator('.monitor-bars__row', { hasText: 'Final training loss' }),
  ).toContainText('0.0417')
})

test('monitor falls back to the /execute training when the endpoint fails', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25_pred: 24.8 }],
      schema: [{ name: 'pm25_pred', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
      training,
    },
    // A run that could not be recorded (store failure) returns run_id 0 / a 404;
    // the report already in the response must still be shown, not hidden.
    'GET /runs/7/training': { http: 404, text: 'run 7 not found' },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Burn compile and training' })
  await expect(panel.getByLabel('Maturity: Real')).toBeVisible()
  await expect(panel.getByText('AirPredictor', { exact: true })).toBeVisible()
})

test('monitor stays an honest empty training panel when a run recorded none', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/training': {
      run_id: 7,
      tenant: '',
      available: false,
      reason: 'no training report was recorded for this run',
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Burn compile and training' })
  await expect(panel.getByLabel('Maturity: Beta')).toBeVisible()
  await expect(
    panel.getByText('no training report was recorded for this run'),
  ).toBeVisible()
})

test('monitor says "couldn\'t load" when the training fetch fails', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/training': { http: 404, text: 'run 7 not found' },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Burn compile and training' })
  await expect(panel.getByLabel('Maturity: Unavailable')).toBeVisible()
  await expect(panel.getByText('training report · couldn\'t load')).toBeVisible()
  await expect(
    panel.getByText('xazz-server answered HTTP 404 · run 7 not found'),
  ).toBeVisible()
})
