#!/usr/bin/env python3
"""Check and run every .xzz example, reporting failures by file and stage.

Examples that are *supposed* to fail (a policy block, a missing external
service) are listed in ``EXPECTED_FAILURES`` and reported as ``XFAIL``; if one
of them unexpectedly passes it becomes ``XPASS`` and fails the run so the
allowlist stays honest. Training-heavy examples are listed in ``HEAVY`` and
skipped unless ``--include-heavy`` is given.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]

# Examples that cannot pass in this harness. Reported as XFAIL; an unexpected
# pass is XPASS and fails the run.
EXPECTED_FAILURES = {
    "examples/duckdb/postgres_demo.xzz": "needs a local PostgreSQL server",
    "examples/security/patient_unsafe.xzz": "intentionally blocked by the policy guardrail (exit 1)",
    "examples/security/patient_secret_leak.xzz": "intentionally blocked by the policy guardrail (exit 1)",
    "examples/security/prompt_unsafe.xzz": "intentionally blocked by the policy guardrail (exit 1)",
}

# Training-heavy examples are skipped by default: a debug build needs many
# minutes per run. Pass --include-heavy to exercise them too.
HEAVY = {
    "examples/deep_learning/air_pipeline.xzz": "trains a 228K-row dataset (slow in debug builds)",
    "examples/deep_learning/air_quality_predictor.xzz": "trains a 228K-row dataset (slow in debug builds)",
    "examples/end_to_end/air_quality_full.xzz": "trains a 228K-row dataset (slow in debug builds)",
}


def run_command(binary: Path, stage: str, source: Path, cwd: Path, timeout: int):
    try:
        result = subprocess.run(
            [str(binary), stage, str(source), *(["--json"] if stage == "run" else [])],
            cwd=cwd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except subprocess.TimeoutExpired:
        return f"timed out after {timeout}s"
    if result.returncode == 0 and stage == "run":
        try:
            payload = json.loads(result.stdout)
        except json.JSONDecodeError:
            return "run returned non-JSON output despite --json"
        runtime_errors = [
            line for line in payload.get("logs", []) if "[xazz RUNTIME ERROR]" in line
        ]
        if (
            runtime_errors
            or not payload.get("success")
            or payload.get("error")
            or payload.get("exit_code") != 0
        ):
            return "runtime result: " + "\n    ".join(
                runtime_errors or [str(payload.get("error") or "success=false")]
            )
    if result.returncode == 0:
        return None
    output = (result.stdout + result.stderr).strip().splitlines()
    return f"exit {result.returncode}: " + "\n    ".join(output[-12:])


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--xazz",
        type=Path,
        default=Path(os.environ.get("XAZZ_BIN", ROOT / "target/debug/xazz")),
        help="CLI binary (keep xazz-runner and xazz-exec beside it)",
    )
    parser.add_argument("--timeout", type=int, default=30, help="seconds per command")
    parser.add_argument("--check-only", action="store_true")
    parser.add_argument(
        "--include-heavy",
        action="store_true",
        help="also run training-heavy examples (skipped by default)",
    )
    args = parser.parse_args()
    binary = args.xazz.resolve()
    if not binary.is_file() or args.timeout < 1:
        parser.error("--xazz must name a built CLI and --timeout must be positive")

    examples = sorted((ROOT / "examples").rglob("*.xzz"))
    if not examples:
        parser.error("no .xzz examples found")
    failures = 0
    for source in examples:
        label = str(source.relative_to(ROOT))
        expected = label in EXPECTED_FAILURES
        if label in HEAVY and not args.include_heavy:
            print(f"SKIP {label} [heavy] {HEAVY[label]}")
            continue
        # Generated charts and checkpoints stay outside the repository.
        with tempfile.TemporaryDirectory(prefix="xazz-example-") as tmp:
            cwd = Path(tmp)
            for name in ("examples", "visual-ide"):
                candidate = ROOT / name
                if candidate.exists():
                    (cwd / name).symlink_to(candidate, target_is_directory=True)
            # Project-style main.xzz files load data beside the source.
            if source.name == "main.xzz":
                for sibling in source.parent.iterdir():
                    if sibling.name == source.name or sibling.name in ("README.md", "xazz.toml"):
                        continue
                    (cwd / sibling.name).symlink_to(sibling, target_is_directory=sibling.is_dir())

            error = run_command(binary, "check", source, cwd, args.timeout)
            if error:
                if expected:
                    print(f"XFAIL {label} [check] {EXPECTED_FAILURES[label]}")
                else:
                    print(f"FAIL {label} [check]\n    {error}")
                    failures += 1
                continue
            print(f"PASS {label} [check]")
            if args.check_only:
                continue
            error = run_command(binary, "run", source, cwd, args.timeout)
            if error:
                if expected:
                    print(f"XFAIL {label} [run] {EXPECTED_FAILURES[label]}")
                else:
                    print(f"FAIL {label} [run]\n    {error}")
                    failures += 1
            elif expected:
                print(f"XPASS {label} [run] expected to fail: {EXPECTED_FAILURES[label]}")
                failures += 1
            else:
                print(f"PASS {label} [run]")

    print(f"{len(examples)} examples; {failures} failed")
    return int(failures != 0)


if __name__ == "__main__":
    raise SystemExit(main())
