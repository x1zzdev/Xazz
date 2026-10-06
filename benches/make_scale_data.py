"""서울 대기환경 CSV를 고정 순서의 UTF-8 벤치마크 데이터로 변환한다.

기본 스케일은 small/medium/large다. --scale xlarge는 large 원본을 49회
반복하며, --xlarge는 기본 세 스케일에 xlarge를 추가하는 기존 사용법이다.
원본은 청크로 정규화하고 파일 조각을 반복 복사하므로 200M 행을 메모리에
한꺼번에 올리지 않는다. 입력 검증 후 완성된 파일만 원자적으로 교체한다.

예시:
    python benches/make_scale_data.py --scale small --source-dir examples/data
    python benches/make_scale_data.py --scale xlarge --output-dir /tmp/bench-data
    python benches/make_scale_data.py --synthetic --rows 1000000
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
from collections import Counter
import csv
import codecs
import os
import json
from pathlib import Path
import shutil
import sys
import tempfile

import numpy as np
import pandas as pd

ROOT = Path(__file__).parent.parent.resolve()
SRC = ROOT / "examples" / "data"
OUT = ROOT / "benches" / "data"
COLUMNS = ("date", "station", "pm10", "pm25")
DEFAULT_CHUNK_ROWS = 100_000
DEFAULT_SYNTHETIC_ROWS = 227_760
# 실행 엔진이 모든 열에서 null로 읽는 토큰은 문자열 열에서 허용하지 않는다.
STRING_NULL_TOKENS = frozenset({"", " ", "-", "점검중", "N/A"})
# 숫자 열에만 기존 CSV null 토큰을 적용한다. station의 NA 같은 이름은 보존한다.
NUMERIC_NULL_VALUES = frozenset({
    "", " ", "-", "점검중", "#N/A", "#N/A N/A", "#NA", "-1.#IND", "-1.#QNAN", "-NaN", "-nan",
    "1.#IND", "1.#QNAN", "<NA>", "N/A", "NA", "NULL", "NaN", "None", "n/a", "nan", "null",
})

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
REPEATS = {"small": 1, "medium": 1, "large": 1, "xlarge": 49}


def source_names(label: str) -> list[str]:
    return GROUPS["large" if label == "xlarge" else label]


def source_encoding(path: Path) -> str:
    """지원하는 UTF-8/EUC-KR 전체 바이트를 엄격하게 검사한다."""
    if not path.is_file():
        raise ValueError(f"{path}: 원본 CSV 파일이 없습니다. Git LFS 파일을 내려받았는지 확인하세요")
    with path.open("rb") as stream:
        if stream.read(128).removeprefix(b"\xef\xbb\xbf").startswith(b"version https://git-lfs.github.com/spec/v1"):
            raise ValueError(f"{path}: 실제 CSV 대신 Git LFS 포인터가 있습니다. git lfs pull이 필요합니다")
    for encoding in ("utf-8-sig", "euc-kr"):
        decoder = codecs.getincrementaldecoder(encoding)(errors="strict")
        try:
            with path.open("rb") as stream:
                while block := stream.read(1_048_576):
                    decoder.decode(block)
                decoder.decode(b"", final=True)
        except UnicodeDecodeError:
            continue
        return encoding
    raise ValueError(f"{path}: UTF-8 또는 EUC-KR로 해석할 수 없는 데이터입니다")


def inspect_source(path: Path) -> tuple[str, dict[str, str], int]:
    """열 누락·중복·별칭 충돌과 잘못된 CSV 레코드를 보정 없이 거부한다."""
    encoding = source_encoding(path)
    try:
        with path.open(encoding=encoding, newline="") as stream:
            reader = csv.reader(stream, strict=True)
            header = next(reader, None)
            if not header or len(set(header)) != len(header):
                raise ValueError("헤더가 없거나 같은 이름의 열이 중복됐습니다")
            selected = {}
            for name in header:
                canonical = RENAME.get(name, name)
                if canonical in COLUMNS:
                    if canonical in selected.values():
                        raise ValueError(f"'{canonical}'에 대응하는 열이 둘 이상입니다")
                    selected[name] = canonical
            missing = set(COLUMNS) - set(selected.values())
            if missing:
                raise ValueError(f"필수 열이 없습니다: {', '.join(sorted(missing))}")
            rows = 0
            for record in reader:
                if len(record) != len(header):
                    raise ValueError(
                        f"CSV {reader.line_num}번째 줄의 열 개수 {len(record)}가 헤더 {len(header)}와 다릅니다"
                    )
                rows += 1
            if rows == 0:
                raise ValueError("측정할 데이터 행이 없습니다")
            return encoding, selected, rows
    except (ValueError, csv.Error) as error:
        raise ValueError(f"{path}: {error}") from error


def normalized_chunks(
    path: Path, spec: tuple[str, dict[str, str], int], chunk_rows: int,
    null_tokens: dict[str, Counter] | None = None,
):
    """숫자 열의 허용 null 토큰만 정규화하고 그 횟수를 보고한다."""
    encoding, selected, _ = spec
    try:
        with pd.read_csv(
            path, encoding=encoding, usecols=list(selected), dtype="string",
            chunksize=chunk_rows, on_bad_lines="error", keep_default_na=False,
        ) as reader:
            for frame in reader:
                frame = frame.rename(columns=selected).loc[:, list(COLUMNS)]
                for name in ("date", "station"):
                    strings = frame[name]
                    if (strings.str.strip().eq("") | strings.isin(STRING_NULL_TOKENS)).any():
                        raise ValueError(f"{name}에 빈 값 또는 실행 엔진이 null로 해석하는 문자열이 있습니다")
                for name in ("pm10", "pm25"):
                    raw = frame[name]
                    missing = raw.isin(NUMERIC_NULL_VALUES)
                    if null_tokens is not None:
                        null_tokens[name].update(raw[missing].value_counts().to_dict())
                    values = pd.to_numeric(raw.mask(missing), errors="raise")
                    if not np.isfinite(values.dropna().to_numpy(dtype=float)).all():
                        raise ValueError(f"'{name}'에 유한하지 않은 숫자가 있습니다")
                    frame[name] = values
                yield frame
    except (ValueError, pd.errors.ParserError) as error:
        raise ValueError(f"{path}: {error}") from error


def load_frame(path: Path) -> pd.DataFrame:
    """단일 원본을 읽는 호환 함수. 대용량 생성은 normalized_chunks를 사용한다."""
    spec = inspect_source(path)
    return pd.concat(normalized_chunks(path, spec, DEFAULT_CHUNK_ROWS), ignore_index=True)


@contextmanager
def atomic_output(dest: Path):
    """같은 디렉터리에 완성한 임시 파일만 교체하고 실패 시 임시 파일을 지운다."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    stream = tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", newline="", prefix=f".{dest.name}.", suffix=".tmp",
        dir=dest.parent, delete=False,
    )
    temporary = Path(stream.name)
    try:
        with stream:
            yield stream
        os.replace(temporary, dest)
    finally:
        temporary.unlink(missing_ok=True)


def generate_real(labels: list[str], source_dir: Path, output_dir: Path, chunk_rows: int) -> None:
    names = list(dict.fromkeys(name for label in labels for name in source_names(label)))
    # 전체 선택 입력을 먼저 검사해 뒤쪽 파일 오류가 기존 결과를 덮어쓰지 않게 한다.
    specs = {name: inspect_source(source_dir / name) for name in names}
    output_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".scale-inputs-", dir=output_dir) as directory:
        pieces = {}
        counts = {}
        source_nulls = {}
        for index, name in enumerate(names):
            piece = Path(directory) / f"{index}.csv"
            count = 0
            null_tokens = {column: Counter() for column in ("pm10", "pm25")}
            with piece.open("w", encoding="utf-8", newline="") as stream:
                for chunk in normalized_chunks(source_dir / name, specs[name], chunk_rows, null_tokens):
                    chunk.to_csv(stream, index=False, header=False, lineterminator="\n")
                    count += len(chunk)
            if count != specs[name][2]:
                raise ValueError(f"{source_dir / name}: 검사한 행 수와 읽은 행 수가 다릅니다")
            pieces[name], counts[name], source_nulls[name] = piece, count, null_tokens
            print("[scale-source] " + json.dumps({
                "path": str(source_dir / name), "encoding": specs[name][0],
                "rows": count, "bytes": (source_dir / name).stat().st_size,
                "numeric_null_tokens": null_tokens,
            }, ensure_ascii=False, sort_keys=True))

        for label in labels:
            dest = output_dir / f"scale_{label}.csv"
            with atomic_output(dest) as stream:
                csv.writer(stream, lineterminator="\n").writerow(COLUMNS)
                for _ in range(REPEATS[label]):
                    for name in source_names(label):
                        with pieces[name].open(encoding="utf-8", newline="") as source:
                            shutil.copyfileobj(source, stream, length=1_048_576)
            rows = sum(counts[name] for name in source_names(label)) * REPEATS[label]
            combined_nulls = {column: Counter() for column in ("pm10", "pm25")}
            for name in source_names(label):
                for column in combined_nulls:
                    combined_nulls[column].update({
                        token: count * REPEATS[label]
                        for token, count in source_nulls[name][column].items()
                    })
            print("[scale-output] " + json.dumps({
                "scale": label, "path": str(dest), "rows": rows, "bytes": dest.stat().st_size,
                "repeats": REPEATS[label], "numeric_null_tokens": combined_nulls,
            }, ensure_ascii=False, sort_keys=True))


def make_synthetic(
    label: str, dest: Path, *, rows: int = DEFAULT_SYNTHETIC_ROWS,
    chunk_rows: int = DEFAULT_CHUNK_ROWS,
) -> None:
    """기존 고정 시드·난수 생성 순서·null 비율을 유지하는 small 합성 데이터."""
    if label != "small":
        raise ValueError(f"--synthetic은 small 스케일만 지원합니다: {label}")
    if rows <= 0 or chunk_rows <= 0:
        raise ValueError("행 수와 청크 행 수는 양의 정수여야 합니다")
    rng = np.random.default_rng(20240904)
    stations = [f"S{i:03d}" for i in range(25)]
    dates = pd.date_range("2022-01-01", periods=rows, freq="min").astype(str)
    frame = pd.DataFrame({
        "date": dates,
        "station": rng.choice(stations, size=rows),
        "pm10": np.round(rng.normal(60.0, 40.0, rows), 1),
        "pm25": np.round(rng.normal(30.0, 20.0, rows), 1),
    })
    missing = rng.random(rows) < 0.03
    frame.loc[missing, "pm10"] = np.nan
    frame.loc[missing, "pm25"] = np.nan
    with atomic_output(dest) as stream:
        frame.to_csv(stream, index=False, lineterminator="\n", chunksize=chunk_rows)
    print("[scale-output] " + json.dumps({
        "scale": label, "path": str(dest), "rows": rows, "bytes": dest.stat().st_size,
        "synthetic": True, "seed": 20240904,
        "generated_nulls": {column: int(missing.sum()) for column in ("pm10", "pm25")},
    }, ensure_ascii=False, sort_keys=True))


def positive_int(raw: str) -> int:
    try:
        value = int(raw)
    except ValueError as error:
        raise argparse.ArgumentTypeError("양의 정수가 필요합니다") from error
    if value <= 0:
        raise argparse.ArgumentTypeError("양의 정수가 필요합니다")
    return value


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="벤치마크 스케일 CSV 생성", allow_abbrev=False)
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--scale", choices=tuple(REPEATS), help="선택한 스케일 하나만 생성")
    selection.add_argument("--xlarge", action="store_true", help="기본 세 스케일에 xlarge 추가")
    parser.add_argument("--synthetic", action="store_true", help="원본 없이 합성 small 데이터 생성")
    parser.add_argument("--rows", type=positive_int, help="합성 데이터 행 수 (기본 227760)")
    parser.add_argument("--source-dir", type=Path, default=SRC, help="원본 CSV 디렉터리")
    parser.add_argument("--output-dir", type=Path, default=OUT, help="생성 파일 디렉터리")
    parser.add_argument("--chunk-rows", type=positive_int, default=DEFAULT_CHUNK_ROWS,
                        help="원본 처리 청크 행 수 (기본 100000)")
    args = parser.parse_args(argv)
    if args.rows is not None and not args.synthetic:
        parser.error("--rows는 --synthetic과 함께 사용해야 합니다")
    if args.synthetic and (args.xlarge or args.scale not in (None, "small")):
        parser.error("--synthetic은 small 스케일만 지원하며 --xlarge와 함께 사용할 수 없습니다")
    try:
        if args.synthetic:
            make_synthetic("small", args.output_dir / "scale_small.csv",
                           rows=args.rows if args.rows is not None else DEFAULT_SYNTHETIC_ROWS,
                           chunk_rows=args.chunk_rows)
        else:
            labels = [args.scale] if args.scale else ["small", "medium", "large"]
            if args.xlarge:
                labels.append("xlarge")
            generate_real(labels, args.source_dir, args.output_dir, args.chunk_rows)
    except (OSError, ValueError) as error:
        print(f"[scale-data] 입력 또는 생성 오류: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
