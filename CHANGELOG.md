# Changelog

All notable changes to Xazz are documented in this file.

Format: [Keep a Changelog](https://keepachangelog.com/en/1.0.0/)  
Versioning: [Semantic Versioning](https://semver.org/)

---

## [Unreleased]

- Open source readiness pass (repository hygiene, documentation)

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
