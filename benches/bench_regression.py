"""
benches/bench_regression.py — 벤치마크 회귀 게이트 (issue #147)
================================================================
`run_readme_benchmark.py`가 만든 새 결과(`--current`)를 커밋된 기준선
(`--baseline`)과 비교해 **회귀**를 판정한다.

절대 지연(ms)은 머신마다 다르므로 비교하지 않는다. 대신 같은 머신·같은 실행
안에서 측정된 **pandas 대비 xazz speedup 비율**(pandas_ms / xazz_ms)을 쓴다 —
양쪽 모두 같은 부하를 처리하므로 머신 속도가 상쇄된다. 기준선 대비 speedup이
`--tol`(기본 0.35 = 35%) 넘게 떨어지면 회귀(exit 1)로 본다.

사용법:
    python benches/bench_regression.py --current /tmp/bench.json \
        --baseline benches/bench_regression_baseline.json
"""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def speedup(entry: dict) -> float | None:
    """한 스케일 항목의 pandas 대비 xazz speedup (pandas_ms / xazz_ms)."""
    try:
        pandas_ms = float(entry["pandas"]["latency_ms"])
        xazz_ms = float(entry["xazz"]["latency_ms"])
    except (KeyError, TypeError, ValueError):
        return None
    if xazz_ms <= 0 or pandas_ms <= 0:
        return None
    return pandas_ms / xazz_ms


def compare(current: dict, baseline: dict, tol: float) -> int:
    """공통 스케일마다 speedup 회귀를 검사하고 종료 코드를 돌려준다."""
    scales = sorted(set(current) & set(baseline))
    if not scales:
        print("[bench-regression] 비교 가능한 공통 스케일이 없습니다", file=sys.stderr)
        return 1

    failed = False
    for scale in scales:
        cur, base = current[scale], baseline[scale]
        if not isinstance(cur, dict) or not isinstance(base, dict):
            continue
        cur_x, base_x = speedup(cur), speedup(base)
        if cur_x is None or base_x is None:
            print(f"[bench-regression] {scale}: speedup 계산 불가 — 건너뜀", file=sys.stderr)
            continue
        floor = base_x * (1.0 - tol)
        ok = cur_x >= floor
        tag = "OK" if ok else "REGRESSION"
        print(
            f"[bench-regression] {scale}: speedup {cur_x:.2f}x "
            f"(baseline {base_x:.2f}x, floor {floor:.2f}x, tol {tol:.0%}) [{tag}]"
        )
        failed = failed or not ok

    if failed:
        print("[bench-regression] 벤치마크 회귀가 감지되었습니다", file=sys.stderr)
        return 1
    print("[bench-regression] 회귀 없음")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="벤치마크 회귀 게이트 (issue #147)")
    parser.add_argument("--current", required=True, help="새로 측정한 결과 JSON")
    parser.add_argument("--baseline", required=True, help="커밋된 기준선 JSON")
    parser.add_argument(
        "--tol",
        type=float,
        default=0.35,
        help="허용 회귀 비율 (기본 0.35 = speedup 35%% 하락까지 허용)",
    )
    args = parser.parse_args()

    current = json.loads(Path(args.current).read_text(encoding="utf-8"))
    baseline = json.loads(Path(args.baseline).read_text(encoding="utf-8"))
    return compare(current, baseline, args.tol)


if __name__ == "__main__":
    raise SystemExit(main())
