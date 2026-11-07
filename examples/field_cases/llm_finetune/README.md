# LLM 파인튜닝 데이터 위생 현장 사례 — `xazz sanitize`

LoRA/QLoRA 파인튜닝 직전에 학습 데이터를 검사하는 현장 사례다. "그냥 모은 대화 로그를
바로 학습에 넣는다"가 왜 위험한지, 그리고 `xazz sanitize`가 그 위험을 **학습 전에**
구조화 리포트로 드러내는 것을 보여준다.

> ⚠️ 데이터는 고정 시드로 생성한 **합성 데이터**다. 실제 고객 대화·개인정보는 한 건도
> 포함하지 않는다. 프레이밍은 [`docs/design/field-validation.md`](../../../docs/design/field-validation.md) 참고.

| 파일 | 내용 |
|---|---|
| `generate_corpus.py` | `data/train.csv`: 합성 support 챗봇 코퍼스 (기본 600 + 근접중복) |
| — | `xazz sanitize` 로 PII · 중복 · 편향을 검사한다 (파이프라인 아님) |

## 실행

```bash
cd examples/field_cases/llm_finetune
python3 generate_corpus.py                 # data/train.csv (시드 42)
xazz sanitize data/train.csv               # 사람이 읽는 리포트
xazz sanitize data/train.csv --json        # 구조화 리포트 (파인튜닝 인테이크 아티팩트)
```

## 무엇을 보여주는가 (시드 42, 720행)

`sanitize` 는 세 가지를 잡는다:

| 검사 | 결과 | 왜 위험한가 |
|---|---|---|
| **PII** | `response` 720건 (email, phone_number) — 원문은 마스킹되어 리포트에만 남음 | 학습 데이터에 개인정보가 들어가면 모델이 그대로 외워 유출한다 |
| **중복** | exact row 14.17%, `response` 근접중복 22.36% | 중복은 과적합·데이터 누수를 만든다 |
| **편향** | `intent` 불균형 12.1x, 지배 라벨 60% (refund 434 vs other 36) | 편향된 분포는 모델을 한쪽으로 치우치게 한다 |

리포트는 원문 PII를 **절대 그대로 출력하지 않는다**(마스킹 샘플만). `--json` 출력은
파인튜닝 인테이크 아티팩트로 저장해 감사 근거로 쓸 수 있다.

## 실무 흐름에서의 위치

```
원시 로그 → [xazz sanitize] → (PII 정화 · 중복 제거 · 편향 리뷰) → LoRA/QLoRA 파인튜닝
                    │
                    └─ 구조화 리포트 = "무엇을 확인했고 무엇을 고쳐야 하는가"의 기록
```

이 게이트는 [Track F3](../../../docs/ROADMAP.md)(파인튜닝 데이터 정화)의 실행면이며,
F1(프롬프트 입력 게이트)·F2(LLM 출력 재스캔)와 함께 GenAI 거버넌스 3-게이트를 이룬다.

## 한국어 요약

파인튜닝 직전 학습 데이터를 검사하는 사례다. 합성 support 챗봇 코퍼스(720행)를
`xazz sanitize` 로 돌리면 PII 720건, exact 중복 14.17%, `intent` 편향 12.1x가
잡힌다. 리포트는 원문 개인정보를 마스킹하며, `--json`은 파인튜닝 인테이크 아티팩트로
쓸 수 있다.
