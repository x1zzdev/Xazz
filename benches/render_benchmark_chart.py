"""
benches/render_benchmark_chart.py — 벤치마크 결과 차트 렌더러
=============================================================
benches/benchmark_results.json 을 읽어 README·발표덱용 PNG 차트를 생성한다.

  왼쪽: 스케일별 파이프라인 지연 시간 (log scale, 직접 라벨)
  오른쪽: pandas 대비 속도향상 (배수)

디자인 규칙
  * 발표덱과 같은 종이색 배경·잉크색 축·단일 강조색을 쓴다. (라이트 에디토리얼)
  * 범례 대신 선 끝 직접 라벨을 쓴다 — 색각 이상과 흑백 인쇄에서도 읽힌다.
  * 수치는 benchmark_results.json 에서만 읽는다. 하드코딩하지 않는다.
"""
from __future__ import annotations

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.ticker import FuncFormatter

ROOT = Path(__file__).parent.parent.resolve()
RESULTS = ROOT / "benches" / "benchmark_results.json"
OUT = ROOT / "docs" / "assets" / "benchmark_chart.png"

PAPER = "#FFFFFF"
INK = "#0A0A0B"
MUTED = "#5F5F63"
RULE = "#E2E2E5"
ACCENT = "#0A0A0B"
SLATE = "#B9B9BD"

plt.rcParams.update({
    "font.family": "DejaVu Sans",
    "font.size": 11,
    "axes.edgecolor": RULE,
    "axes.labelcolor": MUTED,
    "text.color": INK,
    "xtick.color": MUTED,
    "ytick.color": MUTED,
    "svg.fonttype": "none",
})

data = json.loads(RESULTS.read_text(encoding="utf-8"))
scales = ["small", "medium", "large"]
labels = ["228K", "912K", "4.09M"]

pd_lat = [data[s]["pandas"]["latency_ms"] for s in scales]
xz_lat = [data[s]["xazz"]["latency_ms"] for s in scales]
speedup = [p / x for p, x in zip(pd_lat, xz_lat)]

fig, axes = plt.subplots(1, 2, figsize=(11.2, 3.55), dpi=170)
fig.patch.set_facecolor(PAPER)
fig.subplots_adjust(left=0.075, right=0.975, top=0.78, bottom=0.20, wspace=0.30)

# ── Panel 1: latency ──────────────────────────────────────────────
ax = axes[0]
ax.set_facecolor(PAPER)
ax.plot(range(3), pd_lat, marker="o", color=SLATE, linewidth=1.6, markersize=6,
        markerfacecolor=PAPER, markeredgewidth=1.6)
ax.plot(range(3), xz_lat, marker="o", color=ACCENT, linewidth=2.2, markersize=6,
        markerfacecolor=ACCENT)
# Direct labels instead of a legend: the last point carries the series name,
# the value sits on the inner side so the two labels never collide.
ax.annotate("pandas", (2, pd_lat[2]), textcoords="offset points", xytext=(9, 0),
            ha="left", va="center", fontsize=9.5, color=MUTED)
ax.annotate("Xazz", (2, xz_lat[2]), textcoords="offset points", xytext=(9, 0),
            ha="left", va="center", fontsize=9.5, color=ACCENT, fontweight="bold")
ax.annotate(f"{pd_lat[2]:,.0f}", (2, pd_lat[2]), textcoords="offset points", xytext=(-9, 5),
            ha="right", fontsize=8.5, color=MUTED)
ax.annotate(f"{xz_lat[2]:,.0f}", (2, xz_lat[2]), textcoords="offset points", xytext=(-9, 8),
            ha="right", fontsize=8.5, color=ACCENT, fontweight="bold")
for i, v in enumerate(pd_lat[:2]):
    ax.annotate(f"{v:,.0f}", (i, v), textcoords="offset points", xytext=(0, 10),
                ha="center", fontsize=8.5, color=MUTED)
for i, v in enumerate(xz_lat[:2]):
    ax.annotate(f"{v:,.0f}", (i, v), textcoords="offset points", xytext=(0, -16),
                ha="center", fontsize=8.5, color=ACCENT, fontweight="bold")
ax.set_yscale("log")
ax.set_ylim(45, 9000)
ax.set_xlim(-0.25, 2.62)
ax.set_xticks(range(3))
ax.set_xticklabels(labels)
ax.yaxis.set_major_formatter(FuncFormatter(lambda v, _: f"{v:,.0f}"))
ax.set_xlabel("rows", fontsize=9.5, labelpad=6)
ax.set_ylabel("pipeline latency (ms, log)", fontsize=9.5, labelpad=6)
ax.set_title("Latency — same 4-stage pipeline", fontsize=11, color=INK,
             fontweight="bold", loc="left", pad=12)
ax.yaxis.grid(True, color=RULE, linewidth=0.7)
ax.set_axisbelow(True)
for side in ("top", "right"):
    ax.spines[side].set_visible(False)
ax.tick_params(length=0, labelsize=9)

# ── Panel 2: speedup ──────────────────────────────────────────────
ax = axes[1]
ax.set_facecolor(PAPER)
bars = ax.bar(range(3), speedup, width=0.46, color=[ACCENT, ACCENT, ACCENT],
              alpha=1.0, linewidth=0)
for b, v in zip(bars, speedup):
    ax.annotate(f"{v:.2f}×", (b.get_x() + b.get_width() / 2, v), textcoords="offset points",
                xytext=(0, 6), ha="center", fontsize=11.5, fontweight="bold", color=INK)
ax.axhline(1.0, color=SLATE, linewidth=1.0, linestyle=(0, (4, 3)))
ax.set_ylim(0, max(speedup) * 1.28)
ax.set_xlim(-0.55, 2.55)
ax.set_xticks(range(3))
ax.set_xticklabels(labels)
ax.set_xlabel("rows", fontsize=9.5, labelpad=6)
ax.set_ylabel("speedup vs pandas (×)", fontsize=9.5, labelpad=6)
ax.set_title("Speedup — median of 3 runs (pandas = 1×)", fontsize=11, color=INK,
             fontweight="bold", loc="left", pad=12)
ax.yaxis.grid(True, color=RULE, linewidth=0.7)
ax.set_axisbelow(True)
for side in ("top", "right"):
    ax.spines[side].set_visible(False)
ax.tick_params(length=0, labelsize=9)

OUT.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(OUT, facecolor=PAPER)
print(f"saved → {OUT.relative_to(ROOT)}")
