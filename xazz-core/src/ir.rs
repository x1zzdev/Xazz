/// xazz-core/src/ir.rs — Typed IR (semantic representation with static types) v0.3
///
/// The intermediate layer between the AST (syntax) and the backends (Polars/Burn).
/// The type checker builds this IR while validating the AST, and the execution
/// engine consumes this IR (not the raw AST) exactly once to lower to a backend.
///
/// Design principles:
///   - Only plain Rust types, no heavy dependencies. (xazz-core shared kernel)
///   - Every expression (TypedExpr) carries its result column type (ColType).
///   - Data/ML/side effects are split into domain enums (DataOp/MLOp/SideOp), but
///     stored as a sequential `Step`-tagged sequence to **preserve pipeline order**.
///     (e.g. `filter |> withDp |> select` and `filter |> select |> withDp` differ.)
use crate::ast::{BinOpKind, ChartConfig, DpArgs, JoinHow, LayerKind, TrainConfig};

// ─────────────────────────────────────────────────────────────────────────────
// Column types / schema
// ─────────────────────────────────────────────────────────────────────────────

/// Canonical type of a column (used for both schema declarations and expression type inference).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColType {
    String,
    Int,
    Float,
    Bool,
    /// Column whose type cannot be determined (e.g. via an undeclared schema).
    Unknown,
    /// Nullable column. Wraps the `T` of `Option<T>`.
    Nullable(Box<ColType>),
}

impl ColType {
    /// Whether the type is nullable.
    pub fn is_option(&self) -> bool {
        matches!(self, ColType::Nullable(_))
    }

    /// The inner type with nullable stripped off.
    pub fn inner(&self) -> &ColType {
        match self {
            ColType::Nullable(t) => t.inner(),
            other => other,
        }
    }

    /// Whether it is numeric (int/float).
    pub fn is_numeric(&self) -> bool {
        matches!(self.inner(), ColType::Int | ColType::Float)
    }

    /// Canonical type name string.
    pub fn name(&self) -> &'static str {
        match self.inner() {
            ColType::String => "string",
            ColType::Int => "int",
            ColType::Float => "float",
            ColType::Bool => "bool",
            ColType::Unknown => "unknown",
            ColType::Nullable(_) => unreachable!("inner() 은 Nullable 을 벗겨낸다"),
        }
    }
}

/// A single field of a schema (name + column type).
#[derive(Debug, Clone, PartialEq)]
pub struct SchemaField {
    pub name: String,
    pub ty: ColType,
}

impl SchemaField {
    pub fn new(name: impl Into<String>, ty: ColType) -> Self {
        SchemaField {
            name: name.into(),
            ty,
        }
    }
}

/// An unnamed column schema (represents pipeline input/output types).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: Vec<SchemaField>,
}

impl Schema {
    pub fn new(fields: Vec<SchemaField>) -> Self {
        Schema { fields }
    }

    /// Find a field by column name.
    pub fn find(&self, name: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// Find the column type by column name.
    pub fn ty_of(&self, name: &str) -> Option<&ColType> {
        self.find(name).map(|f| &f.ty)
    }

    /// List of column names.
    pub fn names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Typed expressions
// ─────────────────────────────────────────────────────────────────────────────

/// An expression with an attached type. `ty` is the type of the column/value produced by evaluating this expression.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: ColType,
}

impl TypedExpr {
    pub fn new(kind: TypedExprKind, ty: ColType) -> Self {
        TypedExpr { kind, ty }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypedExprKind {
    /// Column reference.
    Column(String),
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    BinOp {
        op: BinOpKind,
        lhs: Box<TypedExpr>,
        rhs: Box<TypedExpr>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Data operations (Data IR — lowered to Polars)
// ─────────────────────────────────────────────────────────────────────────────

/// fillNull fill value (strategy/literal).
#[derive(Debug, Clone, PartialEq)]
pub enum FillValue {
    Int(i64),
    Float(f64),
    Str(String),
    Mean,
    Median,
    Zero,
}

/// Aggregation kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggKind {
    Count,
    /// count() (no arguments) — row count; counts rows per group for grouped aggregation.
    Len,
    Sum,
    Mean,
    Min,
    Max,
    Median,
    Variance,
    Std,
}

/// Data-layer operations. Lowered to a Polars LazyFrame.
#[derive(Debug, Clone, PartialEq)]
pub enum DataOp {
    Filter(TypedExpr),
    Select(Vec<String>),
    GroupBy(String),
    /// Aggregation (grouped if a preceding GroupBy exists, global otherwise).
    Aggregate {
        kind: AggKind,
        col: String,
    },
    Sort {
        col: String,
        desc: bool,
    },
    Limit(i64),
    Sample {
        n: i64,
        seed: Option<i64>,
    },
    DropNull(String),
    FillNull {
        col: String,
        value: FillValue,
    },
    Join {
        other: String,
        left_on: Vec<String>,
        right_on: Vec<String>,
        how: JoinHow,
    },
    WithColumn {
        name: String,
        expr: TypedExpr,
    },
    Cast {
        col: String,
        to: String,
    },
    Rename {
        old: String,
        new: String,
    },
    Replace {
        col: String,
        from: String,
        to: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// ML operations (ML IR — lowered to Burn)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MLOp {
    Train {
        model: String,
        config: TrainConfig,
    },
    Predict {
        model: String,
        as_col: Option<String>,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Side operations (visualization / privacy — separate subsystem)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SideOp {
    Chart(ChartConfig),
    WithDp(DpArgs),
}

// ─────────────────────────────────────────────────────────────────────────────
// Pipeline steps / nodes
// ─────────────────────────────────────────────────────────────────────────────

/// A single step of a pipeline. Wraps domain-specific enums in a tag to preserve order.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Data(DataOp),
    ML(MLOp),
    Side(SideOp),
}

/// Pipeline source (data origin).
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Load {
        file_path: String,
        /// Schema bound via `:: SchemaName` (None if absent).
        schema: Option<Schema>,
    },
    /// Reference to an already-declared variable.
    Ref { var: String },
}

/// A typed node corresponding to a single pipeline (variable declaration).
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineNode {
    /// Sequential index within the program (0-based).
    pub id: usize,
    /// Variable name (None for ExprStmt).
    pub name: Option<String>,
    pub source: Source,
    /// Schema at the start of the pipeline (None if undeterminable).
    pub input_schema: Option<Schema>,
    /// Schema at the end of the pipeline.
    pub output_schema: Schema,
    /// Order-preserving sequence of steps.
    pub steps: Vec<Step>,
    /// Whether the pipeline ends with train() and becomes a model variable.
    pub yields_model: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// Whole program
// ─────────────────────────────────────────────────────────────────────────────

/// A named type declaration (`type Name = { ... }`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub schema: Schema,
}

/// Model declaration (`model Name { ... }`) — an ML graph lowered to Burn.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGraph {
    pub name: String,
    pub layers: Vec<LayerKind>,
}

/// The typed program of the whole compilation unit.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TypedProgram {
    pub types: Vec<TypeDecl>,
    pub models: Vec<ModelGraph>,
    pub pipelines: Vec<PipelineNode>,
}

impl TypedProgram {
    pub fn new() -> Self {
        TypedProgram::default()
    }

    /// Find a type declaration by name.
    pub fn type_decl(&self, name: &str) -> Option<&TypeDecl> {
        self.types.iter().find(|t| t.name == name)
    }

    /// Find a model by name.
    pub fn model(&self, name: &str) -> Option<&ModelGraph> {
        self.models.iter().find(|m| m.name == name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn col(name: &str, ty: ColType) -> SchemaField {
        SchemaField::new(name, ty)
    }

    #[test]
    fn col_type_numeric_and_option() {
        assert!(ColType::Float.is_numeric());
        assert!(ColType::Int.is_numeric());
        assert!(!ColType::String.is_numeric());
        assert!(!ColType::Bool.is_numeric());

        let opt = ColType::Nullable(Box::new(ColType::Float));
        assert!(opt.is_option());
        assert!(opt.is_numeric(), "Nullable<float> 은 숫자형");
        assert_eq!(opt.name(), "float");
        assert_eq!(opt.inner(), &ColType::Float);
    }

    #[test]
    fn schema_lookup() {
        let s = Schema::new(vec![
            col("station", ColType::String),
            col("pm10", ColType::Nullable(Box::new(ColType::Float))),
        ]);
        assert!(s.find("station").is_some());
        assert!(s.find("nope").is_none());
        assert_eq!(s.ty_of("pm10").map(|t| t.is_option()), Some(true));
        assert_eq!(s.names(), vec!["station", "pm10"]);
    }

    #[test]
    fn typed_expr_carries_type() {
        let e = TypedExpr::new(
            TypedExprKind::BinOp {
                op: BinOpKind::Gt,
                lhs: Box::new(TypedExpr::new(
                    TypedExprKind::Column("pm10".into()),
                    ColType::Float,
                )),
                rhs: Box::new(TypedExpr::new(TypedExprKind::Int(10), ColType::Int)),
            },
            ColType::Bool,
        );
        assert_eq!(e.ty, ColType::Bool);
    }

    #[test]
    fn step_preserves_domain_and_order() {
        let steps = vec![
            Step::Data(DataOp::Select(vec!["a".into()])),
            Step::Side(SideOp::WithDp(DpArgs::default())),
            Step::Data(DataOp::Filter(TypedExpr::new(
                TypedExprKind::Bool(true),
                ColType::Bool,
            ))),
        ];
        assert_eq!(steps.len(), 3);
        assert!(matches!(steps[0], Step::Data(DataOp::Select(_))));
        assert!(matches!(steps[1], Step::Side(SideOp::WithDp(_))));
        assert!(matches!(steps[2], Step::Data(DataOp::Filter(_))));
    }

    #[test]
    fn typed_program_lookup() {
        let mut p = TypedProgram::new();
        p.types.push(TypeDecl {
            name: "T".into(),
            schema: Schema::new(vec![col("a", ColType::Int)]),
        });
        p.models.push(ModelGraph {
            name: "M".into(),
            layers: vec![LayerKind::Dense(1)],
        });
        assert!(p.type_decl("T").is_some());
        assert!(p.model("M").is_some());
        assert!(p.type_decl("X").is_none());
    }
}
