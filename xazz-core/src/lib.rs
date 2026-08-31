/// xazz-core — Xazz 공유 핵심 타입 (v0.3)
///
/// 모든 크레이트가 공유하는 AST, 토큰, 에러 타입을 정의한다.
/// 이 크레이트는 Polars / Tokio / Rayon 등 무거운 의존성을 포함하지 않는다.
///
/// v0.3: 딥러닝 모델 선언(ModelDecl), 학습(TrainStmt), 레이어(LayerKind) 추가
pub mod ast;
pub mod error;
pub mod i18n;
pub mod ir;
pub mod token;

// ── 상위 노출 ────────────────────────────────────────────────────────────────

// i18n
pub use i18n::{Lang, tr};

// token
pub use token::{Span, Token, TokenKind};

// ast
pub use ast::{
    BinOpKind, ChartConfig, ChartType, DpArgs, DpMechanism, Expr, FillNullValue, JoinHow,
    LayerKind, PipelineOp, PipelineSource, Program, Stmt, StructField, TrainConfig,
};

// error
pub use error::{CompileError, CompileResult, ErrorKind};

// ir
pub use ir::{
    AggKind, ColType, DataOp, FillValue, MLOp, ModelGraph, PipelineNode, Schema, SchemaField,
    SideOp, Source, Step, TypeDecl, TypedExpr, TypedExprKind, TypedProgram,
};
