"""벤치마크 게이트의 입력 계약과 독립 비율 경계 검증."""
from __future__ import annotations

import contextlib
import copy
import io
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import bench_regression as gate


BASELINE = Path(__file__).with_name("bench_regression_baseline.json")
SCRIPT = Path(__file__).with_name("bench_regression.py")
DEFAULT_BASELINE = object()


def measurement(pandas_ms: float = 200.0, xazz_ms: float = 100.0) -> dict:
    return {
        "small": {
            "rows": 100,
            "pandas": {
                "latency_ms": pandas_ms,
                "runs": [{"latency_ms": pandas_ms} for _ in range(3)],
            },
            "xazz": {
                "latency_ms": xazz_ms,
                "runs": [
                    {"latency_ms": xazz_ms, "fallback_to_wallclock": False}
                    for _ in range(3)
                ],
            },
        }
    }


class ComparisonTests(unittest.TestCase):
    def compare(self, current, baseline=DEFAULT_BASELINE, tol=0.35):
        output, error = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(error):
            code = gate.compare(current, measurement() if baseline is DEFAULT_BASELINE else baseline, tol)
        return code, output.getvalue(), error.getvalue()

    def assert_invalid(self, current, baseline=DEFAULT_BASELINE, tol=0.35):
        code, output, error = self.compare(current, baseline, tol)
        self.assertEqual(code, 1, (output, error))
        self.assertNotIn("회귀 없음", output)
        self.assertIn("입력 오류", error)
        return error

    def test_committed_baseline_accepts_itself_with_optional_provenance(self):
        baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
        current = copy.deepcopy(baseline)
        current["provenance"] = {"system": "Darwin", "processor": None}
        self.assertEqual(self.compare(current, baseline)[0], 0)

    def test_independent_integer_ratio_grid(self):
        # 허용비율 25%: cur_p / cur_x >= (base_p / base_x) * 3/4.
        # 부동소수점 나눗셈을 재사용하지 않고 정수 교차곱으로 판정한다.
        for base_p in range(1, 8):
            for base_x in range(1, 8):
                for cur_p in range(1, 8):
                    for cur_x in range(1, 8):
                        expected = 0 if 4 * cur_p * base_x >= 3 * base_p * cur_x else 1
                        actual, output, error = self.compare(
                            measurement(cur_p, cur_x), measurement(base_p, base_x), 0.25
                        )
                        self.assertEqual(actual, expected, (base_p, base_x, cur_p, cur_x, output, error))

    def test_exact_threshold_is_accepted_and_below_threshold_fails(self):
        self.assertEqual(self.compare(measurement(130, 100))[0], 0)
        code, output, error = self.compare(measurement(129.9, 100))
        self.assertEqual(code, 1)
        self.assertIn("REGRESSION", output)
        self.assertIn("회귀가 감지", error)

    def test_invalid_top_level_and_metadata(self):
        for value in (None, [], "small", 7, {}, {"provenance": {}}):
            with self.subTest(value=value):
                self.assert_invalid(value)
                self.assert_invalid(measurement(), value)
        current = measurement()
        current["provenance"] = []
        self.assert_invalid(current)

    def test_scales_must_match_in_both_directions(self):
        extended = measurement()
        extended["medium"] = copy.deepcopy(extended["small"])
        for current, baseline in ((measurement(), extended), (extended, measurement())):
            self.assert_invalid(current, baseline)
        self.assert_invalid({"typo": measurement()["small"]})

    def test_invalid_entry_and_engine(self):
        for value in (None, [], "invalid", False):
            current = measurement()
            current["small"] = value
            self.assert_invalid(current)
            for engine in ("pandas", "xazz"):
                current = measurement()
                current["small"][engine] = value
                self.assert_invalid(current)
        current = measurement()
        del current["small"]["pandas"]
        self.assert_invalid(current)

    def test_latency_must_be_finite_positive_json_number_everywhere(self):
        bad = (0, -1, float("nan"), float("inf"), float("-inf"), "1", None, True, [], 10**400)
        for value in bad:
            for engine in ("pandas", "xazz"):
                for run in (False, True):
                    with self.subTest(value=str(value), engine=engine, run=run):
                        current = measurement()
                        target = current["small"][engine]
                        if run:
                            target = target["runs"][1]
                        target["latency_ms"] = value
                        self.assert_invalid(current)
                        self.assert_invalid(measurement(), current)
        current = measurement()
        del current["small"]["pandas"]["latency_ms"]
        self.assert_invalid(current)

    def test_tolerance_must_be_finite_in_range(self):
        for tol in (-0.1, 1, 1.1, float("nan"), float("inf"), "0.35", True, None):
            with self.subTest(tol=tol):
                self.assert_invalid(measurement(), tol=tol)
        self.assertEqual(self.compare(measurement(), tol=0)[0], 0)
        self.assertEqual(self.compare(measurement(), tol=0.999)[0], 0)

    def test_rows_must_be_positive_integer_and_equal(self):
        for value in (None, 0, -1, 1.5, "100", True):
            current = measurement()
            current["small"]["rows"] = value
            self.assert_invalid(current)
        current = measurement()
        del current["small"]["rows"]
        self.assert_invalid(current)
        current = measurement()
        current["small"]["rows"] += 1
        self.assert_invalid(current)

    def test_three_samples_required_and_summary_must_match_median(self):
        for runs in (None, [], [None] * 3, [{}] * 3, [{"latency_ms": 100}] * 2):
            current = measurement()
            current["small"]["pandas"]["runs"] = runs
            self.assert_invalid(current)
        current = measurement()
        del current["small"]["pandas"]["runs"]
        self.assert_invalid(current)
        current = measurement()
        current["small"]["pandas"]["latency_ms"] = 201
        self.assert_invalid(current)
        current = measurement()
        current["small"]["pandas"]["runs"] = [
            {"latency_ms": n} for n in (199, 201, 200)
        ]
        self.assertEqual(self.compare(current)[0], 0)

    def test_wallclock_fallback_and_unknown_measurement_mode_fail(self):
        for fallback in (True, None, 0, "false"):
            current = measurement()
            current["small"]["xazz"]["runs"][0]["fallback_to_wallclock"] = fallback
            self.assert_invalid(current)
            self.assert_invalid(measurement(), current)
        current = measurement()
        del current["small"]["xazz"]["runs"][0]["fallback_to_wallclock"]
        self.assert_invalid(current)

    def test_ratio_overflow_or_underflow_is_invalid(self):
        for pandas_ms, xazz_ms in ((1e308, 1e-308), (1e-308, 1e308)):
            current = measurement(pandas_ms, xazz_ms)
            self.assert_invalid(current)

    def test_valid_scale_does_not_hide_invalid_other_scale(self):
        current = measurement()
        baseline = measurement()
        current["medium"] = None
        baseline["medium"] = copy.deepcopy(baseline["small"])
        self.assert_invalid(current, baseline)


class CliTests(unittest.TestCase):
    def run_cli(self, current, baseline=None, tol="0.35"):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            current_path, baseline_path = root / "current.json", root / "baseline.json"
            current_path.write_text(current, encoding="utf-8")
            baseline_path.write_text(
                json.dumps(measurement()) if baseline is None else baseline, encoding="utf-8"
            )
            return subprocess.run(
                [sys.executable, str(SCRIPT), "--current", str(current_path),
                 "--baseline", str(baseline_path), f"--tol={tol}"],
                text=True, encoding="utf-8", capture_output=True, check=False,
                env={**os.environ, "PYTHONIOENCODING": "utf-8"},
            )

    def test_actual_cli_success_and_regression_exit_codes(self):
        self.assertEqual(self.run_cli(json.dumps(measurement())).returncode, 0)
        result = self.run_cli(json.dumps(measurement(100, 100)))
        self.assertEqual(result.returncode, 1)
        self.assertIn("회귀가 감지", result.stderr)

    def test_invalid_json_and_duplicate_keys_fail_with_file_diagnostic(self):
        for raw in ('{', '[]', '{"small": {}, "small": {}}', '{"small": NaN}'):
            for baseline in (False, True):
                with self.subTest(raw=raw, baseline=baseline):
                    result = self.run_cli(json.dumps(measurement()) if baseline else raw,
                                          raw if baseline else None)
                    self.assertEqual(result.returncode, 1, result)
                    self.assertIn("입력 오류", result.stderr)
                    self.assertNotIn("Traceback", result.stderr)
                    self.assertNotIn("회귀 없음", result.stdout)

    def test_invalid_measurement_cli_reports_field_path(self):
        for field, value in (("latency_ms", 0), ("latency_ms", "100"), ("runs", [])):
            current = measurement()
            current["small"]["xazz"][field] = value
            result = self.run_cli(json.dumps(current))
            self.assertEqual(result.returncode, 1)
            self.assertIn(f"current.small.xazz.{field}", result.stderr)
            self.assertNotIn("회귀 없음", result.stdout)
        current = measurement()
        current["small"]["xazz"]["runs"][0]["fallback_to_wallclock"] = True
        result = self.run_cli(json.dumps(current))
        self.assertEqual(result.returncode, 1)
        self.assertIn("fallback_to_wallclock", result.stderr)

    def test_invalid_tolerance_fails_without_traceback(self):
        for tol in ("nan", "inf", "-0.1", "1"):
            result = self.run_cli(json.dumps(measurement()), tol=tol)
            self.assertEqual(result.returncode, 1, result)
            self.assertIn("tol", result.stderr)
            self.assertNotIn("회귀 없음", result.stdout)

    def test_missing_file_reports_clean_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            result = subprocess.run(
                [sys.executable, str(SCRIPT), "--current", str(Path(directory) / "missing.json"),
                 "--baseline", str(BASELINE)], text=True, encoding="utf-8", capture_output=True, check=False,
                env={**os.environ, "PYTHONIOENCODING": "utf-8"},
            )
        self.assertEqual(result.returncode, 1)
        self.assertIn("입력 오류", result.stderr)
        self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
