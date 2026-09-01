// xazz-compiler/src/ir.rs — full re-export of xazz-core::ir
//
// The Typed IR definitions live in the xazz-core crate.
// Internal modules (checker) keep working via the `crate::ir::*` import
// through this file.
pub use xazz_core::ir::*;
