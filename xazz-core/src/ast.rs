//! Xazz - AST node definitions (v0.3)
//!
//! Uses only plain Rust types, with no heavy dependencies such as Polars / Tokio.
//! v0.3: Added Burn deep-learning model declaration (ModelDecl) and training (TrainStmt) AST

/// Expression node
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// Identifier reference (variable name or column name)
    Ident(String),
    /// String literal
    StringLit(String),
    /// Integer literal
    IntLit(i64),
    /// Floating-point literal
    FloatLit(f64),
    /// Boolean literal (true / false)
    BoolLit(bool),
    /// Binary operation (lhs op rhs) — includes comparison and arithmetic operations
    BinOp {
        lhs: Box<Expr>,
        op: BinOpKind,
        rhs: Box<Expr>,
    },
}

/// Binary operator kinds (comparison + arithmetic)
#[derive(Debug, Clone, PartialEq)]
pub enum BinOpKind {
    // ── Comparison operators ──────────────────────
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    // ── Arithmetic operators (v0.16+) ─────────────
    Add,
    Sub,
    Mul,
    Div,
}

/// fillNull fill value kinds
#[derive(Debug, Clone, PartialEq)]
pub enum FillNullValue {
    /// Integer fill value
    Int(i64),
    /// Floating-point fill value
    Float(f64),
    /// String fill value
    Str(String),
    /// Mean fill strategy (strategy: "mean")
    Mean,
    /// Median fill strategy (strategy: "median")
    Median,
    /// Zero fill strategy (strategy: "zero")
    Zero,
}

/// join methods
#[derive(Debug, Clone, PartialEq, Default)]
pub enum JoinHow {
    #[default]
    Inner,
    Left,
    Outer,
    Cross,
}

impl JoinHow {
    /// Parse from a lowercase string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "inner" => Some(JoinHow::Inner),
            "left" => Some(JoinHow::Left),
            "outer" => Some(JoinHow::Outer),
            "cross" => Some(JoinHow::Cross),
            _ => None,
        }
    }

    pub fn as_polars_str(&self) -> &'static str {
        match self {
            JoinHow::Inner => "JoinType::Inner",
            JoinHow::Left => "JoinType::Left",
            JoinHow::Outer => "JoinType::Full",
            JoinHow::Cross => "JoinType::Cross",
        }
    }
}

// ── v0.19 visualization types ──────────────────────────────────────────────────────────

/// Chart types (MVP: bar / line / pie / scatter)
#[derive(Debug, Clone, PartialEq)]
pub enum ChartType {
    Bar,
    Line,
    Pie,
    Scatter,
}

impl ChartType {
    /// Parse from an identifier string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "bar" => Some(ChartType::Bar),
            "line" => Some(ChartType::Line),
            "pie" => Some(ChartType::Pie),
            "scatter" => Some(ChartType::Scatter),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ChartType::Bar => "bar",
            ChartType::Line => "line",
            ChartType::Pie => "pie",
            ChartType::Scatter => "scatter",
        }
    }
}

/// chart { ... } block settings
#[derive(Debug, Clone, PartialEq)]
pub struct ChartConfig {
    pub chart_type: ChartType,
    pub title: Option<String>,
    /// x-axis column name (for bar, line, scatter)
    pub x: Option<String>,
    /// y-axis column name (for bar, line, scatter)
    pub y: Option<String>,
    /// label column name (for pie)
    pub label: Option<String>,
    /// value column name (for pie)
    pub value: Option<String>,
}

/// Aggregation function for the multi-aggregate `agg([...])` operator (v0.23).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggFn {
    Sum,
    Mean,
    Min,
    Max,
    Count,
    Median,
    Variance,
    Std,
}

/// A single aggregation spec inside `agg([...])`, e.g. `mean("measurement")`.
#[derive(Debug, Clone, PartialEq)]
pub struct AggSpec {
    pub func: AggFn,
    pub col: String,
}

/// Pipeline operation step
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOp {
    /// filter(<condition>)
    Filter(Expr),
    /// select([col1, col2, ...])
    Select(Vec<String>),
    /// count  (None: flag to count all rows) / count("col")  (Some: group aggregation)
    Count(Option<String>),
    /// groupBy("col")  — used in pairs with Sum/Mean/Min/Max/Count(Some) afterwards
    GroupBy(String),
    /// sum("col")  — used standalone or after groupBy
    Sum(String),
    /// mean("col")  — used standalone or after groupBy
    Mean(String),
    /// min("col")  — used standalone or after groupBy
    Min(String),
    /// max("col")  — used standalone or after groupBy
    Max(String),
    /// orderBy("col", desc: true/false)
    OrderBy { col: String, desc: bool },
    /// take(n)  — keep only the top n rows
    Take(i64),
    /// dropNull("col")  — remove rows where the column is null
    DropNull(String),
    /// fillNull("col", value)  — fill nulls in the column with value
    FillNull { col: String, value: FillNullValue },
    /// join(other_var, left_on/right_on, how)
    Join {
        other: String,
        left_on: Vec<String>,
        right_on: Vec<String>,
        how: JoinHow,
    },
    /// withColumn("new_col", expr)  — add/transform a new column
    WithColumn { name: String, expr: Expr },
    /// chart { type: ..., x: ..., y: ..., title: "..." }  — pipeline visualization (v0.19)
    Chart(ChartConfig),
    /// cast("col", "float")  — explicitly cast the column type at the DSL level (v0.20)
    Cast { col: String, to_type: String },
    /// rename("old_name", "new_name") — rename a column (v0.21)
    Rename { old_name: String, new_name: String },
    /// replace("col", ".", "") — string replacement (v0.21)
    Replace {
        col: String,
        from: String,
        to: String,
    },
    /// sample(n) / sample(n, seed: 42) — random sampling (v0.22)
    Sample { n: i64, seed: Option<i64> },
    /// median("col") — median aggregation (v0.22)
    Median(String),
    /// variance("col") — variance aggregation (v0.22)
    Variance(String),
    /// std("col") — standard deviation aggregation (v0.22)
    Std(String),
    /// agg([min("c"), mean("c"), max("c")]) — multiple aggregations in one pass (v0.23)
    Agg(Vec<AggSpec>),
    /// train(ModelName, target: "col", epochs: N, lr: F) — training operator (v0.5)
    Train {
        model_name: String,
        config: TrainConfig,
    },
    /// predict(model_var, as: "col") — prediction operator for a trained model (v0.5)
    Predict {
        model_var: String,
        as_col: Option<String>,
    },
    /// withDp(epsilon: 1.0, mechanism: laplace, ...) — differential privacy noise injection (v0.6)
    WithDp(DpArgs),
    /// save("out.parquet", format: "parquet")  — write the pipeline result to an artifact file (v0.3.2, issue #52)
    Save { path: String, format: SaveFormat },
}

/// Output artifact format for `save()` (v0.3.2, issue #52)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveFormat {
    Csv,
    Parquet,
    Arrow,
}

impl SaveFormat {
    /// Parse from an extension string (case-insensitive).
    pub fn from_ext(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "csv" => Some(SaveFormat::Csv),
            "parquet" | "pq" => Some(SaveFormat::Parquet),
            "arrow" | "ipc" | "feather" => Some(SaveFormat::Arrow),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            SaveFormat::Csv => "csv",
            SaveFormat::Parquet => "parquet",
            SaveFormat::Arrow => "arrow",
        }
    }

    pub fn default_extension(&self) -> &'static str {
        match self {
            SaveFormat::Csv => "csv",
            SaveFormat::Parquet => "parquet",
            SaveFormat::Arrow => "arrow",
        }
    }
}

/// Differential privacy noise mechanism kinds (v0.6)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DpMechanism {
    /// Laplace Mechanism — ε-DP. scale b = sensitivity / ε
    Laplace,
    /// Gaussian Mechanism — (ε, δ)-DP. σ = sensitivity·√(2·ln(1.25/δ)) / ε
    Gaussian,
}

impl DpMechanism {
    /// Parse from an identifier/string
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "laplace" => Some(DpMechanism::Laplace),
            "gaussian" => Some(DpMechanism::Gaussian),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DpMechanism::Laplace => "laplace",
            DpMechanism::Gaussian => "gaussian",
        }
    }
}

/// withDp(...) operator arguments (v0.6)
///
/// - `epsilon`     : privacy budget ε (required, > 0). Smaller means stronger protection and more noise.
/// - `mechanism`   : laplace (default) | gaussian
/// - `sensitivity` : query sensitivity Δf (default 1.0)
/// - `delta`       : gaussian-only δ (default 1e-5)
/// - `seed`        : seed for noise reproducibility (for auditing/testing; non-deterministic if unspecified)
#[derive(Debug, Clone, PartialEq)]
pub struct DpArgs {
    pub epsilon: f64,
    pub mechanism: DpMechanism,
    pub sensitivity: f64,
    pub delta: Option<f64>,
    pub seed: Option<i64>,
}

impl Default for DpArgs {
    fn default() -> Self {
        DpArgs {
            epsilon: 1.0,
            mechanism: DpMechanism::Laplace,
            sensitivity: 1.0,
            delta: None,
            seed: None,
        }
    }
}

/// Parsing options for a `load()` source. All fields fall back to the backend
/// default when `None` (comma separator, header row present).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadOptions {
    /// Field separator byte for delimited text (e.g. `b';'`). `None` = backend default (`,`).
    pub separator: Option<u8>,
    /// Whether the source has a header row. `None` = backend default (`true`).
    pub has_header: Option<bool>,
}

/// Pipeline source (data origin)
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineSource {
    /// load("file_path", sep: ";", header: false) :: SchemaName
    Load {
        file_path: String,
        schema_name: String,
        options: LoadOptions,
    },
    /// Reference to an already-declared variable
    VarRef(String),
}

/// A single field of a type declaration
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub field_type: String,
}

/// Vocabulary specification for an [`LayerKind::Embedding`] layer (D3).
///
/// `Embedding(10, 4)` uses one shared table for every input column; the list
/// form `Embedding([4, 7, 2], 3)` gives each input column its own vocabulary,
/// in feature order (so per-column category indices need not be re-based).
#[derive(Debug, Clone, PartialEq)]
pub enum EmbeddingVocab {
    /// One table shared by every input column: `Embedding(vocab_size, embed_dim)`.
    Shared(usize),
    /// A distinct table per input column: `Embedding([v0, v1, ...], embed_dim)`.
    PerColumn(Vec<usize>),
}

impl EmbeddingVocab {
    /// Source-like rendering: `10` or `[4, 7, 2]`.
    pub fn display(&self) -> String {
        match self {
            EmbeddingVocab::Shared(v) => v.to_string(),
            EmbeddingVocab::PerColumn(vs) => {
                let inner = vs
                    .iter()
                    .map(|v| v.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{inner}]")
            }
        }
    }

    /// Whether every vocabulary size is >= 1 (and a per-column list is non-empty).
    pub fn is_valid(&self) -> bool {
        match self {
            EmbeddingVocab::Shared(v) => *v >= 1,
            EmbeddingVocab::PerColumn(vs) => !vs.is_empty() && vs.iter().all(|v| *v >= 1),
        }
    }

    /// Expands to one vocabulary per input column.
    ///
    /// A shared vocab is replicated `input_dim` times; a per-column list must
    /// already have exactly `input_dim` entries (otherwise it is a mismatch and
    /// an error is returned).
    pub fn expand(&self, input_dim: usize) -> Result<Vec<usize>, String> {
        match self {
            EmbeddingVocab::Shared(v) => Ok(vec![*v; input_dim]),
            EmbeddingVocab::PerColumn(vs) if vs.len() == input_dim => Ok(vs.clone()),
            EmbeddingVocab::PerColumn(vs) => Err(if crate::i18n::is_korean() {
                format!(
                    "Embedding 의 컬럼별 vocab {}개가 입력 특성 컬럼 수 {}개와 다릅니다.",
                    vs.len(),
                    input_dim
                )
            } else {
                format!(
                    "Embedding declares {} per-column vocab entr{} but the input has {} feature column(s).",
                    vs.len(),
                    if vs.len() == 1 { "y" } else { "ies" },
                    input_dim
                )
            }),
        }
    }
}

/// Deep-learning layer kinds (Burn mapping)
#[derive(Debug, Clone, PartialEq)]
pub enum LayerKind {
    /// Dense(units) — fully connected layer
    Dense(usize),
    /// ReLU() — activation
    ReLU,
    /// Sigmoid() — activation
    Sigmoid,
    /// Tanh() — activation
    Tanh,
    /// Softmax() — activation
    Softmax,
    /// Dropout(rate) — regularization
    Dropout(f64),
    /// BatchNorm() — normalization
    BatchNorm,
    /// Conv1d(out_channels, kernel_size) — 1D convolution over the feature axis (D3)
    Conv1d {
        out_channels: usize,
        kernel_size: usize,
    },
    /// Embedding(vocab, embed_dim) — categorical input embedding (D3).
    ///
    /// The vocab argument is either a scalar (`Embedding(10, 4)` — one table
    /// shared by every input column) or a list (`Embedding([4, 7], 3)` — one
    /// independent table per input column, in feature order). Must be the first
    /// layer; input feature values are treated as category indices.
    Embedding {
        vocab: EmbeddingVocab,
        embed_dim: usize,
    },
}

impl LayerKind {
    /// Returns the string for Burn code generation (the input dimension is determined by the dataset schema, so it is passed as a function argument).
    pub fn to_burn_str(&self) -> String {
        match self {
            LayerKind::Dense(n) => format!("nn::LinearConfig::new(<in_dim>, {})", n),
            LayerKind::ReLU => "activation::relu()".to_string(),
            LayerKind::Sigmoid => "activation::sigmoid()".to_string(),
            LayerKind::Tanh => "activation::tanh()".to_string(),
            LayerKind::Softmax => "activation::softmax(dim=1)".to_string(),
            LayerKind::Dropout(r) => format!("nn::DropoutConfig::new({})", r),
            LayerKind::BatchNorm => "// BatchNorm: not supported for 1D MLP, skipped".to_string(),
            LayerKind::Conv1d {
                out_channels,
                kernel_size,
            } => format!(
                "nn::Conv1dConfig::new(1, {out_channels}, {kernel_size}).with_padding(PaddingConfig1d::Same)"
            ),
            LayerKind::Embedding { vocab, embed_dim } => {
                format!("nn::EmbeddingConfig::new({}, {embed_dim})", vocab.display())
            }
        }
    }
}

/// Metric used to rank hyperparameter-sweep combinations (D3).
///
/// The default is MSE (the training loss). `mae` and `r2` are computed from the
/// final model's predictions on the same train/validation split used for the
/// loss, and let `train(..., metric: ..)` select a sweep winner by a different
/// criterion. R² is "higher is better"; the error metrics are "lower is better".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SweepMetric {
    /// Mean squared error (default; the training objective).
    #[default]
    Mse,
    /// Mean absolute error.
    Mae,
    /// Coefficient of determination (higher is better).
    R2,
}

impl SweepMetric {
    /// Canonical id (also the `metric:` train() value).
    pub fn id(self) -> &'static str {
        match self {
            SweepMetric::Mse => "mse",
            SweepMetric::Mae => "mae",
            SweepMetric::R2 => "r2",
        }
    }

    /// Parses a `metric:` value, accepting common aliases.
    /// Returns `None` for an unrecognised value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "mse" | "l2" | "loss" => Some(SweepMetric::Mse),
            "mae" | "l1" => Some(SweepMetric::Mae),
            "r2" | "r^2" | "rsquared" => Some(SweepMetric::R2),
            _ => None,
        }
    }

    /// Whether a lower value is better. R² is the only "higher is better" metric.
    pub fn lower_is_better(self) -> bool {
        !matches!(self, SweepMetric::R2)
    }
}

/// Ordering applied to the reported hyperparameter-sweep combinations (D3).
///
/// This only affects how the per-combination table/report is ordered; the winner
/// is always chosen by [`SweepMetric`]. `Metric` sorts best-first, the axis
/// variants sort ascending by that hyperparameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SweepSort {
    /// Best-first by the selection metric (default).
    #[default]
    Metric,
    /// Ascending by `epochs`.
    Epochs,
    /// Ascending by learning rate.
    Lr,
    /// Ascending by `batch_size`.
    Batch,
}

impl SweepSort {
    /// Canonical id (also the `sort:` train() value).
    pub fn id(self) -> &'static str {
        match self {
            SweepSort::Metric => "metric",
            SweepSort::Epochs => "epochs",
            SweepSort::Lr => "lr",
            SweepSort::Batch => "batch",
        }
    }

    /// Parses a `sort:` value, accepting common aliases.
    /// Returns `None` for an unrecognised value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "metric" | "score" | "loss" => Some(SweepSort::Metric),
            "epochs" | "epoch" => Some(SweepSort::Epochs),
            "lr" | "learning_rate" => Some(SweepSort::Lr),
            "batch" | "batch_size" => Some(SweepSort::Batch),
            _ => None,
        }
    }

    /// Whether this variant is a concrete hyperparameter axis.
    ///
    /// [`SweepSort::Metric`] is a pseudo-axis (the selection metric) and cannot be
    /// used as a `tiebreak:` target.
    pub fn is_axis(self) -> bool {
        !matches!(self, SweepSort::Metric)
    }
}

/// How the `validation_split` train/validation partition is drawn (issue #162).
///
/// The default keeps the historical behaviour: the last `validation_split`
/// fraction of rows becomes the validation set in natural row order, and the
/// training rows are shuffled per epoch. When rows are ordered by time this is
/// already leak-free, so [`SplitStrategy::Sequential`] doubles as the
/// time-series split; `time_column:` sorts the rows first when the frame is not
/// pre-sorted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SplitStrategy {
    /// Tail split in natural (or `time_column`-sorted) row order — the default.
    #[default]
    Sequential,
    /// Preserve the target-class ratio across the split (classification targets).
    Stratified,
    /// Shuffle rows with a fixed seed before taking the tail as validation.
    Random,
}

impl SplitStrategy {
    /// Canonical id (also the `split:` train() value).
    pub fn id(self) -> &'static str {
        match self {
            SplitStrategy::Sequential => "sequential",
            SplitStrategy::Stratified => "stratified",
            SplitStrategy::Random => "random",
        }
    }

    /// Parses a `split:` value, accepting common aliases.
    /// Returns `None` for an unrecognised value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "" | "sequential" | "time" | "timeseries" | "temporal" | "chrono" => {
                Some(SplitStrategy::Sequential)
            }
            "stratified" | "stratify" | "class" | "class_balanced" => {
                Some(SplitStrategy::Stratified)
            }
            "random" | "shuffle" => Some(SplitStrategy::Random),
            _ => None,
        }
    }

    /// Whether this strategy needs the target values to build the split.
    pub fn needs_targets(self) -> bool {
        matches!(self, SplitStrategy::Stratified)
    }
}

/// Hyperparameter sweep grid (D3) — list-valued `train()` arguments.
///
/// Each non-empty vector is one axis of a full cartesian-product grid search;
/// an empty vector means "use the scalar value on [`TrainConfig`]".
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SweepGrid {
    /// `epochs: [10, 20]`
    pub epochs: Vec<usize>,
    /// `lr: [0.01, 0.001]`
    pub learning_rate: Vec<f64>,
    /// `batch_size: [16, 32]`
    pub batch_size: Vec<usize>,
}

impl SweepGrid {
    /// Whether any axis was declared as a list.
    pub fn is_empty(&self) -> bool {
        self.epochs.is_empty() && self.learning_rate.is_empty() && self.batch_size.is_empty()
    }

    /// Number of combinations the grid expands to (at least 1).
    pub fn len(&self) -> usize {
        self.epochs.len().max(1) * self.learning_rate.len().max(1) * self.batch_size.len().max(1)
    }
}

/// Training hyperparameter configuration
#[derive(Debug, Clone, PartialEq)]
pub struct TrainConfig {
    /// Column to train on (target)
    pub target: String,
    /// Number of epochs
    pub epochs: usize,
    /// Learning rate
    pub learning_rate: f64,
    /// Batch size (None: all data)
    pub batch_size: Option<usize>,
    /// Validation data ratio (0.0 ~ 1.0)
    pub validation_split: Option<f64>,
    /// Early stopping: stop after this many epochs without validation-loss
    /// improvement (requires `validation_split`). None disables it (D3).
    pub early_stopping_patience: Option<usize>,
    /// Hyperparameter sweep axes (D3). Empty when no list argument was given.
    pub sweep: SweepGrid,
    /// Metric used to pick the sweep winner (D3). Defaults to MSE.
    pub sweep_metric: SweepMetric,
    /// Whether `metric:` was explicitly written in the source (D3).
    ///
    /// The value alone cannot distinguish `metric: "mse"` from an omitted
    /// `metric:` because both equal [`SweepMetric::default`]; this flag records
    /// the explicit mention so the checker can warn about a no-op option.
    pub sweep_metric_explicit: bool,
    /// Ordering of the reported sweep combinations (D3). Defaults to metric.
    pub sweep_sort: SweepSort,
    /// Whether `sort:` was explicitly written in the source (D3). See
    /// [`TrainConfig::sweep_metric_explicit`].
    pub sweep_sort_explicit: bool,
    /// Ordered axes used to break ties after the primary `sort:` key (D3).
    ///
    /// When empty, ties fall back to the axis order implied by `sort:` (the
    /// remaining axes in canonical order). When non-empty, the listed axes are
    /// compared first, in order, then the remaining axes in canonical order.
    /// Only axis variants are accepted; [`SweepSort::Metric`] is rejected by the
    /// parser.
    pub sweep_tiebreak: Vec<SweepSort>,
    /// When set, report only this many best-by-metric combinations (D3).
    pub sweep_top: Option<usize>,
    /// How the validation split is drawn (issue #162). Defaults to the
    /// historical tail split ([`SplitStrategy::Sequential`]).
    pub split_strategy: SplitStrategy,
    /// Column whose ascending order defines the time axis for the split (issue
    /// #162). When set, rows are sorted by it before the tail split so the
    /// validation window never precedes the training window.
    pub time_column: Option<String>,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            target: String::new(),
            epochs: 10,
            learning_rate: 0.01,
            batch_size: None,
            validation_split: None,
            early_stopping_patience: None,
            sweep: SweepGrid::default(),
            sweep_metric: SweepMetric::default(),
            sweep_metric_explicit: false,
            sweep_sort: SweepSort::default(),
            sweep_sort_explicit: false,
            sweep_tiebreak: Vec::new(),
            sweep_top: None,
            split_strategy: SplitStrategy::default(),
            time_column: None,
        }
    }
}

impl TrainConfig {
    /// Whether this config declares a hyperparameter sweep.
    pub fn is_sweep(&self) -> bool {
        !self.sweep.is_empty()
    }

    /// Expands the sweep grid into concrete configs (cartesian product).
    ///
    /// A non-sweep config expands to exactly one config equal to itself. The
    /// returned configs always carry an empty [`SweepGrid`] so they can be run
    /// directly.
    pub fn expand_sweep(&self) -> Vec<TrainConfig> {
        let epochs_axis = if self.sweep.epochs.is_empty() {
            vec![self.epochs]
        } else {
            self.sweep.epochs.clone()
        };
        let lr_axis = if self.sweep.learning_rate.is_empty() {
            vec![self.learning_rate]
        } else {
            self.sweep.learning_rate.clone()
        };
        let bs_axis: Vec<Option<usize>> = if self.sweep.batch_size.is_empty() {
            vec![self.batch_size]
        } else {
            self.sweep.batch_size.iter().copied().map(Some).collect()
        };

        let mut out = Vec::with_capacity(epochs_axis.len() * lr_axis.len() * bs_axis.len());
        for &epochs in &epochs_axis {
            for &lr in &lr_axis {
                for &bs in &bs_axis {
                    out.push(TrainConfig {
                        target: self.target.clone(),
                        epochs,
                        learning_rate: lr,
                        batch_size: bs,
                        validation_split: self.validation_split,
                        early_stopping_patience: self.early_stopping_patience,
                        sweep: SweepGrid::default(),
                        sweep_metric: self.sweep_metric,
                        sweep_metric_explicit: false,
                        sweep_sort: SweepSort::default(),
                        sweep_sort_explicit: false,
                        sweep_tiebreak: Vec::new(),
                        sweep_top: None,
                        split_strategy: self.split_strategy,
                        time_column: self.time_column.clone(),
                    });
                }
            }
        }
        out
    }
}

/// Top-level statement node
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// import "path.xzz"  — bring module type/model/pipeline declarations into scope (v0.3.2, issue #69)
    Import { path: String },
    /// type <Name> = { <fields> }
    TypeDecl {
        name: String,
        fields: Vec<StructField>,
    },
    /// (mut)? v <name> = <source> |> op1 |> op2 ...
    VarDecl {
        var_name: String,
        is_mut: bool,
        source: PipelineSource,
        ops: Vec<PipelineOp>,
    },
    /// expression statement: run the pipeline without assigning to a variable (result discarded)
    ExprStmt {
        source: PipelineSource,
        ops: Vec<PipelineOp>,
    },
    /// model <Name> { Layer1 -> Layer2 -> ... }
    ModelDecl {
        name: String,
        layers: Vec<LayerKind>,
    },
    /// run <var> |> train(ModelName, target: "col", epochs: N, lr: F)
    TrainStmt {
        source_var: String,
        model_name: String,
        config: TrainConfig,
    },
}

/// Compilation unit — the whole-file AST
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

impl Program {
    pub fn new() -> Self {
        Program { stmts: Vec::new() }
    }
}

impl Default for Program {
    fn default() -> Self {
        Program::new()
    }
}

// ── AST unit tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── JoinHow tests ────────────────────────────────────────────────────────

    /// JoinHow::parse — four valid strings
    #[test]
    fn test_join_how_parse_valid() {
        assert_eq!(JoinHow::parse("inner"), Some(JoinHow::Inner));
        assert_eq!(JoinHow::parse("left"), Some(JoinHow::Left));
        assert_eq!(JoinHow::parse("outer"), Some(JoinHow::Outer));
        assert_eq!(JoinHow::parse("cross"), Some(JoinHow::Cross));
    }

    /// JoinHow::parse — invalid strings → None
    #[test]
    fn test_join_how_parse_invalid() {
        assert_eq!(JoinHow::parse("hash"), None);
        assert_eq!(JoinHow::parse("INNER"), None); // case-sensitive
        assert_eq!(JoinHow::parse(""), None);
        assert_eq!(JoinHow::parse("full"), None);
    }

    /// JoinHow::default() → Inner
    #[test]
    fn test_join_how_default_is_inner() {
        assert_eq!(JoinHow::default(), JoinHow::Inner);
    }

    /// JoinHow::as_polars_str — verify the Polars type string mapping
    #[test]
    fn test_join_how_as_polars_str() {
        assert_eq!(JoinHow::Inner.as_polars_str(), "JoinType::Inner");
        assert_eq!(JoinHow::Left.as_polars_str(), "JoinType::Left");
        assert_eq!(JoinHow::Outer.as_polars_str(), "JoinType::Full");
        assert_eq!(JoinHow::Cross.as_polars_str(), "JoinType::Cross");
    }

    // ── ChartType tests ──────────────────────────────────────────────────────

    /// ChartType::parse — four valid strings
    #[test]
    fn test_chart_type_parse_valid() {
        assert_eq!(ChartType::parse("bar"), Some(ChartType::Bar));
        assert_eq!(ChartType::parse("line"), Some(ChartType::Line));
        assert_eq!(ChartType::parse("pie"), Some(ChartType::Pie));
        assert_eq!(ChartType::parse("scatter"), Some(ChartType::Scatter));
    }

    /// ChartType::parse — invalid strings → None
    #[test]
    fn test_chart_type_parse_invalid() {
        assert_eq!(ChartType::parse("heatmap"), None);
        assert_eq!(ChartType::parse("Bar"), None); // case-sensitive
        assert_eq!(ChartType::parse(""), None);
        assert_eq!(ChartType::parse("radar"), None);
    }

    /// ChartType::as_str — verify it returns a lowercase string
    #[test]
    fn test_chart_type_as_str() {
        assert_eq!(ChartType::Bar.as_str(), "bar");
        assert_eq!(ChartType::Line.as_str(), "line");
        assert_eq!(ChartType::Pie.as_str(), "pie");
        assert_eq!(ChartType::Scatter.as_str(), "scatter");
    }

    /// ChartType parse / as_str round-trip verification
    #[test]
    fn test_chart_type_roundtrip() {
        for s in &["bar", "line", "pie", "scatter"] {
            let ct = ChartType::parse(s).unwrap();
            assert_eq!(ct.as_str(), *s);
        }
    }

    // ── Program tests ────────────────────────────────────────────────────────

    /// Program::new() → stmts is empty
    #[test]
    fn test_program_new_is_empty() {
        let p = Program::new();
        assert!(p.stmts.is_empty());
    }

    /// Program::default() == Program::new()
    #[test]
    fn test_program_default_equals_new() {
        assert_eq!(Program::default(), Program::new());
    }

    // ── FillNullValue tests ──────────────────────────────────────────────────

    /// FillNullValue PartialEq — equal value comparison
    #[test]
    fn test_fill_null_value_eq() {
        assert_eq!(FillNullValue::Int(0), FillNullValue::Int(0));
        assert_ne!(FillNullValue::Int(0), FillNullValue::Int(1));
        assert_eq!(
            FillNullValue::Str("N/A".into()),
            FillNullValue::Str("N/A".into())
        );
        assert_ne!(
            FillNullValue::Str("N/A".into()),
            FillNullValue::Str("".into())
        );
    }

    // ── Expr Debug / Clone tests ─────────────────────────────────────────────

    /// Expr::Ident Debug output verification
    #[test]
    fn test_expr_ident_debug() {
        let e = Expr::Ident("pm10".into());
        let debug = format!("{:?}", e);
        assert!(debug.contains("pm10"), "Debug 출력에 pm10 없음: {}", debug);
    }

    /// Expr::BinOp Clone verification
    #[test]
    fn test_expr_binop_clone() {
        let e = Expr::BinOp {
            lhs: Box::new(Expr::Ident("a".into())),
            op: BinOpKind::Gt,
            rhs: Box::new(Expr::IntLit(10)),
        };
        let cloned = e.clone();
        assert_eq!(e, cloned);
    }

    // ── D3 hyperparameter sweep expansion ─────────────────────────────────────

    /// A non-sweep config expands to exactly itself.
    #[test]
    fn test_expand_sweep_single() {
        let config = TrainConfig {
            target: "y".into(),
            epochs: 7,
            learning_rate: 0.02,
            batch_size: Some(4),
            ..Default::default()
        };
        assert!(!config.is_sweep());
        let combos = config.expand_sweep();
        assert_eq!(combos.len(), 1);
        assert_eq!(combos[0].epochs, 7);
        assert_eq!(combos[0].learning_rate, 0.02);
        assert_eq!(combos[0].batch_size, Some(4));
        assert!(!combos[0].is_sweep());
    }

    /// List axes expand to the full cartesian product.
    #[test]
    fn test_expand_sweep_cartesian() {
        let mut config = TrainConfig {
            target: "y".into(),
            epochs: 99,
            learning_rate: 0.5,
            ..Default::default()
        };
        config.sweep.epochs = vec![10, 20];
        config.sweep.learning_rate = vec![0.1, 0.01, 0.001];
        assert!(config.is_sweep());
        assert_eq!(config.sweep.len(), 6);

        let combos = config.expand_sweep();
        assert_eq!(combos.len(), 6);
        assert!(combos.iter().all(|c| c.batch_size.is_none()));
        assert!(combos.iter().all(|c| !c.is_sweep()));
        assert!(
            combos
                .iter()
                .any(|c| c.epochs == 10 && c.learning_rate == 0.001)
        );
        assert!(
            combos
                .iter()
                .any(|c| c.epochs == 20 && c.learning_rate == 0.1)
        );
    }

    // ── D3 sweep selection metric ────────────────────────────────────────────

    #[test]
    fn test_sweep_metric_parse_aliases() {
        assert_eq!(SweepMetric::parse(""), Some(SweepMetric::Mse));
        assert_eq!(SweepMetric::parse(" MSE "), Some(SweepMetric::Mse));
        assert_eq!(SweepMetric::parse("l1"), Some(SweepMetric::Mae));
        assert_eq!(SweepMetric::parse("R2"), Some(SweepMetric::R2));
        assert_eq!(SweepMetric::parse("quantum"), None);
        assert_eq!(SweepMetric::default(), SweepMetric::Mse);
        assert!(SweepMetric::Mse.lower_is_better());
        assert!(SweepMetric::Mae.lower_is_better());
        assert!(!SweepMetric::R2.lower_is_better());
    }

    /// The sweep metric survives grid expansion so every combo ranks the same way.
    #[test]
    fn test_expand_sweep_carries_metric() {
        let mut config = TrainConfig {
            target: "y".into(),
            epochs: 1,
            learning_rate: 0.1,
            ..Default::default()
        };
        config.sweep_metric = SweepMetric::R2;
        config.sweep.epochs = vec![1, 2];
        let combos = config.expand_sweep();
        assert_eq!(combos.len(), 2);
        assert!(
            combos.iter().all(|c| c.sweep_metric == SweepMetric::R2),
            "확장된 조합이 선택 지표를 유지해야 함"
        );
    }

    // ── D3 sweep report ordering / top-N ─────────────────────────────────────

    #[test]
    fn test_sweep_sort_parse_aliases() {
        assert_eq!(SweepSort::parse(""), Some(SweepSort::Metric));
        assert_eq!(SweepSort::parse(" Score "), Some(SweepSort::Metric));
        assert_eq!(SweepSort::parse("epoch"), Some(SweepSort::Epochs));
        assert_eq!(SweepSort::parse("learning_rate"), Some(SweepSort::Lr));
        assert_eq!(SweepSort::parse("BATCH_SIZE"), Some(SweepSort::Batch));
        assert_eq!(SweepSort::parse("quantum"), None);
        assert_eq!(SweepSort::default(), SweepSort::Metric);
        assert_eq!(SweepSort::Batch.id(), "batch");
        assert!(!SweepSort::Metric.is_axis());
        assert!(SweepSort::Epochs.is_axis());
        assert!(SweepSort::Lr.is_axis());
        assert!(SweepSort::Batch.is_axis());
    }

    /// `sort`/`top` default off and survive on the original config, but expanded
    /// combos always carry the neutral defaults since they are run individually.
    #[test]
    fn test_expand_sweep_resets_sort_and_top() {
        let mut config = TrainConfig {
            target: "y".into(),
            epochs: 1,
            learning_rate: 0.1,
            ..Default::default()
        };
        config.sweep.epochs = vec![1, 2];
        config.sweep_sort = SweepSort::Lr;
        config.sweep_tiebreak = vec![SweepSort::Batch];
        config.sweep_top = Some(1);
        let combos = config.expand_sweep();
        assert_eq!(combos.len(), 2);
        assert!(combos.iter().all(|c| c.sweep_sort == SweepSort::Metric));
        assert!(combos.iter().all(|c| c.sweep_tiebreak.is_empty()));
        assert!(combos.iter().all(|c| c.sweep_top.is_none()));
    }
}
