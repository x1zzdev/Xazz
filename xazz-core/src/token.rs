/// xazzLang - Token definitions
/// Span: source location information

#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(line: usize, col: usize) -> Self {
        Span { line, col }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // ── Operators ──────────────────────────────────────
    /// |>
    Pipeline,
    /// ::
    TypeAssign,
    /// =
    Assign,
    /// ==
    EqEq,
    /// !=
    NotEq,
    /// <
    Lt,
    /// >
    Gt,
    /// <=
    LtEq,
    /// >=
    GtEq,
    /// +
    Plus,
    /// -
    Minus,
    /// *
    Star,
    /// /
    Slash,
    /// !
    Bang,
    /// .
    Dot,

    // ── Delimiters ──────────────────────────────────────
    /// {
    LBrace,
    /// }
    RBrace,
    /// (
    LParen,
    /// )
    RParen,
    /// [
    LBracket,
    /// ]
    RBracket,
    /// ,
    Comma,
    /// ;
    Semicolon,
    /// :  (single colon — field type separator)
    Colon,

    // ── Keywords ──────────────────────────────────────
    /// type
    Type,
    /// load
    Load,
    /// filter
    Filter,
    /// select
    Select,
    /// count
    Count,
    /// groupBy
    GroupBy,
    /// sum
    Sum,
    /// mean
    Mean,
    /// min
    Min,
    /// max
    Max,
    /// orderBy
    OrderBy,
    /// take
    Take,
    /// dropNull
    DropNull,
    /// fillNull
    FillNull,
    /// join
    Join,
    /// withColumn
    WithColumn,
    /// on   (named argument of join)
    On,
    /// how  (named argument of join)
    How,
    /// v  (immutable variable declaration)
    V,
    /// mut
    Mut,
    /// Option  (Option<T> type keyword)
    OptionKw,
    /// true (boolean literal)
    True,
    /// false (boolean literal)
    False,
    /// desc  (sort-direction keyword in orderBy)
    Desc,
    /// chart  (pipeline visualization operation)
    Chart,
    /// cast  (type-casting operation — cast("col", "float"))
    Cast,
    /// rename  (column rename — rename("old", "new"))
    Rename,
    /// replace  (string replacement operation — replace("col", ".", ""))
    Replace,
    /// left_on  (left-key named argument of join)
    LeftOn,
    /// right_on  (right-key named argument of join)
    RightOn,
    /// sample  (sampling operation — sample(n) / sample(n, seed: 42))
    Sample,
    /// median  (median aggregation)
    Median,
    /// variance  (variance aggregation)
    Variance,
    /// std  (standard deviation aggregation)
    Std,
    /// seed  (named argument of sample)
    Seed,

    // ── Deep-learning keywords (v0.3) ─────────────────────────
    /// model  (model declaration)
    Model,
    /// run  (run training)
    Run,
    /// train  (training operation)
    Train,
    /// epochs  (named argument of train)
    Epochs,
    /// lr  (named argument of train — learning rate)
    Lr,
    /// target  (named argument of train)
    Target,
    /// withDp named-argument keywords (v0.6)
    /// strategy  (named argument of fillNull)
    Strategy,
    /// save  (output artifact operator — save("out.parquet", format: "parquet"), issue #52)
    Save,
    /// ->  (layer-chaining operator)
    Arrow,

    // ── Literals / identifiers ─────────────────────────────
    /// Generic identifier
    Ident(String),
    /// String literal
    StringLit(String),
    /// Integer literal
    IntLit(i64),
    /// Floating-point literal
    FloatLit(f64),

    // ── End of file ─────────────────────────────────────
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Token { kind, span }
    }
}
