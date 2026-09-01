// xazz-compiler/src/error.rs — full re-export of xazz-core::error
//
// The error type definitions moved to the xazz-core crate.
// Internal modules (lexer, parser) keep working via the
// `crate::error::*` import through this file.
pub use xazz_core::error::*;
