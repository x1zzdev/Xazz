"""xazz — Xazz Python bindings (issue C4)

Pure-Python bindings that wrap the Xazz CLI. The Rust compiler/runtime stays
the single source of truth: `xazz.check(src)` returns the **same** diagnostics
as `xazz check`, and `xazz.run(src)` returns the same execution result as
`xazz run --json`.

Why not PyO3: this build environment has no python3-dev headers and no sudo, so
a C extension cannot be compiled. A subprocess bridge over the compiled CLI
delivers the same contract (`xazz.check(src)` == CLI diagnostics) with zero
build-time C dependencies. When a Python dev environment exists, the Rust CLI
itself is still the engine — the binding is a thin adapter.

Usage:
    import xazz
    result = xazz.check("v x = load('d.csv');")
    run = xazz.run("v x = load('d.csv');")
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
from dataclasses import dataclass, field
from typing import Any, Optional

__all__ = [
    "CheckResult",
    "RunResult",
    "check",
    "run",
    "policy",
    "find_xazz",
    "set_xazz_path",
    "XazzError",
]

# ── Xazz binary discovery ────────────────────────────────────────────────────
# Search order: explicit path (set_xazz_path / XAZZ_PATH), next to this package,
# then PATH. No network, no state.

_XAZZ_PATH: Optional[str] = os.environ.get("XAZZ_PATH")


def set_xazz_path(path: str) -> None:
    """Point the binding at a specific `xazz` executable."""
    global _XAZZ_PATH
    _XAZZ_PATH = path


def find_xazz() -> str:
    """Locate the `xazz` binary."""
    if _XAZZ_PATH and os.path.isfile(_XAZZ_PATH):
        return _XAZZ_PATH
    # Next to the python package (repo checkout layout: target/debug or release).
    for base in (
        os.path.join(os.path.dirname(__file__), "..", "..", "target", "debug"),
        os.path.join(os.path.dirname(__file__), "..", "..", "target", "release"),
    ):
        cand = os.path.join(base, "xazz" + (".exe" if os.name == "nt" else ""))
        if os.path.isfile(cand):
            return cand
    # PATH fallback.
    for p in os.environ.get("PATH", "").split(os.pathsep):
        cand = os.path.join(p, "xazz" + (".exe" if os.name == "nt" else ""))
        if os.path.isfile(cand):
            return cand
    raise XazzError(
        "xazz binary not found. Build it (cargo build -p xazz -p xazz-runner) "
        "or set XAZZ_PATH / xazz.set_xazz_path(path)."
    )


class XazzError(Exception):
    """Raised when the xazz CLI cannot be invoked or returns an unexpected shape."""


def _run_cli(args: list[str], cwd: Optional[str] = None) -> dict[str, Any]:
    """Runs `xazz <args> --json` and returns the parsed JSON object."""
    xazz = find_xazz()
    try:
        proc = subprocess.run(
            [xazz, *args, "--json"],
            capture_output=True,
            text=True,
            cwd=cwd,
            timeout=120,
        )
    except FileNotFoundError as e:
        raise XazzError(f"failed to execute {xazz}: {e}") from e
    except subprocess.TimeoutExpired as e:
        raise XazzError("xazz execution timed out") from e
    out = proc.stdout.strip()
    # The CLI may interleave progress markers on stdout before the JSON; the
    # final JSON document is the last thing printed.
    try:
        return json.loads(out) if out else {}
    except json.JSONDecodeError:
        # Fall back to the last JSON-looking block.
        for line in reversed(out.splitlines()):
            line = line.strip()
            if line.startswith("{") and line.endswith("}"):
                try:
                    return json.loads(line)
                except json.JSONDecodeError:
                    continue
        raise XazzError(f"xazz returned no parseable JSON: {out[:300]!r}")


# ── Results ───────────────────────────────────────────────────────────────────

@dataclass
class CheckResult:
    """Static semantic analysis result — mirrors `xazz check --json`."""

    success: bool
    errors: list[dict[str, Any]] = field(default_factory=list)
    warnings: list[dict[str, Any]] = field(default_factory=list)
    raw: dict[str, Any] = field(default_factory=dict)

    @property
    def error_count(self) -> int:
        return len(self.errors)

    @property
    def warning_count(self) -> int:
        return len(self.warnings)


@dataclass
class RunResult:
    """Execution result — mirrors `xazz run --json`."""

    success: bool
    rows: list[dict[str, Any]] = field(default_factory=list)
    schema: list[dict[str, Any]] = field(default_factory=list)
    logs: list[str] = field(default_factory=list)
    error: Optional[str] = None
    raw: dict[str, Any] = field(default_factory=dict)


# ── Public API ────────────────────────────────────────────────────────────────

def check(source: str) -> CheckResult:
    """Runs `xazz check` on the given source and returns the diagnostics.

    The same line:col / did-you-mean output as the CLI — byte-for-byte.
    """
    with tempfile.NamedTemporaryFile("w", suffix=".xzz", delete=False) as f:
        f.write(source)
        tmp = f.name
    try:
        data = _run_cli(["check", tmp])
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass
    return CheckResult(
        success=data.get("success", False),
        errors=data.get("errors", []),
        warnings=data.get("warnings", []),
        raw=data,
    )


def run(source: str) -> RunResult:
    """Runs `xazz run` on the given source and returns the execution result."""
    with tempfile.NamedTemporaryFile("w", suffix=".xzz", delete=False) as f:
        f.write(source)
        tmp = f.name
    try:
        data = _run_cli(["run", tmp])
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass
    return RunResult(
        success=data.get("success", False),
        rows=data.get("rows", []),
        schema=data.get("schema", []),
        logs=data.get("logs", []),
        error=data.get("error"),
        raw=data,
    )


def policy(source: str) -> dict[str, Any]:
    """Runs `xazz policy` on the given source; returns the policy report dict."""
    with tempfile.NamedTemporaryFile("w", suffix=".xzz", delete=False) as f:
        f.write(source)
        tmp = f.name
    try:
        data = _run_cli(["policy", tmp])
    finally:
        try:
            os.unlink(tmp)
        except OSError:
            pass
    return data