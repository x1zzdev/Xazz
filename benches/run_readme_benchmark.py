"""
benches/run_readme_benchmark.py — README 벤치마크 오케스트레이터
===============================================================
동일한 4단계 파이프라인(P2 정리·필터 → P3 그룹합계 → P4 Top-10 평균 → P7 fill+count)을
Python Pandas(eager)와 Xazz(Rust + Polars LazyFrame)로 실행해 측정한다.

측정 방법 (공평성):
  - 각 엔진 × 스케일 조합마다 워밍업 1회 + 측정 3회 실행, 중앙값(median) 보고
  - 지연 시간: **파이프라인 실행만** 측정 — 인터프리터 부팅은 제외
      * pandas: 스크립트가 내부에서 잰 total_latency_ms (부팅·import 제외)
      * xazz:   런타임의 [xazz:timing] 마커의 pipeline_ms (프로세스 부팅 제외)
  - 피크 RSS: **동일 기준** — 양쪽 모두 프로세스 트리(xazz는 xazz-runner 포함)를
    3ms 주기 폴링
  - 결과는 benches/benchmark_results.json 으로 저장

사용법:
    python benches/run_readme_benchmark.py [--quick]
"""
from __future__ import annotations

import json
import os
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


def measure_tree(
    cmd: list[str], cwd: Path, capture: bool = False, env: dict | None = None
) -> tuple[float, float, str]:
    """서브프로세스 트리 전체의 wall-clock 지연과 피크 RSS(MB)를 측정한다.

    capture=True 면 stdout 을 돌려받는다 — [xazz:timing] 마커 파싱용.
    """
    proc_env = os.environ.copy()
    if env:
        proc_env.update(env)
    proc = subprocess.Popen(
        cmd,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        cwd=cwd,
        env=proc_env,
    )
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
    wall_ms = (time.perf_counter() - t0) * 1000.0
    stdout, _ = proc.communicate()
    if proc.returncode != 0:
        raise RuntimeError(f"exit={proc.returncode}: {' '.join(cmd)}")
    return wall_ms, peak_mb, stdout.decode(errors="replace") if capture else ""


def parse_xazz_timing(stdout: str) -> float | None:
    """[xazz:timing] {"pipeline_ms": N} 마커에서 파이프라인 실행 지연을 추출한다."""
    for line in stdout.splitlines():
        if line.startswith("[xazz:timing] "):
            try:
                return float(json.loads(line[len("[xazz:timing] ") :])["pipeline_ms"])
            except (json.JSONDecodeError, KeyError, TypeError):
                return None
    return None


def bench_pandas(csv: Path) -> dict:
    """pandas 베이스라인: 워밍업 1회 + 측정 3회.

    지연은 스크립트가 내부에서 측정한 total_latency_ms (인터프리터 부팅·import 제외).
    RSS 는 프로세스 트리 기준 (pandas 는 자식이 없어 부모와 동일).
    """
    runs = []
    for i in range(RUNS + 1):
        _, peak, stdout = measure_tree(
            [sys.executable, str(PANDAS_SCRIPT), str(csv)],
            ROOT,
            capture=True,
        )
        metrics = json.loads(stdout.strip().splitlines()[-1])
        if i == 0:
            continue  # 워밍업
        runs.append({
            "latency_ms": round(metrics["total_latency_ms"], 1),
            "peak_mb": round(peak, 1),
        })
    return summarize(runs)


def bench_xazz(csv: Path) -> dict:
    """Xazz 정품 바이너리: 워밍업 1회 + 측정 3회.

    지연은 [xazz:timing] 마커의 pipeline_ms (프로세스 부팅 제외). 마커가 없으면
    wall-clock 으로 폴백하고 note 를 남긴다 (호환성 방어).
    """
    script = ROOT / "benches" / f"_bench_{csv.stem}.xzz"
    script.write_text(TEMPLATE.read_text(encoding="utf-8").replace("SCALE_CSV", str(csv)), encoding="utf-8")
    cmd = [str(XAZZ_BIN), "run", str(script)]
    runs = []
    for i in range(RUNS + 1):
        wall_ms, peak, stdout = measure_tree(cmd, ROOT, capture=True, env={"XAZZ_STREAMING": "1"})
        if i == 0:
            continue  # 워밍업
        pipeline_ms = parse_xazz_timing(stdout)
        runs.append({
            # 우선 파이프라인 실행만 사용 (부팅 제외). 마커가 없으면 wall-clock 폴백.
            "latency_ms": round(pipeline_ms if pipeline_ms is not None else wall_ms, 1),
            "peak_mb": round(peak, 1),
            "fallback_to_wallclock": pipeline_ms is None,
        })
    script.unlink(missing_ok=True)
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
    # 200M 행 스케일은 선택 — make_scale_data.py --xlarge 로 데이터 생성 후 --xlarge 로 측정
    if "--xlarge" in sys.argv and DATA.joinpath("scale_xlarge.csv").exists():
        scales = scales + ["xlarge"]
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
