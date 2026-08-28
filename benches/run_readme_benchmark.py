"""
benches/run_readme_benchmark.py — README 벤치마크 오케스트레이터
================================================================
동일한 4단계 파이프라인(P2 정리·필터 → P3 그룹합계 → P4 Top-10 평균 → P7 fill+count)을
Python Pandas(eager)와 Xazz(Rust + Polars LazyFrame)로 실행해 측정한다.

측정 방법:
  - 각 엔진 × 스케일 조합마다 워밍업 1회 + 측정 3회 실행, 중앙값(median) 보고
  - 지연 시간: wall-clock (time.perf_counter)
  - 피크 RSS: 프로세스 트리(xazz가 스폰하는 xazz-runner 포함)를 3ms 주기 폴링
  - 결과는 benches/benchmark_results.json 으로 저장

사용법:
    python benches/run_readme_benchmark.py [--quick]
"""
from __future__ import annotations

import json
import statistics
import subprocess
import sys
import time
from pathlib import Path

import psutil

ROOT = Path(__file__).parent.parent.resolve()
DATA = ROOT / "benches" / "data"
PANDAS_SCRIPT = ROOT / "benches" / "pandas_pipeline.py"
XAZZ_BIN = ROOT / "target" / "release" / "xazz"
TEMPLATE = ROOT / "benches" / "bench_scale_small.xzz"
RESULTS_PATH = ROOT / "benches" / "benchmark_results.json"

SCALES = ["small", "medium", "large"]
RUNS = 3
POLL_MS = 3


def measure_tree(cmd: list[str], cwd: Path) -> tuple[float, float]:
    """서브프로세스 트리 전체의 wall-clock 지연과 피크 RSS(MB)를 측정한다."""
    proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, cwd=cwd)
    parent = psutil.Process(proc.pid)
    peak_mb = 0.0
    t0 = time.perf_counter()
    while proc.poll() is None:
        total = 0.0
        try:
            for p in [parent, *parent.children(recursive=True)]:
                try:
                    total += p.memory_info().rss
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    pass
            peak_mb = max(peak_mb, total / 1_048_576)
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            pass
        time.sleep(POLL_MS / 1000.0)
    elapsed_ms = (time.perf_counter() - t0) * 1000.0
    if proc.returncode != 0:
        raise RuntimeError(f"exit={proc.returncode}: {' '.join(cmd)}")
    return elapsed_ms, peak_mb


def bench_pandas(csv: Path) -> dict:
    """pandas 베이스라인: 워밍업 1회 + 측정 3회."""
    runs = []
    for i in range(RUNS + 1):
        proc = subprocess.Popen(
            [sys.executable, str(PANDAS_SCRIPT), str(csv)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        parent = psutil.Process(proc.pid)
        peak_mb = 0.0
        t0 = time.perf_counter()
        while proc.poll() is None:
            try:
                rss = parent.memory_info().rss / 1_048_576
                peak_mb = max(peak_mb, rss)
            except (psutil.NoSuchProcess, psutil.AccessDenied):
                pass
            time.sleep(POLL_MS / 1000.0)
        elapsed_ms = (time.perf_counter() - t0) * 1000.0
        out, err = proc.communicate()
        if proc.returncode != 0:
            raise RuntimeError(f"pandas 실패: {err.decode()[:300]}")
        metrics = json.loads(out.decode().strip().splitlines()[-1])
        if i == 0:
            continue  # 워밍업
        runs.append({
            "latency_ms": round(max(elapsed_ms, metrics["total_latency_ms"]), 1),
            "peak_mb": round(peak_mb, 1),
        })
    return summarize(runs)


def bench_xazz(csv: Path) -> dict:
    """Xazz 정품 바이너리: 워밍업 1회 + 측정 3회."""
    script = ROOT / "benches" / f"_bench_{csv.stem}.xzz"
    script.write_text(TEMPLATE.read_text(encoding="utf-8").replace("SCALE_CSV", str(csv)), encoding="utf-8")
    cmd = [str(XAZZ_BIN), "run", str(script)]
    runs = []
    for i in range(RUNS + 1):
        lat, peak = measure_tree(cmd, ROOT)
        if i == 0:
            continue  # 워밍업
        runs.append({"latency_ms": round(lat, 1), "peak_mb": round(peak, 1)})
    script.unlink(missing_ok=True)
    return summarize(runs)


def summarize(runs: list[dict]) -> dict:
    return {
        "latency_ms": round(statistics.median(r["latency_ms"] for r in runs), 1),
        "peak_mb": round(statistics.median(r["peak_mb"] for r in runs), 1),
        "runs": runs,
    }


def main() -> None:
    quick = "--quick" in sys.argv
    scales = ["small"] if quick else SCALES
    results: dict = {}
    for scale in scales:
        csv = DATA / f"scale_{scale}.csv"
        if not csv.exists():
            raise SystemExit(f"{csv} 없음 — 먼저 make_scale_data.py 를 실행하세요")
        rows = sum(1 for _ in csv.open("rb")) - 1
        print(f"\n──── scale = {scale.upper()} ({rows:,} rows, {csv.stat().st_size/1_048_576:.1f} MB)")
        print("  [pandas] …", flush=True)
        results.setdefault(scale, {})["pandas"] = bench_pandas(csv)
        p = results[scale]["pandas"]
        print(f"  [pandas] median latency = {p['latency_ms']:>10,.1f} ms | peak RSS = {p['peak_mb']:,.1f} MB", flush=True)
        print("  [xazz] …", flush=True)
        results.setdefault(scale, {})["xazz"] = bench_xazz(csv)
        x = results[scale]["xazz"]
        print(f"  [xazz] median latency = {x['latency_ms']:>10,.1f} ms | peak RSS = {x['peak_mb']:,.1f} MB", flush=True)
        results[scale]["rows"] = rows

    RESULTS_PATH.write_text(json.dumps(results, indent=2, ensure_ascii=False), encoding="utf-8")
    print(f"\n결과 저장 → {RESULTS_PATH}")
    lg = results[scales[-1]]
    print(f"Speedup (last scale): {lg['pandas']['latency_ms'] / lg['xazz']['latency_ms']:.2f}x")


if __name__ == "__main__":
    main()
