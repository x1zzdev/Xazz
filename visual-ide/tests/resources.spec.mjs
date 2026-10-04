// #128 follow-up — run resource telemetry surfaced in the Monitor view.
//
// GET /runs/{id}/resources reports the runner process tree for one run. The
// Monitor must show those numbers as measured facts, and stay an honest empty
// panel when the endpoint reports `available: false` or no run has resources.
import { expect, test } from '@playwright/test'
import { mockServer } from './mockServer.mjs'

async function fullRun(page) {
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('7')
  await page.getByRole('button', { name: 'Monitor' }).click()
}

test('monitor shows run resource telemetry as measured facts', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/resources': {
      run_id: 7,
      tenant: '',
      available: true,
      resources: {
        duration_ms: 1234.5,
        cpu_user_ms: 800,
        cpu_sys_ms: 200,
        max_rss_kb: 2048,
        source: 'runner-process-tree',
      },
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Resource efficiency' })
  await expect(panel.getByLabel('Maturity: Real')).toBeVisible()
  await expect(panel.locator('.monitor-facts div', { hasText: 'Wall clock' })).toContainText('1234.5 ms')
  await expect(panel.locator('.monitor-facts div', { hasText: 'CPU (user + sys)' })).toContainText('1000 ms')
  await expect(panel.locator('.monitor-facts div', { hasText: 'Peak RSS' })).toContainText('2 MB')
  await expect(panel.getByText('runner-process-tree', { exact: true })).toBeVisible()
})

test('monitor stays an honest empty resource panel when a run has none', async ({ page }) => {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: [{ pm25: 24.8 }],
      schema: [{ name: 'pm25', type: 'f64' }],
      logs: [],
      stdout: '',
      run_id: 7,
    },
    'GET /runs/7/resources': {
      run_id: 7,
      tenant: '',
      available: false,
      reason: 'no resource telemetry was recorded for this run',
    },
  })

  await page.goto('/?screen=workspace')
  await fullRun(page)

  const panel = page.getByRole('region', { name: 'Resource efficiency' })
  await expect(panel.getByLabel('Maturity: Beta')).toBeVisible()
  await expect(
    panel.getByText('no resource telemetry was recorded for this run'),
  ).toBeVisible()
})
