#!/usr/bin/env python3
"""experiments/slm_guardrail/build_dataset.py — 보안 보정 학습 데이터 생성기 (issue #2)

파인튜닝 데이터셋을 **가드레일 엔진 자신에게서** 만든다.

    위반 코드 합성  →  xazz policy --fix --json  →  (위반 리포트, 검증된 안전 코드)

이렇게 하는 이유:

  · 라벨이 사람의 취향이 아니라 **정책 엔진의 판정**이다. 정답이 흔들리지 않는다.
  · 출력 코드는 이미 `verified: true` 로 재검증된 것만 쓴다. 모델이 위반 코드를
    정답으로 배우는 일이 구조적으로 불가능하다.
  · 프롬프트 형식이 추론 시점(`xazz-server/src/slm.rs::build_prompt`)과 한 글자도
    다르지 않다. 학습·추론 형식이 어긋나 파인튜닝 효과가 날아가는 사고를 막는다.
  · 실제 개인정보가 단 한 건도 필요 없다. 전부 합성이다.

사용법:
    cargo build --release -p xazz
    python3 experiments/slm_guardrail/build_dataset.py \\
        --xazz target/release/xazz \\
        --out experiments/slm_guardrail/data/train.jsonl

출력(JSONL) 한 줄:
    {"prompt": "...", "completion": "...", "rule_ids": ["XZP001"], "source": "synthetic-v1"}
"""

from __future__ import annotations

import argparse
import itertools
import json
import pathlib
import subprocess
import sys
import tempfile

# ── 합성 스키마 ──────────────────────────────────────────────────────────────
# 도메인별로 컬럼 이름을 바꿔 가며 같은 규칙을 여러 표기로 학습시킨다.

SCHEMAS: list[tuple[str, str, dict[str, str]]] = [
    (
        "Patient",
        "환자",
        {
            "id": "patient_id",
            "name": "name",
            "contact": "phone",
            "sensitive": "disease",
            "quasi1": "age",
            "quasi2": "gender",
            "quasi3": "zip_code",
            "safe1": "age_band",
            "safe2": "visit_count",
        },
    ),
    (
        "Employee",
        "임직원",
        {
            "id": "employee_id",
            "name": "full_name",
            "contact": "email",
            "sensitive": "salary",
            "quasi1": "age",
            "quasi2": "gender",
            "quasi3": "nationality",
            "safe1": "department_code",
            "safe2": "tenure_years",
        },
    ),
    (
        "Member",
        "회원",
        {
            "id": "member_id",
            "name": "이름",
            "contact": "연락처",
            "sensitive": "신용점수",
            "quasi1": "나이",
            "quasi2": "성별",
            "quasi3": "우편번호",
            "safe1": "grade",
            "safe2": "order_count",
        },
    ),
]

DATA_PATHS = [
    "data/records.csv",
    "examples/security/data/patients.csv",
    "input/dataset.csv",
]


def schema_decl(type_name: str, cols: dict[str, str]) -> str:
    fields = [
        f"    {cols['id']}: string,",
        f"    {cols['name']}: string,",
        f"    {cols['contact']}: string,",
        f"    {cols['quasi1']}: int,",
        f"    {cols['quasi2']}: string,",
        f"    {cols['quasi3']}: string,",
        f"    {cols['sensitive']}: string,",
        f"    {cols['safe1']}: string,",
        f"    {cols['safe2']}: int,",
    ]
    return f"type {type_name} = {{\n" + "\n".join(fields) + "\n};\n"


# ── 위반 파이프라인 템플릿 ───────────────────────────────────────────────────
# 각 템플릿은 (이름, 파이프라인 생성 함수) 쌍이다.

TEMPLATES: list[tuple[str, object]] = [
    (
        "direct_identifier_select",
        lambda t, c, p: f'v result = load("{p}") :: {t}\n'
        f"    |> select([{c['id']}, {c['name']}, {c['safe1']}]);\n",
    ),
    (
        "direct_identifier_contact",
        lambda t, c, p: f'v contacts = load("{p}") :: {t}\n'
        f"    |> filter({c['quasi1']} > 30)\n"
        f"    |> select([{c['name']}, {c['contact']}, {c['safe2']}]);\n",
    ),
    (
        "no_projection_leaks_everything",
        lambda t, c, p: f'v raw = load("{p}") :: {t}\n'
        f'    |> dropNull("{c["safe1"]}");\n',
    ),
    (
        "sensitive_row_level",
        lambda t, c, p: f'v detail = load("{p}") :: {t}\n'
        f"    |> select([{c['quasi1']}, {c['sensitive']}]);\n",
    ),
    (
        "quasi_identifier_combination",
        lambda t, c, p: f'v cohort = load("{p}") :: {t}\n'
        f"    |> select([{c['quasi1']}, {c['quasi2']}, {c['quasi3']}, {c['safe2']}]);\n",
    ),
    (
        "aggregate_without_dp",
        lambda t, c, p: f'v stats = load("{p}") :: {t}\n'
        f'    |> groupBy("{c["sensitive"]}")\n'
        f'    |> count("{c["id"]}");\n',
    ),
    (
        "epsilon_too_large",
        lambda t, c, p: f'v noisy = load("{p}") :: {t}\n'
        f'    |> groupBy("{c["sensitive"]}")\n'
        f'    |> count("{c["id"]}")\n'
        f"    |> withDp(epsilon: 25.0, mechanism: laplace);\n",
    ),
    (
        "identifier_then_aggregate",
        lambda t, c, p: f'v mixed = load("{p}") :: {t}\n'
        f'    |> groupBy("{c["name"]}")\n'
        f'    |> mean("{c["safe2"]}");\n',
    ),
]


# ── 프롬프트 — xazz-server/src/slm.rs::build_prompt 와 반드시 동일해야 한다 ──

PROMPT_HEADER = (
    "당신은 Xazz DSL(.xzz) 보안 코드 보정기입니다.\n"
    "아래 코드는 Policy-as-Code 정적 가드레일에 차단되었습니다.\n"
    "위반을 모두 해소하되 분석 의도는 최대한 보존하는 안전한 코드로 다시 작성하세요.\n\n"
    "규칙:\n"
    "1. 직접 식별자(이름·환자번호·연락처 등)는 출력하지 않습니다.\n"
    "2. 민감 속성은 행 단위로 내보내지 말고 groupBy + 집계로 바꿉니다.\n"
    "3. 민감 속성 집계에는 |> withDp(epsilon: ..., mechanism: laplace) 를 붙입니다.\n"
    "4. 준식별자는 구간화(예: age → age_band)해 일반화합니다.\n"
    "5. 하드코딩된 개인정보·비밀키는 코드에서 제거합니다.\n"
    "6. 설명 없이 .xzz 코드만 출력합니다.\n\n"
)


def build_prompt(code: str, violations: list[dict]) -> str:
    lines = []
    for v in violations:
        lines.append(
            f"- [{v['rule_id']}] {v['rule_name']}: {v['message']}\n"
            f"  보정 방향: {v['remediation_hint']}\n"
        )
    return (
        PROMPT_HEADER
        + "=== 위반 내역 ===\n"
        + "".join(lines)
        + "\n=== 원본 코드 ===\n"
        + code
        + "\n\n=== 보정된 코드 ===\n"
    )


# ── 가드레일 호출 ────────────────────────────────────────────────────────────


def run_guardrail(xazz: str, source: str, policy: str | None) -> dict | None:
    """`xazz policy <file> --fix --json` 을 호출해 리포트+보정 결과를 받는다."""
    env = None
    if policy:
        import os

        env = dict(os.environ, XAZZ_POLICY_PATH=policy)

    with tempfile.NamedTemporaryFile("w", suffix=".xzz", delete=False, encoding="utf-8") as fp:
        fp.write(source)
        path = fp.name
    try:
        completed = subprocess.run(
            [xazz, "policy", path, "--fix", "--json"],
            capture_output=True,
            text=True,
            env=env,
            timeout=60,
        )
        # 위반이 있으면 종료 코드 1 이다 — 그것이 정상 경로다.
        if not completed.stdout.strip():
            sys.stderr.write(f"[warn] 빈 출력: {completed.stderr[:200]}\n")
            return None
        return json.loads(completed.stdout)
    except (subprocess.TimeoutExpired, json.JSONDecodeError) as e:
        sys.stderr.write(f"[warn] 가드레일 호출 실패: {e}\n")
        return None
    finally:
        pathlib.Path(path).unlink(missing_ok=True)


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz 보안 보정 sLM 학습 데이터 생성")
    parser.add_argument("--xazz", default="target/release/xazz", help="xazz 실행 파일 경로")
    parser.add_argument(
        "--policy", default=None, help="정책 JSON 경로 (미지정 시 내장 기본 정책)"
    )
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path("experiments/slm_guardrail/data/train.jsonl"),
        help="출력 JSONL 경로",
    )
    args = parser.parse_args()

    if not pathlib.Path(args.xazz).exists():
        parser.error(
            f"xazz 실행 파일이 없습니다: {args.xazz}\n"
            f"먼저 빌드하세요: cargo build --release -p xazz"
        )

    args.out.parent.mkdir(parents=True, exist_ok=True)

    kept = 0
    skipped = 0
    with args.out.open("w", encoding="utf-8") as fp:
        for (type_name, _label, cols), (name, make), path in itertools.product(
            SCHEMAS, TEMPLATES, DATA_PATHS
        ):
            source = schema_decl(type_name, cols) + "\n" + make(type_name, cols, path)
            result = run_guardrail(args.xazz, source, args.policy)
            if result is None:
                skipped += 1
                continue

            report = result.get("policy") or {}
            remediation = result.get("remediation") or {}
            violations = report.get("violations") or []

            # 학습 대상은 "위반이 있었고, 검증된 보정이 나온" 쌍뿐이다.
            if not violations or not remediation.get("verified"):
                skipped += 1
                continue

            record = {
                "prompt": build_prompt(source, violations),
                "completion": remediation["code"].rstrip() + "\n",
                "rule_ids": sorted({v["rule_id"] for v in violations}),
                "template": name,
                "schema": type_name,
                "source": "synthetic-v1",
            }
            fp.write(json.dumps(record, ensure_ascii=False) + "\n")
            kept += 1

    print(f"학습 쌍 {kept}건을 생성했습니다: {args.out}")
    if skipped:
        print(f"건너뜀 {skipped}건 (위반 없음 또는 자동 보정 불가 — 정상입니다)")
    print("모든 데이터는 합성이며 실제 개인정보를 포함하지 않습니다.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
