"""
benches/render_benchmark_chart.py — 벤치마크 결과 차트 렌더러
=============================================================
benches/benchmark_results.json 을 읽어 README용 PNG 차트를 생성한다.
  패널 1: 스케일별 파이프라인 지연 시간(로그 스케일 라인 차트)
  패널 2: pandas 대비 속도향상 배수(바 차트)
"""
from __future__ import annotations

import json
from pathlib import Path

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

ROOT = Path(__file__).parent.parent.resolve()
RESULTS = ROOT / "benches" / "benchmark_results.json"
OUT = ROOT / "docs" / "assets" / "benchmark_chart.png"

NAVY = "#1e3a5f"
SLATE = "#94a3b8"
INK = "#0f172a"
GRID = "#e2e8f0"

plt.rcParams.update({
    "font.family": "DejaVu Sans",
    "font.size": 11,
    "axes.edgecolor": GRID,
    "axes.linewidth": 1.0,
})

data = json.loads(RESULTS.read_text(encoding="utf-8"))
scales = ["small", "medium", "large"]
labels = ["Small\n228K rows", "Medium\n912K rows", "Large\n4.09M rows"]

pd_lat = [data[s]["pandas"]["latency_ms"] for s in scales]
xz_lat = [data[s]["xazz"]["latency_ms"] for s in scales]
speedup = [p / x for p, x in zip(pd_lat, xz_lat)]

fig, axes = plt.subplots(1, 2, figsize=(11, 4.2), dpi=160)
fig.subplots_adjust(left=0.07, right=0.97, top=0.86, bottom=0.16, wspace=0.28)

# ── Panel 1: latency ──────────────────────────────────────────────
ax = axes[0]
ax.plot(labels, pd_lat, marker="o", color=SLATE, linewidth=2, markersize=7, label="Python pandas (eager)")
ax.plot(labels, xz_lat, marker="o", color=NAVY, linewidth=2.4, markersize=7, label="Xazz (Rust + Polars LazyFrame)")
for i, (p, x) in enumerate(zip(pd_lat, xz_lat)):
    ax.annotate(f"{p:,.0f} ms", (i, p), textcoords="offset points", xytext=(0, 9),
                ha="center", fontsize=9, color="#64748b")
    ax.annotate(f"{x:,.0f} ms", (i, x), textcoords="offset points", xytext=(0, -15),
                ha="center", fontsize=9, color=NAVY, fontweight="bold")
ax.set_yscale("log")
ax.set_ylim(100, 12000)
ax.set_ylabel("Pipeline latency (ms, log scale)", color=INK)
ax.set_title("End-to-end pipeline latency", fontsize=12, fontweight="bold", color=INK, pad=10)
ax.yaxis.grid(True, color=GRID, linewidth=0.8)
ax.set_axisbelow(True)
ax.tick_params(colors="#475569")
ax.legend(frameon=False, fontsize=9, loc="upper left")

# ── Panel 2: speedup ──────────────────────────────────────────────
ax = axes[1]
bars = ax.bar(labels, speedup, width=0.52, color=[SLATE, "#5b7c99", NAVY])
for b, v in zip(bars, speedup):
    ax.annotate(f"{v:.2f}×", (b.get_x() + b.get_width() / 2, v), textcoords="offset points",
                xytext=(0, 5), ha="center", fontsize=11, fontweight="bold", color=INK)
ax.axhline(1.0, color="#cbd5e1", linewidth=1.2, linestyle="--")
ax.set_ylim(0, max(speedup) * 1.25)
ax.set_ylabel("Speedup vs pandas (×)", color=INK)
ax.set_title("Speedup factor (median of 3 runs)", fontsize=12, fontweight="bold", color=INK, pad=10)
ax.yaxis.grid(True, color=GRID, linewidth=0.8)
ax.set_axisbelow(True)
ax.tick_params(colors="#475569")

fig.suptitle("Xazz vs Python pandas — same 4-stage pipeline, real Seoul air-quality data",
             fontsize=12.5, fontweight="bold", color=INK, x=0.07, ha="left", y=0.97)

OUT.parent.mkdir(parents=True, exist_ok=True)
fig.savefig(OUT, facecolor="white")
print(f"saved → {OUT}")
