//! xazz-exec/src/backend.rs — ML compute-backend abstraction at the MLOp boundary.
//!
//! The Typed IR (`xazz_core::ir::MLOp`) is backend-independent. This module is the
//! single dispatch point between that IR and a concrete ML engine: `runtime`
//! calls [`active()`]`.train(..)` / `.predict(..)` instead of a concrete engine.
//!
//! Burn + burn-ndarray (pure-Rust CPU) is the first provider. The same trait is
//! the plug-in slot for the hardware-gated work:
//!
//!   - CUDA (`burn-cuda`) and WebGPU (`burn-wgpu`) — issue D1 (#62)
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
use xazz_compiler::ast::{LayerKind, SweepMetric, TrainConfig};
use xazz_core::i18n::{is_korean, tr};

use crate::dl::{CheckpointManifest, SweepCombo, SweepReport, TrainedModel};

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

    /// Like [`Self::train`], but does not persist the checkpoint.
    ///
    /// The sweep grid evaluates every combination through this method and only
    /// the winner is materialised/saved, so a GPU provider avoids a checkpoint
    /// save (and its device→CPU cost) per combination (issue D1/D3). Defaults to
    /// [`Self::train`] for providers that do not separate the two.
    fn train_unpersisted(
        &self,
        df: &DataFrame,
        model_name: &str,
        layers: &[LayerKind],
        config: &TrainConfig,
    ) -> Result<TrainedModel, String> {
        self.train(df, model_name, layers, config)
    }

    /// `dataset |> train(model, ..)` over a hyperparameter grid (D3).
    ///
    /// The default implementation evaluates every combination via [`Self::train`]
    /// and returns the best model plus a per-combination report. Selection uses
    /// validation loss when a `validation_split` is configured, else training loss.
    fn sweep(
        &self,
        df: &DataFrame,
        model_name: &str,
        layers: &[LayerKind],
        config: &TrainConfig,
    ) -> Result<(TrainedModel, SweepReport), String> {
        let combos = config.expand_sweep();
        let metric: SweepMetric = config.sweep_metric;
        let mut best: Option<(usize, TrainedModel)> = None;
        let mut entries: Vec<SweepCombo> = Vec::with_capacity(combos.len());

        for (index, combo) in combos.iter().enumerate() {
            // Unpersisted: only the winner below is materialised/saved, so a GPU
            // provider does not write a checkpoint per combination (issue D1/D3).
            let trained = self.train_unpersisted(df, model_name, layers, combo)?;
            let report = &trained.report;
            let entry = SweepCombo {
                epochs: report.epochs,
                batch_size: report.batch_size,
                learning_rate: report.learning_rate,
                final_train_loss: report.final_train_loss,
                final_val_loss: report.final_val_loss,
                stopped_early: report.stopped_early,
                best_epoch: report.best_epoch,
                train_mae: report.final_train_mae,
                val_mae: report.final_val_mae,
                train_r2: report.final_train_r2,
                val_r2: report.final_val_r2,
                selected: false,
            };
            let is_better = match &best {
                None => true,
                Some((best_index, _)) => {
                    SweepReport::score(&entry, metric)
                        < SweepReport::score(&entries[*best_index], metric)
                }
            };
            if is_better {
                best = Some((index, trained));
            }
            entries.push(entry);
        }

        let (best_index, best_model) = best.ok_or_else(|| {
            tr(
                "hyperparameter sweep produced no combinations.",
                "하이퍼파라미터 스윕 조합이 없습니다.",
            )
            .to_string()
        })?;

        // Every combination re-trains onto the same checkpoint path, so the file
        // left on disk is the last combination's. Re-save the winner so the
        // checkpoint matches the model returned for downstream predict().
        let base = best_model.report.checkpoint_path.trim_end_matches(".json");
        crate::dl::save_checkpoint(&best_model.model, base)?;
        // Keep the versioned sidecar manifest in sync with the winning weights.
        crate::dl::save_checkpoint_manifest(
            &best_model.report.checkpoint_path,
            &CheckpointManifest::from_trained(&best_model),
        )?;

        // The winner is always the best-by-metric combination, so a `top` filter
        // (best-by-metric) can never drop it. Order the retained set afterwards.
        let total_combos = entries.len();
        let mut indexed: Vec<(usize, SweepCombo)> = entries.into_iter().enumerate().collect();
        if let Some(top) = config.sweep_top {
            let top = top.max(1);
            if top < indexed.len() {
                indexed.sort_by(|(_, a), (_, b)| {
                    SweepReport::score(a, metric)
                        .partial_cmp(&SweepReport::score(b, metric))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                indexed.truncate(top);
            }
        }
        let sort = config.sweep_sort;
        let tiebreak = config.sweep_tiebreak.clone();
        indexed.sort_by(|(_, a), (_, b)| SweepReport::compare(a, b, sort, metric, &tiebreak));

        let report_best = indexed
            .iter()
            .position(|(orig, _)| *orig == best_index)
            .ok_or_else(|| {
                tr(
                    "sweep winner missing from the reported combinations.",
                    "스윕 최적 조합이 리포트에서 누락되었습니다.",
                )
                .to_string()
            })?;
        let mut combos: Vec<SweepCombo> = indexed.into_iter().map(|(_, c)| c).collect();
        combos[report_best].selected = true;

        let report = SweepReport {
            model_name: model_name.to_string(),
            target: config.target.clone(),
            combos,
            best_index: report_best,
            metric,
            sort,
            tiebreak,
            top: config.sweep_top,
            total_combos,
        };
        Ok((best_model, report))
    }
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

    fn train_unpersisted(
        &self,
        df: &DataFrame,
        model_name: &str,
        layers: &[LayerKind],
        config: &TrainConfig,
    ) -> Result<TrainedModel, String> {
        crate::dl::train_unpersisted(df, model_name, layers, config)
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
            "cuda" | "nvidia" | "burn-cuda" => Some(BackendKind::Cuda),
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
// WebGPU device selection (issue D1 #75)
//
// `XAZZ_WGPU_DEVICE` picks which adapter burn-wgpu uses, so a machine with both
// a discrete and an integrated GPU (e.g. RTX 4070 + Intel Arc) can be pinned to
// one without changing code. Parsed into a backend-independent spec so the
// grammar is unit-testable without compiling burn-wgpu.
// ─────────────────────────────────────────────────────────────────────────────

/// Backend-independent WebGPU device selector (mapped to `WgpuDevice` when the
/// `wgpu` feature is compiled in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WgpuDeviceSpec {
    /// cubecl's `DefaultDevice` (highest-power adapter; also honours
    /// `CUBECL_WGPU_DEFAULT_DEVICE`).
    Default,
    /// The software/CPU adapter.
    Cpu,
    /// The `n`-th discrete GPU (0-based).
    Discrete(usize),
    /// The `n`-th integrated GPU (0-based).
    Integrated(usize),
    /// The `n`-th virtual GPU (0-based).
    Virtual(usize),
}

/// Parses an `XAZZ_WGPU_DEVICE` value.
///
/// Accepted forms (case-insensitive): `default`/`auto`/empty, `cpu`,
/// `dgpu[:N]`/`discrete[:N]`, `igpu[:N]`/`integrated[:N]`,
/// `vgpu[:N]`/`virtual[:N]`, and the `DiscreteGpu(N)` spelling. `N` defaults to
/// 0. Returns `None` for an unrecognised value.
pub fn parse_wgpu_device(raw: &str) -> Option<WgpuDeviceSpec> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" | "default" | "auto" => return Some(WgpuDeviceSpec::Default),
        "cpu" => return Some(WgpuDeviceSpec::Cpu),
        _ => {}
    }

    // Split an optional index suffix: `name:2`, `name(2)`, or bare `name`.
    let (name, index) = match lower.split_once(':').or_else(|| lower.split_once('(')) {
        Some((name, rest)) => {
            let digits = rest.trim_end_matches(')').trim();
            (name, digits.parse::<usize>().ok()?)
        }
        None => (lower.as_str(), 0),
    };

    match name {
        "dgpu" | "discrete" | "discretegpu" => Some(WgpuDeviceSpec::Discrete(index)),
        "igpu" | "integrated" | "integratedgpu" => Some(WgpuDeviceSpec::Integrated(index)),
        "vgpu" | "virtual" | "virtualgpu" => Some(WgpuDeviceSpec::Virtual(index)),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Unified accelerator selector (issue D1/D2)
//
// `XAZZ_DEVICE` covers the WebGPU adapter, the CUDA device index, and the ONNX
// execution provider in one grammar, so the three GPU providers share a single
// interface. The per-provider variables (`XAZZ_WGPU_DEVICE`, `XAZZ_CUDA_DEVICE`,
// `XAZZ_ORT_EP`) remain as fallbacks. Parsed into a backend-independent spec so
// the grammar is unit-testable without compiling any GPU backend.
// ─────────────────────────────────────────────────────────────────────────────

/// A unified accelerator selector parsed from `XAZZ_DEVICE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceSpec {
    /// Highest-power/first adapter (cubecl default).
    Auto,
    /// Software/CPU adapter or ONNX CPU execution provider.
    Cpu,
    /// The `n`-th discrete GPU (WebGPU adapter).
    Discrete(usize),
    /// The `n`-th integrated GPU (WebGPU adapter).
    Integrated(usize),
    /// The `n`-th virtual GPU (WebGPU adapter).
    Virtual(usize),
    /// CUDA device `n` (burn-cuda device / ONNX CUDA EP).
    Cuda(usize),
    /// TensorRT device `n` (ONNX EP).
    TensorRt(usize),
    /// DirectML device `n` (ONNX EP).
    DirectMl(usize),
    /// CoreML (ONNX EP).
    CoreMl,
}

/// Parses an `XAZZ_DEVICE` value.
///
/// Accepted forms (case-insensitive): `auto`/`default`/empty, `cpu`, `coreml`,
/// `dgpu[:N]`/`discrete[:N]`, `igpu[:N]`/`integrated[:N]`,
/// `vgpu[:N]`/`virtual[:N]`, `cuda[:N]`, `tensorrt[:N]`/`trt[:N]`,
/// `directml[:N]`/`dml[:N]`, and the `DiscreteGpu(N)` spelling. `N` defaults to
/// 0. Returns `None` for an unrecognised value.
pub fn parse_device_spec(raw: &str) -> Option<DeviceSpec> {
    let lower = raw.trim().to_ascii_lowercase();
    match lower.as_str() {
        "" | "auto" | "default" => return Some(DeviceSpec::Auto),
        "cpu" => return Some(DeviceSpec::Cpu),
        "coreml" => return Some(DeviceSpec::CoreMl),
        _ => {}
    }

    // Split an optional index suffix: `name:2`, `name(2)`, or bare `name`.
    let (name, index) = match lower.split_once(':').or_else(|| lower.split_once('(')) {
        Some((name, rest)) => {
            let digits = rest.trim_end_matches(')').trim();
            (name, digits.parse::<usize>().ok()?)
        }
        None => (lower.as_str(), 0),
    };

    match name {
        "dgpu" | "discrete" | "discretegpu" => Some(DeviceSpec::Discrete(index)),
        "igpu" | "integrated" | "integratedgpu" => Some(DeviceSpec::Integrated(index)),
        "vgpu" | "virtual" | "virtualgpu" => Some(DeviceSpec::Virtual(index)),
        "cuda" => Some(DeviceSpec::Cuda(index)),
        "tensorrt" | "trt" => Some(DeviceSpec::TensorRt(index)),
        "directml" | "dml" => Some(DeviceSpec::DirectMl(index)),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ONNX execution-provider selection (issue D2 #63, ONNX GPU)
//
// `XAZZ_ORT_EP` picks the ONNX Runtime execution providers. Parsed into a
// backend-independent spec so the grammar is unit-testable without compiling
// `ort`/ONNX Runtime.
// ─────────────────────────────────────────────────────────────────────────────

/// An ONNX Runtime execution provider this build understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrtEpKind {
    Cpu,
    Cuda,
    TensorRt,
    DirectML,
    CoreML,
}

impl OrtEpKind {
    /// Canonical `XAZZ_ORT_EP` token.
    pub fn id(self) -> &'static str {
        match self {
            OrtEpKind::Cpu => "cpu",
            OrtEpKind::Cuda => "cuda",
            OrtEpKind::TensorRt => "tensorrt",
            OrtEpKind::DirectML => "directml",
            OrtEpKind::CoreML => "coreml",
        }
    }
}

/// Parsed `XAZZ_ORT_EP` value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrtEpSpec {
    /// Register every GPU EP compiled into this build (falling back to CPU).
    Auto,
    /// Register exactly these EPs, in order (fail-closed if one is unavailable).
    Explicit(Vec<OrtEpKind>),
}

/// Parses an `XAZZ_ORT_EP` value.
///
/// Empty/`auto` → [`OrtEpSpec::Auto`]. Otherwise a comma-separated list of
/// `cpu|cuda|tensorrt|directml|coreml` (case-insensitive, order preserved).
/// Returns an error for an empty or unrecognised list.
pub fn parse_ort_ep_spec(raw: &str) -> Result<OrtEpSpec, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return Ok(OrtEpSpec::Auto);
    }
    let mut kinds = Vec::new();
    for token in trimmed.split(',') {
        let token = token.trim().to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        let kind = match token.as_str() {
            "cpu" => OrtEpKind::Cpu,
            "cuda" => OrtEpKind::Cuda,
            "tensorrt" | "trt" => OrtEpKind::TensorRt,
            "directml" | "dml" => OrtEpKind::DirectML,
            "coreml" => OrtEpKind::CoreML,
            _ => {
                return Err(format!(
                    "unknown XAZZ_ORT_EP entry '{token}' (use auto|cpu|cuda|tensorrt|directml|coreml)"
                ));
            }
        };
        kinds.push(kind);
    }
    if kinds.is_empty() {
        return Err("XAZZ_ORT_EP is empty".to_string());
    }
    Ok(OrtEpSpec::Explicit(kinds))
}

// ─────────────────────────────────────────────────────────────────────────────
// Feature-gated providers
//
// `cuda` (burn-cuda), `wgpu` (burn-wgpu), and `onnx` (ONNX Runtime) are all
// implemented. Each provider's acceptance test beside the trait pins the
// contract it must satisfy.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(feature = "cuda")]
mod cuda {
    use super::*;
    use crate::dl::Mlp;
    use burn::tensor::Tensor;
    use burn_cuda::{Cuda, CudaDevice};
    use std::sync::Mutex;

    /// CUDA provider (`burn-cuda`, native CubeCL). Trains and predicts on an
    /// NVIDIA GPU; the portable checkpoint round-trips back to CPU for storage.
    ///
    /// Uses the same pure-Rust CubeCL stack as `burn-wgpu` (no LibTorch/system
    /// SDK), so it tracks the upstream Burn 0.22 CUDA direction. The device is
    /// resolved once at construction (fail-closed when absent), and loaded
    /// inference modules are kept in a small LRU keyed by checkpoint identity,
    /// so repeated `predict` calls skip the JSON reload (issue D1).
    pub struct CudaBackend {
        device: CudaDevice,
        cache: Mutex<crate::dl::LruCache<crate::dl::ArtifactKey, Mlp<Cuda<f32>>>>,
    }

    /// Resolves the CUDA device index.
    ///
    /// The unified `XAZZ_DEVICE` selector takes precedence (`auto`/`cuda[:N]`),
    /// falling back to the legacy `XAZZ_CUDA_DEVICE` (default `0`).
    fn device_index() -> Result<usize, String> {
        if let Ok(raw) = std::env::var("XAZZ_DEVICE") {
            let spec = parse_device_spec(&raw).ok_or_else(|| {
                tr(
                    "unknown XAZZ_DEVICE value; CUDA accepts auto|cuda[:N]",
                    "알 수 없는 XAZZ_DEVICE 값입니다. CUDA는 auto|cuda[:N] 을 받습니다",
                )
                .to_string()
            })?;
            return match spec {
                DeviceSpec::Auto => Ok(0),
                DeviceSpec::Cuda(i) => Ok(i),
                _ => Err(tr(
                    "XAZZ_DEVICE is not a CUDA selector; use auto|cuda[:N]",
                    "XAZZ_DEVICE가 CUDA 선택자가 아닙니다. auto|cuda[:N] 을 사용하세요",
                )
                .to_string()),
            };
        }
        Ok(std::env::var("XAZZ_CUDA_DEVICE")
            .ok()
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0))
    }

    /// Probes the device with a minimal op, converting CubeCL's CUDA
    /// initialisation failure (missing driver/device, bad index) into a
    /// fallback error instead of a later panic.
    ///
    /// CubeCL has no non-panicking device probe, so this catches the panic and
    /// silences the default hook while doing so. It runs once during provider
    /// construction (before any other CUDA work), so the temporary global hook
    /// swap cannot race with concurrent GPU use.
    fn probe_device(device: &CudaDevice) -> Result<(), String> {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let t = Tensor::<Cuda<f32>, 1>::zeros([1], device);
            let _ = t.into_data();
        }));
        std::panic::set_hook(prev);
        result.map_err(|_| {
            tr(
                "CUDA backend selected but no usable CUDA device/driver was found. \
                 Check the NVIDIA driver and XAZZ_DEVICE/XAZZ_CUDA_DEVICE, or unset XAZZ_BACKEND.",
                "CUDA 백엔드를 선택했지만 사용 가능한 CUDA 장치/드라이버를 찾지 못했습니다. \
                 NVIDIA 드라이버와 XAZZ_DEVICE/XAZZ_CUDA_DEVICE를 확인하거나 XAZZ_BACKEND를 해제하세요.",
            )
            .to_string()
        })
    }

    impl CudaBackend {
        /// Probes the CUDA device once, so an absent device becomes a CPU
        /// fallback at selection time instead of a per-call failure.
        pub fn new() -> Result<Self, String> {
            let device = CudaDevice::new(device_index()?);
            probe_device(&device)?;
            Ok(Self {
                device,
                cache: Mutex::new(crate::dl::LruCache::new(crate::dl::infer_cache_slots())),
            })
        }

        /// Predicts using the cached module, loading it only when the checkpoint
        /// identity changes (issue D1: avoid the per-call disk reload).
        fn predict_cached(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            let key = crate::dl::artifact_key(&trained.report.checkpoint_path);
            let mut guard = self
                .cache
                .lock()
                .map_err(|_| "CUDA inference cache poisoned".to_string())?;
            let model = guard.get_or_insert_with(key, || {
                crate::dl::load_inference_model::<Cuda<f32>>(trained, &self.device)
            })?;
            crate::dl::predict_with_model::<Cuda<f32>>(trained, model, &self.device, df, as_col)
        }
    }

    impl ComputeBackend for CudaBackend {
        fn id(&self) -> &'static str {
            BackendKind::Cuda.id()
        }

        fn train(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            crate::dl::train_on_device::<Cuda<f32>>(df, model_name, layers, config, &self.device)
        }

        fn train_unpersisted(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            crate::dl::train_on_device_unpersisted::<Cuda<f32>>(
                df,
                model_name,
                layers,
                config,
                &self.device,
            )
        }

        fn predict(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            self.predict_cached(trained, df, as_col)
        }
    }
}

#[cfg(feature = "wgpu")]
mod wgpu {
    use super::*;
    use crate::dl::Mlp;
    use burn::tensor::Tensor;
    use burn_wgpu::{Wgpu, WgpuDevice};
    use std::sync::Mutex;

    /// WebGPU provider (`burn-wgpu`). Trains and predicts on a Vulkan/Metal/DX12
    /// device; the portable checkpoint round-trips back to CPU for storage.
    ///
    /// The device is selected from `XAZZ_DEVICE`/`XAZZ_WGPU_DEVICE` and probed
    /// once at construction (issue D1 #75: fall back to CPU when no adapter
    /// exists), and loaded inference modules are kept in a small LRU keyed by
    /// checkpoint identity (issue D1).
    pub struct WgpuBackend {
        device: WgpuDevice,
        cache: Mutex<crate::dl::LruCache<crate::dl::ArtifactKey, Mlp<Wgpu<f32>>>>,
    }

    /// Maps the parsed spec to a cubecl `WgpuDevice`.
    fn to_device(spec: WgpuDeviceSpec) -> WgpuDevice {
        match spec {
            WgpuDeviceSpec::Default => WgpuDevice::DefaultDevice,
            WgpuDeviceSpec::Cpu => WgpuDevice::Cpu,
            WgpuDeviceSpec::Discrete(i) => WgpuDevice::DiscreteGpu(i),
            WgpuDeviceSpec::Integrated(i) => WgpuDevice::IntegratedGpu(i),
            WgpuDeviceSpec::Virtual(i) => WgpuDevice::VirtualGpu(i),
        }
    }

    /// Resolves the adapter from the unified `XAZZ_DEVICE` selector first, then
    /// the legacy `XAZZ_WGPU_DEVICE` (default: cubecl's `DefaultDevice`). An
    /// unrecognised value fails closed rather than silently picking an adapter.
    fn device_from_env() -> Result<WgpuDevice, String> {
        if let Ok(raw) = std::env::var("XAZZ_DEVICE") {
            let spec = parse_device_spec(&raw).ok_or_else(|| {
                tr(
                    "unknown XAZZ_DEVICE value; wgpu accepts auto|cpu|dgpu[:N]|igpu[:N]|vgpu[:N]",
                    "알 수 없는 XAZZ_DEVICE 값입니다. wgpu는 auto|cpu|dgpu[:N]|igpu[:N]|vgpu[:N] 을 받습니다",
                )
                .to_string()
            })?;
            return match spec {
                DeviceSpec::Auto => Ok(WgpuDevice::DefaultDevice),
                DeviceSpec::Cpu => Ok(WgpuDevice::Cpu),
                DeviceSpec::Discrete(i) => Ok(WgpuDevice::DiscreteGpu(i)),
                DeviceSpec::Integrated(i) => Ok(WgpuDevice::IntegratedGpu(i)),
                DeviceSpec::Virtual(i) => Ok(WgpuDevice::VirtualGpu(i)),
                _ => Err(tr(
                    "XAZZ_DEVICE is not a WebGPU selector; use auto|cpu|dgpu[:N]|igpu[:N]|vgpu[:N]",
                    "XAZZ_DEVICE가 WebGPU 선택자가 아닙니다. auto|cpu|dgpu[:N]|igpu[:N]|vgpu[:N] 을 사용하세요",
                )
                .to_string()),
            };
        }
        match std::env::var("XAZZ_WGPU_DEVICE") {
            Ok(raw) => parse_wgpu_device(&raw).map(to_device).ok_or_else(|| {
                tr(
                    "unknown XAZZ_WGPU_DEVICE value; use default|cpu|dgpu[:N]|igpu[:N]|vgpu[:N]",
                    "알 수 없는 XAZZ_WGPU_DEVICE 값입니다. default|cpu|dgpu[:N]|igpu[:N]|vgpu[:N] 중 하나를 사용하세요",
                )
                .to_string()
            }),
            Err(_) => Ok(WgpuDevice::DefaultDevice),
        }
    }

    /// Probes the device with a minimal op, converting cubecl's adapter-selection
    /// panic into a fallback error.
    ///
    /// cubecl has no non-panicking adapter probe, so this catches the panic and
    /// silences the default hook while doing so. It runs once during provider
    /// construction (before any other wgpu work), so the temporary global hook
    /// swap cannot race with concurrent GPU use.
    fn probe_device(device: &WgpuDevice) -> Result<(), String> {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let t = Tensor::<Wgpu<f32>, 1>::zeros([1], device);
            let _ = t.into_data();
        }));
        std::panic::set_hook(prev);
        result.map_err(|_| {
            tr(
                "WebGPU backend selected but no compatible GPU adapter was found. \
                 Set XAZZ_WGPU_DEVICE to a present adapter or unset XAZZ_BACKEND.",
                "WebGPU 백엔드를 선택했지만 호환되는 GPU 어댑터를 찾지 못했습니다. \
                 존재하는 어댑터로 XAZZ_WGPU_DEVICE를 설정하거나 XAZZ_BACKEND를 해제하세요.",
            )
            .to_string()
        })
    }

    impl WgpuBackend {
        /// Resolves and probes the device once, so an absent adapter becomes a
        /// CPU fallback at selection time.
        pub fn new() -> Result<Self, String> {
            let device = device_from_env()?;
            probe_device(&device)?;
            Ok(Self {
                device,
                cache: Mutex::new(crate::dl::LruCache::new(crate::dl::infer_cache_slots())),
            })
        }

        /// Predicts using the cached module, loading it only when the checkpoint
        /// identity changes (issue D1: avoid the per-call disk reload).
        fn predict_cached(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            let key = crate::dl::artifact_key(&trained.report.checkpoint_path);
            let mut guard = self
                .cache
                .lock()
                .map_err(|_| "wgpu inference cache poisoned".to_string())?;
            let model = guard.get_or_insert_with(key, || {
                crate::dl::load_inference_model::<Wgpu<f32>>(trained, &self.device)
            })?;
            crate::dl::predict_with_model::<Wgpu<f32>>(trained, model, &self.device, df, as_col)
        }
    }

    impl ComputeBackend for WgpuBackend {
        fn id(&self) -> &'static str {
            BackendKind::Wgpu.id()
        }

        fn train(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            crate::dl::train_on_device::<Wgpu<f32>>(df, model_name, layers, config, &self.device)
        }

        fn train_unpersisted(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            crate::dl::train_on_device_unpersisted::<Wgpu<f32>>(
                df,
                model_name,
                layers,
                config,
                &self.device,
            )
        }

        fn predict(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            self.predict_cached(trained, df, as_col)
        }
    }
}

#[cfg(feature = "onnx")]
mod onnx {
    use super::*;
    use std::sync::Mutex;

    /// ONNX Runtime provider (D2 #63). ONNX is an interop/inference target, so
    /// `train` runs the CPU reference training and exports the result to a
    /// sibling `.onnx` artifact; `predict` evaluates that graph via ONNX Runtime.
    ///
    /// The exported artifact is reused when it is newer than the checkpoint, and
    /// loaded `Session`s are kept in a small LRU keyed by artifact identity, so
    /// repeated `predict` calls neither re-export nor rebuild the session
    /// (issue D2).
    pub struct OnnxBackend {
        cache: Mutex<crate::dl::LruCache<crate::dl::ArtifactKey, ort::session::Session>>,
    }

    /// `checkpoints/<name>.json` → `checkpoints/<name>.onnx`.
    fn artifact_path(checkpoint_path: &str) -> String {
        format!("{}.onnx", checkpoint_path.trim_end_matches(".json"))
    }

    impl OnnxBackend {
        /// No device probe: ONNX Runtime loads lazily and its execution provider
        /// is configured at the environment level.
        pub fn new() -> Self {
            Self {
                cache: Mutex::new(crate::dl::LruCache::new(crate::dl::infer_cache_slots())),
            }
        }

        /// Predicts through a cached `Session`, rebuilding it only when the
        /// exported `.onnx` identity changes (issue D2).
        fn predict_cached(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            let path = artifact_path(&trained.report.checkpoint_path);
            crate::dl::onnx_export::ensure_export(trained, &path)?;
            let key = crate::dl::artifact_key(&path);

            let mut guard = self
                .cache
                .lock()
                .map_err(|_| "ONNX session cache poisoned".to_string())?;
            let session = guard.get_or_insert_with(key, || {
                crate::dl::onnx_export::init_runtime();
                crate::dl::onnx_export::load_session(&path)
            })?;
            crate::dl::onnx_export::predict_with_session(trained, session, df, as_col)
        }
    }

    impl ComputeBackend for OnnxBackend {
        fn id(&self) -> &'static str {
            BackendKind::Onnx.id()
        }

        fn train(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            let trained = crate::dl::train(df, model_name, layers, config)?;
            crate::dl::onnx_export::export(
                &trained,
                &artifact_path(&trained.report.checkpoint_path),
            )?;
            Ok(trained)
        }

        fn train_unpersisted(
            &self,
            df: &DataFrame,
            model_name: &str,
            layers: &[LayerKind],
            config: &TrainConfig,
        ) -> Result<TrainedModel, String> {
            // No export here: the sweep grid only exports the winning artifact,
            // lazily on the first predict via `ensure_export` (issue D2/D3).
            crate::dl::train_unpersisted(df, model_name, layers, config)
        }

        fn predict(
            &self,
            trained: &TrainedModel,
            df: &DataFrame,
            as_col: Option<&str>,
        ) -> Result<DataFrame, String> {
            self.predict_cached(trained, df, as_col)
        }
    }
}

/// Instantiates a provider that the resolver has already checked is compiled.
///
/// Returns `Err` when the provider is compiled but its device is unavailable
/// (e.g. no WebGPU adapter); the resolver turns that into a CPU fallback with a
/// warning instead of failing every call.
fn build(kind: BackendKind) -> Result<Box<dyn ComputeBackend>, String> {
    match kind {
        BackendKind::Cpu => Ok(cpu()),
        #[cfg(feature = "cuda")]
        BackendKind::Cuda => Ok(Box::new(cuda::CudaBackend::new()?)),
        #[cfg(feature = "wgpu")]
        BackendKind::Wgpu => Ok(Box::new(wgpu::WgpuBackend::new()?)),
        #[cfg(feature = "onnx")]
        BackendKind::Onnx => Ok(Box::new(onnx::OnnxBackend::new())),
        #[allow(unreachable_patterns)]
        _ => Ok(cpu()),
    }
}

/// The always-available CPU reference provider.
fn cpu() -> Box<dyn ComputeBackend> {
    Box::new(CpuBackend)
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

/// Warning for a compiled backend whose device could not be initialised.
fn device_warning(kind: BackendKind, err: &str) -> String {
    if is_korean() {
        format!(
            "XAZZ_BACKEND={} 장치를 사용할 수 없습니다 ({err}). CPU로 폴백합니다.",
            kind.id()
        )
    } else {
        format!(
            "XAZZ_BACKEND={} device is unavailable ({err}); falling back to CPU.",
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
/// CPU selection is pure. A device-backed provider is probed at construction, so
/// a compiled-but-unavailable device is reported as a fallback warning rather
/// than panicking later. Returns the provider plus an optional warning
/// describing a fallback; `None` means the request was honoured (or absent →
/// CPU).
pub fn resolve(requested: Option<&str>) -> (Box<dyn ComputeBackend>, Option<String>) {
    match requested {
        None => (cpu(), None),
        Some(raw) => match BackendKind::parse(raw) {
            Some(kind) if kind.is_compiled() => match build(kind) {
                Ok(backend) => (backend, None),
                Err(err) => (cpu(), Some(device_warning(kind, &err))),
            },
            Some(kind) => (cpu(), Some(fallback_warning(kind))),
            None => (cpu(), Some(unknown_warning(raw))),
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
    use xazz_compiler::ast::EmbeddingVocab;
    use xazz_compiler::ast::SweepSort;

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
            sweep: Default::default(),
            sweep_metric: Default::default(),
            sweep_metric_explicit: false,
            sweep_sort: Default::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
            split_strategy: Default::default(),
            time_column: None,
        };
        (df, layers, config)
    }

    fn cleanup(checkpoint_path: &str) {
        // Remove only this test's files. Deleting the shared `checkpoints/` directory
        // races with other tests running in parallel (a save between create_dir_all and
        // save_file would fail), so the directory is left in place.
        let _ = std::fs::remove_file(checkpoint_path);
        let _ = std::fs::remove_file(crate::dl::manifest_path(checkpoint_path));
    }

    #[test]
    fn parse_accepts_aliases_and_rejects_garbage() {
        assert_eq!(BackendKind::parse("cpu"), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse(""), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse(" burn-ndarray "), Some(BackendKind::Cpu));
        assert_eq!(BackendKind::parse("CUDA"), Some(BackendKind::Cuda));
        assert_eq!(BackendKind::parse("nvidia"), Some(BackendKind::Cuda));
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
        if backend.id() == "cuda" {
            assert!(warning.is_none());
            return;
        }
        assert_eq!(backend.id(), "cpu");
        assert!(warning.unwrap().contains("cuda"));
    }

    #[test]
    fn parse_wgpu_device_accepts_aliases_and_indexes() {
        assert_eq!(parse_wgpu_device(""), Some(WgpuDeviceSpec::Default));
        assert_eq!(
            parse_wgpu_device(" default "),
            Some(WgpuDeviceSpec::Default)
        );
        assert_eq!(parse_wgpu_device("auto"), Some(WgpuDeviceSpec::Default));
        assert_eq!(parse_wgpu_device("cpu"), Some(WgpuDeviceSpec::Cpu));
        assert_eq!(parse_wgpu_device("dgpu"), Some(WgpuDeviceSpec::Discrete(0)));
        assert_eq!(
            parse_wgpu_device("DiscreteGpu(1)"),
            Some(WgpuDeviceSpec::Discrete(1))
        );
        assert_eq!(
            parse_wgpu_device("igpu:2"),
            Some(WgpuDeviceSpec::Integrated(2))
        );
        assert_eq!(
            parse_wgpu_device("integrated"),
            Some(WgpuDeviceSpec::Integrated(0))
        );
        assert_eq!(
            parse_wgpu_device("vgpu:1"),
            Some(WgpuDeviceSpec::Virtual(1))
        );
        assert_eq!(parse_wgpu_device("quantum"), None);
        assert_eq!(parse_wgpu_device("dgpu:notanumber"), None);
    }

    #[test]
    fn parse_device_spec_accepts_aliases_and_rejects_garbage() {
        assert_eq!(parse_device_spec(""), Some(DeviceSpec::Auto));
        assert_eq!(parse_device_spec(" default "), Some(DeviceSpec::Auto));
        assert_eq!(parse_device_spec("cpu"), Some(DeviceSpec::Cpu));
        assert_eq!(parse_device_spec("coreml"), Some(DeviceSpec::CoreMl));
        assert_eq!(parse_device_spec("dgpu"), Some(DeviceSpec::Discrete(0)));
        assert_eq!(
            parse_device_spec("DiscreteGpu(1)"),
            Some(DeviceSpec::Discrete(1))
        );
        assert_eq!(parse_device_spec("igpu:2"), Some(DeviceSpec::Integrated(2)));
        assert_eq!(parse_device_spec("vgpu:1"), Some(DeviceSpec::Virtual(1)));
        assert_eq!(parse_device_spec("cuda"), Some(DeviceSpec::Cuda(0)));
        assert_eq!(parse_device_spec("cuda:1"), Some(DeviceSpec::Cuda(1)));
        assert_eq!(parse_device_spec("trt:0"), Some(DeviceSpec::TensorRt(0)));
        assert_eq!(parse_device_spec("dml:3"), Some(DeviceSpec::DirectMl(3)));
        assert_eq!(parse_device_spec("quantum"), None);
        assert_eq!(parse_device_spec("cuda:notanumber"), None);
    }

    #[test]
    fn device_train_materializes_in_memory_and_predicts() {
        use burn::tensor::Device;
        use burn_ndarray::NdArray;

        let (df, layers, config) = tiny_dataset();
        let device: Device<NdArray<f32>> = Default::default();
        let trained = crate::dl::train_on_device::<NdArray<f32>>(
            &df,
            "backend_unit_dev",
            &layers,
            &config,
            &device,
        )
        .expect("device train (CPU backend exercises the in-memory handoff)");
        assert!(trained.report.final_train_loss.is_finite());

        let out = crate::dl::predict(&trained, &df, Some("pred")).expect("predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }

    #[test]
    fn unpersisted_train_writes_no_checkpoint_but_is_usable() {
        let (df, layers, config) = tiny_dataset();
        let trained =
            crate::dl::train_unpersisted(&df, "backend_unit_unpersisted", &layers, &config)
                .expect("unpersisted train");
        assert!(
            !std::path::Path::new(&trained.report.checkpoint_path).exists(),
            "unpersisted training must not write a checkpoint"
        );
        let out = crate::dl::predict(&trained, &df, Some("pred")).expect("predict");
        assert_eq!(out.height(), df.height());

        cleanup(&trained.report.checkpoint_path);
    }

    #[test]
    fn parse_ort_ep_spec_auto_and_explicit() {
        assert_eq!(parse_ort_ep_spec("").unwrap(), OrtEpSpec::Auto);
        assert_eq!(parse_ort_ep_spec(" auto ").unwrap(), OrtEpSpec::Auto);
        assert_eq!(
            parse_ort_ep_spec("cuda").unwrap(),
            OrtEpSpec::Explicit(vec![OrtEpKind::Cuda])
        );
        assert_eq!(
            parse_ort_ep_spec("cpu, cuda ,trt").unwrap(),
            OrtEpSpec::Explicit(vec![OrtEpKind::Cpu, OrtEpKind::Cuda, OrtEpKind::TensorRt])
        );
        assert_eq!(
            parse_ort_ep_spec("DirectML").unwrap(),
            OrtEpSpec::Explicit(vec![OrtEpKind::DirectML])
        );
        assert_eq!(
            parse_ort_ep_spec("coreml").unwrap(),
            OrtEpSpec::Explicit(vec![OrtEpKind::CoreML])
        );
        assert!(parse_ort_ep_spec("quantum").is_err());
        assert!(parse_ort_ep_spec(",").is_err());
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

    /// D3 CNN: a Conv1d -> ReLU -> Dense model trains and predicts end-to-end on CPU.
    #[test]
    fn cpu_backend_trains_conv1d_model() {
        use polars::prelude::*;

        let df = df!(
            "x1" => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
            "x2" => [1.0f64, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 0.0],
            "x3" => [0.0f64, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5],
            "x4" => [2.0f64, 1.0, 0.0, 1.0, 2.0, 1.0, 0.0, 1.0],
            "y"  => [0.0f64, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0],
        )
        .expect("conv dataset");

        let layers = vec![
            LayerKind::Conv1d {
                out_channels: 4,
                kernel_size: 3,
            },
            LayerKind::ReLU,
            LayerKind::Dense(1),
        ];
        let config = TrainConfig {
            target: "y".to_string(),
            epochs: 2,
            learning_rate: 0.05,
            batch_size: Some(4),
            validation_split: None,
            early_stopping_patience: None,
            sweep: Default::default(),
            sweep_metric: Default::default(),
            sweep_metric_explicit: false,
            sweep_sort: Default::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
            split_strategy: Default::default(),
            time_column: None,
        };

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let trained = backend
            .train(&df, "backend_unit_conv1d", &layers, &config)
            .expect("cpu conv1d train");
        assert_eq!(trained.report.input_dim, 4);
        assert_eq!(trained.report.output_dim, 1);

        let out = backend
            .predict(&trained, &df, Some("pred"))
            .expect("cpu conv1d predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3: a model whose final output is not a single scalar (e.g. Conv1d without a
    /// final Dense(1)) is rejected instead of silently broadcasting the target.
    #[test]
    fn cpu_backend_rejects_non_scalar_output() {
        let (df, _layers, config) = tiny_dataset();
        let layers = vec![
            LayerKind::Conv1d {
                out_channels: 4,
                kernel_size: 3,
            },
            LayerKind::ReLU,
        ];

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let err = backend
            .train(&df, "backend_unit_bad_dim", &layers, &config)
            .expect_err("multi-output model must be rejected");
        assert!(err.contains("Dense(1)"), "오류 메시지에 안내가 없음: {err}");
    }

    /// D3 Embedding: an Embedding -> ReLU -> Dense model trains and predicts end-to-end on CPU.
    #[test]
    fn cpu_backend_trains_embedding_model() {
        use polars::prelude::*;

        let df = df!(
            "cat1" => [0i64, 1, 2, 3, 0, 1, 2, 3],
            "cat2" => [1i64, 0, 1, 0, 1, 0, 1, 0],
            "y"    => [0.0f64, 1.0, 2.0, 3.0, 0.0, 1.0, 2.0, 3.0],
        )
        .expect("embedding dataset");

        let layers = vec![
            LayerKind::Embedding {
                vocab: EmbeddingVocab::Shared(4),
                embed_dim: 3,
            },
            LayerKind::ReLU,
            LayerKind::Dense(1),
        ];
        let config = TrainConfig {
            target: "y".to_string(),
            epochs: 3,
            learning_rate: 0.05,
            batch_size: Some(4),
            validation_split: None,
            early_stopping_patience: None,
            sweep: Default::default(),
            sweep_metric: Default::default(),
            sweep_metric_explicit: false,
            sweep_sort: Default::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
            split_strategy: Default::default(),
            time_column: None,
        };

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let trained = backend
            .train(&df, "backend_unit_embedding", &layers, &config)
            .expect("cpu embedding train");
        assert_eq!(trained.report.input_dim, 2);
        assert_eq!(trained.report.output_dim, 1);

        let out = backend
            .predict(&trained, &df, Some("pred"))
            .expect("cpu embedding predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 Embedding: each input column can use its own vocabulary.
    #[test]
    fn cpu_backend_trains_per_column_embedding_model() {
        use polars::prelude::*;

        let df = df!(
            "cat1" => [0i64, 1, 2, 0, 1, 2],
            "cat2" => [0i64, 4, 2, 3, 1, 0],
            "y"    => [0.0f64, 1.0, 2.0, 0.0, 1.0, 2.0],
        )
        .expect("per-column embedding dataset");

        let layers = vec![
            LayerKind::Embedding {
                vocab: EmbeddingVocab::PerColumn(vec![3, 5]),
                embed_dim: 2,
            },
            LayerKind::ReLU,
            LayerKind::Dense(1),
        ];
        let config = TrainConfig {
            target: "y".to_string(),
            epochs: 3,
            learning_rate: 0.05,
            batch_size: Some(3),
            validation_split: None,
            early_stopping_patience: None,
            sweep: Default::default(),
            sweep_metric: Default::default(),
            sweep_metric_explicit: false,
            sweep_sort: Default::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
            split_strategy: Default::default(),
            time_column: None,
        };

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let trained = backend
            .train(&df, "backend_unit_embedding_percol", &layers, &config)
            .expect("per-column embedding train");
        assert_eq!(trained.report.input_dim, 2);
        assert_eq!(trained.report.output_dim, 1);

        let out = backend
            .predict(&trained, &df, Some("pred"))
            .expect("per-column embedding predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 Embedding: a per-column vocab list must match the feature count.
    #[test]
    fn cpu_backend_rejects_per_column_vocab_mismatch() {
        let (df, _layers, config) = tiny_dataset();
        let layers = vec![
            LayerKind::Embedding {
                vocab: EmbeddingVocab::PerColumn(vec![3]),
                embed_dim: 2,
            },
            LayerKind::Dense(1),
        ];

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let err = backend
            .train(&df, "backend_unit_vocab_mismatch", &layers, &config)
            .expect_err("per-column vocab length mismatch must be rejected");
        assert!(err.contains("vocab"), "오류 안내가 없음: {err}");
    }

    /// D3 sweep: the grid is fully evaluated and the selected best model is usable.
    #[test]
    fn cpu_backend_sweep_selects_best_combination() {
        let (df, layers, mut config) = tiny_dataset();
        config.sweep.epochs = vec![2, 3];
        config.sweep.learning_rate = vec![0.05, 0.01];
        assert!(config.is_sweep());

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let (trained, report) = backend
            .sweep(&df, "backend_unit_sweep", &layers, &config)
            .expect("cpu sweep");
        assert_eq!(report.combos.len(), 4, "2×2 그리드");
        assert!(
            report.combos[report.best_index].selected,
            "best_index 조합이 선택되어야 함"
        );
        assert_eq!(
            report.combos.iter().filter(|c| c.selected).count(),
            1,
            "선택 조합은 하나여야 함"
        );
        assert!(
            report.combos[report.best_index]
                .final_train_loss
                .is_finite(),
            "최적 조합 손실이 유한해야 함"
        );

        let out = backend
            .predict(&trained, &df, Some("pred"))
            .expect("cpu sweep predict");
        assert_eq!(out.height(), df.height());
        assert!(out.column("pred").is_ok());

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 sweep: `metric: r2` records the metric and picks the highest-R² combo.
    #[test]
    fn cpu_backend_sweep_selects_by_r2_metric() {
        let (df, layers, mut config) = tiny_dataset();
        config.sweep.epochs = vec![2, 3];
        config.sweep.learning_rate = vec![0.05, 0.01];
        config.sweep_metric = SweepMetric::R2;

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let (trained, report) = backend
            .sweep(&df, "backend_unit_sweep_r2", &layers, &config)
            .expect("cpu sweep r2");
        assert_eq!(report.metric, SweepMetric::R2);
        assert_eq!(report.combos.len(), 4, "2×2 그리드");
        assert_eq!(
            report.combos.iter().filter(|c| c.selected).count(),
            1,
            "선택 조합은 하나여야 함"
        );

        let winner = &report.combos[report.best_index];
        let winner_score = SweepReport::score(winner, SweepMetric::R2);
        assert!(
            winner_score.is_finite(),
            "R² 선택 점수가 유한해야 함: {winner_score}"
        );
        for c in &report.combos {
            assert!(
                winner_score <= SweepReport::score(c, SweepMetric::R2),
                "최적 조합이 R² 기준 최고여야 함"
            );
        }

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 sweep: `sort: lr` orders the reported combinations ascending by lr,
    /// while the winner is still selected by the metric.
    #[test]
    fn cpu_backend_sweep_sorts_report_by_axis() {
        let (df, layers, mut config) = tiny_dataset();
        config.sweep.epochs = vec![2, 3];
        config.sweep.learning_rate = vec![0.05, 0.01];
        config.sweep_sort = SweepSort::Lr;
        assert!(config.is_sweep());

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let (trained, report) = backend
            .sweep(&df, "backend_unit_sweep_sort", &layers, &config)
            .expect("cpu sweep sort");
        assert_eq!(report.sort, SweepSort::Lr);
        assert_eq!(report.top, None);
        assert_eq!(report.total_combos, 4);
        let lrs: Vec<f32> = report.combos.iter().map(|c| c.learning_rate).collect();
        assert!(
            lrs.windows(2).all(|w| w[0] <= w[1]),
            "lr 기준 오름차순이어야 함: {lrs:?}"
        );
        assert!(report.combos[report.best_index].selected);
        assert_eq!(report.combos.iter().filter(|c| c.selected).count(), 1);

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 sweep: `tiebreak:` is carried onto the report and used to order ties.
    #[test]
    fn cpu_backend_sweep_carries_tiebreak() {
        let (df, layers, mut config) = tiny_dataset();
        config.sweep.epochs = vec![2, 3];
        config.sweep.learning_rate = vec![0.05, 0.01];
        config.sweep_sort = SweepSort::Metric;
        config.sweep_tiebreak = vec![SweepSort::Batch];
        assert!(config.is_sweep());

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let (trained, report) = backend
            .sweep(&df, "backend_unit_sweep_tiebreak", &layers, &config)
            .expect("cpu sweep tiebreak");
        assert_eq!(report.tiebreak, vec![SweepSort::Batch]);
        // The reported order matches the tiebreak-aware comparator exactly.
        assert!(
            report.combos.windows(2).all(|w| {
                SweepReport::compare(&w[0], &w[1], report.sort, report.metric, &report.tiebreak)
                    != std::cmp::Ordering::Greater
            }),
            "tiebreak 기준 정렬이어야 함"
        );
        assert!(report.combos[report.best_index].selected);

        cleanup(&trained.report.checkpoint_path);
    }

    /// D3 sweep: `top: N` keeps only the N best-by-metric combinations, still
    /// includes the winner, and reports the unfiltered total.
    #[test]
    fn cpu_backend_sweep_top_filters_report() {
        let (df, layers, mut config) = tiny_dataset();
        config.sweep.epochs = vec![2, 3];
        config.sweep.learning_rate = vec![0.05, 0.01];
        config.sweep_top = Some(2);
        assert!(config.is_sweep());

        let (backend, warning) = resolve(None);
        assert!(warning.is_none());

        let (trained, report) = backend
            .sweep(&df, "backend_unit_sweep_top", &layers, &config)
            .expect("cpu sweep top");
        assert_eq!(report.combos.len(), 2, "top: 2로 잘려야 함");
        assert_eq!(report.total_combos, 4, "필터 전 전체 조합 수");
        assert_eq!(report.top, Some(2));
        assert!(report.combos[report.best_index].selected);
        assert_eq!(report.combos.iter().filter(|c| c.selected).count(), 1);
        // Default sort is metric, so the report is best-first.
        let scores: Vec<f64> = report
            .combos
            .iter()
            .map(|c| SweepReport::score(c, report.metric))
            .collect();
        assert!(
            scores.windows(2).all(|w| w[0] <= w[1]),
            "metric 기준 최적 우선이어야 함: {scores:?}"
        );

        cleanup(&trained.report.checkpoint_path);
    }

    /// CUDA provider: on a host without a usable CUDA device the provider must
    /// fall back to CPU at selection time with a clear warning instead of
    /// panicking inside CubeCL. On a real CUDA host this test defers to the
    /// `#[ignore]`d acceptance test, which exercises the actual device path.
    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_provider_falls_back_without_device() {
        let (backend, warning) = resolve(Some("cuda"));
        if backend.id() == "cuda" {
            assert!(warning.is_none());
            return;
        }
        assert_eq!(backend.id(), "cpu");
        assert!(warning.unwrap().contains("cuda"));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Hardware acceptance tests (issue D1 #62, D2 #63)
//
// These require a device/SDK absent in CI, so they are `#[ignore]`d; on a
// matching host run:
//
//   cargo test -p xazz-exec --features wgpu   -- --ignored
//   cargo test -p xazz-exec --features cuda   -- --ignored
//   cargo test -p xazz-exec --features onnx   -- --ignored
//
// `wgpu` and `cuda` are implemented: each trains on its device and evaluates the
// portable checkpoint on the same device. The parity check compares inference
// from one shared CPU-trained checkpoint on CPU vs the requested backend
// (identical weights), rather than two independently-initialised training runs —
// random initialisers differ across backends, so comparing losses from separate
// runs would not be meaningful.
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(test, any(feature = "cuda", feature = "wgpu", feature = "onnx")))]
mod acceptance {
    use super::ComputeBackend;
    use super::tests::tiny_dataset;
    use polars::prelude::*;

    fn predictions(df: &DataFrame) -> Vec<f64> {
        df.column("pred")
            .expect("prediction column")
            .f64()
            .expect("float prediction column")
            .into_no_null_iter()
            .collect()
    }

    /// Trains the requested backend for real, then verifies numerical parity by
    /// evaluating the same CPU-trained checkpoint on CPU and on `requested`.
    #[cfg(any(feature = "cuda", feature = "wgpu", feature = "onnx"))]
    fn assert_parity(requested: &str) {
        let (df, layers, config) = tiny_dataset();
        let cpu = super::CpuBackend
            .train(&df, "acc_cpu", &layers, &config)
            .expect("cpu reference train");

        let (backend, warning) = super::resolve(Some(requested));
        assert!(warning.is_none(), "{requested} should be compiled");
        assert_eq!(backend.id(), requested);

        // Real device training must complete and produce a finite loss.
        let gpu = backend
            .train(&df, "acc_gpu", &layers, &config)
            .expect("backend train");
        assert!(
            gpu.report.final_train_loss.is_finite(),
            "{requested} produced a non-finite training loss"
        );

        // Numerical parity: same weights (cpu checkpoint) evaluated on CPU and device.
        let cpu_out = super::CpuBackend
            .predict(&cpu, &df, Some("pred"))
            .expect("cpu predict");
        let gpu_out = backend
            .predict(&cpu, &df, Some("pred"))
            .expect("backend predict");
        let max_diff = predictions(&cpu_out)
            .into_iter()
            .zip(predictions(&gpu_out))
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f64, f64::max);
        assert!(
            max_diff < 1e-3,
            "{requested} inference diverged from CPU by {max_diff}"
        );

        // The device-trained artifact is usable for inference on the device.
        let out = backend
            .predict(&gpu, &df, Some("pred"))
            .expect("backend predict (device-trained)");
        assert_eq!(out.height(), df.height());

        let _ = std::fs::remove_file(&cpu.report.checkpoint_path);
        let _ = std::fs::remove_file(&gpu.report.checkpoint_path);
        let _ = std::fs::remove_file(crate::dl::manifest_path(&cpu.report.checkpoint_path));
        let _ = std::fs::remove_file(crate::dl::manifest_path(&gpu.report.checkpoint_path));
        // ONNX artifacts are written next to the checkpoints by that provider.
        for ckpt in [&cpu.report.checkpoint_path, &gpu.report.checkpoint_path] {
            let _ = std::fs::remove_file(format!("{}.onnx", ckpt.trim_end_matches(".json")));
        }
        // The shared `checkpoints/` directory is intentionally not removed: deleting it
        // races with parallel tests that are mid-save.
    }

    #[cfg(feature = "cuda")]
    #[test]
    #[ignore = "requires a CUDA GPU + burn-cuda: `cargo test -p xazz-exec --features cuda -- --ignored` (issue #62)"]
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
    #[ignore = "builds ONNX Runtime (downloaded by ort) + exports a model: `cargo test -p xazz-exec --features onnx -- --ignored` (issue #63)"]
    fn onnx_matches_cpu_losses() {
        assert_parity("onnx");
    }
}
