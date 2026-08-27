# xazz_dp — 차등 프라이버시(DP) 모듈 Python 구현

Rust 엔진(`xazz-exec/src/dp.rs`)과 **동일한 알고리즘·상수·RNG(SplitMix64)** 를 사용하는
Python 레퍼런스 구현. 같은 seed 를 주면 Rust 엔진과 동일한 노이즈 시퀀스를 재현하므로
두 구현이 서로를 교차 검증한다.

## 용도

- Python 파이프라인(sLM 파인튜닝 데이터셋 등)에서의 비식별화 처리
- Rust 엔진 출력의 독립 검증 (감사 목적)
- 노트북/실험 환경에서의 DP 파라미터(ε, Δf, δ) 탐색

## 사용 예

```python
import pandas as pd
from xazz_dp import apply_dp, PrivacyBudget

df = pd.DataFrame({"region": ["SEOUL", "BUSAN"], "patient_count": [120, 85]})

budget = PrivacyBudget.from_env()          # XAZZ_DP_BUDGET (기본 10.0)
noised, report = apply_dp(
    df,
    epsilon=1.0,                            # 프라이버시 예산 ε (필수)
    mechanism="laplace",                    # laplace(기본) | gaussian
    sensitivity=1.0,                        # 쿼리 민감도 Δf
    seed=42,                                # 지정 시 Rust 엔진과 동일 노이즈 재현
    budget=budget,                          # 세션 ε-budget 차감 (초과 시 거부)
)
print(report)   # Rust 의 [xazz:dp] JSON 마커와 동일 스키마
```

코어(노이즈 생성·예산 관리)는 표준 라이브러리만 사용하며,
`apply_dp` 는 pandas / polars DataFrame 을 duck-typing 으로 지원한다.

## 테스트

```bash
python python/test_xazz_dp.py
```

Rust 테스트(`cargo test -p xazz-exec dp::`)와 동일 시나리오 13개
(노이즈 통계 검증, seed 결정성, SplitMix64 공개 테스트 벡터, ε-budget 차단 등).
