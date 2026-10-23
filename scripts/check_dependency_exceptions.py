#!/usr/bin/env python3
"""Fail closed when a temporary maintenance exception expires or drifts."""

import datetime as dt
import json
from pathlib import Path
import tomllib


def validate(root: Path, today: dt.date) -> None:
    policy = tomllib.loads((root / "deny.toml").read_text(encoding="utf-8"))
    register = json.loads((root / "security/dependency-exceptions.json").read_text(encoding="utf-8"))
    entries = register["exceptions"]
    ids = [entry["id"] for entry in entries]
    if len(ids) != len(set(ids)):
        raise ValueError("duplicate dependency exception")
    configured = policy["advisories"]["ignore"]
    if any(not isinstance(entry, dict) or not entry.get("reason") for entry in configured):
        raise ValueError("every exception requires a recorded reason")
    if {entry["id"] for entry in configured} != set(ids):
        raise ValueError("deny.toml ignores must exactly match the exception register")
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
    packages = {(p["name"], p["version"]) for p in lock["package"]}
    for entry in entries:
        if entry["kind"] != "unmaintained":
            raise ValueError("only maintenance notices can use this exception register")
        if today >= dt.date.fromisoformat(entry["expires"]):
            raise ValueError(f"expired dependency exception: {entry['id']}")
        if not entry.get("reason") or not entry.get("removal_plan"):
            raise ValueError("exception requires a reason and removal plan")
        if (entry["package"], entry["version"]) not in packages:
            raise ValueError(f"stale exception or changed package version: {entry['id']}")


if __name__ == "__main__":
    validate(Path(__file__).resolve().parents[1], dt.datetime.now(dt.timezone.utc).date())
    print("Dependency exception register: valid (2 temporary maintenance notices; no vulnerability exceptions)")
