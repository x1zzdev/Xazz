import assert from 'node:assert/strict'
import { access, mkdir, rename, rm } from 'node:fs/promises'
import { dirname, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'
import { preview as startPreview } from 'vite'

const prototypeRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repositoryRoot = resolve(prototypeRoot, '..')
// Screenshots live under docs/ since the repository restructure (4a6bbc2);
// this still pointed at the removed top-level design/ and wrote outside the repo.
const designRoot = resolve(repositoryRoot, 'docs', 'design')
const outputRoot = resolve(designRoot, 'screenshots')
const temporaryRoot = resolve(designRoot, `.screenshots-tmp-${process.pid}`)
const backupRoot = resolve(designRoot, `.screenshots-backup-${process.pid}`)

for (const target of [outputRoot, temporaryRoot, backupRoot]) {
  assert(
    target.startsWith(`${repositoryRoot}${sep}`),
    'screenshot targets must remain inside the Xazz repository',
  )
}

await import('./build.mjs')
await rm(temporaryRoot, { recursive: true, force: true })
await rm(backupRoot, { recursive: true, force: true })
await mkdir(temporaryRoot, { recursive: true })

const frames = [
  ['landing-desktop-1440', '/', 1440, 960, true],
  ['landing-mobile-390', '/', 390, 844, true],
  ['project-start', '/?screen=start', 1440, 960, true],
  ['project-start-ko', '/?screen=start&lang=ko', 1440, 960, true],
  ['workspace-sample-ready', '/?screen=workspace', 1440, 960, false],
  ['preflight-needs-review', '/?screen=workspace&state=preflight', 1440, 960, false],
  ['run-in-progress', '/?screen=workspace&state=running', 1440, 960, false],
  ['run-success-receipt', '/?screen=workspace&state=success', 1440, 960, false],
  ['error-recovery', '/?screen=workspace&state=error', 1440, 960, false],
  // The Monitor view is deliberately absent from the URL, so these frames need a
  // click step to reach the state they document.
  [
    'monitor-no-run',
    '/?screen=workspace',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: 'Monitor' }).click(),
  ],
  [
    'monitor-after-success',
    '/?screen=workspace&state=success',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: 'Monitor' }).click(),
  ],
  [
    'monitor-after-error',
    '/?screen=workspace&state=error',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: 'Monitor' }).click(),
  ],
  [
    'workspace-korean',
    '/?screen=workspace&lang=ko',
    1440,
    960,
    false,
  ],
  [
    'dag-editor-korean',
    '/?screen=workspace&lang=ko',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: '편집' }).click(),
  ],
  [
    'dag-editor',
    '/?screen=workspace',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: 'Edit' }).click(),
  ],
  [
    'run-preflight-gate',
    '/?screen=workspace&state=preflight',
    1440,
    960,
    false,
  ],
  [
    'ml-compile-band',
    '/?screen=workspace',
    1440,
    960,
    false,
    (page) => page.getByRole('button', { name: /Train model/ }).first().click(),
  ],
]

let browser
let previewServer
let existingMoved = false

async function pathExists(path) {
  try {
    await access(path)
    return true
  } catch (error) {
    if (error.code === 'ENOENT') return false
    throw error
  }
}

try {
  previewServer = await startPreview({
    root: prototypeRoot,
    logLevel: 'warn',
    preview: {
      host: '127.0.0.1',
      port: 0,
      strictPort: true,
    },
  })
  const address = previewServer.httpServer.address()
  assert(address && typeof address === 'object', 'preview did not expose a bound port')
  const baseURL = `http://127.0.0.1:${address.port}`
  const response = await fetch(`${baseURL}/`)
  assert.equal(response.ok, true, `preview returned HTTP ${response.status}`)
  assert.match(
    await response.text(),
    /Xazz · Inspect before you run/,
    'preview did not return the Xazz prototype',
  )

  browser = await chromium.launch({ headless: true })
  for (const [name, route, width, height, fullPage, prepare] of frames) {
    const page = await browser.newPage({
      viewport: { width, height },
      colorScheme: 'light',
      reducedMotion: 'reduce',
    })
    await page.goto(`${baseURL}${route}`, { waitUntil: 'networkidle' })
    await page.evaluate(() => document.fonts.ready)
    assert.match(await page.title(), /^Xazz/, `${name} did not render the Xazz app`)
    if (prepare) {
      await prepare(page)
      await page.waitForTimeout(150)
    }
    await page.screenshot({
      path: resolve(temporaryRoot, `${name}.png`),
      fullPage,
      animations: 'disabled',
    })
    await page.close()
  }

  for (const [name] of frames) {
    await access(resolve(temporaryRoot, `${name}.png`))
  }

  if (await pathExists(outputRoot)) {
    await rename(outputRoot, backupRoot)
    existingMoved = true
  }

  try {
    await rename(temporaryRoot, outputRoot)
  } catch (error) {
    if (existingMoved) await rename(backupRoot, outputRoot)
    throw error
  }
  if (existingMoved) await rm(backupRoot, { recursive: true, force: true })
} finally {
  if (browser) await browser.close()
  if (previewServer) {
    await new Promise((resolveClose, rejectClose) => {
      previewServer.httpServer.close((error) => {
        if (error) rejectClose(error)
        else resolveClose()
      })
    })
  }
  await rm(temporaryRoot, { recursive: true, force: true })
}

console.log(`capture: ok; frames=${frames.length}; output=${outputRoot}`)
