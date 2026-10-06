"""실측 CSV 전체를 표준 라이브러리로 스캔하고 각 실행의 원시 결과를 대조한다.

P2/P3/P4/P7 행 수와 Xazz P7 셀을 검증한다. 출력되지 않은 P2/P3/P4 셀은
검증하지 않는다. --self-test는 작은 고정 입력과 실패 사례로 검증기를 검사한다.
"""
from __future__ import annotations

import argparse
from collections import Counter
import csv
from functools import lru_cache
import hashlib
import json
import math
from pathlib import Path
import re
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
COLUMNS = ("date", "station", "pm10", "pm25")
NUMBER = re.compile(r"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\Z")
STAGES = ("p2", "p3", "p4", "p7")
PIPELINE = re.compile(r"^\[xazz\] Pipeline #(\d+) '(p2|p3|p4|p7)' (?:done|완료): (\d+) × (\d+)$")
LIMITS = ["P2/P3/P4는 실제 출력 행 수를 대조했으며 전체 셀은 로그에 없어 대조하지 않았습니다.",
          "지연·RSS·중앙값과 수집 API 마커는 이 계산 결과 대조기의 검사 대상이 아닙니다. 별도 측정기가 검사·기록합니다.",
          "P3 합계·P4 평균 top10은 독립 계산값이며 실제 엔진 셀과 일치한다고 보증하지 않습니다.",
          "pandas는 result_rows만 출력하므로 pandas P7 셀은 대조하지 않았습니다.",
          "P7 5위 동점에서 선택되는 측정소와 동점끼리의 순서는 고정하지 않습니다."]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, f"JSON 중복 키: {key}")
        result[key] = value
    return result


def finite_float(value):
    number = float(value)
    if not math.isfinite(number):
        raise ValueError(f"유한하지 않은 수: {value}")
    return number


def load_json(text):
    return json.loads(text, object_pairs_hook=unique_object, parse_float=finite_float,
                      parse_constant=lambda value: require(False, f"금지된 JSON 상수: {value}"))


def read_json(path):
    return load_json(path.read_text(encoding="utf-8"))


@lru_cache(maxsize=8192)
def _parsed_numeric(value):
    """동일한 숫자 원문만 재사용하며 캐시 메모리는 8192개로 제한한다."""
    if value == "":
        return None
    if NUMBER.fullmatch(value) is None:
        raise ValueError(f"잘못된 숫자 {value!r}")
    return finite_float(value)


def numeric(value, column, line):
    try:
        return _parsed_numeric(value)
    except ValueError as error:
        raise ValueError(f"{line}행 {column}: {error}") from error


def scan_csv(path):
    """행을 보관하지 않고 측정소별 보정 합계·계수만 유지한다."""
    before = path.stat()
    digest, groups, counts = hashlib.sha256(), {}, Counter()
    total = filtered = 0
    with path.open("rb") as source:
        def decoded_lines():
            for raw in source:
                digest.update(raw)
                yield raw.decode("utf-8")

        reader = csv.reader(decoded_lines(), strict=True)
        require(next(reader, None) == list(COLUMNS), f"{path}: 표준 열 이름·순서 불일치")
        for row in reader:
            if len(row) != 4:
                raise ValueError(f"{path}:{reader.line_num}: 열 수가 4가 아닙니다")
            date, station, raw10, raw25 = row
            if not date.strip() or date in ("-", "점검중", "N/A"):
                raise ValueError(f"{path}:{reader.line_num}: date 빈 값·예약 null 토큰")
            if not station.strip() or station in ("-", "점검중", "N/A"):
                raise ValueError(f"{path}:{reader.line_num}: station 빈 값·예약 null 토큰")
            pm10, pm25 = numeric(raw10, "pm10", reader.line_num), numeric(raw25, "pm25", reader.line_num)
            total += 1
            if pm10 is not None and pm10 < 120 and pm25 is not None and pm25 > 10:
                filtered += 1
                state = groups.setdefault(station, [0.0, 0.0, 0])
                # Kahan 누적으로 같은 측정소의 많은 행을 저장하지 않는다.
                delta = pm10 - state[1]
                updated = state[0] + delta
                state[1], state[0], state[2] = (updated - state[0]) - delta, updated, state[2] + 1
            # P7은 pm25 null을 0으로 채운 뒤 count하므로 필터 통과 행마다 1이다.
            if pm10 is not None and pm10 > 50:
                counts[station] += 1
            if total % 10_000_000 == 0:
                print(f"[독립 대조] {path.name}: {total:,}행 스캔", file=sys.stderr, flush=True)
    after = path.stat()
    require((before.st_size, before.st_mtime_ns) == (after.st_size, after.st_mtime_ns), "스캔 중 입력 파일 변경")
    require(total > 0, "빈 입력 CSV")
    require(all(math.isfinite(state[0]) for state in groups.values()), "독립 P3 합계가 유한하지 않습니다")
    sums = {station: state[0] for station, state in sorted(groups.items())}
    means = sorted(({"station": station, "pm10": state[0] / state[2]} for station, state in groups.items()),
                   key=lambda row: (-row["pm10"], row["station"]))[:10]
    return {"rows": total, "bytes": after.st_size, "sha256": digest.hexdigest(),
            "result_rows": dict(zip(STAGES, (filtered, len(groups), min(10, len(groups)), min(5, len(counts))))),
            "p3_sums": sums, "p4_top10": means, "p7_counts": dict(counts)}


def check_p7(rows, counts):
    require(isinstance(rows, list) and len(rows) == min(5, len(counts)), "P7 행 수 불일치")
    seen, values = set(), []
    cutoff = sorted(counts.values(), reverse=True)[min(5, len(counts)) - 1] if counts else 0
    for row in rows:
        require(isinstance(row, dict) and set(row) == {"station", "pm25"}, "P7 셀 필드 불일치")
        station, count = row["station"], row["pm25"]
        require(isinstance(station, str) and station in counts and station not in seen, "P7 측정소 누락·중복·불일치")
        require(type(count) is int and count == counts[station] and count >= cutoff, "P7 측정소 계수·5위 경계 불일치")
        seen.add(station)
        values.append(count)
    require(values == sorted(values, reverse=True), "P7 내림차순 불일치")
    require({station for station, count in counts.items() if count > cutoff} <= seen, "P7 상위 측정소 누락")


def check_row_counts(actual, expected):
    require(isinstance(actual, dict) and set(actual) == set(STAGES), "단계별 행 수 누락·추가")
    require(all(type(value) is int and value >= 0 for value in actual.values()), "단계별 행 수 형식 오류")
    require(actual == expected, f"단계별 행 수 불일치: 실제={actual}, 독립={expected}")


def verify(results_path, data_dir, logs_dir=None):
    results = read_json(results_path)
    provenance = results["provenance"]
    inputs, recorded_logs = provenance["inputs"], provenance["raw_logs"]
    logs = Path(logs_dir) if logs_dir is not None else Path(recorded_logs)
    measured, warmup = provenance["measured_runs"], provenance["warmup_runs"]
    modes = provenance["streaming_requested_modes"]
    require(type(measured) is int and measured > 0 and type(warmup) is int and warmup == 1, "실행 횟수 계약 불일치")
    require(isinstance(modes, list) and modes and all(mode in ("auto", "on", "off") for mode in modes)
            and len(set(modes)) == len(modes), "스트리밍 모드 누락·불일치")
    require(isinstance(inputs, dict) and inputs and set(results) == {*inputs, "provenance"}, "측정 스케일 누락·불일치")
    report = {"status": "ok", "results": str(results_path.resolve()), "raw_logs": str(logs),
              "recorded_raw_logs": recorded_logs, "limitations": LIMITS, "scales": {}}
    for scale, metadata in inputs.items():
        filename = metadata["file"]
        require(isinstance(filename, str) and Path(filename).name == filename, "입력 파일 이름 계약 불일치")
        path = data_dir / filename
        oracle = scan_csv(path)
        require(all(oracle[key] == metadata[key] for key in ("rows", "bytes", "sha256")), "입력 provenance와 전체 스캔 불일치")
        require(type(results[scale]["rows"]) is int and results[scale]["rows"] == oracle["rows"], "결과 입력 행 수 불일치")
        names = {"pandas": "pandas", **{f"xazz-{mode}": "xazz" if mode == "auto" else f"xazz_streaming_{mode}" for mode in modes}}
        require(set(results[scale]) == {"rows", *names.values()}, "측정 엔진 결과 누락·추가")
        checked = []
        for engine, key in names.items():
            require(len(results[scale][key]["runs"]) == measured, "저장된 측정 반복 수 불일치")
            expected_names = {f"{path.stem}-{engine}-{index}.stdout.log" for index in range(measured + warmup)}
            # glob은 권한 오류를 빈 결과로 숨길 수 있으므로 디렉터리 읽기 오류도 보존한다.
            actual_names = {p.name for p in logs.iterdir()
                            if p.name.startswith(f"{path.stem}-{engine}-") and p.name.endswith(".stdout.log")}
            require(actual_names == expected_names, "원시 실행 로그 누락·추가")
            for index in range(measured + warmup):
                base = logs / f"{path.stem}-{engine}-{index}"
                measurement = read_json(Path(str(base) + ".measurement.json"))
                require(measurement["status"] == "ok" and type(measurement["exit_code"]) is int
                        and measurement["exit_code"] == 0, f"성공하지 않은 실행: {base}")
                stdout = Path(str(base) + ".stdout.log").read_text(encoding="utf-8")
                stderr = Path(str(base) + ".stderr.log").read_text(encoding="utf-8")
                if engine == "pandas":
                    actual = load_json(stdout)
                    check_row_counts(actual["result_rows"], oracle["result_rows"])
                else:
                    require("[xazz RUNTIME ERROR]" not in stdout + stderr, "Xazz 런타임 실패 로그")
                    lines = [line for line in stderr.splitlines() if line.startswith("[xazz] Pipeline #")]
                    parsed = [PIPELINE.fullmatch(line) for line in lines]
                    require(len(parsed) == 4 and all(parsed), "Xazz 단계 완료 로그 누락·추가·형식 오류")
                    actual = {}
                    for number, (stage, match) in enumerate(zip(STAGES, parsed), 1):
                        require(int(match[1]) == number and match[2] == stage and int(match[4]) == (4 if stage == "p2" else 2),
                                "Xazz 단계 순서·열 수 불일치")
                        actual[stage] = int(match[3])
                    check_row_counts(actual, oracle["result_rows"])
                    markers = [line[len("[xazz:result] "):] for line in stdout.splitlines() if line.startswith("[xazz:result] ")]
                    require(len(markers) == 1, "Xazz P7 결과 마커 누락·중복")
                    final = load_json(markers[0])
                    require([column["name"] for column in final["schema"]] == ["station", "pm25"], "P7 결과 스키마 불일치")
                    check_p7(final["rows"], oracle["p7_counts"])
                checked.append({"engine": engine, "run": index, "warmup": index == 0, "log_base": str(base)})
        report["scales"][scale] = {"independent": oracle, "checked_runs": checked}
    return report


def self_test():
    with tempfile.TemporaryDirectory() as temporary:
        folder = Path(temporary)
        path, logs, results_path = folder / "scale_small.csv", folder / "logs", folder / "results.json"
        logs.mkdir()
        content = ("date,station,pm10,pm25\nd1,A,60,20\nd2,A,80,\nd3,B,40,15\nd4,B,130,30\n"
                   "d5,C,75,12\nd6,C,,30\nd7,C,120,25\nd8,B,50,15\nd9,A,60,10\nd10,C,70,\n"
                   "d11,B,90,20\nd12,A,100,20\n")
        path.write_text(content, encoding="utf-8")
        oracle = scan_csv(path)
        expected_rows = {"p2": 6, "p3": 3, "p4": 3, "p7": 3}
        require(oracle["rows"] == 12 and oracle["result_rows"] == expected_rows, "고정 입력 행 수 self-test 실패")
        require(oracle["p3_sums"] == {"A": 160, "B": 180, "C": 75}, "고정 P3 self-test 실패")
        require(oracle["p4_top10"] == [{"station": "A", "pm10": 80}, {"station": "C", "pm10": 75},
                                         {"station": "B", "pm10": 60}], "고정 P4 self-test 실패")
        require(oracle["p7_counts"] == {"A": 4, "B": 2, "C": 3}, "고정 P7 self-test 실패")
        results = {"small": {"rows": 12, **{key: {"runs": [{}]} for key in ("pandas", "xazz_streaming_on", "xazz_streaming_off")}},
                   "provenance": {"inputs": {"small": {"file": path.name, **{key: oracle[key] for key in ("rows", "bytes", "sha256")}}},
                                  "raw_logs": str(logs), "measured_runs": 1, "warmup_runs": 1, "streaming_requested_modes": ["on", "off"]}}
        results_path.write_text(json.dumps(results), encoding="utf-8")
        final = {"rows": [{"station": "A", "pm25": 4}, {"station": "C", "pm25": 3}, {"station": "B", "pm25": 2}],
                 "schema": [{"name": "station", "type": "str"}, {"name": "pm25", "type": "u32"}]}
        for engine in ("pandas", "xazz-on", "xazz-off"):
            for index in (0, 1):
                base = logs / f"scale_small-{engine}-{index}"
                Path(str(base) + ".measurement.json").write_text('{"status":"ok","exit_code":0}', encoding="utf-8")
                stdout = json.dumps({"result_rows": expected_rows}) if engine == "pandas" else "[xazz:result] " + json.dumps(final)
                stderr = "" if engine == "pandas" else "\n".join(
                    f"[xazz] Pipeline #{number} '{stage}' 완료: {expected_rows[stage]} × {4 if stage == 'p2' else 2}"
                    for number, stage in enumerate(STAGES, 1))
                Path(str(base) + ".stdout.log").write_text(stdout, encoding="utf-8")
                Path(str(base) + ".stderr.log").write_text(stderr, encoding="utf-8")
        require(len(verify(results_path, folder)["scales"]["small"]["checked_runs"]) == 6, "전체 로그 대조 self-test 실패")
        rejections = 0

        def rejects(action):
            nonlocal rejections
            try:
                action()
            except (ValueError, OSError, KeyError, csv.Error):
                rejections += 1
            else:
                raise AssertionError("오류를 거부하지 않은 self-test")

        counts = dict(zip("ABCDEFG", (9, 8, 7, 6, 5, 5, 4)))
        rows = [{"station": station, "pm25": counts[station]} for station in "ABCDF"]
        check_p7(rows, counts)
        rejects(lambda: check_p7(rows[:-1], counts))
        rejects(lambda: check_p7(rows[::-1], counts))
        rejects(lambda: check_p7(rows[:-1] + [rows[0]], counts))
        rejects(lambda: check_p7(rows[:-1] + [{"station": "G", "pm25": 4}], counts))
        rejects(lambda: check_p7(rows[:-1] + [{"station": "F", "pm25": 6}], counts))
        for text in ('{"a":1,"a":2}', '{"a":NaN}', '{"a":1e999}', "broken"):
            rejects(lambda text=text: load_json(text))
        target = logs / "scale_small-xazz-on-0.stderr.log"
        previous = target.read_text(encoding="utf-8")
        target.write_text(previous.replace("'p2' 완료: 6", "'p2' 완료: 5"), encoding="utf-8")
        rejects(lambda: verify(results_path, folder))
        target.write_text(previous, encoding="utf-8")
        (logs / "scale_small-pandas-1.stdout.log").unlink()
        rejects(lambda: verify(results_path, folder))
        for invalid in ("d,A,NaN,20", "d,A,inf,20", "d,A,1e999,20", "d,A,1_0,20", "d,A,60,20,extra", 'd,"unfinished,60,20', "d,,60,20"):
            path.write_text("date,station,pm10,pm25\n" + invalid + "\n", encoding="utf-8")
            rejects(lambda: scan_csv(path))
    return {"status": "ok", "self_test": True, "fixture_rows": 12, "verified_runs": 6,
            "rejected_invalid_cases": rejections, "tie_cutoff_alternative_verified": True}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--results", type=Path)
    parser.add_argument("--data-dir", type=Path, default=ROOT / "benches" / "data")
    parser.add_argument("--logs-dir", type=Path, help="원본 결과를 바꾸지 않고 대조할 원시 로그 디렉터리를 명시")
    parser.add_argument("--out", type=Path)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if not args.self_test and args.results is None:
        parser.error("--results 또는 --self-test가 필요합니다")
    output = args.out or (Path(__file__).with_name("verify-self-test.json") if args.self_test
                         else args.results.with_suffix(".verification.json"))
    if args.results is not None and output.resolve() == args.results.resolve():
        parser.error("검증 결과는 원본 측정 결과와 다른 파일이어야 합니다")
    try:
        report = self_test() if args.self_test else verify(args.results, args.data_dir, args.logs_dir)
    except Exception as error:
        report = {"status": "failed", "error": f"{type(error).__name__}: {error}",
                  "results": str(args.results), "limitations": LIMITS}
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False), encoding="utf-8")
    print(json.dumps({"status": report["status"], "report": str(output.resolve()), "error": report.get("error")}, ensure_ascii=False))
    return 0 if report["status"] == "ok" else 1


if __name__ == "__main__":
    raise SystemExit(main())
