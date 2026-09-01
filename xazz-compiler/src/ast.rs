// xazz-compiler/src/ast.rs — full re-export of xazz-core::ast
//
// The AST definitions moved to the xazz-core crate.
// This file re-exports every public type for backward compatibility.
// Internal modules (lexer, parser, codegen, emitter) keep working via the
// `crate::ast::*` import through this file.
pub use xazz_core::ast::*;
