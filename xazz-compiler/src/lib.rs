/// Xazz Compiler Library
///
/// Included modules:
///   - token   → re-exports xazz-core::token (Span, Token, TokenKind)
///   - ast     → re-exports xazz-core::ast (Expr, Stmt, Program, PipelineOp, ...)
///   - error   → re-exports xazz-core::error (CompileError, ErrorKind, ...)
///   - lexer   → Lexer (source string → Token array)
///   - parser  → Parser (Token array → Program AST)
///   - codegen → Codegen (AST → Polars flow mapping strings)
///   - checker → static semantic analyzer (Type Checker)
///   - policy  → Policy-as-Code static security guardrail (issue #2)
///   - emitter → emit_rust (AST → standalone Rust source file generation)
///
/// ⚠️  The runtime execution engine (run_pipeline / Polars LazyFrame) has been
///      separated into the xazz-exec crate.
///      This is an architectural isolation to remove the Polars dependency from the
///      CLI binary.
pub mod ast; // re-exports xazz-core::ast
pub mod checker;
pub mod codegen;
pub mod emitter;
pub mod error; // re-exports xazz-core::error
pub mod ir; // re-exports xazz-core::ir
pub mod lexer;
pub mod opt;
pub mod parser;
pub mod polars_text;
pub mod policy;
pub mod token; // re-exports xazz-core::token

// ── token re-exports ─────────────────────────────────────────────────────────
pub use token::{Span, Token, TokenKind};

// ── i18n re-exports ──────────────────────────────────────────────────────────
pub use xazz_core::i18n::{is_korean, tr};

// ── ast re-exports ───────────────────────────────────────────────────────────
pub use ast::{
    BinOpKind, ChartConfig, ChartType, Expr, FillNullValue, PipelineOp, PipelineSource, Program,
    Stmt, StructField,
};

// ── error re-exports ─────────────────────────────────────────────────────────
pub use error::{CompileError, CompileResult, ErrorKind};

// ── core component re-exports ────────────────────────────────────────────────
pub use checker::{
    CheckResult, CheckerColType, analyze_program, check_program, check_source, compile_ir,
};
pub use codegen::Codegen;
pub use lexer::Lexer;
pub use opt::optimize_program;
pub use parser::Parser;
pub use policy::{
    ActivePolicy, Policy, PolicyError, PolicyReport, Remediation, Severity, Violation,
    analyze as check_policy, analyze_parsed as check_policy_parsed, load_active_policy,
    policy_load_failure_report, remediate,
};
