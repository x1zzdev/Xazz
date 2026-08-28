/// xazz-exec — Polars LazyFrame 실행 엔진 (v0.1)
///
/// 이 크레이트는 무거운 런타임 의존성(Polars, encoding_rs)을 격리한다.
/// CLI 바이너리(xazz)는 이 크레이트에 직접 의존하지 않는다.
/// xazz-runner 바이너리가 이 크레이트를 사용하며,
/// CLI는 xazz-runner를 서브프로세스로 스폰한다.
///
/// 의존성 그래프:
///   xazz (CLI)   → xazz-compiler (NO Polars) ✓
///   xazz-runner  → xazz-exec → Polars        ✓ (분리된 바이너리)
pub mod dl;
pub mod dp;
pub mod lower;
pub mod runtime;
pub mod tensor_bridge;

pub use runtime::run_pipeline;
