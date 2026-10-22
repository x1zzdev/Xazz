"""Build and run exactly one hardware acceptance test, preserving evidence.

Python standard library only. CUDA/ORT runtime libraries must already be installed.
No retries, CPU fallback, driver installation or system environment changes.
"""

import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import time


MODES = {
    "cuda": ("cuda", "cuda", None),
    "onnx-cpu": ("onnx", "onnx", "CPUExecutionProvider"),
    "onnx-cuda": ("onnx-cuda", "onnx", "CUDAExecutionProvider"),
}


def records(log, prefix):
    # libtest can print "test NAME ... " on the same line as the first stdout.
    return [json.loads(line.split(prefix, 1)[1]) for line in log.splitlines()
            if prefix in line]


def validate_result(log, exit_code, backend, provider=None, rejection=None):
    """An empty test selection or an unrelated crash must never pass a gate."""
    summary = re.findall(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", log)
    if rejection:
        if exit_code != 101 or summary != [("FAILED", "0", "1", "0")]:
            raise ValueError("expected exactly one failed test and exit 101")
        if rejection not in log:
            raise ValueError("failure did not contain the expected rejection diagnostic")
        return {"expected_rejection": rejection}
    if exit_code != 0 or summary != [("ok", "1", "0", "0")]:
        raise ValueError("expected exactly one passed test, no failures/ignores, and exit 0")
    evidence = records(log, "XAZZ_GPU_ACCEPTANCE ")
    if len(evidence) != 1 or evidence[0].get("backend") != backend:
        raise ValueError("missing or mismatched backend evidence")
    result = evidence[0]
    if result.get("training_backend") != ("cpu" if backend == "onnx" else backend):
        raise ValueError("unexpected training backend")
    if not isinstance(result.get("rows"), int) or result["rows"] <= 0:
        raise ValueError("empty prediction evidence")
    delta, loss = result.get("max_abs_diff"), result.get("training_loss")
    if not isinstance(delta, (int, float)) or not math.isfinite(delta) or not 0 <= delta < 1e-3:
        raise ValueError("non-finite or excessive prediction difference")
    if not isinstance(loss, (int, float)) or not math.isfinite(loss):
        raise ValueError("non-finite training loss")
    if provider:
        profiles = records(log, "XAZZ_ORT_PROVIDERS ")
        if len(profiles) != 1 or profiles[0].get("expected") != provider:
            raise ValueError("missing explicit ORT provider evidence")
        nodes = profiles[0].get("nodes", {})
        if not isinstance(nodes.get(provider), int) or nodes[provider] <= 0:
            raise ValueError("requested ORT provider executed no nodes")
        result["ort_provider_nodes"] = nodes
    return result


def digest(path):
    with open(path, "rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def capture(command, cwd, env=None):
    return subprocess.run(command, cwd=cwd, env=env, capture_output=True,
                          text=True, encoding="utf-8", errors="replace", check=True).stdout.strip()


def run_logged(command, path, cwd, env, timeout):
    print("Running:", " ".join(str(arg) for arg in command), flush=True)
    start = time.monotonic()
    with path.open("w", encoding="utf-8") as log:
        proc = subprocess.run(command, cwd=cwd, env=env, stdout=log,
                              stderr=subprocess.STDOUT, timeout=timeout, check=False)
    return {"command": [str(arg) for arg in command], "exit_code": proc.returncode,
            "elapsed_seconds": round(time.monotonic() - start, 3)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("--output", type=Path, required=True, help="new evidence directory")
    parser.add_argument("--target-dir", type=Path)
    parser.add_argument("--toolchain", help="Rust toolchain; Windows ONNX defaults to MSVC")
    parser.add_argument("--jobs", type=int, default=4)
    parser.add_argument("--reject-device", choices=["cuda:999"])
    parser.add_argument("--reject-uncompiled-cuda", action="store_true")
    args = parser.parse_args()
    if args.reject_device and args.mode != "onnx-cuda":
        parser.error("--reject-device requires onnx-cuda")
    if args.reject_uncompiled_cuda and args.mode != "onnx-cpu":
        parser.error("--reject-uncompiled-cuda requires onnx-cpu")
    root = Path(__file__).resolve().parents[1]
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    feature, backend, provider = MODES[args.mode]
    env = os.environ.copy()
    # Never inherit an operator's device/backend selection into the test.
    for key in ("XAZZ_DEVICE", "XAZZ_BACKEND", "XAZZ_CUDA_DEVICE", "XAZZ_ORT_EP",
                "XAZZ_ORT_DEVICE", "XAZZ_TEST_EXPECT_ORT_PROVIDER", "XAZZ_TEST_ORT_PROFILE_PREFIX"):
        env.pop(key, None)
    env["XAZZ_LANG"] = "en"
    env["CARGO_TERM_COLOR"] = "never"
    toolchain = args.toolchain
    if not toolchain and os.name == "nt" and backend == "onnx":
        toolchain = "stable-x86_64-pc-windows-msvc"
    cargo = ["cargo"] + ([f"+{toolchain}"] if toolchain else [])
    target_dir = args.target_dir
    if target_dir:
        env["CARGO_TARGET_DIR"] = str(target_dir.resolve())
    if args.mode == "onnx-cuda":
        env.setdefault("ORT_CUDA_VERSION", "13")
    if backend == "onnx":
        env["XAZZ_ORT_EP"] = "cpu" if args.mode == "onnx-cpu" else "cuda"
        env["XAZZ_TEST_EXPECT_ORT_PROVIDER"] = provider
        env["XAZZ_TEST_ORT_PROFILE_PREFIX"] = str(output / "ort-profile")
    else:
        env["XAZZ_DEVICE"] = "cuda:0"
    rejection = None
    if args.reject_device:
        env["XAZZ_DEVICE"] = args.reject_device
        rejection = "Invalid device ID: 999"
    if args.reject_uncompiled_cuda:
        env["XAZZ_ORT_EP"] = "cuda"
        rejection = "ONNX execution provider is not compiled into this binary"
    report = {
        "mode": args.mode, "status": "incomplete", "source_head": capture(["git", "rev-parse", "HEAD"], root),
        "source_diff_sha256": hashlib.sha256(capture(["git", "diff", "HEAD"], root).encode()).hexdigest(),
        "feature": feature, "toolchain": toolchain or "default",
        "environment": {k: env.get(k) for k in ("CUDA_PATH", "CUDARC_CUDA_VERSION", "ORT_CUDA_VERSION",
                        "XAZZ_DEVICE", "XAZZ_ORT_EP", "XAZZ_TEST_EXPECT_ORT_PROVIDER")},
        "source_files": {name: digest(root / name) for name in (
            "Cargo.lock", "xazz-exec/Cargo.toml", "xazz-exec/src/backend.rs",
            "xazz-exec/src/dl/onnx_export.rs", "scripts/verify_gpu_acceptance.py")},
    }
    try:
        report["rustc"] = capture(["rustc"] + ([f"+{toolchain}"] if toolchain else []) + ["-vV"], root)
        if shutil.which("nvidia-smi"):
            report["gpu"] = capture(["nvidia-smi", "--query-gpu=name,driver_version", "--format=csv,noheader"], root)
        build = cargo + ["test", "--release", "--locked", "-p", "xazz-exec", "--lib", "--features", feature,
                         "--no-run", "--message-format=json", "-j", str(args.jobs)]
        report["build"] = run_logged(build, output / "build.log", root, env, 7200)
        if report["build"]["exit_code"]:
            raise ValueError("build failed; see build.log")
        artifacts = []
        for line in (output / "build.log").read_text(encoding="utf-8").splitlines():
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue
            if (event.get("reason") == "compiler-artifact" and event.get("executable")
                    and event.get("target", {}).get("name") == "xazz_exec"
                    and event.get("profile", {}).get("test")):
                artifacts.append(Path(event["executable"]))
        if len(artifacts) != 1:
            raise ValueError(f"expected one freshly resolved library test binary, got {len(artifacts)}")
        binary = artifacts[0]
        report["binary"] = {"path": str(binary), "sha256": digest(binary)}
        # ort-sys may copy provider DLLs only to release/, not release/deps/.
        # Use this build's output; do not search arbitrary older ORT installations.
        if os.name == "nt" and args.mode == "onnx-cuda":
            report["provider_dlls"] = []
            for name in ("onnxruntime_providers_shared.dll", "onnxruntime_providers_cuda.dll"):
                src, dst = binary.parent.parent / name, binary.parent / name
                if not src.is_file():
                    raise ValueError(f"missing provider DLL from current build: {src}")
                if not dst.exists() or digest(src) != digest(dst):
                    shutil.copy2(src, dst)
                report["provider_dlls"].append({"name": name, "sha256": digest(dst)})
        test = f"backend::acceptance::{backend}_matches_cpu_losses"
        run_dir = output / "run"
        run_dir.mkdir()
        report["test"] = run_logged([str(binary), test, "--exact", "--ignored", "--nocapture", "--test-threads=1"],
                                    output / "test.log", run_dir, env, 600)
        log = (output / "test.log").read_text(encoding="utf-8")
        report["evidence"] = validate_result(log, report["test"]["exit_code"], backend, provider, rejection)
        report["status"] = "expected-rejection" if rejection else "passed"
    except (OSError, ValueError, subprocess.SubprocessError) as exc:
        report["status"] = "failed"
        report["error"] = str(exc)
    finally:
        report["files"] = {str(path.relative_to(output)): digest(path)
                           for path in output.glob("*") if path.is_file()}
        (output / "result.json").write_text(json.dumps(report, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2, ensure_ascii=False))
    return 1 if report["status"] == "failed" else 0


if __name__ == "__main__":
    sys.exit(main())
