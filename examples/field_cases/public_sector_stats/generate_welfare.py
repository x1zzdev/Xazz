#!/usr/bin/env python3
"""examples/field_cases/public_sector_stats/generate_welfare.py — 합성 복지 수급 데이터 생성기

공공데이터 개방(가명정보 통계) 현장 사례(`welfare_blocked.xzz` / `welfare_safe.xzz`)에
쓰는 CSV 를 만든다. **실제 주민·수급자 데이터는 한 건도 포함되지 않는다** — 모든 값은
고정 시드에서 결정적으로 합성된다.

"시군구별 복지 수급 통계를 개방하라"는 현장 요청을 재현하기 위한 것이지, 실제 행정
데이터가 아니다. 사례 문서(`docs/design/field-validation.md`)의 Tier 1/Tier 2 구분을 참고한다.

사용법:
    python3 examples/field_cases/public_sector_stats/generate_welfare.py
    python3 examples/field_cases/public_sector_stats/generate_welfare.py --rows 30000 --seed 7
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import pathlib
import random

SIDO = ["서울", "부산", "대구", "인천", "광주", "대전", "경기", "강원"]
BENEFITS = ["기초생활", "차상위", "한부모", "장애수당", "노인연금"]
GENDERS = ["M", "F"]
SURNAMES = ["김", "이", "박", "최", "정", "강", "조", "윤", "장", "임"]
GIVEN = ["민준", "서연", "도윤", "지우", "예준", "하윤", "시우", "지민", "주원", "수아"]

HEADER = [
    "resident_id",
    "name",
    "sido",
    "dong",
    "age",
    "age_band",
    "gender",
    "household_size",
    "welfare_benefit",
    "benefit_amount",
    "disability_grade",
    "application_date",
]


def build_rows(rows: int, rng: random.Random) -> list[list[object]]:
    out: list[list[object]] = []
    base = dt.date(2026, 1, 1)
    for i in range(rows):
        sido = rng.choice(SIDO)
        age = rng.randint(0, 95)
        out.append(
            [
                f"R{i + 1:08d}",
                rng.choice(SURNAMES) + rng.choice(GIVEN),
                sido,
                f"{sido}-{rng.randint(1, 25):02d}동",
                age,
                f"{(age // 10) * 10}s",
                rng.choice(GENDERS),
                rng.randint(1, 7),
                rng.choice(BENEFITS),
                round(rng.choice([200_000, 350_000, 500_000, 700_000]) * rng.uniform(0.8, 1.2), 0),
                rng.choice([0, 0, 0, 1, 2, 3]),
                (base + dt.timedelta(days=rng.randint(0, 180))).isoformat(),
            ]
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz 공공데이터 사례용 합성 복지 데이터 생성")
    parser.add_argument("--rows", type=int, default=30000, help="생성할 행 수 (기본 30000)")
    parser.add_argument("--seed", type=int, default=42, help="난수 시드 (기본 42)")
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).parent / "data" / "welfare.csv",
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

    print(f"합성 복지 데이터 {args.rows}행을 생성했습니다: {args.out}")
    print("이 데이터는 전부 합성이며 실제 주민·수급자 정보를 포함하지 않습니다.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
