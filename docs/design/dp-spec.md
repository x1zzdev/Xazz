# 차등 프라이버시(DP) 사양 — 메커니즘과 조성 회계

상태: 구현됨 (v0.3.0) · 코드: [`xazz-exec/src/dp.rs`](../../xazz-exec/src/dp.rs)

---

## 1. 프라이버시 모델

Xazz 의 `withDp(...)` 는 **집계 결과에 대한 출력 섭동(output perturbation)** 을
적용한다. 즉 각 (εᵢ, δᵢ)-DP 메커니즘의 합성 결과가 곧 쿼리셋 전체의 프라이버시
인증이다.

- **인접 데이터셋(neighboring datasets)**: 한 행이 추가/제거된 두 데이터셋.
- **DP 정의**: (ε, δ)-Differential Privacy — 모든 인접 데이터셋 D, D′ 와 모든
  출력 집합 S 에 대해 Pr[M(D)∈S] ≤ e^ε · Pr[M(D′)∈S] + δ.

---

## 2. 메커니즘

### Laplace (ε-DP, δ=0)

- 노이즈: `Lap(0, Δf/ε)` — scale `b = Δf/ε`.
- 순수 ε-DP 이므로 δ 기여는 **0**.

### Gaussian (ε, δ)-DP

- 노이즈: `N(0, σ²)`, σ = Δf·√(2·ln(1.25/δ))/ε (Dwork & Roth Thm 3.22).
- (ε, δ)-DP 이므로 **ε 과 δ 를 모두** 조성 회계에 반영.

### 민감도(sensitivity)

- `sensitivity` 인자(Δf) — 기본 1.0. 사용자가 집계 함수에 맞게 설정한다
  (예: count → 1, mean → 1/n).
- 클리핑된 쿼리나 그룹 수 검증은 현재 적용 범위 밖이며, 그룹 키가 아닌
  값 컬럼에만 노이즈가 가해진다.

---

## 3. 조성 회계 (Composition Accounting)

`PrivacyBudget` 은 **기본 순차 조성**(Dwork & Roth Thm 3.16) 을 사용한다 —
k 개의 (εᵢ, δᵢ)-DP 메커니즘은 **(Σεᵢ, Σδᵢ)-DP** 로 합성된다(정확).

- Laplace: δ 기여 0 → ε 만 누적.
- Gaussian: ε 과 δ 를 모두 누적.

### 예산 설정

| 환경변수 | 기본값 | 의미 |
|---|---|---|
| `XAZZ_DP_BUDGET` | 10.0 | 총 ε 예산 |
| `XAZZ_DP_DELTA_BUDGET` | 1e-4 | 총 δ 예산 (Gaussian 용) |

### 거부 규칙 (fail-closed)

각 `withDp` 호출은 먼저 `spend(mechanism, ε, δ)` 로 예산을 차감하고,
`Σε > total_ε` 또는 `Σδ > total_δ` 가 되면 해당 쿼리를 **거부**한다.
거부된 요청은 예산을 소모하지 않는다(원자적). 이는 반복 질의를 통한
노이즈 평균화(재구성 공격)를 구조적으로 차단한다.

---

## 4. 표준화 vs 엄밀 검증

이 구현은 **결정적이고 설명 가능한 조성 회계**를 제공한다. 다음 항목은
지금은 다루지 않으며, "엄밀하게 검증된 프라이버시 프레임워크"로 부르기
위해서는 후속 검토가 필요하다:

- 적응형 쿼리(adaptive query)에 대한 이론적 경계
- RDP / advanced composition 을 통한 tighter bound
- 순차 수행을 넘는 병렬 조성 (parallel composition)
- sensitivity 자동 추론 (aggregation-aware, clipping)
- 외부 감사 도구(OpenDP 등)와의 결과 대조

현재 문서의 의미론은 **기본 순차 조성의 정확한 합성**을 그대로 구현한 것이다.