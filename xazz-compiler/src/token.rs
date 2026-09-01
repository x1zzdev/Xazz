// xazz-compiler/src/token.rs — full re-export of xazz-core::token
//
// The Token definitions moved to the xazz-core crate.
// Internal modules (lexer, parser) keep working via the
// `crate::token::*` import through this file.
pub use xazz_core::token::*;
