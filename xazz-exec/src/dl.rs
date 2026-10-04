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

use crate::classification::decode_logits;
use burn::nn::loss::CrossEntropyLossConfig;
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

pub use crate::split::select_split_indices;
use crate::split::{SplitReport, time_order_by_column};
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

/// Structured mirror of the embedding-input warnings. The forward pass clamps
/// out-of-range indices and truncates fractional ones silently, so these counts
/// are surfaced in [`TrainReport`] and the `predict` diagnostics (and thus
/// `--json`) for machine-readable consumption in addition to the stderr
/// diagnostics (issue D3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct EmbeddingDiagnostics {
    /// Finite inputs below 0 or above `vocab_size - 1` (clamped in the forward pass).
    #[serde(rename = "embedding_out_of_range")]
    pub out_of_range: usize,
    /// Finite inputs with a fractional part (truncated to an index in the forward pass).
    #[serde(rename = "embedding_non_integer")]
    pub non_integer: usize,
}

/// Runs the out-of-range embedding diagnostic when the model consumes raw indices,
/// emitting the stderr warnings and returning the counts for [`TrainReport`].
fn check_embedding_indices(
    layers: &[LayerKind],
    values: &[f32],
    feature_count: usize,
) -> EmbeddingDiagnostics {
    let Some(vocabs) = leading_embedding_vocabs(layers, feature_count) else {
        return EmbeddingDiagnostics::default();
    };
    let out_of_range = count_out_of_range_per_column(values, feature_count, &vocabs);
    if out_of_range > 0 {
        warn_embedding_out_of_range(out_of_range, &vocabs);
    }
    let non_integer = count_non_integer_indices(values);
    if non_integer > 0 {
        warn_embedding_non_integer(non_integer);
    }
    EmbeddingDiagnostics {
        out_of_range,
        non_integer,
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
    pub validation: SplitReport,
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
    /// Raw embedding inputs outside `[0, vocab_size - 1]` (clamped silently in the
    /// forward pass) — structured mirror of the stderr warning (issue D3).
    #[serde(default)]
    pub embedding_out_of_range: usize,
    /// Finite raw embedding inputs with a fractional part (truncated to an index
    /// in the forward pass) — structured mirror of the stderr warning (issue D3).
    #[serde(default)]
    pub embedding_non_integer: usize,
    /// Classification metrics when the target is categorical (issue #163).
    /// `None` for regression runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<ClassificationMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_classification: Option<ClassificationMetrics>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub class_labels: Vec<f32>,
}

pub use crate::classification::{
    ClassificationMetrics, classification_metrics, detect_classification,
};

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
    /// Raw embedding inputs outside `[0, vocab_size - 1]` (clamped silently in
    /// the forward pass) — per-combination mirror of [`TrainReport`] (issue D3).
    #[serde(default)]
    pub embedding_out_of_range: usize,
    /// Finite raw embedding inputs with a fractional part (truncated to an index
    /// in the forward pass) — per-combination mirror of [`TrainReport`] (D3).
    #[serde(default)]
    pub embedding_non_integer: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub classification: Option<ClassificationMetrics>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validation_classification: Option<ClassificationMetrics>,
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
            SweepMetric::Mse | SweepMetric::CrossEntropy => {
                (combo.final_val_loss, combo.final_train_loss)
            }
            SweepMetric::Mae => (combo.val_mae, combo.train_mae),
            SweepMetric::R2 => (combo.val_r2, combo.train_r2),
            metric => {
                let metrics = combo
                    .validation_classification
                    .as_ref()
                    .or(combo.classification.as_ref());
                let value = metrics.and_then(|m| match metric {
                    SweepMetric::Accuracy => Some(m.accuracy),
                    SweepMetric::Precision => Some(m.precision),
                    SweepMetric::Recall => Some(m.recall),
                    SweepMetric::F1 => Some(m.f1),
                    SweepMetric::Auc => m.auc,
                    _ => None,
                });
                return value
                    .filter(|v| v.is_finite())
                    .map_or(f64::INFINITY, |v| -v);
            }
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
    #[serde(default)]
    pub class_labels: Vec<f32>,
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
            class_labels: report.class_labels.clone(),
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

    // ── train / validation split (issue #162) ───────────────────────────────
    let val_split = config.validation_split.unwrap_or(0.0);
    if !val_split.is_finite() || !(0.0..=MAX_VALIDATION_SPLIT).contains(&val_split) {
        return Err("validation_split must be finite and between 0 and 0.9".into());
    }
    if config.split_strategy == xazz_core::ast::SplitStrategy::Stratified
        || matches!(layers.last(), Some(LayerKind::Dense(width)) if *width > 1)
    {
        let target_column = df
            .column(&config.target)
            .map_err(|error| error.to_string())?;
        crate::tensor_bridge::validate_class_label_precision(target_column)?;
    }
    let time_order = match config.time_column.as_deref() {
        Some(col) if val_split > 0.0 => Some(time_order_by_column(df, col)?),
        _ => None,
    };
    let (train_idx, val_idx) = select_split_indices(
        n,
        val_split,
        config.split_strategy,
        &targets,
        time_order.as_deref(),
    )?;
    let train_n = train_idx.len();
    if let Some(column) = config.time_column.as_deref().filter(|_| val_split > 0.0) {
        crate::split::validate_time_boundary(df, column, &train_idx, &val_idx)?;
    }
    let validation = SplitReport::new(config, &train_idx, &val_idx, &targets);

    // ── Feature standardization statistics (NaN → mean imputation) ────────────────
    let mut fmean = vec![0f64; input_dim];
    let mut fstd = vec![1f64; input_dim];
    for (j, mean) in fmean.iter_mut().enumerate().take(input_dim) {
        let (mut s, mut c) = (0f64, 0usize);
        for &i in &train_idx {
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
        for &i in &train_idx {
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
    let embedding_diagnostics = if raw_input {
        check_embedding_indices(layers, &xs, input_dim)
    } else {
        EmbeddingDiagnostics::default()
    };

    let tmean: f64 = {
        let (mut s, mut c) = (0f64, 0usize);
        for &i in &train_idx {
            let t = targets[i];
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

    let device_autodiff: Device<Autodiff<B>> = device.clone();
    let mut model = build_mlp::<Autodiff<B>>(layers, input_dim, &device_autodiff)?;
    let class_labels = if model.out_dim > 1 {
        let classes = detect_classification(&targets, 1024).ok_or(
            "A regression target must be scalar (Dense(1)); classification requires finite integer labels and Dense(number_of_classes)."
        )?;
        if model.out_dim != classes.len() || !matches!(layers.last(), Some(LayerKind::Dense(_))) {
            return Err(
                "Classification requires a final Dense(number_of_classes) producing raw logits. Use Dense(1) for regression."
                    .into(),
            );
        }
        classes
    } else {
        Vec::new()
    };
    let is_classification = !class_labels.is_empty();
    if config.sweep_metric.is_classification() && !is_classification {
        return Err(
            "Classification metrics require a multi-class Dense output and integer targets".into(),
        );
    }
    if is_classification && config.sweep_metric_explicit && !config.sweep_metric.is_classification()
    {
        return Err(
            "Choose cross_entropy, accuracy, precision, recall, f1 or auc for classification"
                .into(),
        );
    }
    let classification_loss = CrossEntropyLossConfig::new().init::<Autodiff<B>>(&device_autodiff);

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
            yv.push(if is_classification {
                class_labels
                    .iter()
                    .position(|&c| c == ys[i])
                    .expect("validated class") as f32
            } else {
                ys[i]
            });
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
        let mut order: Vec<usize> = train_idx.clone();
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
            let loss = if is_classification {
                classification_loss.forward(out, y.squeeze_dims::<1>(&[1]).int())
            } else {
                ((out - y).powf_scalar(2.0)).mean()
            };
            let loss_val = loss
                .clone()
                .into_data()
                .to_vec::<f32>()
                .map_err(|error| format!("training loss conversion failed: {error:?}"))?
                .first()
                .copied()
                .ok_or("training loss tensor is empty")? as f64;
            if !loss_val.is_finite() {
                return Err("training loss is not finite".into());
            }

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
            model.training = false;
            let vout = model.forward(xv);
            model.training = true;
            let vloss = if is_classification {
                classification_loss.forward(vout, yv.squeeze_dims::<1>(&[1]).int())
            } else {
                ((vout - yv).powf_scalar(2.0)).mean()
            };
            let value = vloss
                .into_data()
                .to_vec::<f32>()
                .map_err(|error| format!("validation loss conversion failed: {error:?}"))?
                .first()
                .copied()
                .ok_or("validation loss tensor is empty")? as f64;
            if !value.is_finite() {
                return Err("validation loss is not finite".into());
            }
            final_val_loss = Some(value);
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
    let preds = pred_t
        .into_data()
        .to_vec::<f32>()
        .map_err(|error| format!("sample prediction conversion failed: {error:?}"))?;
    let preds = if is_classification {
        decode_logits(&preds, &class_labels)?.0
    } else {
        preds
    };
    let predictions: Vec<f64> = preds.iter().map(|&v| v as f64).collect();
    let targets_out: Vec<f64> = (0..n_pred).map(|i| ys[i] as f64).collect();

    // Evaluate the returned model over every row, then select the exact
    // training/validation indices for losses and regression/classification metrics.
    let mut xall = Vec::with_capacity(n * input_dim);
    for i in 0..n {
        for j in 0..input_dim {
            xall.push(xs[i * input_dim + j]);
        }
    }
    let xall_t = Tensor::<B, 2>::from_data(TensorData::new(xall, [n, input_dim]), &device_plain);
    let all_output = valid_model.forward(xall_t);
    let all_preds = all_output
        .clone()
        .into_data()
        .to_vec::<f32>()
        .map_err(|error| format!("evaluation output conversion failed: {error:?}"))?;
    if all_preds.len() != n * model.out_dim || all_preds.iter().any(|value| !value.is_finite()) {
        return Err("evaluation output must contain finite predictions for every row".into());
    }
    let (classification, validation_classification) = if is_classification {
        let logits = &all_preds;
        let (labels, scores) = decode_logits(logits, &class_labels)?;
        // 최종 보고서와 스윕은 같은 반환 모델의 평가 모드 손실을 사용한다.
        let encoded: Vec<i64> = ys
            .iter()
            .map(|target| {
                class_labels
                    .iter()
                    .position(|class| class == target)
                    .expect("validated class") as i64
            })
            .collect();
        let encoded = Tensor::<B, 1, burn::tensor::Int>::from_data(
            TensorData::new(encoded, [n]),
            &device_plain,
        );
        let evaluation_loss = CrossEntropyLossConfig::new().init::<B>(&device_plain);
        let evaluate_partition =
            |indices: &[usize]| -> Result<(f64, ClassificationMetrics), String> {
                let index_tensor = Tensor::<B, 1, burn::tensor::Int>::from_data(
                    TensorData::new(
                        indices.iter().map(|&i| i as i64).collect::<Vec<_>>(),
                        [indices.len()],
                    ),
                    &device_plain,
                );
                let loss = evaluation_loss
                    .forward(
                        all_output.clone().select(0, index_tensor.clone()),
                        encoded.clone().select(0, index_tensor),
                    )
                    .into_data()
                    .to_vec::<f32>()
                    .map_err(|e| format!("classification loss: {e:?}"))?
                    .first()
                    .copied()
                    .ok_or("evaluation loss tensor is empty")? as f64;
                if !loss.is_finite() {
                    return Err("evaluation loss is not finite".into());
                }
                let predictions: Vec<_> = indices.iter().map(|&i| labels[i]).collect();
                let targets: Vec<_> = indices.iter().map(|&i| ys[i]).collect();
                let probabilities = scores
                    .as_ref()
                    .map(|values| indices.iter().map(|&i| values[i]).collect::<Vec<_>>());
                let metrics = classification_metrics(
                    &predictions,
                    &targets,
                    &class_labels,
                    probabilities.as_deref(),
                )?;
                Ok((loss, metrics))
            };
        let (train_loss, train) = evaluate_partition(&train_idx)?;
        final_train_loss = train_loss;
        let val = if val_idx.is_empty() {
            final_val_loss = None;
            None
        } else {
            let (loss, metrics) = evaluate_partition(&val_idx)?;
            final_val_loss = Some(loss);
            Some(metrics)
        };
        if config.sweep_metric == SweepMetric::Auc && val.as_ref().unwrap_or(&train).auc.is_none() {
            return Err("AUC selection requires binary classes with both classes in the evaluation partition".into());
        }
        (Some(train), val)
    } else {
        (None, None)
    };
    let (final_train_mae, final_train_r2, final_val_mae, final_val_r2) = if !is_classification {
        let tp: Vec<f32> = train_idx.iter().map(|&i| all_preds[i]).collect();
        let tt: Vec<f32> = train_idx.iter().map(|&i| ys[i]).collect();
        let mse = |predictions: &[f32], targets: &[f32]| {
            predictions
                .iter()
                .zip(targets)
                .map(|(&prediction, &target)| (prediction as f64 - target as f64).powi(2))
                .sum::<f64>()
                / predictions.len() as f64
        };
        final_train_loss = mse(&tp, &tt);
        let (train_mae, train_r2) = regression_metrics(&tp, &tt);
        if val_idx.is_empty() {
            final_val_loss = None;
            (train_mae, train_r2, None, None)
        } else {
            let vp: Vec<f32> = val_idx.iter().map(|&i| all_preds[i]).collect();
            let vt: Vec<f32> = val_idx.iter().map(|&i| ys[i]).collect();
            final_val_loss = Some(mse(&vp, &vt));
            let (val_mae, val_r2) = regression_metrics(&vp, &vt);
            (train_mae, train_r2, Some(val_mae), Some(val_r2))
        }
    } else {
        (f64::NAN, f64::NAN, None, None)
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
        validation,
        checkpoint_format_version: CHECKPOINT_FORMAT_VERSION,
        stopped_early,
        best_epoch,
        final_train_mae,
        final_val_mae,
        final_train_r2,
        final_val_r2,
        classification,
        validation_classification,
        class_labels,
        embedding_out_of_range: embedding_diagnostics.out_of_range,
        embedding_non_integer: embedding_diagnostics.non_integer,
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
/// Returns `(xs, rows, feature_count, embedding_diagnostics)`; the diagnostics
/// are the structured mirror of the stderr warnings (issue D3 predict follow-up).
fn prepare_inference_input(
    trained: &TrainedModel,
    df: &DataFrame,
) -> Result<(Vec<f32>, usize, usize, EmbeddingDiagnostics), String> {
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
    let diag = if trained.model.raw_input {
        check_embedding_indices(&trained.layers, &xs, feature_count)
    } else {
        EmbeddingDiagnostics::default()
    };
    Ok((xs, n, feature_count, diag))
}

/// Appends the prediction column to a clone of `df`.
fn attach_prediction(
    trained: &TrainedModel,
    df: &DataFrame,
    preds: &[f32],
    as_col: Option<&str>,
) -> Result<DataFrame, String> {
    let decoded;
    let preds = if trained.report.class_labels.is_empty() {
        preds
    } else {
        decoded = decode_logits(preds, &trained.report.class_labels)?.0;
        &decoded
    };
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
    predict_with_diagnostics(trained, df, as_col).map(|(out, _)| out)
}

/// Like [`predict`], but also returns the structured embedding-input diagnostics
/// so the runtime can surface them in `--json` (issue D3 predict follow-up). The
/// stderr warnings are emitted by [`prepare_inference_input`] either way.
pub fn predict_with_diagnostics(
    trained: &TrainedModel,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<(DataFrame, EmbeddingDiagnostics), String> {
    let device: Device<Plain> = Default::default();
    let mut infer_model = trained.model.clone();
    infer_model.training = false;
    predict_with_model::<Plain>(trained, &infer_model, &device, df, as_col)
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
) -> Result<(DataFrame, EmbeddingDiagnostics), String>
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
/// same predictions. Returns the frame plus embedding-input diagnostics.
pub fn predict_with_model<B>(
    trained: &TrainedModel,
    model: &Mlp<B>,
    device: &Device<B>,
    df: &DataFrame,
    as_col: Option<&str>,
) -> Result<(DataFrame, EmbeddingDiagnostics), String>
where
    B: Backend,
{
    let (xs, n, feature_count, diag) = prepare_inference_input(trained, df)?;

    let preds = forward_predictions(model, &xs, n, feature_count, device);

    Ok((attach_prediction(trained, df, &preds, as_col)?, diag))
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
) -> Result<(DataFrame, EmbeddingDiagnostics), String>
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

    #[test]
    fn invalid_validation_fractions_are_rejected_without_clamping() {
        use polars::prelude::*;
        let frame = df!("x" => [0.,1.,2.,3.], "y" => [0.,1.,0.,1.]).unwrap();
        for fraction in [-0.1, 0.95, f64::NAN, f64::INFINITY, 0.01] {
            let config = TrainConfig {
                target: "y".into(),
                epochs: 1,
                validation_split: Some(fraction),
                ..Default::default()
            };
            let result = train_impl::<Plain>(
                &frame,
                "invalid_fraction",
                &[LayerKind::Dense(1)],
                &config,
                &Default::default(),
                false,
            );
            assert!(result.is_err(), "must reject validation_split={fraction}");
        }
    }

    #[test]
    fn validation_loss_matches_returned_model_with_dropout_disabled() {
        use polars::prelude::*;
        let frame = df!("x" => [0.,1.,2.,3.], "y" => [0.,1.,0.,1.]).unwrap();
        let config = TrainConfig {
            target: "y".into(),
            epochs: 2,
            validation_split: Some(0.5),
            ..Default::default()
        };
        let trained = train_impl::<Plain>(
            &frame,
            "dropout_validation_oracle",
            &[
                LayerKind::Dense(16),
                LayerKind::Dropout(0.75),
                LayerKind::Dense(1),
            ],
            &config,
            &Default::default(),
            false,
        )
        .unwrap();
        let input: Vec<f32> = [2., 3.]
            .iter()
            .map(|x| ((x - trained.fmean[0]) / trained.fstd[0]) as f32)
            .collect();
        let tensor =
            Tensor::<Plain, 2>::from_data(TensorData::new(input, [2, 1]), &Default::default());
        let prediction = trained
            .model
            .forward(tensor)
            .into_data()
            .to_vec::<f32>()
            .unwrap();
        let expected =
            (prediction[0] as f64).powi(2) / 2.0 + (prediction[1] as f64 - 1.0).powi(2) / 2.0;
        assert!(
            (trained.report.final_val_loss.unwrap() - expected).abs() < 1e-5 * expected.max(1.0),
            "reported={:?}, independently recomputed={expected}",
            trained.report.final_val_loss
        );
    }

    #[test]
    fn validation_rows_do_not_fit_training_standardization() {
        use polars::prelude::*;
        let frame =
            df!("x" => [0.0, 2.0, 1000.0, 2000.0], "y" => [1.0, 3.0, 100.0, 200.0]).unwrap();
        let config = TrainConfig {
            target: "y".into(),
            epochs: 1,
            validation_split: Some(0.5),
            ..Default::default()
        };
        let trained = train_impl::<Plain>(
            &frame,
            "SplitStats",
            &[LayerKind::Dense(1)],
            &config,
            &Default::default(),
            false,
        )
        .unwrap();
        assert_eq!(trained.fmean, vec![1.0]);
        assert!((trained.fstd[0] - 2.0f64.sqrt()).abs() < 1e-10);
        assert_eq!(trained.report.validation.train_rows, 2);
        assert_eq!(trained.report.validation.validation_rows, 2);
        let json = serde_json::to_value(&trained.report).unwrap();
        assert_eq!(json["validation"]["strategy"], "sequential");
        assert!(trained.report.final_val_loss.unwrap().is_finite());
    }

    fn embedding_layer(vocab: EmbeddingVocab) -> LayerKind {
        LayerKind::Embedding {
            vocab,
            embed_dim: 2,
        }
    }

    #[test]
    fn classification_losses_match_returned_logits_and_not_training_batches() {
        use polars::prelude::*;
        let frame = df!("x" => [0.,1.,2.,3.], "y" => [0.,1.,0.,1.]).unwrap();
        let config = TrainConfig {
            target: "y".into(),
            epochs: 2,
            batch_size: Some(1),
            validation_split: Some(0.5),
            ..Default::default()
        };
        let trained = train_impl::<Plain>(
            &frame,
            "classification_loss_oracle",
            &[
                LayerKind::Dense(16),
                LayerKind::Dropout(0.75),
                LayerKind::Dense(2),
            ],
            &config,
            &Default::default(),
            false,
        )
        .unwrap();
        let input: Vec<f32> = [0., 1., 2., 3.]
            .iter()
            .map(|x| ((x - trained.fmean[0]) / trained.fstd[0]) as f32)
            .collect();
        let tensor =
            Tensor::<Plain, 2>::from_data(TensorData::new(input, [4, 1]), &Default::default());
        let logits = trained
            .model
            .forward(tensor)
            .into_data()
            .to_vec::<f32>()
            .unwrap();
        let losses: Vec<f64> = logits
            .as_chunks::<2>()
            .0
            .iter()
            .enumerate()
            .map(|(i, row)| {
                let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
                max + row
                    .iter()
                    .map(|&v| (v as f64 - max).exp())
                    .sum::<f64>()
                    .ln()
                    - row[i % 2] as f64
            })
            .collect();
        let expected_val = (losses[2] + losses[3]) / 2.0;
        let expected_train = (losses[0] + losses[1]) / 2.0;
        assert!(
            (trained.report.final_val_loss.unwrap() - expected_val).abs()
                < 1e-5 * expected_val.max(1.0),
            "reported val={:?}, oracle={expected_val}",
            trained.report.final_val_loss
        );
        assert!(
            (trained.report.final_train_loss - expected_train).abs()
                < 1e-5 * expected_train.max(1.0),
            "reported train={}, oracle={expected_train}",
            trained.report.final_train_loss
        );
    }

    fn assert_close(actual: f64, expected: f64, context: &str) {
        assert!(
            (actual - expected).abs() < 1e-5 * expected.abs().max(1.0),
            "{context}: actual={actual}, expected={expected}"
        );
    }

    #[test]
    fn classification_partitions_match_independent_model_oracle() {
        use polars::prelude::*;
        use xazz_core::ast::SplitStrategy;
        let x = [1., 2., 3., 4., 5., 6., 7., 8.];
        let time = [8., 1., 7., 2., 6., 3., 5., 4.];
        let y = [7., -3., 7., 7., -3., -3., 7., -3.];
        let frame = df!("x" => x, "time" => time, "y" => y).unwrap();
        // Fixed expected partitions deliberately differ from a prefix/suffix split.
        let cases = [
            (
                SplitStrategy::Random,
                None,
                vec![5, 7, 4, 6],
                vec![3, 1, 2, 0],
            ),
            (
                SplitStrategy::Stratified,
                None,
                vec![0, 1, 2, 4],
                vec![3, 5, 6, 7],
            ),
            (
                SplitStrategy::Sequential,
                Some("time"),
                vec![1, 3, 5, 7],
                vec![6, 4, 2, 0],
            ),
        ];
        for (strategy, time_column, train, validation) in cases {
            let config = TrainConfig {
                target: "y".into(),
                epochs: 2,
                batch_size: Some(1),
                validation_split: Some(0.5),
                split_strategy: strategy,
                time_column: time_column.map(Into::into),
                ..Default::default()
            };
            let trained = train_impl::<Plain>(
                &frame,
                "combined_partition_oracle",
                &[
                    LayerKind::Dense(8),
                    LayerKind::Dropout(0.7),
                    LayerKind::Dense(2),
                ],
                &config,
                &Default::default(),
                false,
            )
            .unwrap();
            for (j, values) in [x, time].iter().enumerate() {
                let mean = train.iter().map(|&i| values[i]).sum::<f64>() / train.len() as f64;
                let std = (train
                    .iter()
                    .map(|&i| (values[i] - mean).powi(2))
                    .sum::<f64>()
                    / (train.len() - 1) as f64)
                    .sqrt();
                assert_close(trained.fmean[j], mean, "training-only mean");
                assert_close(trained.fstd[j], std, "training-only standard deviation");
            }
            let input: Vec<f32> = (0..x.len())
                .flat_map(|i| {
                    [
                        ((x[i] - trained.fmean[0]) / trained.fstd[0]) as f32,
                        ((time[i] - trained.fmean[1]) / trained.fstd[1]) as f32,
                    ]
                })
                .collect();
            let logits = trained
                .model
                .forward(Tensor::<Plain, 2>::from_data(
                    TensorData::new(input, [8, 2]),
                    &Default::default(),
                ))
                .into_data()
                .to_vec::<f32>()
                .unwrap();
            for (indices, metrics, reported_loss) in [
                (
                    &train,
                    trained.report.classification.as_ref().unwrap(),
                    trained.report.final_train_loss,
                ),
                (
                    &validation,
                    trained.report.validation_classification.as_ref().unwrap(),
                    trained.report.final_val_loss.unwrap(),
                ),
            ] {
                let mut cm = [0usize; 4];
                let mut loss = 0.0;
                for &i in indices {
                    let row = &logits[i * 2..i * 2 + 2];
                    let actual = usize::from(y[i] == 7.0);
                    let predicted = usize::from(row[1] > row[0]);
                    cm[actual * 2 + predicted] += 1;
                    let max = row[0].max(row[1]) as f64;
                    loss += max
                        + row
                            .iter()
                            .map(|&v| (v as f64 - max).exp())
                            .sum::<f64>()
                            .ln()
                        - row[actual] as f64;
                }
                assert_eq!(metrics.confusion_matrix, cm);
                assert_close(
                    reported_loss,
                    loss / indices.len() as f64,
                    "returned-model cross entropy",
                );
                assert_close(
                    metrics.accuracy,
                    (cm[0] + cm[3]) as f64 / indices.len() as f64,
                    "accuracy",
                );
                let (mut precision, mut recall, mut f1) = (0.0, 0.0, 0.0);
                for class in 0..2 {
                    let tp = cm[class * 2 + class] as f64;
                    let actual = (cm[class * 2] + cm[class * 2 + 1]) as f64;
                    let predicted = (cm[class] + cm[2 + class]) as f64;
                    if predicted > 0.0 {
                        precision += tp / predicted / 2.0;
                    }
                    if actual > 0.0 {
                        recall += tp / actual / 2.0;
                    }
                    if actual + predicted > 0.0 {
                        f1 += 2.0 * tp / (actual + predicted) / 2.0;
                    }
                }
                assert_close(metrics.precision, precision, "macro precision");
                assert_close(metrics.recall, recall, "macro recall");
                assert_close(metrics.f1, f1, "macro f1");
                let mut concordance = 0.0;
                let mut pairs = 0;
                for &positive in indices.iter().filter(|&&i| y[i] == 7.0) {
                    for &negative in indices.iter().filter(|&&i| y[i] == -3.0) {
                        let score = |i: usize| logits[2 * i + 1] as f64 - logits[2 * i] as f64;
                        concordance += match score(positive).total_cmp(&score(negative)) {
                            std::cmp::Ordering::Greater => 1.0,
                            std::cmp::Ordering::Equal => 0.5,
                            std::cmp::Ordering::Less => 0.0,
                        };
                        pairs += 1;
                    }
                }
                assert_close(
                    metrics.auc.unwrap(),
                    concordance / pairs as f64,
                    "pairwise AUC",
                );
            }
            assert_eq!(trained.report.validation.train_rows, 4);
            assert_eq!(trained.report.validation.validation_rows, 4);
            let json = serde_json::to_value(&trained.report).unwrap();
            assert_eq!(
                json["validation"]["strategy"],
                serde_json::to_value(strategy).unwrap()
            );
        }
    }

    #[test]
    fn regression_partitions_report_returned_model_mse() {
        use polars::prelude::*;
        use xazz_core::ast::SplitStrategy;
        let x = [1., 2., 3., 4., 5., 6., 7., 8.];
        let y = [7., -3., 7., 7., -3., -3., 7., -3.];
        let frame = df!("x" => x, "y" => y).unwrap();
        for (strategy, train, validation) in [
            (
                SplitStrategy::Sequential,
                vec![0, 1, 2, 3],
                vec![4, 5, 6, 7],
            ),
            (SplitStrategy::Random, vec![5, 7, 4, 6], vec![3, 1, 2, 0]),
            (
                SplitStrategy::Stratified,
                vec![0, 1, 2, 4],
                vec![3, 5, 6, 7],
            ),
        ] {
            let config = TrainConfig {
                target: "y".into(),
                epochs: 2,
                batch_size: Some(3),
                validation_split: Some(0.5),
                split_strategy: strategy,
                ..Default::default()
            };
            let trained = train_impl::<Plain>(
                &frame,
                "regression_partition_oracle",
                &[
                    LayerKind::Dense(8),
                    LayerKind::Dropout(0.7),
                    LayerKind::Dense(1),
                ],
                &config,
                &Default::default(),
                false,
            )
            .unwrap();
            let input = x
                .iter()
                .map(|&v| ((v - trained.fmean[0]) / trained.fstd[0]) as f32)
                .collect::<Vec<_>>();
            let prediction = trained
                .model
                .forward(Tensor::<Plain, 2>::from_data(
                    TensorData::new(input, [8, 1]),
                    &Default::default(),
                ))
                .into_data()
                .to_vec::<f32>()
                .unwrap();
            for (indices, actual) in [
                (&train, trained.report.final_train_loss),
                (&validation, trained.report.final_val_loss.unwrap()),
            ] {
                let expected = indices
                    .iter()
                    .map(|&i| (prediction[i] as f64 - y[i]).powi(2))
                    .sum::<f64>()
                    / indices.len() as f64;
                assert_close(actual, expected, "returned-model regression MSE");
            }
        }
    }

    #[test]
    fn stratified_classification_retains_singletons_and_rejects_excess_validation() {
        use polars::prelude::*;
        let frame =
            df!("x" => [0.,1.,2.,3.,4.,5.,6.], "y" => [10.,10.,10.,20.,20.,20.,99.]).unwrap();
        let mut config = TrainConfig {
            target: "y".into(),
            epochs: 1,
            validation_split: Some(0.5),
            split_strategy: xazz_core::ast::SplitStrategy::Stratified,
            ..Default::default()
        };
        let train = |config: &TrainConfig| {
            train_impl::<Plain>(
                &frame,
                "rare_class_oracle",
                &[LayerKind::Dense(3)],
                config,
                &Default::default(),
                false,
            )
        };
        let trained = train(&config).unwrap();
        let rare = trained
            .report
            .validation
            .classes
            .iter()
            .find(|c| c.label == 99.)
            .unwrap();
        assert_eq!((rare.train_rows, rare.validation_rows), (1, 0));
        assert_eq!(
            trained
                .report
                .classification
                .as_ref()
                .unwrap()
                .confusion_matrix[6..9]
                .iter()
                .sum::<usize>(),
            1
        );
        assert_eq!(
            trained
                .report
                .validation_classification
                .as_ref()
                .unwrap()
                .confusion_matrix[6..9]
                .iter()
                .sum::<usize>(),
            0
        );
        config.validation_split = Some(0.9);
        assert!(
            train(&config)
                .err()
                .expect("excess validation must fail")
                .contains("retain every class")
        );
    }

    #[test]
    fn classification_rejects_labels_changed_by_float_conversion() {
        use polars::prelude::*;
        let columns = [
            Column::new("y".into(), [16_777_216i64, 16_777_217, 0, 0]),
            Column::new("y".into(), [u64::MAX, u64::MAX - 1, 0, 0]),
            Column::new("y".into(), [1.00000001f64, 1., 0., 0.]),
        ];
        for column in columns {
            let frame =
                DataFrame::new(4, vec![Column::new("x".into(), [0., 1., 2., 3.]), column]).unwrap();
            for (strategy, width) in [
                (xazz_core::ast::SplitStrategy::Sequential, 2),
                (xazz_core::ast::SplitStrategy::Stratified, 1),
            ] {
                let config = TrainConfig {
                    target: "y".into(),
                    epochs: 1,
                    validation_split: Some(0.5),
                    split_strategy: strategy,
                    ..Default::default()
                };
                let error = train_impl::<Plain>(
                    &frame,
                    "invalid_label_precision",
                    &[LayerKind::Dense(width)],
                    &config,
                    &Default::default(),
                    false,
                )
                .err()
                .expect("rounded class labels must fail");
                assert!(error.contains("exactly as f32"), "{error}");
            }
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
    fn detect_classification_accepts_integer_classes() {
        let classes =
            detect_classification(&[0.0, 1.0, 2.0, 1.0, 2.0], 10).expect("classification");
        assert_eq!(classes, vec![0.0, 1.0, 2.0]);
    }

    #[test]
    fn detect_classification_rejects_continuous_target() {
        assert!(detect_classification(&[0.0, 0.5, 1.0], 10).is_none());
    }

    #[test]
    fn detect_classification_rejects_single_class() {
        assert!(detect_classification(&[1.0, 1.0, 1.0], 10).is_none());
    }

    /// Expected accuracy and confusion matrix for a three-class example.
    #[test]
    fn classification_metrics_confusion_matrix() {
        let classes = [0.0f32, 1.0, 2.0];
        let targets = [0.0f32, 0.0, 1.0, 1.0, 2.0, 2.0];
        let preds = [0.0f32, 1.0, 1.0, 1.0, 2.0, 0.0];
        let m = classification_metrics(&preds, &targets, &classes, None).unwrap();
        assert_eq!(m.num_classes, 3);
        assert!((m.accuracy - 4.0 / 6.0).abs() < 1e-12);
        // Row-major (actual * 3 + predicted): class0→{0:1,1:1}, class1→{1:2},
        // class2→{2:1,0:1}.
        assert_eq!(m.confusion_matrix, vec![1, 1, 0, 0, 2, 0, 1, 0, 1]);
        assert!(m.precision.is_finite() && m.recall.is_finite() && m.f1.is_finite());
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
            embedding_out_of_range: 0,
            embedding_non_integer: 0,
            classification: None,
            validation_classification: None,
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
            embedding_out_of_range: 0,
            embedding_non_integer: 0,
            classification: None,
            validation_classification: None,
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
            embedding_out_of_range: 0,
            embedding_non_integer: 0,
            classification: None,
            validation_classification: None,
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

    #[test]
    fn check_embedding_indices_reports_structured_counts() {
        // 2 columns sharing vocab 3; row-major. One out-of-range per column and
        // one fractional value. Non-finite values must not be counted.
        let values = [0.0, 1.0, 3.0, 0.5, f32::NAN, 9.0];
        let diag =
            check_embedding_indices(&[embedding_layer(EmbeddingVocab::Shared(3))], &values, 2);
        assert_eq!(diag.out_of_range, 2, "3.0 and 9.0 are above vocab-1");
        assert_eq!(diag.non_integer, 1, "0.5 is finite with a fractional part");

        // A clean raw-index input yields zeros.
        let clean = check_embedding_indices(
            &[embedding_layer(EmbeddingVocab::Shared(3))],
            &[0.0, 1.0, 2.0, 0.0],
            2,
        );
        assert_eq!(clean, EmbeddingDiagnostics::default());

        // Without a leading embedding layer there is nothing to diagnose.
        assert_eq!(
            check_embedding_indices(&[LayerKind::Dense(2)], &[9.0], 1),
            EmbeddingDiagnostics::default()
        );
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
            validation: SplitReport::default(),
            checkpoint_format_version: CHECKPOINT_FORMAT_VERSION,
            stopped_early: false,
            best_epoch: 0,
            final_train_mae: 0.4,
            final_val_mae: Some(0.5),
            final_train_r2: 0.9,
            final_val_r2: Some(0.85),
            classification: None,
            validation_classification: None,
            class_labels: Vec::new(),
            embedding_out_of_range: 0,
            embedding_non_integer: 0,
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
