/// xazz-exec — Polars LazyFrame execution engine (v0.1)
///
/// This crate isolates the heavy runtime dependencies (Polars, encoding_rs).
/// The CLI binary (xazz) does not depend on this crate directly.
/// The xazz-runner binary uses this crate, and the CLI spawns xazz-runner
/// as a subprocess.
///
/// Dependency graph:
///   xazz (CLI)   → xazz-compiler (NO Polars) ✓
///   xazz-runner  → xazz-exec → Polars        ✓ (separate binary)
pub mod chart;
pub mod dl;
pub mod dp;
pub mod lower;
pub mod runtime;
pub mod tensor_bridge;

pub use runtime::run_pipeline;
