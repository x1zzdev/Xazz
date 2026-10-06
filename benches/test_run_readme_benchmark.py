"""작은 입력과 Python 자식 프로세스로 측정기의 실패 계약을 검증한다."""

import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import psutil

SPEC = importlib.util.spec_from_file_location("readme_benchmark", Path(__file__).with_name("run_readme_benchmark.py"))
benchmark = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark)


def collection_markers(requested, engines=None):
    """P2/P3/P4/P7 수집에서 출력하는 진단 마커 네 개를 구성한다."""
    engines = engines or ["in-memory" if requested == "off" else "streaming"] * 4
    return [{"requested": requested, "engine": engine, "status": "ok"} for engine in engines]


def xazz_stdout(collections, timing=True):
    lines = ["[xazz:collect] " + json.dumps(record) for record in collections]
    if timing:
        lines.append('[xazz:timing] {"pipeline_ms": 2.5}')
    return "\n".join(lines)


class ArgumentTests(unittest.TestCase):
    def test_default_and_quick_remain_compatible(self):
        self.assertEqual(benchmark.parse_args([]).scales, ["small", "medium", "large"])
        self.assertEqual(benchmark.parse_args(["--quick"]).scales, ["small"])
        self.assertEqual(benchmark.parse_args(["--quick", "--xlarge"]).scales, ["small", "xlarge"])
        options = benchmark.parse_args(["--scale", "xlarge", "--scale", "small", "--scale", "small", "--streaming", "both"])
        self.assertEqual(options.scales, ["xlarge", "small"])
        self.assertEqual(options.streaming_modes, ["on", "off"])

    def test_invalid_or_conflicting_arguments_fail(self):
        for args in [["--quick", "--scale", "small"], ["--sc", "small"], ["--scale", "missing"], ["--runs", "0"],
                     ["--runs", "1.5"], ["--out"], ["--unknown"], ["--timeout-seconds", "nan"],
                     ["--timeout-seconds", "0"], ["--max-memory-mb", "inf"]]:
            with self.subTest(args=args), contextlib.redirect_stderr(io.StringIO()):
                with self.assertRaises(SystemExit) as error:
                    benchmark.parse_args(args)
                self.assertEqual(error.exception.code, 2)


class MetricTests(unittest.TestCase):
    def test_timing_requires_one_finite_positive_numeric_marker(self):
        self.assertEqual(benchmark.parse_xazz_timing('[xazz:timing] {"pipeline_ms": 1.25}'), 1.25)
        invalid = ["", "no timing", '[xazz:timing] {}', '[xazz:timing] []', '[xazz:timing] invalid']
        invalid += ['[xazz:timing] ' + json.dumps({"pipeline_ms": value})
                    for value in [None, "1", True, 0, -1, float("nan"), float("inf")]]
        invalid += ['[xazz:timing] {"pipeline_ms": 1}\n[xazz:timing] {"pipeline_ms": 2}',
                    '[xazz:timing] {"pipeline_ms": 0, "pipeline_ms": 1}']
        for output in invalid:
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                benchmark.parse_xazz_timing(output)

    def test_collect_diagnostics_match_each_requested_mode(self):
        for mode, engines in [
            ("on", ["streaming"] * 4),
            ("off", ["in-memory"] * 4),
            ("auto", ["streaming", "in-memory", "streaming", "in-memory"]),
        ]:
            with self.subTest(mode=mode):
                expected = collection_markers(mode, engines)
                self.assertEqual(benchmark.parse_xazz_collections(xazz_stdout(expected), mode), expected)

    def test_collect_diagnostics_require_exactly_four_markers(self):
        for count in [0, 1, 3, 5]:
            with self.subTest(count=count), self.assertRaises(RuntimeError):
                benchmark.parse_xazz_collections(xazz_stdout(collection_markers("on")[:1] * count), "on")

    def test_collect_diagnostics_reject_malformed_json_and_duplicate_keys(self):
        valid_prefix = xazz_stdout(collection_markers("on")[:3], timing=False) + "\n[xazz:collect] "
        invalid = ["invalid", "[]", "null", "1", '"string"', "{}",
                   '{"requested":"on","engine":"streaming","status":"ok","status":"ok"}',
                   '{"requested":"off","requested":"on","engine":"streaming","status":"ok"}']
        for payload in invalid:
            with self.subTest(payload=payload), self.assertRaises(RuntimeError):
                benchmark.parse_xazz_collections(valid_prefix + payload, "on")

    def test_collect_diagnostics_reject_missing_wrong_or_failed_fields(self):
        for field, values in {
            "requested": [None, True, [], "auto", "off"],
            "engine": [None, True, [], "in-memory", "unknown"],
            "status": [None, True, [], "failed", "error"],
        }.items():
            for value in values:
                with self.subTest(field=field, value=value), self.assertRaises(RuntimeError):
                    markers = collection_markers("on")
                    markers[-1][field] = value
                    benchmark.parse_xazz_collections(xazz_stdout(markers), "on")
            with self.subTest(missing=field), self.assertRaises(RuntimeError):
                markers = collection_markers("on")
                del markers[-1][field]
                benchmark.parse_xazz_collections(xazz_stdout(markers), "on")
        with self.assertRaises(RuntimeError):
            benchmark.parse_xazz_collections(xazz_stdout(collection_markers("off", ["streaming"] * 4)), "off")
        with self.assertRaises(RuntimeError):
            benchmark.parse_xazz_collections(xazz_stdout(collection_markers("auto", ["unknown"] * 4)), "auto")

    def test_summary_rounding_matches_regression_gate_and_rejects_zero(self):
        result = benchmark.summarize([{"latency_ms": value, "peak_mb": 1.25}
                                      for value in [1.21, 1.29, 2.0]])
        self.assertEqual(result["latency_ms"], 1.3)
        with self.assertRaisesRegex(RuntimeError, "해상도"):
            benchmark.summarize([{"latency_ms": 0.01, "peak_mb": 1.25}])

    def test_warmup_timing_is_validated_and_script_is_removed(self):
        with tempfile.TemporaryDirectory() as folder:
            csv = Path(folder) / "scale_small.csv"
            csv.write_text("x\n1\n", encoding="utf-8")
            with patch.object(benchmark, "measure_tree", return_value=(999, 10, "no timing")) as measure:
                with self.assertRaisesRegex(RuntimeError, "마커"):
                    benchmark.bench_xazz(csv, xazz_bin=Path(sys.executable), runs=1)
                self.assertEqual(measure.call_count, 1)
            self.assertEqual(list(Path(folder).glob("_bench_*.xzz")), [])

    def test_warmup_collect_diagnostics_are_required_before_measured_run(self):
        with tempfile.TemporaryDirectory() as folder:
            csv = Path(folder) / "scale_small.csv"
            csv.write_text("x\n1\n", encoding="utf-8")
            invalid_outputs = [
                '[xazz:timing] {"pipeline_ms": 2.5}',
                xazz_stdout(collection_markers("on", ["in-memory"] * 4)),
            ]
            for stdout in invalid_outputs:
                with self.subTest(stdout=stdout), patch.object(
                    benchmark, "measure_tree", return_value=(999, 10, stdout)
                ) as measure:
                    with self.assertRaises(RuntimeError):
                        benchmark.bench_xazz(csv, xazz_bin=Path(sys.executable), streaming="on", runs=1)
                    self.assertEqual(measure.call_count, 1)
                self.assertEqual(list(Path(folder).glob("_bench_*.xzz")), [])

    def test_measured_collect_failure_does_not_return_a_success_summary(self):
        with tempfile.TemporaryDirectory() as folder:
            csv = Path(folder) / "scale_small.csv"
            csv.write_text("x\n1\n", encoding="utf-8")
            outputs = [(999, 10, xazz_stdout(collection_markers("on"))),
                       (999, 10, '[xazz:timing] {"pipeline_ms": 2.5}')]
            with patch.object(benchmark, "measure_tree", side_effect=outputs) as measure:
                with self.assertRaises(RuntimeError):
                    benchmark.bench_xazz(csv, xazz_bin=Path(sys.executable), streaming="on", runs=1)
                self.assertEqual(measure.call_count, 2)
            self.assertEqual(list(Path(folder).glob("_bench_*.xzz")), [])

    def test_each_streaming_mode_pins_cpu_and_environment(self):
        with tempfile.TemporaryDirectory() as folder:
            csv = Path(folder) / "scale_small.csv"
            csv.write_text("x\n1\n", encoding="utf-8")
            for mode, requested in [("auto", None), ("on", "1"), ("off", "0")]:
                markers = collection_markers(mode)
                with patch.object(benchmark, "measure_tree", return_value=(999, 10, xazz_stdout(markers))) as measure:
                    result = benchmark.bench_xazz(csv, xazz_bin=Path(sys.executable), streaming=mode,
                                                  runs=1, timeout_seconds=5.5)
                    self.assertEqual(result["latency_ms"], 2.5)
                    self.assertFalse(result["runs"][0]["fallback_to_wallclock"])
                    self.assertEqual(result["runs"][0]["collections"], markers)
                    self.assertTrue(result["collection_diagnostics_verified"])
                    self.assertEqual(result["warmup_collections"], markers)
                    self.assertEqual(measure.call_count, 2)
                    for call in measure.call_args_list:
                        environment = call.kwargs["env"]
                        self.assertEqual(environment["XAZZ_BACKEND"], "cpu")
                        self.assertEqual(environment["XAZZ_STREAMING"], requested)
                        self.assertEqual(environment["XAZZ_COLLECT_DIAGNOSTICS"], "1")
                        suffix = ".exe" if os.name == "nt" else ""
                        self.assertEqual(environment["XAZZ_RUNNER_PATH"], str(Path(sys.executable).with_name("xazz-runner" + suffix)))
                        self.assertEqual(environment["XAZZ_EXEC_PATH"], str(Path(sys.executable).with_name("xazz-exec" + suffix)))
                        self.assertEqual(environment["XAZZ_EXEC_TIMEOUT_SECS"], "7")
                        self.assertIn("[xazz RUNTIME ERROR]", call.kwargs["failure_markers"])


class ProcessMeasurementTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)

    def tearDown(self):
        self.temporary.cleanup()

    def measure(self, source, **kwargs):
        return benchmark.measure_tree([sys.executable, "-c", source], self.directory,
                                      capture=True, log_dir=self.directory, **kwargs)

    def test_stdout_and_stderr_larger_than_pipe_capacity_do_not_deadlock(self):
        wall, peak, stdout = self.measure(
            'import sys,time; sys.stdout.write("o"*1048576); sys.stdout.flush(); '
            'sys.stderr.write("e"*1048576); sys.stderr.flush(); time.sleep(0.05)', timeout_seconds=5)
        self.assertEqual(len(stdout), 1048576)
        self.assertEqual((self.directory / "run.stderr.log").stat().st_size, 1048576)
        self.assertGreater(peak, 0)
        self.assertLess(wall, 5000)

    def test_nonzero_exit_preserves_raw_error_and_failed_measurement(self):
        with self.assertRaisesRegex(RuntimeError, "exit=7"):
            self.measure('import sys,time; time.sleep(0.05); print("failure-evidence",file=sys.stderr); sys.exit(7)')
        record = json.loads((self.directory / "run.measurement.json").read_text(encoding="utf-8"))
        self.assertEqual(record["status"], "failed")
        self.assertIn("failure-evidence", (self.directory / "run.stderr.log").read_text(encoding="utf-8"))

    def test_zero_exit_with_runtime_error_is_not_a_success(self):
        with self.assertRaisesRegex(RuntimeError, "실행 오류"):
            self.measure('import json,sys,time; time.sleep(0.05); '
                         'print("[xazz:timing] "+json.dumps({"pipeline_ms":1})); '
                         'print("[xazz RUNTIME ERROR] failed",file=sys.stderr)',
                         failure_markers=("[xazz RUNTIME ERROR]",))
        benchmark.parse_xazz_timing((self.directory / "run.stdout.log").read_text(encoding="utf-8"))
        self.assertEqual(json.loads((self.directory / "run.measurement.json").read_text(encoding="utf-8"))["status"], "failed")

    def test_timeout_terminates_descendants_and_preserves_evidence(self):
        pidfile = self.directory / "child.pid"
        source = (
            'import subprocess,sys,time,pathlib; '
            'child=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"]); '
            f'pathlib.Path({str(pidfile)!r}).write_text(str(child.pid)); time.sleep(30)'
        )
        with self.assertRaisesRegex(RuntimeError, "제한시간"):
            self.measure(source, timeout_seconds=0.5)
        self.assertTrue(pidfile.exists(), "자식 생성 전 제한시간에 도달함")
        pid = int(pidfile.read_text(encoding="utf-8"))
        try:
            child = psutil.Process(pid)
            self.assertEqual(child.status(), psutil.STATUS_ZOMBIE, "자식이 종료되지 않음")
        except psutil.NoSuchProcess:
            pass
        record = json.loads((self.directory / "run.measurement.json").read_text(encoding="utf-8"))
        self.assertEqual(record["status"], "failed")
        self.assertGreater(record["wall_ms"], 0)

    def test_parent_success_does_not_leave_a_running_child(self):
        pidfile = self.directory / "orphan.pid"
        source = (
            'import subprocess,sys,time,pathlib; '
            'child=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"]); '
            f'pathlib.Path({str(pidfile)!r}).write_text(str(child.pid)); time.sleep(0.1)'
        )
        with self.assertRaisesRegex(RuntimeError, "자식 프로세스"):
            self.measure(source)
        try:
            self.assertEqual(psutil.Process(int(pidfile.read_text(encoding="utf-8"))).status(), psutil.STATUS_ZOMBIE)
        except psutil.NoSuchProcess:
            pass

    @unittest.skipUnless(sys.platform != "win32", "POSIX 프로세스 그룹 검사")
    def test_unobserved_child_in_process_group_is_not_success(self):
        pidfile = self.directory / "late-child.pid"
        source = (
            'import subprocess,sys,time,pathlib,os;time.sleep(0.08); '
            'child=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"]); '
            f'pathlib.Path({str(pidfile)!r}).write_text(str(child.pid));os._exit(0)'
        )
        with patch.object(psutil.Process, "children", return_value=[]):
            with self.assertRaisesRegex(RuntimeError, "프로세스 그룹"):
                self.measure(source)
        try:
            self.assertEqual(psutil.Process(int(pidfile.read_text(encoding="utf-8"))).status(), psutil.STATUS_ZOMBIE)
        except psutil.NoSuchProcess:
            pass

    def test_timeout_kills_observed_child_with_separate_session(self):
        pidfile = self.directory / "detached-child.pid"
        source = (
            'import subprocess,sys,time,pathlib; '
            'child=subprocess.Popen([sys.executable,"-c","import time;time.sleep(30)"],start_new_session=True); '
            f'pathlib.Path({str(pidfile)!r}).write_text(str(child.pid));time.sleep(30)'
        )
        with self.assertRaisesRegex(RuntimeError, "제한시간"):
            self.measure(source, timeout_seconds=0.5)
        try:
            self.assertEqual(psutil.Process(int(pidfile.read_text(encoding="utf-8"))).status(), psutil.STATUS_ZOMBIE)
        except psutil.NoSuchProcess:
            pass

    def test_memory_limit_is_a_failure(self):
        with self.assertRaisesRegex(RuntimeError, "RSS 제한"):
            self.measure('import time; data=bytearray(64*1024*1024); time.sleep(30)',
                         timeout_seconds=5, max_memory_mb=32)
        record = json.loads((self.directory / "run.measurement.json").read_text(encoding="utf-8"))
        self.assertEqual(record["status"], "failed")
        self.assertGreater(record["peak_mb"], 32)

    def test_environment_removal_is_not_inherited(self):
        with patch.dict("os.environ", {"XAZZ_STREAMING": "1"}):
            _, _, stdout = self.measure(
                'import os,time; time.sleep(0.05); print(os.environ.get("XAZZ_STREAMING","absent"))',
                env={"XAZZ_STREAMING": None})
        self.assertEqual(stdout.strip(), "absent")


class OrchestrationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.directory = Path(self.temporary.name)
        (self.directory / "scale_small.csv").write_text("date,station,pm10,pm25\n2026-01-01,A,50,20\n", encoding="utf-8")
        self.output = self.directory / "results.json"
        suffix = ".exe" if os.name == "nt" else ""
        self.binary = self.directory / ("xazz" + suffix)
        self.engine = self.directory / ("xazz-exec" + suffix)
        for name in ["xazz", "xazz-runner", "xazz-exec"]:
            path = self.directory / (name + suffix)
            path.write_bytes(b"placeholder: these mocked orchestration tests do not execute this file")
            path.chmod(0o755)
        self.arguments = ["--quick", "--data-dir", str(self.directory), "--xazz-bin", str(self.binary),
                          "--out", str(self.output), "--runs", "1"]
        self.metric = {"latency_ms": 2.0, "peak_mb": 10.0, "runs": [{"latency_ms": 2.0, "peak_mb": 10.0}]}

    def tearDown(self):
        self.temporary.cleanup()

    def call(self, arguments):
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return benchmark.main(arguments)

    def test_requested_missing_xlarge_fails_before_any_measurement(self):
        with patch.object(benchmark, "bench_pandas") as pandas:
            self.assertEqual(self.call(self.arguments + ["--xlarge"]), 1)
            pandas.assert_not_called()
        self.assertFalse(self.output.exists())
        failure = next(self.directory.glob("results-logs-*/failure.json"))
        self.assertIn("scale_xlarge.csv", json.loads(failure.read_text(encoding="utf-8"))["error"])

    def test_missing_engine_fails_before_measurement(self):
        self.engine.unlink()
        with patch.object(benchmark, "bench_pandas") as pandas:
            self.assertEqual(self.call(self.arguments), 1)
            pandas.assert_not_called()
        self.assertFalse(self.output.exists())

    def test_nonexecutable_engine_fails_before_measurement(self):
        # Windows chmod는 실행 권한을 제거하지 못하므로 접근 검사 결과를 명시한다.
        # main은 실행 경로를 resolve한다. 임시 폴더가 junction인 환경도 같은 파일로 비교한다.
        engine = self.engine.resolve()
        if os.name == "nt":
            access = patch.object(benchmark.os, "access", side_effect=lambda path, mode: path != engine)
        else:
            self.engine.chmod(0o644)
            access = contextlib.nullcontext()
        with access as mocked_access, patch.object(benchmark, "bench_pandas") as pandas:
            self.assertEqual(self.call(self.arguments), 1)
            pandas.assert_not_called()
            if mocked_access is not None:
                mocked_access.assert_any_call(engine, os.X_OK)
        self.assertFalse(self.output.exists())

    def test_polars_engine_overrides_fail_without_measurement_or_environment_changes(self):
        previous = '{"previous": "complete"}'
        self.output.write_text(previous, encoding="utf-8")
        overrides = {"POLARS_AUTO_NEW_STREAMING": "0", "POLARS_FORCE_NEW_STREAMING": "0"}
        for name in overrides:
            with self.subTest(variable=name):
                existing_failures = set(self.directory.glob("results-logs-*/failure.json"))
                with patch.dict(benchmark.os.environ, {**overrides, name: "1"}), \
                     patch.object(benchmark, "bench_pandas") as pandas, \
                     patch.object(benchmark, "bench_xazz") as xazz:
                    environment = dict(benchmark.os.environ)
                    self.assertEqual(self.call(self.arguments + ["--streaming", "both"]), 1)
                    pandas.assert_not_called()
                    xazz.assert_not_called()
                    self.assertEqual(dict(benchmark.os.environ), environment)
                self.assertEqual(self.output.read_text(encoding="utf-8"), previous)
                failures = set(self.directory.glob("results-logs-*/failure.json")) - existing_failures
                self.assertEqual(len(failures), 1)
                failure = json.loads(failures.pop().read_text(encoding="utf-8"))
                self.assertIn(name, failure["error"])
                self.assertEqual(failure["status"], "failed")
                self.assertEqual(failure["completed_scales"], [])
                self.assertFalse(failure["success_output_updated"])

    def test_input_metadata_counts_csv_records_with_embedded_newlines(self):
        path = self.directory / "multiline.csv"
        path.write_text('date,station,pm10,pm25\n2026-01-01,"line1\nline2",50,20\n', encoding="utf-8")
        metadata = benchmark._input_metadata(path)
        self.assertEqual(metadata["rows"], 1)
        self.assertEqual(metadata["bytes"], path.stat().st_size)
        self.assertEqual(len(metadata["sha256"]), 64)

    def test_preflight_rejects_short_extra_broken_and_reordered_csv(self):
        contents = [
            "date,station,pm10,pm25\nd1,A,60\n",
            "date,station,pm10,pm25\nd1,A,60,20,extra\n",
            'date,station,pm10,pm25\nd1,"unfinished,60,20\n',
            "date,station,pm25,pm10\nd1,A,20,60\n",
        ]
        for content in contents:
            with self.subTest(content=content):
                (self.directory / "scale_small.csv").write_text(content, encoding="utf-8")
                with patch.object(benchmark, "bench_pandas") as pandas:
                    self.assertEqual(self.call(self.arguments), 1)
                    pandas.assert_not_called()
                self.assertFalse(self.output.exists())

    def test_default_results_keep_baseline_shape(self):
        with patch.object(benchmark, "bench_pandas", return_value=self.metric), \
             patch.object(benchmark, "bench_xazz", return_value=self.metric), \
             patch.object(benchmark, "collect_provenance", return_value={"recorded": True}):
            self.assertEqual(self.call(self.arguments), 0)
        result = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertEqual(set(result["small"]), {"rows", "pandas", "xazz"})

    def test_both_modes_are_stored_separately_without_xazz_alias(self):
        with patch.object(benchmark, "bench_pandas", return_value=self.metric), \
             patch.object(benchmark, "bench_xazz", return_value=self.metric) as xazz, \
             patch.object(benchmark, "collect_provenance", return_value={"recorded": True}):
            self.assertEqual(self.call(self.arguments + ["--streaming", "both"]), 0)
            self.assertEqual([call.kwargs["streaming"] for call in xazz.call_args_list], ["on", "off"])
        result = json.loads(self.output.read_text(encoding="utf-8"))
        self.assertEqual(set(result["small"]), {"rows", "pandas", "xazz_streaming_on", "xazz_streaming_off"})

    def test_partial_failure_never_overwrites_success_output(self):
        self.output.write_text('{"previous": "complete"}', encoding="utf-8")
        with patch.object(benchmark, "bench_pandas", return_value=self.metric), \
             patch.object(benchmark, "bench_xazz", side_effect=[self.metric, RuntimeError("off failed")]):
            self.assertEqual(self.call(self.arguments + ["--streaming", "both"]), 1)
        self.assertEqual(json.loads(self.output.read_text(encoding="utf-8")), {"previous": "complete"})
        failure = json.loads(next(self.directory.glob("results-logs-*/failure.json")).read_text(encoding="utf-8"))
        self.assertFalse(failure["success_output_updated"])
        self.assertEqual(failure["status"], "failed")

    def test_binary_provenance_has_hash_and_never_substitutes_cargo_version(self):
        with patch.object(benchmark, "_cmd_stdout", return_value=None), \
             patch.object(benchmark, "_pkg_version", return_value=None):
            provenance = benchmark.collect_provenance(Path(sys.executable), streaming_modes=["on", "off"])
        self.assertIsNone(provenance["xazz"])
        self.assertFalse(provenance["xazz_version_verified"])
        self.assertFalse(provenance["streaming_actual_engine_verified"])
        self.assertEqual(provenance["streaming_requested_modes"], ["on", "off"])
        self.assertEqual(len(provenance["binary_sha256"][Path(sys.executable).name]), 64)
        self.assertEqual(len(provenance["pandas_pipeline_sha256"]), 64)
        self.assertEqual(len(provenance["runtime_source_sha256"]), 64)
        self.assertIsNone(provenance["git_worktree_status"])


if __name__ == "__main__":
    unittest.main()
