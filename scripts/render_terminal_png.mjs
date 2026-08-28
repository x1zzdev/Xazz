/**
 * scripts/render_terminal_png.mjs — 터미널 출력 → PNG 렌더러
 * 실제 캡처한 텍스트를 그대로 터미널 스타일로 렌더링해 PNG로 저장한다.
 *
 * 사용법: node scripts/render_terminal_png.mjs <input.txt> <output.png> [prompt]
 */
import { pathToFileURL } from "node:url";
const { chromium } = await import(
  pathToFileURL(resolve(process.cwd(), "visual-ide/node_modules/playwright-core/index.mjs"))
);
import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const [, , input, output, prompt = "$ "] = process.argv;
if (!input || !output) {
  console.error("usage: node render_terminal_png.mjs <input.txt> <output.png> [prompt]");
  process.exit(1);
}

const raw = readFileSync(resolve(input), "utf-8").replace(/\n+$/, "");
const esc = (s) =>
  s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

const lineClass = (line) => {
  if (line.startsWith("❌")) return "t-err";
  if (/^\s*💡|Did you mean/.test(line)) return "t-hint";
  if (line.startsWith("✅")) return "t-ok";
  if (line.startsWith("📊") || line.startsWith("🧠") || line.startsWith("🏋")) return "t-hdr";
  if (line.startsWith("[xazz]")) return "t-dim";
  if (/^─+/.test(line) || /^\s*─+/.test(line)) return "t-dim";
  if (/^\s*\[Epoch/.test(line)) return "t-epoch";
  if (/^═══/.test(line)) return "t-hdr";
  if (/^\s*[┌╞╘│└]/.test(line)) return "t-table";
  return "";
};

const body = raw
  .split("\n")
  .map((l) => {
    if (l.startsWith("{") && l.length > 110) return "";
    if (l.length > 150) return l.slice(0, 147) + " …";
    return l;
  })
  .filter((l) => l !== "")
  .map((l) => `<span class="tl ${lineClass(l)}">${esc(l) || "&nbsp;"}</span>`)
  .join("");

const firstCommand = prompt + esc(process.env.TERM_CMD ?? "xazz run pipeline.xzz");

const html = `<!DOCTYPE html>
<html><head><meta charset="utf-8"><style>
  * { margin:0; padding:0; box-sizing:border-box; }
  body { background:#0b1120; padding:0; font-family:'JetBrains Mono','Cascadia Code','Fira Code',Menlo,Consolas,monospace; }
  .win { border-radius:12px; overflow:hidden; box-shadow:0 20px 60px rgba(0,0,0,.55); border:1px solid #1e293b; }
  .bar { background:#16213a; padding:10px 14px; display:flex; align-items:center; gap:8px; }
  .dot { width:12px; height:12px; border-radius:50%; }
  .d1{background:#ff5f57}.d2{background:#febc2e}.d3{background:#28c840}
  .title { margin-left:10px; color:#7c8db0; font-size:12.5px; }
  .term { background:#0d1526; padding:18px 22px 22px; font-size:13px; line-height:1.5; color:#dbe4f3;
          white-space:pre; font-family:'JetBrains Mono','Noto Sans Mono CJK KR','Noto Color Emoji',Menlo,Consolas,monospace; min-width:fit-content; max-width:980px; overflow:hidden; }
  .cmd { color:#7ee2a8; font-weight:600; }
  .cmd .path { color:#6e93d6; font-weight:400; }
  .tl { display:block; }
  .t-dim  { color:#5f7292; }
  .t-err  { color:#ff7b7b; font-weight:600; }
  .t-hint { color:#e5c07b; }
  .t-ok   { color:#4ade80; font-weight:600; }
  .t-hdr  { color:#93c5fd; font-weight:600; }
  .t-epoch{ color:#a5b4fc; }
  .t-table{ color:#c3cfe4; }
</style></head><body>
<div class="win" id="win">
  <div class="bar"><span class="dot d1"></span><span class="dot d2"></span><span class="dot d3"></span>
    <span class="title">Xazz — zsh</span></div>
  <div class="term"><span class="cmd">${firstCommand}</span>\n${body}</div>
</div>
</body></html>`;

const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 2 });
await page.setContent(html);
const box = await page.locator("#win").boundingBox();
await page.setViewportSize({ width: Math.ceil(box.width) + 4, height: Math.ceil(box.height) + 4 });
await page.screenshot({ path: resolve(output), clip: { x: 0, y: 0, width: box.width + 2, height: box.height + 2 } });
await browser.close();
console.log(`saved → ${output}`);
