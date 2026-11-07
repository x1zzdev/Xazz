#!/usr/bin/env python3
"""scripts/make_pitch_deck_pptx.py — 발표덱 PPTX 생성기 (편집 가능한 네이티브 버전)

`docs/demo/pitch-deck.html` 과 같은 원고를 PowerPoint 파일로 옮긴다. 이미지 캡처가 아니라
**텍스트 상자·도형·이미지**로 만들기 때문에 발표장에서 바로 고칠 수 있다.

  슬라이드 13.333 × 7.5 in (16:9), 여백 80px 기준
  글꼴  맑은 고딕 / 코드 Consolas
  배경  전부 흰색, 색은 의미로만 (빨강=차단, 초록=통과)

준비:
    python3 -m pip install --target .venv-pptx/lib python-pptx
실행:
    PYTHONPATH=.venv-pptx/lib python3 scripts/make_pitch_deck_pptx.py
"""

from __future__ import annotations

import importlib.util
import pathlib
import sys

from pptx import Presentation
from pptx.dml.color import RGBColor
from pptx.enum.shapes import MSO_SHAPE
from pptx.enum.text import MSO_ANCHOR, PP_ALIGN
from pptx.oxml.ns import qn
from pptx.util import Emu, Pt

ROOT = pathlib.Path(__file__).resolve().parents[1]
ASSETS = ROOT / "docs" / "assets"
OUT = ROOT / "docs" / "demo" / "pitch-deck.pptx"

# ── palette / type ───────────────────────────────────────────────────────────
INK = RGBColor(0x0A, 0x0A, 0x0B)
MUTED = RGBColor(0x5F, 0x5F, 0x63)
FAINT = RGBColor(0x8A, 0x8A, 0x8E)
LINE = RGBColor(0xE2, 0xE2, 0xE5)
SOFT = RGBColor(0xF5, 0xF5, 0xF3)
WHITE = RGBColor(0xFF, 0xFF, 0xFF)
PANEL = RGBColor(0x0B, 0x0B, 0x0D)
PANEL_INK = RGBColor(0xF4, 0xF4, 0xF1)
GREEN = RGBColor(0x2E, 0x9E, 0x6B)
GREEN_DARK = RGBColor(0x1F, 0x7A, 0x4D)
RED = RGBColor(0xD9, 0x3A, 0x2F)

SANS = "맑은 고딕"
MONO = "Consolas"
PX = 9525  # 1 px → EMU (96 dpi)


def emu(px: float) -> Emu:
    return Emu(int(round(px * PX)))


def load_html_deck():
    spec = importlib.util.spec_from_file_location("mpd", ROOT / "scripts" / "make_pitch_deck.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)  # type: ignore[union-attr]
    return mod


def set_run(run, text, size_px, *, bold=False, color=INK, font=SANS):
    run.text = text
    f = run.font
    f.size = Pt(size_px * 0.75)
    f.bold = bold
    f.color.rgb = color
    f.name = font
    rpr = run._r.get_or_add_rPr()
    ea = rpr.find(qn("a:ea"))
    if ea is None:
        ea = rpr.makeelement(qn("a:ea"), {})
        rpr.append(ea)
    ea.set("typeface", SANS if font == MONO else font)


def textbox(slide, x, y, w, h, paragraphs, *, spacing=1.0, space_after=0,
            align=PP_ALIGN.LEFT, anchor=MSO_ANCHOR.TOP, margins=(0, 0, 0, 0)):
    box = slide.shapes.add_textbox(emu(x), emu(y), emu(w), emu(h))
    tf = box.text_frame
    tf.word_wrap = True
    tf.margin_left, tf.margin_right, tf.margin_top, tf.margin_bottom = (emu(m) for m in margins)
    tf.vertical_anchor = anchor
    first = True
    for para in paragraphs:
        p = tf.paragraphs[0] if first else tf.add_paragraph()
        first = False
        p.alignment = align
        p.line_spacing = spacing
        p.space_after = Pt(space_after)
        for run_spec in para:
            run = p.add_run()
            set_run(run, **run_spec)
    return box


def rect(slide, x, y, w, h, fill, line=None, width=1):
    shp = slide.shapes.add_shape(MSO_SHAPE.RECTANGLE, emu(x), emu(y), emu(w), emu(h))
    shp.shadow.inherit = False
    if fill is None:
        shp.fill.background()
    else:
        shp.fill.solid()
        shp.fill.fore_color.rgb = fill
    if line is None:
        shp.line.fill.background()
    else:
        shp.line.color.rgb = line
        shp.line.width = Pt(width)
    return shp


def hline(slide, x, y, w, color=LINE, thickness=1):
    return rect(slide, x, y, w, thickness, color)


def slide_frame(prs):
    s = prs.slides.add_slide(prs.slide_layouts[6])
    s.background.fill.solid()
    s.background.fill.fore_color.rgb = WHITE
    return s


def h2(slide, text_lines, y=70, size=46, width=1120, color=INK):
    paras = [[{"text": line, "size_px": size, "bold": True, "color": color}] for line in text_lines]
    textbox(slide, 80, y, width, size * 1.35 * len(text_lines) + 12, paras, spacing=1.08)


# ── content ──────────────────────────────────────────────────────────────────

CODE_LINES = [
    ('// examples/field_cases/finance_fds/fds_safe.xzz', True),
    ('v txn_by_category = load("data/transactions.csv") :: Transaction', False),
    ('    |> groupBy("merchant_category")', False),
    ('    |> agg([count("is_fraud"), sum("is_fraud")])', False),
    ('    |> orderBy("merchant_category", desc: true);', False),
    ('', False),
    ('v amount_by_category = load("data/transactions.csv") :: Transaction', False),
    ('    |> groupBy("merchant_category")', False),
    ('    |> mean("transaction_amount")', False),
    ('    |> withDp(epsilon: 1.0, mechanism: laplace, sensitivity: 1.0)', False),
    ('    |> orderBy("merchant_category");', False),
    ('', False),
    ('v fraud_by_band = load("data/transactions.csv") :: Transaction', False),
    ('    |> groupBy("age_band")', False),
    ('    |> agg([count("is_fraud"), sum("is_fraud")])', False),
]


def build() -> pathlib.Path:
    deck = load_html_deck()
    xazz = deck.find_xazz(None)
    _, notes = deck.build_slides(xazz)

    # Real artifacts (same capture path as the HTML deck).
    diag_err = diag_hint = ""
    if xazz is not None:
        import tempfile

        with tempfile.TemporaryDirectory() as td:
            f = pathlib.Path(td) / "typo.xzz"
            f.write_text('type Air = { temperature_c: float }\n'
                         'v raw = load("air.csv") :: Air\n'
                         'v warm = raw |> filter(temperture_c > 20)\n', encoding="utf-8")
            lines = deck.run_capture(xazz, pathlib.Path(td), ["check", "typo.xzz"])
        diag_err = deck.first(lines, "[error")
        diag_hint = deck.first(lines, "Did you mean")
    diag_err = diag_err or "× [error [line 3: col 24]] 표현식: 스키마에 'temperture_c' 컬럼이 존재하지 않습니다."
    diag_hint = diag_hint or '사용 가능한 컬럼: temperature_c · Did you mean: col("temperature_c")?'

    blocked = deck.run_capture(xazz, ROOT / "examples" / "field_cases" / "finance_fds",
                               ["run", "fds_blocked.xzz"])
    block_head = deck.first(blocked, "execution blocked", "[xazz security guardrail] execution blocked")
    xzp = next((i for i, l in enumerate(blocked) if "XZP001" in l), None)
    block_lines = ["$ xazz run fds_blocked.xzz", block_head]
    if xzp is not None:
        block_lines.append(blocked[xzp].strip())

    tamper_meta = ASSETS / "demo" / "audit-tamper.json"
    record = "15"
    if tamper_meta.exists():
        import json

        record = str(json.loads(tamper_meta.read_text(encoding="utf-8")).get("record", "15"))

    prs = Presentation()
    prs.slide_width = emu(1280)
    prs.slide_height = emu(720)
    props = prs.core_properties
    props.title = "Xazz — 2차 평가 발표"
    props.subject = "AI 파이프라인의 실행 전 검사와 실행 후 증명"
    props.author = "x1zz"
    props.keywords = "Xazz, DSL, compiler, governance, differential privacy, audit" 

    # ── 1 cover ─────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    textbox(s, 80, 70, 420, 30, [[{"text": "2026 오픈소스 개발자대회 · 2차 평가",
                                   "size_px": 15, "color": MUTED}]])
    textbox(s, 80, 330, 420, 90, [[{"text": "Xazz", "size_px": 72, "bold": True}]])
    textbox(s, 80, 440, 420, 130, [
        [{"text": "AI 파이프라인의", "size_px": 27, "bold": True}],
        [{"text": "실행 전 검사,", "size_px": 27, "bold": True}],
        [{"text": "실행 후 증명.", "size_px": 27, "bold": True}],
    ], spacing=1.18)
    textbox(s, 80, 640, 420, 30, [[{"text": "github.com/x1zzdev/Xazz", "size_px": 16,
                                    "color": GREEN_DARK, "font": MONO}]])
    s.shapes.add_picture(str(ASSETS / "ide_workspace.png"), emu(520), emu(106), emu(760), emu(507))
    rect(s, 520, 0, 1, 720, LINE)

    # ── 2 problem ───────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["데이터가 흐른 뒤에 드러나는 오류"])
    rows = ["스키마와 코드가 어긋난 채 시작되는 학습",
            "경계마다 반복되는 복사, 학습 시간에 숨는 비용",
            "코드 밖에 남지 않는 실행의 근거"]
    y = 300
    for row in rows:
        rect(s, 80, y, 3, 44, INK)
        textbox(s, 104, y, 980, 50, [[{"text": row, "size_px": 30, "bold": False}]])
        y += 100
    textbox(s, 80, 610, 1040, 70, [[
        {"text": "Python 저장소 210개를 분석한 연구에서, 결함의 15%는 정적 타입 검사기로 막을 수 있었다. "
                 "저자들은 이 값을 하한이라고 밝혔다. — Khan et al., IEEE TSE 2021",
         "size_px": 16, "color": MUTED}]])

    # ── 3 code ──────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["코드에 적힌 스키마와 프라이버시"])
    rect(s, 80, 190, 740, 480, SOFT, LINE)
    code_paras = []
    for text, is_comment in CODE_LINES:
        code_paras.append([{"text": text if text else " ",
                            "size_px": 15.5, "color": FAINT if is_comment else INK, "font": MONO}])
    textbox(s, 96, 208, 708, 448, code_paras, spacing=1.34)
    textbox(s, 860, 210, 340, 30, [[{"text": "이 파일의 실제 실행 출력", "size_px": 16, "bold": True}]])
    rect(s, 860, 246, 340, 168, PANEL)
    out = ["정책 검사 통과 — 위반 없음",
           "DP 적용: laplace · ε=1 · Δf=1",
           "shape: (7, 2)",
           "4 AST · 1 types · 3 pipelines"]
    out_paras = [[{"text": body, "size_px": 15, "color": GREEN, "font": MONO}] for body in out]
    textbox(s, 876, 262, 308, 140, out_paras, spacing=1.5)
    textbox(s, 860, 438, 340, 80, [[{"text": "스키마 선언·집계·노이즈 주입이 한 파일 안에서",
                                     "size_px": 16, "color": MUTED}]])

    # ── 4 gates ─────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["라이브러리가 아닌,", "파이프라인의 연산"])
    # stations
    textbox(s, 176, 300, 200, 60, [[{"text": ".xzz", "size_px": 34, "bold": True}]])
    textbox(s, 463, 300, 220, 60, [[{"text": "Typed IR", "size_px": 34, "bold": True}]], align=PP_ALIGN.CENTER)
    textbox(s, 886, 300, 220, 60, [[{"text": "결과", "size_px": 34, "bold": True}]], align=PP_ALIGN.CENTER)
    # connectors
    hline(s, 276, 328, 234, LINE, 2)
    hline(s, 636, 328, 234, LINE, 2)
    # gates
    rect(s, 396, 258, 3, 86, RED)
    textbox(s, 298, 222, 200, 34, [[{"text": "검사", "size_px": 17, "bold": True, "color": RED}]],
            align=PP_ALIGN.CENTER)
    textbox(s, 288, 370, 220, 30, [[{"text": "스키마 · 타입 · 정책", "size_px": 15, "color": MUTED}]],
            align=PP_ALIGN.CENTER)
    rect(s, 756, 258, 3, 86, GREEN)
    textbox(s, 658, 222, 200, 34, [[{"text": "실행", "size_px": 17, "bold": True, "color": GREEN}]],
            align=PP_ALIGN.CENTER)
    textbox(s, 648, 370, 220, 30, [[{"text": "Polars · DP · Burn", "size_px": 15, "color": MUTED}]],
            align=PP_ALIGN.CENTER)
    # diagnostic
    rect(s, 80, 500, 676, 128, SOFT, LINE)
    clean = lambda t: t.replace("❌", "×").replace("💡", "").strip()
    textbox(s, 96, 514, 644, 100, [
        [{"text": deck.trim(clean(diag_err), 74), "size_px": 15, "color": RED, "font": MONO}],
        [{"text": deck.trim(clean(diag_hint), 74), "size_px": 15, "color": MUTED, "font": MONO}],
    ], spacing=1.5)
    textbox(s, 790, 520, 410, 90, [[{"text": "컬럼 하나가 틀리면 — 데이터를 읽기 전에, 3행 24열에서",
                                     "size_px": 18, "bold": False}]])

    # ── 5 performance ───────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["pandas 대비 2.04 · 3.84 · 2.14배"])
    s.shapes.add_picture(str(ASSETS / "benchmark_chart.png"), emu(80), emu(180), emu(1120), emu(355))
    textbox(s, 80, 570, 1120, 40, [[
        {"text": "409만 행 피크 메모리 ", "size_px": 23},
        {"text": "405 MB", "size_px": 23, "bold": True},
        {"text": " — pandas ", "size_px": 23},
        {"text": "635 MB", "size_px": 23, "bold": True}]])
    textbox(s, 80, 625, 1120, 30, [[{"text": "Windows 11 한 대 · release 빌드 · 3회 중앙값 · 파이프라인 실행 시간만",
                                     "size_px": 16, "color": MUTED}]])

    # ── 6 video ─────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["영상 시연 — 2분 41초"])
    beats = [("0:00", "그래프와 코드, 같은 파일"), ("0:18", "Full Run과 실행 영수증"),
             ("0:30", "DP 원장과 학습 리포트"), ("1:05", "감사 체인의 변조 탐지"),
             ("1:58", "전화번호 차단 — 실행 전"), ("2:30", "CSV 스키마 추론")]
    y = 210
    for t, label in beats:
        textbox(s, 80, y + 6, 96, 30, [[{"text": t, "size_px": 15, "color": FAINT, "font": MONO}]])
        textbox(s, 200, y, 900, 44, [[{"text": label, "size_px": 24, "bold": True}]])
        hline(s, 80, y + 58, 1120, LINE)
        y += 64
    textbox(s, 80, 648, 1040, 30, [[{"text": "실제 서버 응답을 녹화한 영상 · 자막 포함 · 소리 없이 재생",
                                     "size_px": 16, "color": MUTED}]])

    # ── 7 evidence ──────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["실행 전의 차단, 실행 후의 증명"])
    textbox(s, 80, 220, 400, 30, [[{"text": "실행 전 · 차단", "size_px": 16, "bold": True}]])
    rect(s, 80, 256, 540, 140, PANEL)
    term_paras = []
    for line in block_lines:
        line = line.replace("✖", "×").replace("⛨", "")
        color = RGBColor(0xFF, 0x6A, 0x5C) if "×" in line else PANEL_INK
        term_paras.append([{"text": line, "size_px": 16, "color": color, "font": MONO}])
    textbox(s, 96, 270, 508, 112, term_paras, spacing=1.45)
    textbox(s, 80, 414, 540, 90, [[
        {"text": "고객번호·카드번호의 결과 반출 → 실행 전 차단. 사유에 신용정보법 제32조 · 개인정보 보호법 제24조.",
         "size_px": 17, "color": MUTED}]])
    textbox(s, 660, 220, 400, 30, [[{"text": "실행 후 · 변조 탐지", "size_px": 16, "bold": True}]])
    textbox(s, 660, 256, 560, 140, [
        [{"text": "“The chain breaks at record ", "size_px": 26},
         {"text": f"#{record}", "size_px": 26, "bold": True},
         {"text": ":", "size_px": 26}],
        [{"text": "its contents changed after it was written.”", "size_px": 26}],
    ], spacing=1.25)
    textbox(s, 660, 400, 540, 90, [[
        {"text": "레코드 한 글자 수정 → 끊긴 번호. 판정은 서버, 브라우저는 재현.",
         "size_px": 17, "color": MUTED}]])
    textbox(s, 80, 650, 1120, 30, [[
        {"text": "탐지이지, 방지가 아니다 — 전체 재작성은 외부 앵커의 몫",
         "size_px": 16, "color": MUTED}]])

    # ── 8 who ───────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["규제는 엄격하고,", "플랫폼 팀은 없는 팀"])
    cols = [
        ("금융 · 의료", "비식별화를 코드에 강제",
         "결과로 나가는 식별자를 실행 전에 차단하고, 같은 질문은 집계와 노이즈로 답한다.",
         "차단 사유에 법조항이 붙는다"),
        ("대용량 로그", "전처리 비용 절감",
         "Arrow 버퍼를 직접 읽어 복사 경계를 줄인다. 409만 행에서 2.14배.",
         "피크 메모리 405 MB (pandas 635 MB)"),
        ("MLOps", "PR 단계의 게이트",
         "CI에 같은 검사를 심는다. 안전한 파이프라인은 통과하고, 위반 예제는 차단된다.",
         "PR마다 check → policy"),
    ]
    x = 80
    for tag, title, body, ex in cols:
        textbox(s, x, 250, 330, 30, [[{"text": tag, "size_px": 14, "bold": True, "color": GREEN_DARK}]])
        textbox(s, x, 286, 330, 34, [[{"text": title, "size_px": 19, "bold": True}]])
        textbox(s, x, 326, 330, 76, [[{"text": body, "size_px": 16.5, "color": MUTED}]])
        textbox(s, x, 414, 330, 40, [[{"text": ex, "size_px": 16, "color": FAINT}]])
        x += 373
    textbox(s, 80, 510, 1000, 110, [
        [{"text": "읽을 수 있는 검사기.", "size_px": 27, "bold": True}],
        [{"text": "그래서 오픈소스.", "size_px": 27, "bold": True}],
    ], spacing=1.3)
    textbox(s, 80, 650, 1000, 30, [[{"text": "사례 데이터는 합성 · 도입 기관 없음",
                                     "size_px": 16, "color": MUTED}]])

    # ── 9 status ────────────────────────────────────────────────────────────
    s = slide_frame(prs)
    h2(s, ["지금 / 다음"])
    status = [
        ("지금 동작한다", GREEN, ["코어 언어 · 컴파일러 · Typed IR", "Polars 실행 · DP 노이즈 · Burn 학습",
                              "Visual IDE · 거버넌스 패널", "Parquet/Arrow · DuckDB · PostgreSQL",
                              "LSP · VS Code · Docker · CI 게이트", "문서 4개 언어"]),
        ("진행 중", RGBColor(0xC9, 0x8A, 0x2E), ["GenAI 입력/출력 게이트", "GPU 백엔드 실기 검증", "v0.5 준비"]),
        ("계획", FAINT, ["분산 학습", "공개 호스팅 플레이그라운드", "감사 체인 외부 앵커"]),
    ]
    x = 80
    for title, dot, items in status:
        rect(s, x, 208, 9, 9, dot)
        textbox(s, x + 20, 198, 300, 30, [[{"text": title, "size_px": 17, "bold": True}]])
        paras = [[{"text": item, "size_px": 16.5, "color": INK if dot == GREEN else MUTED}] for item in items]
        textbox(s, x, 240, 330, 300, paras, spacing=1.2, space_after=10)
        x += 373
    hline(s, 80, 588, 1120, LINE)
    textbox(s, 80, 604, 1120, 40, [[
        {"text": "알려진 한계 — ", "size_px": 16, "color": MUTED},
        {"text": "burn-cuda", "size_px": 16, "bold": True},
        {"text": " 실기 미검증 · ", "size_px": 16, "color": MUTED},
        {"text": "ONNX Runtime", "size_px": 16, "bold": True},
        {"text": " Windows GNU 미지원 · ", "size_px": 16, "color": MUTED},
        {"text": "벤치마크", "size_px": 16, "bold": True},
        {"text": " 단일 호스트", "size_px": 16, "color": MUTED}]])
    textbox(s, 80, 660, 900, 30, [[{"text": "v0.4.1 · 2026-10-23 · Apache-2.0",
                                     "size_px": 16, "color": MUTED}]])

    # ── 10 closing ──────────────────────────────────────────────────────────
    s = slide_frame(prs)
    textbox(s, 80, 190, 900, 220, [
        [{"text": "실행 전의 검사,", "size_px": 54, "bold": True}],
        [{"text": "실행 후의 증명.", "size_px": 54, "bold": True}],
    ], spacing=1.28)
    hline(s, 80, 470, 1120, LINE)
    textbox(s, 80, 500, 600, 40, [[{"text": "github.com/x1zzdev/Xazz", "size_px": 18,
                                    "color": GREEN_DARK, "font": MONO}]])
    qr = ASSETS / "demo" / "qr-repo.png"
    if qr.exists():
        s.shapes.add_picture(str(qr), emu(1086), emu(496), emu(104), emu(104))

    # ── speaker notes ───────────────────────────────────────────────────────
    for slide, note in zip(prs.slides, notes):
        slide.notes_slide.notes_text_frame.text = note

    OUT.parent.mkdir(parents=True, exist_ok=True)
    prs.save(str(OUT))
    return OUT


def main() -> int:
    try:
        out = build()
    except ModuleNotFoundError as e:  # pragma: no cover
        print(f"missing dependency: {e}. run: python3 -m pip install --target .venv-pptx/lib python-pptx",
              file=sys.stderr)
        return 1
    print(f"wrote {out.relative_to(ROOT)}  ({out.stat().st_size / 1024:.0f} KB)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
