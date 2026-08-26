import { expect, test } from '@playwright/test'
import { pipeline, resultRows } from '../src/data.js'

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

test('keyboard path reaches the preflight dialog and authenticates the run gate', async ({
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

  const dialog = page.getByRole('dialog', { name: /Review what will execute on xazz-server/ })
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

  // Full Run submits to the real backend. Without a reachable xazz-server the UI must
  // report an honest connection failure rather than inventing a synthetic success.
  await page.keyboard.press('Enter')
  await expect(page.getByText(/xazz-server unreachable|Waiting for xazz-exec/).first()).toBeVisible({
    timeout: 10_000,
  })
  assertRuntime()
})

test('pre-run logs and receipt never invent execution evidence', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  const results = page.getByRole('region', { name: 'Pipeline results' })
  await results.getByRole('tab', { name: 'Logs' }).click()
  await expect(results.getByText('Not started', { exact: true })).toBeVisible()
  await expect(results.getByText('Not evaluated', { exact: true })).toBeVisible()
  await expect(results.getByText('Waiting for xazz-exec to return evidence')).toHaveCount(0)

  await results.getByRole('tab', { name: 'Receipt' }).click()
  await expect(results.getByText('No full-run receipt yet')).toBeVisible()
  await expect(results.getByLabel('Process: Not started')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Not evaluated')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Succeeded')).toHaveCount(0)
  assertRuntime()
})

test('graph selection highlights code and exposes measured impact', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  await page.getByRole('button', { name: /Fill null/ }).click()
  await expect(page.locator('.code-pane li.is-selected')).toContainText('fillNull')
  // Derived from the fixture so adding a stage cannot silently stale this assertion.
  const fillIndex = pipeline.findIndex((node) => node.id === 'fill')
  const upstreamCount = fillIndex
  const downstreamCount = pipeline.length - fillIndex - 1
  await expect(page.locator('.flow-node--relation-upstream')).toHaveCount(upstreamCount)
  await expect(page.locator('.flow-node--relation-downstream')).toHaveCount(
    downstreamCount,
  )
  await expect(page.locator('.operation-list button.is-upstream')).toHaveCount(
    upstreamCount,
  )
  await expect(page.locator('.operation-list button.is-downstream')).toHaveCount(
    downstreamCount,
  )
  await expect(page.getByText('Not emitted by current runtime').first()).toBeVisible()

  await page.getByRole('button', { name: /Live Check/ }).click()
  await expect(page.getByRole('status')).toContainText(/xazz-server/)
  await page.getByRole('button', { name: /Check schema/ }).click()
  await expect(page.locator('.code-pane li.is-selected')).toContainText('Option<float>')
  assertRuntime()
})

test('an errored run keeps process and pipeline verdicts separate', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&state=error')

  const results = page.getByRole('region', { name: 'Pipeline results' })
  await expect(results.getByLabel('Process: Exited / blocked')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Partial')).toBeVisible()
  await expect(page.getByLabel('Pipeline: Succeeded')).toHaveCount(0)
  // The scope marker never claims a fresh synthetic run when the last run failed.
  await expect(page.locator('.result-dock__scope')).toContainText(
    'Last Full Run · errored',
  )
  await results.getByRole('tab', { name: 'Preview' }).click()
  await expect(page.getByRole('note')).toContainText('Last Full Run errored')
  assertRuntime()
})

test('all required workspace states are directly reviewable', async ({ page }) => {
  const expectations = {
    ready: 'Compiler Canvas',
    preflight: 'Review what will execute on xazz-server.',
    running: 'Waiting for xazz-exec to return evidence',
    success: 'Pipeline evidence is complete',
    error: 'Last Full Run · errored',
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

test('ML compile band is visible in the graph before any run', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  // The ML half of the program must be legible without running anything.
  for (const label of ['Compile AirNet', 'Train model', 'Predict']) {
    await expect(page.getByRole('button', { name: new RegExp(label) })).toBeVisible()
  }
  const mlNodes = page.locator('.flow-node__band', { hasText: 'ML COMPILE' })
  await expect(mlNodes).toHaveCount(3)

  // Pre-run ML stages state configuration, never an outcome.
  await page.getByRole('button', { name: /Train model/ }).first().click()
  await expect(page.getByText('Not available in this version').first()).toBeVisible()

  assertRuntime()
})

test('monitor view separates a measured contract from a proposed one', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&state=success')

  await page.getByRole('button', { name: 'Monitor' }).click()

  const burn = page.getByLabel('Burn compile and training')
  const privacy = page.getByLabel('Differential privacy budget')
  const resource = page.getByLabel('Resource efficiency')
  await expect(burn).toBeVisible()
  await expect(privacy).toBeVisible()
  await expect(resource).toBeVisible()

  // The measured panel may show TrainReport fields.
  await expect(burn.getByText('209').first()).toBeVisible()
  await expect(burn.getByText('0.0417').first()).toBeVisible()

  // The unimplemented panels carry a permanent maturity badge and synthetic scope,
  // and must never present a number as measured.
  await expect(privacy.getByLabel('Maturity: Research')).toBeVisible()
  await expect(resource.getByLabel('Maturity: Planned')).toBeVisible()
  for (const panel of [privacy, resource]) {
    await expect(
      panel.getByText('Synthetic structure · not measured · proposed contract'),
    ).toBeVisible()
    await expect(panel.getByText('Not available in this version').first()).toBeVisible()
  }

  // A proposed bar is hollow: it has no filled background.
  const proposedBar = resource.locator('.monitor-bars__fill--proposed').first()
  const background = await proposedBar.evaluate(
    (node) => window.getComputedStyle(node).backgroundColor,
  )
  expect(background).toBe('rgba(0, 0, 0, 0)')

  assertRuntime()
})

test('monitor view tells the truth before a run and after a failed run', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)

  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Monitor' }).click()
  const burn = page.getByLabel('Burn compile and training')
  await expect(
    burn.getByText('No Full Run has produced a training report yet.'),
  ).toBeVisible()
  await expect(burn.locator('.monitor-bars__fill--train')).toHaveCount(0)

  await page.goto('/?screen=workspace&state=error')
  await page.getByRole('button', { name: 'Monitor' }).click()
  await expect(
    page
      .getByLabel('Burn compile and training')
      .getByText('The run failed before a training report was emitted.'),
  ).toBeVisible()

  assertRuntime()
})

test('switching to monitor keeps run context and never scrolls the page sideways', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&state=success')

  await page.getByRole('button', { name: 'Monitor' }).click()
  await expect(page.getByRole('button', { name: 'Monitor' })).toHaveAttribute(
    'aria-pressed',
    'true',
  )
  // The Monitor view is deliberately not written to the URL.
  await expect(page).toHaveURL(/state=success/)
  await expect(page).not.toHaveURL(/monitor/)

  for (const width of [1280, 1440]) {
    await page.setViewportSize({ width, height: 900 })
    const overflow = await page.evaluate(
      () => document.documentElement.scrollWidth - window.innerWidth,
    )
    expect(overflow, `horizontal overflow at ${width}px`).toBeLessThanOrEqual(0)
  }

  assertRuntime()
})
