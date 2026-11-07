#!/usr/bin/env python3
"""examples/field_cases/llm_finetune/generate_corpus.py — 합성 파인튜닝 코퍼스 생성기

LLM 파인튜닝 데이터 위생 현장 사례(`xazz sanitize`)에 쓰는 CSV 를 만든다.
**실제 고객 대화·개인정보는 한 건도 포함되지 않는다** — 모든 값은 고정 시드에서
결정적으로 합성된다.

이 코퍼스는 LoRA/QLoRA 파인튜닝 직전의 "지저분한" 학습 데이터를 일부러 재현한다.
`sanitize` 가 세 가지를 잡아내는지 보여주기 위한 것이다:

  · PII      — 응답 본문에 이메일·전화번호가 섞여 있다.
  · 중복      — 동일/유사 응답이 반복된다(정규화 후 근접 중복).
  · 편향      — `intent` 라벨이 refund 에 과도하게 쏠려 있다.

사용법:
    python3 examples/field_cases/llm_finetune/generate_corpus.py
    python3 examples/field_cases/llm_finetune/generate_corpus.py --rows 600 --seed 7
"""

from __future__ import annotations

import argparse
import csv
import pathlib
import random

# intent 분포 — refund 에 쏠리게 설계해 bias 신호를 만든다.
# (dominant share ≈ 60%, max/min ≈ 15x → sanitize 가 flag)
INTENT_WEIGHTS = {
    "refund": 60,
    "shipping": 18,
    "account": 12,
    "payment": 6,
    "other": 4,
}

TEMPLATES = {
    "refund": [
        "환불은 접수 후 3영업일 내 처리됩니다. 추가 문의는 {contact} 로 주세요.",
        "환불 요청이 정상 접수되었습니다. 자세한 안내는 {contact} 입니다.",
        "환불 처리 상태는 {contact} 에서 확인하실 수 있습니다.",
    ],
    "shipping": [
        "배송 조회는 주문번호로 가능합니다. 문의는 {contact} 로 부탁드립니다.",
        "배송은 보통 2~3일 소요됩니다. 지연 시 {contact} 로 연락 주세요.",
    ],
    "account": [
        "비밀번호 재설정은 계정 설정에서 가능합니다. 도움은 {contact} 입니다.",
        "계정 잠금 해제는 본인 확인 후 진행됩니다. 문의: {contact}",
    ],
    "payment": [
        "결제 실패 시 카드사에 문의해 주세요. 지원: {contact}",
        "청구 내역은 마이페이지에서 확인 가능합니다. 문의: {contact}",
    ],
    "other": [
        "문의 주신 내용은 담당 부서로 전달했습니다. 회신은 {contact} 로 드립니다.",
    ],
}

# 합성 연락처 — 실제로 할당되지 않는 대역만 쓴다.
SYNTH_CONTACTS = [
    "010-0000-{n:04d}",
    "support-{n:03d}@example.com",
    "help_{n:03d}@example.com",
]

HEADER = ["intent", "instruction", "response"]


def pick_intent(rng: random.Random) -> str:
    total = sum(INTENT_WEIGHTS.values())
    r = rng.uniform(0, total)
    acc = 0.0
    for intent, weight in INTENT_WEIGHTS.items():
        acc += weight
        if r <= acc:
            return intent
    return "other"


def build_rows(rows: int, rng: random.Random) -> list[list[str]]:
    out: list[list[str]] = []
    for i in range(rows):
        intent = pick_intent(rng)
        template = rng.choice(TEMPLATES[intent])
        contact = rng.choice(SYNTH_CONTACTS).format(n=rng.randint(1, 200))
        response = template.format(contact=contact)
        instruction = {
            "refund": "환불은 언제 처리되나요?",
            "shipping": "배송 조회는 어떻게 하나요?",
            "account": "비밀번호를 잊어버렸어요.",
            "payment": "결제가 실패했어요.",
            "other": "문의드립니다.",
        }[intent]
        out.append([intent, instruction, response])

    # 근접 중복을 심는다: 일부 행을 정규화 후 동일해지도록 복제/변형한다.
    for _ in range(max(1, rows // 5)):
        src = rng.choice(out)
        out.append([src[0], f"  {src[1]}  ", f"{src[2]} "])
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description="Xazz LLM 파인튜닝 사례용 합성 코퍼스 생성")
    parser.add_argument("--rows", type=int, default=600, help="생성할 기본 행 수 (기본 600)")
    parser.add_argument("--seed", type=int, default=42, help="난수 시드 (기본 42)")
    parser.add_argument(
        "--out",
        type=pathlib.Path,
        default=pathlib.Path(__file__).parent / "data" / "train.csv",
        help="출력 CSV 경로",
    )
    args = parser.parse_args()

    if args.rows <= 0:
        parser.error("--rows 는 1 이상이어야 합니다.")

    rng = random.Random(args.seed)
    args.out.parent.mkdir(parents=True, exist_ok=True)
    rows = build_rows(args.rows, rng)

    with args.out.open("w", newline="", encoding="utf-8") as fp:
        writer = csv.writer(fp)
        writer.writerow(HEADER)
        writer.writerows(rows)

    print(f"합성 파인튜닝 코퍼스 {len(rows)}행을 생성했습니다: {args.out}")
    print("이 데이터는 전부 합성이며 실제 고객 대화·개인정보를 포함하지 않습니다.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
