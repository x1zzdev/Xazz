# Typed IR 설계 — Xazz 컴파일러 중간 표현

상태: 구현됨 (v0.3.0) · 코드: [`xazz-core/src/ir.rs`](../../xazz-core/src/ir.rs)

---

## 목적

`.xzz` 소스는 렉서 → 파서 → **raw AST** 를 거친 뒤 **정적 분석(타입체커)** 이
**Typed IR** 을 생성하고, 실행 엔진이 이 IR 을 1회 소비한다. AST(구문)와
백엔드(Polars/Burn) 사이의 정적 의미 계층이다.

핵심 속성:
1. **이중 해석 제거** — 예전 구조는 런타임이 소스를 다시 렉싱·파싱해 raw AST 를
   직접 Polars/Burn 에 해석했다. 이제 컴파일러가 구조화된 IR 을 1회 만들고
   런타임은 1회 소비한다.
2. **모든 표현식이 타입을 가진다** — `TypedExpr { kind, ty }`.
3. **도메인 분리 + 순서 보존** — 데이터/ML/부수 연산을 분리한 enum 에 담고,
   `Step` 태그로 감싼 순차 시퀀스로 저장한다.

---

## 타입 개요

| IR 타입 | 역할 |
|---------|------|
| `ColType` | `String / Int / Float / Bool / Unknown / Nullable<T>` |
| `SchemaField` / `Schema` | 이름 + 타입 목록 (파이프라인 입출력 타입) |
| `TypeDecl` | `type Name = { ... }` (이름 + Schema) |
| `TypedExpr` / `TypedExprKind` | 타입이 붙은 표현식 (Column/리터럴/BinOp) |
| `DataOp` | 데이터 도메인 연산 (Polars lower 대상) |
| `MLOp` | `Train / Predict` (Burn lower 대상) |
| `SideOp` | `Chart / WithDp` (시각화·프라이버시 하위시스템) |
| `Step` | `Data | ML | Side` 태그 — 파이프라인 순서 보존 |
| `PipelineNode` | 소스, 입출력 스키마, Step 시퀀스, `yields_model` |
| `ModelGraph` | `model Name { ... }` 레이어 목록 |
| `TypedProgram` | `types + models + pipelines` (컴파일 단위) |

### 왜 `Step` 으로 순서를 보존하나

순서가 의미를 바꾸는 경우가 있다. `filter |> withDp |> select` 는 노이즈가
주입된 뒤 컬럼을 축소하지만, `filter |> select |> withDp` 는 축소된 결과에
노이즈를 주입한다. 단순히 `data_ops`/`ml_ops` 두 벡터로 나누면 이 순서가
소실된다. `Step` 태그 + 단일 시퀀스가 이를 보존한다.

---

## 생성 (프론트엔드)

타입체커(`xazz-compiler/src/checker.rs`)가 **검사와 IR 생성을 단일 순회**로
수행한다 (이중 추론 금지).

- `analyze_program(&Program) -> (CheckResult, TypedProgram)`
- `compile_ir(source) -> (CompileResult<(Program, TypedProgram)>, CheckResult)` (Span 포함)

체커는 스키마에서 컬럼 타입을 해석하고, 연산자마다 출력 스키마 변화를 추론한다:
- `Select` → 컬럼 축소
- `GroupBy + Aggregate` → 키 + 집계 타입
- `WithColumn` → 새 컬럼 타입 (이 표현식의 결과 타입)
- `Cast` → 타입 교체
- `withDp` → 숫자형 컬럼 float 승격
- `predict(as:)` → float 예측 컬럼 추가

---

## 소비 (백엔드 lowering)

`xazz-exec` 가 `PipelineNode` 를 순회하며 `Step` 태그별로 분기한다:

```
Step::Data(op)  → lower::lower_data(op, lf, symbol_table, pending_group)   // Polars
Step::ML(op)    → dl::train / dl::predict                                  // Burn
Step::Side(op)  → chart::… / dp::apply_dp (+ PrivacyBudget 조성 회계)
```

각 lowering 모듈은 자기 도메인만 안다 (backend-specific knowledge 단일 위치).

---

## 최적화 (xazz-compiler::opt)

최적화는 IR 위에서 수행되어 백엔드에 독립적이다.

- `fold_constants` — 리터럴 이항식 폴딩. 0 나눗셈은 런타임 의미 보존을 위해
  폴딩하지 않는다.
- `merge_selects` — 연속 `Select` 병합 (`Select(A) |> Select(B)` → `Select(B)`).
- `pushdown_filters` — `Filter` 를 그 경계가 참조 컬럼을 보존하면 `Select`/`WithColumn`
  앞으로 이동. `WithColumn` 은 신규 컬럼을 참조하면 이동 불가.

정당성: 구조 단위테스트 + `xazz-exec` 의 **실행 동치 테스트** (원본 IR 과 최적화
IR 을 각각 Polars 로 실행해 DataFrame 동일함을 검증). `xazz-exec --opt` 로 선택
활성화.

---

## 검증 요약

- xazz-core: IR 타입 단위테스트
- xazz-compiler: checker IR 생성 테스트 (pipeline/ML/side op 순서), opt 패스 테스트
- xazz-exec: lower 단위테스트 + 최적화 전/후 동치 테스트