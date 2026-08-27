#!/usr/bin/env python3
"""experiments/slm_guardrail/train_qlora.py — Qwen2.5-Coder-1.5B QLoRA 파인튜닝 (issue #2)

Unsloth + QLoRA 로 `Qwen2.5-Coder-1.5B-Instruct` 를 Xazz 보안 코드 보정에
특화시킨다. 학습 데이터는 `build_dataset.py` 가 가드레일 엔진에서 뽑아낸
(위반 리포트 → 검증된 안전 코드) 쌍이다.

⚠️  이 스크립트는 GPU 가 있는 환경에서 실행해야 한다 (4bit QLoRA 기준 VRAM 8GB 이상).
    CI 나 CPU 전용 환경에서는 `--dry-run` 으로 데이터·설정만 검증한다.

전체 파이프라인
    1. python3 experiments/slm_guardrail/build_dataset.py            # 데이터 생성
    2. python3 experiments/slm_guardrail/train_qlora.py              # QLoRA 학습
    3. python3 experiments/slm_guardrail/train_qlora.py --export-gguf  # GGUF 변환
    4. ollama create xazz-guardrail -f experiments/slm_guardrail/Modelfile
    5. python3 experiments/slm_guardrail/evaluate.py                 # 정책 준수율 평가

의존성 (GPU 환경에서만):
    pip install "unsloth[colab-new] @ git+https://github.com/unslothai/unsloth.git"
    pip install trl peft accelerate bitsandbytes datasets
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys

BASE_MODEL = "unsloth/Qwen2.5-Coder-1.5B-Instruct"
DEFAULT_DATA = pathlib.Path("experiments/slm_guardrail/data/train.jsonl")
DEFAULT_OUT = pathlib.Path("experiments/slm_guardrail/out/xazz-guardrail-lora")

# LoRA 설정 — 1.5B 코드 모델의 보정 태스크에 맞춘 보수적인 값.
LORA_R = 16
LORA_ALPHA = 32
LORA_DROPOUT = 0.0
TARGET_MODULES = [
    "q_proj",
    "k_proj",
    "v_proj",
    "o_proj",
    "gate_proj",
    "up_proj",
    "down_proj",
]

MAX_SEQ_LEN = 2048


def load_records(path: pathlib.Path) -> list[dict]:
    """JSONL 학습 데이터를 읽고 스키마를 검증한다."""
    if not path.exists():
        raise SystemExit(
            f"학습 데이터가 없습니다: {path}\n"
            f"먼저 생성하세요: python3 experiments/slm_guardrail/build_dataset.py"
        )

    records: list[dict] = []
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = line.strip()
        if not line:
            continue
        try:
            record = json.loads(line)
        except json.JSONDecodeError as e:
            raise SystemExit(f"{path}:{lineno} JSON 파싱 실패: {e}") from e
        for key in ("prompt", "completion"):
            if key not in record or not str(record[key]).strip():
                raise SystemExit(f"{path}:{lineno} 필수 필드 '{key}' 가 비었습니다.")
        records.append(record)

    if not records:
        raise SystemExit(f"{path} 에 학습 쌍이 하나도 없습니다.")
    return records


def summarize(records: list[dict]) -> None:
    """데이터 분포를 출력한다 — 특정 규칙에만 쏠려 있으면 여기서 드러난다."""
    from collections import Counter

    rules: Counter[str] = Counter()
    for r in records:
        for rid in r.get("rule_ids", []):
            rules[rid] += 1

    print(f"학습 쌍: {len(records)}건")
    print("규칙별 분포:")
    for rule, count in sorted(rules.items()):
        print(f"  {rule}: {count}건")

    lengths = [len(r["prompt"]) + len(r["completion"]) for r in records]
    print(
        f"길이(문자): 최소 {min(lengths)} / 중앙 {sorted(lengths)[len(lengths) // 2]} "
        f"/ 최대 {max(lengths)}"
    )
    if max(lengths) > MAX_SEQ_LEN * 3:
        print(
            f"⚠️  max_seq_length={MAX_SEQ_LEN} 토큰을 넘길 수 있는 샘플이 있습니다. "
            f"잘림 여부를 확인하세요."
        )


def build_text(record: dict) -> str:
    """프롬프트와 정답을 하나의 학습 텍스트로 합친다.

    추론 시점(`xazz-server/src/slm.rs`)이 `=== 보정된 코드 ===\\n` 뒤부터
    생성하므로, 학습 텍스트도 정확히 그 경계에서 이어져야 한다.
    """
    return f"{record['prompt']}{record['completion']}"


def train(args: argparse.Namespace, records: list[dict]) -> int:
    try:
        from datasets import Dataset
        from trl import SFTConfig, SFTTrainer
        from unsloth import FastLanguageModel
    except ImportError as e:
        print(f"학습 의존성이 없습니다: {e}", file=sys.stderr)
        print(
            "GPU 환경에서 설치하세요:\n"
            '  pip install "unsloth[colab-new] @ git+https://github.com/unslothai/unsloth.git"\n'
            "  pip install trl peft accelerate bitsandbytes datasets",
            file=sys.stderr,
        )
        return 1

    model, tokenizer = FastLanguageModel.from_pretrained(
        model_name=args.base_model,
        max_seq_length=MAX_SEQ_LEN,
        dtype=None,          # 하드웨어에 맞춰 자동 선택
        load_in_4bit=True,   # QLoRA
    )

    model = FastLanguageModel.get_peft_model(
        model,
        r=LORA_R,
        lora_alpha=LORA_ALPHA,
        lora_dropout=LORA_DROPOUT,
        target_modules=TARGET_MODULES,
        bias="none",
        use_gradient_checkpointing="unsloth",
        random_state=args.seed,
    )

    dataset = Dataset.from_list([{"text": build_text(r)} for r in records])

    trainer = SFTTrainer(
        model=model,
        tokenizer=tokenizer,
        train_dataset=dataset,
        args=SFTConfig(
            output_dir=str(args.out),
            dataset_text_field="text",
            max_seq_length=MAX_SEQ_LEN,
            per_device_train_batch_size=args.batch_size,
            gradient_accumulation_steps=args.grad_accum,
            num_train_epochs=args.epochs,
            learning_rate=args.lr,
            warmup_ratio=0.05,
            lr_scheduler_type="cosine",
            logging_steps=10,
            optim="adamw_8bit",
            weight_decay=0.01,
            seed=args.seed,
            report_to="none",
        ),
    )

    trainer.train()

    args.out.mkdir(parents=True, exist_ok=True)
    model.save_pretrained(str(args.out))
    tokenizer.save_pretrained(str(args.out))
    print(f"LoRA 어댑터를 저장했습니다: {args.out}")

    if args.export_gguf:
        gguf_dir = args.out.parent / "gguf"
        gguf_dir.mkdir(parents=True, exist_ok=True)
        model.save_pretrained_gguf(
            str(gguf_dir), tokenizer, quantization_method=args.quantization
        )
        print(f"GGUF 모델을 저장했습니다: {gguf_dir} ({args.quantization})")
        print("Ollama 등록: ollama create xazz-guardrail -f experiments/slm_guardrail/Modelfile")

    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Xazz 보안 코드 보정 sLM — Unsloth + QLoRA 파인튜닝"
    )
    parser.add_argument("--data", type=pathlib.Path, default=DEFAULT_DATA)
    parser.add_argument("--out", type=pathlib.Path, default=DEFAULT_OUT)
    parser.add_argument("--base-model", default=BASE_MODEL)
    parser.add_argument("--epochs", type=float, default=3.0)
    parser.add_argument("--batch-size", type=int, default=2)
    parser.add_argument("--grad-accum", type=int, default=4)
    parser.add_argument("--lr", type=float, default=2e-4)
    parser.add_argument("--seed", type=int, default=42)
    parser.add_argument(
        "--export-gguf",
        action="store_true",
        help="학습 후 GGUF 로 변환한다 (llama.cpp / Ollama 서빙용)",
    )
    parser.add_argument("--quantization", default="q4_k_m", help="GGUF 양자화 방식")
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="학습하지 않고 데이터·설정만 검증한다 (GPU 불필요 — CI 용)",
    )
    args = parser.parse_args()

    records = load_records(args.data)
    summarize(records)

    if args.dry_run:
        print()
        print("--dry-run: 데이터 검증만 수행했습니다. 실제 학습은 GPU 환경에서 실행하세요.")
        print(f"  기반 모델 : {args.base_model}")
        print(f"  LoRA      : r={LORA_R}, alpha={LORA_ALPHA}, modules={len(TARGET_MODULES)}")
        print(f"  에폭      : {args.epochs}")
        print(f"  출력      : {args.out}")
        return 0

    return train(args, records)


if __name__ == "__main__":
    raise SystemExit(main())
