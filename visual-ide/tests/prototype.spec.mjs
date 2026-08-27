import { expect, test } from '@playwright/test'
import { pipeline, resultRows } from '../src/data.js'

function observeRuntime(page) {
  const consoleErrors = []
  const externalRequests = []

  // The workspace probes xazz-server on load and renders an explicit
  // "xazz-server offline" state when it is absent. The browser still logs the
  // failed request itself, which no try/catch can suppress, so an offline probe
  // is not a defect. Real script errors and any other failed resource still fail.
  const API_ORIGIN = 'http://127.0.0.1:8005'
  const isOfflineProbe = (message) => {
    if (!/ERR_CONNECTION_REFUSED|Failed to load resource/.test(message.text())) return false
    return (message.location()?.url || '').startsWith(API_ORIGIN)
  }

  page.on('console', (message) => {
    if (message.type() !== 'error') return
    if (isOfflineProbe(message)) return
    consoleErrors.push(`${message.text()} @ ${message.location()?.url || 'unknown'}`)
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
  // The canvas no longer claims a synthetic row count: evidence now comes from the
  // Full Run response, and the scope line says so.
  await expect(
    page.getByText('Structural pipeline canvas · evidence comes from the Full Run response'),
  ).toBeVisible()
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
  // An errored run opens on Logs, so the receipt has to be opened before its axes
  // can be read.
  await results.getByRole('tab', { name: 'Receipt' }).click()
  await expect(results.getByLabel('Process: Exited / blocked')).toBeVisible()
  await expect(results.getByLabel('Pipeline: Partial')).toBeVisible()
  await expect(page.getByLabel('Pipeline: Succeeded')).toHaveCount(0)
  // The scope marker never claims a fresh synthetic run when the last run failed.
  await expect(page.locator('.result-dock__scope')).toContainText(
    'Last Full Run · errored',
  )
  await results.getByRole('tab', { name: 'Preview' }).click()
  await expect(results.getByRole('note').first()).toContainText('Last Full Run errored')
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

  // Scope by role: each panel's chart carries an aria-label that also contains the
  // panel name, so a bare getByLabel matches both the section and the chart.
  const burn = page.getByRole('region', { name: 'Burn compile and training' })
  const privacy = page.getByRole('region', { name: 'Differential privacy budget' })
  const resource = page.getByRole('region', { name: 'Resource efficiency' })
  await expect(burn).toBeVisible()
  await expect(privacy).toBeVisible()
  await expect(resource).toBeVisible()

  // The Burn panel now reads the real run response. A URL-simulated success state
  // has no training report behind it, so the panel must show the structural
  // fixture fields and still refuse to render a loss it never received.
  await expect(burn.getByText('209').first()).toBeVisible()
  await expect(
    burn.getByText('No Full Run has produced a training report yet.'),
  ).toBeVisible()
  await expect(burn.getByText('0.0417')).toHaveCount(0)

  // The privacy capability is implemented: with no withDp(...) query this run
  // it stays Beta and empty, and never presents a number as measured.
  await expect(privacy.getByLabel('Maturity: Beta')).toBeVisible()
  await expect(
    privacy.getByText('No withDp(...) query ran in this Full Run'),
  ).toBeVisible()
  await expect(privacy.getByText('Not available in this version').first()).toBeVisible()

  // The resource panel is not implemented: it keeps a permanent maturity badge,
  // synthetic scope, and hollow bars.
  await expect(resource.getByLabel('Maturity: Planned')).toBeVisible()
  await expect(
    resource.getByText('Synthetic structure · not measured · proposed contract'),
  ).toBeVisible()
  await expect(resource.getByText('Not available in this version').first()).toBeVisible()

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
  const burn = page.getByRole('region', { name: 'Burn compile and training' })
  await expect(
    burn.getByText('No Full Run has produced a training report yet.'),
  ).toBeVisible()
  await expect(burn.locator('.monitor-bars__fill--train')).toHaveCount(0)

  await page.goto('/?screen=workspace&state=error')
  await page.getByRole('button', { name: 'Monitor' }).click()
  await expect(
    page
      .getByRole('region', { name: 'Burn compile and training' })
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

test('edit view gives the DAG canvas the whole region, not a sliver', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()

  // The editor owns the full canvas region. When `.compiler-split--edit` was
  // missing it fell back to the two-column split and the canvas collapsed to
  // roughly 20px between the palette and the inspector.
  const measured = await page.evaluate(() => {
    const rect = (selector) => {
      const node = document.querySelector(selector)
      return node ? node.getBoundingClientRect().width : 0
    }
    return {
      editor: rect('.dag-editor'),
      canvas: rect('.dag-canvas'),
      area: rect('.compiler-area'),
    }
  })
  expect(measured.editor).toBeGreaterThan(measured.area - 2)
  expect(measured.canvas).toBeGreaterThan(300)

  assertRuntime()
})

test('every DAG node uses the custom renderer and stays inside the canvas', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()

  // A node whose type is not registered in nodeTypes silently falls back to React
  // Flow's default white box, which is unreadable on this dark canvas.
  const fallbacks = await page.evaluate(
    () => document.querySelectorAll('.react-flow__node-default').length,
  )
  expect(fallbacks).toBe(0)
  expect(await page.locator('.dag-node').count()).toBeGreaterThan(0)

  // The seeded layout must fit the canvas at first open rather than being clipped.
  const clipped = await page.evaluate(() => {
    const canvas = document.querySelector('.dag-canvas').getBoundingClientRect()
    return Array.from(document.querySelectorAll('.react-flow__node')).filter((node) => {
      const box = node.getBoundingClientRect()
      return (
        box.x < canvas.x - 1 ||
        box.y < canvas.y - 1 ||
        box.x + box.width > canvas.x + canvas.width + 1 ||
        box.y + box.height > canvas.y + canvas.height + 1
      )
    }).length
  })
  expect(clipped).toBe(0)

  assertRuntime()
})

test('a node added from the palette reaches the generated code', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.addInitScript(() => {
    try {
      localStorage.removeItem('xazz_dag')
    } catch (error) {
      /* private mode */
    }
  })
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()

  const code = page.locator('.dag-code')
  await expect(code).not.toContainText('dropNull')

  // The transpiler resolves operations through NODE_MAPPINGS[node.type]. When the
  // palette wrote a literal 'dag' type instead of the tool id, the node appeared
  // on the canvas and was dropped from the code with no error.
  await page.locator('.dag-palette__tool', { hasText: 'Drop Null' }).click()
  await expect(code).toContainText('dropNull')

  assertRuntime()
})

test('the edit canvas hint stays inside the canvas and clear of its controls', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')
  await page.getByRole('button', { name: 'Edit' }).click()

  const collisions = await page.evaluate(() => {
    const rect = (selector) => {
      const node = document.querySelector(selector)
      return node ? node.getBoundingClientRect() : null
    }
    const overlaps = (a, b) =>
      !!a &&
      !!b &&
      a.x < b.x + b.width &&
      b.x < a.x + a.width &&
      a.y < b.y + b.height &&
      b.y < a.y + a.height
    const canvas = rect('.dag-canvas')
    const hint = rect('.dag-canvas__hint')
    return {
      escapesCanvas:
        hint.x < canvas.x - 1 || hint.x + hint.width > canvas.x + canvas.width + 1,
      hitsMinimap: overlaps(hint, rect('.react-flow__minimap')),
      hitsControls: overlaps(hint, rect('.react-flow__controls')),
    }
  })
  expect(collisions.escapesCanvas).toBe(false)
  expect(collisions.hitsMinimap).toBe(false)
  expect(collisions.hitsControls).toBe(false)

  assertRuntime()
})

test('opening the run gate does not crash the workspace', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&state=preflight')

  // A typo in the acknowledge handler threw a ReferenceError during render and
  // unmounted the entire workspace, so Full Run blanked the screen.
  await expect(page.locator('.workspace-page')).toBeVisible()
  const dialog = page.getByRole('dialog', {
    name: /Review what will execute on xazz-server/,
  })
  await expect(dialog).toBeVisible()

  const startRun = dialog.getByRole('button', { name: 'Start full run' })
  await expect(startRun).toBeDisabled()
  await dialog.getByRole('checkbox').click()
  await expect(startRun).toBeEnabled()

  assertRuntime()
})

test('the language toggle translates the step explanations', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace')

  const inspector = page.locator('.inspector')
  await expect(inspector.locator('.eyebrow').first()).toHaveText('Selected operation')
  await expect(page.locator('.operation-list h2')).toContainText('Pipeline operations')

  const toggle = page.locator('.workspace-topbar .locale-switch')
  await expect(toggle).toBeVisible()
  await toggle.getByRole('button', { name: '한국어' }).click()

  // The prose that explains a step is what a Korean reader needs translated.
  await expect(inspector.locator('.eyebrow').first()).toHaveText('선택한 단계')
  await expect(inspector.locator('h3').first()).toHaveText('이 단계가 하는 일')
  await expect(page.locator('.operation-list h2')).toContainText('파이프라인 단계')
  await expect(page.locator('.flow-node__band').first()).toHaveText('전처리')

  // The choice is shareable and survives a reload.
  await expect(page).toHaveURL(/lang=ko/)
  await page.reload()
  await expect(inspector.locator('.eyebrow').first()).toHaveText('선택한 단계')

  assertRuntime()
})

test('ML terms, column names and generated code stay untranslated', async ({ page }) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&lang=ko')

  // Translating these would put the screen at odds with the .xzz beside it.
  const rail = page.locator('.operation-list')
  await expect(rail).toContainText('40 epochs · loss 0.0417')
  await expect(rail).toContainText('209 params')
  await expect(rail).toContainText('pm25 · Float?')

  // The status axes are the contract vocabulary from docs/design/state-contract.md
  // and keep their exact English wording on every axis.
  await expect(page.getByLabel('Maturity: Available')).toBeVisible()

  await page.getByRole('button', { name: '코드' }).click()
  await expect(page.locator('.code-pane')).toContainText('train(AirPredictor')

  assertRuntime()
})

test('an explicit lang in the URL outranks the remembered choice', async ({ page }) => {
  const assertRuntime = observeRuntime(page)

  await page.goto('/?screen=workspace')
  await page
    .locator('.workspace-topbar .locale-switch')
    .getByRole('button', { name: '한국어' })
    .click()
  await expect(page.locator('.inspector .eyebrow').first()).toHaveText('선택한 단계')

  // Korean is now remembered, but a link that asks for English must open English.
  await page.goto('/?screen=workspace&lang=en')
  await expect(page.locator('.inspector .eyebrow').first()).toHaveText(
    'Selected operation',
  )

  assertRuntime()
})

test('the DAG editor follows the toggle instead of being Korean-only', async ({
  page,
}) => {
  const assertRuntime = observeRuntime(page)
  await page.goto('/?screen=workspace&lang=en')
  await page.getByRole('button', { name: 'Edit' }).click()

  // The editor shipped with its copy hardcoded in Korean inside an English UI.
  await expect(page.locator('.dag-palette__help')).toHaveText(
    'Drag a node onto the canvas, or click to add it',
  )
  await expect(page.locator('.dag-side__empty')).toContainText(
    'Select a node on the canvas',
  )

  await page
    .locator('.workspace-topbar .locale-switch')
    .getByRole('button', { name: '한국어' })
    .click()
  await expect(page.locator('.dag-palette__help')).toHaveText(
    '노드를 캔버스로 끌어놓거나 클릭해 추가하세요',
  )

  assertRuntime()
})
