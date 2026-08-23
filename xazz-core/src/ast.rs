/// Xazz - AST 노드 정의 (v0.3)
///
/// Polars / Tokio 등 무거운 의존성 없이 순수 Rust 타입만 사용한다.
/// v0.3: Burn 딥러닝 모델 선언(ModelDecl) 및 학습(TrainStmt) AST 추가

/// 표현식 노드
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// 식별자 참조 (변수명 또는 컬럼명)
    Ident(String),
    /// 문자열 리터럴
    StringLit(String),
    /// 정수 리터럴
    IntLit(i64),
    /// 부동소수 리터럴
    FloatLit(f64),
    /// 불리언 리터럴 (true / false)
    BoolLit(bool),
    /// 이항 연산 (lhs op rhs) — 비교 및 산술 연산 포함
    BinOp {
        lhs: Box<Expr>,
        op: BinOpKind,
        rhs: Box<Expr>,
    },
}

/// 이항 연산자 종류 (비교 + 산술)
#[derive(Debug, Clone, PartialEq)]
pub enum BinOpKind {
    // ── 비교 연산자 ──────────────────────
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    // ── 산술 연산자 (v0.16+) ─────────────
    Add,
    Sub,
    Mul,
    Div,
}

/// fillNull 채우기 값 종류
#[derive(Debug, Clone, PartialEq)]
pub enum FillNullValue {
    /// 정수 채우기 값
    Int(i64),
    /// 부동소수 채우기 값
    Float(f64),
    /// 문자열 채우기 값
    Str(String),
    /// 평균 채우기 전략 (strategy: "mean")
    Mean,
    /// 중앙값 채우기 전략 (strategy: "median")
    Median,
    /// 0으로 채우기 전략 (strategy: "zero")
    Zero,
}

/// join 방식
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
    /// 소문자 문자열에서 파싱
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

// ── v0.19 시각화 타입 ──────────────────────────────────────────────────────────

/// 차트 종류 (MVP: bar / line / pie / scatter)
#[derive(Debug, Clone, PartialEq)]
pub enum ChartType {
    Bar,
    Line,
    Pie,
    Scatter,
}

impl ChartType {
    /// 식별자 문자열에서 파싱
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

/// chart { ... } 블록 설정값
#[derive(Debug, Clone, PartialEq)]
pub struct ChartConfig {
    pub chart_type: ChartType,
    pub title: Option<String>,
    /// x축 컬럼명 (bar, line, scatter 용)
    pub x: Option<String>,
    /// y축 컬럼명 (bar, line, scatter 용)
    pub y: Option<String>,
    /// 레이블 컬럼명 (pie 용)
    pub label: Option<String>,
    /// 값 컬럼명 (pie 용)
    pub value: Option<String>,
}

/// 파이프라인 연산 단계
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineOp {
    /// filter(<조건식>)
    Filter(Expr),
    /// select([col1, col2, ...])
    Select(Vec<String>),
    /// count  (None: 전체 행 수용 플래그) / count("col")  (Some: 그룹 집계)
    Count(Option<String>),
    /// groupBy("col")  — 이후 Sum/Mean/Min/Max/Count(Some) 와 쌍으로 사용
    GroupBy(String),
    /// sum("col")  — 단독 또는 groupBy 뒤에 사용
    Sum(String),
    /// mean("col")  — 단독 또는 groupBy 뒤에 사용
    Mean(String),
    /// min("col")  — 단독 또는 groupBy 뒤에 사용
    Min(String),
    /// max("col")  — 단독 또는 groupBy 뒤에 사용
    Max(String),
    /// orderBy("col", desc: true/false)
    OrderBy { col: String, desc: bool },
    /// take(n)  — 상위 n 행만 유지
    Take(i64),
    /// dropNull("col")  — 해당 컬럼이 null인 행 제거
    DropNull(String),
    /// fillNull("col", value)  — 해당 컬럼의 null을 value로 채우기
    FillNull { col: String, value: FillNullValue },
    /// join(other_var, left_on/right_on, how)
    Join {
        other: String,
        left_on: Vec<String>,
        right_on: Vec<String>,
        how: JoinHow,
    },
    /// withColumn("new_col", expr)  — 새로운 컬럼 추가/변환
    WithColumn { name: String, expr: Expr },
    /// chart { type: ..., x: ..., y: ..., title: "..." }  — 파이프라인 시각화 (v0.19)
    Chart(ChartConfig),
    /// cast("col", "float")  — 컬럼 타입을 DSL 레벨에서 명시적으로 캐스팅 (v0.20)
    Cast { col: String, to_type: String },
    /// rename("old_name", "new_name") — 컬럼 이름 변경 (v0.21)
    Rename { old_name: String, new_name: String },
    /// replace("col", ".", "") — 문자열 치환 (v0.21)
    Replace {
        col: String,
        from: String,
        to: String,
    },
    /// sample(n) / sample(n, seed: 42) — 무작위 샘플링 (v0.22)
    Sample { n: i64, seed: Option<i64> },
    /// median("col") — 중앙값 집계 (v0.22)
    Median(String),
    /// variance("col") — 분산 집계 (v0.22)
    Variance(String),
    /// std("col") — 표준편차 집계 (v0.22)
    Std(String),
    /// train(ModelName, target: "col", epochs: N, lr: F) — 학습 연산자 (v0.5)
    Train {
        model_name: String,
        config: TrainConfig,
    },
    /// predict(model_var, as: "col") — 학습 모델 예측 연산자 (v0.5)
    Predict {
        model_var: String,
        as_col: Option<String>,
    },
}

/// 파이프라인의 소스 (데이터 원천)
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineSource {
    /// load("파일경로") :: SchemaName
    Load {
        file_path: String,
        schema_name: String,
    },
    /// 이미 선언된 변수를 참조
    VarRef(String),
}

/// 타입 선언의 필드 하나
#[derive(Debug, Clone, PartialEq)]
pub struct StructField {
    pub name: String,
    pub field_type: String,
}

/// 딥러닝 레이어 종류 (Burn 매핑)
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
    /// Burn 코드 생성용 문자열 반환 (입력 차원은 데이터셋 스키마에서 결정되므로 함수 인자로 받는다).
    pub fn to_burn_str(&self) -> String {
        match self {
            LayerKind::Dense(n) => format!("nn::LinearConfig::new(<in_dim>, {})", n),
            LayerKind::ReLU => "activation::relu()".to_string(),
            LayerKind::Sigmoid => "activation::sigmoid()".to_string(),
            LayerKind::Tanh => "activation::tanh()".to_string(),
            LayerKind::Softmax => "activation::softmax(dim=1)".to_string(),
            LayerKind::Dropout(r) => format!("nn::DropoutConfig::new({})", r),
            LayerKind::BatchNorm => "BatchNorm(생략)".to_string(),
        }
    }
}

/// 학습 하이퍼파라미터 설정
#[derive(Debug, Clone, PartialEq)]
pub struct TrainConfig {
    /// 학습 대상 컬럼 (target)
    pub target: String,
    /// 에폭 수
    pub epochs: usize,
    /// 학습률 (learning rate)
    pub learning_rate: f64,
    /// 배치 크기 (None: 전체 데이터)
    pub batch_size: Option<usize>,
    /// 검증 데이터 비율 (0.0 ~ 1.0)
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

/// 최상위 구문 노드
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
    /// expression statement: 변수에 할당하지 않고 파이프라인 실행 (결과 버림)
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

/// 컴파일 단위 — 파일 전체 AST
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

// ── AST 유닛 테스트 ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── JoinHow 테스트 ────────────────────────────────────────────────────────

    /// JoinHow::from_str — 유효한 문자열 4종
    #[test]
    fn test_join_how_from_str_valid() {
        assert_eq!(JoinHow::from_str("inner"), Some(JoinHow::Inner));
        assert_eq!(JoinHow::from_str("left"), Some(JoinHow::Left));
        assert_eq!(JoinHow::from_str("outer"), Some(JoinHow::Outer));
        assert_eq!(JoinHow::from_str("cross"), Some(JoinHow::Cross));
    }

    /// JoinHow::from_str — 유효하지 않은 문자열 → None
    #[test]
    fn test_join_how_from_str_invalid() {
        assert_eq!(JoinHow::from_str("hash"), None);
        assert_eq!(JoinHow::from_str("INNER"), None); // 대소문자 구분
        assert_eq!(JoinHow::from_str(""), None);
        assert_eq!(JoinHow::from_str("full"), None);
    }

    /// JoinHow::default() → Inner
    #[test]
    fn test_join_how_default_is_inner() {
        assert_eq!(JoinHow::default(), JoinHow::Inner);
    }

    /// JoinHow::as_polars_str — Polars 타입 문자열 매핑 검증
    #[test]
    fn test_join_how_as_polars_str() {
        assert_eq!(JoinHow::Inner.as_polars_str(), "JoinType::Inner");
        assert_eq!(JoinHow::Left.as_polars_str(), "JoinType::Left");
        assert_eq!(JoinHow::Outer.as_polars_str(), "JoinType::Full");
        assert_eq!(JoinHow::Cross.as_polars_str(), "JoinType::Cross");
    }

    // ── ChartType 테스트 ──────────────────────────────────────────────────────

    /// ChartType::from_str — 유효한 문자열 4종
    #[test]
    fn test_chart_type_from_str_valid() {
        assert_eq!(ChartType::from_str("bar"), Some(ChartType::Bar));
        assert_eq!(ChartType::from_str("line"), Some(ChartType::Line));
        assert_eq!(ChartType::from_str("pie"), Some(ChartType::Pie));
        assert_eq!(ChartType::from_str("scatter"), Some(ChartType::Scatter));
    }

    /// ChartType::from_str — 유효하지 않은 문자열 → None
    #[test]
    fn test_chart_type_from_str_invalid() {
        assert_eq!(ChartType::from_str("heatmap"), None);
        assert_eq!(ChartType::from_str("Bar"), None); // 대소문자 구분
        assert_eq!(ChartType::from_str(""), None);
        assert_eq!(ChartType::from_str("radar"), None);
    }

    /// ChartType::as_str — 소문자 문자열 반환 검증
    #[test]
    fn test_chart_type_as_str() {
        assert_eq!(ChartType::Bar.as_str(), "bar");
        assert_eq!(ChartType::Line.as_str(), "line");
        assert_eq!(ChartType::Pie.as_str(), "pie");
        assert_eq!(ChartType::Scatter.as_str(), "scatter");
    }

    /// ChartType from_str / as_str 왕복 변환 검증
    #[test]
    fn test_chart_type_roundtrip() {
        for s in &["bar", "line", "pie", "scatter"] {
            let ct = ChartType::from_str(s).unwrap();
            assert_eq!(ct.as_str(), *s);
        }
    }

    // ── Program 테스트 ────────────────────────────────────────────────────────

    /// Program::new() → stmts 비어 있음
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

    // ── FillNullValue 테스트 ──────────────────────────────────────────────────

    /// FillNullValue PartialEq — 같은 값 비교
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

    // ── Expr Debug / Clone 테스트 ─────────────────────────────────────────────

    /// Expr::Ident Debug 출력 검증
    #[test]
    fn test_expr_ident_debug() {
        let e = Expr::Ident("pm10".into());
        let debug = format!("{:?}", e);
        assert!(debug.contains("pm10"), "Debug 출력에 pm10 없음: {}", debug);
    }

    /// Expr::BinOp Clone 검증
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
