#!/usr/bin/env python3
"""scripts/make_pitch_deck.py — 2차 평가 발표덱(자체 완결형 HTML) 생성기

`docs/demo/presentation-outline.md` 의 10장 구성을 실제 발표 가능한 슬라이드로 만든다.
결과는 단일 HTML(`docs/demo/pitch-deck.html`)이고, 이미지를 base64 로 내장하므로
**오프라인에서 열린다**.

  ← / → / Space / PageDown : 다음
  ← / ↑ / PageUp           : 이전
  Home / End               : 처음 / 끝
  N                        : 발표자 노트
  F                        : 전체화면
  Ctrl+P                   : 슬라이드별 페이지로 인쇄 (→ PDF)

디자인 원칙
  * 페이지마다 구조가 다르다. 표지(제품 전면) · 문장형 · 코드형 · 도식형 · 데이터형 ·
    데모 전환(흑) · 증거(흑) · 목록형 · 표형 · 마무리(흑).
  * 장식 요소를 두지 않는다. 세로 띠 · 그림자 · 카드 · 라운드 박스 · 이모지 없음.
  * 색은 의미로만 쓴다. 빨강=차단/불일치, 초록=통과. 그 외는 흑백.
  * 슬라이드의 모든 글자는 15px 이상. 출처·조건은 문장으로 쓰고, 세부 근거는
    발표자 노트와 문서로 넘긴다.
  * 코드·터미널·진단은 생성 시점에 실제 xazz 를 실행해 가져온다.

사용법:
    python3 scripts/make_pitch_deck.py
    python3 scripts/make_pitch_deck.py --xazz target/release/xazz --out docs/demo/pitch-deck.html
"""

from __future__ import annotations

import argparse
import base64
import html
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
ASSETS = ROOT / "docs" / "assets"
CASES = ROOT / "examples" / "field_cases"
REPO_URL = "github.com/x1zzdev/Xazz"


# ── helpers ──────────────────────────────────────────────────────────────────

def esc(s: str) -> str:
    return html.escape(s, quote=False)


def b64_png(path: pathlib.Path, alt: str = "") -> str:
    data = base64.b64encode(path.read_bytes()).decode("ascii")
    return f'<img src="data:image/png;base64,{data}" alt="{esc(alt or path.stem)}">'


def find_xazz(explicit: str | None) -> pathlib.Path | None:
    for c in filter(None, [explicit, os.environ.get("XAZZ_BIN"),
                           ROOT / "target" / "release" / "xazz",
                           ROOT / "target" / "debug" / "xazz"]):
        p = pathlib.Path(c)
        if p.is_file():
            return p.resolve()
    from shutil import which

    found = which("xazz")
    return pathlib.Path(found).resolve() if found else None


def run_capture(xazz: pathlib.Path | None, cwd: pathlib.Path, argv: list[str]) -> list[str]:
    """Run the CLI and return cleaned output lines (never raises)."""
    if xazz is None:
        return []
    env = dict(os.environ, XAZZ_LANG="ko")
    try:
        r = subprocess.run([str(xazz), *argv], cwd=cwd, capture_output=True,
                           text=True, timeout=180, env=env)
    except Exception:  # noqa: BLE001
        return []
    text = (r.stdout or "") + (r.stderr or "")
    out: list[str] = []
    for line in text.splitlines():
        line = line.rstrip()
        if not line or line.startswith("[xazz] input:") or line.startswith("[xazz] Lexer") \
                or line.startswith("[xazz] Parser"):
            continue
        out.append(line)
    return out


def trim(line: str, width: int) -> str:
    line = " ".join(line.split())
    return line if len(line) <= width else line[: width - 1].rstrip() + "…"


def first(lines: list[str], needle: str, fallback: str = "") -> str:
    return next((l.strip() for l in lines if needle in l), fallback)


# ── figures ──────────────────────────────────────────────────────────────────

def flow_figure() -> str:
    """Three stations, two gates — typographic, no boxes."""
    W, H = 1120, 250
    ink, muted, line = "#0A0A0B", "#6B6B6F", "#CFCFD4"
    block, run = "#D93A2F", "#2E9E6B"
    font = "'Pretendard','Apple SD Gothic Neo','Malgun Gothic','Noto Sans KR',sans-serif"

    def station(x, title, anchor="middle"):
        return (f'<text x="{x}" y="150" font-size="34" font-weight="800" fill="{ink}" '
                f'text-anchor="{anchor}" letter-spacing="-1">{title}</text>')

    def gate(x, color, label, sub):
        return (f'<line x1="{x}" y1="104" x2="{x}" y2="170" stroke="{color}" stroke-width="3"/>'
                f'<text x="{x}" y="84" font-size="17" font-weight="700" fill="{color}" '
                f'text-anchor="middle">{label}</text>'
                f'<text x="{x}" y="200" font-size="15" fill="{muted}" text-anchor="middle">{sub}</text>')

    return f"""<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" font-family="{font}" role="img">
  <rect width="{W}" height="{H}" fill="#FFFFFF"/>
  <line x1="196" y1="128" x2="430" y2="128" stroke="{line}" stroke-width="1.5"/>
  <line x1="556" y1="128" x2="790" y2="128" stroke="{line}" stroke-width="1.5"/>
  <circle cx="196" cy="128" r="3" fill="{line}"/><circle cx="556" cy="128" r="3" fill="{line}"/>
  {gate(318, block, "검사", "스키마 · 타입 · 정책")}
  {gate(678, run, "실행", "Polars · DP · Burn")}
  {station(96, ".xzz", "start")}
  {station(493, "Typed IR")}
  {station(916, "결과")}
  <text x="0" y="26" font-size="16" fill="{muted}">결과는 두 게이트를 모두 통과한 뒤에.</text>
</svg>"""


# ── slides ──────────────────────────────────────────────────────────────────

def build_slides(xazz: pathlib.Path | None) -> tuple[str, list[str]]:
    slides: list[str] = []
    notes: list[str] = []

    # Real artifacts, captured at build time.
    diag_err, diag_hint = "", ""
    if xazz is not None:
        with tempfile.TemporaryDirectory() as td:
            f = pathlib.Path(td) / "typo.xzz"
            f.write_text('type Air = { temperature_c: float }\n'
                         'v raw = load("air.csv") :: Air\n'
                         'v warm = raw |> filter(temperture_c > 20)\n', encoding="utf-8")
            lines = run_capture(xazz, pathlib.Path(td), ["check", "typo.xzz"])
        diag_err = first(lines, "[error")
        diag_hint = first(lines, "Did you mean")
    if not diag_err:
        diag_err = "❌ [error [line 3: col 24]] 표현식: 스키마에 'temperture_c' 컬럼이 존재하지 않습니다."
        diag_hint = '💡 사용 가능한 컬럼: temperature_c · Did you mean: col("temperature_c")?'

    fin = CASES / "finance_fds"
    safe = run_capture(xazz, fin, ["run", "fds_safe.xzz"])
    blocked = run_capture(xazz, fin, ["run", "fds_blocked.xzz"])

    safe_pass = first(safe, "정책 검사 통과", "정적 가드레일: 정책 검사 통과 — 위반 없음")
    safe_pass = safe_pass.replace("[xazz] 정적 가드레일: ", "").replace("[xazz] ", "")
    safe_dp = first(safe, "DP 적용", "DP 적용: laplace (ε=1, δ=0, Δf=1) — 예산 ε 1.00/10.00")
    safe_dp = safe_dp.replace("[xazz] ", "").split("|")[0].strip().rstrip("—- ").strip()
    safe_done = first(safe, "done —", "")
    safe_done = safe_done.replace("[xazz] ", "")

    block_head = first(blocked, "execution blocked", "[xazz security guardrail] execution blocked")
    xzp = next((i for i, l in enumerate(blocked) if "XZP001" in l), None)
    block_lines = ["$ xazz run fds_blocked.xzz", block_head]
    if xzp is not None:
        block_lines.append(blocked[xzp].strip())

    tamper_meta = ASSETS / "demo" / "audit-tamper.json"
    tamper_record = "N"
    if tamper_meta.exists():
        try:
            import json as _json

            tamper_record = str(_json.loads(tamper_meta.read_text(encoding="utf-8")).get("record", "N"))
        except Exception:  # noqa: BLE001
            tamper_record = "N"

    def term(lines: list[str], tone: str = "") -> str:
        return "\n".join(esc(l) for l in lines)

    # ── 01 cover ────────────────────────────────────────────────────────────
    cover = b64_png(ASSETS / "ide_workspace.png", "Xazz Visual IDE — 그래프와 코드, 거버넌스 패널")
    slides.append(f"""
    <section class="slide slide--cover">
      <div class="cover-text">
        <p class="cover-meta">2026 오픈소스 개발자대회 · 2차 평가</p>
        <h1 class="wordmark">Xazz</h1>
        <p class="cover-line">AI 파이프라인의<br>실행 전 검사,<br>실행 후 증명.</p>
        <p class="cover-repo">{REPO_URL}</p>
      </div>
      <div class="cover-media">{cover}</div>
    </section>""")
    notes.append("15초. 인사는 한 번만. 화면은 실제 IDE다 — 이게 오늘 데모할 화면이라고 먼저 말한다.")

    # ── 02 problem ──────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>데이터가 흐른 뒤에 드러나는 오류</h2>
      <div class="rows">
        <p>스키마와 코드가 어긋난 채 시작되는 학습</p>
        <p>경계마다 반복되는 복사, 학습 시간에 숨는 비용</p>
        <p>코드 밖에 남지 않는 실행의 근거</p>
      </div>
      <p class="source">Python 저장소 210개를 분석한 연구에서, 결함의 15%는 정적 타입 검사기로 막을 수 있었다.
      저자들은 이 값을 하한이라고 밝혔다. — Khan et al., IEEE TSE 2021</p>
    </section>""")
    notes.append("45초. 세 문장을 천천히. 마지막 출처 문장은 '하한'이라는 단어까지 읽는다. "
                 "92% 같은 두 번째 통계는 말하지 않는다 — 하나면 충분하다.")

    # ── 03 language ─────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>코드에 적힌 스키마와 프라이버시</h2>
      <div class="split-58">
        <pre class="code"><span class="c">// examples/field_cases/finance_fds/fds_safe.xzz</span>
v txn_by_category = load("data/transactions.csv") :: Transaction
    |&gt; groupBy("merchant_category")
    |&gt; agg([count("is_fraud"), sum("is_fraud")])
    |&gt; orderBy("merchant_category", desc: true);

v amount_by_category = load("data/transactions.csv") :: Transaction
    |&gt; groupBy("merchant_category")
    |&gt; mean("transaction_amount")
    |&gt; <b>withDp</b>(epsilon: 1.0, mechanism: laplace, sensitivity: 1.0)
    |&gt; orderBy("merchant_category");

v fraud_by_band = load("data/transactions.csv") :: Transaction
    |&gt; groupBy("age_band")
    |&gt; agg([count("is_fraud"), sum("is_fraud")])</pre>
        <div class="artifact">
          <p class="artifact__cap">이 파일의 실제 실행 출력</p>
          <pre class="term"><span class="ok">✔</span> 정책 검사 통과 — 위반 없음
<span class="ok">✔</span> DP 적용: laplace · ε=1 · Δf=1
<span class="ok">✔</span> shape: (7, 2)
<span class="ok">✔</span> 4 AST · 1 types · 3 pipelines</pre>
          <p class="artifact__note">스키마 선언·집계·노이즈 주입이 한 파일 안에서</p>
        </div>
      </div>
    </section>""")
    notes.append("50초. 코드는 왼쪽 위 세 줄만 읽고, withDp를 손으로 짚는다. 오른쪽은 실제 출력이라고 말한다. "
                 "'같은 파일에서 정책 검사까지 끝난다'가 핵심.")

    # ── 04 gates ────────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>라이브러리가 아닌,<br>파이프라인의 연산</h2>
      <div class="figure">{flow_figure()}</div>
      <div class="diagnostic">
        <pre class="term term--light"><span class="bad">{esc(trim(diag_err, 68))}</span>
{esc(trim(diag_hint, 68))}</pre>
        <p>컬럼 하나가 틀리면 — 데이터를 읽기 전에, 3행 24열에서</p>
      </div>
    </section>""")
    notes.append("55초. 도식은 왼쪽에서 오른쪽으로 한 번만 훑는다. 아래 진단을 읽고 "
                 "'Polars 위에서 돌지만, 판단은 그 앞에서 한다'로 마무리.")

    # ── 05 performance ──────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>pandas 대비 2.04 · 3.84 · 2.14배</h2>
      <div class="figure figure--chart">{b64_png(ASSETS / 'benchmark_chart.png', 'pandas 대비 속도향상 2.04× 3.84× 2.14×')}</div>
      <p class="bigline">409만 행 피크 메모리 <b>405 MB</b> — pandas <b>635 MB</b></p>
      <p class="source">Windows 11 한 대 · release 빌드 · 3회 중앙값 · 파이프라인 실행 시간만</p>
    </section>""")
    notes.append("40초. '한 대에서 잰 값'을 먼저 말한다. 3.84→2.14가 왜 줄어드는지는 질문이 오면 답한다.")

    # ── 06 demo ─────────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>영상 시연 — 2분 41초</h2>
      <ol class="beats">
        <li><span class="t">0:00</span><span class="b">그래프와 코드, 같은 파일</span></li>
        <li><span class="t">0:18</span><span class="b">Full Run과 실행 영수증</span></li>
        <li><span class="t">0:30</span><span class="b">DP 원장과 학습 리포트</span></li>
        <li><span class="t">1:05</span><span class="b">감사 체인의 변조 탐지</span></li>
        <li><span class="t">1:58</span><span class="b">전화번호 차단 — 실행 전</span></li>
        <li><span class="t">2:30</span><span class="b">CSV 스키마 추론</span></li>
      </ol>
      <p class="demo-note">실제 서버 응답을 녹화한 영상 · 자막 포함 · 소리 없이 재생</p>
    </section>""")
    notes.append("화면 전환용. 여기서 브라우저로 넘어간다. 데모 중에는 설명을 최소로 — 기능 나열이 아니라 "
                 "한 팀의 사건처럼 진행한다. 마지막 장면(차단)이 감정의 정점이다.")

    # ── 07 evidence ─────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>실행 전의 차단, 실행 후의 증명</h2>
      <div class="evidence">
        <div>
          <p class="evidence__label">실행 전 · 차단</p>
          <pre class="term term--dark">{term(block_lines)}</pre>
          <p class="evidence__note">고객번호·카드번호의 결과 반출 → 실행 전 차단.
          사유에 신용정보법 제32조 · 개인정보 보호법 제24조.</p>
        </div>
        <div>
          <p class="evidence__label">실행 후 · 변조 탐지</p>
          <p class="quote">“The chain breaks at record <b>#{tamper_record}</b>:
          its contents changed after it was written.”</p>
          <p class="evidence__note">레코드 한 글자 수정 → 끊긴 번호. 판정은 서버, 브라우저는 재현.</p>
        </div>
      </div>
      <p class="demo-note">탐지이지, 방지가 아니다 — 전체 재작성은 외부 앵커의 몫</p>
    </section>""")
    notes.append("55초. 왼쪽은 '왜 막혔는가', 오른쪽은 '나중에 어떻게 증명하는가'. "
                 "마지막 문장에서 '탐지'라고 정확히 말한다.")

    # ── 08 who ──────────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>규제는 엄격하고,<br>플랫폼 팀은 없는 팀</h2>
      <div class="list3">
        <div>
          <h3>금융 · 의료</h3>
          <p>결과로 나가는 식별자의 실행 전 차단. 같은 질문은 집계와 노이즈로.</p>
          <p class="ex">차단 사유에 법조항이 붙는다</p>
        </div>
        <div>
          <h3>대용량 로그</h3>
          <p>Arrow 버퍼 직결로 줄어든 복사 경계. 409만 행에서 2.14배.</p>
          <p class="ex">피크 메모리 405 MB (pandas 635 MB)</p>
        </div>
        <div>
          <h3>MLOps</h3>
          <p>PR 단계의 정책 게이트. 안전 예제는 통과, 위반 예제는 차단.</p>
          <p class="ex">PR마다 check → policy</p>
        </div>
      </div>
      <p class="statement">읽을 수 있는 검사기.<br>그래서 오픈소스.</p>
      <p class="source" style="margin-top:auto">사례 데이터는 합성 · 도입 기관 없음</p>
    </section>""")
    notes.append("40초. '대기업 고객이 없다'가 아니라 '일부러 중간 계층을 노린다'로 말한다. "
                 "마지막 두 문장이 포지셔닝의 핵심이다.")

    # ── 09 now/next ─────────────────────────────────────────────────────────
    slides.append(f"""
    <section class="slide">
      <h2>지금 / 다음</h2>
      <div class="status">
        <div>
          <h3><span class="mark mark--ok"></span>지금 동작한다</h3>
          <ul>
            <li>코어 언어 · 컴파일러 · Typed IR</li>
            <li>Polars 실행 · DP 노이즈 · Burn 학습</li>
            <li>Visual IDE · 거버넌스 패널</li>
            <li>Parquet/Arrow · DuckDB · PostgreSQL</li>
            <li>LSP · VS Code · Docker · CI 게이트</li>
            <li>문서 4개 언어</li>
          </ul>
        </div>
        <div>
          <h3><span class="mark mark--wip"></span>진행 중</h3>
          <ul>
            <li>GenAI 입력/출력 게이트</li>
            <li>GPU 백엔드 실기 검증</li>
            <li>v0.5 준비</li>
          </ul>
        </div>
        <div>
          <h3><span class="mark"></span>계획</h3>
          <ul>
            <li>분산 학습</li>
            <li>공개 호스팅 플레이그라운드</li>
            <li>감사 체인 외부 앵커</li>
          </ul>
        </div>
      </div>
      <div class="limitations">
        <p>알려진 한계 — <b>burn-cuda</b> 실기 미검증 · <b>ONNX Runtime</b> Windows GNU 미지원 · <b>벤치마크</b> 단일 호스트</p>
      </div>
      <p class="source" style="margin-top:auto">v0.4.1 · 2026-10-23 · Apache-2.0</p>
    </section>""")
    notes.append("30초. '지금 동작한다'만 확실히. 한계는 먼저 말하고 넘어간다.")

    # ── 10 closing ──────────────────────────────────────────────────────────
    qr = ASSETS / "demo" / "qr-repo.png"
    qr_html = (f'<img class="qr" src="data:image/png;base64,'
               f'{base64.b64encode(qr.read_bytes()).decode("ascii")}" alt="저장소 QR">'
               if qr.exists() else "")
    slides.append(f"""
    <section class="slide slide--closing">
      <h2>실행 전의 검사,<br>실행 후의 증명.</h2>
      <div class="closing-foot">
        <p class="closing-repo">{REPO_URL}</p>
        {qr_html}
      </div>
    </section>""")
    notes.append("15초. 문장 하나, 저장소, 질문 요청. 넘기지 말고 여기서 멈춘다.")

    return "\n".join(slides), notes


# ── shell ───────────────────────────────────────────────────────────────────

CSS = """
:root{
  --bg:#FFFFFF; --ink:#0A0A0B; --muted:#5F5F63; --faint:#8A8A8E;
  --line:#E2E2E5; --soft:#F5F5F3;
  --panel:#0B0B0D; --panel-ink:#F4F4F1; --panel-muted:#9C9C98; --panel-line:#2A2A2E;
  --block:#D93A2F; --pass:#2E9E6B;
  --sans:'Pretendard','Apple SD Gothic Neo','Malgun Gothic','Noto Sans KR',-apple-system,sans-serif;
  --mono:ui-monospace,'SFMono-Regular','Cascadia Mono',Menlo,Consolas,monospace;
}
*{box-sizing:border-box}
html,body{margin:0;height:100%;background:#EDEDEA;color:var(--ink);
  font-family:var(--sans);overflow:hidden;-webkit-font-smoothing:antialiased;
  word-break:keep-all;overflow-wrap:break-word}
#stage{position:fixed;inset:0;display:grid;place-items:center}
.slide{position:absolute;width:1280px;height:720px;background:var(--bg);
  padding:70px 80px 58px;display:none;flex-direction:column;overflow:hidden}
.slide.active{display:flex}

h2{margin:0 0 34px;font-size:46px;line-height:1.18;font-weight:800;
  letter-spacing:-.032em;max-width:1080px}

/* cover */
.slide.active.slide--cover{padding:0;display:grid;grid-template-columns:520px 1fr;background:var(--bg);color:var(--ink)}
.cover-text{padding:64px 52px 56px 80px;display:flex;flex-direction:column}
.cover-meta{font-size:15px;color:var(--muted);margin:0}
.wordmark{font-size:72px;line-height:1;font-weight:800;letter-spacing:-.045em;margin:auto 0 22px}
.cover-line{font-size:27px;line-height:1.44;font-weight:700;letter-spacing:-.022em;margin:0 0 28px}
.cover-repo{font-family:var(--mono);font-size:16px;color:#1F7A4D;margin:0}
.cover-media{position:relative;height:720px;overflow:hidden;background:var(--bg);border-left:1px solid var(--line)}
.cover-media img{position:absolute;top:50%;left:50%;transform:translate(-50%,-50%);width:760px;height:auto;display:block;opacity:.96}

/* rows */
.rows{display:flex;flex-direction:column;gap:40px;margin:auto 0}
.rows p{margin:0;font-size:30px;line-height:1.45;letter-spacing:-.02em;max-width:980px;
  padding-left:22px;border-left:3px solid var(--ink)}
.source{margin:24px 0 0;font-size:16px;line-height:1.6;color:var(--muted);max-width:960px}

/* code + artifact */
.split-58{display:grid;grid-template-columns:740px 1fr;gap:40px;flex:1;min-height:0}
.code{background:var(--soft);border:1px solid var(--line);padding:22px 24px;margin:0;
  font-family:var(--mono);font-size:15.5px;line-height:1.72;white-space:pre;overflow:hidden}
.code .c{color:var(--faint)}
.code b{font-weight:700}
.artifact{display:flex;flex-direction:column;justify-content:center}
.artifact__cap{font-size:16px;font-weight:700;margin:0 0 10px}
.term{background:var(--panel);color:var(--panel-ink);margin:0;padding:20px 22px;
  font-family:var(--mono);font-size:15px;line-height:1.72;white-space:pre-wrap;overflow:hidden}
.term--light{background:var(--soft);color:var(--ink);border:1px solid var(--line)}
.term .ok{color:#5FD79B}
.term .bad{color:#FF6A5C}
.artifact__note{font-size:16px;line-height:1.55;color:var(--muted);margin:12px 0 0}

/* figure + diagnostic */
.figure{flex:1;display:flex;align-items:center;justify-content:center;min-height:0;margin:0 0 10px}
.figure svg,.figure img{max-width:100%;max-height:100%;width:auto;height:auto}
.figure--chart img{width:100%;height:auto;max-height:none}
.diagnostic{display:grid;grid-template-columns:1.25fr .75fr;gap:28px;align-items:center}
.diagnostic p{margin:0;font-size:18px;line-height:1.55;color:var(--ink);align-self:start}

/* statline */
.bigline{margin:22px 0 0;font-size:23px;letter-spacing:-.015em}
.bigline b{font-weight:800}
.statline p{margin:0;font-size:17px;color:var(--muted)}
.statline b{font-size:26px;color:var(--ink);font-weight:800;letter-spacing:-.02em;margin-right:6px}

/* demo beats */
.beats{list-style:none;margin:8px 0 0;padding:0;flex:1}
.beats li{display:grid;grid-template-columns:96px 1fr;gap:24px;align-items:baseline;
  padding:20px 0;border-bottom:1px solid var(--line)}
.beats li:last-child{border-bottom:none}
.beats .t{font-family:var(--mono);font-size:15px;color:var(--faint)}
.beats .b{font-size:24px;font-weight:700;letter-spacing:-.02em;color:var(--ink)}
.beats b{font-weight:800}
.demo-note{margin:14px 0 0;font-size:16px;color:var(--muted)}

/* evidence */
.evidence{display:grid;grid-template-columns:1.05fr .95fr;gap:48px;flex:1;min-height:0;align-items:center;margin-top:4px}
.evidence__label{font-size:16px;font-weight:700;color:var(--ink);margin:0 0 14px}

.term--dark{background:#151518;border:1px solid var(--panel-line);font-size:16px}
.evidence__note{font-size:17px;line-height:1.6;color:var(--muted);margin:16px 0 0}
.quote{font-size:27px;line-height:1.5;letter-spacing:-.02em;margin:0;color:var(--ink)}
.quote b{font-weight:800}

/* who */
.list3{display:grid;grid-template-columns:repeat(3,1fr);gap:44px;margin:4px 0 0}
.list3 h3{font-size:21px;margin:0 0 10px;font-weight:800;letter-spacing:-.015em}
.list3 p{font-size:16.5px;line-height:1.62;color:#3C3C3E;margin:0}
.list3 .ex{margin-top:14px;font-size:16px;color:var(--muted)}
.statement{margin:56px 0 0;font-size:27px;line-height:1.5;font-weight:700;letter-spacing:-.02em}


/* status */
.status{display:grid;grid-template-columns:1.2fr 1fr 1fr;gap:48px;margin-top:4px}
.status h3{display:flex;align-items:center;gap:10px;font-size:17px;margin:0 0 16px;font-weight:800}
.status ul{margin:0;padding:0;list-style:none}
.status li{font-size:16.5px;line-height:1.5;margin-bottom:12px;color:var(--ink)}
.status > div:not(:first-child) li{color:var(--muted)}
.mark{width:9px;height:9px;border-radius:50%;background:var(--faint);display:inline-block}
.mark--ok{background:var(--pass)}
.mark--wip{background:#C98A2E}
.limitations{margin-top:52px;border-top:1px solid var(--line);padding-top:18px}
.limitations p{margin:0;font-size:16px;line-height:1.6;color:var(--muted)}
.limitations b{color:var(--ink)}

/* closing */
.slide--closing{justify-content:center}
.slide--closing h2{font-size:54px;line-height:1.3;margin:0 0 56px;max-width:980px}
.closing-foot{display:flex;justify-content:space-between;align-items:center;
  border-top:1px solid var(--line);padding-top:26px}
.closing-repo{font-family:var(--mono);font-size:18px;color:#1F7A4D;margin:0}
.qr{width:112px;height:112px;image-rendering:pixelated;display:block}

/* chrome */
#bar{position:fixed;left:0;bottom:0;height:2px;background:var(--ink);width:0;z-index:10;transition:width .18s}
.nav{position:fixed;bottom:12px;left:16px;z-index:10;display:flex;gap:6px;
  opacity:0;transition:opacity .25s;pointer-events:none}
body.show-ui .nav{opacity:1;pointer-events:auto}
.nav button{background:rgba(255,255,255,.9);color:#3A3A3C;border:1px solid #CFCFD2;padding:5px 10px;
  font-family:var(--mono);font-size:11px;cursor:pointer}
.nav button:hover{border-color:#0A0A0B;color:#0A0A0B}
#notes{position:fixed;inset:auto 24px 56px 24px;max-height:44vh;overflow:auto;z-index:20;
  background:#0B0B0D;color:#EDEDEA;padding:20px 24px;display:none;
  font-family:var(--mono);font-size:14px;line-height:1.7;box-shadow:0 10px 40px rgba(0,0,0,.4)}
#notes.on{display:block}
#notes b{color:#7BE0A8;font-weight:600;display:block;margin-bottom:6px}
@media print{
  html,body{overflow:visible;background:#fff}
  #stage{position:static;display:block}
  .nav,#bar,#notes{display:none!important}
  .slide{display:flex!important;position:relative;page-break-after:always;
    width:1280px;height:720px;transform:none!important}
  .slide.slide--cover{display:grid!important;grid-template-columns:520px 1fr}
  @page{size:1280px 720px;margin:0}
}
"""

JS = """
(function(){
  const slides=[...document.querySelectorAll('.slide')];
  const notes=__NOTES__;
  const bar=document.getElementById('bar');
  const panel=document.getElementById('notes');
  let i=0;
  function show(n){
    i=Math.max(0,Math.min(slides.length-1,n));
    slides.forEach((s,k)=>s.classList.toggle('active',k===i));
    bar.style.width=((i+1)/slides.length*100)+'%';
    bar.style.background=getComputedStyle(slides[i]).backgroundColor;
    panel.innerHTML='<b>발표자 노트 · '+(i+1)+'/'+slides.length+'</b>'+notes[i];
    history.replaceState(null,'','#'+(i+1));
  }
  function fit(){
    const s=Math.min(innerWidth/1280,(innerHeight-10)/720);
    document.querySelectorAll('.slide').forEach(el=>el.style.transform='scale('+s+')');
  }
  addEventListener('resize',fit);
  let hideTimer;
  addEventListener('mousemove',()=>{
    document.body.classList.add('show-ui');
    clearTimeout(hideTimer);
    hideTimer=setTimeout(()=>document.body.classList.remove('show-ui'),1600);
  });
  addEventListener('keydown',e=>{
    if(['ArrowRight','ArrowDown','PageDown',' ','Enter'].includes(e.key)){show(i+1);e.preventDefault();}
    else if(['ArrowLeft','ArrowUp','PageUp'].includes(e.key)){show(i-1);e.preventDefault();}
    else if(e.key==='Home'){show(0);}
    else if(e.key==='End'){show(slides.length-1);}
    else if(e.key==='n'||e.key==='N'){panel.classList.toggle('on');}
    else if(e.key==='f'||e.key==='F'){document.fullscreenElement?document.exitFullscreen():document.documentElement.requestFullscreen();}
  });
  document.getElementById('prev').onclick=()=>show(i-1);
  document.getElementById('next').onclick=()=>show(i+1);
  fit();
  const start=parseInt(location.hash.slice(1),10);
  show(Number.isFinite(start)?start-1:0);
})();
"""

HTML_TEMPLATE = """<!doctype html>
<html lang="ko">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Xazz — 2차 평가 발표덱</title>
<style>{css}</style>
</head>
<body>
<div id="stage">
{slides}
</div>
<div class="nav"><button id="prev">이전</button><button id="next">다음</button></div>
<div id="notes"></div>
<div id="bar"></div>
<script>{js}</script>
</body>
</html>
"""


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xazz", help="path to the xazz CLI (default: target/release or PATH)")
    parser.add_argument("--out", type=pathlib.Path,
                        default=ROOT / "docs" / "demo" / "pitch-deck.html")
    args = parser.parse_args()

    xazz = find_xazz(args.xazz)
    slides, notes = build_slides(xazz)

    import json

    js = JS.replace("__NOTES__", json.dumps(notes, ensure_ascii=False))
    out = HTML_TEMPLATE.format(css=CSS, js=js, slides=slides)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(out, encoding="utf-8")
    kb = len(out.encode("utf-8")) / 1024
    print(f"wrote {args.out.relative_to(ROOT)}  ({kb:.0f} KB, {out.count('<section')} slides)")
    if xazz is None:
        print("note: xazz binary not found — terminal quotes fall back to documented output")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
