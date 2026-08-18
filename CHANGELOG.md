# Changelog

All notable changes to Xazz are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)  
Versioning: [Semantic Versioning](https://semver.org/)

---

## [Unreleased]

### Added
- Burn 딥러닝 실행 엔진 (`xazz-exec`): `model {}` 선언 → `train()` 학습 (Adam + MSE), 특성 표준화, train/validation 분할, in-sample 예측, `checkpoints/<model>.json` 체크포인트 저장
- `emit rust` 딥러닝 코드 생성: `.xzz`의 `model {}`/`train()` → 독립 Burn nn 모듈 + 학습 코드로 변환
- `fillNull(strategy:)` 연산자: 평균(`"mean"`)/중앙값(`"median"`)/0(`"zero"`) 채우기 전략 (README 예제 호환)
- 서버 `/execute`가 `[xazz:train]` 마커를 파싱해 학습 결과(`training`)를 반환
- 딥러닝 예제 (`examples/deep_learning/air_quality_predictor.xzz`) 개선 및 검증

### Changed
- `LayerKind::to_burn_str` Dense 매핑 오류 수정 (`Linear(n,n)` → 입력 차원 스키마 기반 추론)
- README 기능/로드맵 상태 테이블을 실제 구현 상태로 정리

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
