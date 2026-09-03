/// Xazz - AST node definitions (v0.3)
///
/// Uses only plain Rust types, with no heavy dependencies such as Polars / Tokio.
/// v0.3: Added Burn deep-learning model declaration (ModelDecl) and training (TrainStmt) AST

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
#[derive(Debug, Clone, PartialEq)]
pub enum JoinHow {
    Inner,
    Left,
    Outer,
    Cross,
}

impl Default for JoinHow {
    fn default() -> Self {
        JoinHow::Inner
    }
}

impl JoinHow {
    /// Parse from a lowercase string
    pub fn from_str(s: &str) -> Option<Self> {
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
    pub fn from_str(s: &str) -> Option<Self> {
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
    Save {
        path: String,
        format: SaveFormat,
    },
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
    pub fn from_str(s: &str) -> Option<Self> {
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

/// Pipeline source (data origin)
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineSource {
    /// load("file_path") :: SchemaName
    Load {
        file_path: String,
        schema_name: String,
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
        }
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
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig {
            target: String::new(),
            epochs: 10,
            learning_rate: 0.01,
            batch_size: None,
            validation_split: None,
        }
    }
}

/// Top-level statement node
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
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

    /// JoinHow::from_str — four valid strings
    #[test]
    fn test_join_how_from_str_valid() {
        assert_eq!(JoinHow::from_str("inner"), Some(JoinHow::Inner));
        assert_eq!(JoinHow::from_str("left"), Some(JoinHow::Left));
        assert_eq!(JoinHow::from_str("outer"), Some(JoinHow::Outer));
        assert_eq!(JoinHow::from_str("cross"), Some(JoinHow::Cross));
    }

    /// JoinHow::from_str — invalid strings → None
    #[test]
    fn test_join_how_from_str_invalid() {
        assert_eq!(JoinHow::from_str("hash"), None);
        assert_eq!(JoinHow::from_str("INNER"), None); // case-sensitive
        assert_eq!(JoinHow::from_str(""), None);
        assert_eq!(JoinHow::from_str("full"), None);
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

    /// ChartType::from_str — four valid strings
    #[test]
    fn test_chart_type_from_str_valid() {
        assert_eq!(ChartType::from_str("bar"), Some(ChartType::Bar));
        assert_eq!(ChartType::from_str("line"), Some(ChartType::Line));
        assert_eq!(ChartType::from_str("pie"), Some(ChartType::Pie));
        assert_eq!(ChartType::from_str("scatter"), Some(ChartType::Scatter));
    }

    /// ChartType::from_str — invalid strings → None
    #[test]
    fn test_chart_type_from_str_invalid() {
        assert_eq!(ChartType::from_str("heatmap"), None);
        assert_eq!(ChartType::from_str("Bar"), None); // case-sensitive
        assert_eq!(ChartType::from_str(""), None);
        assert_eq!(ChartType::from_str("radar"), None);
    }

    /// ChartType::as_str — verify it returns a lowercase string
    #[test]
    fn test_chart_type_as_str() {
        assert_eq!(ChartType::Bar.as_str(), "bar");
        assert_eq!(ChartType::Line.as_str(), "line");
        assert_eq!(ChartType::Pie.as_str(), "pie");
        assert_eq!(ChartType::Scatter.as_str(), "scatter");
    }

    /// ChartType from_str / as_str round-trip verification
    #[test]
    fn test_chart_type_roundtrip() {
        for s in &["bar", "line", "pie", "scatter"] {
            let ct = ChartType::from_str(s).unwrap();
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
}
