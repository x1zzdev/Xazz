import json
import unittest

from verify_gpu_acceptance import validate_result


class AcceptanceEvidenceTests(unittest.TestCase):
    def success(self, backend="cuda", **changes):
        evidence = {"backend": backend, "training_backend": "cpu" if backend == "onnx" else backend,
                    "rows": 6, "max_abs_diff": 1e-5, "training_loss": 0.5}
        evidence.update(changes)
        return "XAZZ_GPU_ACCEPTANCE " + json.dumps(evidence) + "\n" + (
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 100 filtered out;\n")

    def test_real_success(self):
        self.assertEqual(validate_result(self.success(), 0, "cuda")["rows"], 6)

    def test_libtest_prefix_on_marker_line(self):
        self.assertEqual(validate_result("test acceptance ... " + self.success(), 0, "cuda")["rows"], 6)

    def test_zero_tests_and_ignored_test_are_failures(self):
        for summary in ("0 passed; 0 failed; 0 ignored", "0 passed; 0 failed; 1 ignored"):
            with self.subTest(summary=summary), self.assertRaises(ValueError):
                validate_result(self.success().replace("1 passed; 0 failed; 0 ignored", summary), 0, "cuda")

    def test_crash_even_after_success_marker_fails(self):
        with self.assertRaises(ValueError):
            validate_result(self.success(), 101, "cuda")

    def test_cpu_fallback_is_not_cuda_success(self):
        with self.assertRaises(ValueError):
            validate_result(self.success(backend="cpu"), 0, "cuda")

    def test_invalid_numeric_evidence_fails(self):
        for changes in ({"rows": 0}, {"max_abs_diff": float("nan")}, {"max_abs_diff": float("inf")},
                        {"max_abs_diff": 0.001}, {"max_abs_diff": -0.1}, {"training_loss": float("nan")}):
            with self.subTest(changes=changes), self.assertRaises(ValueError):
                validate_result(self.success(**changes), 0, "cuda")

    def test_missing_or_duplicate_evidence_fails(self):
        for log in (self.success().split("\n", 1)[1], self.success() + self.success().split("\n")[0]):
            with self.assertRaises(ValueError):
                validate_result(log, 0, "cuda")

    def test_onnx_cuda_requires_actual_cuda_nodes(self):
        for nodes in ({}, {"CPUExecutionProvider": 3}, {"CUDAExecutionProvider": 0}):
            profile = {"expected": "CUDAExecutionProvider", "nodes": nodes}
            with self.subTest(nodes=nodes), self.assertRaises(ValueError):
                validate_result(self.success("onnx") + "XAZZ_ORT_PROVIDERS " + json.dumps(profile),
                                0, "onnx", "CUDAExecutionProvider")

    def test_onnx_profile_reports_cpu_nodes_as_well(self):
        profile = {"expected": "CUDAExecutionProvider", "nodes": {"CUDAExecutionProvider": 4, "CPUExecutionProvider": 1}}
        result = validate_result(self.success("onnx") + "XAZZ_ORT_PROVIDERS " + json.dumps(profile),
                                 0, "onnx", "CUDAExecutionProvider")
        self.assertEqual(result["ort_provider_nodes"]["CPUExecutionProvider"], 1)

    def test_rejection_must_be_the_expected_runtime_failure(self):
        log = "Invalid device ID: 999\ntest result: FAILED. 0 passed; 1 failed; 0 ignored;"
        self.assertIn("expected_rejection", validate_result(log, 101, "onnx", rejection="Invalid device ID: 999"))
        for exit_code, diagnostic in ((0, "Invalid device ID: 999"), (101, "missing DLL")):
            with self.assertRaises(ValueError):
                validate_result(log, exit_code, "onnx", rejection=diagnostic)


if __name__ == "__main__":
    unittest.main()
