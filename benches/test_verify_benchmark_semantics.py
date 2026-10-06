"""실제 실행 검사의 누락·변조 거부 계약. 실행 파일 자체는 별도 CI 단계에서 검사한다."""
import contextlib
import copy
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

from benchmark_semantics import FIXTURE_ROWS, FIXTURES, STAGES, TEXT_ROWS, TOPK_ROWS, oracle
import verify_benchmark_semantics as semantics


def output_fixture(stage="p7", mode="on", records=FIXTURE_ROWS):
    expected = oracle(records)
    count = STAGES.index(stage) + 1
    schema = [{"name": name, "type": "str" if name in ("date", "station") else "u32" if stage == "p7" else "f64"}
              for name in semantics.columns_for(stage)]
    collection = {"requested": mode, "engine": "streaming" if mode == "on" else "in-memory", "status": "ok"}
    stdout = "\n".join(["[xazz:collect] " + json.dumps(collection)] * count + [
        '[xazz:timing] {"pipeline_ms": 1}',
        "[xazz:result] " + json.dumps({"rows": expected[stage], "schema": schema})])
    stderr = "\n".join(f"[xazz] Pipeline #{index + 1} '{name}' done: {len(expected[name])} × {len(semantics.columns_for(name))}"
                       for index, name in enumerate(STAGES[:count]))
    return stdout, stderr, expected


class SemanticVerificationTests(unittest.TestCase):
    def test_shared_oracle_has_fixed_answers_and_literal_text(self):
        expected = oracle(FIXTURE_ROWS)
        self.assertEqual([len(expected[stage]) for stage in STAGES], [6, 3, 3, 3])
        self.assertEqual(expected["p3"], [{"station": "A", "pm10": 160}, {"station": "B", "pm10": 180}, {"station": "C", "pm10": 75}])
        self.assertEqual(expected["p4"], [{"station": "A", "pm10": 80}, {"station": "C", "pm10": 75}, {"station": "B", "pm10": 60}])
        self.assertEqual(expected["p7"], [{"station": "A", "pm25": 4}, {"station": "C", "pm25": 3}, {"station": "B", "pm25": 2}])
        self.assertEqual(oracle(TEXT_ROWS)["p7"], [{"station": "NA", "pm25": 1}, {"station": "NULL", "pm25": 1}])

    def test_template_prefixes_and_every_mode_stage_contract(self):
        sources = semantics.stage_sources(semantics.TEMPLATE.read_text(encoding="utf-8"))
        for index, stage in enumerate(STAGES):
            self.assertEqual(sources[stage].count("\nv p"), index + 1)
            for records in FIXTURES.values():
                for mode in ("on", "off"):
                    stdout, stderr, expected = output_fixture(stage, mode, records)
                    result = semantics.validate_output(stdout, stderr, mode, stage, expected)
                    self.assertEqual(result["rows"], expected[stage])

    def test_topk_fixture_has_distinct_cutoffs_and_fractional_answers(self):
        expected = oracle(TOPK_ROWS)
        self.assertEqual(len(TOPK_ROWS), 78)
        self.assertEqual([len(expected[stage]) for stage in STAGES], [78, 12, 10, 5])
        self.assertEqual(expected["p4"], [
            {"station": "S12", "pm10": 72.25}, {"station": "S11", "pm10": 71.25},
            {"station": "S10", "pm10": 70.25}, {"station": "S09", "pm10": 69.25},
            {"station": "S08", "pm10": 68.25}, {"station": "S07", "pm10": 67.25},
            {"station": "S06", "pm10": 66.25}, {"station": "S05", "pm10": 65.25},
            {"station": "S04", "pm10": 64.25}, {"station": "S03", "pm10": 63.25},
        ])
        self.assertEqual(expected["p7"], [
            {"station": "S12", "pm25": 12}, {"station": "S11", "pm25": 11},
            {"station": "S10", "pm25": 10}, {"station": "S09", "pm25": 9},
            {"station": "S08", "pm25": 8},
        ])
        self.assertEqual(expected["p3"][-1], {"station": "S12", "pm10": 867.0})

    def test_missing_or_wrong_take_is_rejected_for_both_cutoffs(self):
        for stage, limit, field in (("p4", 10, "pm10"), ("p7", 5, "pm25")):
            stdout, stderr, expected = output_fixture(stage, records=TOPK_ROWS)
            marker = stdout.splitlines()[-1]
            final = json.loads(marker.removeprefix("[xazz:result] "))
            all_rows = [{"station": f"S{station:02d}", field: 60.25 + station if stage == "p4" else station}
                        for station in range(12, 0, -1)]
            mutants = {"take omitted": all_rows, "limit too large": all_rows[:limit + 1],
                       "limit too small": all_rows[:limit - 1], "wrong boundary": all_rows[:limit - 1] + [all_rows[limit]]}
            for label, rows in mutants.items():
                changed = {**final, "rows": rows}
                # stderr가 올바른 행 수를 주장해도 실제 결과 셀의 절단 오류를 거부해야 한다.
                with self.subTest(stage=stage, mutant=label), self.assertRaises(AssertionError):
                    semantics.validate_output(stdout.replace(marker, "[xazz:result] " + json.dumps(changed)),
                                              stderr, "on", stage, expected)

    def test_diagnostic_missing_duplicate_invalid_and_wrong_engine_fail(self):
        stdout, stderr, expected = output_fixture()
        first = stdout.splitlines()[0]
        invalid = [stdout.replace(first + "\n", "", 1), first + "\n" + stdout,
                   stdout.replace('"engine": "streaming"', '"engine": "in-memory"', 1),
                   stdout.replace('"requested": "on"', '"requested": "off"', 1),
                   stdout.replace('"status": "ok"', '"status": "failed"', 1),
                   stdout.replace(first, '[xazz:collect] {"status":"ok","status":"ok"}', 1),
                   stdout.replace(first, '[xazz:collect] broken', 1),
                   stdout.replace('[xazz:timing] {"pipeline_ms": 1}', '[xazz:timing] {"pipeline_ms": NaN}')]
        for output in invalid:
            with self.subTest(output=output), self.assertRaises(ValueError):
                semantics.validate_output(output, stderr, "on", "p7", expected)
        with self.assertRaises(ValueError):
            semantics.validate_output(stdout, stderr + "\n[xazz RUNTIME ERROR] broken", "on", "p7", expected)
        with self.assertRaises(ValueError):
            semantics.validate_output(stdout, stderr.replace("'p2' done: 6", "'p2' done: 5"), "on", "p7", expected)

    def test_changed_cells_schemas_order_and_row_count_fail(self):
        for stage in STAGES:
            stdout, stderr, expected = output_fixture(stage)
            marker = stdout.splitlines()[-1]
            valid = json.loads(marker.removeprefix("[xazz:result] "))
            changed_cell = copy.deepcopy(valid)
            field = "pm25" if stage == "p7" else "pm10"
            changed_cell["rows"][0][field] += 1
            wrong_type = copy.deepcopy(valid)
            wrong_type["schema"][-1]["type"] = "str"
            wrong_field = copy.deepcopy(valid)
            wrong_field["rows"][0]["extra"] = 1
            missing_row = copy.deepcopy(valid)
            missing_row["rows"].pop()
            invalid = [changed_cell, wrong_type, wrong_field, missing_row]
            if stage in ("p4", "p7"):
                reversed_rows = copy.deepcopy(valid)
                reversed_rows["rows"].reverse()
                invalid.append(reversed_rows)
            for final in invalid:
                with self.subTest(stage=stage, final=final), self.assertRaises((AssertionError, ValueError)):
                    semantics.validate_output(stdout.replace(marker, "[xazz:result] " + json.dumps(final)), stderr, "on", stage, expected)

    def test_missing_executable_is_failure_with_evidence_and_no_skip(self):
        with tempfile.TemporaryDirectory() as temporary:
            folder = Path(temporary)
            output = folder / "report.json"
            with patch.object(semantics, "measure_tree") as execute, contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(semantics.main(["--xazz-exec", str(folder / "missing"), "--out", str(output)]), 1)
            execute.assert_not_called()
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["runs"], [])
            self.assertEqual(report["expected_runs"], len(FIXTURES) * 2 * len(STAGES))
            self.assertEqual(report["expected_runs"], 24)
            self.assertIn("실행 가능한", report["error"])

    def test_nonzero_exit_preserves_failed_run_and_stops(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary) / "report.json"

            def fail_measure(command, cwd, **options):
                base = options["log_dir"] / options["log_name"]
                Path(str(base) + ".stdout.log").write_text("", encoding="utf-8")
                Path(str(base) + ".stderr.log").write_text("실패 근거", encoding="utf-8")
                Path(str(base) + ".measurement.json").write_text('{"status":"failed","exit_code":7}', encoding="utf-8")
                raise RuntimeError("exit=7: 실패 근거")

            with patch.object(semantics, "measure_tree", side_effect=fail_measure) as execute, \
                 contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(semantics.main(["--xazz-exec", sys.executable, "--out", str(output)]), 1)
            self.assertEqual(execute.call_count, 1)
            report = json.loads(output.read_text(encoding="utf-8"))
            self.assertEqual(report["status"], "failed")
            self.assertEqual(report["runs"][0]["exit_code"], 7)
            self.assertEqual(report["runs"][0]["status"], "failed")
            self.assertTrue(Path(report["runs"][0]["stderr"]).exists())


if __name__ == "__main__":
    unittest.main()
