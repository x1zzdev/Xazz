// xazz-compiler/src/ast.rs — xazz-core::ast 완전 재노출
//
// AST 정의는 xazz-core 크레이트로 이동했습니다.
// 이 파일은 하위 호환성을 위해 모든 공개 타입을 재노출합니다.
// 내부 모듈(lexer, parser, codegen, emitter)의 `crate::ast::*` import는
// 이 파일을 통해 그대로 동작합니다.
pub use xazz_core::ast::*;
