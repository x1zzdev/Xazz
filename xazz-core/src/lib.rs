/// xazz-core — Xazz shared core types (v0.3)
///
/// Defines the AST, token, and error types shared by all crates.
/// This crate does not include heavy dependencies such as Polars / Tokio / Rayon.
///
/// v0.3: Added deep-learning model declarations (ModelDecl), training (TrainStmt), and layers (LayerKind)
pub mod ast;
pub mod error;
pub mod i18n;
pub mod ir;
pub mod token;

// ── Top-level exports ────────────────────────────────────────────────────────

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
