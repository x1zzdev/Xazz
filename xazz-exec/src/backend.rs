//! xazz-exec/src/backend.rs — ML compute-backend abstraction at the MLOp boundary.
//!
//! The Typed IR (`xazz_core::ir::MLOp`) is backend-independent. This module is the
//! single dispatch point between that IR and a concrete ML engine: `runtime`
//! calls [`active()`]`.train(..)` / `.predict(..)` instead of a concrete engine.
//!
//! Burn + burn-ndarray (pure-Rust CPU) is the first provider. The same trait is
//! the plug-in slot for the hardware-gated work:
//!
//!   - CUDA (`burn-tch`) and WebGPU (`burn-wgpu`) — issue D1 (#62)
//!   - ONNX Runtime interop — issue D2 (#63)
//!   - burn-engine / remote inference — issue F4 (#73)
//!
//! ## Selection contract
//!
//! `XAZZ_BACKEND` selects the provider: `cpu` (default) | `cuda` | `wgpu` | `onnx`.
//! A backend that was not compiled into the binary — or an unrecognised value —
//! falls back to CPU with an explicit warning; the runtime never silently
//! switches device without telling the operator.
//!
//! ```text
//! XAZZ_BACKEND=cuda xazz run model.xzz   # needs --features cuda + a CUDA host
//! ```
//!
//! CPU is always compiled: it is both the fallback and the reference
//! implementation for the parity acceptance tests. The GPU/ONNX acceptance
//! tests are `#[ignore]`d behind their feature (see the bottom of this file) so
//! they can be executed on a host that has the hardware — the implementation
//! lives behind the same trait, so enabling the feature is the only change.

use std::sync::OnceLock;

use polars::prelude::DataFrame;
use xazz_compiler::ast::{LayerKind, TrainConfig};
use xazz_core::i18n::is_korean;

use crate::dl::TrainedModel;

/// A pluggable ML engine behind the Typed IR's `MLOp` boundary.
///
/// `train` returns the serialized [`TrainedModel`] artifact (the ndarray
/// checkpoint format). That artifact is the backend-neutral handoff consumed by
/// `predict`, so a GPU provider can train on its device and materialise the
/// portable artifact for storage.
pub trait ComputeBackend: Send + Sync {
    /// Stable identifier surfaced in logs (`cpu`, `cuda`, `wgpu`, `onnx`).
    fn id(&self) -> &'static str;

    /// `dataset |> train(model, target: .., ..)`.
    fn train(
        &self,
        df: &DataFrame,
        model_name: &str,
        layers: &[LayerKind],
        config: &TrainConfig,
    ) -> Result<TrainedModel, String>;

    /// `dataset |> predict(model_var, as: "col")`.
    fn predict(
        &self,
        trained: &TrainedModel,
        df: &DataFrame,
        as_col: Option<&str>,
    ) -> Result<DataFrame, String>;
}

/// Reference provider: Burn + burn-ndarray (pure-Rust CPU).
pub struct CpuBackend;

impl ComputeBackend for CpuBackend {
    fn id(&self) -> &'static str {
        BackendKind::Cpu.id()
    }

    fn train(
        &self,
        df: &DataFrame,
        model_name: &str,
        layers: &[LayerKind],
        config: &TrainConfig,
    ) -> Result<TrainedModel, String> {
        crate::dl::train(df, model_name, layers, config)
    }

    fn predict(
        &self,
        trained: &TrainedModel,
        df: &DataFrame,
        as_col: Option<&str>,
    ) -> Result<DataFrame, String> {
        crate::dl::predict(trained, df, as_col)
    }
}

/// The set of selectable providers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    Cpu,
    Cuda,
    Wgpu,
    Onnx,
}

impl BackendKind {
    /// Every kind the resolver understands, in selection order.
    pub const ALL: [BackendKind; 4] = [
        BackendKind::Cpu,
        BackendKind::Cuda,
        BackendKind::Wgpu,
        BackendKind::Onnx,
    ];

    /// Canonical id (also the `XAZZ_BACKEND` value).
    pub fn id(self) -> &'static str {
        match self {
            BackendKind::Cpu => "cpu",
            BackendKind::Cuda => "cuda",
            BackendKind::Wgpu => "wgpu",
            BackendKind::Onnx => "onnx",
        }
    }

    /// Parses an `XAZZ_BACKEND` value, accepting common engine aliases.
    /// Returns `None` for an unrecognised value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "cpu" | "ndarray" | "burn" | "burn-ndarray" => Some(BackendKind::Cpu),
            "cuda" | "tch" | "torch" | "libtorch" | "burn-tch" => Some(BackendKind::Cuda),
            "wgpu" | "gpu" | "webgpu" | "burn-wgpu" => Some(BackendKind::Wgpu),
            "onnx" | "onnxruntime" | "ort" => Some(BackendKind::Onnx),
            _ => None,
        }
    }

    /// Whether this provider was compiled into the current binary.
    ///
    /// CPU is unconditional (fallback + reference). The others require the
    /// matching cargo feature. Device presence is probed by the provider once
    /// its real dependency is wired; unknown/absent device also falls back to
    /// CPU through the same warning path.
    pub fn is_compiled(self) -> bool {
        match self {
            BackendKind::Cpu => true,
            BackendKind::Cuda => cfg!(feature = "cuda"),
            BackendKind::Wgpu => cfg!(feature = "wgpu"),
            BackendKind::Onnx => cfg!(feature = "onnx"),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature-gated provider scaffolds
//
// Each is the place a real provider lands: swap the body for the burn-tch /
// burn-wgpu / onnxruntime implementation. The acceptance test beside the trait
// pins the contract those providers must satisfy.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "cuda")]
mod cuda {
    use super::*;
    use xazz_core::i18n::tr;

    /// CUDA provider (`burn-tch`). Scaffold — see issue D1 (#62).
    pub struct CudaBackend;

    impl ComputeBackend for CudaBackend {
        fn id(&self) -> &'static str {
            BackendKind::Cuda.id()
        }

        fn train(
            &self,
            _df: &DataFrame,
            _model_name: &str,
            _layers: &[LayerKind],
            _config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            Err(tr(
                "CUDA backend is a scaffold: add `burn-tch` and implement it (issue #62).",
                "CUDA 백엔드는 스캐폴드입니다: `burn-tch`를 추가하고 구현하세요 (이슈 #62).",
            )
            .into())
        }

        fn predict(
            &self,
            _trained: &TrainedModel,
            _df: &DataFrame,
            _as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            Err(tr(
                "CUDA backend is a scaffold: add `burn-tch` and implement it (issue #62).",
                "CUDA 백엔드는 스캐폴드입니다: `burn-tch`를 추가하고 구현하세요 (이슈 #62).",
            )
            .into())
        }
    }
}

#[cfg(feature = "wgpu")]
mod wgpu {
    use super::*;
    use xazz_core::i18n::tr;

    /// WebGPU provider (`burn-wgpu`). Scaffold — see issue D1 (#62).
    pub struct WgpuBackend;

    impl ComputeBackend for WgpuBackend {
        fn id(&self) -> &'static str {
            BackendKind::Wgpu.id()
        }

        fn train(
            &self,
            _df: &DataFrame,
            _model_name: &str,
            _layers: &[LayerKind],
            _config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            Err(tr(
                "WGPU backend is a scaffold: add `burn-wgpu` and implement it (issue #62).",
                "WGPU 백엔드는 스캐폴드입니다: `burn-wgpu`를 추가하고 구현하세요 (이슈 #62).",
            )
            .into())
        }

        fn predict(
            &self,
            _trained: &TrainedModel,
            _df: &DataFrame,
            _as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            Err(tr(
                "WGPU backend is a scaffold: add `burn-wgpu` and implement it (issue #62).",
                "WGPU 백엔드는 스캐폴드입니다: `burn-wgpu`를 추가하고 구현하세요 (이슈 #62).",
            )
            .into())
        }
    }
}

#[cfg(feature = "onnx")]
mod onnx {
    use super::*;
    use xazz_core::i18n::tr;

    /// ONNX Runtime provider. Scaffold — see issue D2 (#63).
    pub struct OnnxBackend;

    impl ComputeBackend for OnnxBackend {
        fn id(&self) -> &'static str {
            BackendKind::Onnx.id()
        }

        fn train(
            &self,
            _df: &DataFrame,
            _model_name: &str,
            _layers: &[LayerKind],
            _config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            Err(tr(
                "ONNX backend is a scaffold: add `onnxruntime` and implement it (issue #63).",
                "ONNX 백엔드는 스캐폴드입니다: `onnxruntime`을 추가하고 구현하세요 (이슈 #63).",
            )
            .into())
        }

        fn predict(
            &self,
            _trained: &TrainedModel,
            _df: &DataFrame,
            _as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            Err(tr(
                "ONNX backend is a scaffold: add `onnxruntime` and implement it (issue #63).",
                "ONNX 백엔드는 스캐폴드입니다: `onnxruntime`을 추가하고 구현하세요 (이슈 #63).",
            )
            .into())
        }
    }
}

/// Instantiates a provider that the resolver has already checked is compiled.
fn build(kind: BackendKind) -> Box<dyn ComputeBackend> {
    match kind {
        BackendKind::Cpu => Box::new(CpuBackend),
        #[cfg(feature = "cuda")]
        BackendKind::Cuda => Box::new(cuda::CudaBackend),
        #[cfg(feature = "wgpu")]
        BackendKind::Wgpu => Box::new(wgpu::WgpuBackend),
        #[cfg(feature = "onnx")]
        BackendKind::Onnx => Box::new(onnx::OnnxBackend),
        #[allow(unreachable_patterns)]
        _ => Box::new(CpuBackend),
    }
}

fn fallback_warning(kind: BackendKind) -> String {
    if is_korean() {
        format!(
            "XAZZ_BACKEND={} 백엔드가 이 바이너리에 포함되지 않았습니다. CPU로 폴백합니다.",
            kind.id()
        )
    } else {
        format!(
            "XAZZ_BACKEND={} is not compiled into this binary; falling back to CPU.",
            kind.id()
        )
    }
}

fn unknown_warning(raw: &str) -> String {
    if is_korean() {
        format!(
            "알 수 없는 XAZZ_BACKEND='{raw}' 입니다. 사용 가능: {}. CPU로 폴백합니다.",
            BackendKind::ALL
                .iter()
                .map(|k| k.id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    } else {
        format!(
            "unknown XAZZ_BACKEND='{raw}'. Available: {}. Falling back to CPU.",
            BackendKind::ALL
                .iter()
                .map(|k| k.id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Resolves a requested backend value into a provider.
///
/// Pure (no environment access) so it is directly testable. Returns the provider
/// plus an optional warning describing a fallback; `None` means the request was
/// honoured (or absent → CPU).
pub fn resolve(requested: Option<&str>) -> (Box<dyn ComputeBackend>, Option<String>) {
    match requested {
        None => (build(BackendKind::Cpu), None),
        Some(raw) => match BackendKind::parse(raw) {
            Some(kind) if kind.is_compiled() => (build(kind), None),
            Some(kind) => (build(BackendKind::Cpu), Some(fallback_warning(kind))),
            None => (build(BackendKind::Cpu), Some(unknown_warning(raw))),
        },
    }
}

/// The process-wide active provider, chosen once from `XAZZ_BACKEND`.
///
/// On first use it logs the fallback warning (if any) or the selected non-CPU
/// backend to stderr. Subsequent calls reuse the resolved provider.
pub fn active() -> &'static dyn ComputeBackend {
    static ACTIVE: OnceLock<Box<dyn ComputeBackend>> = OnceLock::new();
    ACTIVE
        .get_or_init(|| {
            let requested = std::env::var("XAZZ_BACKEND").ok();
            let (backend, warning) = resolve(requested.as_deref());
            match warning {
                Some(msg) => eprintln!("[xazz] {msg}"),
                None if backend.id() != BackendKind::Cpu.id() => {
                    eprintln!("[xazz] ML backend: {}", backend.id())
                }
                None => {}
            }
            backend
        })
        .as_ref()
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Tiny separable regression set shared by the CPU round-trip test and the
    /// hardware acceptance tests.
    #[cfg(any(test, feature = "cuda", feature = "wgpu", feature = "onnx"))]
    pub(super) fn tiny_dataset() -> (DataFrame, Vec<LayerKind>, TrainConfig) {
        use polars::prelude::*;
        let df = df!(
            "x1" => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0],
            "x2" => [1.0f64, 1.0, 0.0, 0.0, 1.0, 1.0],
            "y"  => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0],
        )
        .expect("tiny dataset");
        let layers = vec![LayerKind::Dense(4), LayerKind::ReLU, LayerKind::Dense(1)];
        let config = TrainConfig {
            target: "y".to_string(),
            epochs: 2,
            learning_rate: 0.05,
            batch_size: Some(3),
            validation_split: None,
            early_stopping_patience: None,
        };
        (df, layers, config)
    }

    fn cleanup(checkpoint_path: &str) {
        let _ = std::fs::remove_file(checkpoint_path);
        let _ = std::fs::remove_dir("checkpoints");
    }

    #[test]
    fn parse_accepts_aliases_and_rejects_garbage() {
        assert_eq!(BackendKind::parse("cpu"), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse(""), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse(" burn-ndarray "), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse("CUDA"), Some(BackendKind::Cuda));
        assert_eq!(BackendKind::parse("tch"), Some(BackendKind::Cuda));
        assert_eq!(BackendKind::parse("webgpu"), Some(BackendKind::Wgpu));
        assert_eq!(BackendKind::parse("ort"), Some(BackendKind::Onnx));
        assert_eq!(BackendKind::parse("quantum"), None);
    }

    #[test]
    fn resolve_defaults_to_cpu_without_warning() {
        let (backend, warning) = resolve(None);
        assert_eq!(backend.id(), "cpu");
        assert!(warning.is_none());
    }

    #[test]
    fn resolve_unknown_value_falls_back_with_warning() {
        let (backend, warning) = resolve(Some("quantum"));
        assert_eq!(backend.id(), "cpu");
        assert!(warning.unwrap().contains("quantum"));
    }

    #[test]
    fn resolve_uncompiled_backend_falls_back_with_warning() {
        let (backend, warning) = resolve(Some("cuda"));
        if cfg!(feature = "cuda") {
            assert_eq!(backend.id(), "cuda");
            assert!(warning.is_none());
        } else {
            assert_eq!(backend.id(), "cpu");
            assert!(warning.unwrap().contains("cuda"));
        }
    }

    #[test]
    fn cpu_backend_trains_and_predicts_through_the_trait() {
        let (df, layers, config) = tiny_dataset();
        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let trained = backend
            .train(&df, "backend_unit_mlp", &layers, &config)
            .expect("cpu train");
        assert_eq!(trained.report.input_dim, 2);
        assert_eq!(trained.report.output_dim, 1);

        let out = backend
            .predict(&trained, &df, Some("pred"))
            .expect("cpu predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Hardware acceptance tests (issue D1 #62, D2 #63)
//
// These encode the contract the compiled provider must satisfy but require a
// device/SDK absent in CI. They are `#[ignore]`d; on a matching host run:
//
//   cargo test -p xazz-exec --features cuda   -- --ignored
//   cargo test -p xazz-exec --features wgpu   -- --ignored
//   cargo test -p xazz-exec --features onnx   -- --ignored
//
// Until the provider dependency is wired they fail with the scaffold error,
// which is why they are excluded from the default run.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(test, any(feature = "cuda", feature = "wgpu", feature = "onnx")))]
mod acceptance {
    use super::ComputeBackend;
    use super::tests::tiny_dataset;

    /// Same data + hyperparameters on CPU and `requested`; reported training
    /// loss must agree within tolerance, and prediction shape must match.
    #[cfg(any(feature = "cuda", feature = "wgpu", feature = "onnx"))]
    fn assert_parity(requested: &str) {
        let (df, layers, config) = tiny_dataset();
        let cpu = super::CpuBackend
            .train(&df, "acc_cpu", &layers, &config)
            .expect("cpu reference train");
        let (backend, warning) = super::resolve(Some(requested));
        assert!(warning.is_none(), "{requested} should be compiled");
        assert_eq!(backend.id(), requested);

        let gpu = backend
            .train(&df, "acc_gpu", &layers, &config)
            .expect("backend train");
        let diff = (cpu.report.final_train_loss - gpu.report.final_train_loss).abs();
        assert!(
            diff < 1e-3,
            "{requested} train loss diverged from CPU by {diff} (cpu={}, gpu={})",
            cpu.report.final_train_loss,
            gpu.report.final_train_loss
        );

        let out = backend
            .predict(&gpu, &df, Some("pred"))
            .expect("backend predict");
        assert_eq!(out.height(), df.height());

        let _ = std::fs::remove_file(&cpu.report.checkpoint_path);
        let _ = std::fs::remove_file(&gpu.report.checkpoint_path);
        let _ = std::fs::remove_dir("checkpoints");
    }

    #[cfg(feature = "cuda")]
    #[test]
    #[ignore = "requires a CUDA GPU + burn-tch: `cargo test -p xazz-exec --features cuda -- --ignored` (issue #62)"]
    fn cuda_matches_cpu_losses() {
        assert_parity("cuda");
    }

    #[cfg(feature = "wgpu")]
    #[test]
    #[ignore = "requires a WebGPU device + burn-wgpu: `cargo test -p xazz-exec --features wgpu -- --ignored` (issue #62)"]
    fn wgpu_matches_cpu_losses() {
        assert_parity("wgpu");
    }

    #[cfg(feature = "onnx")]
    #[test]
    #[ignore = "requires onnxruntime + an exported model: `cargo test -p xazz-exec --features onnx -- --ignored` (issue #63)"]
    fn onnx_matches_cpu_losses() {
        assert_parity("onnx");
    }
}
