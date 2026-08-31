#!/usr/bin/env python3
"""demo/make_screenshots.py — README 터미널 스크린샷 생성기

`demo/capture.sh` 가 남긴 실제 실행 출력(demo/_capture/*.txt)을
터미널 창 스타일 PNG로 렌더링해 docs/assets/ 아래에 쓴다.

화면의 모든 글자는 실제 명령 출력이다 — 손으로 쓴 가짜 출력은 없다.

    bash demo/capture.sh               # 먼저 캡처 갱신
    python3 demo/make_screenshots.py   # → docs/assets/demo_*.png

의존성: pillow (pip install --user --break-system-packages pillow)
"""

from __future__ import annotations

import pathlib
import re

from PIL import Image, ImageDraw, ImageFont

ROOT = pathlib.Path(__file__).resolve().parent.parent
CAPTURE = ROOT / "demo" / "_capture"
OUT = ROOT / "docs" / "assets"

FONT_PATH = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
FONT_BOLD = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono-Bold.ttf"
FONT_SIZE = 16
LINE_H = 24
PAD_X = 30
PAD_TOP = 58  # title bar 포함
TITLE_H = 38

BG = (13, 17, 23)          # #0d1117
TITLE_BG = (33, 38, 45)    # #21262d
FG = (201, 209, 217)       # #c9d1d9
DIM = (139, 148, 158)      # #8b949e
GREEN = (63, 185, 80)      # #3fb950
RED = (248, 81, 73)        # #f85149
YELLOW = (210, 153, 34)    # #d29922
BLUE = (88, 166, 255)      # #58a6ff
DOT_COLORS = [(255, 95, 86), (255, 189, 46), (39, 201, 63)]

# DejaVu Sans Mono 가 없는 이모지를 텍스트 기호로 치환
CHAR_MAP = {
    "✅": "✓",
    "❌": "✗",
    "⚠️": "⚠",
    "⚠": "⚠",
    "💡": "▸",
    "📊": "",
    "🧠": "",
    "💾": "",
    "🚀": "",
    "⚡": "",
}

# 스크린샷에 부적합한 기계 판독용 JSON 마커 라인
JSON_MARKERS = (
    "[xazz:policy]",
    "[xazz:chart]",
    "[xazz:result]",
    "[xazz:dp]",
    "[xazz:train]",
    "[xazz:model]",
    "[xazz:diagnostics]",
)


def sanitize(text: str) -> str:
    for src, dst in CHAR_MAP.items():
        text = text.replace(src, dst)
    return text.rstrip()


def load_capture(name: str, drop_json: bool = True) -> list[str]:
    raw = (CAPTURE / f"{name}.txt").read_text(encoding="utf-8").splitlines()
    lines: list[str] = []
    for ln in raw:
        t = ln.strip()
        if drop_json and any(t.startswith(m) for m in JSON_MARKERS):
            continue
        # 마커 직후의 한 줄 JSON 덤프 제거
        if drop_json and t.startswith("{") and len(t) > 120:
            continue
        lines.append(sanitize(ln))
    while lines and not lines[-1].strip():
        lines.pop()
    return lines


def line_slice(lines: list[str], start_marker: str, end_marker: str) -> list[str]:
    """start_marker 를 포함하는 첫 줄부터 end_marker 를 포함하는 줄까지 자른다."""
    s = next(i for i, ln in enumerate(lines) if start_marker in ln)
    e = next(i for i, ln in enumerate(lines) if end_marker in ln)
    return lines[s : e + 1]


def line_color(ln: str) -> tuple[int, int, int]:
    t = ln.strip()
    if t and set(t) <= {"─"}:
        return DIM
    if t.startswith(("┌", "│", "└", "╞", "═")):
        return DIM
    if "✓" in ln or "passed" in ln:
        return GREEN
    if "✗" in ln or "[error" in ln or "execution blocked" in ln:
        return RED
    if "⚠" in ln or "warning" in ln.lower() or "▸" in ln:
        return YELLOW
    if "[xazz Execution Result" in ln or "Model Declaration" in ln:
        return BLUE
    return FG


def render(
    lines: list[str],
    out_path: pathlib.Path,
    title: str,
    bold_markers: tuple[str, ...] = (),
) -> None:
    font = ImageFont.truetype(FONT_PATH, FONT_SIZE)
    font_b = ImageFont.truetype(FONT_BOLD, FONT_SIZE)
    char_w = font.getbbox("M")[2]

    content = [ln.rstrip() for ln in lines]
    max_len = max((len(ln) for ln in content), default=20)
    width = min(1400, max(760, max_len * char_w + PAD_X * 2))
    height = PAD_TOP + len(content) * LINE_H + 26

    img = Image.new("RGB", (width, height), BG)
    draw = ImageDraw.Draw(img)

    # ── 타이틀 바 ────────────────────────────────────────────────────────────
    draw.rectangle([0, 0, width, TITLE_H], fill=TITLE_BG)
    for i, c in enumerate(DOT_COLORS):
        cx = 24 + i * 24
        draw.ellipse([cx - 7, TITLE_H // 2 - 7, cx + 7, TITLE_H // 2 + 7], fill=c)
    title_font = ImageFont.truetype(FONT_PATH, 13)
    tw = draw.textlength(title, font=title_font)
    draw.text(((width - tw) / 2, (TITLE_H - 13) / 2 - 1), title, font=title_font, fill=DIM)

    # ── 본문 ────────────────────────────────────────────────────────────────
    y = PAD_TOP
    for ln in content:
        color = line_color(ln)
        f = font
        stripped = ln.strip()
        if any(m in stripped for m in bold_markers):
            f = font_b
        draw.text((PAD_X, y), ln, font=f, fill=color)
        y += LINE_H

    img.save(out_path)
    print(f"  {out_path.relative_to(ROOT)}  ({width}×{height}, {len(content)} lines)")


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)

    # 1. 오타 감지 — did-you-mean 제안 (demo_check.png)
    render(
        load_capture("check_typo"),
        OUT / "demo_check.png",
        "xazz check — typo caught before execution, with did-you-mean",
    )

    # 2. Polars 전처리 + 차트 (demo_preprocess.png)
    render(
        load_capture("preprocess"),
        OUT / "demo_preprocess.png",
        "xazz run demo/preprocess_chart.xzz",
        bold_markers=("[xazz Execution Result",),
    )

    # 3. Burn 학습 — 모델 선언부터 체크포인트까지 (demo_training.png)
    train = load_capture("train")
    render(
        line_slice(train, "Model Declaration", "checkpoint"),
        OUT / "demo_training.png",
        "xazz run demo/deep_learning.xzz",
        bold_markers=("training complete", "Model Declaration"),
    )

    # 4. 차등 프라이버시 (demo_dp.png)
    render(
        load_capture("dp"),
        OUT / "demo_dp.png",
        "xazz run demo/dp.xzz",
        bold_markers=("[xazz Execution Result", "DP applied"),
    )


if __name__ == "__main__":
    main()