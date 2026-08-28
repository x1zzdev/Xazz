/**
 * scripts/capture_ide.mjs — Visual IDE 실사 스크린샷 캡처
 * 실제 구동 중인 Visual IDE(Vite 5173) + xazz-server(8005)에 접속해
 * 워크스페이스 · Full Run 실행 결과 · 모니터 화면을 PNG로 저장한다.
 */
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";
const { chromium } = await import(
  pathToFileURL(resolve(process.cwd(), "visual-ide/node_modules/playwright-core/index.mjs"))
);

const BASE = "http://127.0.0.1:5173";
const OUT = "docs/assets";

const browser = await chromium.launch();
const page = await browser.newPage({
  viewport: { width: 1600, height: 940 },
  deviceScaleFactor: 1.6,
});

// 1) 워크스페이스 진입 (서버 연결 상태 확인)
await page.goto(`${BASE}/?screen=workspace`);
await page.waitForLoadState("networkidle");
await page.waitForTimeout(2500);
await page.screenshot({ path: `${OUT}/ide_workspace.png` });
console.log("saved ide_workspace.png");

// 2) Full Run 실행 → 결과 (실행 확인 게이트 통과)
const runBtn = page.getByRole("button", { name: /Full Run/ }).first();
await runBtn.click();
console.log("clicked Full Run…");
const confirmBox = page.getByRole("checkbox", { name: /I understand|동의/ }).first();
if (await confirmBox.count()) {
  await confirmBox.check().catch(() => confirmBox.click());
}
const startBtn = page.getByRole("button", { name: /Start full run|실행/ }).first();
await startBtn.click();
console.log("confirmed — running…");
await page.waitForFunction(
  () => /Succeeded|Completed|실행 완료|success/i.test(document.body.innerText),
  undefined,
  { timeout: 180000 },
);
await page.waitForTimeout(1500);
await page.screenshot({ path: `${OUT}/ide_run_result.png` });
console.log("saved ide_run_result.png");

// 3) 모니터 탭
const monitorTab = page.getByRole("button", { name: /Monitor|모니터/ }).first();
if (await monitorTab.count()) {
  await monitorTab.click();
  await page.waitForTimeout(1500);
  await page.screenshot({ path: `${OUT}/ide_monitor.png` });
  console.log("saved ide_monitor.png");
}

// 4) 한국어 UI (실행 완료 상태 유지)
const koBtn = page.getByRole("button", { name: "한국어" }).first();
if (await koBtn.count()) {
  await koBtn.click();
  await page.waitForTimeout(1200);
  await page.screenshot({ path: `${OUT}/ide_monitor_ko.png` });
  console.log("saved ide_monitor_ko.png");
}

await browser.close();
