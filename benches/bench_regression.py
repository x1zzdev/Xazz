"""벤치마크 회귀 게이트 (이슈 #147).

동일 스케일·행 수의 파이프라인 지연을 pandas_ms / xazz_ms 비율로 비교한다.
기준선보다 speedup이 --tol(기본 0.35) 넘게 떨어지면 종료 코드 1을 반환한다.
입력 오류와 측정 방식 불일치도 실패다. 상세 계약은 bench_regression.md에 있다.

사용법:
    python benches/bench_regression.py --current /tmp/bench.json \
        --baseline benches/bench_regression_baseline.json
"""
from __future__ import annotations

import argparse
from fractions import Fraction
import json
import math
from pathlib import Path
import statistics
import sys


class InvalidMeasurement(ValueError):
    """비교에 필요한 측정 계약을 충족하지 못한 입력."""


def positive_number(value: object, path: str) -> float:
    if type(value) not in (int, float):
        raise InvalidMeasurement(f"{path}: 유한한 양수 JSON 숫자가 필요합니다")
    try:
        valid = math.isfinite(value) and value > 0
    except OverflowError:
        valid = False
    if not valid:
        raise InvalidMeasurement(f"{path}: 유한한 양수 JSON 숫자가 필요합니다")
    return float(value)


def speedup(entry: dict, path: str) -> Fraction:
    """원시 3회 측정과 중앙값을 검증한 뒤 정확한 십진 비율을 반환한다."""
    if not isinstance(entry, dict):
        raise InvalidMeasurement(f"{path}: 스케일 측정 객체가 필요합니다")
    rows = entry.get("rows")
    if type(rows) is not int or rows <= 0:
        raise InvalidMeasurement(f"{path}.rows: 양의 정수가 필요합니다")

    latencies = {}
    for engine in ("pandas", "xazz"):
        engine_path = f"{path}.{engine}"
        metrics = entry.get(engine)
        if not isinstance(metrics, dict):
            raise InvalidMeasurement(f"{engine_path}: 엔진 측정 객체가 필요합니다")
        latency = positive_number(metrics.get("latency_ms"), f"{engine_path}.latency_ms")
        runs = metrics.get("runs")
        if not isinstance(runs, list) or len(runs) != 3:
            raise InvalidMeasurement(f"{engine_path}.runs: 워밍업을 제외한 3회 측정이 필요합니다")
        samples = []
        for index, run in enumerate(runs):
            run_path = f"{engine_path}.runs[{index}]"
            if not isinstance(run, dict):
                raise InvalidMeasurement(f"{run_path}: 측정 객체가 필요합니다")
            samples.append(positive_number(run.get("latency_ms"), f"{run_path}.latency_ms"))
            if engine == "xazz" and run.get("fallback_to_wallclock") is not False:
                raise InvalidMeasurement(
                    f"{run_path}.fallback_to_wallclock: false가 필요합니다; "
                    "wall-clock 대체 측정이나 방식이 불명확한 측정은 비교할 수 없습니다"
                )
        expected = round(statistics.median(samples), 1)
        if latency != expected:
            raise InvalidMeasurement(
                f"{engine_path}.latency_ms: 원시 측정 중앙값({expected})과 다릅니다 ({latency})"
            )
        latencies[engine] = latency
    ratio = latencies["pandas"] / latencies["xazz"]
    positive_number(ratio, f"{path}.speedup")
    # 십진 JSON 값으로 경계를 비교해 정확히 허용 한계인 값을 반올림 오차로 거부하지 않는다.
    return Fraction(str(latencies["pandas"])) / Fraction(str(latencies["xazz"]))


def scales_in(result: object, path: str) -> set[str]:
    if not isinstance(result, dict):
        raise InvalidMeasurement(f"{path}: 최상위 JSON 객체가 필요합니다")
    if "provenance" in result and not isinstance(result["provenance"], dict):
        raise InvalidMeasurement(f"{path}.provenance: 호스트 메타데이터 객체가 필요합니다")
    scales = set(result) - {"provenance"}
    if not scales or any(not isinstance(scale, str) or not scale for scale in scales):
        raise InvalidMeasurement(f"{path}: 하나 이상의 이름 있는 스케일이 필요합니다")
    return scales


def compare(current: object, baseline: object, tol: float) -> int:
    """측정 계약을 먼저 검사하고 모든 스케일의 회귀 여부를 판정한다."""
    try:
        if type(tol) not in (int, float) or not math.isfinite(tol) or not 0 <= tol < 1:
            raise InvalidMeasurement("--tol: 0 이상 1 미만의 유한한 숫자가 필요합니다")
        current_scales = scales_in(current, "current")
        baseline_scales = scales_in(baseline, "baseline")
        if current_scales != baseline_scales:
            missing = sorted(baseline_scales - current_scales)
            extra = sorted(current_scales - baseline_scales)
            raise InvalidMeasurement(f"스케일 불일치: current 누락={missing}, baseline 누락={extra}")
        measurements = []
        for scale in sorted(baseline_scales):
            cur_x = speedup(current[scale], f"current.{scale}")
            base_x = speedup(baseline[scale], f"baseline.{scale}")
            if current[scale]["rows"] != baseline[scale]["rows"]:
                raise InvalidMeasurement(
                    f"{scale}.rows: current={current[scale]['rows']}, "
                    f"baseline={baseline[scale]['rows']}로 행 수가 다릅니다"
                )
            measurements.append((scale, cur_x, base_x))
    except (InvalidMeasurement, OverflowError) as error:
        print(f"[bench-regression] 입력 오류: {error}", file=sys.stderr)
        return 1

    failed = False
    for scale, cur_x, base_x in measurements:
        floor = base_x * (1 - Fraction(str(tol)))
        ok = cur_x >= floor
        tag = "OK" if ok else "REGRESSION"
        print(
            f"[bench-regression] {scale}: speedup {float(cur_x):.2f}x "
            f"(baseline {float(base_x):.2f}x, floor {float(floor):.2f}x, tol {tol:.0%}) [{tag}]"
        )
        failed = failed or not ok
    if failed:
        print("[bench-regression] 벤치마크 회귀가 감지되었습니다", file=sys.stderr)
        return 1
    print("[bench-regression] 회귀 없음")
    return 0


def reject_constant(value: str) -> None:
    raise InvalidMeasurement(f"JSON 표준 숫자가 아닙니다: {value}")


def unique_object(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise InvalidMeasurement(f"중복 JSON 키: {key}")
        result[key] = value
    return result


def read_result(path: str) -> object:
    try:
        return json.loads(
            Path(path).read_text(encoding="utf-8"),
            parse_constant=reject_constant,
            object_pairs_hook=unique_object,
        )
    except (OSError, ValueError) as error:
        raise InvalidMeasurement(f"{path}: {error}") from error


def main() -> int:
    parser = argparse.ArgumentParser(description="벤치마크 회귀 게이트 (이슈 #147)")
    parser.add_argument("--current", required=True, help="새로 측정한 결과 JSON")
    parser.add_argument("--baseline", required=True, help="커밋된 기준선 JSON")
    parser.add_argument(
        "--tol", type=float, default=0.35,
        help="허용 회귀 비율: 0 이상 1 미만 (기본 0.35 = speedup 35%% 하락까지 허용)",
    )
    args = parser.parse_args()
    try:
        current = read_result(args.current)
        baseline = read_result(args.baseline)
    except InvalidMeasurement as error:
        print(f"[bench-regression] 입력 오류: {error}", file=sys.stderr)
        return 1
    return compare(current, baseline, args.tol)


if __name__ == "__main__":
    raise SystemExit(main())
