#!/usr/bin/env python3
"""examples/field_cases/finance_fds/generate_txns.py — 합성 카드 거래 데이터 생성기

금융 FDS(이상거래탐지) 현장 사례(`fds_blocked.xzz` / `fds_safe.xzz`)에 쓰는
CSV 를 만든다. **실제 고객·거래 데이터는 한 건도 포함되지 않는다** — 모든 값은
고정 시드에서 결정적으로 합성된다.

이 데이터는 "실무에서 쓰이는 형태의 스키마"를 재현하기 위한 것이지, 실제 금융기관의
데이터가 아니다. 사례 문서(`docs/design/field-validation.md`)의 Tier 1(실데이터 재현)
과 Tier 2(PoC 제안) 구분을 참고한다.

사용법:
    python3 examples/field_cases/finance_fds/generate_txns.py
    python3 examples/field_cases/finance_fds/generate_txns.py --rows 20000 --seed 7
"""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import pathlib
import random

# 가맹점 업종 — 이상거래율이 업종별로 다르게 나타나도록 설계한다.
CATEGORIES = ["grocery", "electronics", "travel", "fuel", "dining", "online", "atm"]
# 업종별 기준 이상거래율 (atm/online 이 높다).
FRAUD_RATE = {
    "grocery": 0.002,
    "electronics": 0.010,
    "travel": 0.018,
    "fuel": 0.003,
    "dining": 0.002,
    "online": 0.025,
    "atm": 0.030,
}
GENDERS = ["M", "F"]

HEADER = [
    "txn_id",
    "customer_id",
    "card_no",
    "age",
    "age_band",
    "gender",
    "zip_code",
    "merchant_category",
    "transaction_amount",
    "is_fraud",
    "txn_date",
]


def build_rows(rows: int, rng: random.Random) -> list[list[object]]:
    out: list[list[object]] = []
    base = dt.date(2026, 1, 1)
    for i in range(rows):
        category = rng.choice(CATEGORIES)
        amount = round(max(1.0, rng.gauss(60_000, 40_000)), 2)
        is_fraud = 1 if rng.random() < FRAUD_RATE[category] else 0
        age = rng.randint(19, 79)
        out.append(
            [
                f"T{i + 1:08d}",
                f"C{rng.randint(1, rows // 3 + 1):07d}",
                # 9410- 대역은 실제로 할당되지 않는 합성 카드번호다.
                f"9410-0000-0000-{rng.randint(0, 9999):04d}",
                age,
                f"{(age // 10) * 10}s",
                rng.choice(GENDERS),
                f"{rng.randint(1, 63):02d}{rng.randint(0, 999):03d}",
                category,
                amount,
                is_fraud,
                (base + dt.timedelta(days=rng.randint(0, 180))).isoformat(),
            ]
        )
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz 금융 FDS 사례용 합성 거래 데이터 생성")
    parser.add_argument("--rows", type=int, default=20000, help="생성할 행 수 (기본 20000)")
    parser.add_argument("--seed", type=int, default=42, help="난수 시드 (기본 42)")
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).parent / "data" / "transactions.csv",
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

    print(f"합성 거래 데이터 {args.rows}행을 생성했습니다: {args.out}")
    print("이 데이터는 전부 합성이며 실제 고객·거래 정보를 포함하지 않습니다.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
