"""
benches/make_scale_data.py — README 벤치마크용 스케일 데이터셋 빌더
====================================================================
실제 서울시 대기환경 측정 데이터(공공데이터포털, examples/data/)를 읽어
UTF-8 · 영문 헤더(date, station, pm10, pm25) 스케일 파일 3종을 생성한다.

  scale_small.csv  : 2022년 1개 파일           (~228K rows)
  scale_medium.csv : 2020-2021 ~ 2023 3개 파일 (~912K rows)
  scale_large.csv  : 전체 8개 파일             (~4.09M rows)

사용법:
    python benches/make_scale_data.py
"""
import pandas as pd
from pathlib import Path

ROOT = Path(__file__).parent.parent.resolve()
SRC = ROOT / "examples" / "data"
OUT = ROOT / "benches" / "data"

RENAME = {
    "일시": "date",
    "구분": "station",
    "미세먼지(PM10)": "pm10",
    "초미세먼지(PM25)": "pm25",
    "초미세먼지(PM2.5)": "pm25",
}

GROUPS = {
    "small": ["seoul_air_2022.csv"],
    "medium": ["seoul_air_2020-2021.csv", "seoul_air_2022.csv", "seoul_air_2023.csv"],
    "large": [
        "seoul_air_2008_2011.csv", "seoul_air_2012_2015.csv", "seoul_air_2016-2019.csv",
        "seoul_air_2020-2021.csv", "seoul_air_2022.csv", "seoul_air_2023.csv",
        "seoul_air_2024.csv", "seoul_air_2026.csv",
    ],
}


def load_frame(path: Path) -> pd.DataFrame:
    """EUC-KR 원본(또는 UTF-8 정규본)을 읽어 표준 4컬럼으로 정규화한다."""
    df = pd.read_csv(path, encoding="euc-kr", low_memory=False)
    cols = [c for c in RENAME if c in df.columns]
    if cols:
        return df[cols].rename(columns=RENAME)
    canonical = {"date", "station", "pm10", "pm25"}
    if not canonical.issubset(df.columns):
        raise SystemExit(f"{path.name}: 표준 컬럼을 찾을 수 없습니다 — {list(df.columns)}")
    return df[list(canonical)]


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    for label, files in GROUPS.items():
        frames = []
        for name in files:
            df = load_frame(SRC / name)
            frames.append(df)
            print(f"  {label}/{name}: {len(df):,} rows")
        merged = pd.concat(frames, ignore_index=True)
        dest = OUT / f"scale_{label}.csv"
        merged.to_csv(dest, index=False, encoding="utf-8")
        size_mb = dest.stat().st_size / 1_048_576
        print(f"== {label}: {len(merged):,} rows -> {dest.name} ({size_mb:.1f} MB)")


if __name__ == "__main__":
    main()
