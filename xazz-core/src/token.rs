/// xazzLang - Token definitions
/// Span: 소스 위치 정보

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
    // ── 연산자 ──────────────────────────────────────
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

    // ── 구분자 ──────────────────────────────────────
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
    /// :  (단일 콜론 — 필드 타입 구분자)
    Colon,

    // ── 키워드 ──────────────────────────────────────
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
    /// on   (join 의 명명 인수)
    On,
    /// how  (join 의 명명 인수)
    How,
    /// v  (불변 변수 선언)
    V,
    /// mut
    Mut,
    /// Option  (Option<T> 타입 키워드)
    OptionKw,
    /// true (불리언 리터럴)
    True,
    /// false (불리언 리터럴)
    False,
    /// desc  (orderBy 내 정렬 방향 키워드)
    Desc,
    /// chart  (파이프라인 시각화 연산)
    Chart,
    /// cast  (타입 캐스팅 연산 — cast("col", "float"))
    Cast,
    /// rename  (컬럼 이름 변경 — rename("old", "new"))
    Rename,
    /// replace  (문자열 치환 연산 — replace("col", ".", ""))
    Replace,
    /// left_on  (join 의 left 키 명명 인수)
    LeftOn,
    /// right_on  (join 의 right 키 명명 인수)
    RightOn,
    /// sample  (샘플링 연산 — sample(n) / sample(n, seed: 42))
    Sample,
    /// median  (중앙값 집계)
    Median,
    /// variance  (분산 집계)
    Variance,
    /// std  (표준편차 집계)
    Std,
    /// seed  (sample 의 명명 인수)
    Seed,

    // ── 리터럴 / 식별자 ─────────────────────────────
    /// 일반 식별자
    Ident(String),
    /// 문자열 리터럴
    StringLit(String),
    /// 정수 리터럴
    IntLit(i64),
    /// 부동소수 리터럴
    FloatLit(f64),

    // ── 파일 끝 ─────────────────────────────────────
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
