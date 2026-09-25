//! xazz-exec/src/dl.rs — Burn deep-learning execution engine (v0.4)
//!
//! Translates `.xzz` `model {}` declarations and `run v |> train(...)` syntax into
//! actual Burn neural-network training and executes it.
//!
//! Backend: burn-ndarray (pure Rust CPU). Structured around the `Backend` generic,
//! so switching to torch / wgpu only requires swapping this module's `AD`/`Plain` type aliases.
//!
//!   - AD    = NdArrayAutodiff<f32>  (for training: autodiff graph enabled)
//!   - Plain = NdArray<f32>          (for inference)

use std::cmp::Ordering;
use std::collections::HashMap;

use burn::{
    backend::Autodiff,
    module::{AutodiffModule, Module},
    nn::{
        DropoutConfig, Embedding, EmbeddingConfig, Linear, LinearConfig, PaddingConfig1d,
        conv::{Conv1d, Conv1dConfig},
    },
    optim::{AdamConfig, GradientsParams, Optimizer},
    record::{BinBytesRecorder, FullPrecisionSettings, PrettyJsonFileRecorder, Recorder},
    tensor::{
        Device, Tensor, TensorData,
        activation::{relu, sigmoid, softmax, tanh},
        backend::{AutodiffBackend, Backend, BackendTypes},
    },
};
use burn_ndarray::NdArray;
use polars::prelude::{Column, DataFrame};
use xazz_compiler::ast::{LayerKind, SweepMetric, SweepSort, TrainConfig};
use xazz_core::i18n::{is_korean, tr};

use crate::tensor_bridge::{extract_data, series_to_f32};

/// ONNX export + inference path (D2 #63), enabled by the `onnx` feature. A child
/// module so it can read `Mlp`'s private graph/weights without widening them.
#[cfg(feature = "onnx")]
pub(crate) mod onnx_export;

/// Autodiff backend for training (CPU): NdArray + Autodiff wrapper.
pub type AD = Autodiff<NdArray<f32>>;
/// Pure backend for inference (CPU).
pub type Plain = NdArray<f32>;

/// One training batch on an autodiff backend: `(features, targets)`.
type TrainBatch<B> = (Tensor<Autodiff<B>, 2>, Tensor<Autodiff<B>, 2>);

/// Upper bound of the validation split ratio — at most this fraction of the data can be held out for validation.
const MAX_VALIDATION_SPLIT: f64 = 0.9;

/// Application-level checkpoint schema version (D3).
///
/// The Burn record carries its own serialization version; this constant versions
/// Xazz's sidecar manifest so a checkpoint produced by a newer build is rejected
/// instead of being silently misread. Bump it whenever the manifest layout or the
/// checkpoint's semantics change incompatibly.
pub const CHECKPOINT_FORMAT_VERSION: u32 = 1;

/// DSL model block activation/normalization layer kinds.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Activation {
    None,
    ReLU,
    Sigmoid,
    Tanh,
    Softmax,
    Dropout(f64),
}

/// Applies the per-layer activation function.
/// Dropout is applied only during training (training=true); inference passes it through as the identity function.
fn apply_activation<B: Backend, const D: usize>(
    act: &Activation,
    x: Tensor<B, D>,
    training: bool,
) -> Tensor<B, D> {
    match act {
        Activation::None => x,
        Activation::ReLU => relu(x),
        Activation::Sigmoid => sigmoid(x),
        Activation::Tanh => tanh(x),
        Activation::Softmax => softmax(x, 1),
        Activation::Dropout(prob) => {
            if training {
                DropoutConfig::new(*prob).init().forward(x)
            } else {
                x
            }
        }
    }
}

/// A single forward operation in the compiled model graph, in declaration order.
/// The payload indexes into [`Mlp::linears`] / [`Mlp::convs`].
#[derive(Debug, Clone, Copy, PartialEq)]
enum LayerOp {
    /// Dense (Linear) layer.
    Dense(usize),
    /// Conv1d layer (1D convolution over the feature axis).
    Conv1d(usize),
    /// Embedding layer (categorical index → vector), always the first op.
    Embedding(usize),
}

/// Burn module expressing the DSL `model { Dense -> ReLU -> ... }` as a dynamic
/// graph of Dense / Conv1d layers — a multi-layer perceptron (MLP) and/or a 1D
/// convolutional network over tabular features.
// (The Burn Module derive provides Clone)
#[derive(Module, Debug)]
pub struct Mlp<B: Backend> {
    /// Sequence of Dense(units) layers.
    linears: Vec<Linear<B>>,
    /// Sequence of Conv1d(out_channels, kernel_size) layers.
    convs: Vec<Conv1d<B>>,
    /// Sequence of Embedding(vocab, embed_dim) layers.
    embeddings: Vec<Embedding<B>>,
    /// Per-embedding, per-column vocabulary size (for index clamping); parallel
    /// to `embeddings`.
    #[module(skip)]
    embed_vocab: Vec<Vec<usize>>,
    /// Per-embedding, per-column row offset into the combined table; parallel to
    /// `embeddings` and `embed_vocab`.
    #[module(skip)]
    embed_offsets: Vec<Vec<usize>>,
    /// Forward order: each entry pairs an op with the activation applied after it.
    #[module(skip)]
    ops: Vec<(LayerOp, Activation)>,
    /// Output feature dimension (result of the declared graph) — reported in the train report.
    #[module(skip)]
    out_dim: usize,
    /// Whether the graph consumes raw category indices (first layer is Embedding).
    #[module(skip)]
    raw_input: bool,
    /// Whether in training mode — determines Dropout application (false during inference).
    #[module(skip)]
    training: bool,
}

impl<B: Backend> Mlp<B> {
    /// Forward pass: [batch, input_dim] → [batch, output_dim].
    fn forward(&self, input: Tensor<B, 2>) -> Tensor<B, 2> {
        let mut x = input;
        for (op, act) in &self.ops {
            x = match op {
                LayerOp::Dense(i) => {
                    apply_activation(act, self.linears[*i].forward(x), self.training)
                }
                LayerOp::Conv1d(i) => {
                    // [batch, len] → [batch, 1, len] → conv (Same padding keeps len) → [batch, channels, len]
                    let [batch, len] = x.dims();
                    let y = self.convs[*i].forward(x.reshape([batch, 1, len]));
                    let [b, c, l] = y.dims();
                    apply_activation(act, y.reshape([b, c * l]), self.training)
                }
                LayerOp::Embedding(i) => {
                    // [batch, len] category indices → [batch, len, embed_dim] → [batch, len * embed_dim].
                    // Each input column j has its own vocab and row offset in the combined table.
                    let vocabs = &self.embed_vocab[*i];
                    let offsets = &self.embed_offsets[*i];
                    let [_, len] = x.dims();
                    let mut cols: Vec<Tensor<B, 2>> = Vec::with_capacity(len);
                    for (j, (&v, &offset)) in vocabs.iter().zip(offsets).enumerate() {
                        let idx = x
                            .clone()
                            .narrow(1, j, 1)
                            .clamp(0.0, v.saturating_sub(1) as f32)
                            + offset as f32;
                        cols.push(idx);
                    }
                    let idx = Tensor::cat(cols, 1).int();
                    let y = self.embeddings[*i].forward(idx);
                    let [b, l, d] = y.dims();
                    apply_activation(act, y.reshape([b, l * d]), self.training)
                }
            };
        }
        x
    }
}

/// Builds the MLP/CNN from the DSL layer list and input dimension.
fn build_mlp<B: Backend>(
    layers: &[LayerKind],
    input_dim: usize,
    device: &Device<B>,
) -> Result<Mlp<B>, String> {
    let mut linears: Vec<Linear<B>> = Vec::new();
    let mut convs: Vec<Conv1d<B>> = Vec::new();
    let mut embeddings: Vec<Embedding<B>> = Vec::new();
    let mut embed_vocab: Vec<Vec<usize>> = Vec::new();
    let mut embed_offsets: Vec<Vec<usize>> = Vec::new();
    let mut ops: Vec<(LayerOp, Activation)> = Vec::new();
    let mut cur = input_dim;

    for layer in layers {
        match layer {
            LayerKind::Dense(n) if *n > 0 => {
                linears.push(LinearConfig::new(cur, *n).init(device));
                ops.push((LayerOp::Dense(linears.len() - 1), Activation::None));
                cur = *n;
            }
            LayerKind::Dense(_) => {
                return Err(tr(
                    "Dense layer unit count must be >= 1.",
                    "Dense 레이어의 유닛 수는 1 이상이어야 합니다.",
                )
                .into());
            }
            LayerKind::Conv1d {
                out_channels,
                kernel_size,
            } if *out_channels > 0 && *kernel_size > 0 => {
                let config = Conv1dConfig::new(1, *out_channels, *kernel_size)
                    .with_padding(PaddingConfig1d::Same);
                convs.push(config.init(device));
                ops.push((LayerOp::Conv1d(convs.len() - 1), Activation::None));
                // Same padding + stride 1 preserves the length: [batch, 1, cur] → [batch, out_channels, cur].
                cur *= *out_channels;
            }
            LayerKind::Conv1d { .. } => {
                return Err(tr(
                    "Conv1d out_channels and kernel_size must be >= 1.",
                    "Conv1d 의 out_channels 와 kernel_size 는 1 이상이어야 합니다.",
                )
                .into());
            }
            LayerKind::Embedding { vocab, embed_dim } if vocab.is_valid() && *embed_dim > 0 => {
                // One combined table per embedding layer; input column j owns the
                // disjoint row range [offset_j, offset_j + vocab_j).
                let sizes = vocab.expand(cur)?;
                let total: usize = sizes.iter().sum();
                if total == 0 {
                    return Err(tr(
                        "Embedding has no vocabulary entries to embed.",
                        "Embedding 에 임베딩할 vocab 항목이 없습니다.",
                    )
                    .into());
                }
                embeddings.push(EmbeddingConfig::new(total, *embed_dim).init(device));
                let mut offsets = Vec::with_capacity(sizes.len());
                let mut offset = 0usize;
                for size in &sizes {
                    offsets.push(offset);
                    offset += size;
                }
                ops.push((LayerOp::Embedding(embeddings.len() - 1), Activation::None));
                embed_vocab.push(sizes);
                embed_offsets.push(offsets);
                // Each of the `cur` input positions is embedded into `embed_dim` features.
                cur *= *embed_dim;
            }
            LayerKind::Embedding { .. } => {
                return Err(tr(
                    "Embedding vocab and embed_dim must be >= 1.",
                    "Embedding 의 vocab 과 embed_dim 은 1 이상이어야 합니다.",
                )
                .into());
            }
            LayerKind::ReLU => set_activation(&mut ops, Activation::ReLU),
            LayerKind::Sigmoid => set_activation(&mut ops, Activation::Sigmoid),
            LayerKind::Tanh => set_activation(&mut ops, Activation::Tanh),
            LayerKind::Softmax => set_activation(&mut ops, Activation::Softmax),
            LayerKind::Dropout(r) => set_activation(&mut ops, Activation::Dropout(*r)),
            // BatchNorm (1D MLP) is omitted because its layout differs from Burn's 2D BatchNorm.
            LayerKind::BatchNorm => { /* pass-through */ }
        }
    }

    if ops.is_empty() {
        return Err(tr(
            "The model has no Dense or Conv1d layers.",
            "모델에 Dense 또는 Conv1d 레이어가 하나도 없습니다.",
        )
        .into());
    }
    Ok(Mlp {
        linears,
        convs,
        embeddings,
        embed_vocab,
        embed_offsets,
        ops,
        out_dim: cur,
        raw_input: matches!(layers.first(), Some(LayerKind::Embedding { .. })),
        training: true,
    })
}

/// Records the activation after the last Dense/Conv1d op (consecutive activations keep only the last one).
fn set_activation(ops: &mut [(LayerOp, Activation)], act: Activation) {
    if let Some(last) = ops.last_mut() {
        last.1 = act;
    }
}

/// Per-column vocabulary sizes of the leading Embedding layer, if the model
/// consumes raw category indices. The checker enforces Embedding-first, so there
/// is at most one such layer.
fn leading_embedding_vocabs(layers: &[LayerKind], feature_count: usize) -> Option<Vec<usize>> {
    match layers.first() {
        Some(LayerKind::Embedding { vocab, .. }) if vocab.is_valid() => {
            vocab.expand(feature_count).ok()
        }
        _ => None,
    }
}

/// Counts raw embedding indices outside `[0, vocab_size - 1]`. The forward pass
/// clamps them silently, so this feeds the runtime diagnostic below (issue D3).
/// Non-finite values are excluded: the forward pass maps them to index 0.
fn count_out_of_range_indices(values: &[f32], vocab_size: usize) -> usize {
    let max = vocab_size.saturating_sub(1) as f32;
    values
        .iter()
        .filter(|v| v.is_finite() && (**v < 0.0 || **v > max))
        .count()
}

/// Counts raw embedding inputs that are finite but not whole numbers. The
/// forward pass casts the raw value to an integer (truncation), so a continuous
/// feature fed to Embedding silently loses its fractional part — this feeds the
/// runtime diagnostic below (issue D3). Non-finite values are excluded because
/// the forward pass maps them to index 0.
fn count_non_integer_indices(values: &[f32]) -> usize {
    values
        .iter()
        .filter(|v| v.is_finite() && v.fract() != 0.0)
        .count()
}

/// Counts out-of-range indices across all columns, each against its own vocab.
fn count_out_of_range_per_column(values: &[f32], feature_count: usize, vocabs: &[usize]) -> usize {
    if feature_count == 0 {
        return 0;
    }
    let mut count = 0usize;
    for (idx, v) in values.iter().enumerate() {
        if let Some(&vocab) = vocabs.get(idx % feature_count) {
            count += count_out_of_range_indices(std::slice::from_ref(v), vocab);
        }
    }
    count
}

/// Emits the out-of-range embedding diagnostic to stderr. Non-fatal — the value
/// is clamped, matching the documented forward-pass behaviour.
fn warn_embedding_out_of_range(count: usize, vocabs: &[usize]) {
    let uniform = vocabs.windows(2).all(|w| w[0] == w[1]);
    let msg = if uniform {
        let vocab_size = vocabs.first().copied().unwrap_or(0);
        let max = vocab_size.saturating_sub(1);
        if is_korean() {
            format!(
                "Embedding 입력 범주 인덱스 {count}개가 범위를 벗어났습니다 (vocab_size={vocab_size}). forward 에서 [0, {max}] 로 clamp 됩니다. 범주형 컬럼 값/스키마를 확인하세요."
            )
        } else {
            format!(
                "{count} embedding input index/indices are out of range (vocab_size={vocab_size}); they are clamped to [0, {max}] in the forward pass. Check the categorical column values/schema."
            )
        }
    } else {
        let list = vocabs
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        if is_korean() {
            format!(
                "Embedding 입력 범주 인덱스 {count}개가 범위를 벗어났습니다 (컬럼별 vocab=[{list}]). forward 에서 컬럼별 [0, vocab-1] 로 clamp 됩니다. 범주형 컬럼 값/스키마를 확인하세요."
            )
        } else {
            format!(
                "{count} embedding input index/indices are out of range (per-column vocab=[{list}]); they are clamped to each column's [0, vocab-1] in the forward pass. Check the categorical column values/schema."
            )
        }
    };
    eprintln!("[xazz] {msg}");
}

/// Emits the non-integer embedding diagnostic to stderr. Non-fatal — the value
/// is truncated, matching the documented forward-pass behaviour. A continuous
/// feature must not be fed to Embedding: z-score is skipped for raw-index models,
/// so the raw value becomes an index and its fractional part is lost.
fn warn_embedding_non_integer(count: usize) {
    let msg = if is_korean() {
        format!(
            "Embedding 입력 {count}개가 정수가 아닌 연속형 값입니다. raw 인덱스 모델은 z-score 를 건너뛰므로 소수부가 버려져(truncate) 인덱스로 사용됩니다. 범주형(정수 인코딩) 컬럼인지 확인하세요."
        )
    } else {
        format!(
            "{count} embedding input value(s) are non-integer continuous values; raw-index models skip z-score, so the fractional part is truncated to form an index. Check that the columns are categorical (integer-coded)."
        )
    };
    eprintln!("[xazz] {msg}");
}

/// Runs the out-of-range embedding diagnostic when the model consumes raw indices.
fn check_embedding_indices(layers: &[LayerKind], values: &[f32], feature_count: usize) {
    if let Some(vocabs) = leading_embedding_vocabs(layers, feature_count) {
        let count = count_out_of_range_per_column(values, feature_count, &vocabs);
        if count > 0 {
            warn_embedding_out_of_range(count, &vocabs);
        }
        let non_integer = count_non_integer_indices(values);
        if non_integer > 0 {
            warn_embedding_non_integer(non_integer);
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Training result report
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize)]
pub struct TrainReport {
    pub model_name: String,
    pub target: String,
    pub feature_names: Vec<String>,
    pub input_dim: usize,
    pub output_dim: usize,
    pub num_params: usize,
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f32,
    pub final_train_loss: f64,
    pub final_val_loss: Option<f64>,
    pub predictions: Vec<f64>,
    pub targets: Vec<f64>,
    pub checkpoint_path: String,
    /// Xazz checkpoint format version written with this artifact (D3).
    #[serde(default)]
    pub checkpoint_format_version: u32,
    /// Whether training stopped early (validation loss plateaued) — issue D3.
    #[serde(default)]
    pub stopped_early: bool,
    /// Best validation epoch (1-based); 0 when no validation split was used.
    #[serde(default)]
    pub best_epoch: usize,
    /// Final mean absolute error over the training split (D3 sweep metric).
    #[serde(default)]
    pub final_train_mae: f64,
    /// Final mean absolute error over the validation split (if any).
    #[serde(default)]
    pub final_val_mae: Option<f64>,
    /// Final coefficient of determination (R²) over the training split.
    #[serde(default)]
    pub final_train_r2: f64,
    /// Final coefficient of determination (R²) over the validation split (if any).
    #[serde(default)]
    pub final_val_r2: Option<f64>,
}

/// Trained model — also holds the standardization statistics needed for predict().
#[derive(Debug, Clone)]
pub struct TrainedModel {
    /// Pure model for inference (no autodiff graph).
    pub model: Mlp<Plain>,
    /// Training result report (for markers/logs).
    pub report: TrainReport,
    /// Declared layer graph, kept so a non-CPU provider can rebuild the module
    /// structure and load the portable checkpoint onto its own device.
    pub layers: Vec<LayerKind>,
    /// Feature column order (1:1 correspondence with standardization statistics).
    pub feature_names: Vec<String>,
    /// Per-feature mean (z-score).
    pub fmean: Vec<f64>,
    /// Per-feature standard deviation (z-score).
    pub fstd: Vec<f64>,
    /// Training target column.
    pub target: String,
}

/// One evaluated point of a hyperparameter sweep (D3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SweepCombo {
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f32,
    pub final_train_loss: f64,
    pub final_val_loss: Option<f64>,
    pub stopped_early: bool,
    pub best_epoch: usize,
    /// Final training-split MAE (D3 sweep metric).
    #[serde(default)]
    pub train_mae: f64,
    /// Final validation-split MAE, when a split is configured.
    #[serde(default)]
    pub val_mae: Option<f64>,
    /// Final training-split R² (D3 sweep metric).
    #[serde(default)]
    pub train_r2: f64,
    /// Final validation-split R², when a split is configured.
    #[serde(default)]
    pub val_r2: Option<f64>,
    /// Whether this combination was selected as the sweep winner.
    pub selected: bool,
}

/// Result of a grid-search hyperparameter sweep (D3).
#[derive(Debug, Clone, serde::Serialize)]
pub struct SweepReport {
    pub model_name: String,
    pub target: String,
    pub combos: Vec<SweepCombo>,
    /// Index into `combos` of the selected (best) combination.
    pub best_index: usize,
    /// Metric the winner was selected by (D3). Defaults to MSE.
    #[serde(default)]
    pub metric: SweepMetric,
    /// Ordering of the reported combinations (D3). Defaults to metric.
    #[serde(default)]
    pub sort: SweepSort,
    /// Tiebreak axes tried in order after the primary `sort:` key (D3). Empty
    /// uses the per-sort canonical fallback.
    #[serde(default)]
    pub tiebreak: Vec<SweepSort>,
    /// When set, only this many best-by-metric combinations are reported (D3).
    #[serde(default)]
    pub top: Option<usize>,
    /// Number of combinations evaluated before any `top` filter (D3).
    #[serde(default)]
    pub total_combos: usize,
}

impl SweepReport {
    /// Selection score for `combo` under `metric` — lower is always better.
    ///
    /// The validation split is preferred over the training split when available;
    /// R² is negated so that minimising the score maximises R². Non-finite
    /// metrics rank last.
    pub fn score(combo: &SweepCombo, metric: SweepMetric) -> f64 {
        let (primary, fallback) = match metric {
            SweepMetric::Mse => (combo.final_val_loss, combo.final_train_loss),
            SweepMetric::Mae => (combo.val_mae, combo.train_mae),
            SweepMetric::R2 => (combo.val_r2, combo.train_r2),
        };
        let value = match primary {
            Some(v) if v.is_finite() => v,
            _ => fallback,
        };
        if !value.is_finite() {
            return f64::INFINITY;
        }
        if metric.lower_is_better() {
            value
        } else {
            -value
        }
    }

    /// Ordering of two combinations under the report `sort` (D3).
    ///
    /// `Metric` sorts best-first (ascending [`Self::score`]); the axis variants
    /// sort ascending by that hyperparameter. Ties fall back to the remaining
    /// axes so the order is deterministic. Each entry of `tiebreak` (axis values
    /// only) is compared before the canonical remaining axes, in the given order;
    /// an empty slice keeps the per-sort canonical fallback.
    pub fn compare(
        a: &SweepCombo,
        b: &SweepCombo,
        sort: SweepSort,
        metric: SweepMetric,
        tiebreak: &[SweepSort],
    ) -> Ordering {
        let axis = |ax: SweepSort, a: &SweepCombo, b: &SweepCombo| match ax {
            SweepSort::Epochs => a.epochs.cmp(&b.epochs),
            SweepSort::Lr => a
                .learning_rate
                .partial_cmp(&b.learning_rate)
                .unwrap_or(Ordering::Equal),
            SweepSort::Batch => a.batch_size.cmp(&b.batch_size),
            SweepSort::Metric => Ordering::Equal,
        };
        let primary = match sort {
            SweepSort::Metric => Self::score(a, metric)
                .partial_cmp(&Self::score(b, metric))
                .unwrap_or(Ordering::Equal),
            axis_sort => axis(axis_sort, a, b),
        };

        // The explicit tiebreak axes (if any) are tried first, in order, then the
        // remaining axes in canonical order — excluding the primary sort axis and
        // any axis already listed so each axis is compared at most once.
        let mut fallback: Vec<SweepSort> = Vec::with_capacity(3);
        for &t in tiebreak {
            if t.is_axis() && t != sort && !fallback.contains(&t) {
                fallback.push(t);
            }
        }
        for ax in [SweepSort::Epochs, SweepSort::Lr, SweepSort::Batch] {
            if ax != sort && !fallback.contains(&ax) {
                fallback.push(ax);
            }
        }

        let mut ord = primary;
        for ax in fallback {
            ord = ord.then_with(|| axis(ax, a, b));
        }
        ord
    }
}

/// Persists an inference model to `path` (Burn appends the `.json` extension).
pub fn save_checkpoint(model: &Mlp<Plain>, path: &str) -> Result<(), String> {
    let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
    model.clone().save_file(path, &recorder).map_err(|e| {
        format!(
            "{}: {e}",
            tr("checkpoint save failed", "체크포인트 저장 실패")
        )
    })
}

/// Sidecar metadata written next to a Burn checkpoint (`<name>.json` →
/// `<name>.meta.json`).
///
/// The Burn record holds only the weights; the manifest records the Xazz format
/// version plus the model shape and training provenance, so a checkpoint can be
/// validated against the model it is loaded into and a future format change can
/// be detected instead of silently misread.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CheckpointManifest {
    pub format_version: u32,
    pub xazz_version: String,
    pub model_name: String,
    pub target: String,
    pub input_dim: usize,
    pub output_dim: usize,
    pub feature_names: Vec<String>,
    /// Declared layer graph rendered via `Debug` (e.g. `Dense(4)`, `ReLU`).
    pub layers: Vec<String>,
    pub epochs: usize,
    pub batch_size: usize,
    pub learning_rate: f32,
    pub final_train_loss: f64,
    pub final_val_loss: Option<f64>,
    pub stopped_early: bool,
    pub best_epoch: usize,
}

impl CheckpointManifest {
    /// Builds a manifest from a training report and the declared layer graph.
    pub fn from_report(report: &TrainReport, layers: &[LayerKind]) -> Self {
        Self {
            format_version: CHECKPOINT_FORMAT_VERSION,
            xazz_version: env!("CARGO_PKG_VERSION").to_string(),
            model_name: report.model_name.clone(),
            target: report.target.clone(),
            input_dim: report.input_dim,
            output_dim: report.output_dim,
            feature_names: report.feature_names.clone(),
            layers: layers.iter().map(|l| format!("{l:?}")).collect(),
            epochs: report.epochs,
            batch_size: report.batch_size,
            learning_rate: report.learning_rate,
            final_train_loss: report.final_train_loss,
            final_val_loss: report.final_val_loss,
            stopped_early: report.stopped_early,
            best_epoch: report.best_epoch,
        }
    }

    /// Builds a manifest from a materialised [`TrainedModel`].
    pub fn from_trained(trained: &TrainedModel) -> Self {
        Self::from_report(&trained.report, &trained.layers)
    }
}

/// Sidecar manifest path for a Burn checkpoint path (`.json` → `.meta.json`).
pub fn manifest_path(checkpoint_path: &str) -> String {
    format!("{}.meta.json", checkpoint_path.trim_end_matches(".json"))
}

/// Writes the sidecar manifest next to the Burn checkpoint.
pub fn save_checkpoint_manifest(
    checkpoint_path: &str,
    manifest: &CheckpointManifest,
) -> Result<(), String> {
    let path = manifest_path(checkpoint_path);
    let json = serde_json::to_string_pretty(manifest).map_err(|e| {
        format!(
            "{}: {e}",
            tr(
                "checkpoint manifest encode failed",
                "체크포인트 매니페스트 인코딩 실패"
            )
        )
    })?;
    std::fs::write(&path, json).map_err(|e| {
        format!(
            "{} '{path}': {e}",
            tr(
                "checkpoint manifest save failed",
                "체크포인트 매니페스트 저장 실패"
            )
        )
    })
}

/// Cheap on-disk identity of a checkpoint/artifact for in-memory cache
/// invalidation (issue D1/D2: repeated `predict` must not reload/re-export).
///
/// Combines the path with the file's mtime and length so a retrain that rewrites
/// the same path (different weights) invalidates a cached inference module,
/// while a stable file is reused across calls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactKey {
    pub path: String,
    /// File mtime in nanoseconds since the epoch (0 when unavailable).
    pub modified_nanos: u64,
    /// File length in bytes (0 when unavailable).
    pub len: u64,
}

/// Builds an [`ArtifactKey`] for `path`. A missing file yields a key with zero
/// mtime/len, which never equals a real on-disk key (so the caller reloads and
/// surfaces the real "file not found" error).
pub fn artifact_key(path: &str) -> ArtifactKey {
    let (modified_nanos, len) = std::fs::metadata(path)
        .map(|m| {
            let nanos = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0);
            (nanos, m.len())
        })
        .unwrap_or((0, 0));
    ArtifactKey {
        path: path.to_string(),
        modified_nanos,
        len,
    }
}

/// Inference-cache slot count from `XAZZ_INFER_CACHE_SLOTS` (default 4).
///
/// A single slot thrashes when a workload alternates between models; a small
/// bounded LRU keeps the most recently used inference modules resident while
/// never growing without bound. Set to `1` to reproduce the old single-slot
/// behaviour, or higher for model-round-robin workloads.
pub fn infer_cache_slots() -> usize {
    std::env::var("XAZZ_INFER_CACHE_SLOTS")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(4)
}

/// Bounded least-recently-used cache for backend inference artifacts (issue
/// D1/D2). The most recently used entry is kept at index 0.
pub struct LruCache<K, V> {
    capacity: usize,
    entries: Vec<(K, V)>,
}

impl<K: PartialEq, V> LruCache<K, V> {
    /// Creates a cache holding at most `capacity` entries (minimum 1).
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: Vec::new(),
        }
    }

    /// Returns the value for `key`, promoting it to most-recently-used, loading
    /// and inserting it via `load` on a miss.
    pub fn get_or_insert_with(
        &mut self,
        key: K,
        load: impl FnOnce() -> Result<V, String>,
    ) -> Result<&mut V, String> {
        match self.entries.iter().position(|(k, _)| *k == key) {
            Some(pos) => {
                if pos != 0 {
                    let entry = self.entries.remove(pos);
                    self.entries.insert(0, entry);
                }
            }
            None => {
                let value = load()?;
                self.entries.insert(0, (key, value));
                self.entries.truncate(self.capacity);
            }
        }
        Ok(&mut self.entries[0].1)
    }

    /// Number of resident entries (test-only helper).
    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Reads and validates the sidecar manifest for a checkpoint.
///
/// Returns `Ok(None)` when the manifest is absent — a legacy checkpoint written
/// before versioning — so callers can fall back to the Burn record. Returns an
/// error when the manifest is unreadable or declares a format version newer than
/// this build understands (fail-closed on forward incompatibility).
pub fn load_checkpoint_manifest(
    checkpoint_path: &str,
) -> Result<Option<CheckpointManifest>, String> {
    let path = manifest_path(checkpoint_path);
    if !std::path::Path::new(&path).exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        format!(
            "{} '{path}': {e}",
            tr(
                "checkpoint manifest read failed",
                "체크포인트 매니페스트 읽기 실패"
            )
        )
    })?;
    let manifest: CheckpointManifest = serde_json::from_str(&raw).map_err(|e| {
        format!(
            "{} '{path}': {e}",
            tr(
                "checkpoint manifest parse failed",
                "체크포인트 매니페스트 파싱 실패"
            )
        )
    })?;
    if manifest.format_version > CHECKPOINT_FORMAT_VERSION {
        return Err(format!(
            "{} (manifest v{}, supported v{})",
            tr(
                "checkpoint format is newer than this build supports",
                "체크포인트 형식이 이 빌드가 지원하는 버전보다 최신입니다"
            ),
            manifest.format_version,
            CHECKPOINT_FORMAT_VERSION
        ));
    }
    Ok(Some(manifest))
}

// ─────────────────────────────────────────────────────────────────────────────
// Public API
// ─────────────────────────────────────────────────────────────────────────────

/// Backend-agnostic training output: a plain (non-autodiff) module on backend `B`
/// plus everything needed to build the portable [`TrainedModel`] artifact.
struct RawTrained<B: Backend> {
    model: Mlp<B>,
    report: TrainReport,
    feature_names: Vec<String>,
    fmean: Vec<f64>,
    fstd: Vec<f64>,
    target: String,
}

/// Computes `(mae, r2)` for a set of predictions against targets (D3 sweep metrics).
///
/// MAE is the mean absolute error. R² is `1 - SS_res / SS_tot`; when the targets
/// have zero variance it is reported as `0.0` (undefined) rather than NaN so that
/// sweep ranking stays well-defined.
fn regression_metrics(preds: &[f32], targets: &[f32]) -> (f64, f64) {
    let n = preds.len().min(targets.len());
    if n == 0 {
        return (f64::NAN, f64::NAN);
    }
    let mut abs_sum = 0.0f64;
    let mut mean_t = 0.0f64;
    for i in 0..n {
        abs_sum += (preds[i] as f64 - targets[i] as f64).abs();
        mean_t += targets[i] as f64;
    }
    let mae = abs_sum / n as f64;
    mean_t /= n as f64;

    let mut ss_res = 0.0f64;
    let mut ss_tot = 0.0f64;
    for i in 0..n {
        let p = preds[i] as f64;
        let t = targets[i] as f64;
        ss_res += (t - p).powi(2);
        ss_tot += (t - mean_t).powi(2);
    }
    let r2 = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    (mae, r2)
}

/// Runs `dataset |> train(<model>, target: "...", ...)` on the default CPU backend.
pub fn train(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
) -> Result<TrainedModel, String> {
    train_cpu(df, model_name, layers, config, true)
}

/// Like [`train`], but does not persist the checkpoint — used by the sweep grid
/// so only the winning combination is written (issue D1/D3).
pub fn train_unpersisted(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
) -> Result<TrainedModel, String> {
    train_cpu(df, model_name, layers, config, false)
}

/// CPU training core shared by [`train`] and [`train_unpersisted`].
fn train_cpu(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
    persist: bool,
) -> Result<TrainedModel, String> {
    let device: Device<NdArray<f32>> = Default::default();
    let raw = train_impl::<NdArray<f32>>(df, model_name, layers, config, &device, persist)?;
    Ok(TrainedModel {
        model: raw.model,
        report: raw.report,
        layers: layers.to_vec(),
        feature_names: raw.feature_names,
        fmean: raw.fmean,
        fstd: raw.fstd,
        target: raw.target,
    })
}

/// Trains on an arbitrary Burn backend `B`, then materialises the portable CPU
/// [`TrainedModel`] artifact by round-tripping the checkpoint through Burn's
/// backend-neutral record format. GPU providers (e.g. `burn-wgpu`) call this so
/// the artifact they hand back is structurally identical to the CPU reference,
/// and `predict`/`predict_on` can consume it on any device.
pub fn train_on<B>(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
) -> Result<TrainedModel, String>
where
    B: Backend,
    Autodiff<B>: AutodiffBackend,
    Autodiff<B>: BackendTypes<Device = Device<B>>,
    Mlp<Autodiff<B>>: AutodiffModule<Autodiff<B>, InnerModule = Mlp<B>>,
{
    train_on_device::<B>(df, model_name, layers, config, &Default::default())
}

/// Like [`train_on`], but trains on an explicit device `B::Device`.
///
/// GPU providers pass their device here so training runs on the intended
/// accelerator (rather than the backend's default device), and so the device
/// index is explicit. The returned artifact is the portable CPU
/// [`TrainedModel`], transferred from the device **in memory** (issue D1/D2), so
/// no checkpoint save→load disk round-trip is added to training time.
pub fn train_on_device<B>(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
    device: &Device<B>,
) -> Result<TrainedModel, String>
where
    B: Backend,
    Autodiff<B>: AutodiffBackend,
    Autodiff<B>: BackendTypes<Device = Device<B>>,
    Mlp<Autodiff<B>>: AutodiffModule<Autodiff<B>, InnerModule = Mlp<B>>,
{
    train_on_device_with(df, model_name, layers, config, device, true)
}

/// Like [`train_on_device`], but does not persist the checkpoint — used by the
/// sweep grid so only the winning combination is written (issue D1/D3).
pub fn train_on_device_unpersisted<B>(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
    device: &Device<B>,
) -> Result<TrainedModel, String>
where
    B: Backend,
    Autodiff<B>: AutodiffBackend,
    Autodiff<B>: BackendTypes<Device = Device<B>>,
    Mlp<Autodiff<B>>: AutodiffModule<Autodiff<B>, InnerModule = Mlp<B>>,
{
    train_on_device_with(df, model_name, layers, config, device, false)
}

/// Shared device-training core: train on `device`, then materialise the portable
/// CPU artifact in memory. `persist` controls the on-disk checkpoint write.
fn train_on_device_with<B>(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
    device: &Device<B>,
    persist: bool,
) -> Result<TrainedModel, String>
where
    B: Backend,
    Autodiff<B>: AutodiffBackend,
    Autodiff<B>: BackendTypes<Device = Device<B>>,
    Mlp<Autodiff<B>>: AutodiffModule<Autodiff<B>, InnerModule = Mlp<B>>,
{
    let raw = train_impl::<B>(df, model_name, layers, config, device, persist)?;
    materialize_cpu_model(raw, layers)
}

/// Transfers a device-trained `Mlp<B>` to the portable CPU artifact **in
/// memory** via Burn's bincode byte recorder, instead of reloading the
/// pretty-JSON checkpoint from disk (issue D1/D2). The on-disk checkpoint, when
/// written, stays the pretty-JSON format consumed by `predict`.
fn materialize_cpu_model<B: Backend>(
    raw: RawTrained<B>,
    layers: &[LayerKind],
) -> Result<TrainedModel, String> {
    let RawTrained {
        model,
        report,
        feature_names,
        fmean,
        fstd,
        target,
    } = raw;
    let recorder = BinBytesRecorder::<FullPrecisionSettings>::default();
    let bytes = recorder.record(model.into_record(), ()).map_err(|e| {
        format!(
            "{}: {e}",
            tr("checkpoint transfer failed", "체크포인트 전송 실패")
        )
    })?;
    let device: Device<NdArray<f32>> = Default::default();
    let record: <Mlp<NdArray<f32>> as Module<NdArray<f32>>>::Record =
        recorder.load(bytes, &device).map_err(|e| {
            format!(
                "{}: {e}",
                tr(
                    "checkpoint transfer load failed",
                    "체크포인트 전송 로드 실패"
                )
            )
        })?;
    let template = build_mlp::<NdArray<f32>>(layers, report.input_dim, &device)?;
    let mut model = template.load_record(record);
    model.training = false;
    Ok(TrainedModel {
        model,
        report,
        layers: layers.to_vec(),
        feature_names,
        fmean,
        fstd,
        target,
    })
}

/// Backend-agnostic training core: trains `Mlp<B>` and returns its report and stats.
///
/// `persist` controls whether the checkpoint (Burn record) and its sidecar
/// manifest are written to disk. The sweep grid passes `false` for every
/// combination and materialises only the winner, so a GPU provider does not pay
/// a checkpoint save per combination (issue D1/D3).
fn train_impl<B>(
    df: &DataFrame,
    model_name: &str,
    layers: &[LayerKind],
    config: &TrainConfig,
    device: &Device<B>,
    persist: bool,
) -> Result<RawTrained<B>, String>
where
    B: Backend,
    Autodiff<B>: AutodiffBackend,
    Autodiff<B>: BackendTypes<Device = Device<B>>,
    Mlp<Autodiff<B>>: AutodiffModule<Autodiff<B>, InnerModule = Mlp<B>>,
{
    let (feature_names, features, targets) = extract_data(df, &config.target)?;
    let n = features.len();
    let input_dim = feature_names.len();
    // Embedding-first models consume raw category indices — z-score normalization
    // would destroy category identity, so keep raw values (NaN → 0).
    let raw_input = matches!(layers.first(), Some(LayerKind::Embedding { .. }));
    if input_dim == 0 {
        return Err(tr(
            "No numeric feature columns available for training.",
            "학습 가능한 숫자형 특성(컬럼)이 없습니다.",
        )
        .into());
    }
    if n == 0 {
        return Err(tr("Training data is empty.", "학습 데이터가 비어 있습니다.").into());
    }

    // ── Feature standardization statistics (NaN → mean imputation) ────────────────
    let mut fmean = vec![0f64; input_dim];
    let mut fstd = vec![1f64; input_dim];
    for (j, mean) in fmean.iter_mut().enumerate().take(input_dim) {
        let (mut s, mut c) = (0f64, 0usize);
        for i in 0..n {
            if let Some(v) = features.get(i).and_then(|row| row.get(j)) {
                let v = *v as f64;
                if v.is_finite() {
                    s += v;
                    c += 1;
                }
            }
        }
        *mean = if c > 0 { s / c as f64 } else { 0.0 };
    }
    for j in 0..input_dim {
        let (mut s, mut c) = (0f64, 0usize);
        for i in 0..n {
            if let Some(v) = features.get(i).and_then(|row| row.get(j)) {
                let d = *v as f64 - fmean[j];
                if d.is_finite() {
                    s += d * d;
                    c += 1;
                }
            }
        }
        fstd[j] = if c > 1 {
            (s / (c - 1) as f64).max(1e-8).sqrt()
        } else {
            1.0
        };
    }

    let mut xs = Vec::with_capacity(n * input_dim);
    for row in features.iter().take(n) {
        for j in 0..input_dim {
            let v = row[j] as f64;
            if raw_input {
                xs.push(if v.is_finite() { v as f32 } else { 0.0 });
            } else {
                let v = if v.is_finite() { v } else { fmean[j] };
                xs.push(((v - fmean[j]) / fstd[j]) as f32);
            }
        }
    }
    if raw_input {
        check_embedding_indices(layers, &xs, input_dim);
    }

    let tmean: f64 = {
        let (mut s, mut c) = (0f64, 0usize);
        for &t in &targets {
            if t.is_finite() {
                s += t as f64;
                c += 1;
            }
        }
        if c > 0 { s / c as f64 } else { 0.0 }
    };
    let ys: Vec<f32> = targets
        .iter()
        .map(|&t| if t.is_finite() { t } else { tmean as f32 })
        .collect();

    // ── train / validation split ────────────────────────────────────────────
    let val_split = config
        .validation_split
        .unwrap_or(0.0)
        .clamp(0.0, MAX_VALIDATION_SPLIT);
    let val_n = (n as f64 * val_split) as usize;
    let train_n = n - val_n;
    let val_idx: Vec<usize> = (train_n..n).collect();

    let device_autodiff: Device<Autodiff<B>> = device.clone();
    let mut model = build_mlp::<Autodiff<B>>(layers, input_dim, &device_autodiff)?;
    // A regression target is a single scalar; a model that ends in Conv1d/Embedding
    // (or a multi-unit Dense without a final Dense(1)) produces several outputs.
    // Fail closed instead of silently broadcasting the target (which then breaks
    // predict() with a column-length error).
    if model.out_dim != 1 {
        return Err(if is_korean() {
            format!(
                "모델 '{model_name}' 의 출력 차원이 {} 입니다. 회귀 타겟은 스칼라 하나여야 합니다. 마지막에 Dense(1) 을 추가하세요.",
                model.out_dim
            )
        } else {
            format!(
                "Model '{model_name}' outputs {} values; a regression target must be a single scalar. Add a final Dense(1).",
                model.out_dim
            )
        });
    }

    let batch_size = config.batch_size.unwrap_or(train_n.max(1));
    let lr = config.learning_rate;
    let mut optim = AdamConfig::new().init::<Autodiff<B>, _>();

    let make_batch = |idx: &[usize]| -> Option<TrainBatch<B>> {
        if idx.is_empty() {
            return None;
        }
        let b = idx.len();
        let mut xv = Vec::with_capacity(b * input_dim);
        let mut yv = Vec::with_capacity(b);
        for &i in idx {
            for j in 0..input_dim {
                xv.push(xs[i * input_dim + j]);
            }
            yv.push(ys[i]);
        }
        let x = Tensor::<Autodiff<B>, 2>::from_data(
            TensorData::new(xv, [b, input_dim]),
            &device_autodiff,
        );
        let y = Tensor::<Autodiff<B>, 2>::from_data(TensorData::new(yv, [b, 1]), &device_autodiff);
        Some((x, y))
    };

    let mut final_train_loss = f64::NAN;
    let mut final_val_loss: Option<f64> = None;

    // Early stopping state (issue D3) — needs a validation split.
    let patience = config.early_stopping_patience.filter(|p| *p > 0);
    let mut best_val_loss = f64::INFINITY;
    let mut best_epoch: usize = 0;
    let mut epochs_no_improve = 0usize;
    let mut stopped_early = false;

    for epoch in 0..config.epochs {
        // Deterministic shuffle (epoch seed)
        let mut order: Vec<usize> = (0..train_n).collect();
        let mut seed = epoch as u64 + 1;
        for i in (1..order.len()).rev() {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let j = (seed >> 33) as usize % (i + 1);
            order.swap(i, j);
        }

        let mut epoch_loss = 0f64;
        let mut steps = 0usize;
        for chunk in order.chunks(batch_size) {
            let (x, y) = match make_batch(chunk) {
                Some(v) => v,
                None => continue,
            };
            let out = model.forward(x);
            let loss = ((out - y).powf_scalar(2.0)).mean();
            let loss_val = loss
                .clone()
                .into_data()
                .to_vec::<f32>()
                .map(|v| v[0] as f64)
                .unwrap_or(f64::NAN);

            let grads = loss.backward();
            let grads = GradientsParams::from_grads(grads, &model);
            model = optim.step(lr, model, grads);

            epoch_loss += loss_val;
            steps += 1;
        }
        final_train_loss = if steps > 0 {
            epoch_loss / steps as f64
        } else {
            f64::NAN
        };

        if !val_idx.is_empty()
            && let Some((xv, yv)) = make_batch(&val_idx)
        {
            let vout = model.forward(xv);
            let vloss = ((vout - yv).powf_scalar(2.0)).mean();
            final_val_loss = vloss.into_data().to_vec::<f32>().map(|v| v[0] as f64).ok();
        }

        // Early stopping: track the best validation loss and count epochs
        // without improvement (issue D3).
        if let Some(v) = final_val_loss {
            if v < best_val_loss {
                best_val_loss = v;
                best_epoch = epoch + 1;
                epochs_no_improve = 0;
            } else {
                epochs_no_improve += 1;
            }
        }

        let val_line = final_val_loss
            .map(|v| format!("  val_loss = {v:.6}"))
            .unwrap_or_default();
        println!(
            "  [Epoch {:>3}/{}]  train_loss = {final_train_loss:.6}{val_line}",
            epoch + 1,
            config.epochs
        );

        if let Some(p) = patience
            && !val_idx.is_empty()
            && epochs_no_improve >= p
        {
            println!(
                "  [Early stop] no val_loss improvement for {p} epoch(s); \
                     best epoch {best_epoch} (val_loss = {best_val_loss:.6})"
            );
            stopped_early = true;
            break;
        }
    }

    // ── Sample predictions (in-sample) ──────────────────────────────────────
    let n_pred = n.min(10);
    let device_plain: Device<B> = device.clone();
    let mut xv = Vec::with_capacity(n_pred * input_dim);
    for i in 0..n_pred {
        for j in 0..input_dim {
            xv.push(xs[i * input_dim + j]);
        }
    }
    let xp = Tensor::<B, 2>::from_data(TensorData::new(xv, [n_pred, input_dim]), &device_plain);
    let mut valid_model: Mlp<B> = model.valid();
    valid_model.training = false;
    let pred_t = valid_model.forward(xp);
    let preds = pred_t.into_data().to_vec::<f32>().unwrap_or_default();
    let predictions: Vec<f64> = preds.iter().map(|&v| v as f64).collect();
    let targets_out: Vec<f64> = (0..n_pred).map(|i| ys[i] as f64).collect();

    // ── Full-set regression metrics (D3 sweep metrics) ──────────────────────
    // MAE/R² are evaluated over every row (not just the sample above) so a sweep
    // can select its winner by a metric other than MSE.
    let mut xall = Vec::with_capacity(n * input_dim);
    for i in 0..n {
        for j in 0..input_dim {
            xall.push(xs[i * input_dim + j]);
        }
    }
    let xall_t = Tensor::<B, 2>::from_data(TensorData::new(xall, [n, input_dim]), &device_plain);
    let all_preds = valid_model.forward(xall_t).into_data().to_vec::<f32>();
    let (final_train_mae, final_train_r2, final_val_mae, final_val_r2) = match all_preds {
        Ok(all_preds) if all_preds.len() == n => {
            let (train_mae, train_r2) = regression_metrics(&all_preds[..train_n], &ys[..train_n]);
            if val_idx.is_empty() {
                (train_mae, train_r2, None, None)
            } else {
                let vp: Vec<f32> = val_idx.iter().map(|&i| all_preds[i]).collect();
                let vt: Vec<f32> = val_idx.iter().map(|&i| ys[i]).collect();
                let (val_mae, val_r2) = regression_metrics(&vp, &vt);
                (train_mae, train_r2, Some(val_mae), Some(val_r2))
            }
        }
        _ => (f64::NAN, f64::NAN, None, None),
    };

    let num_params = model.num_params();
    let output_dim = model.out_dim;

    // ── Checkpoint save ────────────────────────────────────────────────────
    let ckpt_dir = "checkpoints";
    std::fs::create_dir_all(ckpt_dir).map_err(|e| {
        format!(
            "{}: {e}",
            tr(
                "failed to create checkpoints/ directory",
                "checkpoints/ 디렉토리 생성 실패"
            )
        )
    })?;
    let ckpt = format!("{ckpt_dir}/{model_name}");
    if persist {
        let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
        valid_model
            .clone()
            .save_file(&ckpt, &recorder)
            .map_err(|e| {
                format!(
                    "{}: {e}",
                    tr("checkpoint save failed", "체크포인트 저장 실패")
                )
            })?;
    }

    let report = TrainReport {
        model_name: model_name.to_string(),
        target: config.target.clone(),
        feature_names: feature_names.clone(),
        input_dim,
        output_dim,
        num_params,
        epochs: config.epochs,
        batch_size,
        learning_rate: lr as f32,
        final_train_loss,
        final_val_loss,
        predictions,
        targets: targets_out,
        checkpoint_path: format!("{ckpt}.json"),
        checkpoint_format_version: CHECKPOINT_FORMAT_VERSION,
        stopped_early,
        best_epoch,
        final_train_mae,
        final_val_mae,
        final_train_r2,
        final_val_r2,
    };

    // Versioned sidecar manifest (D3) — written next to the Burn record so a
    // later load can validate format compatibility and model shape. Skipped for
    // unpersisted sweep combinations; the winner is written once by `sweep`.
    if persist {
        let manifest = CheckpointManifest::from_report(&report, layers);
        save_checkpoint_manifest(&report.checkpoint_path, &manifest)?;
    }

    Ok(RawTrained {
        model: valid_model,
        report,
        feature_names,
        fmean,
        fstd,
        target: config.target.clone(),
    })
}

/// Shared inference preprocessing: validates the frame, reads the feature columns
/// in training order, and applies the same standardization used during training.
/// Returns `(xs, rows, feature_count)`.
fn prepare_inference_input(
    trained: &TrainedModel,
    df: &DataFrame,
) -> Result<(Vec<f32>, usize, usize), String> {
    let feature_count = trained.feature_names.len();
    let n = df.height();
    if feature_count == 0 {
        return Err(tr(
            "The model has no feature information. Train it first with train().",
            "모델에 특성 정보가 없습니다. 먼저 train()으로 학습하세요.",
        )
        .into());
    }
    if n == 0 {
        return Err(tr("No data to predict on.", "예측할 데이터가 비어 있습니다.").into());
    }

    let mut col_vecs: Vec<Vec<f32>> = Vec::with_capacity(feature_count);
    for j in 0..feature_count {
        let name = &trained.feature_names[j];
        let col = df.column(name.as_str()).map_err(|e| {
            format!(
                "{} '{name}': {e}",
                tr("prediction feature column access failed", "예측 특성 컬럼")
            )
        })?;
        col_vecs.push(series_to_f32(col));
    }

    let mut xs = Vec::with_capacity(n * feature_count);
    for i in 0..n {
        for (j, col) in col_vecs.iter().enumerate().take(feature_count) {
            let v = col[i] as f64;
            if trained.model.raw_input {
                xs.push(if v.is_finite() { v as f32 } else { 0.0 });
            } else {
                let v = if v.is_finite() { v } else { trained.fmean[j] };
                xs.push(((v - trained.fmean[j]) / trained.fstd[j]) as f32);
            }
        }
    }
    if trained.model.raw_input {
        check_embedding_indices(&trained.layers, &xs, feature_count);
    }
    Ok((xs, n, feature_count))
}

/// Appends the prediction column to a clone of `df`.
fn attach_prediction(
    trained: &TrainedModel,
    df: &DataFrame,
    preds: &[f32],
    as_col: Option<&str>,
) -> Result<DataFrame, String> {
    let out_col = match as_col {
        Some(c) => c.to_string(),
        None => format!("{}_pred", trained.target),
    };
    let pred_f64: Vec<f64> = preds.iter().map(|&v| v as f64).collect();

    let mut out = df.clone();
    out.with_column(Column::new(out_col.into(), pred_f64))
        .map_err(|e| {
            format!(
                "{}: {e}",
                tr("prediction column add failed", "예측 컬럼 추가 실패")
            )
        })?;
    Ok(out)
}

/// Default number of rows per inference chunk.
///
/// Uploading `[n, feature_count]` in one tensor can exhaust device memory (or
/// stall on a single large transfer) for big frames. Chunking bounds the peak
/// device footprint without changing the result. Override with
/// `XAZZ_INFER_CHUNK` (0 = upload everything at once).
const DEFAULT_INFER_CHUNK: usize = 4096;

/// Rows per inference chunk from `XAZZ_INFER_CHUNK` (default
/// [`DEFAULT_INFER_CHUNK`]; `0` disables chunking).
pub(crate) fn infer_chunk_size() -> usize {
    std::env::var("XAZZ_INFER_CHUNK")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_INFER_CHUNK)
}

/// Splits `n` rows into `[start, end)` ranges of at most `chunk` rows. A `chunk`
/// of 0 yields a single range covering all rows.
pub(crate) fn chunk_ranges(n: usize, chunk: usize) -> Vec<(usize, usize)> {
    if n == 0 {
        return Vec::new();
    }
    let step = if chunk == 0 { n } else { chunk };
    let mut ranges = Vec::new();
    let mut start = 0;
    while start < n {
        let end = (start + step).min(n);
        ranges.push((start, end));
        start = end;
    }
    ranges
}

/// Runs the forward pass over `xs` (row-major `[n, feature_count]`) in chunks and
/// returns every prediction in input order.
fn forward_predictions<B: Backend>(
    model: &Mlp<B>,
    xs: &[f32],
    n: usize,
    feature_count: usize,
    device: &Device<B>,
) -> Vec<f32> {
    let mut preds = Vec::with_capacity(n);
    for (start, end) in chunk_ranges(n, infer_chunk_size()) {
        let rows = end - start;
        let slice = xs[start * feature_count..end * feature_count].to_vec();
        let x = Tensor::<B, 2>::from_data(TensorData::new(slice, [rows, feature_count]), device);
        preds.extend(
            model
                .forward(x)
                .into_data()
                .to_vec::<f32>()
                .unwrap_or_default(),
        );
    }
    preds
}

/// `dataset |> predict(model_var, as: "col")` — adds a prediction column using the trained model.
///
/// Default prediction column name: `<target>_pred`. Runs on the CPU reference
/// backend using the in-memory model.
pub fn predict(
    trained: &TrainedModel,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<DataFrame, String> {
    let (xs, n, feature_count) = prepare_inference_input(trained, df)?;

    let device: Device<Plain> = Default::default();
    let mut infer_model = trained.model.clone();
    infer_model.training = false;
    let preds = forward_predictions(&infer_model, &xs, n, feature_count, &device);

    attach_prediction(trained, df, &preds, as_col)
}

/// `predict` on an arbitrary Burn backend `B`.
///
/// Rebuilds the declared module graph on `B` and loads the portable checkpoint
/// saved during training, so a GPU provider runs the forward pass on its own
/// device. The checkpoint is backend-neutral, so the same artifact can be
/// evaluated on CPU and GPU with matching predictions.
pub fn predict_on<B>(
    trained: &TrainedModel,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<DataFrame, String>
where
    B: Backend,
{
    predict_on_device::<B>(trained, df, as_col, &Default::default())
}

/// Loads the portable checkpoint into an inference module on `device`.
///
/// Validates the sidecar manifest (fail-closed on a newer format), rebuilds the
/// declared graph on `B`, and loads the backend-neutral record. Split out from
/// [`predict_on_device`] so a provider can cache the loaded module and skip the
/// disk round-trip on subsequent predictions (issue D1/D2).
pub fn load_inference_model<B>(trained: &TrainedModel, device: &Device<B>) -> Result<Mlp<B>, String>
where
    B: Backend,
{
    if trained.feature_names.is_empty() {
        return Err(tr(
            "The model has no feature information. Train it first with train().",
            "모델에 특성 정보가 없습니다. 먼저 train()으로 학습하세요.",
        )
        .into());
    }

    // Fail closed on a checkpoint from a newer Xazz when a manifest is present;
    // legacy checkpoints without one still load through the Burn record.
    load_checkpoint_manifest(trained.report.checkpoint_path.as_str())?;

    let template = build_mlp::<B>(&trained.layers, trained.feature_names.len(), device)?;
    let recorder = PrettyJsonFileRecorder::<FullPrecisionSettings>::new();
    let mut infer_model = template
        .load_file(trained.report.checkpoint_path.as_str(), &recorder, device)
        .map_err(|e| {
            format!(
                "{}: {e}",
                tr("checkpoint load failed", "체크포인트 로드 실패")
            )
        })?;
    infer_model.training = false;
    Ok(infer_model)
}

/// Runs inference with an already-loaded module — no checkpoint disk access.
///
/// `device` must be the device `model` lives on. Preprocessing (feature order,
/// standardization) is identical to [`predict`], so a cached module produces the
/// same predictions.
pub fn predict_with_model<B>(
    trained: &TrainedModel,
    model: &Mlp<B>,
    device: &Device<B>,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<DataFrame, String>
where
    B: Backend,
{
    let (xs, n, feature_count) = prepare_inference_input(trained, df)?;

    let preds = forward_predictions(model, &xs, n, feature_count, device);

    attach_prediction(trained, df, &preds, as_col)
}

/// Like [`predict_on`], but evaluates on an explicit device `B::Device`.
///
/// Providers pass their device so the forward pass runs on the intended
/// accelerator. The checkpoint is loaded on each call; providers that predict
/// repeatedly should cache via
/// [`load_inference_model`] + [`predict_with_model`].
pub fn predict_on_device<B>(
    trained: &TrainedModel,
    df: &DataFrame,
    as_col: Option<&str>,
    device: &Device<B>,
) -> Result<DataFrame, String>
where
    B: Backend,
{
    let model = load_inference_model::<B>(trained, device)?;
    predict_with_model::<B>(trained, &model, device, df, as_col)
}

/// Layered model registry helper: <model name, LayerKind list>.
pub type ModelRegistry = HashMap<String, Vec<LayerKind>>;

#[cfg(test)]
mod tests {
    use super::*;
    use xazz_compiler::ast::EmbeddingVocab;

    fn embedding_layer(vocab: EmbeddingVocab) -> LayerKind {
        LayerKind::Embedding {
            vocab,
            embed_dim: 2,
        }
    }

    #[test]
    fn chunk_ranges_splits_and_handles_edges() {
        // Exact multiple, remainder, single chunk, and disabled (0).
        assert_eq!(chunk_ranges(6, 2), vec![(0, 2), (2, 4), (4, 6)]);
        assert_eq!(chunk_ranges(5, 2), vec![(0, 2), (2, 4), (4, 5)]);
        assert_eq!(chunk_ranges(3, 10), vec![(0, 3)]);
        assert_eq!(chunk_ranges(3, 0), vec![(0, 3)]);
        assert_eq!(chunk_ranges(0, 2), Vec::<(usize, usize)>::new());
    }

    #[test]
    fn lru_cache_promotes_and_evicts() {
        let mut cache: LruCache<u32, String> = LruCache::new(2);
        assert_eq!(cache.get_or_insert_with(1, || Ok("a".into())).unwrap(), "a");
        assert_eq!(cache.get_or_insert_with(2, || Ok("b".into())).unwrap(), "b");
        // Touch 1 so it becomes most-recently-used, then insert 3 → 2 is evicted.
        assert_eq!(
            cache.get_or_insert_with(1, || Ok("reload".into())).unwrap(),
            "a"
        );
        assert_eq!(cache.get_or_insert_with(3, || Ok("c".into())).unwrap(), "c");
        assert_eq!(cache.len(), 2);
        // 2 was evicted, so its loader runs again.
        assert_eq!(
            cache.get_or_insert_with(2, || Ok("b2".into())).unwrap(),
            "b2"
        );
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn lru_cache_minimum_capacity_is_one() {
        let mut cache: LruCache<u32, u32> = LruCache::new(0);
        assert_eq!(*cache.get_or_insert_with(1, || Ok(10)).unwrap(), 10);
        assert_eq!(*cache.get_or_insert_with(2, || Ok(20)).unwrap(), 20);
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn regression_metrics_mae_and_r2() {
        // Perfect predictions: MAE 0, R² 1.
        let (mae, r2) = regression_metrics(&[1.0, 2.0, 3.0], &[1.0, 2.0, 3.0]);
        assert!(mae.abs() < 1e-12, "완벽 예측 MAE: {mae}");
        assert!((r2 - 1.0).abs() < 1e-12, "완벽 예측 R²: {r2}");

        // Constant offset of 1 on [0, 2, 4]: MAE 1, R² 1 - (3)/(8) = 0.625.
        let (mae, r2) = regression_metrics(&[1.0, 3.0, 5.0], &[0.0, 2.0, 4.0]);
        assert!((mae - 1.0).abs() < 1e-12, "MAE: {mae}");
        assert!((r2 - 0.625).abs() < 1e-12, "R²: {r2}");

        // Zero-variance targets: R² is defined as 0.0, not NaN.
        let (mae, r2) = regression_metrics(&[5.0, 5.0], &[3.0, 3.0]);
        assert!((mae - 2.0).abs() < 1e-12, "MAE: {mae}");
        assert_eq!(r2, 0.0);

        // Empty input is NaN, never a panic.
        let (mae, r2) = regression_metrics(&[], &[]);
        assert!(mae.is_nan() && r2.is_nan());
    }

    #[test]
    fn sweep_score_respects_selected_metric() {
        let mk = |val_loss: f64,
                  train_loss: f64,
                  val_mae: Option<f64>,
                  train_mae: f64,
                  val_r2: Option<f64>,
                  train_r2: f64| SweepCombo {
            epochs: 1,
            batch_size: 1,
            learning_rate: 0.01,
            final_train_loss: train_loss,
            final_val_loss: Some(val_loss),
            stopped_early: false,
            best_epoch: 1,
            train_mae,
            val_mae,
            train_r2,
            val_r2,
            selected: false,
        };

        // a has the better MSE, b has the better MAE and R².
        let a = mk(0.10, 0.20, Some(0.50), 0.50, Some(0.10), 0.10);
        let b = mk(0.30, 0.40, Some(0.20), 0.20, Some(0.90), 0.90);

        assert!(
            SweepReport::score(&a, SweepMetric::Mse) < SweepReport::score(&b, SweepMetric::Mse)
        );
        assert!(
            SweepReport::score(&b, SweepMetric::Mae) < SweepReport::score(&a, SweepMetric::Mae)
        );
        // R² is negated so the higher value wins the minimisation.
        assert!(SweepReport::score(&b, SweepMetric::R2) < SweepReport::score(&a, SweepMetric::R2));

        // Without a validation split the training-split metric is the fallback.
        let mut no_val = b.clone();
        no_val.final_val_loss = None;
        no_val.val_mae = None;
        no_val.val_r2 = None;
        assert_eq!(SweepReport::score(&no_val, SweepMetric::Mae), 0.20);
        assert_eq!(SweepReport::score(&no_val, SweepMetric::R2), -0.90);

        // A non-finite metric ranks last.
        let mut nan = b.clone();
        nan.val_mae = Some(f64::NAN);
        nan.train_mae = f64::NAN;
        assert_eq!(SweepReport::score(&nan, SweepMetric::Mae), f64::INFINITY);
    }

    #[test]
    fn sweep_compare_orders_by_requested_axis() {
        let mk = |epochs: usize, batch_size: usize, learning_rate: f32, val_loss: f64| SweepCombo {
            epochs,
            batch_size,
            learning_rate,
            final_train_loss: val_loss,
            final_val_loss: Some(val_loss),
            stopped_early: false,
            best_epoch: 1,
            train_mae: val_loss,
            val_mae: Some(val_loss),
            train_r2: 0.0,
            val_r2: Some(0.0),
            selected: false,
        };
        let a = mk(3, 8, 0.05, 0.10);
        let b = mk(2, 4, 0.01, 0.30);

        // Axis sorts are ascending by that hyperparameter.
        assert_eq!(
            SweepReport::compare(&b, &a, SweepSort::Epochs, SweepMetric::Mse, &[]),
            Ordering::Less
        );
        assert_eq!(
            SweepReport::compare(&b, &a, SweepSort::Lr, SweepMetric::Mse, &[]),
            Ordering::Less
        );
        assert_eq!(
            SweepReport::compare(&b, &a, SweepSort::Batch, SweepMetric::Mse, &[]),
            Ordering::Less
        );
        // Metric sort is best-first, so the lower-loss combo orders first.
        assert_eq!(
            SweepReport::compare(&a, &b, SweepSort::Metric, SweepMetric::Mse, &[]),
            Ordering::Less
        );
        // Equal on the sort axis falls through to a deterministic tiebreak.
        let c = mk(2, 4, 0.01, 0.30);
        assert_eq!(
            SweepReport::compare(&b, &c, SweepSort::Epochs, SweepMetric::Mse, &[]),
            Ordering::Equal
        );
    }

    /// A user-specified tiebreak axis replaces the canonical fallback order.
    #[test]
    fn sweep_compare_honours_explicit_tiebreak() {
        let mk = |epochs: usize, batch_size: usize, learning_rate: f32, val_loss: f64| SweepCombo {
            epochs,
            batch_size,
            learning_rate,
            final_train_loss: val_loss,
            final_val_loss: Some(val_loss),
            stopped_early: false,
            best_epoch: 1,
            train_mae: val_loss,
            val_mae: Some(val_loss),
            train_r2: 0.0,
            val_r2: Some(0.0),
            selected: false,
        };
        // Same metric (tie) but different axis values.
        let a = mk(2, 16, 0.10, 0.20);
        let b = mk(5, 4, 0.01, 0.20);

        // Canonical metric fallback breaks the tie by epochs: `a` (2) before `b` (5).
        assert_eq!(
            SweepReport::compare(&a, &b, SweepSort::Metric, SweepMetric::Mse, &[]),
            Ordering::Less
        );
        // A batch tiebreak flips the order (4 before 16).
        assert_eq!(
            SweepReport::compare(
                &a,
                &b,
                SweepSort::Metric,
                SweepMetric::Mse,
                &[SweepSort::Batch]
            ),
            Ordering::Greater
        );
        // A tiebreak equal to the sort axis is redundant, not an error.
        assert_eq!(
            SweepReport::compare(&a, &b, SweepSort::Lr, SweepMetric::Mse, &[SweepSort::Lr]),
            Ordering::Greater
        );
        // Multiple tiebreak axes are applied in order: `lr` decides before the
        // canonical `epochs` fallback would.
        let low_lr = mk(9, 4, 0.01, 0.20);
        let high_lr = mk(1, 4, 0.90, 0.20);
        assert_eq!(
            SweepReport::compare(
                &low_lr,
                &high_lr,
                SweepSort::Metric,
                SweepMetric::Mse,
                &[SweepSort::Lr, SweepSort::Batch]
            ),
            Ordering::Less
        );
    }

    #[test]
    fn leading_embedding_vocabs_expands_shared_and_per_column() {
        // A shared vocab is replicated for every input column.
        assert_eq!(
            leading_embedding_vocabs(&[embedding_layer(EmbeddingVocab::Shared(5))], 3),
            Some(vec![5, 5, 5])
        );
        // A per-column list is returned as-is when its length matches.
        assert_eq!(
            leading_embedding_vocabs(&[embedding_layer(EmbeddingVocab::PerColumn(vec![4, 7]))], 2),
            Some(vec![4, 7])
        );
        // A length mismatch cannot be diagnosed here (build_mlp rejects it).
        assert_eq!(
            leading_embedding_vocabs(&[embedding_layer(EmbeddingVocab::PerColumn(vec![4]))], 2),
            None
        );
        assert_eq!(leading_embedding_vocabs(&[LayerKind::Dense(4)], 2), None);
        assert_eq!(
            leading_embedding_vocabs(
                &[
                    LayerKind::Dense(4),
                    embedding_layer(EmbeddingVocab::Shared(5))
                ],
                2
            ),
            None,
            "Embedding 은 첫 레이어여야 한다"
        );
    }

    #[test]
    fn counts_out_of_range_per_column_against_each_vocab() {
        // 2 columns, row-major: col0 vocab 3, col1 vocab 5.
        let values = vec![0.0, 0.0, 3.0, 4.0, -1.0, 9.0];
        assert_eq!(count_out_of_range_per_column(&values, 2, &[3, 5]), 3);
        // A uniform shared vocab still checks every column.
        assert_eq!(
            count_out_of_range_per_column(&[0.0, 9.0, 2.0, 1.0], 2, &[3, 3]),
            1
        );
    }

    #[test]
    fn counts_only_out_of_range_finite_indices() {
        // [0, vocab-1] 안쪽은 세지 않는다.
        assert_eq!(count_out_of_range_indices(&[0.0, 1.0, 4.0], 5), 0);
        // vocab-1 초과
        assert_eq!(count_out_of_range_indices(&[0.0, 3.0, 4.0], 3), 2);
        // 음수 인덱스
        assert_eq!(count_out_of_range_indices(&[-1.0, 0.0], 5), 1);
        // non-finite 는 forward 에서 0 으로 매핑되므로 세지 않는다.
        assert_eq!(count_out_of_range_indices(&[f32::NAN, f32::INFINITY], 5), 0);
    }

    #[test]
    fn counts_only_non_integer_finite_indices() {
        // 정수값(소수부 0)은 세지 않는다.
        assert_eq!(count_non_integer_indices(&[0.0, 1.0, 4.0]), 0);
        // 소수부가 있는 연속형 값은 truncate 되므로 센다.
        assert_eq!(count_non_integer_indices(&[0.5, 1.0, 2.25]), 2);
        // 음수 비정수도 소수부가 버려진다.
        assert_eq!(count_non_integer_indices(&[-0.5, 3.0]), 1);
        // non-finite 는 forward 에서 0 으로 매핑되므로 세지 않는다.
        assert_eq!(count_non_integer_indices(&[f32::NAN, f32::INFINITY]), 0);
    }

    // ── Checkpoint versioning (D3) ──────────────────────────────────────────

    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Isolated temp directory per test (avoids the shared `checkpoints/` dir).
    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let n = TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("xazz_dl_{tag}_{}_{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn sample_report() -> TrainReport {
        TrainReport {
            model_name: "ManifestMlp".to_string(),
            target: "y".to_string(),
            feature_names: vec!["x1".to_string(), "x2".to_string()],
            input_dim: 2,
            output_dim: 1,
            num_params: 17,
            epochs: 3,
            batch_size: 2,
            learning_rate: 0.05,
            final_train_loss: 0.25,
            final_val_loss: Some(0.3),
            predictions: vec![1.0],
            targets: vec![1.0],
            checkpoint_path: "checkpoints/ManifestMlp.json".to_string(),
            checkpoint_format_version: CHECKPOINT_FORMAT_VERSION,
            stopped_early: false,
            best_epoch: 0,
            final_train_mae: 0.4,
            final_val_mae: Some(0.5),
            final_train_r2: 0.9,
            final_val_r2: Some(0.85),
        }
    }

    #[test]
    fn checkpoint_manifest_round_trips_with_version_and_layers() {
        let dir = temp_dir("manifest");
        let ckpt = dir.join("ManifestMlp.json").to_string_lossy().to_string();
        let layers = vec![LayerKind::Dense(4), LayerKind::ReLU, LayerKind::Dense(1)];
        let manifest = CheckpointManifest::from_report(&sample_report(), &layers);

        save_checkpoint_manifest(&ckpt, &manifest).expect("write manifest");
        assert_eq!(
            manifest_path(&ckpt),
            format!("{}.meta.json", ckpt.trim_end_matches(".json"))
        );

        let loaded = load_checkpoint_manifest(&ckpt)
            .expect("read manifest")
            .expect("manifest present");
        assert_eq!(loaded, manifest);
        assert_eq!(loaded.format_version, CHECKPOINT_FORMAT_VERSION);
        assert_eq!(loaded.layers, vec!["Dense(4)", "ReLU", "Dense(1)"]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_manifest_rejects_newer_format() {
        let dir = temp_dir("newer");
        let ckpt = dir.join("Newer.json").to_string_lossy().to_string();
        let mut manifest =
            CheckpointManifest::from_report(&sample_report(), &[LayerKind::Dense(1)]);
        manifest.format_version = CHECKPOINT_FORMAT_VERSION + 1;
        save_checkpoint_manifest(&ckpt, &manifest).expect("write manifest");

        let err = load_checkpoint_manifest(&ckpt).expect_err("newer format must be rejected");
        assert!(err.contains("newer") || err.contains("최신"), "{err}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn checkpoint_manifest_absent_is_legacy() {
        let dir = temp_dir("legacy");
        let ckpt = dir.join("Absent.json").to_string_lossy().to_string();
        assert_eq!(load_checkpoint_manifest(&ckpt).expect("no error"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── Artifact cache key (D1/D2 predict caching) ──────────────────────────

    #[test]
    fn artifact_key_missing_file_is_zeroed() {
        let dir = temp_dir("key_missing");
        let path = dir.join("nope.json").to_string_lossy().to_string();
        let key = artifact_key(&path);
        assert_eq!(key.path, path);
        assert_eq!(key.modified_nanos, 0);
        assert_eq!(key.len, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn artifact_key_tracks_content_changes() {
        let dir = temp_dir("key_change");
        let path = dir.join("w.json").to_string_lossy().to_string();

        std::fs::write(&path, b"one").expect("write");
        let first = artifact_key(&path);
        assert_eq!(first.len, 3);

        // Rewriting different-length content must change the key.
        std::fs::write(&path, b"two-longer").expect("rewrite");
        let second = artifact_key(&path);
        assert_eq!(second.len, 10);
        assert_ne!(
            first, second,
            "content change must invalidate the cache key"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
