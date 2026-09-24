// Reproducible second-round demo (#154). Drives the Visual IDE against a real
// xazz-server, records one captioned video per language and refreshes the
// docs/assets screenshots the IDE issues (#107–#116) ask for.
//
//   # repo root — build once, then start the server from the repo root so
//   # load("visual-ide/data/...") resolves. Do NOT set XAZZ_EXEC_PATH: xazz-runner
//   # reads the same variable to find xazz-exec and would re-run `xazz` instead.
//   cargo build --release -p xazz -p xazz-server -p xazz-runner -p xazz-exec
//   ./target/release/xazz-server
//
//   # visual-ide/
//   node scripts/demo.mjs                       # EN + KO videos and screenshots
//   node scripts/demo.mjs --lang en             # one language
//   node scripts/demo.mjs --tamper ../audit_log/audit.jsonl
//        # also edits one audit record to show the chain break, then restores it
//
// Server state it leaves behind: new runs and audit records (append-only by
// design). A policy pack it installs is removed again at the end.
import assert from 'node:assert/strict'
import { copyFile, mkdir, readFile, rm, writeFile } from 'node:fs/promises'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from 'playwright'
import { createServer } from 'vite'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const repo = resolve(root, '..')
const assets = resolve(repo, 'docs', 'assets')
const args = process.argv.slice(2)
const option = (name, fallback) => {
  const index = args.indexOf(name)
  return index >= 0 ? args[index + 1] : fallback
}
const SERVER = option('--server', 'http://127.0.0.1:8005')
const languages = option('--lang', 'both') === 'both' ? ['en', 'ko'] : [option('--lang')]
const tamperPath = option('--tamper') ? resolve(process.cwd(), option('--tamper')) : null

const health = await fetch(`${SERVER}/health`).then((r) => r.json()).catch(() => null)
assert.equal(health?.status, 'ok', `xazz-server is not answering at ${SERVER}/health — start it first (see header)`)

const captions = {
  en: {
    graph: '1 · Typed .xzz pipeline — graph and code stay linked',
    run: '2 · Full Run — progress shows only what is measured',
    receipt: '3 · Receipt carries the server run id',
    monitor: '4 · Monitor — Burn training and this run’s DP report',
    governance: '5 · Governance — tenant ε ledger and audit hash chain',
    tamper: '6 · Edit one audit record on disk → the chain names it',
    history: '7 · History — reopen a run and restore its rows',
    lineage: '8 · Column lineage — the aggregate traces back to its source',
    pack: '9 · Install a finance policy pack — the change is recorded',
    block: '10 · Policy-as-Code blocks an identifier before execution',
    schema: '11 · CSV upload — xazz-server infers the schema (UTF-8 / EUC-KR)',
  },
  ko: {
    graph: '1 · 타입이 있는 .xzz 파이프라인 — 그래프와 코드가 연결됩니다',
    run: '2 · 전체 실행 — 측정한 진행 상태만 보여줍니다',
    receipt: '3 · 실행 기록에 서버 실행 ID가 남습니다',
    monitor: '4 · 모니터 — 이번 실행의 Burn 학습과 DP 리포트',
    governance: '5 · 거버넌스 — 테넌트 ε 원장과 감사 해시 체인',
    tamper: '6 · 디스크의 감사 레코드 하나를 고치면 체인이 위치를 짚습니다',
    history: '7 · 히스토리 — 실행을 다시 열고 결과를 복원합니다',
    lineage: '8 · 컬럼 계보 — 집계 결과를 원본 컬럼까지 추적합니다',
    pack: '9 · 금융 정책 팩 설치 — 변경이 이력에 남습니다',
    block: '10 · 정책 코드가 실행 전에 식별자 노출을 막습니다',
    schema: '11 · CSV 업로드 — xazz-server가 스키마를 추론합니다 (UTF-8 / EUC-KR)',
  },
}

async function caption(page, text) {
  await page.evaluate((value) => {
    let node = document.getElementById('demo-caption')
    if (!node) {
      node = document.createElement('div')
      node.id = 'demo-caption'
      node.style.cssText =
        'position:fixed;z-index:9999;left:50%;bottom:248px;transform:translateX(-50%);max-width:80vw;' +
        'padding:10px 16px;border-radius:8px;background:#202b26;border:1px solid #596c63;color:#f2f6f3;' +
        'font:600 15px/1.4 ui-sans-serif,system-ui,sans-serif;pointer-events:none'
      document.body.append(node)
    }
    node.textContent = value
    node.hidden = false
  }, text)
}

// Screenshots are evidence of the UI, so the demo caption is hidden while one is taken.
async function shot(page, name, locator) {
  await page.evaluate(() => document.getElementById('demo-caption')?.setAttribute('hidden', ''))
  await (locator ?? page).screenshot({ path: resolve(assets, name) })
  await page.evaluate(() => document.getElementById('demo-caption')?.removeAttribute('hidden'))
}

const view = (page, index) => page.locator('.canvas-toolbar .segmented-control button').nth(index)
const tab = (page, index) => page.locator('.result-dock__tabs [role=tab]').nth(index)
const govPanel = (page, index) => page.locator('.gov-section .monitor-panel').nth(index)
const pause = (page, ms = 3500) => page.waitForTimeout(ms)
// Scroll only the nearest scrolling panel; scrollIntoView would also move the page.
const reveal = (locator) =>
  locator.evaluate((node) => {
    let box = node.parentElement
    while (box && !/auto|scroll/.test(getComputedStyle(box).overflowY)) box = box.parentElement
    if (box) box.scrollTop += node.getBoundingClientRect().top - box.getBoundingClientRect().top - 12
  })

async function scenario(page, language, capture) {
  const say = (key) => caption(page, captions[language][key])

  await page.goto(`/?screen=workspace&lang=${language}`)
  await say('graph')
  await page.locator('.operation-list button').nth(3).click()
  await pause(page)

  await say('run')
  await page.locator('.workspace-topbar .button--tool-primary').click()
  await page.getByRole('checkbox').check()
  await pause(page, 800)
  await page.locator('.preflight-dialog .button--tool-primary').click()
  await page.locator('.run-overlay').waitFor()
  if (capture) await shot(page, 'ide_run_progress.png')
  await page.locator('.run-overlay').waitFor({ state: 'detached', timeout: 120_000 })

  await say('receipt')
  await tab(page, 4).click()
  await pause(page)

  await say('monitor')
  await view(page, 4).click()
  await pause(page, 4500)

  await say('governance')
  await reveal(govPanel(page, 0))
  await pause(page)
  if (capture) await shot(page, 'ide_dp_ledger.png', govPanel(page, 0))
  await reveal(govPanel(page, 1))
  await pause(page)
  if (capture) await shot(page, 'ide_audit_chain.png', govPanel(page, 1))
  if (!capture && language === 'ko') await shot(page, 'ide_governance_ko.png')

  if (tamperPath) {
    await say('tamper')
    const original = await readFile(tamperPath, 'utf8')
    try {
      const lines = original.trimEnd().split('\n')
      const record = JSON.parse(lines[lines.length - 1])
      record.outcome = record.outcome === 'success' ? 'failed' : 'success'
      lines[lines.length - 1] = JSON.stringify(record)
      await writeFile(tamperPath, `${lines.join('\n')}\n`)
      await govPanel(page, 1).locator('.gov-inline-form button').last().click()
      await govPanel(page, 1).locator('.gov-verdict--broken').waitFor()
      await pause(page, 5000)
      if (capture) await shot(page, 'ide_audit_tamper.png', govPanel(page, 1))
    } finally {
      await writeFile(tamperPath, original)
    }
    await govPanel(page, 1).locator('.gov-inline-form button').last().click()
    await govPanel(page, 1).locator('.gov-verdict--ok').waitFor()
    await pause(page, 1200)
  }

  await say('history')
  await tab(page, 5).click()
  await page.locator('.run-history li button').first().click()
  await page.locator('.run-detail').waitFor()
  await pause(page)
  if (capture) await shot(page, 'ide_run_history.png', page.locator('.result-dock'))
  await page.locator('.run-detail .button--compact').click()
  await pause(page)

  await say('lineage')
  await view(page, 2).click()
  await page.locator('.lineage-panel > button').click()
  await page.locator('.lineage-table').waitFor()
  await page.locator('.lineage-table button').last().click()
  await reveal(page.locator('.lineage-panel'))
  await pause(page)
  if (capture) await shot(page, 'ide_lineage.png', page.locator('.inspector'))

  await say('pack')
  await view(page, 4).click()
  await reveal(govPanel(page, 2))
  await govPanel(page, 2).locator('.gov-file input').setInputFiles(resolve(repo, 'examples/security/finance_policy.json'))
  await govPanel(page, 2).locator('.gov-pack-form .button--tool-primary').click()
  await govPanel(page, 2).locator('.gov-timeline').waitFor()
  await pause(page)
  if (capture) await shot(page, 'ide_policy_packs.png', govPanel(page, 2))

  await say('block')
  await view(page, 0).click()
  await page.locator('.react-flow__node').filter({ hasText: 'Select' }).first().click()
  await page.locator('.dag-params input.dag-field__input').first().fill('observed_at, district, pm25, temperature_c, phone')
  await pause(page, 1200)
  await view(page, 4).click()
  await page.locator('.guardrail-toolbar button').nth(0).click()
  await page.locator('.guardrail-result--blocked').waitFor()
  await page.locator('.guardrail-toolbar button').nth(1).click()
  await page.locator('.guardrail-remediation').waitFor()
  await reveal(page.locator('.guardrail-result--blocked'))
  await pause(page, 4500)
  if (capture) await shot(page, 'ide_policy_block.png')
  // Take the identifier back out: the same check now passes and Full Run unlocks.
  await view(page, 0).click()
  await page.locator('.react-flow__node').filter({ hasText: 'Select' }).first().click()
  await page.locator('.dag-params input.dag-field__input').first().fill('observed_at, district, pm25, temperature_c')
  await view(page, 4).click()
  await page.locator('.guardrail-toolbar button').nth(0).click()
  await page.locator('.guardrail-result--pass').waitFor()
  await reveal(page.locator('.guardrail-result--pass'))
  await pause(page)

  await say('schema')
  await view(page, 0).click()
  await page.locator('.react-flow__node').filter({ hasText: 'File Input' }).first().click()
  await page.locator('.dag-params input[type=file]').setInputFiles(resolve(root, 'data/seoul_air_quality.csv'))
  await page.locator('.dag-notice').filter({ hasText: /xazz-server/ }).waitFor()
  await pause(page, 4500)
  if (capture) await shot(page, 'ide_schema_inference.png')
}

process.env.VITE_API_BASE_URL = SERVER
const vite = await createServer({ root, logLevel: 'error', server: { host: '127.0.0.1', port: 5299, strictPort: true } })
await vite.listen()
const browser = await chromium.launch()
const videoDir = resolve(assets, 'demo')
await mkdir(videoDir, { recursive: true })
try {
  for (const [index, language] of languages.entries()) {
    const context = await browser.newContext({
      baseURL: 'http://127.0.0.1:5299',
      viewport: { width: 1440, height: 960 },
      recordVideo: { dir: resolve(videoDir, '.tmp'), size: { width: 1440, height: 960 } },
    })
    const page = await context.newPage()
    await scenario(page, language, index === 0)
    await context.close()
    await copyFile(await page.video().path(), resolve(videoDir, `xazz-demo-${language}.webm`))
    console.log(`demo: recorded docs/assets/demo/xazz-demo-${language}.webm`)
  }
} finally {
  await rm(resolve(videoDir, '.tmp'), { recursive: true, force: true })
  await browser.close()
  await vite.close()
  // Leave the tenant on the policy it had before the demo installed a pack.
  await fetch(`${SERVER}/security/policy`, { method: 'DELETE' }).catch(() => {})
}
