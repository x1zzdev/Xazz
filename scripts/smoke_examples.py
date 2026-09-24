#!/usr/bin/env python3
"""Check and run every .xzz example, reporting failures by file and stage."""

import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]


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
    args = parser.parse_args()
    binary = args.xazz.resolve()
    if not binary.is_file() or args.timeout < 1:
        parser.error("--xazz must name a built CLI and --timeout must be positive")

    examples = sorted((ROOT / "examples").rglob("*.xzz"))
    if not examples:
        parser.error("no .xzz examples found")
    failures = 0
    for source in examples:
        label = source.relative_to(ROOT)
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
                print(f"FAIL {label} [check]\n    {error}")
                failures += 1
                continue
            print(f"PASS {label} [check]")
            if args.check_only:
                continue
            error = run_command(binary, "run", source, cwd, args.timeout)
            if error:
                print(f"FAIL {label} [run]\n    {error}")
                failures += 1
            else:
                print(f"PASS {label} [run]")

    print(f"{len(examples)} examples; {failures} failed")
    return int(failures != 0)


if __name__ == "__main__":
    raise SystemExit(main())
