import { expect, test } from '@playwright/test'
import { resultRows } from '../src/data.js'

function observeRuntime(page) {
  const consoleErrors = []
  const externalRequests = []

  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text())
  })
  page.on('pageerror', (error) => consoleErrors.push(error.message))
  page.on('request', (request) => {
    const url = new URL(request.url())
    if (!['127.0.0.1', 'localhost'].includes(url.hostname)) {
      externalRequests.push(request.url())
    }
  })

  return () => {
    expect(consoleErrors, `console errors: ${consoleErrors.join('\n')}`).toEqual([])
    expect(externalRequests, `external requests: ${externalRequests.join('\n')}`).toEqual(
      [],
    )
  }
}

async function tabTo(page, target, maximumTabs = 40) {
  for (let index = 0; index < maximumTabs; index += 1) {
    await page.keyboard.press('Tab')
    if (await target.evaluate((node) => node === document.activeElement)) return
  }
  throw new Error(`Target was not reachable within ${maximumTabs} Tab presses`)
}

test('landing communicates outcome and preserves the sample-first route', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/')

  await expect(
    page.getByRole('heading', { name: 'Catch data errors before training starts.' }),
  ).toBeVisible()
  await expect(page.getByLabel('Maturity: Available').first()).toBeVisible()
  await expect(page.getByText('Future Labs')).toBeVisible()
  await expect(page.getByText('Computed', { exact: true })).toBeVisible()
  await expect(page.getByText(/Future contract/).first()).toBeVisible()
  const proof = page.getByRole('table', { name: 'Synthetic result preview' })
  await expect(proof.getByRole('row')).toHaveCount(4)
  for (const [index, fixtureRow] of resultRows.slice(0, 3).entries()) {
    await expect(proof.getByRole('row').nth(index + 1)).toContainText(
      fixtureRow.observed_at,
    )
    await expect(proof.getByRole('row').nth(index + 1)).toContainText(
      fixtureRow.district,
    )
    await expect(proof.getByRole('row').nth(index + 1)).toContainText(
      fixtureRow.pm25.toFixed(1),
    )
  }

  await page
    .getByRole('button', { name: 'Open a sample pipeline' })
    .first()
    .click()
  await expect(page).toHaveURL(/\?screen=start/)
  await expect(
    page.getByRole('heading', { name: 'Begin with something you can inspect.' }),
  ).toBeVisible()

  await page.getByRole('button', { name: /Run the air-quality sample/ }).click()
  await expect(page).toHaveURL(/\?screen=workspace/)
  await expect(page.getByText('Compiler Canvas').first()).toBeVisible()
  await expect(page.getByText('100 synthetic rows')).toBeVisible()
  assertRuntime()
})

test('project start supports a truthful Korean validation state', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=start')

  await page.getByRole('button', { name: '한국어' }).click()
  await expect(page).toHaveURL(/\?screen=start&lang=ko/)
  await expect(
    page.getByRole('heading', {
      name: '직접 확인할 수 있는 것부터 시작하세요.',
    }),
  ).toBeVisible()
  await expect(page.getByText('프로토타입 미지원').first()).toBeVisible()

  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow).toBeLessThanOrEqual(0)
  assertRuntime()
})

test('keyboard path reaches a receipt only after explicit preflight review', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/')

  const openSample = page
    .getByRole('button', { name: 'Open a sample pipeline' })
    .first()
  await tabTo(page, openSample)
  await expect(openSample).toBeFocused()
  const outline = await openSample.evaluate(
    (node) => getComputedStyle(node).outlineStyle,
  )
  expect(outline).not.toBe('none')
  await page.keyboard.press('Enter')

  const sampleChoice = page.getByRole('button', {
    name: /Run the air-quality sample/,
  })
  await tabTo(page, sampleChoice)
  await page.keyboard.press('Enter')

  const fullRun = page.getByRole('button', { name: 'Full Run' })
  await tabTo(page, fullRun)
  await page.keyboard.press('Enter')

  const dialog = page.getByRole('dialog', { name: /Review what will execute locally/ })
  await expect(dialog).toBeVisible()
  const startRun = dialog.getByRole('button', { name: 'Start full run' })
  await expect(startRun).toBeDisabled()

  const review = dialog.getByRole('checkbox')
  const close = dialog.getByRole('button', { name: 'Close preflight' })
  const back = dialog.getByRole('button', { name: 'Back to canvas' })
  await expect(page.locator('.workspace-topbar')).toHaveAttribute('inert', '')
  await expect(page.locator('.workspace-shell')).toHaveAttribute('inert', '')
  await expect(dialog.locator('.preflight-warning__check')).toBeVisible()
  await expect(review).toBeFocused()
  await expect(dialog.getByLabel('Pipeline: Not evaluated')).toBeVisible()
  await expect(dialog.getByLabel('Control: Not configured')).toBeVisible()
  await expect(dialog.getByLabel('Run confirmation: Required')).toBeVisible()

  await page.keyboard.press('Shift+Tab')
  await expect(close).toBeFocused()
  await page.keyboard.press('Shift+Tab')
  await expect(back).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(close).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(review).toBeFocused()

  await page.keyboard.press('Escape')
  await expect(dialog).toBeHidden()
  await expect(fullRun).toBeFocused()
  await page.keyboard.press('Enter')
  await expect(review).toBeFocused()
  await page.keyboard.press('Space')
  await expect(review).toBeChecked()
  await expect(dialog.getByLabel('Run confirmation: Confirmed')).toBeVisible()
  await page.keyboard.press('Tab')
  await expect(back).toBeFocused()
  await page.keyboard.press('Tab')
  await expect(startRun).toBeFocused()
  await page.keyboard.press('Enter')

  await expect(page.getByText('Waiting for xazz-exec to return evidence')).toBeVisible()
  const complete = page.getByRole('button', { name: 'Show success' })
  await tabTo(page, complete)
  await page.keyboard.press('Enter')

  await expect(page.getByText('Pipeline evidence is complete')).toBeVisible()
  const receipt = page.getByRole('region', { name: 'Pipeline results' })
  await expect(receipt.getByLabel('Process: Exited')).toBeVisible()
  await expect(receipt.getByLabel('Pipeline: Succeeded')).toBeVisible()
  await expect(receipt.getByLabel('Control: Not configured')).toBeVisible()
  await expect(receipt.getByLabel('Integrity: Computed')).toBeVisible()
  await expect(receipt.getByLabel('Artifact: Not requested')).toBeVisible()
  await expect(page.getByText(/SHA-256 · computed · not persisted/)).toBeVisible()
  await expect(page.getByText('Fixture ID')).toBeVisible()
  await expect(receipt.getByText('Run ID', { exact: true })).toBeVisible()
  await expect(receipt.getByText('Engine version', { exact: true })).toBeVisible()
  await expect(
    receipt.getByText('Not available in browser prototype').first(),
  ).toBeVisible()
  await expect(receipt.getByText('Warnings', { exact: true })).toBeVisible()
  await expect(receipt.getByText('Capability maturity', { exact: true })).toBeVisible()
  assertRuntime()
})

test('pre-run logs and receipt never invent execution evidence', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  const results = page.getByRole('region', { name: 'Pipeline results' })
  await results.getByRole('tab', { name: 'Logs' }).click()
  await expect(results.getByText('Not started', { exact: true })).toBeVisible()
  await expect(results.getByText('Not evaluated', { exact: true })).toBeVisible()
  await expect(results.getByText('Exited with code 0')).toHaveCount(0)

  await results.getByRole('tab', { name: 'Receipt' }).click()
  await expect(results.getByText('No full-run receipt yet')).toBeVisible()
  await expect(results.getByLabel('Process: Not started')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Not evaluated')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Succeeded')).toHaveCount(0)
  await expect(results.getByLabel('Integrity: Computed')).toHaveCount(0)
  assertRuntime()
})

test('graph selection highlights code and exposes measured impact', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  await page.getByRole('button', { name: /Fill null/ }).click()
  await expect(page.locator('.code-pane li.is-selected')).toContainText('fillNull')
  await expect(page.locator('.flow-node--relation-upstream')).toHaveCount(2)
  await expect(page.locator('.flow-node--relation-downstream')).toHaveCount(2)
  await expect(page.locator('.operation-list button.is-upstream')).toHaveCount(2)
  await expect(page.locator('.operation-list button.is-downstream')).toHaveCount(2)
  await expect(page.getByText(`−6`, { exact: true }).first()).toBeVisible()
  await expect(page.getByText('Not emitted by current runtime').first()).toBeVisible()

  await page.getByRole('button', { name: /Live Check/ }).click()
  await expect(page.getByRole('status')).toContainText('6 nulls found')
  await page.getByRole('button', { name: /Check schema/ }).click()
  await expect(page.locator('.code-pane li.is-selected')).toContainText('pm25: Float?')
  assertRuntime()
})

test('runtime error keeps process exit separate from a partial pipeline verdict', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&state=error')

  await expect(
    page.getByRole('heading', {
      name: 'Fill null failed, even though the process exited 0.',
    }),
  ).toBeVisible()
  const results = page.getByRole('region', { name: 'Pipeline results' })
  await expect(results.getByLabel('Process: Exited')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Partial')).toBeVisible()
  await expect(page.getByLabel('Pipeline: Succeeded')).toHaveCount(0)
  await expect(page.getByLabel('Pipeline: Failed')).toHaveCount(0)
  await expect(page.getByText('2 downstream nodes')).toBeVisible()
  await expect(page.getByText('Not available in failed run').first()).toBeVisible()
  await expect(page.locator('.result-dock__scope')).toContainText(
    'Last Live Check · stale · not current run',
  )

  await page.getByRole('button', { name: 'Apply as draft' }).click()
  await expect(page.getByText('cast(pm25, Float)', { exact: true })).toBeVisible()
  await expect(page.getByText(/Nothing has been applied/)).toBeVisible()
  await expect(page.getByRole('button', { name: /Retry from here/ })).toBeDisabled()
  await expect(
    page.getByRole('button', { name: /Restore last success/ }),
  ).toBeDisabled()
  await results.getByRole('tab', { name: 'Preview' }).click()
  await expect(page.getByRole('note')).toContainText(
    'stale · not current Full Run evidence',
  )
  assertRuntime()
})

test('all required workspace states are directly reviewable', async ({ page }) => {
  const expectations = {
    ready: 'Compiler Canvas',
    preflight: 'Review what will execute locally.',
    running: 'Waiting for xazz-exec to return evidence',
    success: 'Pipeline evidence is complete',
    error: 'Fill null failed, even though the process exited 0.',
  }

  for (const [state, text] of Object.entries(expectations)) {
    await page.goto(`/?screen=workspace&state=${state}`)
    await expect(page.getByText(text, { exact: false }).first()).toBeVisible()
  }
})

test('390px landing has no horizontal overflow and keeps primary proof', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.setViewportSize({ width: 390, height: 844 })
  await page.goto('/')

  await expect(
    page.getByRole('heading', { name: 'Catch data errors before training starts.' }),
  ).toBeVisible()
  await expect(page.getByText('Sample pipeline proof')).toHaveCount(0)
  await expect(page.locator('.landing-pipeline')).toBeVisible()
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - window.innerWidth,
  )
  expect(overflow).toBeLessThanOrEqual(0)
  const clippedElements = await page.locator('.landing-page').evaluate((root) =>
    Array.from(root.querySelectorAll('*'))
      .filter((node) => {
        const style = getComputedStyle(node)
        const rect = node.getBoundingClientRect()
        return (
          style.display !== 'none' &&
          style.visibility !== 'hidden' &&
          rect.width > 0 &&
          rect.height > 0 &&
          (rect.left < -0.5 || rect.right > window.innerWidth + 0.5)
        )
      })
      .map((node) => {
        const rect = node.getBoundingClientRect()
        return {
          tag: node.tagName,
          className: node.className,
          left: rect.left,
          right: rect.right,
        }
      }),
  )
  expect(clippedElements).toEqual([])
  await expect(
    page.getByRole('button', { name: 'Open a sample pipeline' }).first(),
  ).toBeVisible()
  assertRuntime()
})
