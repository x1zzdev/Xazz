"""작은 CSV에서 pandas·실제 Xazz 실행 파일의 네 단계 모든 셀을 대조한다.

실행 파일이 없거나 하나라도 실패하면 실패 종료한다. 대용량 실측이나 성능
검사가 아니며, 등록한 고정 입력 전체 × on/off × 최종 단계 네 개를 검사한다.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import math
import os
from pathlib import Path
import re
import tempfile

import pandas_pipeline as pandas_pipeline
from benchmark_semantics import COLUMNS, FIXTURES, STAGES, check_stage_rows, oracle
from run_readme_benchmark import measure_tree
from verify_benchmark_results import PIPELINE, load_json, require

TEMPLATE = Path(__file__).with_name("bench_scale_small.xzz")


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def stage_sources(template):
    starts = list(re.finditer(r"^v (p2|p3|p4|p7) = ", template, flags=re.MULTILINE))
    require(tuple(match[1] for match in starts) == STAGES, "벤치마크 템플릿 단계 누락·순서 불일치")
    return {stage: template[:starts[index + 1].start()] if index + 1 < len(starts) else template
            for index, stage in enumerate(STAGES)}


def markers(stdout, kind):
    prefix = f"[xazz:{kind}]"
    lines = [line for line in stdout.splitlines() if line.startswith(prefix)]
    require(all(line.startswith(prefix + " ") for line in lines), f"{kind} 마커 형식 오류")
    return [load_json(line[len(prefix) + 1:]) for line in lines]


def columns_for(stage):
    return list(COLUMNS) if stage == "p2" else ["station", "pm25" if stage == "p7" else "pm10"]


def check_schema(stage, schema):
    require(isinstance(schema, list) and all(isinstance(column, dict) and set(column) == {"name", "type"}
            for column in schema), "결과 스키마 형식 오류")
    require([column["name"] for column in schema] == columns_for(stage), "결과 스키마 열 이름·순서 불일치")
    for column in schema:
        allowed = {"str"} if column["name"] in ("date", "station") else {"u32", "u64"} if stage == "p7" else {"f64"}
        require(column["type"] in allowed, f"결과 스키마 자료형 불일치: {column}")


def validate_output(stdout, stderr, mode, stage, expected):
    require("ERROR]" not in stdout + stderr, "성공 종료와 함께 실행 오류가 출력됐습니다")
    count = STAGES.index(stage) + 1
    collections = markers(stdout, "collect")
    selected = {"requested": mode, "engine": "streaming" if mode == "on" else "in-memory", "status": "ok"}
    require(len(collections) == count and all(record == selected for record in collections), "수집 API 마커 누락·모드·엔진·상태 불일치")
    timing = markers(stdout, "timing")
    require(len(timing) == 1 and isinstance(timing[0], dict) and set(timing[0]) == {"pipeline_ms"}, "타이밍 마커 누락·중복·형식 오류")
    value = timing[0]["pipeline_ms"]
    require(type(value) in (int, float) and math.isfinite(value) and value > 0, "타이밍은 유한한 양수여야 합니다")
    lines = [line for line in stderr.splitlines() if line.startswith("[xazz] Pipeline #")]
    parsed = [PIPELINE.fullmatch(line) for line in lines]
    require(len(parsed) == count and all(parsed), "단계 완료 로그 누락·중복·형식 오류")
    for index, match in enumerate(parsed):
        name = STAGES[index]
        require(int(match[1]) == index + 1 and match[2] == name
                and int(match[3]) == len(expected[name]) and int(match[4]) == len(columns_for(name)),
                f"{name} 실행 단계·행·열 수 불일치")
    final = markers(stdout, "result")
    require(len(final) == 1 and isinstance(final[0], dict) and set(final[0]) == {"rows", "schema"}, "최종 결과 마커 누락·중복·형식 오류")
    check_schema(stage, final[0]["schema"])
    check_stage_rows(stage, final[0]["rows"], expected[stage])
    return {**final[0], "collections": collections, "pipeline_ms": value}


def check_pandas(path, expected):
    frames = pandas_pipeline.run_pipelines(pandas_pipeline.load_csv(path))
    require(set(frames) == set(STAGES), "pandas 단계 누락")
    evidence = {}
    types = pandas_pipeline.pd.api.types
    for stage, frame in frames.items():
        require(list(frame.columns) == columns_for(stage), f"pandas {stage} 스키마 열 불일치")
        for name in frame.columns:
            valid = (types.is_string_dtype(frame[name]) if name in ("date", "station") else
                     types.is_integer_dtype(frame[name]) if stage == "p7" else types.is_float_dtype(frame[name]))
            require(valid, f"pandas {stage}.{name} 스키마 자료형 불일치")
        rows = frame.to_dict(orient="records")
        check_stage_rows(stage, rows, expected[stage])
        evidence[stage] = {"rows": rows, "schema": [{"name": name, "type": str(frame[name].dtype)} for name in frame.columns]}
    return evidence


def run_checks(binary, log_dir, report, timeout_seconds):
    require(binary.is_file() and os.access(binary, os.X_OK), f"실행 가능한 xazz-exec 파일이 없습니다: {binary}")
    for name in ("POLARS_AUTO_NEW_STREAMING", "POLARS_FORCE_NEW_STREAMING"):
        require(os.environ.get(name) != "1", f"{name}=1은 요청 엔진을 우회할 수 있어 검사할 수 없습니다")
    report["binary_sha256"] = sha256(binary)
    template = TEMPLATE.read_text(encoding="utf-8")
    sources = stage_sources(template)
    report["source_sha256"] = {path.name: sha256(path) for path in (
        Path(__file__), Path(__file__).with_name("benchmark_semantics.py"),
        Path(__file__).with_name("pandas_pipeline.py"), TEMPLATE)}
    environment = {"XAZZ_BACKEND": "cpu", "XAZZ_COLLECT_DIAGNOSTICS": "1", "XAZZ_LANG": "en"}
    for case, records in FIXTURES.items():
        csv_path = log_dir / f"{case}.csv"
        with csv_path.open("w", encoding="utf-8", newline="") as output:
            writer = csv.writer(output)
            writer.writerow(COLUMNS)
            writer.writerows(records)
        expected = oracle(records)
        report["fixtures"][case] = {"csv_sha256": sha256(csv_path), "input_rows": len(records),
                                    "oracle": expected, "pandas": check_pandas(csv_path, expected)}
        for mode in ("on", "off"):
            overrides = {**environment, "XAZZ_STREAMING": "1" if mode == "on" else "0"}
            for stage, source in sources.items():
                stem = f"{case}-{mode}-{stage}"
                script, stdout_path, stderr_path = (log_dir / f"{stem}{suffix}" for suffix in (".xzz", ".stdout.log", ".stderr.log"))
                script.write_text(source.replace("SCALE_CSV", csv_path.name), encoding="utf-8")
                record = {"case": case, "mode": mode, "stage": stage, "status": "failed",
                          "command": [str(binary), script.name], "stdout": str(stdout_path), "stderr": str(stderr_path),
                          "env_overrides": overrides}
                report["runs"].append(record)
                measurement_path = log_dir / f"{stem}.measurement.json"
                record["measurement"] = str(measurement_path)
                try:
                    _, _, stdout = measure_tree(record["command"], log_dir, capture=True, env=overrides,
                                                timeout_seconds=timeout_seconds, log_dir=log_dir, log_name=stem,
                                                failure_markers=("ERROR]",))
                finally:
                    if measurement_path.is_file():
                        record["exit_code"] = load_json(measurement_path.read_text(encoding="utf-8"))["exit_code"]
                record.update(validate_output(stdout, stderr_path.read_text(encoding="utf-8"),
                                              mode, stage, expected), status="ok")
    expected_runs = len(FIXTURES) * 2 * len(STAGES)
    require(len(report["runs"]) == expected_runs, f"필수 실제 실행 {expected_runs}개 누락")
    report["status"] = "ok"


def main(argv=None):
    import json

    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xazz-exec", required=True, type=Path)
    parser.add_argument("--out", required=True, type=Path)
    parser.add_argument("--timeout-seconds", type=float, default=120.0)
    args = parser.parse_args(argv)
    if not math.isfinite(args.timeout_seconds) or args.timeout_seconds <= 0:
        parser.error("--timeout-seconds는 유한한 양수여야 합니다")
    binary, output = args.xazz_exec.resolve(), args.out.resolve()
    if binary == output:
        parser.error("검사 보고서는 실행 파일과 다른 경로여야 합니다")
    output.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix=output.stem + "-logs-", dir=output.parent))
    report = {"status": "failed", "xazz_exec": str(binary), "raw_logs": str(logs), "fixtures": {}, "runs": [],
              "expected_runs": len(FIXTURES) * 2 * len(STAGES),
              "limitations": ["작은 고정 입력의 모든 셀을 비교하며, 기존 대용량 실행의 출력되지 않은 셀을 사후 검증하지 않습니다.",
                              "수집 마커는 요청한 API 선택·완료의 증거이며 Polars 내부 모든 노드의 실행 엔진을 보증하지 않습니다."],
              "numeric_tolerance": {"relative": 1e-12, "absolute": 1e-12}}
    try:
        run_checks(binary, logs, report, args.timeout_seconds)
    except (Exception, KeyboardInterrupt) as error:
        report["error"] = f"{type(error).__name__}: {error}"
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False), encoding="utf-8")
    print(json.dumps({"status": report["status"], "completed_runs": sum(run["status"] == "ok" for run in report["runs"]),
                      "report": str(output), "error": report.get("error")}, ensure_ascii=False))
    return 0 if report["status"] == "ok" else 1


if __name__ == "__main__":
    raise SystemExit(main())
