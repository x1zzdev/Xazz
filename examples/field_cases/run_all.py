#!/usr/bin/env python3
"""examples/field_cases/run_all.py — 실무 사례 증거 러너

모든 실무 사례를 한 번에 재현하고, 각 단계의 기대 결과를 검증해 요약을 출력한다.
발표·리뷰 전에 "지금 저장소 상태에서 사례가 실제로 동작한다"는 증거를 만드는 용도다.

각 사례는 데이터를 다시 생성하고, 정책 게이트와 실행을 돌린 뒤, 기대한
exit code 와 일치하는지 확인한다(위반 예제는 차단되어야 하고, 안전 예제는 통과해야 한다).

사용법:
    python3 examples/field_cases/run_all.py
    python3 examples/field_cases/run_all.py --xazz target/release/xazz
"""

from __future__ import annotations

import argparse
import os
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
CASES = ROOT / "examples" / "field_cases"


def find_xazz(explicit: str | None) -> pathlib.Path:
    candidates = []
    if explicit:
        candidates.append(pathlib.Path(explicit))
    env = os.environ.get("XAZZ_BIN")
    if env:
        candidates.append(pathlib.Path(env))
    candidates += [
        ROOT / "target" / "release" / "xazz",
        ROOT / "target" / "debug" / "xazz",
    ]
    for c in candidates:
        if c.is_file():
            return c.resolve()
    # Fall back to PATH.
    from shutil import which

    found = which("xazz")
    if found:
        return pathlib.Path(found).resolve()
    sys.exit("xazz binary not found — build it or pass --xazz <path>")


def run(cmd: list[str], cwd: pathlib.Path) -> tuple[int, str]:
    result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    return result.returncode, (result.stdout + result.stderr)


# (case dir, generator, [steps]) where each step is (label, argv, expected_exit)
SCENARIOS = [
    (
        "finance_fds",
        "generate_txns.py",
        [
            ("blocked policy", ["policy", "fds_blocked.xzz"], 1),
            ("blocked run", ["run", "fds_blocked.xzz"], 1),
            ("safe policy", ["policy", "fds_safe.xzz"], 0),
            ("safe run", ["run", "fds_safe.xzz"], 0),
        ],
    ),
    (
        "public_sector_stats",
        "generate_welfare.py",
        [
            ("blocked policy", ["policy", "welfare_blocked.xzz"], 1),
            ("blocked run", ["run", "welfare_blocked.xzz"], 1),
            ("safe policy", ["policy", "welfare_safe.xzz"], 0),
            ("safe run", ["run", "welfare_safe.xzz"], 0),
        ],
    ),
]

SANITIZE_CASE = ("llm_finetune", "generate_corpus.py", "data/train.csv")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--xazz", help="path to the xazz CLI (default: target/release or PATH)")
    args = parser.parse_args()
    xazz = str(find_xazz(args.xazz))
    failures = 0

    print(f"xazz: {xazz}\n")

    for case, generator, steps in SCENARIOS:
        cwd = CASES / case
        print(f"── {case} ─────────────────────────────────────────")
        rc, out = run([sys.executable, generator], cwd)
        if rc != 0:
            print(f"  FAIL generator {generator}\n    {out.strip()}")
            failures += 1
            continue
        print(f"  ok   generate {out.strip().splitlines()[0] if out.strip() else generator}")
        for label, argv, expected in steps:
            rc, out = run([xazz, *argv], cwd)
            mark = "ok  " if rc == expected else "FAIL"
            if rc != expected:
                failures += 1
            print(f"  {mark} {label} (exit {rc}, expected {expected})")
        print()

    case, generator, target = SANITIZE_CASE
    cwd = CASES / case
    print(f"── {case} ─────────────────────────────────────────")
    rc, out = run([sys.executable, generator], cwd)
    if rc != 0:
        print(f"  FAIL generator {generator}")
        failures += 1
    else:
        rc, out = run([xazz, "sanitize", target, "--json"], cwd)
        flagged = "sanitize" in out
        mark = "ok  " if rc == 0 and flagged else "FAIL"
        if not (rc == 0 and flagged):
            failures += 1
        print(f"  {mark} sanitize flagged issues (exit {rc})")
    print()

    print(f"{'ALL OK' if failures == 0 else f'{failures} FAILURE(S)'}")
    return int(failures != 0)


if __name__ == "__main__":
    raise SystemExit(main())
