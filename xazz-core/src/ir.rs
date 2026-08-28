/// xazz-core/src/ir.rs — Typed IR (정적 타입이 부착된 의미 표현) v0.3
///
/// AST(구문)와 백엔드(Polars/Burn) 사이의 중간 계층.
/// 타입체커(semantic analysis)가 AST를 검사하며 이 IR을 생성하고,
/// 실행 엔진은 raw AST 대신 이 IR을 1회 소비하여 백엔드를 lowering 한다.
///
/// 설계 원칙:
///   - 무거운 의존성 없이 순수 Rust 타입만 사용한다. (xazz-core 공유 커널)
///   - 모든 표현식(TypedExpr)은 결과 컬럼 타입(ColType)을 가진다.
///   - 데이터/ML/부수 연산을 도메인별 enum(DataOp/MLOp/SideOp)으로 분리하되,
///     파이프라인의 **순서 보존**을 위해 `Step` 태그로 감싼 순차 시퀀스로 저장한다.
///     (예: `filter |> withDp |> select` 와 `filter |> select |> withDp` 는 의미가 다르다.)

use crate::ast::{BinOpKind, ChartConfig, DpArgs, JoinHow, LayerKind, TrainConfig};

// ─────────────────────────────────────────────────────────────────────────────
// 컬럼 타입 / 스키마
// ─────────────────────────────────────────────────────────────────────────────

/// 컬럼의 정규화된 타입 (스키마 선언과 표현식 추론 결과에 공통으로 사용).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ColType {
    String,
    Int,
    Float,
    Bool,
    /// 타입을 결정할 수 없는 컬럼 (예: 미선언 스키마 경유).
    Unknown,
    /// 널 허용 컬럼. `Option<T>` 의 T 를 감싼다.
    Nullable(Box<ColType>),
}

impl ColType {
    /// nullable 여부.
    pub fn is_option(&self) -> bool {
        matches!(self, ColType::Nullable(_))
    }

    /// nullable 을 벗겨낸 내부 타입.
    pub fn inner(&self) -> &ColType {
        match self {
            ColType::Nullable(t) => t.inner(),
            other => other,
        }
    }

    /// 숫자형(int/float)인지 여부.
    pub fn is_numeric(&self) -> bool {
        matches!(self.inner(), ColType::Int | ColType::Float)
    }

    /// canonical 타입명 문자열.
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

/// 스키마의 필드 하나 (이름 + 컬럼 타입).
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

/// 이름 없는 컬럼 스키마 (파이프라인 입력/출력 타입 표현).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Schema {
    pub fields: Vec<SchemaField>,
}

impl Schema {
    pub fn new(fields: Vec<SchemaField>) -> Self {
        Schema { fields }
    }

    /// 컬럼명으로 필드를 찾는다.
    pub fn find(&self, name: &str) -> Option<&SchemaField> {
        self.fields.iter().find(|f| f.name == name)
    }

    /// 컬럼명으로 컬럼 타입을 찾는다.
    pub fn ty_of(&self, name: &str) -> Option<&ColType> {
        self.find(name).map(|f| &f.ty)
    }

    /// 컬럼명 목록.
    pub fn names(&self) -> Vec<&str> {
        self.fields.iter().map(|f| f.name.as_str()).collect()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 타입이 붙은 표현식
// ─────────────────────────────────────────────────────────────────────────────

/// 타입이 부착된 표현식. `ty` 는 이 표현식이 평가되어 만들어내는 컬럼/값의 타입.
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
    /// 컬럼 참조.
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
// 데이터 연산 (Data IR — Polars lowering 대상)
// ─────────────────────────────────────────────────────────────────────────────

/// fillNull 채우기 값 (전략형/리터럴).
#[derive(Debug, Clone, PartialEq)]
pub enum FillValue {
    Int(i64),
    Float(f64),
    Str(String),
    Mean,
    Median,
    Zero,
}

/// 집계 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggKind {
    Count,
    Sum,
    Mean,
    Min,
    Max,
    Median,
    Variance,
    Std,
}

/// 데이터 계층 연산. Polars LazyFrame 으로 lowering 되는 대상.
#[derive(Debug, Clone, PartialEq)]
pub enum DataOp {
    Filter(TypedExpr),
    Select(Vec<String>),
    GroupBy(String),
    /// 집계 (선행 GroupBy 가 있으면 그룹 집계, 없으면 전역 집계).
    Aggregate { kind: AggKind, col: String },
    Sort { col: String, desc: bool },
    Limit(i64),
    Sample { n: i64, seed: Option<i64> },
    DropNull(String),
    FillNull { col: String, value: FillValue },
    Join {
        other: String,
        left_on: Vec<String>,
        right_on: Vec<String>,
        how: JoinHow,
    },
    WithColumn { name: String, expr: TypedExpr },
    Cast { col: String, to: String },
    Rename { old: String, new: String },
    Replace { col: String, from: String, to: String },
}

// ─────────────────────────────────────────────────────────────────────────────
// ML 연산 (ML IR — Burn lowering 대상)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum MLOp {
    Train { model: String, config: TrainConfig },
    Predict { model: String, as_col: Option<String> },
}

// ─────────────────────────────────────────────────────────────────────────────
// 부수 연산 (시각화 / 프라이버시 — 별도 하위시스템)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SideOp {
    Chart(ChartConfig),
    WithDp(DpArgs),
}

// ─────────────────────────────────────────────────────────────────────────────
// 파이프라인 단계 / 노드
// ─────────────────────────────────────────────────────────────────────────────

/// 파이프라인의 한 단계. 도메인별 enum 을 태그로 감싸 순서를 보존한다.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Data(DataOp),
    ML(MLOp),
    Side(SideOp),
}

/// 파이프라인의 소스 (데이터 원천).
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    Load {
        file_path: String,
        /// `:: SchemaName` 으로 바인딩된 스키마 (없으면 None).
        schema: Option<Schema>,
    },
    /// 이미 선언된 변수를 참조.
    Ref { var: String },
}

/// 하나의 파이프라인(변수 선언)에 대응하는 타입이 붙은 노드.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineNode {
    /// 프로그램 내 순번 (0-based).
    pub id: usize,
    /// 변수명 (ExprStmt 는 None).
    pub name: Option<String>,
    pub source: Source,
    /// 파이프라인 시작 시점의 스키마 (결정 불가 시 None).
    pub input_schema: Option<Schema>,
    /// 파이프라인 종료 시점의 스키마.
    pub output_schema: Schema,
    /// 순서 보존된 단계 시퀀스.
    pub steps: Vec<Step>,
    /// train() 으로 끝나 모델 변수가 되는지 여부.
    pub yields_model: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
// 프로그램 전체
// ─────────────────────────────────────────────────────────────────────────────

/// 이름 있는 타입 선언 (`type Name = { ... }`).
#[derive(Debug, Clone, PartialEq)]
pub struct TypeDecl {
    pub name: String,
    pub schema: Schema,
}

/// 모델 선언 (`model Name { ... }`) — Burn 으로 lowering 되는 ML 그래프.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelGraph {
    pub name: String,
    pub layers: Vec<LayerKind>,
}

/// 컴파일 단위 전체의 타입이 붙은 프로그램.
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

    /// 이름으로 타입 선언을 찾는다.
    pub fn type_decl(&self, name: &str) -> Option<&TypeDecl> {
        self.types.iter().find(|t| t.name == name)
    }

    /// 이름으로 모델을 찾는다.
    pub fn model(&self, name: &str) -> Option<&ModelGraph> {
        self.models.iter().find(|m| m.name == name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 테스트
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
