#!/usr/bin/env python3
"""examples/security/generate_patients.py — 합성 환자 데이터 생성기 (issue #2)

보안 가드레일 데모(`patient_unsafe.xzz` / `patient_safe.xzz`)에 쓰는
CSV 를 만든다. **실제 개인정보는 한 건도 포함되지 않는다** — 모든 값은
고정 시드에서 결정적으로 합성된다.

CSV 를 저장소에 커밋하지 않고 생성기만 두는 이유:
  · 이 저장소는 `*.csv` 를 Git LFS 로 추적한다(.gitattributes). 데모용
    소용량 CSV 까지 LFS 로 밀어 넣으면 클론 비용만 늘어난다.
  · 합성 개인정보를 닮은 데이터는 저장소에 남기지 않는 편이 낫다.

사용법:
    python3 examples/security/generate_patients.py
    python3 examples/security/generate_patients.py --rows 5000 --seed 7
"""

from __future__ import annotations

import argparse
import csv
import pathlib
import random

SURNAMES = ["김", "이", "박", "최", "정", "강", "조", "윤", "장", "임"]
GIVEN = ["민준", "서연", "도윤", "지우", "예준", "하윤", "시우", "지민", "주원", "수아"]
DISEASES = [
    "hypertension",
    "diabetes",
    "asthma",
    "influenza",
    "gastritis",
    "migraine",
    "rare_metabolic_disorder",
]
GENDERS = ["M", "F"]

HEADER = [
    "patient_id",
    "name",
    "phone",
    "age",
    "gender",
    "zip_code",
    "disease",
    "age_band",
    "visit_count",
]


def age_band(age: int) -> str:
    """나이를 10년 단위 구간으로 일반화한다 — 준식별자 완화의 표준 기법."""
    return f"{(age // 10) * 10}s"


def build_rows(rows: int, rng: random.Random) -> list[list[object]]:
    out: list[list[object]] = []
    for i in range(rows):
        age = rng.randint(1, 95)
        out.append(
            [
                f"P{i + 1:06d}",
                rng.choice(SURNAMES) + rng.choice(GIVEN),
                # 010-0000-0000 대역은 실제로 할당되지 않는 합성 번호다.
                f"010-0000-{rng.randint(0, 9999):04d}",
                age,
                rng.choice(GENDERS),
                f"{rng.randint(1, 63):02d}{rng.randint(0, 999):03d}",
                rng.choice(DISEASES),
                age_band(age),
                rng.randint(1, 30),
            ]
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz 보안 데모용 합성 환자 데이터 생성")
    parser.add_argument("--rows", type=int, default=2000, help="생성할 행 수 (기본 2000)")
    parser.add_argument("--seed", type=int, default=42, help="난수 시드 (기본 42)")
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).parent / "data" / "patients.csv",
        help="출력 CSV 경로",
    )
    args = parser.parse_args()

    if args.rows <= 0:
        parser.error("--rows 는 1 이상이어야 합니다.")

    rng = random.Random(args.seed)
    args.out.parent.mkdir(parents=True, exist_ok=True)

    with args.out.open("w", newline="", encoding="utf-8") as fp:
        writer = csv.writer(fp)
        writer.writerow(HEADER)
        writer.writerows(build_rows(args.rows, rng))

    print(f"합성 환자 데이터 {args.rows}행을 생성했습니다: {args.out}")
    print("이 데이터는 전부 합성이며 실제 개인정보를 포함하지 않습니다.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
