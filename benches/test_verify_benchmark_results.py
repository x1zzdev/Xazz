"""독립 대조기가 고정 정답과 의도적으로 손상한 측정 근거를 구분하는지 검사한다."""

import hashlib
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

from verify_benchmark_results import _parsed_numeric, numeric, scan_csv, self_test, verify


class VerificationTests(unittest.TestCase):
    def test_known_answers_and_corrupted_evidence(self):
        report = self_test()
        self.assertEqual(report["status"], "ok")
        self.assertEqual(report["fixture_rows"], 12)
        self.assertEqual(report["verified_runs"], 6)
        self.assertEqual(report["rejected_invalid_cases"], 18)
        self.assertTrue(report["tie_cutoff_alternative_verified"])

    def test_log_access_error_is_preserved(self):
        with patch("verify_benchmark_results.Path.iterdir", side_effect=PermissionError("로그 접근 거부")):
            with self.assertRaisesRegex(PermissionError, "로그 접근 거부"):
                self_test()

    def test_explicit_log_override_preserves_original_results(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            csv_path, results_path = folder / "scale_small.csv", folder / "results.json"
            logs, recorded = folder / "restored-logs", folder / "unavailable-original-logs"
            logs.mkdir()
            csv_path.write_text("date,station,pm10,pm25\nd1,A,60,20\n", encoding="utf-8")
            raw_csv = csv_path.read_bytes()
            results = {
                "small": {"rows": 1, "pandas": {"runs": [{}]}, "xazz_streaming_on": {"runs": [{}]}},
                "provenance": {"inputs": {"small": {"file": csv_path.name, "rows": 1,
                    "bytes": len(raw_csv), "sha256": hashlib.sha256(raw_csv).hexdigest()}},
                    "raw_logs": str(recorded), "measured_runs": 1, "warmup_runs": 1,
                    "streaming_requested_modes": ["on"]},
            }
            results_path.write_text(json.dumps(results), encoding="utf-8")
            original = results_path.read_bytes()
            final = {"rows": [{"station": "A", "pm25": 1}],
                     "schema": [{"name": "station"}, {"name": "pm25"}]}
            for engine in ("pandas", "xazz-on"):
                for index in (0, 1):
                    base = logs / f"scale_small-{engine}-{index}"
                    Path(str(base) + ".measurement.json").write_text('{"status":"ok","exit_code":0}', encoding="utf-8")
                    stdout = (json.dumps({"result_rows": dict.fromkeys(("p2", "p3", "p4", "p7"), 1)})
                              if engine == "pandas" else "[xazz:result] " + json.dumps(final))
                    stderr = "" if engine == "pandas" else "\n".join(
                        f"[xazz] Pipeline #{number} '{stage}' 완료: 1 × {4 if stage == 'p2' else 2}"
                        for number, stage in enumerate(("p2", "p3", "p4", "p7"), 1))
                    Path(str(base) + ".stdout.log").write_text(stdout, encoding="utf-8")
                    Path(str(base) + ".stderr.log").write_text(stderr, encoding="utf-8")
            with self.assertRaises(FileNotFoundError):
                verify(results_path, folder)
            report = verify(results_path, folder, logs_dir=logs)
            self.assertEqual(report["status"], "ok")
            self.assertEqual(report["raw_logs"], str(logs))
            self.assertEqual(report["recorded_raw_logs"], str(recorded))
            self.assertEqual(len(report["scales"]["small"]["checked_runs"]), 4)
            self.assertEqual(results_path.read_bytes(), original)

    def test_numeric_cache_is_bounded_and_preserves_exact_input(self):
        _parsed_numeric.cache_clear()
        self.addCleanup(_parsed_numeric.cache_clear)
        for value in range(8200):
            self.assertEqual(numeric(str(value), "pm10", 2), value)
        self.assertEqual(_parsed_numeric.cache_info().maxsize, 8192)
        self.assertEqual(_parsed_numeric.cache_info().currsize, 8192)
        self.assertIsNone(numeric("", "pm25", 3))
        self.assertEqual(numeric("+.5", "pm25", 3), 0.5)
        self.assertEqual(numeric("1e2", "pm25", 3), 100)
        # 검증된 원문 '2'가 있어도 다른 문자열을 숫자로 보정하지 않는다.
        self.assertEqual(numeric("2", "pm10", 4), 2)
        for value in (" 2", "2 ", "1_0", "NaN", "inf", "1e999"):
            for column, line in (("pm10", 23), ("pm25", 91)):
                with self.subTest(value=value, column=column), self.assertRaisesRegex(
                    ValueError, f"{line}행 {column}:"
                ):
                    numeric(value, column, line)

    def test_every_later_record_keeps_width_text_and_numeric_validation(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "input.csv"
            for row, message in (
                ("d2,A,60,20,extra", ":3: 열 수"),
                (",A,60,20", ":3: date"),
                ("d2,점검중,60,20", ":3: station"),
                ("d2,A,1e999,20", "3행 pm10:"),
                ("d2,A,60,1e999", "3행 pm25:"),
            ):
                path.write_text("date,station,pm10,pm25\nd1,A,60,20\n" + row + "\n", encoding="utf-8")
                with self.subTest(row=row), self.assertRaisesRegex(ValueError, message):
                    scan_csv(path)


if __name__ == "__main__":
    unittest.main()
