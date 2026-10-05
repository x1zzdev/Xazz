"""스케일 생성기의 명시적 선택, CSV 의미, 실패 시 파일 보존 검증."""
from __future__ import annotations

import contextlib
import csv
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

import make_scale_data as generator


SCRIPT = Path(__file__).with_name("make_scale_data.py")
COLUMNS = ["date", "station", "pm10", "pm25"]


def read_rows(path):
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream)
        if reader.fieldnames != COLUMNS:
            raise AssertionError(reader.fieldnames)
        return [
            (row["date"], row["station"],
             None if row["pm10"] == "" else float(row["pm10"]),
             None if row["pm25"] == "" else float(row["pm25"]))
            for row in reader
        ]


class ScaleDataTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="scale-contract-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.output = self.root / "output"
        self.source.mkdir()
        self.expected = {}
        for i, name in enumerate(generator.GROUPS["large"]):
            # 입력 열 순서를 바꾸고 UTF-8/EUC-KR을 섞어 위치 의존을 탐지한다.
            path = self.source / name
            rows = [(f"2022-01-{i+1:02d}", f"서울{i}", i + 0.5, None),
                    (f"2022-02-{i+1:02d}", f"부산{i}", None, i + 1.25)]
            self.expected[name] = rows
            if i % 2:
                header = ["station", "pm25", "date", "pm10"]
                records = [[s, b, d, a] for d, s, a, b in rows]
                encoding = "utf-8"
            else:
                header = ["초미세먼지(PM2.5)", "일시", "미세먼지(PM10)", "구분"]
                records = [[b, d, a, s] for d, s, a, b in rows]
                encoding = "euc-kr"
            with path.open("w", encoding=encoding, newline="") as stream:
                writer = csv.writer(stream)
                writer.writerow(header)
                writer.writerows(records)

    def run_main(self, *arguments):
        out, error = io.StringIO(), io.StringIO()
        args = ["--source-dir", str(self.source), "--output-dir", str(self.output), *arguments]
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(error):
            code = generator.main(args)
        return code, out.getvalue(), error.getvalue()

    def expected_scale(self, label):
        names = generator.GROUPS["large" if label == "xlarge" else label]
        return [row for name in names for row in self.expected[name]] * (49 if label == "xlarge" else 1)

    def test_xlarge_is_large_sources_repeated_49_times_with_bounded_chunks(self):
        original = generator.pd.read_csv
        observed = []
        def read_csv(*args, **kwargs):
            observed.append(kwargs.get("chunksize"))
            return original(*args, **kwargs)
        with patch.object(generator.pd, "read_csv", side_effect=read_csv), patch.object(
            generator.pd, "concat", side_effect=AssertionError("전체 프레임 결합 금지")
        ):
            code, _, error = self.run_main("--scale", "xlarge", "--chunk-rows", "1")
        self.assertEqual(code, 0, error)
        self.assertEqual(list(self.output.glob("scale_*.csv")), [self.output / "scale_xlarge.csv"])
        self.assertEqual(read_rows(self.output / "scale_xlarge.csv"), self.expected_scale("xlarge"))
        self.assertEqual(observed, [1] * 8)

    def test_default_and_legacy_xlarge_selection(self):
        for args, names in (((), ["small", "medium", "large"]),
                            (("--xlarge",), ["small", "medium", "large", "xlarge"])):
            code, _, error = self.run_main(*args, "--chunk-rows", "3")
            self.assertEqual(code, 0, error)
            self.assertEqual(sorted(p.stem[6:] for p in self.output.glob("scale_*.csv")), sorted(names))
            for label in names:
                self.assertEqual(read_rows(self.output / f"scale_{label}.csv"), self.expected_scale(label))
                (self.output / f"scale_{label}.csv").unlink()

    def test_missing_and_pointer_input_preserve_existing_output(self):
        path = self.source / generator.GROUPS["small"][0]
        self.output.mkdir()
        dest = self.output / "scale_small.csv"
        for content in (None, b"version https://git-lfs.github.com/spec/v1\noid sha256:abc\nsize 42\n"):
            if content is None:
                path.unlink()
            else:
                path.write_bytes(content)
            dest.write_bytes(b"existing-result")
            code, _, error = self.run_main("--scale", "small")
            self.assertEqual(code, 1)
            self.assertIn(path.name, error)
            self.assertEqual(dest.read_bytes(), b"existing-result")
            self.assertEqual(list(self.output.iterdir()), [dest])

    def test_invalid_csv_is_not_corrected_and_existing_output_survives(self):
        invalid = [
            b"date,station,pm10\n2022,A,1\n",
            b"date,station,pm10,pm10,pm25\n2022,A,1,2,3\n",
            b"date,station,pm10,pm25\n2022,A,not-a-number,2\n",
            b"date,station,pm10,pm25\n2022,A,inf,2\n",
            b"date,station,pm10,pm25\n2022,A,1,2,extra\n",
            b"date,station,pm10,pm25\n2022,A,1\n",
            b"date,station,pm10,pm25\n",
            b"date,station,pm10,pm25\n\xff\xff\xff\n",
            '일시,구분,미세먼지(PM10),초미세먼지(PM25),초미세먼지(PM2.5)\n2022,A,1,2,3\n'.encode(),
        ]
        path = self.source / generator.GROUPS["small"][0]
        self.output.mkdir()
        dest = self.output / "scale_small.csv"
        for content in invalid:
            with self.subTest(content=content):
                path.write_bytes(content)
                dest.write_bytes(b"existing-result")
                code, _, error = self.run_main("--scale", "small", "--chunk-rows", "1")
                self.assertEqual(code, 1, error)
                self.assertIn(path.name, error)
                self.assertEqual(dest.read_bytes(), b"existing-result")
                self.assertEqual(list(self.output.iterdir()), [dest])

    def test_station_null_tokens_are_names_and_numeric_nulls_are_normalized(self):
        path = self.source / generator.GROUPS["small"][0]
        path.write_text("date,station,pm10,pm25\n2022,NA,NA,1\n2023,NULL,2,null\n", encoding="utf-8")
        code, _, error = self.run_main("--scale", "small", "--chunk-rows", "1")
        self.assertEqual(code, 0, error)
        self.assertEqual(read_rows(self.output / "scale_small.csv"),
                         [("2022", "NA", None, 1.0), ("2023", "NULL", 2.0, None)])
        for station in ("", "   "):
            path.write_text(f"date,station,pm10,pm25\n2022,{station},1,2\n", encoding="utf-8")
            before = (self.output / "scale_small.csv").read_bytes()
            code, _, error = self.run_main("--scale", "small")
            self.assertEqual(code, 1, error)
            self.assertIn("station", error)
            self.assertEqual((self.output / "scale_small.csv").read_bytes(), before)

    def test_string_null_tokens_are_rejected_before_engine_can_change_them(self):
        path = self.source / generator.GROUPS["small"][0]
        for column in ("date", "station"):
            for token in ("", " ", "   ", "-", "점검중", "N/A"):
                with self.subTest(column=column, token=token):
                    row = {"date": "2022", "station": "station", "pm10": "1", "pm25": "2"}
                    row[column] = token
                    with path.open("w", encoding="utf-8", newline="") as stream:
                        writer = csv.DictWriter(stream, fieldnames=COLUMNS)
                        writer.writeheader()
                        writer.writerow(row)
                    code, _, error = self.run_main("--scale", "small")
                    self.assertEqual(code, 1, error)
                    self.assertIn(column, error)
                    self.assertFalse((self.output / "scale_small.csv").exists())

    def test_allowed_null_token_conversions_are_reported_explicitly(self):
        path = self.source / generator.GROUPS["small"][0]
        with path.open("w", encoding="utf-8", newline="") as stream:
            writer = csv.writer(stream)
            writer.writerow(COLUMNS)
            for token in ("", " ", "-", "점검중", "N/A"):
                writer.writerow(["2022", "station", token, "1"])
        code, out, error = self.run_main("--scale", "small", "--chunk-rows", "2")
        self.assertEqual(code, 0, error)
        source = json.loads(next(line.split("] ", 1)[1] for line in out.splitlines()
                                 if line.startswith("[scale-source] ")))
        output = json.loads(next(line.split("] ", 1)[1] for line in out.splitlines()
                                 if line.startswith("[scale-output] ")))
        expected = {token: 1 for token in ("", " ", "-", "점검중", "N/A")}
        self.assertEqual(source["numeric_null_tokens"]["pm10"], expected)
        self.assertEqual(output["numeric_null_tokens"]["pm10"], expected)
        self.assertEqual(source["rows"], 5)
        self.assertEqual(output["rows"], 5)
        self.assertEqual(output["bytes"], (self.output / "scale_small.csv").stat().st_size)
        self.assertTrue(all(row[2] is None for row in read_rows(self.output / "scale_small.csv")))

    def test_late_invalid_source_does_not_replace_any_existing_scale(self):
        self.output.mkdir()
        existing = self.output / "scale_small.csv"
        existing.write_bytes(b"existing-result")
        (self.source / generator.GROUPS["large"][-1]).write_text("date,station,pm10,pm25\n2022,A,bad,2\n")
        code, _, error = self.run_main()
        self.assertEqual(code, 1, error)
        self.assertEqual(existing.read_bytes(), b"existing-result")
        self.assertEqual(list(self.output.iterdir()), [existing])

    def test_atomic_replace_failure_keeps_old_file_and_cleans_temporary_files(self):
        self.output.mkdir()
        dest = self.output / "scale_small.csv"
        dest.write_bytes(b"existing-result")
        with patch.object(generator.os, "replace", side_effect=OSError("쓰기 실패")):
            code, _, error = self.run_main("--scale", "small")
        self.assertEqual(code, 1, error)
        self.assertEqual(dest.read_bytes(), b"existing-result")
        self.assertEqual(list(self.output.iterdir()), [dest])

    def test_invalid_options_are_rejected_before_any_output(self):
        cases = [
            ("--unknown",), ("--sc", "small"), ("--scale", "unknown"), ("--chunk-rows", "0"),
            ("--chunk-rows", "-2"), ("--rows", "10"),
            ("--synthetic", "--rows", "0"), ("--scale", "small", "--xlarge"),
            ("--synthetic", "--scale", "medium"), ("--synthetic", "--xlarge"),
        ]
        for args in cases:
            with self.subTest(args=args):
                with self.assertRaises(SystemExit) as failure:
                    self.run_main(*args)
                self.assertEqual(failure.exception.code, 2)
                self.assertFalse(self.output.exists())

    def test_synthetic_small_row_override_is_deterministic(self):
        code, _, error = self.run_main("--synthetic", "--rows", "64", "--chunk-rows", "7")
        self.assertEqual(code, 0, error)
        dest = self.output / "scale_small.csv"
        first = dest.read_bytes()
        rows = read_rows(dest)
        self.assertEqual(len(rows), 64)
        self.assertEqual(rows[0][0], "2022-01-01 00:00:00")
        self.assertTrue(any(row[2] is None for row in rows))
        self.assertTrue(all((row[2] is None) == (row[3] is None) for row in rows))
        code, _, error = self.run_main("--synthetic", "--rows", "64", "--chunk-rows", "13")
        self.assertEqual(code, 0, error)
        self.assertEqual(dest.read_bytes(), first)

    def test_synthetic_default_size_is_preserved(self):
        with patch.object(generator, "make_synthetic") as make:
            code, _, error = self.run_main("--synthetic")
        self.assertEqual(code, 0, error)
        self.assertEqual(make.call_args.kwargs["rows"], 227_760)

    def test_hash_seed_and_source_column_order_do_not_change_output_schema(self):
        results = []
        for seed in ("1", "7", "99"):
            out_dir = self.root / f"hash-{seed}"
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--scale", "small", "--source-dir", str(self.source),
                 "--output-dir", str(out_dir), "--chunk-rows", "1"],
                env=dict(os.environ, PYTHONHASHSEED=seed), text=True, capture_output=True, check=False,
            )
            self.assertEqual(result.returncode, 0, result.stderr)
            results.append((out_dir / "scale_small.csv").read_bytes())
            self.assertEqual(read_rows(out_dir / "scale_small.csv"), self.expected_scale("small"))
        self.assertTrue(all(value == results[0] for value in results))


if __name__ == "__main__":
    unittest.main()
