#!/usr/bin/env python3
"""Xazz 라이선스 사전 검증 (오픈소스 개발자대회 제출 전 self-check).

FossID 기반 공식 검증(auditoss.kr)과 동일한 목적을 로컬에서 값싸게 재현한다.
AI 호출 없이 결정적으로 동작하며, 다음을 확인한다.

  1. cargo-deny 로 Rust 의존성 라이선스/금지/출처/보안 검사
  2. Rust 의존성 라이선스 전수 목록 + copyleft(GPL/AGPL/SSPL 등) 탐지
  3. npm(visual-ide, vscode-xazz) 의존성 라이선스 전수 목록 + copyleft 탐지
  4. 제출 압축본 위생 검사 (node_modules/target/book 등 벤더 산출물 포함 여부)
  5. 추적 소스에서 GPL/AGPL 문자열 및 vendored 번들(min.js/fonts) 탐지

사용법:  python3 scripts/license_audit.py [--root .]
종료코드: copyleft 또는 cargo-deny 실패 시 1, 그 외 0.
"""

from __future__ import annotations

import argparse
import collections
import json
import os
import subprocess
import sys

COPYLEFT_TOKENS = ("GPL", "AGPL", "SSPL", "EUPL", "CPAL", "OSL", "CDDL")
COPYLEFT_EXEMPT = ("LGPL",)  # LGPL은 OR 선택지가 있으면 permissive 선택 가능
# 제출 압축본에 포함되면 안 되는 벤더/빌드 산출물 디렉터리
VENDOR_DIRS = (
    "node_modules",
    "target",
    "book",
    ".venv-bench",
    "dist",
    "out",
    "__pycache__",
)
VENDOR_FILE_SUFFIXES = (".min.js", ".min.css", ".woff", ".woff2", ".ttf", ".eot")


def run(cmd: list[str], root: str) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, cwd=root, capture_output=True, text=True)


def check_cargo_deny(root: str) -> bool:
    print("=" * 70)
    print("[1] cargo deny check (licenses / bans / advisories / sources)")
    print("=" * 70)
    from check_dependency_exceptions import validate
    from datetime import datetime, timezone
    from pathlib import Path

    validate(Path(root), datetime.now(timezone.utc).date())
    proc = run(["cargo", "deny", "--workspace", "--all-features", "--locked", "check"], root)
    tail = (proc.stdout + proc.stderr).strip().splitlines()
    for line in tail[-6:]:
        print("   " + line)
    ok = proc.returncode == 0
    print(f"   -> {'PASS' if ok else 'FAIL'}\n")
    return ok


def is_copyleft(license_expr: str) -> bool:
    up = license_expr.upper()
    if any(e in up for e in COPYLEFT_EXEMPT):
        return False
    return any(t in up for t in COPYLEFT_TOKENS)


def rust_inventory(root: str) -> list[tuple[str, str]]:
    print("=" * 70)
    print("[2] Rust 의존성 라이선스 목록 (crates.io)")
    print("=" * 70)
    proc = run(["cargo", "metadata", "--all-features", "--locked", "--format-version", "1"], root)
    if proc.returncode != 0:
        print("   cargo metadata 실패:", proc.stderr.strip()[:200])
        return []
    meta = json.loads(proc.stdout)
    counter: collections.Counter = collections.Counter()
    flags: list[tuple[str, str]] = []
    for pkg in meta["packages"]:
        if pkg.get("source") is None:  # 워크스페이스 자체 크레이트 제외
            continue
        lic = pkg.get("license") or pkg.get("license_file") or "UNKNOWN"
        counter[lic] += 1
        if is_copyleft(lic):
            flags.append((pkg["name"], lic))
    print(f"   외부 크레이트 {sum(counter.values())}개")
    for lic, count in counter.most_common():
        print(f"   {count:4d}  {lic}")
    if flags:
        print("   !! COPYLEFT:", flags)
    else:
        print("   -> copyleft 없음\n")
    return flags


def npm_inventory(root: str) -> list[tuple[str, str]]:
    print("=" * 70)
    print("[3] npm 의존성 라이선스 목록 (package-lock.json)")
    print("=" * 70)
    flags: list[tuple[str, str]] = []
    locks = []
    for dirpath, dirnames, filenames in os.walk(root):
        dirnames[:] = [d for d in dirnames if d not in (".git", "node_modules", "target")]
        if "package-lock.json" in filenames:
            locks.append(os.path.join(dirpath, "package-lock.json"))
    if not locks:
        print("   package-lock.json 없음\n")
        return flags
    for lock in sorted(locks):
        rel = os.path.relpath(lock, root)
        data = json.load(open(lock, encoding="utf-8"))
        counter: collections.Counter = collections.Counter()
        missing: list[str] = []
        for path, meta in data.get("packages", {}).items():
            if not path:
                continue
            name = meta.get("name") or path.split("node_modules/")[-1]
            lic = meta.get("license")
            if isinstance(lic, dict):
                lic = " ".join(lic.get("type", "") or "")
            if not lic:
                missing.append(name)
                continue
            counter[lic] += 1
            if is_copyleft(lic):
                flags.append((name, lic))
        print(f"   {rel}: {sum(counter.values())}개 패키지")
        for lic, count in counter.most_common():
            print(f"      {count:4d}  {lic}")
        if missing:
            print(f"      MISSING LICENSE: {missing}")
    print("   -> copyleft:", flags or "없음", "\n")
    return flags


def packaging_hygiene(root: str) -> None:
    print("=" * 70)
    print("[4] 제출 압축본 위생 (벤더/빌드 산출물 포함 여부)")
    print("=" * 70)
    found_dirs: list[str] = []
    for dirpath, dirnames, filenames in os.walk(root):
        if ".git" in dirnames:
            dirnames.remove(".git")
        for d in list(dirnames):
            if d in VENDOR_DIRS:
                found_dirs.append(os.path.relpath(os.path.join(dirpath, d), root))
                dirnames.remove(d)
        for f in filenames:
            if f.lower().endswith(VENDOR_FILE_SUFFIXES):
                print(f"   vendored 파일: {os.path.relpath(os.path.join(dirpath, f), root)}")
    if found_dirs:
        print("   !! 아래 디렉터리를 압축에서 제외하세요 (벤더 산출물, false-positive 원인):")
        for d in sorted(set(found_dirs)):
            print(f"      - {d}")
    else:
        print("   -> 제외 대상 없음 (깨끗)")
    print()


def source_scan(root: str) -> list[str]:
    print("=" * 70)
    print("[5] 추적 소스 GPL/AGPL 문자열 탐지 (git grep)")
    print("=" * 70)
    proc = run(
        ["git", "grep", "-ilE", r"GNU (General|Affero) Public|GPL-3|AGPL", "--", "."],
        root,
    )
    hits = [h for h in proc.stdout.split() if h]
    if hits:
        for h in hits:
            print(f"   {h}")
    else:
        print("   -> copyleft 문자열 없음")
    print()
    return hits


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", default=".")
    args = ap.parse_args()
    root = os.path.abspath(args.root)

    deny_ok = check_cargo_deny(root)
    rust_flags = rust_inventory(root)
    npm_flags = npm_inventory(root)
    packaging_hygiene(root)
    gpl_hits = source_scan(root)

    print("=" * 70)
    print("요약")
    print("=" * 70)
    print(f"   cargo-deny:        {'PASS' if deny_ok else 'FAIL'}")
    print(f"   Rust copyleft:     {rust_flags or 'none'}")
    print(f"   npm copyleft:      {npm_flags or 'none'}")
    print(f"   소스 GPL 문자열:   {len(gpl_hits)}건")
    bad = (not deny_ok) or rust_flags or npm_flags
    print(f"   => {'이슈 있음 — 조치 필요' if bad else '사전 검증 통과'}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
