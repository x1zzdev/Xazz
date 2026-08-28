# Changelog

All notable changes to Xazz are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)  
Versioning: [Semantic Versioning](https://semver.org/)

---

## [v0.3.0] — 2026-08-29

> **Typed IR 컴파일러 아키텍처 마일스톤.** 이번 릴리스는 "기능 추가"가 아니라
> **핵심 컴파일러 추상화 도입**에 초점을 맞췄다. AST와 백엔드(Polars/Burn) 사이에
> 정적 타입이 부착된 중간 표현(Typed IR)을 두어 기존의 **이중 해석 구조**를 제거했다.
>
> 변경 전: 컴파일러가 문자열 codegen만 만들고, 런타임(`xazz-exec`)이 소스를
> **다시 렉싱·파싱**해 raw AST를 직접 Polars/Burn에 해석.
> 변경 후: `Lexer → Parser → AST → (정적 분석) → Typed IR → (최적화) → lowering → 백엔드`.
> 컴파일러가 구조화된 IR을 1회 생성하고, 런타임은 이를 1회 소비한다.

### Added — Typed IR / 컴파일러 (이슈 #37~#45)

- **Typed IR** (`xazz-core::ir`): `ColType`/`Schema`/`TypedExpr`(모든 표현식이 결과 타입 보유), `DataOp`(데이터), `MLOp`(학습/예측), `SideOp`(차트/DP), `Step`(순서 보존 태그), `PipelineNode`, `TypedProgram`
- **이중 해석 제거**: 타입체커가 검사와 함께 IR을 **단일 순회**로 생성 (`analyze_program`/`compile_ir`). `xazz-exec`는 raw AST 대신 IR을 소비 (기존 `execute_var_decl`/`handle_train_stmt` 등 구 인터프리터 -606줄 삭제)
- **IR 최적화 계층** (`xazz-compiler::opt`): 상수 폴딩, 연속 `Select` 병합(projection 축소), 조건 푸시다운(`filter`를 `select` 앞으로). Polars 실행 동치 테스트로 정당성 검증. `xazz-exec --opt` 로 선택 활성화
- **zero-copy 텐서 브리지**: 연속(단일 청크·무결측) `Float64/Float32` 컬럼은 Arrow raw 버퍼(`cont_slice`)를 직접 읽어 컬럼별 중간 `Vec` 복사 제거. 불가피한 복사 경계(f64→f32 강등, columnar→row-major, host→device)는 모듈 주석에 명시
- **DP 조성 회계**: `PrivacyBudget`가 ε 단순 합산 대신 `(ε, δ)` 병행 누적(기본 순차 조성). Laplace는 순수 ε-DP(δ=0), Gaussian은 δ도 소모. `XAZZ_DP_DELTA_BUDGET` 신설. `[xazz:dp]` 마커에 `budget_spent_delta`/`total_delta`/`query_count` 추가
- **실행 타임아웃 하드닝**: `xazz-runner`가 서브프로세스 최대 실행 시간 제한(기본 300초, `XAZZ_EXEC_TIMEOUT_SECS`), 미세 프로세스 격리 ≠ OS 샌드박스 명시
- **God runtime 해체**: `xazz-exec`를 도메인별 모듈로 분리 — `lower`(DataOp→Polars), `dl`(Burn), `dp`, `chart`(시각화), `runtime`(얇은 오케스트레이션, 1433줄 → 720줄)
- 전 크레이트 버전 통일: workspace 단일 버전(0.3.0), `xazz-server`도 workspace 버전·edition(2024) 흡수, 내부 path 의존성 version 하드코딩 제거

### Added — (이전 [Unreleased] 기능 전부, v0.3.0 에 포함)

- **Policy-as-Code 정적 보안 가드레일** (`xazz-compiler/src/policy/`, issue #2): `.xzz` 파이프라인이 실행되기 전에 개인정보 유출·보안 컴플라이언스 위반을 정적으로 탐지·차단
  - 규칙 12종: 직접 식별자 노출(`XZP001`), 민감 속성 행 단위 노출(`XZP002`), 준식별자 결합 재식별(`XZP003`), 민감 집계 DP 미적용(`XZP004`), ε 상한 초과(`XZP005`), PII/비밀키 하드코딩(`XZP010`·`XZP011`), 민감 경로 접근(`XZP012`), 경로 탈출(`XZP013`), 스키마 미해석(`XZP014`), 파싱 실패(`XZP000`), 정책 로딩 실패(`XZP999`)
  - **출력 컬럼 추론**(`PipelineShape`): 집계 결과 컬럼을 식별자와 구분해 정상 통계 쿼리의 오탐을 제거 (`groupBy("region") |> count("patient_id")` 는 통과)
  - **리터럴 스캐너**: 주민등록번호 체크섬·신용카드 Luhn+IIN·API 키 접두사·PEM 개인키 검증. 정규식 크레이트 없이 구현해 CLI 경량성 유지, 탐지값은 항상 마스킹해 보고
  - **fail-closed**: 정책 로딩 실패·파싱 실패는 실행 허용이 아니라 실행 거부
  - **Policy-as-Code JSON**: `XAZZ_POLICY_PATH` 또는 `xazz.policy.json` 으로 컬럼 분류·임계치·ε 상한·규칙별 심각도를 교체
  - **Domain Policy Pack 3종**: 의료(`healthcare_policy.json`) · 금융(`finance_policy.json`) · 공공(`public_sector_policy.json`)
  - **감사 증빙(Compliance Evidence)**: 위반마다 `rule_id` · `source_ref` · `policy_version` · `domain` · `risk_level` 기록
- **3중 실행 게이트**: CLI(`xazz run`) · 실행 엔진(`xazz-exec` STEP 3.6) · API 서버(`POST /execute`)
- **자동 보정 (결정적)** (`policy/remediate.rs`) + **AST → `.xzz` 프린터** (`policy/printer.rs`, 왕복 파싱 보장)
- **온프레미스 sLM 보정 어댑터** (`xazz-server/src/slm.rs`): Qwen2.5-Coder-1.5B (Ollama), sLM 제안은 같은 정책 엔진 재검증 통과 시에만 채택
- 신규 CLI `xazz policy` / 신규 API `GET /security/policy`, `POST /security/policy/check`, `POST /security/remediate` (위반 시 HTTP 422)
- `[xazz:policy]` stdout 마커, **sLM 학습·평가 스캐폴드** (`experiments/slm_guardrail/`), 보안 예제 (`examples/security/`)
- 문서: [`docs/SECURITY_GUARDRAIL.md`](docs/SECURITY_GUARDRAIL.md)
- **Burn 딥러닝 실행 엔진** (`xazz-exec/src/dl.rs`): `model {}` → `train()` (Adam + MSE), 특성 표준화, train/validation 분할, in-sample 예측, 체크포인트 저장
- **train/predict 파이프라인 연산자**: `train(Model, ...)` / `predict(model, as: "col")`, `TrainedModel` (모델 + 표준화 통계) 바인딩, 레거시 `run |> train` 유지, `ModelDecl` 위치 무관 사용
- `emit rust` 딥러닝 코드 생성
- **정적 의미 분석기 (Type Checker)** (`xazz-compiler/checker.rs`): 미선언 변수/모델/스키마, 스키마 컬럼 존재성(`SafeLoadViolation` + Did-you-mean), groupBy→집계 누락, 문자열 집계 경고, train/predict 참조 검증. `xazz check` CLI 교체. 실행 전 preflight + `[xazz:diagnostics]`
- **정적 진단 소스 위치(Span)**: `check_source`가 명령문 단위 토큰 분할로 `[N행:M열]` 위치 표시
- **감사 로그** (`xazz-server/audit_log.rs`): SHA-256 append-only JSONL + `/security/audit`, `/security/verify`
- `DivisionByZero` 검출, `xazz run --json`, 서버 `/execute` → `[xazz:train]` 파싱, `diagnostics` 필드
- 신규 연산자: `sample(n)`, `median()`, `variance()`, `std()`
- **Visual IDE** (Vite + React + @xyflow/react): 컴파일러 캔버스(Burn ML 단계), Monitoring 뷰, 실서버(`xazz-server`) 연결, 크로스플랫폼 실행
- 테스트/CI: checker 유닛 20개+IR 테스트, 폴리시 실행 통합테스트, 클리피 강제, 프런트엔드 빌드 잡

### Changed

- **컴파일러 파이프라인**: AST → Typed IR → (최적화) → lowering 구조로 전환 (문자열 codegen 실행 경로 제거)
- 프로젝트명/패키지 리브랜딩: `x1zz` → `xazz` (README, 크레이트명, CI 전반)
- `chart { type: ... }` 필드명 통일 (`kind` → `type`)
- 예제를 목적별 디렉토리로 재구조화: `examples/{data, deep_learning, end_to_end, preprocessing, visualization}`
- 리포지토리 재구조화: 문서를 `docs/` 하위로 통합, `ui-prototype` → `visual-ide` 리네이밍
- README 기능/로드맵 상태 테이블을 실제 구현 상태로 정리

### Fixed

- `LayerKind::to_burn_str` Dense 매핑 오류 (입력 차원 스키마 기반 추론)
- `PipelineOp`에 `Train`/`Predict` variant 누락 컴파일 에러
- checker/emitter/integration 테스트 입력 문법 오류 수정
- CI `cargo fmt`/클리피 회귀, CI 컴포넌트 설치 오류
- `<0.3.0` 내부 path 의존성 버전 불일치 (workspace 단일 버전으로 통일)

### Architecture

- `xazz-core`: `ir` 모듈 (Typed IR) 신설
- `xazz-compiler`: `checker` → IR 생성, `opt` 최적화 패스 신설
- `xazz-exec`: `lower`/`dl`/`dp`/`chart` 도메인 분리, IR 단일 소비
- `xazz-runner`: 실행 타임아웃 하드닝
- 전 워크스페이스 버전·edition 통일

---

## [v0.2.8] — 2026

### Changed
- CI: removed macOS x64 release target (arm64 only)

---

## [v0.2.7] — 2026

### Fixed
- CI: removed bash-only shell command from Windows packaging step

---

## [v0.2.5 / v0.2.4] — 2026

### Fixed
- CI: stabilized multi-platform packaging and archive validation

---

## [v0.2.3] — 2026

### Fixed
- Cargo workspace configuration
- CI pipeline fixes

---

## [v0.2.2] — 2026

### Added
- GitHub Actions release pipeline (`.github/workflows/release.yml`)
- Multi-platform build matrix: Windows x64, Linux x64, macOS arm64
- Automated archive packaging and checksum generation

---

## [v0.2.1] — 2026

### Added
- Initial release pipeline
- Binary separation: `xazz` CLI + `xazz-runner` + `xazz-exec`

---

## [v0.2.0] — 2026

### Added
- MVP release
- `xazz new` — project scaffolding with sample CSV
- `xazz import` — CSV schema auto-inference (EUC-KR/CP949 support)
- `xazz run` — pipeline execution via `xazz-runner` subprocess
- `xazz emit rust` — transpile `.xzz` to Rust (Polars LazyFrame)
- `xazz check` — experimental NQP static analysis stub
- `xazz sde` — synthetic data engine integration stub
- Chart visualization: `chart {}` block (bar, line, pie, scatter)
- Pipeline operators: `filter`, `groupBy`, `join`, `withColumn`, `cast`, `rename`, `sort`, `select`, `mean`, `fillNull`
- `Option<T>` null-safe type system
- Dependency isolation: Polars removed from CLI binary, isolated to `xazz-exec`
- Multi-crate workspace: `xazz-core`, `xazz-compiler`, `xazz-exec`, `xazz-runner`, `xazz-server`
- CSV LFS migration for large example data files
- Benchmark: 3.84× speedup over pandas on 3.4M-row workload

### Architecture
- `xazz` CLI binary: no Polars, no Tokio (~2–5 MB)
- `xazz-runner` spawned as subprocess for pipeline execution
- `xazz-exec` carries Polars LazyFrame runtime (~30+ MB)

---

*Earlier development history is available via `git log`.*