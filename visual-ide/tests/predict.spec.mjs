// D3 — predict-path embedding diagnostics surfaced in the Monitor view.
//
// The server echoes the `[xazz:predict]` marker as `prediction` on the /execute
// response; the Monitor must show those counters as measured facts and stay an
// honest empty panel when no predict(...) statement ran.
import { expect, test } from '@playwright/test'
import { mockServer } from './mockServer.mjs'

const prediction = {
  type: 'predict_stmt',
  success: true,
  model_name: 'AirPredictor',
  report: { embedding_out_of_range: 3, embedding_non_integer: 1 },
}

test('monitor shows predict embedding diagnostics as measured facts', async ({ page }) => {
  await mockServer(page, {
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
      prediction,
    },
  })

  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('7')

  await page.getByRole('button', { name: 'Monitor' }).click()
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
