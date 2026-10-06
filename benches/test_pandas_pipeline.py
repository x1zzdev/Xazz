"""측정 입력의 엄격한 검사와 pandas를 사용하지 않는 계산 대조."""
from __future__ import annotations

import contextlib
import csv
import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock
import warnings

import pandas_pipeline as pipeline

from benchmark_semantics import FIXTURE_ROWS, assert_rows_equal, oracle


class PipelineTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.csv = Path(self.directory.name) / "input.csv"

    def write_rows(self, rows, headers=pipeline.COLUMNS, encoding="utf-8"):
        with self.csv.open("w", encoding=encoding, newline="") as output:
            writer = csv.writer(output)
            writer.writerow(headers)
            writer.writerows(rows)

    def test_four_stages_match_independent_oracle(self):
        self.write_rows(FIXTURE_ROWS)
        actual = pipeline.run_pipelines(pipeline.load_csv(self.csv))
        for stage, expected in oracle(FIXTURE_ROWS).items():
            with self.subTest(stage=stage):
                assert_rows_equal(json.loads(actual[stage].to_json(orient="records")), expected)

    def test_literal_na_station_is_preserved(self):
        rows = [["NA", "NA", 80, 20], ["d2", "NULL", 60, None]]
        self.write_rows(rows)
        frame = pipeline.load_csv(self.csv)
        self.assertEqual(frame["date"].iloc[0], "NA")
        self.assertEqual(frame["station"].tolist(), ["NA", "NULL"])
        assert_rows_equal(pipeline.run_pipelines(frame)["p7"].to_dict(orient="records"),
                          oracle(rows)["p7"])

    def test_empty_or_whitespace_station_is_rejected(self):
        for station in ("", " ", "\t "):
            with self.subTest(station=station):
                self.write_rows([["d1", station, 60, 20]])
                with self.assertRaisesRegex(ValueError, "station"):
                    pipeline.load_csv(self.csv)

    def test_required_text_rejects_xazz_null_tokens(self):
        for index in (0, 1):
            for value in ("", " ", "\t ", "-", "점검중", "N/A"):
                with self.subTest(column=pipeline.COLUMNS[index], value=value):
                    row = ["d1", "A", 60, 20]
                    row[index] = value
                    self.write_rows([row])
                    with self.assertRaisesRegex(ValueError, pipeline.COLUMNS[index]):
                        pipeline.load_csv(self.csv)

    def test_empty_date_and_empty_dataset_are_rejected(self):
        self.write_rows([["", "A", 60, 20]])
        with self.assertRaisesRegex(ValueError, "date"):
            pipeline.load_csv(self.csv)
        self.write_rows([])
        with self.assertRaisesRegex(ValueError, "데이터 행"):
            pipeline.load_csv(self.csv)

    def test_junk_nonfinite_and_raw_null_tokens_are_rejected(self):
        for index in (2, 3):
            for value in ("oops", "-", "NA", "N/A", "NULL", "점검중", "NaN", "nan", "inf", "-inf"):
                with self.subTest(column=pipeline.COLUMNS[index], value=value):
                    row = ["d1", "A", 60, 20]
                    row[index] = value
                    self.write_rows([row])
                    with self.assertRaises(ValueError):
                        pipeline.load_csv(self.csv)

    def test_reordered_columns_are_rejected(self):
        self.write_rows([["d1", "A", 5, 80]], ("date", "station", "pm25", "pm10"))
        with self.assertRaisesRegex(ValueError, "열 이름과 순서"):
            pipeline.load_csv(self.csv)

    def test_euc_kr_decode_fallback_is_reported(self):
        self.write_rows([["d1", "서울", 60, 20]],
                        ("일시", "구분", "미세먼지(PM10)", "초미세먼지(PM2.5)"), "euc-kr")
        error = io.StringIO()
        with contextlib.redirect_stderr(error):
            frame = pipeline.load_csv(self.csv)
        self.assertEqual(frame["station"].tolist(), ["서울"])
        self.assertIn("EUC-KR", error.getvalue())
        self.assertIn("열 이름", error.getvalue())

    def test_parser_error_is_not_retried_as_encoding_error(self):
        with mock.patch.object(pipeline.pd, "read_csv", side_effect=pipeline.pd.errors.ParserError("bad CSV")) as read:
            with self.assertRaises(pipeline.pd.errors.ParserError):
                pipeline.load_csv(self.csv)
        self.assertEqual(read.call_count, 1)

    def test_extra_column_is_not_silently_truncated(self):
        self.csv.write_text("date,station,pm10,pm25\nd1,A,60,20,extra\n", encoding="utf-8")
        with self.assertRaises((pipeline.pd.errors.ParserError, pipeline.pd.errors.ParserWarning)):
            pipeline.load_csv(self.csv)

    def test_unrelated_warnings_remain_visible(self):
        self.write_rows([["d1", "A", 60, 20]])
        read = pipeline.pd.read_csv

        def warn_and_read(*args, **kwargs):
            warnings.warn("측정 경고 보존", RuntimeWarning)
            return read(*args, **kwargs)

        with mock.patch.object(pipeline.pd, "read_csv", side_effect=warn_and_read):
            with warnings.catch_warnings(record=True) as observed:
                warnings.simplefilter("always")
                pipeline.load_csv(self.csv)
        self.assertTrue(any("측정 경고 보존" in str(item.message) for item in observed))

    def test_cli_keeps_one_json_line_and_failure_exit_code(self):
        self.write_rows(FIXTURE_ROWS)
        command = [sys.executable, str(Path(pipeline.__file__)), str(self.csv)]
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(result.stdout.splitlines()), 1)
        metrics = json.loads(result.stdout)
        self.assertGreater(metrics["total_latency_ms"], 0)
        self.assertEqual(metrics["memory_measurement"], "end_rss_not_peak")
        self.assertEqual(metrics["result_rows"], {"p2": 6, "p3": 3, "p4": 3, "p7": 3})
        self.write_rows([["d1", "A", "oops", 20]])
        result = subprocess.run(command, text=True, capture_output=True, check=False)
        self.assertEqual(result.returncode, 1)
        self.assertIn("error", json.loads(result.stdout))


if __name__ == "__main__":
    unittest.main()
