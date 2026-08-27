# sLM 보안 코드 보정 — Qwen2.5-Coder-1.5B QLoRA (issue #2)

Policy-as-Code 정적 가드레일에 차단된 `.xzz` 코드를, 온프레미스 소형 언어모델이
안전한 코드로 자동 보정하도록 파인튜닝하는 실험 디렉터리다.

> **먼저 읽어야 할 것**: 이 디렉터리는 **재현 가능한 학습·평가 스캐폴드**다.
> 학습된 가중치(LoRA 어댑터·GGUF)는 저장소에 포함되어 있지 않으며, 실제 QLoRA
> 학습에는 GPU 가 필요하다. 무엇이 완료됐고 무엇이 남았는지는 아래
> [현재 상태](#현재-상태)에 정확히 적어 두었다.

---

## 왜 이렇게 설계했는가

### 1. 정답지를 사람이 만들지 않는다

학습 데이터의 정답(안전 코드)을 사람이 손으로 쓰면, 라벨 품질이 작성자의
컨디션을 따라간다. 대신 **가드레일 엔진 자신**이 정답을 만든다.

```
위반 코드 합성 → xazz policy --fix --json → (위반 리포트, verified: true 인 안전 코드)
```

`build_dataset.py` 는 `verified: true` 인 쌍만 채택한다. 정책 엔진이 스스로
재검증한 코드만 정답이 되므로, 모델이 위반 코드를 정답으로 학습하는 일이
구조적으로 불가능하다.

### 2. 모델의 출력을 믿지 않는다

sLM 이 낸 코드는 **채택 전에 같은 정책 엔진으로 다시 검증**된다
(`xazz-server/src/guardrail.rs::remediate_with_slm`). 통과하지 못하면 폐기하고
결정적 보정 결과로 되돌아가며, 그 사실이 응답의 `notes` 에 남는다.

```
sLM 제안 → 재파싱 → 정책 재검증 → 통과해야만 채택
                              ↘ 실패 → 결정적 보정으로 폴백 (사용자에게 고지)
```

생성 모델을 보안 경로에 넣을 때 이 재검증 단계가 없으면, "그럴듯하지만 여전히
위반인 코드"가 안전하다는 이름표를 달고 나간다.

### 3. 학습 프롬프트와 추론 프롬프트가 같은 문자열이다

`build_dataset.py::PROMPT_HEADER` 와 `xazz-server/src/slm.rs::build_prompt` 는
같은 형식을 만든다. 형식이 어긋나면 파인튜닝 효과가 조용히 사라지므로,
프롬프트를 수정할 때는 **반드시 양쪽을 함께** 고쳐야 한다.

### 4. 개인정보를 한 건도 쓰지 않는다

모든 학습·평가 데이터는 합성이다. 실제 개인정보는 물론, 실제 개인정보를
비식별화한 데이터조차 사용하지 않는다.

---

## 파일

| 파일 | 역할 |
|---|---|
| `build_dataset.py` | 가드레일 엔진에서 (위반 → 검증된 안전 코드) 쌍을 뽑아 JSONL 생성 |
| `train_qlora.py` | Unsloth + QLoRA 파인튜닝 및 GGUF 변환 (`--dry-run` 은 GPU 불필요) |
| `evaluate.py` | 보정 결과를 가드레일로 재채점 — 정책 준수율·과잉 수정률·의도 보존율·지연 |
| `Modelfile` | Ollama 등록 정의 (온도 0.1, 정지 토큰, 시스템 프롬프트) |
| `data/seed_pairs.jsonl` | 커밋된 시드 데이터셋 72쌍 (재생성 가능) |
| `data/baseline_deterministic.json` | 결정적 보정 기준선 측정 결과 |

---

## 전체 파이프라인

```bash
# 0. 가드레일 빌드
cargo build --release -p xazz

# 1. 학습 데이터 생성 (실제 개인정보 불필요)
python3 experiments/slm_guardrail/build_dataset.py \
    --xazz target/release/xazz \
    --out experiments/slm_guardrail/data/train.jsonl

# 2. 데이터·설정 검증 (GPU 불필요)
python3 experiments/slm_guardrail/train_qlora.py \
    --data experiments/slm_guardrail/data/train.jsonl --dry-run

# 3. QLoRA 파인튜닝 (GPU 필요 — VRAM 8GB 이상)
python3 experiments/slm_guardrail/train_qlora.py \
    --data experiments/slm_guardrail/data/train.jsonl --export-gguf

# 4. Ollama 등록 및 서빙
ollama create xazz-guardrail -f experiments/slm_guardrail/Modelfile
ollama serve

# 5. 평가 — 정책 준수율 산출
python3 experiments/slm_guardrail/evaluate.py \
    --mode slm --model xazz-guardrail --xazz target/release/xazz \
    --report experiments/slm_guardrail/data/eval_slm.json

# 6. 서버에서 sLM 보정 켜기
XAZZ_SLM_ENABLED=1 XAZZ_SLM_MODEL=xazz-guardrail cargo run -p xazz-server
```

학습 의존성 (GPU 환경에서만):

```bash
pip install "unsloth[colab-new] @ git+https://github.com/unslothai/unsloth.git"
pip install trl peft accelerate bitsandbytes datasets
```

---

## 현재 상태

정직하게 나눠 적는다.

### 이 PR 에서 완료된 것

| 항목 | 상태 | 근거 |
|---|---|---|
| Policy-as-Code 정적 가드레일 | **완료** | `xazz-compiler/src/policy/` · 테스트 58건 |
| 위반 코드 실행 차단 (3중 게이트) | **완료** | CLI · xazz-exec · xazz-server, 테스트로 검증 |
| 결정적 자동 보정 + 재검증 | **완료** | `policy/remediate.rs` |
| 위반 리포트 JSON API | **완료** | `POST /security/policy/check`, `POST /security/remediate` |
| Ollama 어댑터 + 재검증 폴백 | **완료** | `xazz-server/src/slm.rs`, `guardrail.rs` |
| 학습 데이터 생성기 + 시드 72쌍 | **완료** | `build_dataset.py`, `data/seed_pairs.jsonl` |
| QLoRA 학습 스크립트 | **완료 (미실행)** | `train_qlora.py` — GPU 필요 |
| 평가 하네스 + 기준선 측정 | **완료** | `evaluate.py`, 아래 기준선 참조 |

### 후속 작업으로 남은 것

| 항목 | 필요 조건 |
|---|---|
| Qwen2.5-Coder-1.5B QLoRA 실제 학습 | GPU (VRAM 8GB+) |
| GGUF 변환 및 Ollama 로딩 검증 | 위 학습 완료 |
| sLM 보정 정확도·정책 준수율 측정 | 위 학습 완료 후 `evaluate.py --mode slm` |
| 학습 데이터 규모 확대 (72쌍 → 수천 쌍) | 템플릿·스키마 추가 |

**학습되지 않은 상태에서도 시스템은 완전히 동작한다.** `XAZZ_SLM_ENABLED` 가
꺼져 있거나 Ollama 에 연결하지 못하면 결정적 보정이 그대로 쓰이며, 이는
테스트로 보장된다 (`guardrail::tests::falls_back_when_slm_unreachable`).

---

## 결정적 보정 기준선

sLM 이 넘어야 할 기준선이다. `data/baseline_deterministic.json` 에 원본이 있다.

```
mode                     deterministic
samples                  72
parse_rate               1.0     (보정 코드가 항상 파싱된다)
policy_pass_rate         1.0     (보정 코드가 항상 정책을 통과한다)
over_edit_rate           0.0     (파이프라인을 지워서 통과시키지 않는다)
intent_retention_rate    1.0     (load 경로와 집계 연산이 보존된다)
mean_latency_ms          6.3
```

재현:

```bash
python3 experiments/slm_guardrail/evaluate.py \
    --mode deterministic --xazz target/release/xazz
```

**기준선이 이미 100% 인데 sLM 이 왜 필요한가?** 결정적 보정은 *투영 제거*와
*DP 삽입*만 한다. 위반은 확실히 없애지만, 사용자가 원했던 질문을 바꿔 놓는
경우가 있다. 예를 들어

```
원본:  select([age, disease])            "40대 이상 환자의 진단명을 보고 싶다"
결정적: select([age])                    → 위반은 사라졌지만 질문도 사라졌다
sLM 기대: groupBy("age_band")
         |> count("disease")
         |> withDp(epsilon: 1.0)         → 같은 질문에 안전하게 답한다
```

즉 sLM 의 목표 지표는 `policy_pass_rate`(이미 100%)가 아니라
**`intent_retention_rate` 를 유지한 채 질문을 살리는 재작성**이다.
`evaluate.py` 는 이 둘을 분리해 측정한다.
