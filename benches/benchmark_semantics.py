"""작은 고정 입력과 DataFrame을 사용하지 않는 네 단계 셀 정답."""
from collections import Counter, defaultdict
import math

COLUMNS = ("date", "station", "pm10", "pm25")
FIXTURE_ROWS = [
    ["d1", "A", 60, 20], ["d2", "A", 80, None],
    ["d3", "B", 40, 15], ["d4", "B", 130, 30],
    ["d5", "C", 75, 12], ["d6", "C", None, 30],
    ["d7", "C", 120, 25], ["d8", "B", 50, 15],
    ["d9", "A", 60, 10], ["d10", "C", 70, None],
    ["d11", "B", 90, 20], ["d12", "A", 100, 20],
]
TEXT_ROWS = [["NA", "NA", 80, 20], ["d2", "NULL", 60, None]]
# 12개 측정소의 평균과 계수를 모두 다르게 만들어 10위·5위 절단을 검사한다.
TOPK_ROWS = [[f"d{station:02d}-{index:02d}", f"S{station:02d}", 60.25 + station, 10.5]
             for station in range(1, 13) for index in range(1, station + 1)]
FIXTURES = {"null-boundaries": FIXTURE_ROWS, "literal-na-null": TEXT_ROWS, "topk-cutoffs": TOPK_ROWS}
STAGES = ("p2", "p3", "p4", "p7")


def oracle(rows):
    """필터·표준 합계·계수로 계산하며 엔진의 출력에 의존하지 않는다."""
    records = [dict(zip(COLUMNS, row)) for row in rows]
    p2 = [row for row in records if row["pm10"] is not None and row["pm10"] < 120
          and row["pm25"] is not None and row["pm25"] > 10]
    groups = defaultdict(list)
    for row in p2:
        groups[row["station"]].append(row["pm10"])
    p3 = [{"station": station, "pm10": math.fsum(values)} for station, values in groups.items()]
    p4 = sorted([{"station": station, "pm10": math.fsum(values) / len(values)}
                 for station, values in groups.items()], key=lambda row: -row["pm10"])[:10]
    counts = Counter(row["station"] for row in records if row["pm10"] is not None and row["pm10"] > 50)
    p7 = sorted([{"station": station, "pm25": count} for station, count in counts.items()],
                key=lambda row: -row["pm25"])[:5]
    return {"p2": p2, "p3": p3, "p4": p4, "p7": p7}


def assert_rows_equal(actual, expected):
    """순서가 정의되지 않은 행은 정렬해 대조하고 숫자는 유한값만 허용한다."""
    if not isinstance(actual, list) or len(actual) != len(expected) or any(not isinstance(row, dict) for row in actual):
        raise AssertionError(f"행 수·형식 불일치: {actual!r}")
    key = lambda row: (row["station"], str(row.get("date", "")))
    actual, expected = sorted(actual, key=key), sorted(expected, key=key)
    for left, right in zip(actual, expected):
        if left.keys() != right.keys():
            raise AssertionError(f"열 불일치: {left!r}, {right!r}")
        for field in left:
            a, b = left[field], right[field]
            if isinstance(b, (int, float)):
                equal = (type(a) in (int, float) and math.isfinite(a)
                         and math.isclose(a, b, rel_tol=1e-12, abs_tol=1e-12))
            else:
                equal = type(a) is type(b) and a == b
            if not equal:
                raise AssertionError(f"셀 불일치: {field}: {a!r} != {b!r}")


def check_stage_rows(stage, actual, expected):
    assert_rows_equal(actual, expected)
    if stage != "p2" and len({row["station"] for row in actual}) != len(actual):
        raise AssertionError("집계 측정소 중복")
    if stage in ("p4", "p7"):
        field = "pm10" if stage == "p4" else "pm25"
        values = [row[field] for row in actual]
        if values != sorted(values, reverse=True):
            raise AssertionError(f"{stage} 내림차순 불일치")
    if stage == "p7" and any(type(row["pm25"]) is not int for row in actual):
        raise AssertionError("P7 계수는 정수여야 합니다")
