// #174 — result charts drawn only from the rows a Full Run returned.
import { expect, test } from '@playwright/test'
import { fiveNumber, histogram } from '../src/chartStats.js'
import { mockServer } from './mockServer.mjs'

const districts = ['Mapo', 'Jongno', 'Gangnam', 'Songpa', 'Yongsan']
const rows = Array.from({ length: 40 }, (_, i) => ({
  district: districts[i % districts.length],
  pm25: i === 7 ? null : i === 13 ? 95 : 20 + ((i * 7) % 17),
}))
const pm25 = rows.map((row) => row.pm25).filter((value) => value !== null)

async function runAndOpenCharts(page, result = { rows, extraSchema: [] }) {
  await mockServer(page, {
    'POST /execute': {
      success: true,
      rows: result.rows,
      schema: [
        { name: 'district', type: 'str' },
        { name: 'pm25', type: 'f64' },
        ...result.extraSchema,
      ],
      logs: [],
      stdout: '',
      run_id: 3,
    },
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Full Run' }).click()
  await page.getByRole('checkbox').check()
  await page.getByRole('button', { name: 'Start full run' }).click()
  await expect(page.locator('.receipt__rows')).toContainText('3')
  await page.getByRole('tab', { name: 'Chart' }).click()
}

test('mean by group averages only numeric values — a null is not a zero', async ({ page }) => {
  await runAndOpenCharts(page)
  // Gangnam holds the null (row 7); its mean is over the other seven values.
  const gangnam = rows.filter((row) => row.district === 'Gangnam' && row.pm25 !== null).map((row) => row.pm25)
  const expected = gangnam.reduce((a, b) => a + b, 0) / gangnam.length
  await expect(page.locator('.bar-chart__row', { hasText: 'Gangnam' }).locator('strong')).toHaveText(
    String(parseFloat(expected.toPrecision(4))),
  )
})

test('a column with no numeric value and negative means get honest states, not broken bars', async ({ page }) => {
  const withEmpty = rows.map((row, i) => ({ ...row, delta: i % 2 ? -3 : -1, empty: null }))
  await runAndOpenCharts(page, {
    rows: withEmpty,
    extraSchema: [
      { name: 'delta', type: 'f64' },
      { name: 'empty', type: 'f64' },
    ],
  })
  await page.getByLabel('Numeric column').selectOption('delta')
  await expect(page.getByText('Some group means are negative')).toBeVisible()
  await expect(page.locator('.bar-chart')).toHaveCount(0)
  await page.getByLabel('Numeric column').selectOption('empty')
  await page.getByRole('button', { name: 'Distribution' }).click()
  await expect(page.getByText('empty has no numeric value in the returned rows')).toBeVisible()
  await page.getByRole('button', { name: 'Box plot' }).click()
  await expect(page.getByText('empty has no numeric value in the returned rows')).toBeVisible()
})

test('the distribution view bins the returned values and says what it left out', async ({ page }) => {
  await runAndOpenCharts(page)
  await page.getByRole('button', { name: 'Distribution' }).click()
  const bins = histogram(pm25)
  await expect(page.locator('.histogram__bin')).toHaveCount(bins.length)
  await expect(page.getByText('39 values · 1 row(s) without a number left out')).toBeVisible()
  await expect(page.locator('.histogram__bin strong')).toHaveText(String(Math.max(...bins.map((bin) => bin.count))))
  await page.getByText('Table alternative').click()
  const counts = await page.locator('.chart-panel tbody td:nth-child(2)').allInnerTexts()
  expect(counts.map(Number).reduce((a, b) => a + b, 0)).toBe(pm25.length)
})

test('the box plot shows quartiles, whiskers and the outlier per group', async ({ page }) => {
  await runAndOpenCharts(page)
  await page.getByRole('button', { name: 'Box plot' }).click()
  await expect(page.getByText('pm25 · all rows')).toBeVisible()
  const all = fiveNumber(pm25)
  await expect(page.locator('.boxplot__outlier')).toHaveCount(all.outliers.length)
  await expect(page.locator('.boxplot__row strong')).toHaveText(String(all.median))

  await page.getByLabel('Group by').selectOption('district')
  await expect(page.locator('.boxplot__row')).toHaveCount(districts.length)
  const gangnam = fiveNumber(rows.filter((row) => row.district === 'Gangnam' && row.pm25 !== null).map((row) => row.pm25))
  await expect(page.locator('.boxplot__row', { hasText: 'Gangnam' })).toHaveAttribute(
    'title',
    new RegExp(`n ${gangnam.n} · min ${gangnam.min} · Q1 ${parseFloat(gangnam.q1.toPrecision(4))} · median ${gangnam.median}`),
  )
})

test('training curves stay an honest gap until the runtime emits them', async ({ page }) => {
  await runAndOpenCharts(page)
  await page.getByRole('button', { name: 'Training curves' }).click()
  await expect(page.getByText(/Not available in this version\. Loss curves need per-epoch history/)).toBeVisible()
  await expect(page.locator('.histogram, .boxplot, .bar-chart')).toHaveCount(0)
  await expect(page.getByLabel('View: Not available')).toBeVisible()
})

test('chart modes follow the language toggle', async ({ page }) => {
  await runAndOpenCharts(page)
  await page.getByRole('button', { name: '한국어' }).click()
  await page.getByRole('button', { name: '박스플롯' }).click()
  await expect(page.getByText('pm25 · 전체 행')).toBeVisible()
  await expect(page.getByText('그룹 기준')).toBeVisible()
})
