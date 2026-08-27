# Changelog

All notable changes to Xazz are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)  
Versioning: [Semantic Versioning](https://semver.org/)

---

## [Unreleased]

### Added
- **Policy-as-Code 정적 보안 가드레일** (`xazz-compiler/src/policy/`, issue #2): `.xzz` 파이프라인이 실행되기 전에 개인정보 유출·보안 컴플라이언스 위반을 정적으로 탐지·차단
  - 규칙 12종: 직접 식별자 노출(`XZP001`), 민감 속성 행 단위 노출(`XZP002`), 준식별자 결합 재식별(`XZP003`), 민감 집계 DP 미적용(`XZP004`), ε 상한 초과(`XZP005`), PII/비밀키 하드코딩(`XZP010`·`XZP011`), 민감 경로 접근(`XZP012`), 경로 탈출(`XZP013`), 스키마 미해석(`XZP014`), 파싱 실패(`XZP000`), 정책 로딩 실패(`XZP999`)
  - **출력 컬럼 추론**(`PipelineShape`): 집계 결과 컬럼을 식별자와 구분해 정상 통계 쿼리의 오탐을 제거 (`groupBy("region") |> count("patient_id")` 는 통과)
  - **리터럴 스캐너**: 주민등록번호 체크섬·신용카드 Luhn·API 키 접두사·PEM 개인키 검증. 정규식 크레이트 없이 구현해 CLI 경량성 유지, 탐지값은 항상 마스킹해 보고
  - **fail-closed**: 정책 로딩 실패·파싱 실패는 실행 허용이 아니라 실행 거부
  - **Policy-as-Code JSON**: `XAZZ_POLICY_PATH` 또는 `xazz.policy.json` 으로 컬럼 분류·임계치·ε 상한·규칙별 심각도를 교체
  - **Domain Policy Pack 3종**: 의료(`healthcare_policy.json`) · 금융(`finance_policy.json`) · 공공(`public_sector_policy.json`). 공통 기준은 내장 정책이 담당하고 도메인별 규제는 팩으로 확장한다
  - **감사 증빙(Compliance Evidence)**: 모든 위반이 `rule_id` · `source_ref`(규제 근거) · `policy_version` · `domain` · `risk_level`(저·중·고위험)을 함께 기록해, 사후 감사에서 차단 근거를 따라갈 수 있다
- **3중 실행 게이트**: CLI(`xazz run`) · 실행 엔진(`xazz-exec` STEP 3.6) · API 서버(`POST /execute`). 실제 Polars 실행 직전인 `xazz-exec` 가 최종 관문이므로 어느 경로로 들어와도 정책이 적용된다
- **자동 보정 (결정적)** (`policy/remediate.rs`): AST 를 직접 고쳐 안전한 대체 코드를 생성하고 **재검증**까지 수행. 자동으로 고칠 수 없는 위반(하드코딩된 비밀값)은 `residual` 로 남겨 사람이 처리하도록 명시
- **AST → `.xzz` 프린터** (`policy/printer.rs`): 보정 코드를 문자열 치환이 아니라 AST 재출력으로 생성. `parse(print(parse(src))) == parse(src)` 왕복 성질을 테스트로 보장
- **온프레미스 sLM 보정 어댑터** (`xazz-server/src/slm.rs`): 파인튜닝된 Qwen2.5-Coder-1.5B 를 Ollama 로 로컬 서빙. **sLM 제안은 같은 정책 엔진으로 재검증을 통과할 때만 채택**되며, 실패·미연결 시 결정적 보정으로 자동 폴백
- 신규 CLI: `xazz policy <file> [--json] [--fix] [--out <path>]`
- 신규 API: `GET /security/policy`, `POST /security/policy/check`, `POST /security/remediate`. `POST /execute` 는 위반 시 **HTTP 422** + 위반 리포트 반환, 차단도 감사 로그에 `outcome: "blocked"` 로 기록
- `[xazz:policy]` stdout 마커: 차단·통과와 무관하게 항상 출력되어 프런트엔드가 검사 수행 사실을 신뢰할 수 있음
- **sLM 학습·평가 스캐폴드** (`experiments/slm_guardrail/`): 가드레일 엔진에서 (위반 → 검증된 안전 코드) 학습 쌍을 뽑는 생성기, Unsloth+QLoRA 학습 스크립트, GGUF/Ollama Modelfile, 정책 준수율·과잉 수정률·의도 보존율 평가 하네스, 시드 데이터셋 72쌍
- 보안 예제 (`examples/security/`): 위반·비밀키 유출·안전 파이프라인 3종 + 강화 의료 정책 + 합성 데이터 생성기
- 문서: [`docs/SECURITY_GUARDRAIL.md`](docs/SECURITY_GUARDRAIL.md) — 규칙 카탈로그, 게이트 구조, 정책 스키마, API, 알려진 한계(PATH 셰도잉)
- **Burn 딥러닝 실행 엔진** (`xazz-exec/src/dl.rs`): `model {}` 선언 → `train()` 학습 (Adam + MSE), 특성 표준화, train/validation 분할, in-sample 예측, `checkpoints/<model>.json` 체크포인트 저장
- **train/predict 파이프라인 연산자 전환**: `train(Model, ...)` / `predict(model, as: "col")`을 파이프라인 연산자로 추가
  - 학습 결과를 `TrainedModel`(모델 + 표준화 통계)에 바인딩해 예측·시각화로 연결
  - 레거시 슈가 `run |> train` 유지
  - `ModelDecl`을 실행 전 선언부에서 먼저 등록해 **위치 무관 사용** 지원
- `emit rust` 딥러닝 코드 생성: `.xzz`의 `model {}`/`train()` → 독립 Burn `nn` 모듈 + 학습 코드로 변환
- **정적 의미 분석기 (Type Checker)** 도입 (`xazz-compiler/checker.rs`):
  - 미선언 변수/모델/스키마, 중복 선언, 스키마 컬럼 존재성(`SafeLoadViolation` + Did-you-mean), join 대상, cast 타입, groupBy→집계 누락, 문자열 집계 경고, train/predict 참조 검증
  - `xazz check` CLI: 기존 NQP mock → 실제 정적 분석으로 교체 (오류 시 exit 1)
  - 실행 전 preflight + `[xazz:diagnostics]` JSON 마커
- **정적 진단 소스 위치(Span) 첨부**: `check_source`가 토큰 스트림을 명령문 단위로 분할(`segment_statements`)하여 각 오류/경고에 `[N행:M열]` 위치 표시, 식별자 위치 검색 실패 시 명령문 시작 위치로 폴백
- **감사 로그** (`xazz-server/audit_log.rs`): SHA-256 **append-only JSONL** 감사 로그(해시 체인), `/security/audit` 로그 영구 저장 + 로그·해시 조회·체인 무결성 검증 API
- 서버 보안 엔드포인트: `/health`, `/security/audit`, `/security/verify`
- `DivisionByZero` 검출 + Safe-Load 검증 강화
- `xazz run --json`: 구조화 JSON 실행 결과(rows/schema/diagnostics/logs) 출력
- 서버 `/execute`가 `[xazz:train]` 마커 파싱해 학습 결과(`training`) 반환, 응답에 `diagnostics` 필드 추가
- 신규 파이프라인 연산자: `sample(n)`, `median()`, `variance()`, `std()`
- **Visual IDE** (Vite + React + @xyflow/react):
  - 승인된 UX 프로토타입·디자인 산출물(디자인 spec v0.1.1, design-system, design-evidence, 9장 참고 스크린샷) 도입
  - 컴파일러 캔버스를 Burn ML 단계로 확장: 전처리 → Compile AirNet / Train model / Predict, 노드에 `band`·`from`·`position` 명시 (다중 소스 연산자 표현)
  - Monitoring 뷰 추가 (Graph/Split/Code와 통합): Burn 컴파일·학습(Beta) / DP 예산(Research) / 리소스 효율(Planned) 3단계 증거 등급 분리
  - visual IDE를 실서버 `xazz-server`에 연결: `api.js`(/execute, /health), Full Run이 `.xzz` 코드를 POST `/execute`로 실행하고 Preview/Logs/Receipt/Monitor가 실응답 렌더, 데모 데이터(`seoul_air_quality.csv`) 및 실행 가능한 `.xzz` 소스 추가
  - `find_xazz_exe` 크로스플랫폼 지원(`xazz`/`xazz.exe`)
- 테스트: compiler·codegen 테스트 슈트, emitter 코드젠 단위테스트 6개, xazz-exec 폴리시 실행 통합테스트 3개, checker 유닛 테스트 20개
- CI 강화: `clippy -D warnings` 강제, 프런트엔드(Vite 빌드 + 컨트랙트 + 대비 테스트) 잡 추가

### Changed
- 프로젝트명/패키지 리브랜딩: `x1zz` → `xazz` (README, 크레이트명, CI 전반)
- `chart { type: ... }` 필드명 통일 (`kind` → `type`)
- 예제를 목적별 디렉토리로 재구조화: `examples/{data, deep_learning, end_to_end, preprocessing, visualization}`
- 리포지토리 재구조화: 문서를 `docs/` 하위로 통합, `ui-prototype` → `visual-ide` 리네이밍, 빌드 산출물/`core_code.zip` 제거
- 대회 제출용 시각자료 계획(`docs/submission_visuals_plan.md`) 신규 + 결과보고서 서술을 '파이썬 하이퍼 퍼포먼스 모듈' 톤으로 재구성
- README 기능/로드맵 상태 테이블을 실제 구현 상태로 정리, contributions 재개

### Fixed
- `LayerKind::to_burn_str` Dense 매핑 오류 수정 (`Linear(n,n)` → 입력 차원 스키마 기반 추론)
- `PipelineOp` enum에 `Train`/`Predict` variant 누락으로 인한 컴파일 에러 수정 (`xazz-core/src/ast.rs`)
- checker/emitter/integration 테스트 입력 문법 오류 수정: 예약 키워드 `v`/`model` 변수명, 모델 레이어 괄호(`ReLU()` → `ReLU`), `train()` 명명 인자 중괄호, join 키/방식 문자열 리터럴 교정
- CI `cargo fmt` 포맷팅 이슈, `clippy -D warnings` 회귀로 인한 레거시 린트 차단 → 경고 허용으로 복원
- rustfmt/clippy CI 컴포넌트 설치 오류 수정
- 사소한 린트: trailing blank line 정리, 미사용 바인딩 정리

### Troubleshooting
- `xazz check`/Type Checker 파생 테스트 문법이 실제 파서 문법과 불일치 → 테스트 입력을 실제 문법에 맞게 교정, emitter 테스트는 `load_csv` 헬퍼 기준 단언으로 수정(`LazyCsvReader` 아님)
- Burn 레이어 치수가 `n`→`n` 으로 잘못 매핑되어 Dense 오류 → 스키마 입력 차원 기반 추론으로 교체
- 파이프라인 연산자 `Train`/`Predict` variant가 `xazz-compiler`에서 참조됐지만 `xazz-core` enum에 미정의 → 컴파일 실패 디버깅 후 variant 추가

### Architecture
- `xazz-core`: `LayerKind`, `TrainConfig`, `ModelDecl`, `TrainStmt` AST 추가
- `xazz-compiler`: model/train 파싱 + codegen, 정적 의미 분석기, Span 진단
- `xazz-exec`: Burn 실행 런타임(+DL 파이프라인), preflight + 진단 마커
- `xazz-server`: 감사로그 해시 체인 + 보안/진단 마커 파싱 + Health
- 레거시 정리: `cli_integration/`, `test/`, `poc_*_generated.rs`, `core_code.zip`, `commitGuide.txt` 삭제

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