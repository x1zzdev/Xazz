"""정규화된 CSV에서 Xazz와 같은 P2·P3·P4·P7을 계산하는 pandas 기준선.

입력 계약은 date, station, pm10, pm25 순서의 네 열이다. 숫자 결측값은
빈 셀로만 표현하고 잘못된 숫자나 무한대, 빈 날짜·측정소는 오류로 처리한다.
날짜·측정소의 -, 점검중, N/A도 Xazz가 null로 해석하므로 명시적으로 거부한다.
문자열 NA를 결측값으로 바꾸지 않는다. 원본 데이터의 행 폭·결측 토큰 정규화는
make_scale_data.py에서 측정 전에 수행한다. 측정 중 CSV를 다시 훑지 않는다.

UTF-8 해독에 실패한 경우에만 EUC-KR을 시도한다. 네 개의 한국어 열 이름도
지원하지만 인코딩 전환이나 열 이름 변환이 발생하면 stderr에 명시한다.
지연에는 CSV 읽기·형 검사·계산을 포함하고 프로세스 시작·import·결과 출력은
포함하지 않는다. 원래 파이프라인의 필터·집계·정렬 의미는 유지한다.

사용법: python benches/pandas_pipeline.py <정규화된 CSV>
stdout은 total_latency_ms를 포함하는 JSON 한 줄이며 오류 시 종료 코드는 1이다.
프로세스 트리의 최대 RSS는 호출자가 폴링한다. 호환 필드 peak_memory_mb는
종료 시점 RSS이며 실제 최대값이 아니므로 memory_measurement에 이를 명시한다.
"""
from __future__ import annotations

import json
import os
from pathlib import Path
import sys
import time
import warnings

import numpy as np
import pandas as pd
import psutil


COLUMNS = ("date", "station", "pm10", "pm25")
RESERVED_TEXT_NULLS = frozenset(("-", "점검중", "N/A"))
VALIDATION_CHUNK_ROWS = 65_536
RENAME_MAP = {
    "일시": "date",
    "구분": "station",
    "미세먼지(PM10)": "pm10",
    "초미세먼지(PM25)": "pm25",
    "초미세먼지(PM2.5)": "pm25",
}


def load_csv(path: str | Path) -> pd.DataFrame:
    """정규화 계약을 검사하며 읽는다. 숫자를 null로 보정하지 않는다."""
    dtypes = {name: "float64" if name in ("pm10", "pm25") else "str" for name in COLUMNS}
    dtypes.update({source: dtypes[target] for source, target in RENAME_MAP.items()})
    nulls = {name: [""] for name, dtype in dtypes.items() if dtype == "float64"}
    for encoding in ("utf-8", "euc-kr"):
        try:
            with warnings.catch_warnings():
                # index_col=False에서 열을 잘라 버리겠다는 경고는 측정 실패다.
                # 그 외 경고는 기본 동작대로 stderr에 남긴다.
                warnings.simplefilter("error", pd.errors.ParserWarning)
                frame = pd.read_csv(
                    path, encoding=encoding, low_memory=False,
                    keep_default_na=False, na_values=nulls, dtype=dtypes,
                    index_col=False, on_bad_lines="error",
                )
            break
        except UnicodeDecodeError:
            if encoding == "euc-kr":
                raise
            print("[pandas 입력] UTF-8 해독 실패: EUC-KR로 다시 읽습니다", file=sys.stderr)

    if any(column in RENAME_MAP for column in frame.columns):
        print("[pandas 입력] 한국어 열 이름을 표준 열 이름으로 변환합니다", file=sys.stderr)
        frame = frame.rename(columns=RENAME_MAP)
    if tuple(frame.columns) != COLUMNS:
        raise ValueError(f"열 이름과 순서가 {list(COLUMNS)}여야 합니다: {list(frame.columns)}")
    if frame.empty:
        raise ValueError("측정할 데이터 행이 없습니다")
    # 전체 문자열 열의 strip 복사나 전체 고유값 집합을 만들지 않는다.
    # 날짜가 모두 다르더라도 검증의 임시 메모리는 청크 크기로 제한한다.
    for name in ("date", "station"):
        for start in range(0, len(frame), VALIDATION_CHUNK_ROWS):
            values = frame[name].iloc[start:start + VALIDATION_CHUNK_ROWS].unique()
            if any(not value.strip() or value in RESERVED_TEXT_NULLS for value in values):
                raise ValueError(f"{name}에 빈 값·공백 또는 Xazz의 null 토큰이 있습니다")
    for name in ("pm10", "pm25"):
        if np.isinf(frame[name].to_numpy(copy=False)).any():
            raise ValueError(f"{name}에 무한대가 있습니다")
    return frame


def run_pipelines(frame: pd.DataFrame) -> dict[str, pd.DataFrame]:
    """동일한 원시 프레임에서 네 계산 결과를 보존한다."""
    p2 = frame.dropna(subset=["pm10"])
    p2 = p2[p2["pm10"] < 120]
    p2 = p2[p2["pm25"] > 10]
    p3 = p2.groupby("station", as_index=False).agg(pm10=("pm10", "sum"))
    p4 = (
        p2.groupby("station", as_index=False)
        .agg(pm10=("pm10", "mean"))
        .sort_values("pm10", ascending=False)
        .head(10)
        .reset_index(drop=True)
    )
    p7 = frame.copy()
    p7["pm25"] = p7["pm25"].fillna(0)
    p7 = p7[p7["pm10"] > 50]
    p7 = (
        p7.groupby("station", as_index=False)
        .agg(pm25=("pm25", "count"))
        .sort_values("pm25", ascending=False)
        .head(5)
        .reset_index(drop=True)
    )
    return {"p2": p2, "p3": p3, "p4": p4, "p7": p7}


def benchmark(path: str | Path) -> dict:
    proc = psutil.Process(os.getpid())
    started = time.perf_counter()
    frame = load_csv(path)
    results = run_pipelines(frame)
    elapsed_ms = (time.perf_counter() - started) * 1000.0
    # frame과 네 결과가 살아 있는 시점이다. 이 값은 최대 RSS가 아니다.
    end_rss_mb = proc.memory_info().rss / (1024 * 1024)
    return {
        "total_latency_ms": round(elapsed_ms, 4),
        "peak_memory_mb": round(end_rss_mb, 4),
        "memory_measurement": "end_rss_not_peak",
        "result_rows": {name: len(result) for name, result in results.items()},
    }


def main(argv: list[str] | None = None) -> int:
    arguments = sys.argv[1:] if argv is None else argv
    if len(arguments) != 1:
        print(json.dumps({"error": "정규화된 CSV 경로 하나가 필요합니다"}, ensure_ascii=False))
        return 1
    try:
        result = benchmark(arguments[0])
    except Exception as error:
        print(json.dumps({"error": str(error)}, ensure_ascii=False))
        return 1
    print(json.dumps(result, ensure_ascii=False, allow_nan=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
